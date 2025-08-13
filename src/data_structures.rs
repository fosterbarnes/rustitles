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
    EmbeddedExists(String), // full message
    Failed(String),
}

/// Represents a single subtitle download job
#[derive(Clone)]
pub struct DownloadJob {
    pub video_path: PathBuf,
    pub status: JobStatus,
    pub subtitle_paths: Vec<PathBuf>,
}

/// Main application state for the subtitle downloader
pub struct SubtitleDownloader {
    // Download state
    pub downloads_completed: usize,
    pub total_downloads: usize,
    pub is_downloading: bool,
    pub downloading: bool,
    pub download_thread_handle: Option<std::thread::JoinHandle<()>>,
    pub cancel_flag: Arc<std::sync::atomic::AtomicBool>,
    pub download_jobs: DownloadJobs,

    // Python/Subliminal state
    pub python_installed: bool,
    pub python_version: Option<String>,
    pub pipx_installed: bool,
    pub subliminal_installed: bool,
    pub installing_python: bool,
    pub installing_subliminal: bool,
    pub python_install_result: Arc<Mutex<Option<Result<(), String>>>>,
    pub subliminal_install_result: Arc<Mutex<Option<Result<(), String>>>>,

    // User settings
    pub selected_languages: Vec<String>,
    pub force_download: bool,
    pub overwrite_existing: bool,
    pub concurrent_downloads: usize,
    pub ignore_local_extras: bool,
    pub keep_dropdown_open: bool,

    // Folder and scan state
    pub folder_path: String,
    pub scanned_videos: SharedPaths,
    pub videos_missing_subs: SharedPaths,
    pub scanning: bool,
    pub scan_done_receiver: Option<std::sync::mpsc::Receiver<usize>>,
    pub ignored_extra_folders: usize,

    // UI status
    pub status: String,
    pub pipx_copied: bool, // Add this field to track copy state
    pub pipx_copy_time: Option<std::time::Instant>, // For timing the copied message
    
    // Auto-refresh state (unused but kept for potential future use)
    #[allow(dead_code)]
    pub last_refresh_time: std::time::Instant,
    #[allow(dead_code)]
    pub refresh_interval: std::time::Duration,
    
    // Cached jobs for UI rendering (to avoid cloning every frame)
    pub cached_jobs: Vec<DownloadJob>,
    pub last_jobs_update: std::time::Instant,
    
    // Background installation status checking
    pub background_check_handle: Option<std::thread::JoinHandle<()>>,
    pub background_check_sender: Option<std::sync::mpsc::Sender<(bool, bool)>>, // (_pipx_available, subliminal_installed)
    pub background_check_receiver: Option<std::sync::mpsc::Receiver<(bool, bool)>>,

    // Version check state
    pub latest_version: Option<String>,
    pub version_check_error: Option<String>,
    pub version_checked: bool,
} 