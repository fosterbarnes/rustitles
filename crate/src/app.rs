//! Application logic for the Rustitles subtitle downloader
//! 
//! This module contains the main application state and logic.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::sync::mpsc::{self, Receiver};

use crate::data_structures::{SubtitleDownloader, DownloadJob, JobStatus, InitCheckResult};
use crate::settings::Settings;
use crate::python_manager::PythonManager;
use crate::subtitle_utils::SubtitleUtils;
use crate::helper_functions::Utils;

// Use the logging macros directly from the crate root
use crate::{info, warn, debug, error};

// Version check state
use once_cell::sync::Lazy;
static VERSION_PTR: Lazy<std::sync::Arc<std::sync::Mutex<(Option<String>, Option<String>, bool)>>> = Lazy::new(|| {
    std::sync::Arc::new(std::sync::Mutex::new((None, None, false)))
});

impl Default for SubtitleDownloader {
    fn default() -> Self {
        info!("Initializing SubtitleDownloader");
        let settings = Settings::load();

        // Kick off dependency checks in background so the window appears instantly
        let (init_tx, init_rx) = mpsc::channel();
        thread::spawn(move || {
            let python_version = PythonManager::get_version();
            let python_installed = python_version.is_some();

            // pipx is only used on Linux
            #[cfg(any(windows, target_os = "macos"))]
            let (pipx_installed, pipx_version) = (true, None::<String>);
            #[cfg(target_os = "linux")]
            let (pipx_installed, pipx_version) = if python_installed {
                let available = PythonManager::_pipx_available();
                let installed = if !available {
                    info!("pipx not found, attempting to install pipx");
                    if PythonManager::try_install_pipx() { PythonManager::_pipx_available() } else { false }
                } else {
                    available
                };
                let ver = if installed { PythonManager::get_pipx_version() } else { None };
                (installed, ver)
            } else {
                (false, None)
            };

            let (subliminal_installed, subliminal_version) = if python_installed && pipx_installed {
                PythonManager::check_subliminal()
            } else {
                (false, None)
            };

            info!("Python installed: {}, version: {:?}", python_installed, python_version);
            info!("pipx installed: {}, version: {:?}", pipx_installed, pipx_version);
            info!("Subliminal installed: {}, version: {:?}", subliminal_installed, subliminal_version);

            let _ = init_tx.send(InitCheckResult { python_version, pipx_installed, pipx_version, subliminal_installed, subliminal_version });
        });

        // Start version check in background
        let version_ptr_clone = VERSION_PTR.clone();
        thread::spawn(move || {
            let url = "https://api.github.com/repos/fosterbarnes/rustitles/releases/latest";
            let client = reqwest::blocking::Client::new();
            let resp = client.get(url)
                .header("User-Agent", "rustitles-version-check")
                .send();
            let (mut latest, mut err) = (None, None);
            match resp {
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
            *version_ptr_clone.lock().unwrap() = (latest, err, true);
        });

        Self {
            downloads_completed: 0,
            total_downloads: 0,
            is_downloading: false,
            downloading: false,
            download_thread_handle: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            download_jobs: Arc::new(Mutex::new(Vec::new())),
            python_installed: false,
            python_version: None,
            pipx_installed: false,
            pipx_version: None,
            subliminal_installed: false,
            subliminal_version: None,
            installing_python: false,
            installing_subliminal: false,
            python_install_result: Arc::new(Mutex::new(None)),
            subliminal_install_result: Arc::new(Mutex::new(None)),
            selected_languages: settings.selected_languages,
            force_download: settings.force_download,
            overwrite_existing: settings.overwrite_existing,
            ignore_local_extras: settings.ignore_local_extras,
            concurrent_downloads: settings.concurrent_downloads,
            keep_dropdown_open: false,
            folder_path: String::new(),
            scanned_videos: Arc::new(Mutex::new(Vec::new())),
            videos_missing_subs: Arc::new(Mutex::new(Vec::new())),
            scanning: false,
            scan_done_receiver: None,
            scan_cancel_flag: Arc::new(AtomicBool::new(false)),
            ignored_extra_folders: 0,
            status: "Checking dependencies...".to_string(),
            pipx_copied: false,
            pipx_copy_time: None,
            last_refresh_time: std::time::Instant::now(),
            refresh_interval: std::time::Duration::from_secs(2),
            cached_jobs: Vec::new(),
            last_jobs_update: std::time::Instant::now(),
            background_check_handle: None,
            background_check_receiver: None,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            latest_version: None,
            version_check_error: None,
            version_checked: false,
            checking_deps: true,
            init_check_receiver: Some(init_rx),
        }
    }
}

impl SubtitleDownloader {
    /// Save the current user settings to disk
    pub fn save_current_settings(&self) {
        let settings = Settings {
            selected_languages: self.selected_languages.clone(),
            force_download: self.force_download,
            overwrite_existing: self.overwrite_existing,
            ignore_local_extras: self.ignore_local_extras,
            concurrent_downloads: self.concurrent_downloads,
        };
        
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
            self.scanning = false;
            self.scan_done_receiver = None;
            info!("Previous scan cancelled for rescan");
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

        let (tx, rx) = mpsc::channel();
        self.scan_done_receiver = Some(rx);

        let scanned_videos = Arc::clone(&self.scanned_videos);
        let videos_missing_subs = Arc::clone(&self.videos_missing_subs);
        let folder_path = self.folder_path.clone();
        let selected_languages = self.selected_languages.clone();
        let overwrite_existing = self.overwrite_existing;
        let ignore_local_extras = self.ignore_local_extras;
        let ignored_folders_count = Arc::new(Mutex::new(0));

        // Clear previous results and download state
        {
            *scanned_videos.lock().unwrap() = Vec::new();
            *videos_missing_subs.lock().unwrap() = Vec::new();
        }
        {
            let mut jobs = self.download_jobs.lock().unwrap();
            jobs.clear();
        }
        self.cached_jobs.clear();
        self.downloads_completed = 0;
        self.total_downloads = 0;

        // Cancel any active downloads
        if self.downloading {
            self.cancel_flag.store(true, Ordering::SeqCst);
            self.downloading = false;
            self.is_downloading = false;
        }
        self.ignored_extra_folders = 0;

        let ignored_folders_count_clone = Arc::clone(&ignored_folders_count);
        thread::spawn(move || {
            let mut found_videos = Vec::new();
            let mut missing_subtitles = Vec::new();

            fn visit_dirs(
                dir: &Path,
                videos: &mut Vec<PathBuf>,
                ignore_extras: bool,
                ignored_count: &Arc<Mutex<usize>>,
                cancel: &Arc<AtomicBool>,
            ) -> bool {
                if cancel.load(Ordering::SeqCst) {
                    return false;
                }
                if let Ok(entries) = dir.read_dir() {
                    for entry in entries.flatten() {
                        if cancel.load(Ordering::SeqCst) {
                            return false;
                        }
                        let path = entry.path();
                        if path.is_dir() {
                            if ignore_extras {
                                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                                    let extras_folders = [
                                        "Behind The Scenes", "Deleted Scenes", "Featurettes",
                                        "Interviews", "Scenes", "Shorts", "Trailers", "Other"
                                    ];
                                    if extras_folders.contains(&dir_name) {
                                        info!("Ignoring local extras folder: {}", path.display());
                                        if let Ok(mut count) = ignored_count.lock() {
                                            *count += 1;
                                        }
                                        continue;
                                    }
                                }
                            }
                            if !visit_dirs(&path, videos, ignore_extras, ignored_count, cancel) {
                                return false;
                            }
                        } else if Utils::is_video_file(&path) {
                            videos.push(path);
                        }
                    }
                }
                true
            }

            let completed = visit_dirs(
                Path::new(&folder_path),
                &mut found_videos,
                ignore_local_extras,
                &ignored_folders_count_clone,
                &cancel_flag,
            );

            if !completed || cancel_flag.load(Ordering::SeqCst) {
                info!("Scan cancelled, discarding results");
                return;
            }

            if overwrite_existing {
                missing_subtitles = found_videos.clone();
                info!("Overwrite mode enabled - including all {} videos", found_videos.len());
            } else {
                for video in &found_videos {
                    if cancel_flag.load(Ordering::SeqCst) {
                        info!("Scan cancelled during subtitle check, discarding results");
                        return;
                    }
                    if SubtitleUtils::video_missing_subtitle(video, &selected_languages) {
                        missing_subtitles.push(video.clone());
                    }
                }
                info!("Found {} videos, {} missing subtitles", found_videos.len(), missing_subtitles.len());
            }

            if cancel_flag.load(Ordering::SeqCst) {
                info!("Scan cancelled before writing results, discarding");
                return;
            }

            let found_count = found_videos.len();
            let missing_count = missing_subtitles.len();
            
            *scanned_videos.lock().unwrap() = found_videos;
            *videos_missing_subs.lock().unwrap() = missing_subtitles;

            if ignore_local_extras {
                info!("Folder scan completed with local extras ignored - found {} videos, {} missing subtitles", found_count, missing_count);
            } else {
                info!("Folder scan completed - found {} videos, {} missing subtitles", found_count, missing_count);
            }
            
            let ignored_count = if let Ok(count) = ignored_folders_count_clone.lock() {
                *count
            } else {
                0
            };
            let _ = tx.send(ignored_count);
        });
    }

    /// Start subtitle downloads for all videos missing subtitles
    pub fn start_downloads(&mut self) {
        if self.downloading || self.selected_languages.is_empty() {
            self.status = "Select at least one language and ensure no downloads are in progress.".to_string();
            warn!("Cannot start downloads: downloading={}, languages={:?}", self.downloading, self.selected_languages);
            return;
        }

        let videos_missing = self.videos_missing_subs.lock().unwrap().clone();
        if videos_missing.is_empty() {
            self.status = "No videos missing subtitles.".to_string();
            info!("No videos to download subtitles for");
            return;
        }

        info!("Starting subtitle downloads for {} videos with languages: {:?}", videos_missing.len(), self.selected_languages);
        self.status = "Starting subtitle downloads...".to_string();
        self.downloads_completed = 0;
        self.total_downloads = 0;
        self.is_downloading = true;

        let langs = self.selected_languages.clone();
        let jobs: Vec<_> = videos_missing.into_iter()
            .map(|video_path| DownloadJob { video_path, status: JobStatus::Pending, subtitle_paths: Vec::new() })
            .collect();

        self.total_downloads = jobs.len();
        *self.download_jobs.lock().unwrap() = jobs;
        self.cached_jobs.clear(); // Clear cached jobs when starting new downloads
        self.downloading = true;

        self.cancel_flag.store(false, Ordering::SeqCst);

        let cancel_flag = Arc::clone(&self.cancel_flag);
        let jobs_arc = Arc::clone(&self.download_jobs);
        let max_concurrent = self.concurrent_downloads;
        let force_download = self.force_download;
        let overwrite_existing = self.overwrite_existing;

        info!("Starting download thread with {} concurrent downloads, force={}, overwrite={}", max_concurrent, force_download, overwrite_existing);

        self.download_thread_handle = Some(thread::spawn(move || {
            let mut pending_indexes: VecDeque<usize> = (0..jobs_arc.lock().unwrap().len()).collect();
            let mut running_threads = Vec::new();

            while !pending_indexes.is_empty() || !running_threads.is_empty() {
                running_threads.retain(|handle: &thread::JoinHandle<()>| !handle.is_finished());

                while running_threads.len() < max_concurrent && !pending_indexes.is_empty() {
                    if cancel_flag.load(Ordering::SeqCst) {
                        info!("Download cancelled by user");
                        let mut jobs_lock = jobs_arc.lock().unwrap();
                        for job in jobs_lock.iter_mut() {
                            if job.status == JobStatus::Pending || job.status == JobStatus::Running {
                                job.status = JobStatus::Failed("Cancelled".to_string());
                            }
                        }
                        return;
                    }

                    let idx = pending_indexes.pop_front().unwrap();

                    {
                        let mut jobs_lock = jobs_arc.lock().unwrap();
                        if let Some(job) = jobs_lock.get_mut(idx) {
                            job.status = JobStatus::Running;
                        }
                    }

                    let job_path = {
                        let jobs_lock = jobs_arc.lock().unwrap();
                        jobs_lock[idx].video_path.clone()
                    };

                    let langs_clone = langs.clone();
                    let jobs_clone = Arc::clone(&jobs_arc);
                    let cancel_flag_clone = Arc::clone(&cancel_flag);

                    let handle = thread::spawn(move || {
                        if cancel_flag_clone.load(Ordering::SeqCst) {
                            let mut jobs_lock = jobs_clone.lock().unwrap();
                            if let Some(job) = jobs_lock.iter_mut().find(|j| j.video_path == job_path) {
                                job.status = JobStatus::Failed("Cancelled".to_string());
                            }
                            return;
                        }

                        debug!("Processing video: {}", job_path.display());

                        // Create cache directory and set environment variables to fix DBM cache issues on Windows
                        let cache_dir = PythonManager::ensure_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("subliminal_cache"));
                        let mut env_vars = std::collections::HashMap::<String, String>::new();
                        env_vars.insert("PYTHONIOENCODING".to_string(), "utf-8".to_string());
                        env_vars.insert("SUBLIMINAL_CACHE_DIR".to_string(), cache_dir.to_string_lossy().to_string());
                        env_vars.insert("PYTHONHASHSEED".to_string(), "0".to_string());
                        
                        // Additional environment variables to help with Windows DBM cache issues
                        #[cfg(windows)]
                        {
                            env_vars.insert("SUBLIMINAL_CACHE_BACKEND".to_string(), "memory".to_string());
                            env_vars.insert("PYTHONPATH".to_string(), std::env::var("PYTHONPATH").unwrap_or_default());
                        }
                        
                        // Build command arguments with multiple -l flags for each language
                        let mut args = vec!["download"];
                        if force_download {
                            args.push("--force");
                        }
                        if overwrite_existing {
                            args.push("--force");
                        }
                        for lang in &langs_clone {
                            args.push("-l");
                            args.push(lang);
                        }
                        
                        // Run subliminal with multiple failsafes
                        let mut all_args = args.clone();
                        all_args.push(job_path.to_str().unwrap());
                        
                        debug!("Running subliminal command: subliminal {}", all_args.join(" "));
                        
                        let output = PythonManager::run_command_hidden("subliminal", &all_args, &env_vars)
                            .or_else(|_| {
                                debug!("Subliminal direct command failed, trying python -m subliminal");
                                let mut python_args = vec!["-m", "subliminal"];
                                python_args.extend(&all_args);
                                PythonManager::run_command_hidden("python", &python_args, &env_vars)
                            })
                            .or_else(|_| {
                                debug!("Python command failed, trying py -m subliminal");
                                let mut python_args = vec!["-m", "subliminal"];
                                python_args.extend(&all_args);
                                PythonManager::run_command_hidden("py", &python_args, &env_vars)
                            })
                            .or_else(|_| {
                                debug!("Py command failed, trying python3 -m subliminal");
                                let mut python_args = vec!["-m", "subliminal"];
                                python_args.extend(&all_args);
                                PythonManager::run_command_hidden("python3", &python_args, &env_vars)
                            });

                        let mut jobs_lock = jobs_clone.lock().unwrap();
                        let job_opt = jobs_lock.iter_mut().find(|j| j.video_path == job_path);

                        let embedded_phrases = [
                            "embedded", "already exists", "no need to download", "subtitle(s) already present", "has embedded subtitles", "skipping"
                        ];
                        if let Ok(out) = output {
                            let stdout_str = String::from_utf8_lossy(&out.stdout).to_lowercase();
                            let stderr_str = String::from_utf8_lossy(&out.stderr).to_lowercase();
                            let combined_output = format!("{}\n{}", stdout_str, stderr_str).trim().to_string();
                            let subtitle_paths = SubtitleUtils::find_all_subtitle_files(&job_path, &langs_clone);
                            
                            // --- LOGGING: Full Subliminal output ---
                            info!("Subliminal output for {}:\n{}", job_path.display(), combined_output);
                            info!("END subliminal output");
                            
                            if let Some(job) = job_opt {
                                // --- LOGGING: Video name and status ---
                                let video_name = job_path.file_name().unwrap_or_default().to_string_lossy();
                                let status_str = match &job.status {
                                    JobStatus::Success => "Success",
                                    JobStatus::EmbeddedExists(_) => "Embedded",
                                    JobStatus::Failed(_) => "Failed",
                                    JobStatus::Pending => "Pending",
                                    JobStatus::Running => "Running",
                                };
                                info!("SUBTITLE JOBS OUTPUT: {} - {}", video_name, status_str);
                                // --- LOGGING: Subtitle file paths ---
                                for sub_path in &subtitle_paths {
                                    info!("SUBTITLE JOBS OUTPUT: {}", sub_path.display());
                                }
                                // --- END LOGGING ---
                                
                                if combined_output.contains("downloaded 0 subtitle") {
                                    if !subtitle_paths.is_empty() {
                                        // If any subtitles were downloaded, always report Success (even if ignoring embedded)
                                        job.status = JobStatus::Success;
                                    } else if !force_download {
                                        // Only check for embedded if not forcing download
                                        if let Some(lang_name) = SubtitleUtils::has_embedded_subtitle(&job_path, &langs_clone) {
                                            job.status = JobStatus::EmbeddedExists(format!("Embedded {} subtitles already exist (no external subtitles found online)", lang_name));
                                        } else if embedded_phrases.iter().any(|phrase| combined_output.contains(phrase)) {
                                            let lang_code = langs_clone.get(0).cloned().unwrap_or_else(|| "unknown".to_string());
                                            let lang_name = SubtitleUtils::language_code_to_name(&lang_code).to_string();
                                            job.status = JobStatus::EmbeddedExists(format!("Embedded {} subtitles already exist (no external subtitles found online)", lang_name));
                                        } else {
                                            job.status = JobStatus::Failed("No subtitles found (no embedded or external subtitles available)".to_string());
                                        }
                                    } else {
                                        // Forced, but nothing downloaded
                                        job.status = JobStatus::Failed("No subtitles found online".to_string());
                                    }
                                } else if combined_output.contains("error") || combined_output.contains("failed") {
                                    // Check if this is a DBM cache error (which is often recoverable)
                                    if combined_output.contains("dbm.error") || combined_output.contains("db type could not be determined") {
                                        if !subtitle_paths.is_empty() {
                                            // If subtitles were downloaded despite cache error, mark as success
                                            job.status = JobStatus::Success;
                                            warn!("DBM cache error occurred but subtitles were downloaded successfully for {}", job_path.display());
                                        } else {
                                            // Cache error with no subtitles - this might be recoverable
                                            job.status = JobStatus::Failed("DBM cache error - try again later".to_string());
                                            warn!("DBM cache error for {} - this is often recoverable", job_path.display());
                                        }
                                    } else if !subtitle_paths.is_empty() {
                                        // Other error but subtitles were downloaded
                                        job.status = JobStatus::Success;
                                    } else {
                                        // Other error with no subtitles
                                        job.status = JobStatus::Failed("Subliminal error: see log".to_string());
                                    }
                                } else {
                                    job.status = JobStatus::Success;
                                }
                                job.subtitle_paths = subtitle_paths;
                            }
                        } else {
                            error!("Failed to run subliminal for {}", job_path.display());
                            if let Some(job) = job_opt {
                                job.status = JobStatus::Failed("Failed to run subliminal".to_string());
                            }
                        }
                    });

                    running_threads.push(handle);
                }

                if cancel_flag.load(Ordering::SeqCst) {
                    info!("Download cancelled by user");
                    let mut jobs_lock = jobs_arc.lock().unwrap();
                    for job in jobs_lock.iter_mut() {
                        if job.status == JobStatus::Pending || job.status == JobStatus::Running {
                            job.status = JobStatus::Failed("Cancelled".to_string());
                        }
                    }
                    break;
                }

                thread::sleep(std::time::Duration::from_millis(200));
            }
            
            info!("Download thread completed");
        }));
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
        
        // Use cached jobs for progress calculations
        let success_count = self.cached_jobs.iter().filter(|j| j.status == JobStatus::Success || matches!(j.status, JobStatus::EmbeddedExists(_))).count();
        let running_count = self.cached_jobs.iter().filter(|j| j.status == JobStatus::Running).count();
        let failed_count = self.cached_jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed(_))).count();
        
        let previous_completed = self.downloads_completed;
        self.downloads_completed = success_count;

        // Log progress changes
        if self.downloads_completed != previous_completed {
            debug!("Download progress: {}/{} completed, {} running, {} failed", 
                self.downloads_completed, self.total_downloads, running_count, failed_count);
        }

        // Check if download thread is finished
        if let Some(handle) = &self.download_thread_handle {
            if handle.is_finished() {
                self.downloading = false;
                self.download_thread_handle = None;
                
                // Count completed jobs using cached jobs
                let failed_count = self.cached_jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed(_))).count();
                let success_count = self.cached_jobs.iter().filter(|j| j.status == JobStatus::Success || matches!(j.status, JobStatus::EmbeddedExists(_))).count();
                
                info!("Download session completed: {} successful, {} failed", success_count, failed_count);
                self.status = format!("Subliminal jobs completed: {} successful, {} failed", success_count, failed_count);
                self.is_downloading = false;
            } else {
                // Update status while downloading
                if running_count > 0 {
                    self.status = format!("Downloading: {} completed, {} running, {} pending", 
                        success_count, running_count, self.total_downloads - success_count - running_count);
                }
            }
        }
    }

    /// Handle the one-time startup dependency check result
    pub fn poll_init_check(&mut self) {
        if !self.checking_deps { return; }

        let result = match self.init_check_receiver.as_ref().and_then(|rx| rx.try_recv().ok()) {
            Some(r) => r,
            None => return,
        };
        self.init_check_receiver = None;
        self.checking_deps = false;

        self.python_version = result.python_version;
        self.python_installed = self.python_version.is_some();
        self.pipx_installed = result.pipx_installed;
        self.pipx_version = result.pipx_version;
        self.subliminal_installed = result.subliminal_installed;
        self.subliminal_version = result.subliminal_version;

        // Always ensure Python Scripts folders are in PATH when Python is present
        if self.python_installed {
            if let Err(e) = PythonManager::add_scripts_to_path() {
                warn!("Failed to add Python Scripts to PATH: {}", e);
            }
            if let Err(e) = PythonManager::refresh_environment() {
                warn!("Failed to refresh environment: {}", e);
            }
        }

        // Auto-install subliminal if python (and pipx on Linux) are ready
        if self.python_installed && self.pipx_installed && !self.subliminal_installed {
            info!("Starting automatic Subliminal installation");
            self.installing_subliminal = true;
            self.status = "Python and pipx detected. Installing Subliminal...".to_string();
            let result_ptr = self.subliminal_install_result.clone();
            thread::spawn(move || {
                let success = PythonManager::install_subliminal();
                let result = if success {
                    PythonManager::add_scripts_to_path().map_err(|e| format!("Subliminal installed, but failed to update PATH: {}", e))
                } else {
                    Err("pipx/pip install failed".to_string())
                };
                *result_ptr.lock().unwrap() = Some(result);
            });
        } else {
            self.status = "Scanning will start automatically when a folder is selected".to_string();
        }

        // Start background polling thread to pick up later dependency changes
        let (tx, rx) = mpsc::channel();
        let bg_shutdown = Arc::clone(&self.shutdown_flag);
        let bg = thread::spawn(move || {
            loop {
                if bg_shutdown.load(Ordering::Relaxed) { return; }

                #[cfg(any(windows, target_os = "macos"))]
                {
                    let sub = PythonManager::is_subliminal_installed();
                    if tx.send((true, sub)).is_err() { break; }
                    if sub { break; }
                }
                #[cfg(target_os = "linux")]
                {
                    let pipx = PythonManager::_pipx_available();
                    let sub = if pipx { PythonManager::is_subliminal_installed() } else { false };
                    if tx.send((pipx, sub)).is_err() { break; }
                    if pipx && sub { break; }
                }

                // Sleep in short intervals so we notice shutdown quickly
                for _ in 0..50 {
                    if bg_shutdown.load(Ordering::Relaxed) { return; }
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        });
        self.background_check_handle = Some(bg);
        self.background_check_receiver = Some(rx);
    }

    /// Refresh installation status using background thread results
    pub fn refresh_installation_status(&mut self) {
        let mut last_status = None;
        if let Some(receiver) = &self.background_check_receiver {
            while let Ok(status) = receiver.try_recv() {
                last_status = Some(status);
            }
        }
        if let Some((_pipx_available, subliminal_installed)) = last_status {
            let _old_pipx = self.pipx_installed;
            let old_subliminal = self.subliminal_installed;

            #[cfg(target_os = "linux")]
            {
                self.pipx_installed = _pipx_available;
            }
            #[cfg(any(windows, target_os = "macos"))]
            {
                self.pipx_installed = true;
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
                    let result_ptr = self.subliminal_install_result.clone();

                    std::thread::spawn(move || {
                        let success = PythonManager::install_subliminal();
                        let result = if success {
                            match PythonManager::add_scripts_to_path() {
                                Ok(_) => Ok(()),
                                Err(e) => Err(format!("Subliminal installed, but failed to update PATH: {}", e)),
                            }
                        } else {
                            Err("pipx/pip install failed".to_string())
                        };
                        *result_ptr.lock().unwrap() = Some(result);
                    });
                }
            }

            // If subliminal became available, update status
            if !old_subliminal && self.subliminal_installed {
                info!("Subliminal became available");
                self.status = "All dependencies installed. Ready to download subtitles.".to_string();
            }
            
            // Stop background checking and free resources
            #[cfg(any(windows, target_os = "macos"))]
            {
                if self.subliminal_installed {
                    self.shutdown_flag.store(true, Ordering::Relaxed);
                    self.background_check_handle = None;
                    self.background_check_receiver = None;
                }
            }
            
            #[cfg(target_os = "linux")]
            {
                if self.pipx_installed && self.subliminal_installed {
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
            if let Some(result) = self.python_install_result.lock().unwrap().take() {
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
                        self.python_version = PythonManager::get_version();
                        self.python_installed = self.python_version.is_some();
                        self.status = "Python installed successfully. Installing Subliminal...".to_string();
                        self.subliminal_installed = PythonManager::is_subliminal_installed();

                        // Start installing subliminal automatically
                        self.installing_subliminal = true;
                        let result_ptr = self.subliminal_install_result.clone();
                        std::thread::spawn(move || {
                            let success = PythonManager::install_subliminal();
                            let result = if success {
                                match PythonManager::add_scripts_to_path() {
                                    Ok(_) => Ok(()),
                                    Err(e) => Err(format!("Subliminal installed, but failed to update PATH: {}", e)),
                                }
                            } else {
                                Err("pip install failed".to_string())
                            };
                            *result_ptr.lock().unwrap() = Some(result);
                        });
                    }
                    Err(e) => {
                        error!("Python installation failed: {}", e);
                        self.status = format!("Python install failed: {}", e);
                    }
                }
            }
        }

        if self.installing_subliminal {
            if let Some(result) = self.subliminal_install_result.lock().unwrap().take() {
                self.installing_subliminal = false;
                match result {
                    Ok(_) => {
                        info!("Subliminal installation completed successfully");
                        // Refresh environment to pick up new subliminal installation
                        if let Err(e) = PythonManager::refresh_environment() {
                            error!("Failed to refresh environment: {}", e);
                        }
                        
                        self.subliminal_installed = true;
                        self.subliminal_version = PythonManager::get_subliminal_version();
                        self.status = "Subliminal installed.".to_string();
                    }
                    Err(e) => {
                        error!("Subliminal installation failed: {}", e);
                        self.status = format!("Subliminal install failed: {}", e);
                    }
                }
            }
        }
    }

    /// Poll for version check results
    pub fn poll_version_check(&mut self) {
        if self.version_checked { return; }
        let lock = VERSION_PTR.lock().unwrap();
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
                .split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()
        };
        let c = parse(current);
        let l = parse(latest);
        for (a, b) in c.iter().zip(l.iter()) {
            if a < b { return true; }
            if a > b { return false; }
        }
        c.len() < l.len() // e.g. 1.0 < 1.0.1
    }

    // Getters for GUI access
    pub fn is_installing_python(&self) -> bool { self.installing_python }
    pub fn is_installing_subliminal(&self) -> bool { self.installing_subliminal }
    pub fn is_subliminal_installed(&self) -> bool { self.subliminal_installed }
    pub fn is_python_installed(&self) -> bool { self.python_installed }
    pub fn is_pipx_installed(&self) -> bool { self.pipx_installed }
    pub fn get_python_version(&self) -> Option<&String> { self.python_version.as_ref() }
    pub fn get_pipx_version(&self) -> Option<&String> { self.pipx_version.as_ref() }
    pub fn get_subliminal_version(&self) -> Option<&String> { self.subliminal_version.as_ref() }
    pub fn get_status(&self) -> &str { &self.status }
    pub fn get_folder_path(&self) -> &str { &self.folder_path }
    pub fn is_scanning(&self) -> bool { self.scanning }
    pub fn is_downloading(&self) -> bool { self.downloading }
    pub fn get_downloads_completed(&self) -> usize { self.downloads_completed }
    pub fn get_total_downloads(&self) -> usize { self.total_downloads }
    pub fn get_cached_jobs(&self) -> &Vec<DownloadJob> { &self.cached_jobs }
    pub fn get_latest_version(&self) -> Option<&String> { self.latest_version.as_ref() }
    pub fn get_version_check_error(&self) -> Option<&String> { self.version_check_error.as_ref() }
    pub fn is_version_checked(&self) -> bool { self.version_checked }
    pub fn is_pipx_copied(&self) -> bool { self.pipx_copied }
    pub fn get_pipx_copy_time(&self) -> Option<std::time::Instant> { self.pipx_copy_time }

    // Setters for GUI access
    pub fn set_installing_python(&mut self, installing: bool) { self.installing_python = installing; }
    pub fn set_python_install_result(&mut self, result: Arc<Mutex<Option<Result<(), String>>>>) { self.python_install_result = result; }
    pub fn set_folder_path(&mut self, path: String) { self.folder_path = path; }
    pub fn set_pipx_copied(&mut self, copied: bool) { self.pipx_copied = copied; }
    pub fn set_pipx_copy_time(&mut self, time: Option<std::time::Instant>) { self.pipx_copy_time = time; }
    pub fn set_keep_dropdown_open(&mut self, open: bool) { self.keep_dropdown_open = open; }
    pub fn get_keep_dropdown_open(&self) -> bool { self.keep_dropdown_open }

    // Mutable access to settings
    pub fn get_selected_languages_mut(&mut self) -> &mut Vec<String> { &mut self.selected_languages }
    pub fn get_force_download_mut(&mut self) -> &mut bool { &mut self.force_download }
    pub fn get_overwrite_existing(&self) -> bool { self.overwrite_existing }
    pub fn get_overwrite_existing_mut(&mut self) -> &mut bool { &mut self.overwrite_existing }
    pub fn get_ignore_local_extras(&self) -> bool { self.ignore_local_extras }
    pub fn get_ignore_local_extras_mut(&mut self) -> &mut bool { &mut self.ignore_local_extras }
    pub fn get_ignored_extra_folders(&self) -> usize { self.ignored_extra_folders }
    pub fn get_concurrent_downloads_mut(&mut self) -> &mut usize { &mut self.concurrent_downloads }
    pub fn get_scan_done_receiver_mut(&mut self) -> &mut Option<Receiver<usize>> { &mut self.scan_done_receiver }
    pub fn get_shutdown_flag(&self) -> &Arc<AtomicBool> { &self.shutdown_flag }
    pub fn get_background_check_handle_mut(&mut self) -> &mut Option<thread::JoinHandle<()>> { &mut self.background_check_handle }

    /// Start Python installation in a background thread (Windows only)
    #[cfg(windows)]
    pub fn start_python_install(&mut self) {
        if self.installing_python {
            return; // Already installing
        }
        self.installing_python = true;
        self.status = "Installing Python... Check your taskbar for a UAC prompt (shield icon)".to_string();
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
            *result_ptr.lock().unwrap() = Some(result);
        });
    }
} 