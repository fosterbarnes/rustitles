//! Data structures and types for the Rustitles subtitle downloader
//!
//! This module contains the core data structures including download jobs,
//! application state, and shared data types used throughout the application.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Type alias for shared download jobs
pub type DownloadJobs = Arc<Mutex<Vec<DownloadJob>>>;

/// Type alias for shared paths
pub type SharedPaths = Arc<Mutex<Vec<PathBuf>>>;

/// Status of a subtitle download job
#[derive(Clone, PartialEq)]
pub enum JobStatus {
    Pending,
    Running,
    Success,
    Skipped,
    EmbeddedExists(String), // full message
    Failed(String),
}

/// Represents a single subtitle download job
#[derive(Clone)]
pub struct DownloadJob {
    pub video_path: PathBuf,
    pub status: JobStatus,
    /// Latest subprocess status shown for pending or running jobs.
    pub output: String,
    pub subtitle_paths: Vec<PathBuf>,
}

/// Initial dependency state before the main UI becomes available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPhase {
    Checking,
    Installing,
    Ready,
}

impl StartupPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Verified result of a Subliminal installation.
pub struct SubliminalInstallResult {
    pub installed: bool,
    pub version: Option<String>,
}

/// Main application state for the subtitle downloader
pub struct SubtitleDownloader {
    // Download state
    pub downloads_completed: usize,
    pub total_downloads: usize,
    pub downloading: bool,
    pub download_thread_handle: Option<std::thread::JoinHandle<()>>,
    pub cancel_flag: Arc<std::sync::atomic::AtomicBool>,
    pub download_jobs: DownloadJobs,

    // Python/Subliminal state
    pub python_command: Option<String>,
    pub python_installed: bool,
    pub python_version: Option<String>,
    pub pipx_installed: bool,
    pub pipx_version: Option<String>,
    pub subliminal_installed: bool,
    pub subliminal_version: Option<String>,
    pub ffmpeg_installed: bool,
    pub homebrew_installed: bool,
    pub installing_python: bool,
    pub installing_subliminal: bool,
    pub python_install_result: Arc<Mutex<Option<Result<(), String>>>>,
    pub subliminal_install_result: Arc<Mutex<Option<Result<SubliminalInstallResult, String>>>>,

    // User settings
    pub selected_languages: Vec<String>,
    pub skip_scanned_media: bool,
    pub force_download: bool,
    pub overwrite_existing: bool,
    pub concurrent_downloads: usize,
    pub ignore_local_extras: bool,
    pub providers: Vec<String>,
    pub refiners: Vec<String>,
    pub minimum_score: u8,
    pub exclude_hearing_impaired: bool,
    pub matching_options_open: bool,
    pub keep_dropdown_open: bool,
    pub opensubtitles_max_pages: Option<u8>,
    pub opensubtitlescom_username: String,
    pub opensubtitlescom_password: String,
    pub opensubtitlescom_apikey: String,
    pub show_opensubtitles_apikey: bool,

    // Folder and scan state
    pub folder_path: String,
    pub scanned_videos: SharedPaths,
    pub videos_missing_subs: SharedPaths,
    pub scanning: bool,
    pub scan_thread_handle: Option<std::thread::JoinHandle<()>>,
    pub scan_generation: Arc<std::sync::atomic::AtomicUsize>,
    pub scan_done_receiver: Option<std::sync::mpsc::Receiver<(usize, usize)>>,
    pub scan_cancel_flag: Arc<std::sync::atomic::AtomicBool>,
    pub ignored_extra_folders: usize,
    pub skipped_scanned_count: usize,

    // UI status
    pub status: String,

    // Cached jobs for UI rendering (to avoid cloning every frame)
    pub cached_jobs: Vec<DownloadJob>,
    pub last_jobs_update: std::time::Instant,

    // Background installation status checking
    pub background_check_handle: Option<std::thread::JoinHandle<()>>,
    pub background_check_receiver: Option<std::sync::mpsc::Receiver<(bool, bool, bool, bool)>>,
    pub shutdown_flag: Arc<std::sync::atomic::AtomicBool>,

    // Version check state
    pub latest_version: Option<String>,
    pub version_check_error: Option<String>,
    pub version_checked: bool,

    // Startup dependency check (non-blocking init)
    pub startup_phase: StartupPhase,
    pub init_check_receiver: Option<std::sync::mpsc::Receiver<InitCheckResult>>,
    pub splash_started: std::time::Instant,
    pub splash_dismiss_started: Option<std::time::Instant>,
    pub scan_history: Arc<Mutex<crate::scan_history::ScanHistory>>,
}

/// Result of the background startup dependency check
pub struct InitCheckResult {
    pub python_command: Option<String>,
    pub python_version: Option<String>,
    pub pipx_installed: bool,
    pub pipx_version: Option<String>,
    pub subliminal_installed: bool,
    pub subliminal_version: Option<String>,
    pub ffmpeg_installed: bool,
    pub homebrew_installed: bool,
}

#[cfg(test)]
mod tests {
    use super::StartupPhase;

    #[test]
    fn startup_phase_is_terminal_only_when_ready() {
        assert!(!StartupPhase::Checking.is_terminal());
        assert!(!StartupPhase::Installing.is_terminal());
        assert!(StartupPhase::Ready.is_terminal());
    }
}
