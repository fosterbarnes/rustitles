//! Application data structures.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Shared download jobs.
pub type DownloadJobs = Arc<Mutex<Vec<DownloadJob>>>;

/// Shared paths.
pub type SharedPaths = Arc<Mutex<Vec<PathBuf>>>;

/// Subtitle download status.
#[derive(Clone, PartialEq)]
pub enum JobStatus {
    Pending,
    Running,
    Success,
    Skipped,
    EmbeddedExists(String),
    Failed(String),
}

/// A subtitle download job.
#[derive(Clone)]
pub struct DownloadJob {
    pub video_path: PathBuf,
    pub status: JobStatus,
    /// Latest status shown for pending or running jobs.
    pub output: String,
    pub subtitle_paths: Vec<PathBuf>,
}

/// Startup dependency state.
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

/// Verified Subliminal installation result.
#[derive(Default)]
pub struct SubliminalInstallResult {
    pub installed: bool,
    pub version: Option<String>,
    pub command: Option<crate::python_manager::SubliminalCommand>,
}

/// Main application state.
pub struct SubtitleDownloader {
    // Downloads
    pub downloads_completed: usize,
    pub total_downloads: usize,
    pub downloading: bool,
    pub download_thread_handle: Option<std::thread::JoinHandle<()>>,
    pub cancel_flag: Arc<std::sync::atomic::AtomicBool>,
    pub download_jobs: DownloadJobs,

    // Python and Subliminal
    pub python_command: Option<String>,
    pub python_installed: bool,
    pub python_version: Option<String>,
    pub pipx_installed: bool,
    pub pipx_version: Option<String>,
    pub subliminal_installed: bool,
    pub subliminal_version: Option<String>,
    pub subliminal_command: Option<crate::python_manager::SubliminalCommand>,
    pub ffmpeg_installed: bool,
    pub homebrew_installed: bool,
    pub installing_python: bool,
    pub installing_subliminal: bool,
    pub python_install_result: Arc<Mutex<Option<Result<(), String>>>>,
    pub subliminal_install_result: Arc<Mutex<Option<Result<SubliminalInstallResult, String>>>>,

    // Settings
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

    // Folder and scan
    pub folder_path: String,
    pub scanned_videos: SharedPaths,
    pub videos_missing_subs: SharedPaths,
    pub scanning: bool,
    pub scan_thread_handle: Option<std::thread::JoinHandle<()>>,
    pub scan_generation: Arc<std::sync::atomic::AtomicUsize>,
    pub scan_done_receiver:
        Option<std::sync::mpsc::Receiver<(usize, usize, crate::settings::Settings)>>,
    pub scan_cancel_flag: Arc<std::sync::atomic::AtomicBool>,
    pub ignored_extra_folders: usize,
    pub skipped_scanned_count: usize,

    // Status
    pub status: String,

    // Cached jobs
    pub cached_jobs: Vec<DownloadJob>,
    pub last_jobs_update: std::time::Instant,

    // Background dependency checks
    pub background_check_handle: Option<std::thread::JoinHandle<()>>,
    pub background_check_receiver:
        Option<std::sync::mpsc::Receiver<(bool, SubliminalInstallResult, bool, bool)>>,
    pub shutdown_flag: Arc<std::sync::atomic::AtomicBool>,

    // Version check
    pub latest_version: Option<String>,
    pub version_check_error: Option<String>,
    pub version_checked: bool,

    // Startup check
    pub startup_phase: StartupPhase,
    pub init_check_receiver: Option<std::sync::mpsc::Receiver<InitCheckResult>>,
    pub splash_started: std::time::Instant,
    pub splash_dismiss_started: Option<std::time::Instant>,
    pub scan_history: Arc<Mutex<crate::scan_history::ScanHistory>>,
}

/// Background startup dependency result.
pub struct InitCheckResult {
    pub python_command: Option<String>,
    pub python_version: Option<String>,
    pub pipx_installed: bool,
    pub pipx_version: Option<String>,
    pub subliminal_installed: bool,
    pub subliminal_version: Option<String>,
    pub subliminal_command: Option<crate::python_manager::SubliminalCommand>,
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
