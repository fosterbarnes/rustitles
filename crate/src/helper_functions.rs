//! Common utility functions and validation helpers
//!
//! This module provides utility functions for file operations, string formatting,
//! progress tracking, and input validation used throughout the application.

use crate::config::{EXTRAS_FOLDER_NAMES, MAX_CONCURRENT_DOWNLOADS, VIDEO_EXTENSIONS};
use std::path::Path;

/// Common utility functions used throughout the application
pub struct Utils;

impl Utils {
    /// Safely get the file name from a path, returning a default if not available
    pub fn get_file_name(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }

    /// Truncate a string to a maximum length, adding ellipsis if needed
    pub fn truncate_string(s: &str, max_len: usize) -> String {
        if s.chars().count() <= max_len {
            s.to_string()
        } else if max_len <= 3 {
            s.chars().take(max_len).collect()
        } else {
            let prefix: String = s.chars().take(max_len - 3).collect();
            format!("{prefix}...")
        }
    }

    /// Check if a path is a video file based on its extension
    pub fn is_video_file(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                VIDEO_EXTENSIONS
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(ext))
            })
    }

    /// True when a directory name is a Plex local-extras folder (case-insensitive)
    pub fn is_plex_extras_folder(name: &str) -> bool {
        EXTRAS_FOLDER_NAMES
            .iter()
            .any(|folder| folder.eq_ignore_ascii_case(name))
    }

    /// Create a progress percentage string
    pub fn format_progress(current: usize, total: usize) -> String {
        if total == 0 {
            "0%".to_string()
        } else {
            let percentage = (current as f32 / total as f32 * 100.0) as usize;
            format!("{}%", percentage)
        }
    }

    /// Get the path to the log file
    pub fn get_log_path() -> Result<std::path::PathBuf, String> {
        #[cfg(windows)]
        {
            let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe_dir = exe_path
                .parent()
                .ok_or("Failed to get executable directory")?;
            Ok(exe_dir.join("rustitles_log.txt"))
        }
        #[cfg(not(windows))]
        {
            let xdg_dirs = xdg::BaseDirectories::new();
            if let Some(cache_dir) = xdg_dirs.get_cache_home() {
                Ok(cache_dir.join("rustitles").join("rustitles.log"))
            } else {
                let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
                Ok(home_dir.join(".rustitles").join("rustitles.log"))
            }
        }
    }

    /// Open the file browser with the log file highlighted
    pub fn open_log_file() -> Result<(), String> {
        let log_path = Self::get_log_path()?;
        if !log_path.exists() {
            return Err("Log file does not exist yet".to_string());
        }
        Self::open_containing_folder(&log_path)
    }

    /// Open the containing folder of a file in the system's file explorer
    pub fn open_containing_folder(path: &Path) -> Result<(), String> {
        let _folder = path.parent().ok_or("No parent folder")?;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let canonical = path.canonicalize().map_err(|e| e.to_string())?;
            let path_str = canonical.to_string_lossy().replace("/", "\\");
            let mut cmd = std::process::Command::new("explorer.exe");
            cmd.arg("/select,").arg(&path_str);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            cmd.spawn().map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "linux")]
        {
            let canonical = path
                .parent()
                .ok_or("No parent folder")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let status = std::process::Command::new("xdg-open")
                .arg(canonical)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("xdg-open failed: {:?}", status));
            }
        }
        #[cfg(target_os = "macos")]
        {
            let canonical = path.canonicalize().map_err(|e| e.to_string())?;
            let status = std::process::Command::new("open")
                .arg("-R")
                .arg(&canonical)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("open -R failed: {:?}", status));
            }
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            return Err("Open folder not supported on this OS".to_string());
        }
        Ok(())
    }
}

/// Input validation utilities
pub struct Validation;

impl Validation {
    /// Validate that a folder path exists and is a directory
    pub fn is_valid_folder(path: &str) -> bool {
        if path.is_empty() {
            return false;
        }

        let path = Path::new(path);
        path.exists() && path.is_dir()
    }

    /// Validate concurrent downloads setting
    pub fn is_valid_concurrent_downloads(value: usize) -> bool {
        value > 0 && value <= MAX_CONCURRENT_DOWNLOADS
    }
}

#[cfg(test)]
mod tests {
    use super::Utils;
    use std::path::Path;

    #[test]
    fn truncate_string_respects_character_boundaries() {
        assert_eq!(Utils::truncate_string("字幕文件名称", 5), "字幕...");
        assert_eq!(Utils::truncate_string("abcdef", 3), "abc");
        assert_eq!(Utils::truncate_string("abcdef", 0), "");
    }

    #[test]
    fn is_video_file_accepts_mkv_and_rejects_m4a() {
        assert!(Utils::is_video_file(Path::new("show.mkv")));
        assert!(Utils::is_video_file(Path::new("SHOW.MKV")));
        assert!(!Utils::is_video_file(Path::new("track.m4a")));
        assert!(!Utils::is_video_file(Path::new("audiobook.m4b")));
        assert!(!Utils::is_video_file(Path::new("ringtone.m4r")));
    }

    #[test]
    fn plex_extras_folder_match_is_case_insensitive() {
        assert!(Utils::is_plex_extras_folder("Behind The Scenes"));
        assert!(Utils::is_plex_extras_folder("behind the scenes"));
        assert!(Utils::is_plex_extras_folder("FEATURETTES"));
        assert!(!Utils::is_plex_extras_folder("Season 01"));
    }
}
