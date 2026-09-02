//! Application logic for the Rustitles subtitle downloader
//!
//! This module contains the main application state and logic.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::data_structures::{
    DownloadJob, InitCheckResult, JobStatus, StartupPhase, SubliminalInstallResult,
    SubtitleDownloader,
};
use crate::helper_functions::Utils;
use crate::python_manager::PythonManager;
use crate::scan_history::ScanHistory;
use crate::settings::Settings;
use crate::subtitle_utils::SubtitleUtils;

// Use the logging macros directly from the crate root
use crate::{debug, error, info, warn};

// Version check state
use once_cell::sync::Lazy;
type VersionCheckState = (Option<String>, Option<String>, bool);

static VERSION_PTR: Lazy<std::sync::Arc<std::sync::Mutex<VersionCheckState>>> =
    Lazy::new(|| std::sync::Arc::new(std::sync::Mutex::new((None, None, false))));

const VERSION_CHECK_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const VERSION_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_STATUS_BUFFER_BYTES: usize = 64 * 1024;
const WORKER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2_000);

fn join_worker_with_timeout(handle: Option<thread::JoinHandle<()>>, worker_name: &str) {
    let Some(handle) = handle else {
        return;
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    match rx.recv_timeout(WORKER_JOIN_TIMEOUT) {
        Ok(_) => {
            info!("{worker_name} thread exited");
        }
        Err(_) => {
            warn!(
                "{worker_name} thread did not exit within {:?}, continuing",
                WORKER_JOIN_TIMEOUT
            );
        }
    }
}

fn latest_non_empty_line(output: &str) -> Option<&str> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

fn next_char_at(s: &str, idx: usize) -> Option<char> {
    s[idx..].chars().next()
}

fn redact_sensitive(text: &str) -> String {
    let mut out = text.to_string();
    for key in ["apikey", "api_key", "password", "token"] {
        let mut search_from = 0;
        loop {
            let lower = out.to_ascii_lowercase();
            let Some(pos) = lower[search_from..].find(key) else {
                break;
            };
            let key_start = search_from + pos;
            let after_key = key_start + key.len();
            if let Some(eq_rel) = out[after_key..].find(['=', ':']) {
                let eq_pos = after_key + eq_rel;
                let mut value_start = eq_pos + 1;
                while value_start < out.len() {
                    let Some(character) = next_char_at(&out, value_start) else {
                        break;
                    };
                    if !character.is_whitespace() {
                        break;
                    }
                    value_start += character.len_utf8();
                }

                if value_start >= out.len() {
                    search_from = after_key;
                    continue;
                }

                let delimiter = match next_char_at(&out, value_start) {
                    Some(character) => character,
                    None => {
                        search_from = after_key;
                        continue;
                    }
                };
                let (start, end) = if matches!(delimiter, '"' | '\'') {
                    let start = value_start + delimiter.len_utf8();
                    let mut end = start;
                    let mut escaped = false;
                    let mut closed = false;
                    while end < out.len() {
                        let Some(character) = next_char_at(&out, end) else {
                            break;
                        };
                        end += character.len_utf8();
                        if escaped {
                            escaped = false;
                        } else if character == '\\' {
                            escaped = true;
                        } else if character == delimiter {
                            closed = true;
                            break;
                        }
                    }
                    let end = if closed {
                        end - delimiter.len_utf8()
                    } else {
                        out.len()
                    };
                    (start, end)
                } else {
                    let start = value_start;
                    let mut end = start;
                    while end < out.len() {
                        let Some(character) = next_char_at(&out, end) else {
                            break;
                        };
                        if character.is_whitespace() || matches!(character, '"' | '\'' | ',') {
                            break;
                        }
                        end += character.len_utf8();
                    }
                    (start, end)
                };
                if end > start {
                    out.replace_range(start..end, "***");
                }
                search_from = start + 3;
            } else {
                search_from = after_key;
            }
        }
    }
    out
}

fn escape_toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn subtitle_file_signature(path: &Path) -> Option<(u64, u128)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((metadata.len(), modified.as_nanos()))
}

fn subliminal_config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        dirs::config_dir()
            .map(|p| p.join("subliminal").join("subliminal.toml"))
            .or_else(|| {
                dirs::data_local_dir()
                    .map(|p| p.join("Acme").join("subliminal").join("subliminal.toml"))
            })
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|p| {
            p.join("Library")
                .join("Application Support")
                .join("subliminal")
                .join("subliminal.toml")
        })
    }
    #[cfg(target_os = "linux")]
    {
        xdg::BaseDirectories::new()
            .get_config_home()
            .map(|p| p.join("subliminal").join("subliminal.toml"))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn sync_subliminal_credentials(username: &str, password: &str, apikey: &str) {
    let Some(path) = subliminal_config_path() else {
        return;
    };
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let header = "[provider.opensubtitlescom]";
    let values = [
        ("username", username),
        ("password", password),
        ("apikey", apikey),
    ];
    if !lines.iter().any(|line| line.trim() == header) {
        if values.iter().all(|(_, value)| value.is_empty()) {
            return;
        }
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(header.to_string());
    }

    let mut in_target = false;
    let mut found = [false; 3];
    let mut rewritten = Vec::with_capacity(lines.len() + values.len());
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_target = trimmed == header;
        }
        if in_target {
            let key = trimmed.split_once('=').map(|(key, _)| key.trim());
            if let Some(index) = values.iter().position(|(name, _)| Some(*name) == key) {
                found[index] = true;
                if !values[index].1.is_empty() {
                    rewritten.push(format!(
                        "{} = \"{}\"",
                        values[index].0,
                        escape_toml_string(values[index].1)
                    ));
                }
                continue;
            }
        }
        rewritten.push(line);
    }

    if let Some(header_index) = rewritten.iter().position(|line| line.trim() == header) {
        let mut insert_at = header_index + 1;
        for (index, (name, value)) in values.iter().enumerate() {
            if !found[index] && !value.is_empty() {
                rewritten.insert(
                    insert_at,
                    format!("{} = \"{}\"", name, escape_toml_string(value)),
                );
                insert_at += 1;
            }
        }
    }

    let mut content = rewritten.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            crate::warn!(
                "Failed to create Subliminal config directory {}: {}",
                parent.display(),
                error
            );
        }
    }
    if let Err(error) = std::fs::write(&path, content) {
        crate::warn!(
            "Failed to write Subliminal config {}: {}",
            path.display(),
            error
        );
    }
}

fn probe_startup_dependencies() -> InitCheckResult {
    let python_info = PythonManager::get_python_info();
    let (python_command, python_version) = python_info
        .map(|(command, version)| (Some(command), Some(version)))
        .unwrap_or((None, None));
    let python_installed = python_command.is_some();

    if python_installed {
        if let Err(error) = PythonManager::add_scripts_to_path() {
            warn!("Failed to add Python Scripts to PATH: {}", error);
        }
        if let Err(error) = PythonManager::refresh_environment() {
            warn!("Failed to refresh environment: {}", error);
        }
    }

    // Probe commands that depend on the refreshed environment only after Python setup.
    let ffmpeg_handle = thread::spawn(PythonManager::is_ffmpeg_installed);
    let homebrew_handle = thread::spawn(PythonManager::is_homebrew_installed);
    #[cfg(target_os = "macos")]
    let pipx_handle = thread::spawn(PythonManager::_pipx_available);

    let ffmpeg_installed = ffmpeg_handle.join().unwrap_or(false);
    let homebrew_installed = homebrew_handle.join().unwrap_or(false);

    // Linux pipx bootstrapping depends on Python and must remain ordered.
    #[cfg(windows)]
    let (pipx_installed, pipx_version) = (true, None::<String>);
    #[cfg(target_os = "macos")]
    let (pipx_installed, pipx_version) = (pipx_handle.join().unwrap_or(false), None::<String>);
    #[cfg(target_os = "linux")]
    let (pipx_installed, pipx_version) = if python_installed {
        let available = PythonManager::_pipx_available();
        let installed = if !available {
            info!("pipx not found, attempting to install pipx");
            if PythonManager::try_install_pipx() {
                if let Err(error) = PythonManager::add_scripts_to_path() {
                    warn!("Failed to add pipx Scripts directory to PATH: {}", error);
                }
                if let Err(error) = PythonManager::refresh_environment() {
                    warn!(
                        "Failed to refresh environment after pipx installation: {}",
                        error
                    );
                }
                PythonManager::_pipx_available()
            } else {
                false
            }
        } else {
            available
        };
        let version = if installed {
            PythonManager::get_pipx_version()
        } else {
            None
        };
        (installed, version)
    } else {
        (false, None)
    };

    let (subliminal_installed, subliminal_version) =
        if python_installed && (pipx_installed || cfg!(target_os = "macos")) {
            PythonManager::check_subliminal_with_python(python_command.as_deref())
        } else {
            (false, None)
        };

    info!(
        "Python installed: {}, version: {:?}",
        python_installed, python_version
    );
    info!(
        "pipx installed: {}, version: {:?}",
        pipx_installed, pipx_version
    );
    info!(
        "Subliminal installed: {}, version: {:?}",
        subliminal_installed, subliminal_version
    );
    info!("FFmpeg installed: {}", ffmpeg_installed);
    info!("Homebrew installed: {}", homebrew_installed);

    InitCheckResult {
        python_command,
        python_version,
        pipx_installed,
        pipx_version,
        subliminal_installed,
        subliminal_version,
        ffmpeg_installed,
        homebrew_installed,
    }
}

fn install_subliminal_and_verify(
    python_command: Option<String>,
) -> Result<SubliminalInstallResult, String> {
    if !PythonManager::install_subliminal() {
        return Err("pipx/pip install failed".to_string());
    }
    PythonManager::add_scripts_to_path()
        .map_err(|e| format!("Subliminal installed, but failed to update PATH: {}", e))?;
    PythonManager::refresh_environment().map_err(|e| {
        format!(
            "Subliminal installed, but failed to refresh environment: {}",
            e
        )
    })?;

    let (installed, version) =
        PythonManager::check_subliminal_with_python(python_command.as_deref());
    if installed {
        Ok(SubliminalInstallResult { installed, version })
    } else {
        Err("Subliminal installation could not be verified".to_string())
    }
}

fn spawn_subliminal_install(
    result_ptr: Arc<Mutex<Option<Result<SubliminalInstallResult, String>>>>,
    python_command: Option<String>,
) {
    thread::spawn(move || {
        let result = install_subliminal_and_verify(python_command);
        *result_ptr.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });
}

fn verified_subliminal_state(
    result: Result<SubliminalInstallResult, String>,
) -> Result<(bool, Option<String>), String> {
    result.map(|result| (result.installed, result.version))
}

impl Default for SubtitleDownloader {
    fn default() -> Self {
        info!("Initializing SubtitleDownloader");
        let settings = Settings::load();
        if !settings.opensubtitlescom_username.is_empty()
            || !settings.opensubtitlescom_password.is_empty()
            || !settings.opensubtitlescom_apikey.is_empty()
        {
            sync_subliminal_credentials(
                &settings.opensubtitlescom_username,
                &settings.opensubtitlescom_password,
                &settings.opensubtitlescom_apikey,
            );
        }

        // Kick off dependency checks in background so the window appears instantly
        let (init_tx, init_rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = init_tx.send(probe_startup_dependencies());
        });

        // Start version check in background
        let version_ptr_clone = VERSION_PTR.clone();
        thread::spawn(move || {
            let url = "https://api.github.com/repos/fosterbarnes/rustitles/releases/latest";
            let (mut latest, mut err) = (None, None);
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(VERSION_CHECK_CONNECT_TIMEOUT)
                .timeout(VERSION_CHECK_TIMEOUT)
                .build();
            match client.and_then(|client| {
                client
                    .get(url)
                    .header("User-Agent", "rustitles-version-check")
                    .send()
            }) {
                Ok(r) if !r.status().is_success() => {
                    err = Some(format!("HTTP error: {}", r.status()));
                }
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>() {
                        if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                            latest = Some(tag.to_string());
                        } else {
                            err = Some("No tag_name in response".to_string());
                        }
                    } else {
                        err = Some("Failed to parse JSON".to_string());
                    }
                }
                Err(e) => err = Some(format!("HTTP error: {}", e)),
            }
            *version_ptr_clone.lock().unwrap_or_else(|e| e.into_inner()) = (latest, err, true);
        });

        Self {
            downloads_completed: 0,
            total_downloads: 0,
            downloading: false,
            download_thread_handle: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            download_jobs: Arc::new(Mutex::new(Vec::new())),
            python_command: None,
            python_installed: false,
            python_version: None,
            pipx_installed: false,
            pipx_version: None,
            subliminal_installed: false,
            subliminal_version: None,
            ffmpeg_installed: false,
            homebrew_installed: false,
            installing_python: false,
            installing_subliminal: false,
            python_install_result: Arc::new(Mutex::new(None)),
            subliminal_install_result: Arc::new(Mutex::new(None)),
            selected_languages: settings.selected_languages,
            skip_scanned_media: settings.skip_scanned_media,
            force_download: settings.force_download,
            overwrite_existing: settings.overwrite_existing,
            ignore_local_extras: settings.ignore_local_extras,
            concurrent_downloads: settings.concurrent_downloads,
            providers: settings.providers,
            refiners: settings.refiners,
            minimum_score: settings.minimum_score,
            exclude_hearing_impaired: settings.exclude_hearing_impaired,
            matching_options_open: settings.matching_options_open,
            opensubtitles_max_pages: settings.opensubtitles_max_pages,
            opensubtitlescom_username: settings.opensubtitlescom_username,
            opensubtitlescom_password: settings.opensubtitlescom_password,
            opensubtitlescom_apikey: settings.opensubtitlescom_apikey,
            show_opensubtitles_apikey: false,
            keep_dropdown_open: false,
            folder_path: String::new(),
            scanned_videos: Arc::new(Mutex::new(Vec::new())),
            videos_missing_subs: Arc::new(Mutex::new(Vec::new())),
            scanning: false,
            scan_thread_handle: None,
            scan_generation: Arc::new(AtomicUsize::new(0)),
            scan_done_receiver: None,
            scan_cancel_flag: Arc::new(AtomicBool::new(false)),
            ignored_extra_folders: 0,
            skipped_scanned_count: 0,
            status: "Checking dependencies...".to_string(),
            cached_jobs: Vec::new(),
            last_jobs_update: std::time::Instant::now(),
            background_check_handle: None,
            background_check_receiver: None,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            latest_version: None,
            version_check_error: None,
            version_checked: false,
            startup_phase: StartupPhase::Checking,
            init_check_receiver: Some(init_rx),
            splash_started: std::time::Instant::now(),
            splash_dismiss_started: None,
            scan_history: Arc::new(Mutex::new(ScanHistory::load())),
        }
    }
}

impl SubtitleDownloader {
    /// Save the current user settings to disk
    pub fn save_current_settings(&self) {
        let settings = Settings {
            selected_languages: self.selected_languages.clone(),
            skip_scanned_media: self.skip_scanned_media,
            force_download: self.force_download,
            overwrite_existing: self.overwrite_existing,
            ignore_local_extras: self.ignore_local_extras,
            concurrent_downloads: self.concurrent_downloads,
            providers: self.providers.clone(),
            refiners: self.refiners.clone(),
            minimum_score: self.minimum_score,
            exclude_hearing_impaired: self.exclude_hearing_impaired,
            matching_options_open: self.matching_options_open,
            opensubtitles_max_pages: self.opensubtitles_max_pages,
            opensubtitlescom_username: self.opensubtitlescom_username.clone(),
            opensubtitlescom_password: self.opensubtitlescom_password.clone(),
            opensubtitlescom_apikey: self.opensubtitlescom_apikey.clone(),
        };
        sync_subliminal_credentials(
            &self.opensubtitlescom_username,
            &self.opensubtitlescom_password,
            &self.opensubtitlescom_apikey,
        );

        if let Err(e) = settings.save() {
            warn!("Failed to save settings: {}", e);
        } else {
            debug!("Settings saved successfully");
        }
    }

    /// Scan the selected folder for video files and update the missing subtitles list
    pub fn scan_folder(&mut self) {
        if self.folder_path.is_empty() {
            return;
        }

        // If already scanning, cancel the previous scan first
        if self.scanning {
            self.scan_cancel_flag.store(true, Ordering::SeqCst);
            join_worker_with_timeout(self.scan_thread_handle.take(), "scan");
            self.scanning = false;
            self.scan_done_receiver = None;
            info!("Previous scan cancelled for rescan");
        }

        // Cancel any active downloads before clearing state
        if self.downloading {
            self.cancel_flag.store(true, Ordering::SeqCst);
            join_worker_with_timeout(self.download_thread_handle.take(), "download");
            self.downloading = false;
        }

        info!("Starting folder scan: {}", self.folder_path);
        if self.ignore_local_extras {
            info!("Ignore Local Extras is enabled - will skip local extras folders during scan");
        }
        self.status = "Scanning...".to_string();
        self.scanning = true;

        // Reset cancel flag for the new scan
        self.scan_cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&self.scan_cancel_flag);
        let scan_generation = Arc::clone(&self.scan_generation);
        let scan_generation_id = self.scan_generation.fetch_add(1, Ordering::SeqCst) + 1;

        let (tx, rx) = mpsc::channel();
        self.scan_done_receiver = Some(rx);

        let scanned_videos = Arc::clone(&self.scanned_videos);
        let videos_missing_subs = Arc::clone(&self.videos_missing_subs);
        let folder_path = self.folder_path.clone();
        let selected_languages = self.selected_languages.clone();
        let overwrite_existing = self.overwrite_existing;
        let skip_scanned_media = self.skip_scanned_media;
        let ignore_local_extras = self.ignore_local_extras;
        let exclude_hearing_impaired = self.exclude_hearing_impaired;
        let ignored_folders_count = Arc::new(AtomicUsize::new(0));
        let skipped_count = Arc::new(AtomicUsize::new(0));

        // Clear previous results and download state
        {
            *scanned_videos.lock().unwrap_or_else(|e| e.into_inner()) = Vec::new();
            *videos_missing_subs
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Vec::new();
        }
        {
            let mut jobs = self.download_jobs.lock().unwrap_or_else(|e| e.into_inner());
            jobs.clear();
        }
        self.cached_jobs.clear();
        self.downloads_completed = 0;
        self.total_downloads = 0;

        self.cancel_flag = Arc::new(AtomicBool::new(false));
        // Keep cancelled workers from mutating the next scan/download session.
        self.download_jobs = Arc::new(Mutex::new(Vec::new()));
        self.ignored_extra_folders = 0;
        self.skipped_scanned_count = 0;

        let ignored_folders_count_clone = Arc::clone(&ignored_folders_count);
        let skipped_count_clone = Arc::clone(&skipped_count);
        let scan_history_snapshot = self
            .scan_history
            .lock()
            .map(|history| history.clone())
            .unwrap_or_default();
        let scan_handle = thread::spawn(move || {
            let mut found_videos = Vec::new();
            let mut missing_subtitles = Vec::new();

            let walk_cancel = Arc::clone(&cancel_flag);
            let walk_ignored = Arc::clone(&ignored_folders_count_clone);
            let walker = jwalk::WalkDir::new(&folder_path)
                .skip_hidden(false)
                .process_read_dir(move |_depth, _path, _state, children| {
                    if walk_cancel.load(Ordering::SeqCst) {
                        children.clear();
                        return;
                    }
                    if !ignore_local_extras {
                        return;
                    }
                    for child in children.iter_mut() {
                        let Ok(entry) = child else { continue };
                        if !entry.file_type.is_dir() {
                            continue;
                        }
                        let Some(dir_name) = entry.file_name().to_str() else {
                            continue;
                        };
                        if Utils::is_plex_extras_folder(dir_name) {
                            debug!("Ignoring local extras folder: {}", entry.path().display());
                            walk_ignored.fetch_add(1, Ordering::Relaxed);
                            entry.read_children_path = None;
                        }
                    }
                });

            for entry in walker {
                if cancel_flag.load(Ordering::SeqCst) {
                    info!("Scan cancelled, discarding results");
                    return;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                if entry.file_type().is_file() && Utils::is_video_file(&entry.path()) {
                    found_videos.push(entry.path());
                }
            }

            if cancel_flag.load(Ordering::SeqCst) {
                info!("Scan cancelled, discarding results");
                return;
            }

            if overwrite_existing {
                missing_subtitles = found_videos.clone();
                info!(
                    "Overwrite mode enabled - including all {} videos",
                    found_videos.len()
                );
            } else {
                let mut videos_by_parent: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
                for video in found_videos.iter() {
                    if let Some(parent) = video.parent() {
                        videos_by_parent
                            .entry(parent.to_path_buf())
                            .or_default()
                            .push(video.clone());
                    }
                }
                for (parent, videos) in videos_by_parent {
                    if cancel_flag.load(Ordering::SeqCst) {
                        info!("Scan cancelled during subtitle check, discarding results");
                        return;
                    }
                    let listing = SubtitleUtils::list_subtitle_files_in_folder(&parent);
                    for video in videos {
                        if SubtitleUtils::video_missing_subtitle_in_listing(
                            &video,
                            &selected_languages,
                            exclude_hearing_impaired,
                            &listing,
                        ) {
                            missing_subtitles.push(video);
                        }
                    }
                }
                info!(
                    "Found {} videos, {} missing subtitles",
                    found_videos.len(),
                    missing_subtitles.len()
                );
            }

            if skip_scanned_media {
                let mut skipped = 0;
                missing_subtitles.retain(|video| {
                    if scan_history_snapshot.should_skip(
                        video,
                        &selected_languages,
                        exclude_hearing_impaired,
                    ) {
                        skipped += 1;
                        false
                    } else {
                        true
                    }
                });
                if skipped > 0 {
                    info!("Skip scanned media: {} previously successful file(s) excluded from download queue", skipped);
                }
                skipped_count_clone.store(skipped, Ordering::Relaxed);
            }

            if cancel_flag.load(Ordering::SeqCst)
                || scan_generation.load(Ordering::SeqCst) != scan_generation_id
            {
                info!("Scan cancelled or superseded before writing results, discarding");
                return;
            }

            let found_count = found_videos.len();
            let missing_count = missing_subtitles.len();

            *scanned_videos.lock().unwrap_or_else(|e| e.into_inner()) = found_videos;
            *videos_missing_subs
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = missing_subtitles;

            if ignore_local_extras {
                info!("Folder scan completed with local extras ignored - found {} videos, {} missing subtitles", found_count, missing_count);
            } else {
                info!(
                    "Folder scan completed - found {} videos, {} missing subtitles",
                    found_count, missing_count
                );
            }
            let ignored_count = ignored_folders_count_clone.load(Ordering::Relaxed);
            let skipped_count = skipped_count_clone.load(Ordering::Relaxed);
            let _ = tx.send((ignored_count, skipped_count));
        });
        self.scan_thread_handle = Some(scan_handle);
    }

    /// Signal workers to stop and wait briefly for scan/download threads (app exit).
    pub fn prepare_for_exit(&mut self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        self.scan_cancel_flag.store(true, Ordering::SeqCst);
        self.cancel_flag.store(true, Ordering::SeqCst);
        join_worker_with_timeout(self.scan_thread_handle.take(), "scan");
        join_worker_with_timeout(self.download_thread_handle.take(), "download");
    }

    /// Start subtitle downloads for all videos missing subtitles
    pub fn start_downloads(&mut self) {
        if self.downloading || self.selected_languages.is_empty() {
            self.status =
                "Select at least one language and ensure no downloads are in progress.".to_string();
            warn!(
                "Cannot start downloads: downloading={}, languages={:?}",
                self.downloading, self.selected_languages
            );
            return;
        }

        let videos_missing = self
            .videos_missing_subs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if videos_missing.is_empty() {
            self.status = "No videos missing subtitles.".to_string();
            info!("No videos to download subtitles for");
            return;
        }

        info!(
            "Starting subtitle downloads for {} videos with languages: {:?}",
            videos_missing.len(),
            self.selected_languages
        );
        self.status = "Starting subtitle downloads...".to_string();
        self.downloads_completed = 0;
        self.total_downloads = 0;
        let langs = self.selected_languages.clone();
        let jobs: Vec<_> = videos_missing
            .into_iter()
            .map(|video_path| DownloadJob {
                video_path,
                status: JobStatus::Pending,
                output: "Queued".to_string(),
                subtitle_paths: Vec::new(),
            })
            .collect();

        self.total_downloads = jobs.len();
        let mut download_jobs = self.download_jobs.lock().unwrap_or_else(|e| e.into_inner());
        *download_jobs = jobs;
        self.cached_jobs = download_jobs.clone();
        drop(download_jobs);
        self.downloading = true;

        self.cancel_flag.store(false, Ordering::SeqCst);

        let cancel_flag = Arc::clone(&self.cancel_flag);
        let jobs_arc = Arc::clone(&self.download_jobs);
        let max_concurrent = self.concurrent_downloads.max(1);
        let force_download = self.force_download;
        let overwrite_existing = self.overwrite_existing;
        let providers = self.providers.clone();
        let refiners = self.refiners.clone();
        let minimum_score = self.minimum_score;
        let exclude_hearing_impaired = self.exclude_hearing_impaired;
        let opensubtitles_max_pages = self.opensubtitles_max_pages;
        let python_command = self.python_command.clone();
        let scan_history = Arc::clone(&self.scan_history);

        info!("Starting download thread with {} concurrent downloads, force={}, overwrite={}, providers={:?}, refiners={:?}, minimum_score={}, exclude_sdh={}", max_concurrent, force_download, overwrite_existing, providers, refiners, minimum_score, exclude_hearing_impaired);

        self.download_thread_handle = Some(thread::spawn(move || {
            let mut pending_indexes: VecDeque<usize> = jobs_arc
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .enumerate()
                .filter(|(_, job)| job.status == JobStatus::Pending)
                .map(|(idx, _)| idx)
                .collect();
            let mut running_threads: Vec<thread::JoinHandle<()>> = Vec::new();
            let folder_cache: Arc<Mutex<HashMap<PathBuf, Vec<PathBuf>>>> =
                Arc::new(Mutex::new(HashMap::new()));

            while !pending_indexes.is_empty() || !running_threads.is_empty() {
                let mut active_threads = Vec::with_capacity(running_threads.len());
                for handle in running_threads.drain(..) {
                    if handle.is_finished() {
                        let _ = handle.join();
                    } else {
                        active_threads.push(handle);
                    }
                }
                running_threads = active_threads;

                while running_threads.len() < max_concurrent && !pending_indexes.is_empty() {
                    if cancel_flag.load(Ordering::SeqCst) {
                        info!("Download cancelled by user");
                        break;
                    }

                    let Some(idx) = pending_indexes.pop_front() else {
                        break;
                    };

                    {
                        let mut jobs_lock = jobs_arc.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(job) = jobs_lock.get_mut(idx) {
                            job.status = JobStatus::Running;
                            job.output = "Starting Subliminal...".to_string();
                        }
                    }

                    let job_path = {
                        let jobs_lock = jobs_arc.lock().unwrap_or_else(|e| e.into_inner());
                        jobs_lock.get(idx).map(|job| job.video_path.clone())
                    };
                    let Some(job_path) = job_path else {
                        warn!("Download worker skipped stale job index {idx}");
                        continue;
                    };

                    let langs_clone = langs.clone();
                    let providers_clone = providers.clone();
                    let refiners_clone = refiners.clone();
                    let opensubtitles_max_pages_clone = opensubtitles_max_pages;
                    let python_command_clone = python_command.clone();
                    let jobs_clone = Arc::clone(&jobs_arc);
                    let history_clone = Arc::clone(&scan_history);
                    let cancel_flag_clone = Arc::clone(&cancel_flag);
                    let folder_cache_clone = Arc::clone(&folder_cache);

                    let handle = thread::spawn(move || {
                        if cancel_flag_clone.load(Ordering::SeqCst) {
                            let mut jobs_lock =
                                jobs_clone.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(job) = jobs_lock.get_mut(idx) {
                                job.status = JobStatus::Failed("Cancelled".to_string());
                            }
                            return;
                        }

                        debug!("Processing video: {}", job_path.display());
                        // ponytail: cache per-folder listing to avoid duplicate read_dir for videos in same folder
                        let existing_subtitles = {
                            let listing = if let Some(parent) = job_path.parent() {
                                let cached = folder_cache_clone
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .get(parent)
                                    .cloned();
                                if let Some(v) = cached {
                                    v
                                } else {
                                    let v = SubtitleUtils::list_subtitle_files_in_folder(parent);
                                    folder_cache_clone
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .insert(parent.to_path_buf(), v.clone());
                                    v
                                }
                            } else {
                                Vec::new()
                            };
                            let mut v = SubtitleUtils::subtitle_files_for_video_in_listing(
                                &job_path, &listing,
                            );
                            v.sort();
                            v
                        };
                        let existing_subtitle_signatures: HashMap<PathBuf, (u64, u128)> =
                            existing_subtitles
                                .iter()
                                .filter_map(|path| {
                                    subtitle_file_signature(path)
                                        .map(|signature| (path.clone(), signature))
                                })
                                .collect();
                        if force_download || overwrite_existing {
                            let overwrite_targets =
                                SubtitleUtils::find_all_subtitle_files_in_listing(
                                    &job_path,
                                    &langs_clone,
                                    &existing_subtitles,
                                );
                            if let Err(error) =
                                SubtitleUtils::backup_subtitle_files(&overwrite_targets)
                            {
                                let mut jobs_lock =
                                    jobs_clone.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(job) = jobs_lock.get_mut(idx) {
                                    job.status = JobStatus::Failed(format!(
                                        "Failed to back up existing subtitles: {}",
                                        error
                                    ));
                                }
                                return;
                            }
                        }

                        // Create cache directory and set environment variables to fix DBM cache issues on Windows
                        let cache_dir = PythonManager::ensure_cache_dir()
                            .unwrap_or_else(|_| std::env::temp_dir().join("subliminal_cache"));
                        let mut env_vars = std::collections::HashMap::<String, String>::new();
                        env_vars.insert("PYTHONIOENCODING".to_string(), "utf-8".to_string());
                        env_vars.insert(
                            "SUBLIMINAL_CACHE_DIR".to_string(),
                            cache_dir.to_string_lossy().to_string(),
                        );
                        env_vars.insert("PYTHONHASHSEED".to_string(), "0".to_string());

                        // Additional environment variables to help with Windows DBM cache issues
                        #[cfg(windows)]
                        {
                            env_vars.insert(
                                "SUBLIMINAL_CACHE_BACKEND".to_string(),
                                "memory".to_string(),
                            );
                            env_vars.insert(
                                "PYTHONPATH".to_string(),
                                std::env::var("PYTHONPATH").unwrap_or_default(),
                            );
                        }
                        // Build command arguments with provider and matching controls.
                        // --debug is intentionally never added; it leaks provider tokens into stdout.
                        let mut all_args: Vec<String> = Vec::new();
                        if let Some(pages) = opensubtitles_max_pages_clone {
                            all_args
                                .push("--provider.opensubtitlescom.max_result_pages".to_string());
                            all_args.push(pages.to_string());
                        }
                        all_args.push("download".to_string());
                        if force_download || overwrite_existing {
                            all_args.push("--force".to_string());
                        }
                        for provider in &providers_clone {
                            all_args.push("--provider".to_string());
                            all_args.push(provider.clone());
                        }
                        for refiner in &refiners_clone {
                            if refiner == "metadata"
                                && job_path.to_string_lossy().starts_with(r"\\")
                            {
                                info!(
                                    "Skipping metadata refiner for UNC path: {}",
                                    job_path.display()
                                );
                                continue;
                            }
                            all_args.push("--refiner".to_string());
                            all_args.push(refiner.clone());
                        }
                        if minimum_score > 0 {
                            all_args.push("--min-score".to_string());
                            all_args.push(minimum_score.to_string());
                        }
                        for lang in &langs_clone {
                            all_args.push("-l".to_string());
                            all_args.push(lang.clone());
                        }

                        let Some(job_path_arg) = job_path.to_str() else {
                            if let Some(job) = jobs_clone
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get_mut(idx)
                            {
                                job.status =
                                    JobStatus::Failed("Video path is not valid UTF-8".to_string());
                            }
                            return;
                        };
                        all_args.push(job_path_arg.to_string());
                        debug!("Running subliminal with {} arguments", all_args.len());
                        let mut stdout_status_buffer = String::new();
                        let mut stderr_status_buffer = String::new();
                        let mut suppress_stdout_status = false;
                        let mut suppress_stderr_status = false;
                        let update_job_output = |stream: &str, status: &str| {
                            if let Ok(mut jobs_lock) = jobs_clone.lock() {
                                if let Some(job) = jobs_lock.get_mut(idx) {
                                    job.output = format!("{}: {}", stream, status);
                                }
                            }
                        };
                        let mut on_output =
                            |stream: &str, bytes: &[u8], _elapsed: std::time::Duration| {
                                let (status_buffer, suppress_status) = match stream {
                                    "stdout" => {
                                        (&mut stdout_status_buffer, &mut suppress_stdout_status)
                                    }
                                    _ => (&mut stderr_status_buffer, &mut suppress_stderr_status),
                                };
                                if *suppress_status {
                                    return;
                                }
                                status_buffer.push_str(&String::from_utf8_lossy(bytes));
                                if status_buffer.len() > MAX_STATUS_BUFFER_BYTES {
                                    // Discard the whole stream so a secret key cannot be separated from its value.
                                    status_buffer.clear();
                                    *suppress_status = true;
                                }
                            };
                        let output = PythonManager::run_subliminal(
                            &all_args,
                            &env_vars,
                            &cancel_flag_clone,
                            &mut on_output,
                            python_command_clone.as_deref(),
                        );

                        for (stream, status_buffer, suppress_status) in [
                            ("stdout", stdout_status_buffer, suppress_stdout_status),
                            ("stderr", stderr_status_buffer, suppress_stderr_status),
                        ] {
                            if suppress_status {
                                continue;
                            }
                            let redacted_output = redact_sensitive(&status_buffer);
                            if let Some(status) = latest_non_empty_line(&redacted_output) {
                                update_job_output(stream, status);
                            }
                        }

                        if cancel_flag_clone.load(Ordering::SeqCst) {
                            if let Some(job) = jobs_clone
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get_mut(idx)
                            {
                                job.status = JobStatus::Failed("Cancelled".to_string());
                            }
                            return;
                        }

                        let embedded_phrases = [
                            "embedded",
                            "already exists",
                            "no need to download",
                            "subtitle(s) already present",
                            "has embedded subtitles",
                            "skipping",
                        ];
                        let mut subtitle_paths = Vec::new();
                        let mut changed_subtitle_paths = Vec::new();
                        let job_status = match output {
                            Ok(out) => {
                                let stdout_str =
                                    String::from_utf8_lossy(&out.stdout).to_lowercase();
                                let stderr_str =
                                    String::from_utf8_lossy(&out.stderr).to_lowercase();
                                let combined_output =
                                    format!("{}\n{}", stdout_str, stderr_str).trim().to_string();
                                let folder_listing = job_path
                                    .parent()
                                    .map(SubtitleUtils::list_subtitle_files_in_folder)
                                    .unwrap_or_default();
                                subtitle_paths = SubtitleUtils::find_all_subtitle_files_in_listing(
                                    &job_path,
                                    &langs_clone,
                                    &folder_listing,
                                );
                                let all_subtitle_paths =
                                    SubtitleUtils::subtitle_files_for_video_in_listing(
                                        &job_path,
                                        &folder_listing,
                                    );
                                let mut excluded_sdh_count = 0;
                                if exclude_hearing_impaired {
                                    let rejected_paths: Vec<PathBuf> = all_subtitle_paths
                                        .iter()
                                        .filter(|path| {
                                            SubtitleUtils::is_hearing_impaired_path(path)
                                        })
                                        .cloned()
                                        .collect();
                                    let existing_subtitle_set: HashSet<PathBuf> =
                                        existing_subtitles.iter().cloned().collect();
                                    let rejected_path_set: HashSet<PathBuf> =
                                        rejected_paths.iter().cloned().collect();
                                    excluded_sdh_count = rejected_paths.len();
                                    for path in rejected_paths
                                        .iter()
                                        .filter(|path| !existing_subtitle_set.contains(*path))
                                    {
                                        if let Err(error) = std::fs::remove_file(path) {
                                            warn!(
                                                "Failed to remove excluded SDH subtitle {}: {}",
                                                path.display(),
                                                error
                                            );
                                        }
                                    }
                                    subtitle_paths.retain(|path| !rejected_path_set.contains(path));
                                }

                                changed_subtitle_paths = subtitle_paths
                                    .iter()
                                    .filter(|path| {
                                        match (
                                            existing_subtitle_signatures.get(*path),
                                            subtitle_file_signature(path),
                                        ) {
                                            (Some(before), Some(after)) => before != &after,
                                            (None, Some(_)) => true,
                                            _ => false,
                                        }
                                    })
                                    .cloned()
                                    .collect();

                                let video_name =
                                    job_path.file_name().unwrap_or_default().to_string_lossy();
                                info!("SUBTITLE JOBS OUTPUT: {} - Running", video_name);
                                for sub_path in &subtitle_paths {
                                    info!("SUBTITLE JOBS OUTPUT: {}", sub_path.display());
                                }
                                if combined_output.contains("downloaded 0 subtitle") {
                                    if !changed_subtitle_paths.is_empty() {
                                        JobStatus::Success
                                    } else if !force_download {
                                        let embedded_languages =
                                            SubtitleUtils::embedded_subtitle_languages(
                                                &job_path,
                                                &langs_clone,
                                            );
                                        if !embedded_languages.is_empty() {
                                            let embedded_names = embedded_languages
                                                .iter()
                                                .map(|code| {
                                                    SubtitleUtils::language_code_to_name(code)
                                                })
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            let missing_names = langs_clone
                                                .iter()
                                                .filter(|code| !embedded_languages.contains(code))
                                                .map(|code| {
                                                    SubtitleUtils::language_code_to_name(code)
                                                })
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            if missing_names.is_empty() {
                                                JobStatus::EmbeddedExists(format!("Embedded {} subtitles already exist (no external subtitles found online)", embedded_names))
                                            } else {
                                                JobStatus::Failed(format!("Embedded {} subtitles exist, but no external subtitles were found for {}", embedded_names, missing_names))
                                            }
                                        } else if excluded_sdh_count == 0
                                            && embedded_phrases
                                                .iter()
                                                .any(|phrase| combined_output.contains(phrase))
                                        {
                                            let lang_code = langs_clone
                                                .first()
                                                .cloned()
                                                .unwrap_or_else(|| "unknown".to_string());
                                            let lang_name =
                                                SubtitleUtils::language_code_to_name(&lang_code)
                                                    .to_string();
                                            JobStatus::EmbeddedExists(format!("Embedded {} subtitles already exist (no external subtitles found online)", lang_name))
                                        } else {
                                            JobStatus::Failed("No subtitles found (no embedded or external subtitles available)".to_string())
                                        }
                                    } else {
                                        JobStatus::Failed("No subtitles found online".to_string())
                                    }
                                } else if combined_output.contains("error")
                                    || combined_output.contains("failed")
                                {
                                    if combined_output.contains("dbm.error")
                                        || combined_output
                                            .contains("db type could not be determined")
                                    {
                                        if !changed_subtitle_paths.is_empty() {
                                            warn!("DBM cache error occurred but subtitles were downloaded successfully for {}", job_path.display());
                                            JobStatus::Success
                                        } else {
                                            warn!("DBM cache error for {} - this is often recoverable", job_path.display());
                                            JobStatus::Failed(
                                                "DBM cache error - try again later".to_string(),
                                            )
                                        }
                                    } else if !changed_subtitle_paths.is_empty() {
                                        JobStatus::Success
                                    } else {
                                        JobStatus::Failed("Subliminal error: see log".to_string())
                                    }
                                } else if excluded_sdh_count > 0 && subtitle_paths.is_empty() {
                                    JobStatus::Failed(
                                        "Only SDH or caption subtitles were found and excluded"
                                            .to_string(),
                                    )
                                } else if !changed_subtitle_paths.is_empty() {
                                    JobStatus::Success
                                } else {
                                    JobStatus::Failed("No new subtitles downloaded".to_string())
                                }
                            }
                            Err(error) => {
                                error!(
                                    "Failed to run subliminal for {}: {}",
                                    job_path.display(),
                                    error
                                );
                                JobStatus::Failed(format!("Failed to run subliminal: {}", error))
                            }
                        };

                        if matches!(job_status, JobStatus::Success)
                            && !changed_subtitle_paths.is_empty()
                        {
                            let mut langs = ScanHistory::covered_langs(
                                &job_path,
                                &changed_subtitle_paths,
                                &langs_clone,
                            );
                            if langs.is_empty() {
                                langs = langs_clone.clone();
                            }
                            if !langs.is_empty() {
                                if let Ok(mut history) = history_clone.lock() {
                                    let hearing_impaired_langs =
                                        ScanHistory::hearing_impaired_langs(
                                            &job_path,
                                            &changed_subtitle_paths,
                                            &langs,
                                        );
                                    history.record_success(
                                        &job_path,
                                        &langs,
                                        &hearing_impaired_langs,
                                    );
                                }
                            }
                        }

                        let mut jobs_lock = jobs_clone.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(job) = jobs_lock.get_mut(idx) {
                            job.status = job_status;
                            job.subtitle_paths = subtitle_paths;
                        }
                    });

                    running_threads.push(handle);
                }

                if cancel_flag.load(Ordering::SeqCst) {
                    info!("Download cancelled by user");
                    break;
                }

                let at_capacity = running_threads.len() >= max_concurrent;
                let waiting_on_workers = pending_indexes.is_empty() && !running_threads.is_empty();
                if at_capacity || waiting_on_workers {
                    thread::sleep(std::time::Duration::from_millis(50));
                }
            }

            for handle in running_threads {
                let _ = handle.join();
            }
            if let Ok(mut history) = scan_history.lock() {
                if let Err(error) = history.save_if_dirty() {
                    warn!("Failed to save scan history: {}", error);
                }
            }
            if cancel_flag.load(Ordering::SeqCst) {
                let mut jobs_lock = jobs_arc.lock().unwrap_or_else(|e| e.into_inner());
                for job in jobs_lock.iter_mut() {
                    if job.status == JobStatus::Pending || job.status == JobStatus::Running {
                        job.status = JobStatus::Failed("Cancelled".to_string());
                    }
                }
            }

            info!("Download thread completed");
        }));
    }

    /// Count jobs that are no longer pending or running (success, skipped, failed, etc.).
    fn download_jobs_progress(jobs: &[DownloadJob]) -> (usize, usize, usize) {
        let mut completed_count = 0;
        let mut running_count = 0;
        let mut failed_count = 0;
        for job in jobs {
            match &job.status {
                JobStatus::Pending => {}
                JobStatus::Running => running_count += 1,
                JobStatus::Failed(_) => {
                    completed_count += 1;
                    failed_count += 1;
                }
                _ => completed_count += 1,
            }
        }
        (completed_count, running_count, failed_count)
    }

    /// Update cached jobs if needed (to avoid cloning every frame)
    pub fn update_cached_jobs(&mut self) {
        let now = std::time::Instant::now();
        // Update cache every 500ms to improve performance
        if now.duration_since(self.last_jobs_update) >= std::time::Duration::from_millis(500) {
            if let Ok(jobs) = self.download_jobs.lock() {
                self.cached_jobs = jobs.clone();
                self.last_jobs_update = now;
            }
        }
    }

    /// Check if all downloads are complete and update progress
    pub fn check_download_completion(&mut self) {
        if !self.downloading {
            return;
        }

        // Update cached jobs if needed
        self.update_cached_jobs();

        // Use live job list for progress (cache can lag up to 500ms behind the worker thread).
        let (completed_count, running_count, failed_count) =
            if let Ok(jobs) = self.download_jobs.lock() {
                Self::download_jobs_progress(&jobs)
            } else {
                Self::download_jobs_progress(&self.cached_jobs)
            };

        let previous_completed = self.downloads_completed;
        self.downloads_completed = completed_count;

        // Log progress changes
        if self.downloads_completed != previous_completed {
            debug!(
                "Download progress: {}/{} completed, {} running, {} failed",
                self.downloads_completed, self.total_downloads, running_count, failed_count
            );
        }

        // Check if download thread is finished
        if let Some(handle) = &self.download_thread_handle {
            if handle.is_finished() {
                self.downloading = false;
                self.download_thread_handle = None;
                if let Ok(jobs) = self.download_jobs.lock() {
                    self.cached_jobs = jobs.clone();
                    self.last_jobs_update = std::time::Instant::now();
                    let (finished, _, _) = Self::download_jobs_progress(&jobs);
                    self.downloads_completed = finished;
                }

                // Count completed jobs using cached jobs
                let failed_count = self
                    .cached_jobs
                    .iter()
                    .filter(|j| matches!(j.status, JobStatus::Failed(_)))
                    .count();
                let success_count = self
                    .cached_jobs
                    .iter()
                    .filter(|j| {
                        j.status == JobStatus::Success
                            || matches!(j.status, JobStatus::EmbeddedExists(_))
                    })
                    .count();
                let skipped_count = self
                    .cached_jobs
                    .iter()
                    .filter(|j| j.status == JobStatus::Skipped)
                    .count();

                info!(
                    "Download session completed: {} successful, {} skipped, {} failed",
                    success_count, skipped_count, failed_count
                );
                self.status = format!(
                    "Subliminal jobs completed: {} successful, {} skipped, {} failed",
                    success_count, skipped_count, failed_count
                );
            } else {
                // Update status while downloading
                if running_count > 0 {
                    self.status = format!(
                        "Running: {} completed, {} running, {} pending",
                        completed_count,
                        running_count,
                        self.total_downloads - completed_count - running_count
                    );
                }
            }
        }
    }

    /// Handle the one-time startup dependency check result
    pub fn poll_init_check(&mut self) {
        if self.startup_phase != StartupPhase::Checking {
            return;
        }

        let result = match self
            .init_check_receiver
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            Some(r) => r,
            None => return,
        };
        self.init_check_receiver = None;

        self.python_command = result.python_command;
        self.python_version = result.python_version;
        self.python_installed = self.python_version.is_some();
        self.pipx_installed = result.pipx_installed;
        self.pipx_version = result.pipx_version;
        self.subliminal_installed = result.subliminal_installed;
        self.subliminal_version = result.subliminal_version;
        self.ffmpeg_installed = result.ffmpeg_installed;
        self.homebrew_installed = result.homebrew_installed;

        // Auto-install subliminal if python (and pipx on Linux) are ready
        if self.python_installed && self.pipx_installed && !self.subliminal_installed {
            info!("Starting automatic Subliminal installation");
            self.startup_phase = StartupPhase::Installing;
            self.installing_subliminal = true;
            self.status = "Python and pipx detected. Installing Subliminal...".to_string();
            let result_ptr = Arc::clone(&self.subliminal_install_result);
            spawn_subliminal_install(result_ptr, self.python_command.clone());
        } else {
            self.status = "Scanning will start automatically when a folder is selected".to_string();
            self.finish_initial_startup();
        }
    }

    fn finish_initial_startup(&mut self) {
        self.startup_phase = StartupPhase::Ready;
        self.start_background_check();
    }

    fn start_background_check(&mut self) {
        if self.background_check_handle.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let bg_shutdown = Arc::clone(&self.shutdown_flag);
        let bg = thread::spawn(move || {
            loop {
                if bg_shutdown.load(Ordering::Relaxed) {
                    return;
                }

                #[cfg(windows)]
                {
                    let sub = PythonManager::is_subliminal_installed();
                    if tx.send((true, sub, true, false)).is_err() {
                        break;
                    }
                    if sub {
                        break;
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let pipx = PythonManager::_pipx_available();
                    let sub = PythonManager::is_subliminal_installed();
                    let ffmpeg = PythonManager::is_ffmpeg_installed();
                    let homebrew = PythonManager::is_homebrew_installed();
                    if tx.send((pipx, sub, ffmpeg, homebrew)).is_err() {
                        break;
                    }
                    if pipx && sub && ffmpeg {
                        break;
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let pipx = PythonManager::_pipx_available();
                    let sub = if pipx {
                        PythonManager::is_subliminal_installed()
                    } else {
                        false
                    };
                    let ffmpeg = PythonManager::is_ffmpeg_installed();
                    if tx.send((pipx, sub, ffmpeg, false)).is_err() {
                        break;
                    }
                    if pipx && sub && ffmpeg {
                        break;
                    }
                }

                // Sleep in short intervals so we notice shutdown quickly
                for _ in 0..50 {
                    if bg_shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        });
        self.background_check_handle = Some(bg);
        self.background_check_receiver = Some(rx);
    }

    /// Refresh installation status using background thread results
    pub fn refresh_installation_status(&mut self) {
        if !self.startup_phase.is_terminal() {
            return;
        }

        let mut last_status = None;
        if let Some(receiver) = &self.background_check_receiver {
            while let Ok(status) = receiver.try_recv() {
                last_status = Some(status);
            }
        }
        if let Some((_pipx_available, subliminal_installed, ffmpeg_installed, homebrew_installed)) =
            last_status
        {
            let _old_pipx = self.pipx_installed;
            let old_subliminal = self.subliminal_installed;
            let old_ffmpeg = self.ffmpeg_installed;
            self.ffmpeg_installed = ffmpeg_installed;
            self.homebrew_installed = homebrew_installed;

            #[cfg(target_os = "linux")]
            {
                self.pipx_installed = _pipx_available;
            }
            #[cfg(windows)]
            {
                self.pipx_installed = true;
            }
            #[cfg(target_os = "macos")]
            {
                self.pipx_installed = _pipx_available;
            }

            #[cfg(any(windows, target_os = "macos"))]
            {
                if self.python_installed {
                    self.subliminal_installed = subliminal_installed;
                }
            }

            #[cfg(target_os = "linux")]
            {
                if self.python_installed && self.pipx_installed {
                    self.subliminal_installed = subliminal_installed;
                }
            }

            // If pipx became available (Linux only), start installing subliminal automatically
            #[cfg(target_os = "linux")]
            {
                if !_old_pipx && self.pipx_installed && !self.subliminal_installed {
                    info!("pipx became available, starting automatic Subliminal installation");
                    self.status = "pipx detected! Installing Subliminal...".to_string();
                    self.installing_subliminal = true;
                    let result_ptr = Arc::clone(&self.subliminal_install_result);
                    spawn_subliminal_install(result_ptr, self.python_command.clone());
                }
            }

            // If subliminal became available, update status
            if (!old_subliminal || !old_ffmpeg) && self.dependencies_ready() {
                info!("Subliminal became available");
                self.status =
                    "All dependencies installed. Ready to download subtitles.".to_string();
            }

            // Stop background checking and free resources
            #[cfg(any(windows, target_os = "macos"))]
            {
                if self.dependencies_ready() {
                    self.shutdown_flag.store(true, Ordering::Relaxed);
                    self.background_check_handle = None;
                    self.background_check_receiver = None;
                }
            }

            #[cfg(target_os = "linux")]
            {
                if self.dependencies_ready() {
                    self.shutdown_flag.store(true, Ordering::Relaxed);
                    self.background_check_handle = None;
                    self.background_check_receiver = None;
                }
            }
        }
    }

    /// Handle Python and Subliminal installation states
    pub fn handle_installation_states(&mut self) {
        if self.installing_python {
            if let Some(result) = self
                .python_install_result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                self.installing_python = false;
                match result {
                    Ok(_) => {
                        info!("Python installation completed successfully");
                        // Refresh environment to pick up new Python installation
                        if let Err(e) = PythonManager::refresh_environment() {
                            error!("Failed to refresh environment: {}", e);
                        }
                        if let Err(e) = PythonManager::add_scripts_to_path() {
                            error!("Failed to add Python Scripts to PATH: {}", e);
                        }
                        let python_info = PythonManager::get_python_info();
                        self.python_command =
                            python_info.as_ref().map(|(command, _)| command.clone());
                        self.python_version = python_info.map(|(_, version)| version);
                        self.python_installed = self.python_version.is_some();
                        self.status =
                            "Python installed successfully. Installing Subliminal...".to_string();
                        self.subliminal_installed = false;

                        // Start installing subliminal automatically
                        self.installing_subliminal = true;
                        let result_ptr = Arc::clone(&self.subliminal_install_result);
                        spawn_subliminal_install(result_ptr, self.python_command.clone());
                    }
                    Err(e) => {
                        error!("Python installation failed: {}", e);
                        self.status = format!("Python install failed: {}", e);
                    }
                }
            }
        }

        if self.installing_subliminal {
            let result = self
                .subliminal_install_result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(result) = result {
                let initial_startup_install = self.startup_phase == StartupPhase::Installing;
                self.installing_subliminal = false;
                match verified_subliminal_state(result) {
                    Ok((installed, version)) => {
                        info!("Subliminal installation completed successfully");
                        self.subliminal_installed = installed;
                        self.subliminal_version = version;
                        self.status = "Subliminal installed.".to_string();
                    }
                    Err(e) => {
                        error!("Subliminal installation failed: {}", e);
                        self.status = format!("Subliminal install failed: {}", e);
                    }
                }
                if initial_startup_install {
                    self.finish_initial_startup();
                }
            }
        }
    }

    /// Poll for version check results
    pub fn poll_version_check(&mut self) {
        if self.version_checked {
            return;
        }
        let lock = VERSION_PTR.lock().unwrap_or_else(|e| e.into_inner());
        if lock.2 {
            self.latest_version = lock.0.clone();
            self.version_check_error = lock.1.clone();
            self.version_checked = true;
        }
    }

    /// Compare two version strings (ignoring 'v' prefix). Returns true if current < latest.
    pub fn is_outdated(current: &str, latest: &str) -> bool {
        let parse = |s: &str| {
            s.trim_start_matches('v')
                .split('.')
                .map(|x| x.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        };
        let c = parse(current);
        let l = parse(latest);
        for (a, b) in c.iter().zip(l.iter()) {
            if a < b {
                return true;
            }
            if a > b {
                return false;
            }
        }
        c.len() < l.len() // e.g. 1.0 < 1.0.1
    }

    // Getters for GUI access
    pub fn is_installing_python(&self) -> bool {
        self.installing_python
    }
    pub fn is_installing_subliminal(&self) -> bool {
        self.installing_subliminal
    }
    pub fn is_subliminal_installed(&self) -> bool {
        self.subliminal_installed
    }
    pub fn is_ffmpeg_installed(&self) -> bool {
        self.ffmpeg_installed
    }
    pub fn is_python_installed(&self) -> bool {
        self.python_installed
    }
    pub fn is_pipx_installed(&self) -> bool {
        self.pipx_installed
    }
    pub fn get_python_version(&self) -> Option<&String> {
        self.python_version.as_ref()
    }
    pub fn get_pipx_version(&self) -> Option<&String> {
        self.pipx_version.as_ref()
    }
    pub fn get_subliminal_version(&self) -> Option<&String> {
        self.subliminal_version.as_ref()
    }
    pub fn get_status(&self) -> &str {
        &self.status
    }
    pub fn get_folder_path(&self) -> &str {
        &self.folder_path
    }
    pub fn is_scanning(&self) -> bool {
        self.scanning
    }
    pub fn is_downloading(&self) -> bool {
        self.downloading
    }
    pub fn get_downloads_completed(&self) -> usize {
        self.downloads_completed
    }
    pub fn get_total_downloads(&self) -> usize {
        self.total_downloads
    }
    pub fn get_cached_jobs(&self) -> &Vec<DownloadJob> {
        &self.cached_jobs
    }
    pub fn get_latest_version(&self) -> Option<&String> {
        self.latest_version.as_ref()
    }
    pub fn get_version_check_error(&self) -> Option<&String> {
        self.version_check_error.as_ref()
    }
    pub fn is_version_checked(&self) -> bool {
        self.version_checked
    }
    pub fn is_startup_ready(&self) -> bool {
        self.startup_phase.is_terminal()
    }
    pub fn splash_min_elapsed(&self) -> bool {
        self.splash_started.elapsed() >= crate::config::MIN_SPLASH_DURATION
    }
    pub fn begin_splash_dismiss_if_ready(&mut self) {
        if self.splash_dismiss_started.is_none()
            && self.is_startup_ready()
            && self.splash_min_elapsed()
        {
            self.splash_dismiss_started = Some(std::time::Instant::now());
        }
    }
    pub fn splash_out_alpha(&self) -> f32 {
        let Some(started) = self.splash_dismiss_started else {
            return 1.0;
        };
        let elapsed = started.elapsed().as_secs_f32();
        let wait = crate::config::SPLASH_FADE_WAIT.as_secs_f32();
        let fade = crate::config::SPLASH_FADE_DURATION.as_secs_f32();
        if elapsed <= wait {
            1.0
        } else {
            (1.0 - (elapsed - wait) / fade).clamp(0.0, 1.0)
        }
    }
    pub fn can_show_main_ui(&self) -> bool {
        let Some(started) = self.splash_dismiss_started else {
            return false;
        };
        started.elapsed()
            >= crate::config::SPLASH_FADE_WAIT
                + crate::config::SPLASH_FADE_DURATION
                + crate::config::SPLASH_POST_FADE_WAIT
    }
    pub fn dependencies_ready(&self) -> bool {
        #[cfg(windows)]
        {
            self.python_installed && self.subliminal_installed
        }
        #[cfg(not(windows))]
        {
            self.python_installed && self.subliminal_installed && self.ffmpeg_installed
        }
    }

    // Setters for GUI access
    pub fn set_installing_python(&mut self, installing: bool) {
        self.installing_python = installing;
    }
    pub fn set_python_install_result(&mut self, result: Arc<Mutex<Option<Result<(), String>>>>) {
        self.python_install_result = result;
    }
    pub fn set_folder_path(&mut self, path: String) {
        self.folder_path = path;
    }
    pub fn set_keep_dropdown_open(&mut self, open: bool) {
        self.keep_dropdown_open = open;
    }
    pub fn get_keep_dropdown_open(&self) -> bool {
        self.keep_dropdown_open
    }
    pub fn set_matching_options_open(&mut self, open: bool) {
        self.matching_options_open = open;
    }
    pub fn get_matching_options_open(&self) -> bool {
        self.matching_options_open
    }

    // Mutable access to settings
    pub fn get_selected_languages_mut(&mut self) -> &mut Vec<String> {
        &mut self.selected_languages
    }
    pub fn get_skip_scanned_media_mut(&mut self) -> &mut bool {
        &mut self.skip_scanned_media
    }
    pub fn get_force_download_mut(&mut self) -> &mut bool {
        &mut self.force_download
    }
    pub fn get_overwrite_existing(&self) -> bool {
        self.overwrite_existing
    }
    pub fn get_overwrite_existing_mut(&mut self) -> &mut bool {
        &mut self.overwrite_existing
    }
    pub fn get_ignore_local_extras(&self) -> bool {
        self.ignore_local_extras
    }
    pub fn get_ignore_local_extras_mut(&mut self) -> &mut bool {
        &mut self.ignore_local_extras
    }
    pub fn get_ignored_extra_folders(&self) -> usize {
        self.ignored_extra_folders
    }
    pub fn get_skipped_scanned_count(&self) -> usize {
        self.skipped_scanned_count
    }
    pub fn get_concurrent_downloads_mut(&mut self) -> &mut usize {
        &mut self.concurrent_downloads
    }
    pub fn get_scan_done_receiver_mut(&mut self) -> &mut Option<Receiver<(usize, usize)>> {
        &mut self.scan_done_receiver
    }
    pub fn get_shutdown_flag(&self) -> &Arc<AtomicBool> {
        &self.shutdown_flag
    }

    /// Start Subliminal installation in a background thread.
    pub fn start_subliminal_install(&mut self) {
        if self.installing_subliminal || !self.python_installed {
            return;
        }

        self.installing_subliminal = true;
        self.status = "Installing Subliminal...".to_string();
        *self
            .subliminal_install_result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        let result_ptr = Arc::clone(&self.subliminal_install_result);
        spawn_subliminal_install(result_ptr, self.python_command.clone());
    }

    /// Start Python installation in a background thread (Windows only)
    #[cfg(windows)]
    pub fn start_python_install(&mut self) {
        if self.installing_python {
            return; // Already installing
        }
        self.installing_python = true;
        self.status =
            "Installing Python... Check your taskbar for a UAC prompt (shield icon)".to_string();
        let result_ptr = self.python_install_result.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let installer = crate::python_manager::PythonManager::download_installer()
                    .map_err(|e| format!("Failed to download installer: {}", e))?;
                let ok = crate::python_manager::PythonManager::install_silent(&installer)
                    .map_err(|e| format!("Failed to run installer: {}", e))?;
                if ok {
                    Ok(())
                } else {
                    Err("Installer did not complete successfully".to_string())
                }
            })();
            *result_ptr.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        escape_toml_string, latest_non_empty_line, redact_sensitive, verified_subliminal_state,
    };
    use crate::data_structures::SubliminalInstallResult;

    #[test]
    fn verified_install_result_preserves_version() {
        let state = verified_subliminal_state(Ok(SubliminalInstallResult {
            installed: true,
            version: Some("subliminal 2.4.0".to_string()),
        }))
        .expect("verified installation should produce dependency state");

        assert_eq!(state, (true, Some("subliminal 2.4.0".to_string())));
    }

    #[test]
    fn latest_non_empty_line_keeps_partial_output() {
        assert_eq!(
            latest_non_empty_line("first\nfinal status"),
            Some("final status")
        );
    }

    #[test]
    fn redaction_handles_unicode_without_panicking() {
        assert_eq!(
            redact_sensitive("préface password=éclair token=abc apikey = \"secret value\""),
            "préface password=*** token=*** apikey = \"***\""
        );
    }

    #[test]
    fn toml_escaping_handles_special_characters() {
        assert_eq!(escape_toml_string("a\\b\"c\nd\te"), "a\\\\b\\\"c\\nd\\te");
    }
}
