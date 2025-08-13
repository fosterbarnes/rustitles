//! Application settings and persistence management
//! 
//! This module handles loading, saving, and managing user preferences
//! and application settings that persist between sessions.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::config::DEFAULT_CONCURRENT_DOWNLOADS;

/// Application settings that persist between sessions
#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub selected_languages: Vec<String>,
    pub force_download: bool,
    pub overwrite_existing: bool,
    pub concurrent_downloads: usize,
    pub ignore_local_extras: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_languages: Vec::new(),
            force_download: false,
            overwrite_existing: false,
            concurrent_downloads: DEFAULT_CONCURRENT_DOWNLOADS,
            ignore_local_extras: false,
        }
    }
}

impl Settings {
    /// Get the path where settings are stored
    pub fn get_path() -> std::io::Result<PathBuf> {
        #[cfg(windows)]
        {
            let exe_path = std::env::current_exe()?;
            let exe_dir = exe_path.parent().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to get executable directory")
            })?;
            Ok(exe_dir.join("rustitles_settings.json"))
        }
        
        #[cfg(not(windows))]
        {
            // Use XDG config directory on Linux
            if let Ok(xdg_dirs) = xdg::BaseDirectories::new() {
                let config_dir = xdg_dirs.get_config_home();
                let app_dir = config_dir.join("rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("settings.json"))
            } else {
                // Fallback to home directory
                let home_dir = dirs::home_dir().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to get home directory")
                })?;
                let app_dir = home_dir.join(".rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("settings.json"))
            }
        }
    }

    /// Load settings from disk, falling back to defaults if file doesn't exist
    pub fn load() -> Self {
        match Self::get_path() {
            Ok(path) => {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str(&content) {
                            Ok(settings) => {
                                crate::info!("Settings loaded from {}", path.display());
                                settings
                            }
                            Err(e) => {
                                crate::warn!("Failed to parse settings file: {}. Using defaults.", e);
                                Settings::default()
                            }
                        }
                    }
                    Err(e) => {
                        crate::debug!("Settings file not found or unreadable: {}. Using defaults.", e);
                        Settings::default()
                    }
                }
            }
            Err(e) => {
                crate::warn!("Failed to get settings path: {}. Using defaults.", e);
                Settings::default()
            }
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_path().map_err(|e| format!("Failed to get settings path: {}", e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write settings file: {}", e))?;
        crate::debug!("Settings saved to {}", path.display());
        Ok(())
    }
} 