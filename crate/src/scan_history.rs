//! Persistent record of successful downloads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::subtitle_utils::SubtitleUtils;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanRecord {
    pub size: u64,
    pub mtime_secs: u64,
    #[serde(default)]
    pub mtime_nanos: Option<u32>,
    #[serde(default)]
    pub file_id: Option<u128>,
    pub langs: Vec<String>,
    #[serde(default)]
    pub contains_hearing_impaired: bool,
    #[serde(default)]
    pub hearing_impaired_langs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScanHistory {
    #[serde(default)]
    pub records: HashMap<String, ScanRecord>,
    #[serde(skip)]
    dirty: bool,
}

impl ScanHistory {
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
            Ok(exe_dir.join("rustitles_scan_history.json"))
        }

        #[cfg(not(windows))]
        {
            let xdg_dirs = xdg::BaseDirectories::new();
            if let Some(config_dir) = xdg_dirs.get_config_home() {
                let app_dir = config_dir.join("rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("scan_history.json"))
            } else {
                let home_dir = dirs::home_dir().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Failed to get home directory",
                    )
                })?;
                let app_dir = home_dir.join(".rustitles");
                std::fs::create_dir_all(&app_dir)?;
                Ok(app_dir.join("scan_history.json"))
            }
        }
    }

    pub fn load() -> Self {
        match Self::get_path() {
            Ok(path) => match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<ScanHistory>(&content) {
                    Ok(history) => {
                        crate::info!("Scan history loaded from {}", path.display());
                        history
                    }
                    Err(e) => {
                        crate::warn!("Failed to parse scan history: {}. Starting empty.", e);
                        let bak = path.with_extension("json.bak");
                        let _ = std::fs::rename(&path, &bak);
                        ScanHistory::default()
                    }
                },
                Err(_) => ScanHistory::default(),
            },
            Err(e) => {
                crate::warn!("Failed to get scan history path: {}. Starting empty.", e);
                ScanHistory::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path =
            Self::get_path().map_err(|e| format!("Failed to get scan history path: {}", e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize scan history: {}", e))?;
        crate::helper_functions::Utils::write_atomic(&path, json.as_bytes())
            .map_err(|e| format!("Failed to commit scan history: {}", e))?;
        Ok(())
    }

    pub fn save_if_dirty(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        self.save()?;
        self.dirty = false;
        Ok(())
    }

    pub fn file_identity(path: &Path) -> Option<(u64, u64)> {
        let (size, mtime_secs, _, _) = Self::precise_file_identity(path)?;
        Some((size, mtime_secs))
    }

    fn precise_file_identity(path: &Path) -> Option<(u64, u64, u32, u128)> {
        #[cfg(windows)]
        {
            let file = std::fs::File::open(path).ok()?;
            let meta = file.metadata().ok()?;
            let size = meta.len();
            let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
            Some((
                size,
                modified.as_secs(),
                modified.subsec_nanos(),
                file_id_from_handle(&file)?,
            ))
        }
        #[cfg(not(windows))]
        {
            let meta = std::fs::metadata(path).ok()?;
            let size = meta.len();
            let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
            Some((
                size,
                modified.as_secs(),
                modified.subsec_nanos(),
                file_id(path, &meta)?,
            ))
        }
    }

    pub fn should_skip(
        &self,
        path: &Path,
        selected_languages: &[String],
        exclude_hearing_impaired: bool,
    ) -> bool {
        let key = path.to_string_lossy().to_string();
        let Some(record) = self.records.get(&key) else {
            return false;
        };
        // Check cheap fields before the full identity.
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        let size = meta.len();
        if record.size != size {
            return false;
        }
        let Ok(modified) = meta.modified().and_then(|t| {
            t.duration_since(UNIX_EPOCH)
                .map_err(|_| std::io::Error::other("time before epoch"))
        }) else {
            return false;
        };
        if record.mtime_secs != modified.as_secs()
            || record.mtime_nanos != Some(modified.subsec_nanos())
        {
            return false;
        }
        // Check the full identity after the cheap fields match.
        let Some((_, _, _, file_id)) = Self::precise_file_identity(path) else {
            return false;
        };
        if record.file_id != Some(file_id) {
            return false;
        }
        record_allows_skip(record, selected_languages, exclude_hearing_impaired)
    }

    pub fn covers_languages(recorded: &[String], selected: &[String]) -> bool {
        if selected.is_empty() {
            return false;
        }
        selected
            .iter()
            .all(|lang| recorded.iter().any(|stored| stored == lang))
    }

    pub fn covered_langs(
        video_path: &Path,
        subtitle_paths: &[PathBuf],
        requested: &[String],
    ) -> Vec<String> {
        requested
            .iter()
            .filter(|lang| {
                subtitle_paths
                    .iter()
                    .any(|path| SubtitleUtils::matches_language(video_path, path, lang))
            })
            .cloned()
            .collect()
    }

    pub fn hearing_impaired_langs(
        video_path: &Path,
        subtitle_paths: &[PathBuf],
        requested: &[String],
    ) -> Vec<String> {
        let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };
        let generic_is_hearing_impaired = subtitle_paths.iter().any(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(stem))
                && SubtitleUtils::is_hearing_impaired_path(video_path, path)
        });

        requested
            .iter()
            .filter(|lang| {
                let matching = subtitle_paths
                    .iter()
                    .filter(|path| SubtitleUtils::matches_language(video_path, path, lang));
                let mut has_regular = false;
                let mut has_hearing_impaired = generic_is_hearing_impaired;
                for path in matching {
                    if SubtitleUtils::is_hearing_impaired_path(video_path, path) {
                        has_hearing_impaired = true;
                    } else {
                        has_regular = true;
                    }
                }
                has_hearing_impaired && !has_regular
            })
            .cloned()
            .collect()
    }

    pub fn record_success(
        &mut self,
        path: &Path,
        langs: &[String],
        hearing_impaired_langs: &[String],
    ) {
        let Some((size, mtime_secs, mtime_nanos, file_id)) = Self::precise_file_identity(path)
        else {
            crate::warn!("Could not read identity for {}", path.display());
            return;
        };
        let key = path.to_string_lossy().to_string();
        let record = merge_record(
            self.records.get(&key),
            size,
            mtime_secs,
            mtime_nanos,
            file_id,
            langs,
            hearing_impaired_langs,
        );
        self.records.insert(key, record);
        self.dirty = true;
    }
}

fn merge_record(
    existing: Option<&ScanRecord>,
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    file_id: u128,
    langs: &[String],
    hearing_impaired_langs: &[String],
) -> ScanRecord {
    let same_identity = existing.is_some_and(|record| {
        record.size == size
            && record.mtime_secs == mtime_secs
            && ((record.mtime_nanos == Some(mtime_nanos) && record.file_id == Some(file_id))
                || (record.mtime_nanos.is_none() && record.file_id.is_none()))
    });
    let mut merged = if same_identity {
        existing
            .map(|record| record.langs.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    for lang in langs {
        if !merged.iter().any(|stored| stored == lang) {
            merged.push(lang.clone());
        }
    }
    let mut merged_hearing_impaired = if same_identity {
        existing
            .map(|record| {
                if record.hearing_impaired_langs.is_empty() && record.contains_hearing_impaired {
                    record.langs.clone()
                } else {
                    record.hearing_impaired_langs.clone()
                }
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    for lang in langs {
        if hearing_impaired_langs.iter().any(|stored| stored == lang) {
            if !merged_hearing_impaired.iter().any(|stored| stored == lang) {
                merged_hearing_impaired.push(lang.clone());
            }
        } else {
            merged_hearing_impaired.retain(|stored| stored != lang);
        }
    }
    let contains_hearing_impaired = !merged_hearing_impaired.is_empty();
    ScanRecord {
        size,
        mtime_secs,
        mtime_nanos: Some(mtime_nanos),
        file_id: Some(file_id),
        langs: merged,
        contains_hearing_impaired,
        hearing_impaired_langs: merged_hearing_impaired,
    }
}

fn record_allows_skip(
    record: &ScanRecord,
    selected_languages: &[String],
    exclude_hearing_impaired: bool,
) -> bool {
    let contains_excluded_hearing_impaired = if record.hearing_impaired_langs.is_empty() {
        record.contains_hearing_impaired
    } else {
        selected_languages.iter().any(|lang| {
            record
                .hearing_impaired_langs
                .iter()
                .any(|stored| stored == lang)
        })
    };
    (!exclude_hearing_impaired || !contains_excluded_hearing_impaired)
        && ScanHistory::covers_languages(&record.langs, selected_languages)
}

#[cfg(windows)]
fn file_id_from_handle(file: &std::fs::File) -> Option<u128> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info).ok()?;
    }
    let index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    Some(((info.dwVolumeSerialNumber as u128) << 64) | index as u128)
}

#[cfg(unix)]
fn file_id(_path: &Path, meta: &std::fs::Metadata) -> Option<u128> {
    use std::os::unix::fs::MetadataExt;

    Some(((meta.dev() as u128) << 64) | meta.ino() as u128)
}

#[cfg(not(any(unix, windows)))]
fn file_id(_path: &Path, _meta: &std::fs::Metadata) -> Option<u128> {
    None
}

#[cfg(test)]
mod tests {
    use super::{merge_record, record_allows_skip, ScanHistory, ScanRecord};
    use std::path::{Path, PathBuf};

    #[test]
    fn skip_requires_identity_and_language_subset() {
        let mut history = ScanHistory::default();
        history.records.insert(
            "C:\\media\\Movie.mkv".to_string(),
            ScanRecord {
                size: 100,
                mtime_secs: 50,
                mtime_nanos: Some(0),
                file_id: Some(1),
                langs: vec!["en".to_string()],
                contains_hearing_impaired: false,
                hearing_impaired_langs: Vec::new(),
            },
        );
        assert!(ScanHistory::covers_languages(
            &["en".to_string()],
            &["en".to_string()]
        ));
        assert!(!ScanHistory::covers_languages(
            &["en".to_string()],
            &["en".to_string(), "fr".to_string()]
        ));
        assert!(ScanHistory::covers_languages(
            &["en".to_string(), "fr".to_string()],
            &["fr".to_string()]
        ));

        let record = history.records.get("C:\\media\\Movie.mkv").unwrap();
        assert_eq!(record.size, 100);
        assert_ne!(record.size, 101);
        assert_ne!(record.mtime_secs, 51);
    }

    #[test]
    fn covered_langs_reads_sidecars() {
        let video = Path::new("C:/media/Movie.mkv");
        let subs = vec![
            PathBuf::from("C:/media/Movie.en.srt"),
            PathBuf::from("C:/media/Movie.fr.sdh.srt"),
        ];
        let langs = ScanHistory::covered_langs(
            video,
            &subs,
            &["en".to_string(), "de".to_string(), "fr".to_string()],
        );
        assert_eq!(langs, vec!["en".to_string(), "fr".to_string()]);
        let hearing_impaired = ScanHistory::hearing_impaired_langs(
            video,
            &subs,
            &["en".to_string(), "de".to_string(), "fr".to_string()],
        );
        assert_eq!(hearing_impaired, vec!["fr".to_string()]);
    }

    #[test]
    fn failed_jobs_do_not_write_records() {
        let history = ScanHistory::default();
        assert!(history.records.is_empty());
    }

    #[test]
    fn changed_file_identity_discards_old_languages() {
        let old = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(0),
            file_id: Some(1),
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: false,
            hearing_impaired_langs: Vec::new(),
        };

        let merged = merge_record(Some(&old), 101, 51, 0, 2, &["en".to_string()], &[]);

        assert_eq!(merged.langs, vec!["en"]);
    }

    #[test]
    fn hearing_impaired_history_is_not_skipped_when_excluded() {
        let record = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(0),
            file_id: Some(1),
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: true,
            hearing_impaired_langs: Vec::new(),
        };

        assert!(!record_allows_skip(&record, &["en".to_string()], true));
        assert!(record_allows_skip(&record, &["en".to_string()], false));
    }

    #[test]
    fn hearing_impaired_history_updates_per_language() {
        let old = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(0),
            file_id: Some(1),
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: true,
            hearing_impaired_langs: vec!["en".to_string(), "fr".to_string()],
        };

        let merged = merge_record(Some(&old), 100, 50, 0, 1, &["en".to_string()], &[]);

        assert_eq!(merged.hearing_impaired_langs, vec!["fr"]);
        assert!(record_allows_skip(&merged, &["en".to_string()], true));
        assert!(!record_allows_skip(&merged, &["fr".to_string()], true));
    }

    #[test]
    fn legacy_hearing_impaired_flag_clears_after_full_refresh() {
        let old = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(0),
            file_id: Some(1),
            langs: vec!["en".to_string()],
            contains_hearing_impaired: true,
            hearing_impaired_langs: Vec::new(),
        };

        let merged = merge_record(Some(&old), 100, 50, 0, 1, &["en".to_string()], &[]);

        assert!(!merged.contains_hearing_impaired);
        assert!(record_allows_skip(&merged, &["en".to_string()], true));
    }

    #[test]
    fn legacy_hearing_impaired_flag_migrates_by_refreshed_language() {
        let old = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(0),
            file_id: Some(1),
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: true,
            hearing_impaired_langs: Vec::new(),
        };

        let merged = merge_record(Some(&old), 100, 50, 0, 1, &["en".to_string()], &[]);

        assert_eq!(merged.hearing_impaired_langs, vec!["fr"]);
        assert!(record_allows_skip(&merged, &["en".to_string()], true));
        assert!(!record_allows_skip(&merged, &["fr".to_string()], true));
    }

    #[test]
    fn legacy_identity_preserves_history_during_refresh() {
        let old = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: None,
            file_id: None,
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: true,
            hearing_impaired_langs: Vec::new(),
        };

        let merged = merge_record(Some(&old), 100, 50, 11, 2, &["en".to_string()], &[]);

        assert_eq!(merged.langs, vec!["en", "fr"]);
        assert_eq!(merged.hearing_impaired_langs, vec!["fr"]);
        assert_eq!(merged.mtime_nanos, Some(11));
        assert_eq!(merged.file_id, Some(2));
    }

    #[test]
    fn precise_identity_requires_subsecond_timestamp() {
        let record = ScanRecord {
            size: 100,
            mtime_secs: 50,
            mtime_nanos: Some(10),
            file_id: Some(1),
            langs: vec!["en".to_string(), "fr".to_string()],
            contains_hearing_impaired: false,
            hearing_impaired_langs: Vec::new(),
        };

        assert!(record_allows_skip(&record, &["en".to_string()], false));
        let replaced = merge_record(Some(&record), 100, 50, 11, 1, &["en".to_string()], &[]);
        assert_eq!(replaced.langs, vec!["en"]);
        assert_eq!(record.mtime_nanos, Some(10));
        assert_ne!(record.mtime_nanos, replaced.mtime_nanos);
        let replaced_file = merge_record(Some(&record), 100, 50, 10, 2, &["en".to_string()], &[]);
        assert_eq!(replaced_file.langs, vec!["en"]);
        assert_ne!(record.file_id, replaced_file.file_id);
    }
}
