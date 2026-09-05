//! Application settings and persistence.

use crate::config::{DEFAULT_CONCURRENT_DOWNLOADS, MAX_CONCURRENT_DOWNLOADS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Settings persisted between sessions.
#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default = "default_selected_languages")]
    pub selected_languages: Vec<String>,
    #[serde(default = "default_true")]
    pub skip_scanned_media: bool,
    #[serde(default = "default_true")]
    pub force_download: bool,
    #[serde(default = "default_true")]
    pub overwrite_existing: bool,
    #[serde(default = "default_concurrent_downloads")]
    pub concurrent_downloads: usize,
    #[serde(default = "default_true")]
    pub ignore_local_extras: bool,
    #[serde(default = "default_providers")]
    pub providers: Vec<String>,
    #[serde(default = "default_refiners")]
    pub refiners: Vec<String>,
    #[serde(default)]
    pub minimum_score: u8,
    #[serde(default)]
    pub exclude_hearing_impaired: bool,
    #[serde(default = "default_true")]
    pub matching_options_open: bool,
    #[serde(default = "default_max_pages")]
    pub opensubtitles_max_pages: Option<u8>,
    #[serde(default)]
    pub opensubtitlescom_username: String,
    #[serde(default)]
    pub opensubtitlescom_password: String,
    #[serde(default)]
    pub opensubtitlescom_apikey: String,
}

fn default_true() -> bool {
    true
}

fn default_selected_languages() -> Vec<String> {
    vec!["en".to_string()]
}

fn default_concurrent_downloads() -> usize {
    DEFAULT_CONCURRENT_DOWNLOADS
}

fn default_refiners() -> Vec<String> {
    vec![
        "hash".to_string(),
        "metadata".to_string(),
        "tmdb".to_string(),
        "tvdb".to_string(),
    ]
}

fn default_max_pages() -> Option<u8> {
    Some(3)
}

fn default_providers() -> Vec<String> {
    vec![
        "addic7ed".to_string(),
        "gestdown".to_string(),
        "napiprojekt".to_string(),
        "opensubtitles".to_string(),
        "opensubtitlescom".to_string(),
        "podnapisi".to_string(),
        "tvsubtitles".to_string(),
    ]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_languages: default_selected_languages(),
            skip_scanned_media: true,
            force_download: true,
            overwrite_existing: true,
            concurrent_downloads: DEFAULT_CONCURRENT_DOWNLOADS,
            ignore_local_extras: true,
            providers: default_providers(),
            refiners: default_refiners(),
            minimum_score: 0,
            exclude_hearing_impaired: false,
            matching_options_open: true,
            opensubtitles_max_pages: Some(3),
            opensubtitlescom_username: String::new(),
            opensubtitlescom_password: String::new(),
            opensubtitlescom_apikey: String::new(),
        }
    }
}

impl Settings {
    pub fn normalize(mut self) -> Self {
        const PROVIDERS: &[&str] = &[
            "addic7ed",
            "gestdown",
            "napiprojekt",
            "opensubtitles",
            "opensubtitlescom",
            "podnapisi",
            "tvsubtitles",
        ];
        const REFINERS: &[&str] = &["hash", "metadata", "tmdb", "tvdb"];

        self.providers
            .retain(|provider| PROVIDERS.contains(&provider.as_str()));
        self.refiners
            .retain(|refiner| REFINERS.contains(&refiner.as_str()));
        if !(1..=MAX_CONCURRENT_DOWNLOADS).contains(&self.concurrent_downloads) {
            self.concurrent_downloads = DEFAULT_CONCURRENT_DOWNLOADS;
        }
        if self.minimum_score > 100 {
            self.minimum_score = 0;
        }
        if let Some(pages) = self.opensubtitles_max_pages {
            if !(1..=20).contains(&pages) {
                self.opensubtitles_max_pages = Some(3);
            }
        }
        self
    }

    /// Explain where credentials are stored.
    pub fn credentials_storage_blurb() -> &'static str {
        #[cfg(windows)]
        {
            "Stored locally in rustitles_settings.json (next to the app) and synced to subliminal.toml for Subliminal. No key is baked into the app."
        }
        #[cfg(not(windows))]
        {
            "Stored locally in settings.json in your rustitles config folder (XDG on Linux/macOS) and synced to subliminal.toml for Subliminal. No key is baked into the app."
        }
    }

    /// Get the settings path.
    pub fn get_path() -> std::io::Result<PathBuf> {
        #[cfg(windows)]
        {
            let exe_path = std::env::current_exe()?;
            let exe_dir = exe_path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Failed to get executable directory",
                )
            })?;
            Ok(exe_dir.join("rustitles_settings.json"))
        }

        #[cfg(not(windows))]
        {
            // Use the XDG config directory.
            let xdg_dirs = xdg::BaseDirectories::new();
            if let Some(config_dir) = xdg_dirs.get_config_home() {
                let app_dir = config_dir.join("rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("settings.json"))
            } else {
                let home_dir = dirs::home_dir().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Failed to get home directory",
                    )
                })?;
                let app_dir = home_dir.join(".rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("settings.json"))
            }
        }
    }

    /// Load settings from disk, or use defaults.
    pub fn load() -> Self {
        match Self::get_path() {
            Ok(path) => match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<Settings>(&content) {
                    Ok(settings) => {
                        crate::info!("Settings loaded from {}", path.display());
                        settings.normalize()
                    }
                    Err(e) => {
                        crate::warn!("Failed to parse settings file: {}. Using defaults.", e);
                        let bak = path.with_extension("json.bak");
                        let _ = std::fs::rename(&path, &bak);
                        Settings::default()
                    }
                },
                Err(e) => {
                    crate::debug!(
                        "Settings file not found or unreadable: {}. Using defaults.",
                        e
                    );
                    Settings::default()
                }
            },
            Err(e) => {
                crate::warn!("Failed to get settings path: {}. Using defaults.", e);
                Settings::default()
            }
        }
    }

    /// Save settings atomically.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_path().map_err(|e| format!("Failed to get settings path: {}", e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        crate::helper_functions::Utils::write_atomic(&path, json.as_bytes())
            .map_err(|e| format!("Failed to commit settings file: {}", e))?;
        crate::debug!("Settings saved to {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;
    use crate::config::DEFAULT_CONCURRENT_DOWNLOADS;

    #[test]
    fn normalize_discards_invalid_matching_settings() {
        let settings = Settings {
            providers: vec!["opensubtitlescom".to_string(), "unknown".to_string()],
            refiners: vec!["metadata".to_string(), "unknown".to_string()],
            concurrent_downloads: 0,
            minimum_score: 101,
            ..Settings::default()
        };
        let normalized = settings.normalize();

        assert_eq!(normalized.providers, vec!["opensubtitlescom"]);
        assert_eq!(normalized.refiners, vec!["metadata"]);
        assert_eq!(
            normalized.concurrent_downloads,
            DEFAULT_CONCURRENT_DOWNLOADS
        );
        assert_eq!(normalized.minimum_score, 0);
    }

    #[test]
    fn defaults_enable_requested_controls() {
        let settings = Settings::default();

        assert_eq!(settings.selected_languages, vec!["en"]);
        assert!(settings.skip_scanned_media);
        assert!(settings.force_download);
        assert!(settings.overwrite_existing);
        assert!(settings.ignore_local_extras);
        assert_eq!(settings.providers.len(), 7);
        assert_eq!(settings.refiners, vec!["hash", "metadata", "tmdb", "tvdb"]);
        assert!(!settings.exclude_hearing_impaired);
        assert!(settings.matching_options_open);
        assert_eq!(settings.opensubtitles_max_pages, Some(3));
    }
}
