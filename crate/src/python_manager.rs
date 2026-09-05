//! Python and Subliminal installation utilities.

use log::{error, info, warn};
use std::env;
#[cfg(windows)]
use std::io::Write;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

// Logging macros
use crate::debug;

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

// Unix imports
#[cfg(any(target_os = "linux", target_os = "macos"))]
use dirs;

/// Python and Subliminal utilities.
pub struct PythonManager;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubliminalCommand {
    pub program: PathBuf,
    pub prefix: Vec<String>,
}

const SUBLIMINAL_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
const SUBLIMINAL_MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const OUTPUT_CHANNEL_CAPACITY: usize = 128;
const MAX_OUTPUT_CHUNKS_PER_POLL: usize = 64;
const MAX_CAPTURED_OUTPUT_BYTES: usize = 1024 * 1024;
const DEPENDENCY_PROBE_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const DEPENDENCY_PROBE_MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const DEPENDENCY_INSTALL_MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const FFPROBE_MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const PROCESS_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(target_os = "linux")]
static LINUX_PACKAGE_MANAGER: once_cell::sync::OnceCell<&'static str> =
    once_cell::sync::OnceCell::new();
#[cfg(windows)]
const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(windows)]
const HTTP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Clone, Copy)]
enum SubliminalStream {
    Stdout,
    Stderr,
}

impl SubliminalStream {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

fn read_subliminal_output(
    mut reader: impl Read,
    sender: mpsc::SyncSender<(SubliminalStream, Vec<u8>)>,
    stream: SubliminalStream,
) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            return Ok(());
        }
        if sender
            .send((stream, buffer[..bytes_read].to_vec()))
            .is_err()
        {
            return Ok(());
        }
    }
}

fn append_capped(target: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= MAX_CAPTURED_OUTPUT_BYTES {
        target.clear();
        target.extend_from_slice(&bytes[bytes.len() - MAX_CAPTURED_OUTPUT_BYTES..]);
        return;
    }

    let overflow = target
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(MAX_CAPTURED_OUTPUT_BYTES);
    if overflow > 0 {
        target.drain(..overflow);
    }
    target.extend_from_slice(bytes);
}

fn process_subliminal_output(
    receiver: &mpsc::Receiver<(SubliminalStream, Vec<u8>)>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    started: std::time::Instant,
    on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
) -> bool {
    let mut output_received = false;
    for _ in 0..MAX_OUTPUT_CHUNKS_PER_POLL {
        let Ok((stream, bytes)) = receiver.try_recv() else {
            break;
        };
        output_received = true;
        match stream {
            SubliminalStream::Stdout => append_capped(stdout, &bytes),
            SubliminalStream::Stderr => append_capped(stderr, &bytes),
        }
        on_output(stream.as_str(), &bytes, started.elapsed());
    }
    output_received
}

struct SubliminalOutput {
    stdout_thread: std::thread::JoinHandle<io::Result<()>>,
    stderr_thread: std::thread::JoinHandle<io::Result<()>>,
    output_tx: mpsc::SyncSender<(SubliminalStream, Vec<u8>)>,
    output_rx: mpsc::Receiver<(SubliminalStream, Vec<u8>)>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    started: std::time::Instant,
}

impl SubliminalOutput {
    fn process(&mut self, on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration)) -> bool {
        process_subliminal_output(
            &self.output_rx,
            &mut self.stdout,
            &mut self.stderr,
            self.started,
            on_output,
        )
    }

    fn collect(
        self,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        self.collect_until(on_output, PROCESS_CLEANUP_TIMEOUT)
    }

    fn collect_until(
        mut self,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
        timeout: std::time::Duration,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        let deadline = std::time::Instant::now() + timeout;
        while !(self.stdout_thread.is_finished() && self.stderr_thread.is_finished()) {
            self.process(on_output);
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::ResourceBusy,
                    "Subliminal output readers did not exit after process cleanup",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        while self.process(on_output) {}

        let SubliminalOutput {
            stdout_thread,
            stderr_thread,
            output_tx,
            output_rx,
            stdout,
            stderr,
            started,
        } = self;
        drop(output_tx);
        let stdout_result = stdout_thread
            .join()
            .map_err(|_| io::Error::other("Subliminal stdout reader panicked"))
            .and_then(|result| result);
        let stderr_result = stderr_thread
            .join()
            .map_err(|_| io::Error::other("Subliminal stderr reader panicked"))
            .and_then(|result| result);
        let mut stdout = stdout;
        let mut stderr = stderr;
        while process_subliminal_output(&output_rx, &mut stdout, &mut stderr, started, on_output) {}
        stdout_result.and(stderr_result).map(|_| (stdout, stderr))
    }

    fn readers_finished(&self) -> bool {
        self.stdout_thread.is_finished() && self.stderr_thread.is_finished()
    }
}

#[cfg(windows)]
struct ProcessJob {
    handle: HANDLE,
}

#[cfg(windows)]
impl ProcessJob {
    fn attach(child: &std::process::Child) -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }
            .map_err(io::Error::other)?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(error) = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        } {
            let _ = unsafe { CloseHandle(handle) };
            return Err(io::Error::other(error));
        }
        if let Err(error) =
            unsafe { AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle() as _)) }
        {
            let _ = unsafe { CloseHandle(handle) };
            return Err(io::Error::other(error));
        }
        if let Err(error) = Self::resume_child(child.id()) {
            let _ = unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn resume_child(pid: u32) -> io::Result<()> {
        let snapshot =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(io::Error::other)?;
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe { Thread32First(snapshot, &mut entry) } {
            let _ = unsafe { CloseHandle(snapshot) };
            return Err(io::Error::other(error));
        }
        loop {
            if entry.th32OwnerProcessID == pid {
                let thread =
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                        .map_err(io::Error::other)?;
                let resume_result = unsafe { ResumeThread(thread) };
                let close_result = unsafe { CloseHandle(thread) };
                let _ = unsafe { CloseHandle(snapshot) };
                close_result.map_err(io::Error::other)?;
                if resume_result == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            if let Err(error) = unsafe { Thread32Next(snapshot, &mut entry) } {
                let _ = unsafe { CloseHandle(snapshot) };
                return Err(io::Error::other(error));
            }
        }
    }

    fn terminate(&self) -> io::Result<()> {
        unsafe { TerminateJobObject(self.handle, 1) }.map_err(io::Error::other)
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

struct OwnedChild {
    child: std::process::Child,
    #[cfg(windows)]
    job: ProcessJob,
}

impl OwnedChild {
    fn spawn(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
    ) -> io::Result<Self> {
        let child = Self::spawn_command(cmd, args, env_vars).spawn()?;
        #[cfg(windows)]
        let mut child = child;
        #[cfg(windows)]
        let job = match ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
        })
    }

    fn spawn_command(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
    ) -> Command {
        let mut command = Command::new(cmd);
        command.envs(env_vars);
        command.args(args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.env("PYTHONUNBUFFERED", "1");

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows::Win32::System::Threading::CREATE_SUSPENDED;
            command.creation_flags(0x08000000 | CREATE_SUSPENDED.0); // Hide the console and assign before execution.
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);

            #[cfg(target_os = "linux")]
            command.env("DEBIAN_FRONTEND", "noninteractive");
        }

        command
    }

    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    fn stop(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        self.job.terminate()?;

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let process_group = format!("-{}", self.child.id());
            let status = Command::new("kill")
                .args(["-KILL", "--", &process_group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() && self.child.try_wait()?.is_none() {
                return Err(io::Error::other(format!(
                    "Failed to terminate Subliminal process group (exit {})",
                    status.code().unwrap_or(-1)
                )));
            }
        }

        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn wait(&mut self) -> io::Result<()> {
        let deadline = std::time::Instant::now() + PROCESS_CLEANUP_TIMEOUT;
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::ResourceBusy,
                    "Subliminal process did not exit after termination",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl PythonManager {
    fn python_commands() -> &'static [&'static str] {
        #[cfg(target_os = "macos")]
        {
            &[
                "/opt/homebrew/bin/python3",
                "/usr/local/bin/python3",
                "python3",
                "python",
                "py",
            ]
        }
        #[cfg(target_os = "linux")]
        {
            &["python3", "python", "py"]
        }
        #[cfg(windows)]
        {
            &["python", "py", "python3"]
        }
    }

    fn python_probe_commands(preferred: Option<&str>) -> Vec<&str> {
        let mut commands = Vec::new();
        if let Some(preferred) = preferred {
            commands.push(preferred);
        }
        commands.extend(
            Self::python_commands()
                .iter()
                .copied()
                .filter(|command| Some(*command) != preferred),
        );
        commands
    }

    /// Get the installed Python version.
    pub fn get_version() -> Option<String> {
        Self::get_python_info().map(|(_, version)| version)
    }

    /// Get the first valid Python 3 command and version.
    pub fn get_python_info() -> Option<(String, String)> {
        for cmd in Self::python_commands() {
            if let Ok(output) =
                Self::run_command_hidden(cmd, &["--version"], &std::collections::HashMap::new())
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let version = if !stdout.is_empty() { stdout } else { stderr };
                    debug!("Python version output for {}: {}", cmd, version);
                    if version.starts_with("Python 3.") {
                        debug!(
                            "Found valid Python 3 version: {} using command: {}",
                            version, cmd
                        );
                        return Some(((*cmd).to_string(), version));
                    }
                }
            }
        }
        debug!("No valid Python 3 installation found");
        None
    }

    /// Get the installed Subliminal version.
    pub fn get_subliminal_version() -> Option<String> {
        Self::check_subliminal().1
    }

    /// Check whether Subliminal is installed.
    pub fn is_subliminal_installed() -> bool {
        Self::check_subliminal().0
    }

    /// Check whether FFmpeg is on PATH.
    pub fn is_ffmpeg_installed() -> bool {
        #[cfg(target_os = "macos")]
        let commands = [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "ffmpeg",
        ];
        #[cfg(not(target_os = "macos"))]
        let commands = ["ffmpeg"];

        commands.iter().any(|command| {
            Self::run_command_hidden(command, &["-version"], &std::collections::HashMap::new())
                .map(|output| output.status.success())
                .unwrap_or(false)
        })
    }

    /// Check whether Homebrew is installed on macOS.
    pub fn is_homebrew_installed() -> bool {
        #[cfg(target_os = "macos")]
        let commands = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew", "brew"];
        #[cfg(not(target_os = "macos"))]
        let commands: [&str; 0] = [];

        commands.iter().any(|command| {
            Self::run_command_hidden(command, &["--version"], &std::collections::HashMap::new())
                .map(|output| output.status.success())
                .unwrap_or(false)
        })
    }

    /// Check Subliminal and return its version when available.
    pub fn check_subliminal() -> (bool, Option<String>) {
        Self::check_subliminal_with_python(None)
    }

    /// Check Subliminal, preferring a resolved Python command.
    pub fn check_subliminal_with_python(preferred_python: Option<&str>) -> (bool, Option<String>) {
        let (command, version) = Self::resolve_subliminal(preferred_python);
        (command.is_some(), version)
    }

    pub fn supported_subliminal_version(text: &str) -> bool {
        if !text.to_ascii_lowercase().contains("subliminal") {
            return false;
        }
        let Some(version) = text.split_whitespace().last() else {
            return false;
        };
        let parts: Option<Vec<u32>> = version.split('.').map(|part| part.parse().ok()).collect();
        parts.is_some_and(|parts| parts.len() == 3 && parts.as_slice() >= [2, 4, 0].as_slice())
    }

    fn resolve_program(command: &str) -> Option<PathBuf> {
        Self::resolve_program_in_path(command, &env::var_os("PATH").unwrap_or_default())
    }

    fn resolve_program_in_path(command: &str, search_path: &std::ffi::OsStr) -> Option<PathBuf> {
        let path = std::path::Path::new(command);
        let candidates = if path.components().count() > 1 {
            vec![path.to_path_buf()]
        } else {
            env::split_paths(search_path)
                .map(|folder| folder.join(command))
                .collect()
        };
        candidates.into_iter().find_map(|path| {
            #[cfg(windows)]
            let path = if path.extension().is_none() {
                path.with_extension("exe")
            } else {
                path
            };
            path.is_file().then_some(path)
        })
    }

    pub fn resolve_subliminal(
        preferred_python: Option<&str>,
    ) -> (Option<SubliminalCommand>, Option<String>) {
        let mut attempts = vec![("subliminal", Vec::new())];
        attempts.extend(
            Self::python_probe_commands(preferred_python)
                .into_iter()
                .map(|program| (program, vec!["-m".to_string(), "subliminal".to_string()])),
        );
        let mut unsupported = None;
        for (program, prefix) in attempts {
            let Some(program) = Self::resolve_program(program) else {
                continue;
            };
            let Some(program_arg) = program.to_str() else {
                continue;
            };
            let mut args: Vec<_> = prefix.iter().map(String::as_str).collect();
            args.push("--version");
            let Ok(output) =
                Self::run_command_hidden(program_arg, &args, &std::collections::HashMap::new())
            else {
                continue;
            };
            if !output.status.success() {
                continue;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let text = if stdout.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            if Self::supported_subliminal_version(text) {
                return (
                    Some(SubliminalCommand { program, prefix }),
                    Some(text.to_string()),
                );
            }
            if text.to_ascii_lowercase().contains("subliminal") {
                unsupported = Some(text.to_string());
            }
        }
        (None, unsupported)
    }

    /// Install Subliminal.
    pub fn install_subliminal() -> Result<(), String> {
        #[cfg(windows)]
        {
            info!("Installing Subliminal via pip on Windows");
            let mut last_error = None;
            for cmd in &["python", "py", "python3"] {
                match Self::run_install_command_hidden(
                    cmd,
                    &["-m", "pip", "install", "--upgrade", "subliminal>=2.4.0"],
                    &std::collections::HashMap::new(),
                ) {
                    Ok(output) if output.status.success() => {
                        info!("Subliminal installed successfully using {}", cmd);
                        return Ok(());
                    }
                    Ok(output) => {
                        let error = format!(
                            "{} exited with {}: {}",
                            cmd,
                            output.status,
                            String::from_utf8_lossy(&output.stderr).trim()
                        );
                        warn!("Failed to install Subliminal: {}", error);
                        last_error = Some(error);
                    }
                    Err(error) => {
                        let error = format!("{}: {}", cmd, error);
                        warn!("Failed to install Subliminal: {}", error);
                        last_error = Some(error);
                    }
                }
            }
            let error = last_error.unwrap_or_else(|| "No Python command was available".to_string());
            error!(
                "Failed to install Subliminal with all Python commands: {}",
                error
            );
            Err(error)
        }

        #[cfg(target_os = "macos")]
        {
            info!("Installing Subliminal via pipx on macOS");
            match Self::run_install_command_hidden(
                "pipx",
                &["install", "--force", "subliminal>=2.4.0"],
                &std::collections::HashMap::new(),
            ) {
                Ok(output) if output.status.success() => {
                    info!("Subliminal installed successfully using pipx");
                    Ok(())
                }
                Ok(output) => {
                    let error = format!(
                        "pipx exited with {}: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                    error!("Failed to install Subliminal with pipx on macOS: {}", error);
                    Err(error)
                }
                Err(error) => {
                    error!("Failed to install Subliminal with pipx on macOS: {}", error);
                    Err(error.to_string())
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            info!("Installing Subliminal via pipx on Linux");
            let mut last_error = None;

            if let Ok(output) =
                Self::run_command_hidden("pipx", &["--version"], &std::collections::HashMap::new())
            {
                if !output.status.success() {
                    info!("pipx not found, attempting to install pipx first");
                    let pipx_install_attempts = [
                        ("python3", vec!["-m", "pip", "install", "--user", "pipx"]),
                        ("python", vec!["-m", "pip", "install", "--user", "pipx"]),
                        ("apt", vec!["install", "-y", "pipx"]),
                        ("dnf", vec!["install", "-y", "python3-pipx"]),
                        ("pacman", vec!["-S", "--noconfirm", "python-pipx"]),
                    ];

                    for (cmd, args) in &pipx_install_attempts {
                        let args_refs: Vec<&str> = args.iter().map(|s| &**s).collect();
                        match Self::run_install_command_hidden(
                            cmd,
                            &args_refs,
                            &std::collections::HashMap::new(),
                        ) {
                            Ok(output) if output.status.success() => {
                                info!("pipx installed successfully using {}", cmd);
                                break;
                            }
                            Ok(output) => {
                                last_error = Some(format!(
                                    "{} exited with {}: {}",
                                    cmd,
                                    output.status,
                                    String::from_utf8_lossy(&output.stderr).trim()
                                ));
                            }
                            Err(error) => last_error = Some(format!("{}: {}", cmd, error)),
                        }
                    }
                }
            }

            match Self::run_install_command_hidden(
                "pipx",
                &["install", "--force", "subliminal>=2.4.0"],
                &std::collections::HashMap::new(),
            ) {
                Ok(output) if output.status.success() => {
                    info!("Subliminal installed successfully using pipx");
                    return Ok(());
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Failed to install Subliminal using pipx: {}", stderr);
                    last_error = Some(format!(
                        "pipx exited with {}: {}",
                        output.status,
                        stderr.trim()
                    ));
                }
                Err(error) => last_error = Some(format!("pipx: {}", error)),
            }

            info!("pipx installation failed, trying pip install as fallback");
            for cmd in &["python3", "python"] {
                match Self::run_install_command_hidden(
                    cmd,
                    &[
                        "-m",
                        "pip",
                        "install",
                        "--user",
                        "--upgrade",
                        "subliminal>=2.4.0",
                    ],
                    &std::collections::HashMap::new(),
                ) {
                    Ok(output) if output.status.success() => {
                        info!("Subliminal installed successfully using {} pip", cmd);
                        return Ok(());
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("Failed to install Subliminal using {} pip: {}", cmd, stderr);
                        last_error = Some(format!(
                            "{} pip exited with {}: {}",
                            cmd,
                            output.status,
                            stderr.trim()
                        ));
                    }
                    Err(error) => last_error = Some(format!("{} pip: {}", cmd, error)),
                }
            }

            let error = last_error
                .unwrap_or_else(|| "No pipx or Python install command was available".to_string());
            error!(
                "Failed to install Subliminal with pipx and pip fallback: {}",
                error
            );
            Err(error)
        }
    }

    /// Add Python Scripts directories to PATH.
    pub fn add_scripts_to_path() -> Result<(), String> {
        #[cfg(windows)]
        {
            let mut scripts_dirs: Vec<String> = Vec::new();

            for cmd in &["python", "py"] {
                let output = Self::run_command_hidden(
                    cmd,
                    &[
                        "-c",
                        "import sys, os; print(os.path.dirname(sys.executable))",
                    ],
                    &std::collections::HashMap::new(),
                );
                if let Ok(out) = output {
                    if out.status.success() {
                        let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !dir.is_empty() {
                            scripts_dirs.push(format!("{}\\Scripts", dir));
                            break;
                        }
                    }
                }
            }

            for cmd in &["python", "py"] {
                let output = Self::run_command_hidden(
                    cmd,
                    &[
                        "-c",
                        "import sysconfig; print(sysconfig.get_path('scripts', 'nt_user'))",
                    ],
                    &std::collections::HashMap::new(),
                );
                if let Ok(out) = output {
                    if out.status.success() {
                        let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !dir.is_empty() {
                            scripts_dirs.push(dir);
                            break;
                        }
                    }
                }
            }

            if scripts_dirs.is_empty() {
                return Err("Failed to locate any Python Scripts directory".to_string());
            }

            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let env = hkcu
                .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
                .map_err(|e| format!("Failed to open registry: {}", e))?;

            let mut current_path: String = env.get_value("Path").unwrap_or_else(|_| "".into());
            let mut changed = false;

            for scripts_path in &scripts_dirs {
                if !current_path
                    .to_lowercase()
                    .contains(&scripts_path.to_lowercase())
                {
                    if current_path.trim().is_empty() {
                        current_path = scripts_path.clone();
                    } else {
                        current_path = format!("{current_path};{scripts_path}");
                    }
                    changed = true;
                }
            }

            if changed {
                env.set_value("Path", &current_path)
                    .map_err(|e| format!("Failed to set PATH: {}", e))?;

                unsafe {
                    let param = "Environment\0".encode_utf16().collect::<Vec<u16>>();

                    SendMessageTimeoutW(
                        HWND_BROADCAST,
                        WM_SETTINGCHANGE,
                        WPARAM(0),
                        LPARAM(param.as_ptr() as isize),
                        SMTO_ABORTIFHUNG,
                        5000,
                        Some(null_mut()),
                    );
                }
            }

            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            let home_dir =
                dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
            let mut paths_to_add = vec![home_dir.join(".local/bin").to_string_lossy().into_owned()];

            if std::path::Path::new("/opt/homebrew/bin").exists() {
                paths_to_add.push("/opt/homebrew/bin".to_string());
            }
            if std::path::Path::new("/usr/local/bin").exists() {
                paths_to_add.push("/usr/local/bin".to_string());
            }

            let py_lib = home_dir.join("Library").join("Python");
            if py_lib.exists() {
                if let Ok(entries) = std::fs::read_dir(&py_lib) {
                    for entry in entries.flatten() {
                        let bin_path = entry.path().join("bin");
                        if bin_path.exists() {
                            paths_to_add.push(bin_path.to_string_lossy().to_string());
                        }
                    }
                }
            }

            let mut current_path = env::var("PATH").unwrap_or_default();
            for path in paths_to_add {
                if !current_path.contains(&path) {
                    current_path = format!("{}:{}", path, current_path);
                }
            }
            env::set_var("PATH", current_path);

            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            let home_dir =
                dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
            let local_bin = home_dir.join(".local").join("bin");

            if local_bin.exists() {
                let current_path = env::var("PATH").unwrap_or_default();
                if !current_path.contains(local_bin.to_string_lossy().as_ref()) {
                    let new_path = format!("{}:{}", local_bin.display(), current_path);
                    env::set_var("PATH", new_path);
                }
            }

            Ok(())
        }
    }

    /// Refresh PATH changes.
    pub fn refresh_environment() -> Result<(), String> {
        #[cfg(windows)]
        {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let env = hkcu
                .open_subkey_with_flags("Environment", KEY_READ)
                .map_err(|e| format!("Failed to open registry: {}", e))?;

            let user_path: String = env.get_value("Path").unwrap_or_else(|_| "".into());

            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            let sys_env = hklm
                .open_subkey_with_flags(
                    "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                    KEY_READ,
                )
                .map_err(|e| format!("Failed to open system registry: {}", e))?;

            let system_path: String = sys_env.get_value("Path").unwrap_or_else(|_| "".into());

            let combined_path = if system_path.trim().is_empty() {
                user_path
            } else if user_path.trim().is_empty() {
                system_path
            } else {
                format!("{system_path};{user_path}")
            };

            std::env::set_var("PATH", combined_path);

            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            let home_dir =
                dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
            let mut paths_to_add = vec![home_dir.join(".local/bin").to_string_lossy().into_owned()];

            for path in &["/opt/homebrew/bin", "/usr/local/bin"] {
                if std::path::Path::new(path).exists() {
                    paths_to_add.push(path.to_string());
                }
            }

            let py_lib = home_dir.join("Library").join("Python");
            if py_lib.exists() {
                if let Ok(entries) = std::fs::read_dir(&py_lib) {
                    for entry in entries.flatten() {
                        let bin_path = entry.path().join("bin");
                        if bin_path.exists() {
                            paths_to_add.push(bin_path.to_string_lossy().to_string());
                        }
                    }
                }
            }

            let current_path = env::var("PATH").unwrap_or_default();
            let mut new_path_parts = paths_to_add;
            new_path_parts.push(current_path);
            let new_path = new_path_parts.join(":");
            env::set_var("PATH", new_path);

            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            let home_dir =
                dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
            let local_bin = home_dir.join(".local").join("bin");

            if local_bin.exists() {
                let current_path = env::var("PATH").unwrap_or_default();
                if !current_path.contains(local_bin.to_string_lossy().as_ref()) {
                    let new_path = format!("{}:{}", local_bin.display(), current_path);
                    env::set_var("PATH", new_path);
                }
            }

            Ok(())
        }
    }

    #[cfg(windows)]
    /// Get the latest Python 3 installer URL on Windows.
    pub fn get_latest_python_url() -> io::Result<String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .map_err(|e| io::Error::other(format!("Failed to create HTTP client: {}", e)))?;
        let resp = client
            .get("https://endoflife.date/api/python.json")
            .send()
            .map_err(|e| io::Error::other(format!("Failed to fetch Python version info: {}", e)))?;
        if !resp.status().is_success() {
            return Err(io::Error::other(format!(
                "Python version info request failed with HTTP {}",
                resp.status()
            )));
        }

        let releases: serde_json::Value = resp.json().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse Python version JSON: {}", e),
            )
        })?;

        let version = releases
            .as_array()
            .and_then(|arr| {
                arr.iter().find_map(|entry| {
                    let cycle = entry.get("cycle")?.as_str()?;
                    if cycle.starts_with("3.") {
                        entry.get("latest")?.as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "No stable Python 3.x release found",
                )
            })?;

        info!("Latest Python version from API: {}", version);
        Ok(format!(
            "https://www.python.org/ftp/python/{version}/python-{version}-amd64.exe"
        ))
    }

    #[cfg(windows)]
    /// Download the Python installer.
    pub fn download_installer() -> io::Result<PathBuf> {
        let url = Self::get_latest_python_url()?;
        info!("Downloading Python installer from: {}", url);
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .map_err(io::Error::other)?;
        let response = client.get(&url).send().map_err(io::Error::other)?;
        if !response.status().is_success() {
            return Err(io::Error::other(format!(
                "Python installer request failed with HTTP {}",
                response.status()
            )));
        }

        let temp_dir = env::temp_dir();
        let installer_path = temp_dir.join("python-installer.exe");
        let mut file = File::create(&installer_path)?;
        let bytes = response.bytes().map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        Ok(installer_path)
    }

    #[cfg(windows)]
    /// Install Python silently.
    pub fn install_silent(_installer_path: &PathBuf) -> io::Result<bool> {
        let mut command = Command::new(_installer_path);
        command.args([
            "/quiet",
            "InstallAllUsers=1",
            "PrependPath=1",
            "Include_pip=1",
        ]);

        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // Hide the console.

        let status = command.status()?;
        Ok(status.success())
    }

    /// Ensure the Subliminal cache directory exists.
    pub fn ensure_cache_dir() -> io::Result<tempfile::TempDir> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("rustitles-subliminal-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        builder.tempdir()
    }

    /// Clean up corrupted cache files.
    pub fn cleanup_cache() -> io::Result<()> {
        let cache_dir = env::temp_dir().join("subliminal_cache");
        if cache_dir.exists() {
            let cache_files = ["cache.dbm", "cache.dir", "cache.pag", "cache.db", "cache"];
            for file_name in &cache_files {
                let cache_file = cache_dir.join(file_name);
                if cache_file.exists() {
                    let _ = std::fs::remove_file(&cache_file);
                }
            }
            let _ = std::fs::remove_dir_all(&cache_dir);
            std::fs::create_dir_all(&cache_dir)?;
        }
        Ok(())
    }

    /// Run Subliminal with bounded output and timeouts.
    pub fn run_subliminal(
        args: &[String],
        env_vars: &std::collections::HashMap<String, String>,
        cancel_flag: &std::sync::atomic::AtomicBool,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
        command: &SubliminalCommand,
    ) -> io::Result<std::process::Output> {
        let program = command.program.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Subliminal executable path is not valid UTF-8",
            )
        })?;
        let mut command_args: Vec<_> = command.prefix.iter().map(String::as_str).collect();
        command_args.extend(args.iter().map(String::as_str));
        Self::run_subliminal_command(program, &command_args, env_vars, cancel_flag, on_output)
    }

    /// Run one Subliminal command with output and timeout limits.
    fn run_subliminal_command(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
        cancel_flag: &std::sync::atomic::AtomicBool,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
    ) -> io::Result<std::process::Output> {
        Self::run_subliminal_command_with_timeouts(
            cmd,
            args,
            env_vars,
            cancel_flag,
            on_output,
            SUBLIMINAL_INACTIVITY_TIMEOUT,
            SUBLIMINAL_MAX_TIMEOUT,
        )
    }

    fn run_subliminal_command_with_timeouts(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
        cancel_flag: &std::sync::atomic::AtomicBool,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
        inactivity_timeout: std::time::Duration,
        max_timeout: std::time::Duration,
    ) -> io::Result<std::process::Output> {
        let mut process = OwnedChild::spawn(cmd, args, env_vars)?;
        let stdout = process
            .child
            .stdout
            .take()
            .expect("Subliminal stdout must be piped");
        let stderr = process
            .child
            .stderr
            .take()
            .expect("Subliminal stderr must be piped");
        let (output_tx, output_rx) = mpsc::sync_channel(OUTPUT_CHANNEL_CAPACITY);
        let stdout_thread = {
            let output_tx = output_tx.clone();
            std::thread::spawn(move || {
                read_subliminal_output(stdout, output_tx, SubliminalStream::Stdout)
            })
        };
        let stderr_tx = output_tx.clone();
        let stderr_thread = std::thread::spawn(move || {
            read_subliminal_output(stderr, stderr_tx, SubliminalStream::Stderr)
        });
        let started = std::time::Instant::now();
        let mut last_output = started;
        let mut captured = SubliminalOutput {
            stdout_thread,
            stderr_thread,
            output_tx,
            output_rx,
            stdout: Vec::new(),
            stderr: Vec::new(),
            started,
        };

        loop {
            let output_received = captured.process(on_output);
            if output_received {
                last_output = std::time::Instant::now();
            }

            let status = match process.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    if let Err(cleanup_error) =
                        Self::terminate_subliminal_command(&mut process, captured, on_output)
                    {
                        return Err(io::Error::other(format!(
                            "{}; process cleanup failed: {}",
                            error, cleanup_error
                        )));
                    }
                    return Err(error);
                }
            };
            if let Some(status) = status {
                if !captured.readers_finished() {
                    process.stop()?;
                    process.wait()?;
                }
                let (stdout, stderr) = captured.collect(on_output)?;
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }

            let termination_reason = if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                Some((
                    io::ErrorKind::Interrupted,
                    "Subliminal command cancelled".to_string(),
                ))
            } else if started.elapsed() >= max_timeout {
                Some((
                    io::ErrorKind::TimedOut,
                    format!(
                        "Subliminal command reached its absolute limit of {} seconds",
                        max_timeout.as_secs()
                    ),
                ))
            } else if last_output.elapsed() >= inactivity_timeout {
                Some((
                    io::ErrorKind::TimedOut,
                    format!(
                        "Subliminal command produced no output for {} seconds",
                        inactivity_timeout.as_secs()
                    ),
                ))
            } else {
                None
            };

            if let Some((error_kind, error_message)) = termination_reason {
                let elapsed = started.elapsed();
                let since_output = last_output.elapsed();
                let cleanup_error =
                    Self::terminate_subliminal_command(&mut process, captured, on_output).err();
                if error_kind == io::ErrorKind::Interrupted {
                    info!(
                        "Subliminal command cancelled after {} seconds",
                        elapsed.as_secs()
                    );
                } else if error_message.contains("absolute limit") {
                    warn!(
                        "Subliminal command reached its {}-second absolute limit after {} seconds",
                        max_timeout.as_secs(),
                        elapsed.as_secs()
                    );
                } else {
                    warn!(
                        "Subliminal command produced no output for {} seconds",
                        inactivity_timeout.as_secs()
                    );
                }
                let message = format!(
                    "{} (elapsed: {} seconds; last output: {} seconds ago)",
                    error_message,
                    elapsed.as_secs(),
                    since_output.as_secs()
                );
                return Err(match cleanup_error {
                    Some(cleanup_error) => io::Error::other(format!(
                        "{message}; process cleanup failed: {cleanup_error}"
                    )),
                    None => io::Error::new(error_kind, message),
                });
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    fn terminate_subliminal_command(
        process: &mut OwnedChild,
        captured: SubliminalOutput,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
    ) -> io::Result<()> {
        process.stop()?;
        let wait_result = process.wait();
        let output_result = captured.collect(on_output).map(|_| ());
        wait_result.and(output_result)
    }

    /// Run a command without a console window.
    pub fn run_command_hidden(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
    ) -> io::Result<std::process::Output> {
        Self::run_command_hidden_with_timeouts(
            cmd,
            args,
            env_vars,
            DEPENDENCY_PROBE_INACTIVITY_TIMEOUT,
            DEPENDENCY_PROBE_MAX_TIMEOUT,
        )
    }

    fn run_install_command_hidden(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
    ) -> io::Result<std::process::Output> {
        Self::run_command_hidden_with_timeouts(
            cmd,
            args,
            env_vars,
            DEPENDENCY_INSTALL_MAX_TIMEOUT,
            DEPENDENCY_INSTALL_MAX_TIMEOUT,
        )
    }

    pub fn run_ffprobe(args: &[&str]) -> io::Result<std::process::Output> {
        Self::run_command_hidden_with_timeouts(
            "ffprobe",
            args,
            &std::collections::HashMap::new(),
            FFPROBE_MAX_TIMEOUT,
            FFPROBE_MAX_TIMEOUT,
        )
    }

    fn run_command_hidden_with_timeouts(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
        inactivity_timeout: std::time::Duration,
        max_timeout: std::time::Duration,
    ) -> io::Result<std::process::Output> {
        let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: std::time::Duration| {};
        Self::run_subliminal_command_with_timeouts(
            cmd,
            args,
            env_vars,
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
            inactivity_timeout,
            max_timeout,
        )
    }

    /// Check whether pipx is available.
    pub fn _pipx_available() -> bool {
        if let Ok(output) =
            Self::run_command_hidden("pipx", &["--version"], &std::collections::HashMap::new())
        {
            return output.status.success();
        }
        false
    }

    /// Get the first supported Linux package manager on PATH.
    #[cfg(target_os = "linux")]
    pub fn linux_package_manager() -> &'static str {
        *LINUX_PACKAGE_MANAGER.get_or_init(|| {
            for manager in ["apt", "dnf", "pacman"] {
                if let Ok(output) = Self::run_command_hidden(
                    manager,
                    &["--version"],
                    &std::collections::HashMap::new(),
                ) {
                    if output.status.success() {
                        return manager;
                    }
                }
            }
            "apt"
        })
    }

    /// Get the pipx version on Linux.
    #[cfg(target_os = "linux")]
    pub fn get_pipx_version() -> Option<String> {
        let output =
            Self::run_command_hidden("pipx", &["--version"], &std::collections::HashMap::new())
                .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let s = stdout.trim();
        let ver = s
            .strip_prefix("pipx")
            .map(|t| t.trim().trim_start_matches("version").trim())
            .unwrap_or(s);
        if ver.is_empty()
            || !ver
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        {
            return None;
        }
        Some(ver.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn get_pipx_version() -> Option<String> {
        None
    }

    /// Try common pipx installation methods.
    #[allow(dead_code)]
    pub fn try_install_pipx() -> bool {
        let install_attempts = [
            ("python3", vec!["-m", "pip", "install", "--user", "pipx"]),
            ("python", vec!["-m", "pip", "install", "--user", "pipx"]),
            ("apt", vec!["install", "-y", "pipx"]),
            ("dnf", vec!["install", "-y", "python3-pipx"]),
            ("pacman", vec!["-S", "--noconfirm", "python-pipx"]),
        ];
        for (cmd, args) in &install_attempts {
            let args_refs: Vec<&str> = args.iter().map(|s| &**s).collect();
            if let Ok(output) =
                Self::run_command_hidden(cmd, &args_refs, &std::collections::HashMap::new())
            {
                if output.status.success() {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{append_capped, PythonManager, MAX_CAPTURED_OUTPUT_BYTES};
    use std::time::Duration;

    #[test]
    fn requires_a_recognized_stable_subliminal_version() {
        for version in [
            "subliminal, version 2.4.0",
            "subliminal 2.5.1",
            "subliminal 3.0.0",
        ] {
            assert!(PythonManager::supported_subliminal_version(version));
        }
        for version in [
            "subliminal 2.3.9",
            "subliminal 2.4.0rc1",
            "Python 3.14.0",
            "subliminal available",
        ] {
            assert!(!PythonManager::supported_subliminal_version(version));
        }
    }

    #[test]
    fn concurrent_caches_are_distinct_and_owned() {
        let first = PythonManager::ensure_cache_dir().unwrap();
        let second = PythonManager::ensure_cache_dir().unwrap();
        assert_ne!(first.path(), second.path());
        let old_path = first.path().to_owned();
        drop(first);
        assert!(!old_path.exists());
        assert!(second.path().exists());
    }

    #[test]
    fn resolves_a_pipx_executable_in_a_minimal_search_path() {
        let folder = tempfile::tempdir().unwrap();
        let executable = folder.path().join(if cfg!(windows) {
            "subliminal.exe"
        } else {
            "subliminal"
        });
        std::fs::write(&executable, []).unwrap();
        let search_path = std::env::join_paths([folder.path()]).unwrap();
        assert_eq!(
            PythonManager::resolve_program_in_path("subliminal", &search_path),
            Some(executable)
        );
        assert!(PythonManager::resolve_program_in_path("missing", &search_path).is_none());
    }

    #[test]
    fn resolved_python_command_is_probed_before_fallbacks() {
        let commands = PythonManager::python_probe_commands(Some("resolved-python"));

        assert_eq!(commands.first(), Some(&"resolved-python"));
        assert_eq!(
            commands
                .iter()
                .filter(|command| **command == "resolved-python")
                .count(),
            1
        );
    }

    #[test]
    fn captured_output_keeps_only_the_bounded_tail() {
        let mut captured = Vec::new();
        append_capped(&mut captured, &vec![b'a'; MAX_CAPTURED_OUTPUT_BYTES + 10]);
        append_capped(&mut captured, b"tail");

        assert_eq!(captured.len(), MAX_CAPTURED_OUTPUT_BYTES);
        assert_eq!(&captured[MAX_CAPTURED_OUTPUT_BYTES - 4..], b"tail");
    }

    #[test]
    fn streams_output_before_child_exits() {
        let (command, args) = if cfg!(windows) {
            (
                "cmd",
                vec![
                    "/C",
                    "echo stdout & echo stderr 1>&2 & ping -n 3 127.0.0.1 > NUL",
                ],
            )
        } else {
            (
                "sh",
                vec!["-c", "printf stdout; printf stderr >&2; sleep 2"],
            )
        };
        let mut events = Vec::new();
        let mut on_output = |stream: &str, bytes: &[u8], elapsed: Duration| {
            events.push((stream.to_string(), bytes.to_vec(), elapsed));
        };

        let output = PythonManager::run_subliminal_command(
            command,
            &args,
            &std::collections::HashMap::new(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
        )
        .expect("child command should complete");

        assert!(String::from_utf8_lossy(&output.stdout).contains("stdout"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stderr"));
        assert!(events.iter().any(|(stream, bytes, _)| {
            stream == "stdout" && String::from_utf8_lossy(bytes).contains("stdout")
        }));
        assert!(events.iter().any(|(stream, bytes, _)| {
            stream == "stderr" && String::from_utf8_lossy(bytes).contains("stderr")
        }));
        assert!(events
            .iter()
            .any(|(_, _, elapsed)| *elapsed < Duration::from_secs(1)));
    }

    #[test]
    fn output_resets_inactivity_timeout() {
        let (command, args) = if cfg!(windows) {
            (
                "cmd",
                vec![
                    "/C",
                    "echo first & ping -n 2 127.0.0.1 > NUL & echo second & ping -n 2 127.0.0.1 > NUL",
                ],
            )
        } else {
            (
                "sh",
                vec!["-c", "printf first; sleep 1; printf second; sleep 1"],
            )
        };
        let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: Duration| {};

        let output = PythonManager::run_subliminal_command_with_timeouts(
            command,
            &args,
            &std::collections::HashMap::new(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
            Duration::from_millis(1500),
            Duration::from_secs(5),
        )
        .expect("output should keep the child alive");

        assert!(String::from_utf8_lossy(&output.stdout).contains("first"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("second"));
    }

    #[test]
    fn stops_after_output_inactivity() {
        let (command, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/C", "echo started & ping -n 5 127.0.0.1 > NUL"],
            )
        } else {
            ("sh", vec!["-c", "printf started; sleep 3"])
        };
        let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: Duration| {};

        let error = PythonManager::run_subliminal_command_with_timeouts(
            command,
            &args,
            &std::collections::HashMap::new(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
            Duration::from_millis(500),
            Duration::from_secs(5),
        )
        .expect_err("silent child should hit the inactivity limit");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("no output"));
    }

    #[test]
    fn absolute_timeout_wins_over_continued_output() {
        let (command, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/C", "for /L %i in (1,1,10000000) do @echo tick"],
            )
        } else {
            ("sh", vec!["-c", "while true; do printf tick; done"])
        };
        let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: Duration| {};

        let error = PythonManager::run_subliminal_command_with_timeouts(
            command,
            &args,
            &std::collections::HashMap::new(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
            Duration::from_secs(5),
            Duration::from_millis(700),
        )
        .expect_err("continuously active child should hit the absolute limit");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("absolute limit"));
    }

    #[test]
    fn cancellation_reaps_child_and_readers() {
        let (command, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/C", "echo started & ping -n 20 127.0.0.1 > NUL"],
            )
        } else {
            ("sh", vec!["-c", "printf started; sleep 20"])
        };
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_flag = std::sync::Arc::clone(&cancel_flag);
        let worker = std::thread::spawn(move || {
            let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: Duration| {};
            PythonManager::run_subliminal_command_with_timeouts(
                command,
                &args,
                &std::collections::HashMap::new(),
                &worker_flag,
                &mut on_output,
                Duration::from_secs(30),
                Duration::from_secs(30),
            )
        });

        std::thread::sleep(Duration::from_millis(100));
        cancel_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        let error = worker
            .join()
            .expect("cancellation worker should join")
            .expect_err("cancelled child should return an error");

        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(error.to_string().contains("cancelled"));
    }

    #[test]
    fn parent_exit_does_not_leave_pipe_holding_descendant() {
        let (command, args) = if cfg!(windows) {
            (
                "powershell.exe",
                vec![
                    "-NoProfile",
                    "-Command",
                    "$p=[Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]@{FileName='ping';Arguments='127.0.0.1 -n 20';UseShellExecute=$false}); exit 0",
                ],
            )
        } else {
            ("sh", vec!["-c", "sleep 20 & exit 0"])
        };
        let started = std::time::Instant::now();
        let mut on_output = |_stream: &str, _bytes: &[u8], _elapsed: Duration| {};

        let output = PythonManager::run_subliminal_command_with_timeouts(
            command,
            &args,
            &std::collections::HashMap::new(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut on_output,
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .expect("pipe-holding descendants should be cleaned up");

        assert!(output.status.success());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
