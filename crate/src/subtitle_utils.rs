//! Subtitle file utilities and language detection
//!
//! This module provides functions for finding subtitle files, detecting
//! embedded subtitles, and handling language code conversions.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Utilities for working with subtitle files and language detection
pub struct SubtitleUtils;

impl SubtitleUtils {
    /// Find all subtitle files for a video and a set of languages
    pub fn find_all_subtitle_files(video_path: &Path, langs: &[String]) -> Vec<PathBuf> {
        let folder = match video_path.parent() {
            Some(f) => f,
            None => return Vec::new(),
        };
        Self::find_all_subtitle_files_in_listing(
            video_path,
            langs,
            &Self::list_subtitle_files_in_folder(folder),
        )
    }

    /// Find all matching subtitle files from a pre-read folder listing.
    pub fn find_all_subtitle_files_in_listing(
        video_path: &Path,
        langs: &[String],
        folder_subtitles: &[PathBuf],
    ) -> Vec<PathBuf> {
        let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Vec::new(),
        };
        let mut found_subtitles = Vec::new();

        crate::debug!("Searching for subtitle files for {}", video_path.display());

        let subtitle_files =
            Self::subtitle_files_for_video_in_listing(video_path, folder_subtitles);

        // Try language-specific first
        for lang in langs {
            let expected_stem = format!("{}.{}", stem, lang);
            if let Some(path) = subtitle_files
                .iter()
                .find(|path| Self::language_specific_stem_matches(path, &expected_stem))
            {
                crate::debug!("Found language-specific subtitle: {}", path.display());
                found_subtitles.push(path.clone());
            }
        }
        // Then try generic
        if let Some(path) = subtitle_files.iter().find(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(stem))
        }) {
            crate::debug!("Found generic subtitle: {}", path.display());
            found_subtitles.push(path.clone());
        }

        if found_subtitles.is_empty() {
            crate::debug!("No subtitle files found for {}", video_path.display());
        } else {
            crate::debug!(
                "Found {} subtitle files for {}",
                found_subtitles.len(),
                video_path.display()
            );
        }

        found_subtitles
    }

    /// List subtitle files in a folder once (no per-video stem filter, unsorted)
    pub fn list_subtitle_files_in_folder(folder: &Path) -> Vec<PathBuf> {
        const EXTENSIONS: &[&str] = &["srt", "sub", "ssa", "ass", "vtt"];
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };
        entries
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry.path()),
                Err(_) => None,
            })
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| {
                        EXTENSIONS
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(ext))
                    })
            })
            .collect()
    }

    /// Filter a folder listing to sidecar files for one video (unsorted)
    pub fn subtitle_files_for_video_in_listing(
        video_path: &Path,
        folder_subtitles: &[PathBuf],
    ) -> Vec<PathBuf> {
        let stem = match video_path.file_stem().and_then(|value| value.to_str()) {
            Some(stem) => stem,
            None => return Vec::new(),
        };

        folder_subtitles
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| {
                        name.eq_ignore_ascii_case(stem) || {
                            let prefix_len = stem.len();
                            name.len() > prefix_len + 1
                                && name.is_char_boundary(prefix_len)
                                && name.as_bytes().get(prefix_len) == Some(&b'.')
                                && name[..prefix_len].eq_ignore_ascii_case(stem)
                        }
                    })
            })
            .cloned()
            .collect()
    }

    /// Match exact language sidecars and their SDH/caption variants.
    /// The caller filters those variants when exclusion is enabled.
    fn language_specific_stem_matches(path: &Path, expected_stem: &str) -> bool {
        let Some(value) = path.file_stem().and_then(|value| value.to_str()) else {
            return false;
        };
        if value.eq_ignore_ascii_case(expected_stem) {
            return true;
        }

        let prefix_len = expected_stem.len();
        if value.len() <= prefix_len + 1
            || !value.is_char_boundary(prefix_len)
            || value.as_bytes().get(prefix_len) != Some(&b'.')
            || !value[..prefix_len].eq_ignore_ascii_case(expected_stem)
        {
            return false;
        }
        Self::is_hearing_impaired_path(path)
    }

    /// Find every adjacent subtitle file belonging to a video.
    pub fn find_subtitle_files_for_video(video_path: &Path) -> Vec<PathBuf> {
        let folder = match video_path.parent() {
            Some(folder) => folder,
            None => return Vec::new(),
        };
        let mut files = Self::subtitle_files_for_video_in_listing(
            video_path,
            &Self::list_subtitle_files_in_folder(folder),
        );
        files.sort();
        files
    }

    /// Preserve existing subtitles before an overwrite using numbered backups.
    pub fn backup_subtitle_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        let mut backups = Vec::with_capacity(paths.len());
        for path in paths {
            let mut source = std::fs::File::open(path)
                .map_err(|error| format!("{}: {}", path.display(), error))?;
            let file_name = path
                .file_name()
                .ok_or_else(|| format!("subtitle path has no file name: {}", path.display()))?;
            let mut backup_name = OsString::from(file_name);
            backup_name.push(".bak");
            let mut index = 0;
            loop {
                let backup_path = if index == 0 {
                    path.with_file_name(&backup_name)
                } else {
                    let mut numbered_name = OsString::from(&backup_name);
                    numbered_name.push(format!(".{}", index));
                    path.with_file_name(numbered_name)
                };
                let mut destination = match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&backup_path)
                {
                    Ok(file) => file,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        index += 1;
                        continue;
                    }
                    Err(error) => {
                        return Err(format!(
                            "{} -> {}: {}",
                            path.display(),
                            backup_path.display(),
                            error
                        ));
                    }
                };

                if let Err(error) = std::io::copy(&mut source, &mut destination) {
                    let _ = std::fs::remove_file(&backup_path);
                    return Err(format!(
                        "{} -> {}: {}",
                        path.display(),
                        backup_path.display(),
                        error
                    ));
                }
                crate::info!(
                    "Backed up subtitle {} to {}",
                    path.display(),
                    backup_path.display()
                );
                backups.push(backup_path);
                break;
            }
        }
        Ok(backups)
    }

    /// Convert a language code to a human-readable name
    pub fn language_code_to_name(code: &str) -> &str {
        match code {
            // Regional Variants (high priority)
            "en" => "English",
            "en-us" => "English (US)",
            "en-gb" => "English (UK)",
            "fr" => "French",
            "fr-ca" => "French (Canada)",
            "es" => "Spanish",
            "es-mx" => "Spanish (Mexico)",
            "es-es" => "Spanish (Spain)",
            "de" => "German",
            "de-at" => "German (Austria)",
            "de-ch" => "German (Switzerland)",
            "it" => "Italian",
            "it-ch" => "Italian (Switzerland)",
            "pt" => "Portuguese",
            "pt-br" => "Portuguese (Brazil)",
            "pt-pt" => "Portuguese (Portugal)",
            "nl" => "Dutch",
            "nl-be" => "Dutch (Belgium)",

            // Additional European Languages
            "pl" => "Polish",
            "ru" => "Russian",
            "sr" => "Serbian",
            "sv" => "Swedish",
            "fi" => "Finnish",
            "da" => "Danish",
            "no" => "Norwegian",
            "cs" => "Czech",
            "hu" => "Hungarian",
            "ro" => "Romanian",
            "bg" => "Bulgarian",
            "hr" => "Croatian",
            "et" => "Estonian",
            "el" => "Greek",
            "is" => "Icelandic",
            "lv" => "Latvian",
            "lt" => "Lithuanian",
            "mt" => "Maltese",
            "sk" => "Slovak",
            "sl" => "Slovenian",
            "tr" => "Turkish",
            "uk" => "Ukrainian",

            // Additional Asian Languages
            "he" => "Hebrew",
            "ar" => "Arabic",
            "ja" => "Japanese",
            "ko" => "Korean",
            "zh" => "Chinese",
            "zh-cn" => "Chinese (Simplified)",
            "zh-tw" => "Chinese (Traditional)",
            "th" => "Thai",
            "vi" => "Vietnamese",
            "id" => "Indonesian",
            "ms" => "Malay",
            "fil" => "Filipino/Tagalog",
            "bn" => "Bengali",
            "hi" => "Hindi",
            "ur" => "Urdu",
            "fa" => "Persian/Farsi",

            // Additional African Languages
            "af" => "Afrikaans",
            "sw" => "Swahili",
            "zu" => "Zulu",
            "xh" => "Xhosa",

            // Additional Middle Eastern Languages
            "ku" => "Kurdish",
            "az" => "Azerbaijani",
            "ka" => "Georgian",
            "am" => "Amharic",

            // Additional Indian Subcontinent Languages
            "ta" => "Tamil",
            "te" => "Telugu",
            "kn" => "Kannada",
            "ml" => "Malayalam",
            "gu" => "Gujarati",
            "pa" => "Punjabi",
            "or" => "Odia",

            // Additional East Asian Languages
            "mn" => "Mongolian",
            "my" => "Burmese",
            "lo" => "Lao",
            "km" => "Khmer",

            _ => code,
        }
    }

    /// Return selected languages found in embedded subtitle streams.
    pub fn embedded_subtitle_languages(video_path: &Path, langs: &[String]) -> Vec<String> {
        let mut cmd = Command::new("ffprobe");
        cmd.arg("-v")
            .arg("error")
            .arg("-select_streams")
            .arg("s")
            .arg("-show_entries")
            .arg("stream=index:stream_tags=language")
            .arg("-of")
            .arg("csv=p=0")
            .arg(video_path);
        // Hide the window on Windows
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // On Linux, just redirect output
        #[cfg(not(windows))]
        {
            use std::process::Stdio;
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        }
        let output = cmd.output();
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let requested_languages = langs
                    .iter()
                    .map(|code| code.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let mut found = Vec::new();
                for line in stdout.lines() {
                    // Each line: index,language (e.g., 0,eng)
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 2 {
                        let lang = parts[1].trim().to_ascii_lowercase();
                        for (req, requested) in langs.iter().zip(&requested_languages) {
                            // Accept both 2-letter and 3-letter codes
                            if lang == *requested || lang.starts_with(requested) {
                                if !found.iter().any(|value| value == req) {
                                    found.push(req.clone());
                                }
                                break;
                            }
                        }
                    }
                }
                return found;
            }
        }
        Vec::new()
    }

    /// Check for an embedded subtitle using ffprobe.
    pub fn has_embedded_subtitle(video_path: &Path, langs: &[String]) -> Option<String> {
        Self::embedded_subtitle_languages(video_path, langs)
            .first()
            .map(|code| Self::language_code_to_name(code).to_string())
    }

    /// Identify common hearing-impaired or caption markers in a subtitle filename.
    #[allow(clippy::manual_pattern_char_comparison)]
    pub fn is_hearing_impaired_path(path: &Path) -> bool {
        let stem = match path.file_stem().and_then(|value| value.to_str()) {
            Some(value) => value.to_lowercase(),
            None => return false,
        };

        stem.split(|value: char| matches!(value, '.' | '-' | '_' | ' ' | '[' | ']' | '(' | ')'))
            .any(|part| matches!(part, "sdh" | "cc" | "caption" | "captions"))
    }

    /// Check if a video is missing subtitles for any selected language
    pub fn video_missing_subtitle(
        video_path: &Path,
        selected_languages: &[String],
        exclude_hearing_impaired: bool,
    ) -> bool {
        let folder = match video_path.parent() {
            Some(folder) => folder,
            None => return false,
        };
        Self::video_missing_subtitle_in_listing(
            video_path,
            selected_languages,
            exclude_hearing_impaired,
            &Self::list_subtitle_files_in_folder(folder),
        )
    }

    /// Same missing-sub rules against a pre-read folder listing (unsorted)
    pub fn video_missing_subtitle_in_listing(
        video_path: &Path,
        selected_languages: &[String],
        exclude_hearing_impaired: bool,
        folder_subtitles: &[PathBuf],
    ) -> bool {
        let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
            return false;
        };
        let subtitle_files =
            Self::subtitle_files_for_video_in_listing(video_path, folder_subtitles)
                .into_iter()
                .filter(|path| !exclude_hearing_impaired || !Self::is_hearing_impaired_path(path))
                .collect::<Vec<_>>();
        let has_generic = subtitle_files.iter().any(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(stem))
        });

        for lang in selected_languages {
            let expected_stem = format!("{}.{}", stem, lang);
            let has_language_specific = subtitle_files
                .iter()
                .any(|path| Self::language_specific_stem_matches(path, &expected_stem));
            if !has_language_specific && !has_generic {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::SubtitleUtils;
    use std::path::Path;

    #[test]
    fn identifies_caption_markers_without_matching_regular_words() {
        assert!(SubtitleUtils::is_hearing_impaired_path(Path::new(
            "Movie.en.sdh.srt"
        )));
        assert!(SubtitleUtils::is_hearing_impaired_path(Path::new(
            "Movie.en.cc.srt"
        )));
        assert!(SubtitleUtils::is_hearing_impaired_path(Path::new(
            "Movie.en.[CC].srt"
        )));
        assert!(!SubtitleUtils::is_hearing_impaired_path(Path::new(
            "Movie.hi.srt"
        )));
        assert!(!SubtitleUtils::is_hearing_impaired_path(Path::new(
            "Movie.en.srt"
        )));
        assert!(!SubtitleUtils::is_hearing_impaired_path(Path::new(
            "The.History.srt"
        )));
    }

    #[test]
    fn creates_numbered_backups_without_replacing_existing_backups() {
        let folder = std::env::temp_dir().join(format!(
            "rustitles-backup-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let subtitle = folder.join("Movie.en.srt");
        std::fs::write(&subtitle, "old").unwrap();

        let first = SubtitleUtils::backup_subtitle_files(std::slice::from_ref(&subtitle)).unwrap();
        assert_eq!(first, vec![folder.join("Movie.en.srt.bak")]);
        assert_eq!(std::fs::read_to_string(&first[0]).unwrap(), "old");

        std::fs::write(&subtitle, "new").unwrap();
        let second = SubtitleUtils::backup_subtitle_files(std::slice::from_ref(&subtitle)).unwrap();
        assert_eq!(second, vec![folder.join("Movie.en.srt.bak.1")]);
        assert_eq!(std::fs::read_to_string(&second[0]).unwrap(), "new");

        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn reports_missing_subtitle_backup_source() {
        let folder = std::env::temp_dir().join(format!(
            "rustitles-missing-backup-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("missing.srt");

        let result = SubtitleUtils::backup_subtitle_files(&[path]);

        assert!(result.is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn missing_subtitle_uses_language_generic_and_sdh_rules() {
        let folder = std::env::temp_dir().join(format!(
            "rustitles-missing-sub-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let video = folder.join("Movie.mkv");
        std::fs::write(&video, []).unwrap();
        let langs = vec!["en".to_string()];

        assert!(SubtitleUtils::video_missing_subtitle(&video, &langs, false));

        std::fs::write(folder.join("Movie.en.srt"), "sub").unwrap();
        let listing = SubtitleUtils::list_subtitle_files_in_folder(&folder);
        assert!(!SubtitleUtils::video_missing_subtitle_in_listing(
            &video, &langs, false, &listing
        ));

        std::fs::remove_file(folder.join("Movie.en.srt")).unwrap();
        std::fs::write(folder.join("Movie.srt"), "sub").unwrap();
        let listing = SubtitleUtils::list_subtitle_files_in_folder(&folder);
        assert!(!SubtitleUtils::video_missing_subtitle_in_listing(
            &video, &langs, false, &listing
        ));

        std::fs::remove_file(folder.join("Movie.srt")).unwrap();
        std::fs::write(folder.join("Movie.en.sdh.srt"), "sub").unwrap();
        let listing = SubtitleUtils::list_subtitle_files_in_folder(&folder);
        assert!(!SubtitleUtils::video_missing_subtitle_in_listing(
            &video, &langs, false, &listing
        ));
        assert!(SubtitleUtils::video_missing_subtitle_in_listing(
            &video, &langs, true, &listing
        ));

        std::fs::remove_dir_all(folder).unwrap();
    }
}
