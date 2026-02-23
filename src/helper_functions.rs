//! Common utility functions and validation helpers
//! 
//! This module provides utility functions for file operations, string formatting,
//! progress tracking, and input validation used throughout the application.

use std::path::Path;
use crate::config::{VIDEO_EXTENSIONS, MAX_CONCURRENT_DOWNLOADS};

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
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len - 3])
        }
    }

    /// Check if a path is a video file based on its extension
    pub fn is_video_file(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| VIDEO_EXTENSIONS.iter().any(|&v| v.eq_ignore_ascii_case(ext)))
            .unwrap_or(false)
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
            let canonical = path.parent().ok_or("No parent folder")?.canonicalize().map_err(|e| e.to_string())?;
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
                .arg(canonical)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("open -R failed: {:?}", status));
            }
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