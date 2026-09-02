//! Python and Subliminal installation and management utilities
//!
//! This module handles Python installation, Subliminal setup, and environment
//! configuration for the subtitle downloading functionality.

use log::{error, info, warn};
use std::env;
#[cfg(windows)]
use std::io::Write;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

// Use the logging macros directly from the crate root
use crate::debug;

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

// Unix-specific imports (Linux and macOS)
#[cfg(any(target_os = "linux", target_os = "macos"))]
use dirs;

/// Python and Subliminal installation and management utilities
pub struct PythonManager;

const SUBLIMINAL_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
const SUBLIMINAL_MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const OUTPUT_CHANNEL_CAPACITY: usize = 128;
const MAX_OUTPUT_CHUNKS_PER_POLL: usize = 64;
const MAX_CAPTURED_OUTPUT_BYTES: usize = 1024 * 1024;
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
        mut self,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        while !(self.stdout_thread.is_finished() && self.stderr_thread.is_finished()) {
            self.process(on_output);
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

    /// Check if Python is installed and return its version
    pub fn get_version() -> Option<String> {
        Self::get_python_info().map(|(_, version)| version)
    }

    /// Return the command and version of the first valid Python 3 installation.
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

    /// Return the installed Subliminal version string, if available
    pub fn get_subliminal_version() -> Option<String> {
        Self::check_subliminal().1
    }

    /// Check if Subliminal is installed
    pub fn is_subliminal_installed() -> bool {
        Self::check_subliminal().0
    }

    /// Check if FFmpeg is installed and available on PATH.
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

    /// Check if Homebrew is installed on macOS.
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

    /// Combined check: returns (installed, version) in a single pass to avoid
    /// redundant subprocess spawns. Each probe that proves subliminal exists
    /// also extracts the version string when possible.
    pub fn check_subliminal() -> (bool, Option<String>) {
        Self::check_subliminal_with_python(None)
    }

    /// Check Subliminal while preferring an already resolved Python command.
    pub fn check_subliminal_with_python(preferred_python: Option<&str>) -> (bool, Option<String>) {
        let empty_env = std::collections::HashMap::new();

        // 1) Direct command -- fastest path
        if let Ok(output) = Self::run_command_hidden("subliminal", &["--version"], &empty_env) {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!(
                "subliminal --version stdout: {} | stderr: {}",
                stdout, stderr
            );
            if output.status.success() {
                let text = if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                };
                if text.to_lowercase().contains("subliminal") {
                    debug!("Subliminal found as direct command");
                    return (true, Some(text));
                }
            }
        }

        // 2) pipx list (Linux/macOS)
        if let Ok(output) = Self::run_command_hidden("pipx", &["list"], &empty_env) {
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("pipx list output: {}", stdout);
            if output.status.success() && stdout.to_lowercase().contains("subliminal") {
                debug!("Subliminal found via pipx list");
                // pipx list doesn't give a clean version; try to extract it
                let version = stdout
                    .lines()
                    .find(|l| l.to_lowercase().contains("subliminal"))
                    .map(|l| l.trim().to_string());
                return (true, version);
            }
        }

        // 3) pip show -- gives both installed status and version
        for cmd in Self::python_probe_commands(preferred_python) {
            if let Ok(output) =
                Self::run_command_hidden(cmd, &["-m", "pip", "show", "subliminal"], &empty_env)
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                debug!("{} -m pip show subliminal output: {}", cmd, stdout);
                if output.status.success() && stdout.contains("Name: subliminal") {
                    debug!("Subliminal found via pip show using {}", cmd);
                    let version = stdout
                        .lines()
                        .find_map(|line| line.strip_prefix("Version: "))
                        .map(|ver| format!("subliminal {}", ver.trim()));
                    return (true, version);
                }
            }
        }

        // 4) Direct import -- last resort, no version info
        for cmd in Self::python_probe_commands(preferred_python) {
            if let Ok(output) = Self::run_command_hidden(
                cmd,
                &["-c", "import subliminal; print('subliminal available')"],
                &empty_env,
            ) {
                let stdout = String::from_utf8_lossy(&output.stdout);
                debug!("{} -c import subliminal output: {}", cmd, stdout);
                if output.status.success() && stdout.contains("subliminal available") {
                    debug!("Subliminal found via direct import using {}", cmd);
                    return (true, None);
                }
            }
        }

        debug!("Subliminal not found");
        (false, None)
    }

    /// Install Subliminal via pipx on Unix or pip on Windows.
    pub fn install_subliminal() -> bool {
        #[cfg(windows)]
        {
            info!("Installing Subliminal via pip on Windows");
            for cmd in &["python", "py", "python3"] {
                if let Ok(output) = Self::run_command_hidden(
                    cmd,
                    &["-m", "pip", "install", "subliminal"],
                    &std::collections::HashMap::new(),
                ) {
                    if output.status.success() {
                        info!("Subliminal installed successfully using {}", cmd);
                        return true;
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("Failed to install Subliminal using {}: {}", cmd, stderr);
                    }
                }
            }
            error!("Failed to install Subliminal with all Python commands");
            false
        }

        #[cfg(target_os = "macos")]
        {
            info!("Installing Subliminal via pipx on macOS");
            if let Ok(output) = Self::run_command_hidden(
                "pipx",
                &["install", "subliminal"],
                &std::collections::HashMap::new(),
            ) {
                if output.status.success() {
                    info!("Subliminal installed successfully using pipx");
                    return true;
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to install Subliminal using pipx: {}", stderr);
            }
            error!("Failed to install Subliminal with pipx on macOS");
            false
        }

        #[cfg(target_os = "linux")]
        {
            info!("Installing Subliminal via pipx on Linux");

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
                        if let Ok(output) = Self::run_command_hidden(
                            cmd,
                            &args_refs,
                            &std::collections::HashMap::new(),
                        ) {
                            if output.status.success() {
                                info!("pipx installed successfully using {}", cmd);
                                break;
                            }
                        }
                    }
                }
            }

            if let Ok(output) = Self::run_command_hidden(
                "pipx",
                &["install", "subliminal"],
                &std::collections::HashMap::new(),
            ) {
                if output.status.success() {
                    info!("Subliminal installed successfully using pipx");
                    return true;
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Failed to install Subliminal using pipx: {}", stderr);
                }
            }

            info!("pipx installation failed, trying pip install as fallback");
            for cmd in &["python3", "python"] {
                if let Ok(output) = Self::run_command_hidden(
                    cmd,
                    &["-m", "pip", "install", "--user", "subliminal"],
                    &std::collections::HashMap::new(),
                ) {
                    if output.status.success() {
                        info!("Subliminal installed successfully using {} pip", cmd);
                        return true;
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("Failed to install Subliminal using {} pip: {}", cmd, stderr);
                    }
                }
            }

            error!("Failed to install Subliminal with pipx and pip fallback");
            false
        }
    }

    /// Add Python Scripts directories to the user PATH (both the system
    /// install Scripts folder next to python.exe, and the per-user Scripts
    /// folder from `sysconfig`).
    pub fn add_scripts_to_path() -> Result<(), String> {
        #[cfg(windows)]
        {
            let mut scripts_dirs: Vec<String> = Vec::new();

            // 1) System/install Scripts: directory containing python.exe + \Scripts
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

            // 2) Per-user Scripts: sysconfig gives the exact versioned path
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
            let mut paths_to_add = Vec::new();

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

            let current_path = env::var("PATH").unwrap_or_default();
            for path in paths_to_add {
                if !current_path.contains(&path) {
                    let new_path = format!("{}:{}", path, current_path);
                    env::set_var("PATH", new_path);
                }
            }

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

    /// Refresh environment variables to pick up PATH changes
    pub fn refresh_environment() -> Result<(), String> {
        #[cfg(windows)]
        {
            // Get the updated PATH from registry
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let env = hkcu
                .open_subkey_with_flags("Environment", KEY_READ)
                .map_err(|e| format!("Failed to open registry: {}", e))?;

            let user_path: String = env.get_value("Path").unwrap_or_else(|_| "".into());

            // Get system PATH
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            let sys_env = hklm
                .open_subkey_with_flags(
                    "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                    KEY_READ,
                )
                .map_err(|e| format!("Failed to open system registry: {}", e))?;

            let system_path: String = sys_env.get_value("Path").unwrap_or_else(|_| "".into());

            // Combine system and user paths
            let combined_path = if system_path.trim().is_empty() {
                user_path
            } else if user_path.trim().is_empty() {
                system_path
            } else {
                format!("{system_path};{user_path}")
            };

            // Update current process environment
            std::env::set_var("PATH", combined_path);

            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            let home_dir =
                dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
            let mut paths_to_add = Vec::new();

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
    /// Query the endoflife.date API for the latest stable Python 3.x version
    /// and return the download URL for the amd64 Windows installer.
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
    /// Download Python installer from official website
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
    /// Install Python silently with required options
    pub fn install_silent(_installer_path: &PathBuf) -> io::Result<bool> {
        let mut command = Command::new(_installer_path);
        command.args([
            "/quiet",
            "InstallAllUsers=1",
            "PrependPath=1",
            "Include_pip=1",
        ]);

        // On Windows, try to hide the console window
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let status = command.status()?;
        Ok(status.success())
    }

    /// Ensure the Subliminal cache directory exists.
    pub fn ensure_cache_dir() -> io::Result<PathBuf> {
        let cache_dir = env::temp_dir().join("subliminal_cache");

        // Create the directory if it doesn't exist
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)?;
        }

        Ok(cache_dir)
    }

    /// Clean up corrupted cache files (call this when DBM errors persist)
    pub fn cleanup_cache() -> io::Result<()> {
        let cache_dir = env::temp_dir().join("subliminal_cache");
        if cache_dir.exists() {
            // Remove all cache files to force a fresh start
            let cache_files = ["cache.dbm", "cache.dir", "cache.pag", "cache.db", "cache"];
            for file_name in &cache_files {
                let cache_file = cache_dir.join(file_name);
                if cache_file.exists() {
                    let _ = std::fs::remove_file(&cache_file);
                }
            }
            // Also try to remove the directory and recreate it
            let _ = std::fs::remove_dir_all(&cache_dir);
            std::fs::create_dir_all(&cache_dir)?;
        }
        Ok(())
    }

    /// Run Subliminal using available command forms, streaming bounded output while
    /// enforcing the configured timeout.
    pub fn run_subliminal(
        args: &[String],
        env_vars: &std::collections::HashMap<String, String>,
        cancel_flag: &std::sync::atomic::AtomicBool,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
        preferred_python: Option<&str>,
    ) -> io::Result<std::process::Output> {
        let mut attempts = vec![("subliminal".to_string(), Vec::new())];
        attempts.extend(
            Self::python_probe_commands(preferred_python)
                .into_iter()
                .map(|command| {
                    (
                        command.to_string(),
                        vec!["-m".to_string(), "subliminal".to_string()],
                    )
                }),
        );
        let mut last_output = None;
        let mut last_error = None;

        for (command, prefix) in attempts {
            let mut command_args = prefix;
            command_args.extend_from_slice(args);
            let command_arg_refs: Vec<&str> = command_args.iter().map(String::as_str).collect();
            match Self::run_subliminal_command(
                &command,
                &command_arg_refs,
                env_vars,
                cancel_flag,
                on_output,
            ) {
                Ok(output) if output.status.success() => return Ok(output),
                Ok(output) if Self::is_interpreter_launch_failure(&output) => {
                    debug!(
                        "{} could not run Subliminal: trying the next interpreter",
                        command
                    );
                    last_output = Some(output);
                }
                Ok(output) => return Ok(output),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Err(error);
                }
                Err(error) => {
                    debug!("{} could not be started: {}", command, error);
                    last_error = Some(error);
                }
            }
        }

        if let Some(output) = last_output {
            Ok(output)
        } else {
            Err(last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "No Subliminal command could be started",
                )
            }))
        }
    }

    fn is_interpreter_launch_failure(output: &std::process::Output) -> bool {
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_lowercase();
        text.contains("no module named subliminal")
            || text.contains("module not found")
            || text.contains("can't open file")
            || text.contains("cannot open file")
    }

    /// Spawn one Subliminal command, forward stdout/stderr chunks while running,
    /// retain bounded output, and enforce both inactivity and absolute limits.
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
        let mut child = Self::hidden_command(cmd, args, env_vars).spawn()?;
        let stdout = child
            .stdout
            .take()
            .expect("Subliminal stdout must be piped");
        let stderr = child
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

            let status = match child.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    if let Err(cleanup_error) =
                        Self::terminate_subliminal_command(&mut child, captured, on_output)
                    {
                        warn!("Failed to clean up Subliminal command: {}", cleanup_error);
                    }
                    return Err(error);
                }
            };
            if let Some(status) = status {
                Self::stop_subliminal_process(&mut child);
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
                if let Err(cleanup_error) =
                    Self::terminate_subliminal_command(&mut child, captured, on_output)
                {
                    warn!("Failed to clean up Subliminal command: {}", cleanup_error);
                }
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
                return Err(io::Error::new(
                    error_kind,
                    format!(
                        "{} (elapsed: {} seconds; last output: {} seconds ago)",
                        error_message,
                        elapsed.as_secs(),
                        since_output.as_secs()
                    ),
                ));
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    fn terminate_subliminal_command(
        child: &mut std::process::Child,
        captured: SubliminalOutput,
        on_output: &mut dyn FnMut(&str, &[u8], std::time::Duration),
    ) -> io::Result<()> {
        Self::stop_subliminal_process(child);
        let wait_result = child.wait().map(|_| ());
        let output_result = captured.collect(on_output).map(|_| ());
        wait_result.and(output_result)
    }

    fn stop_subliminal_process(child: &mut std::process::Child) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            let pid = child.id().to_string();
            let taskkill_status = Command::new("taskkill")
                .args(["/PID", &pid, "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .status();
            if let Err(error) = taskkill_status {
                warn!("Failed to terminate Subliminal process tree: {}", error);
            }
        }

        #[cfg(unix)]
        {
            let process_group = format!("-{}", child.id());
            match Command::new("kill")
                .args(["-KILL", "--", &process_group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) if !status.success() => {
                    debug!("Subliminal process group was already stopped: {}", status);
                }
                Err(error) => warn!("Failed to terminate Subliminal process group: {}", error),
                _ => {}
            }
        }

        if let Err(error) = child.kill() {
            debug!("Subliminal process was already stopped: {}", error);
        }
    }

    fn hidden_command(
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
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
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

    /// Run a command with hidden console window
    pub fn run_command_hidden(
        cmd: &str,
        args: &[&str],
        env_vars: &std::collections::HashMap<String, String>,
    ) -> io::Result<std::process::Output> {
        Self::hidden_command(cmd, args, env_vars).output()
    }

    /// Check if pipx is available
    pub fn _pipx_available() -> bool {
        if let Ok(output) =
            Self::run_command_hidden("pipx", &["--version"], &std::collections::HashMap::new())
        {
            return output.status.success();
        }
        false
    }

    /// Return the first supported Linux package manager available on PATH.
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

    /// Get pipx version string (e.g. "1.2.3"). Linux only.
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
        // pipx --version may output "1.2.3" or "pipx 1.2.3" or "pipx version 1.2.3"
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

    /// Try to install pipx using common methods
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
}
