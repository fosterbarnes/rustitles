//! Subtitle file utilities and language detection.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::python_manager::PythonManager;

/// Subtitle file utilities.
pub struct SubtitleUtils;

impl SubtitleUtils {
    /// Find subtitle files for a video and its languages.
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

    /// Find matching subtitle files in a folder listing.
    pub fn find_all_subtitle_files_in_listing(
        video_path: &Path,
        langs: &[String],
        folder_subtitles: &[PathBuf],
    ) -> Vec<PathBuf> {
        let mut found: Vec<_> = folder_subtitles
            .iter()
            .filter(|path| {
                Self::sidecar_suffix(video_path, path) == Some("")
                    || langs
                        .iter()
                        .any(|lang| Self::matches_language(video_path, path, lang))
            })
            .cloned()
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// List subtitle files in a folder.
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
                path.is_file()
                    && path
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|ext| {
                            EXTENSIONS
                                .iter()
                                .any(|known| known.eq_ignore_ascii_case(ext))
                        })
            })
            .collect()
    }

    /// Filter a folder listing to one video's sidecars.
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

    fn sidecar_suffix<'a>(video_path: &Path, path: &'a Path) -> Option<&'a str> {
        let video = video_path.file_stem()?.to_str()?;
        let subtitle = path.file_stem()?.to_str()?;
        if !subtitle.get(..video.len())?.eq_ignore_ascii_case(video) {
            return None;
        }
        let suffix = &subtitle[video.len()..];
        if suffix.is_empty() {
            Some("")
        } else {
            suffix.strip_prefix('.')
        }
    }

    fn is_caption_marker(part: &str) -> bool {
        matches!(
            part.trim_matches(['[', ']', '(', ')'])
                .to_ascii_lowercase()
                .as_str(),
            "hi" | "sdh" | "cc" | "caption" | "captions"
        )
    }

    pub fn matches_language(video_path: &Path, path: &Path, language: &str) -> bool {
        let Some(suffix) = Self::sidecar_suffix(video_path, path) else {
            return false;
        };
        let parts: Vec<_> = suffix.split('.').collect();
        parts.iter().enumerate().any(|(index, part)| {
            part.eq_ignore_ascii_case(language)
                && parts
                    .iter()
                    .enumerate()
                    .all(|(other, part)| other == index || Self::is_caption_marker(part))
        })
    }

    /// Find adjacent subtitle files for a video.
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

    /// Back up existing subtitles before an overwrite.
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

    /// Convert a language code to a display name.
    pub fn language_code_to_name(code: &str) -> &str {
        match code {
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

            "af" => "Afrikaans",
            "sw" => "Swahili",
            "zu" => "Zulu",
            "xh" => "Xhosa",

            "ku" => "Kurdish",
            "az" => "Azerbaijani",
            "ka" => "Georgian",
            "am" => "Amharic",

            "ta" => "Tamil",
            "te" => "Telugu",
            "kn" => "Kannada",
            "ml" => "Malayalam",
            "gu" => "Gujarati",
            "pa" => "Punjabi",
            "or" => "Odia",

            "mn" => "Mongolian",
            "my" => "Burmese",
            "lo" => "Lao",
            "km" => "Khmer",

            _ => code,
        }
    }

    /// Find selected languages in embedded subtitle streams.
    pub fn embedded_subtitle_languages(video_path: &Path, langs: &[String]) -> Vec<String> {
        let video_path = video_path.to_string_lossy().into_owned();
        let args = [
            "-v",
            "error",
            "-select_streams",
            "s",
            "-show_entries",
            "stream=index:stream_tags=language",
            "-of",
            "csv=p=0",
            video_path.as_str(),
        ];
        let output = PythonManager::run_ffprobe(&args);
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Self::parse_embedded_languages(&stdout, langs);
            }
        }
        Vec::new()
    }

    /// Check for embedded subtitles.
    pub fn has_embedded_subtitle(video_path: &Path, langs: &[String]) -> Option<String> {
        Self::embedded_subtitle_languages(video_path, langs)
            .first()
            .map(|code| Self::language_code_to_name(code).to_string())
    }

    /// Inspect the sidecar suffix, not words in the video title.
    pub fn is_hearing_impaired_path(video_path: &Path, path: &Path) -> bool {
        let Some(suffix) = Self::sidecar_suffix(video_path, path) else {
            return false;
        };
        suffix.contains('.') && suffix.split('.').any(Self::is_caption_marker)
    }

    fn parse_embedded_languages(output: &str, langs: &[String]) -> Vec<String> {
        let embedded: Vec<_> = output
            .lines()
            .filter_map(|line| line.split_once(','))
            .map(|(_, code)| Self::normalize_language(code.trim()))
            .collect();
        langs
            .iter()
            .filter(|code| embedded.contains(&Self::normalize_language(code)))
            .cloned()
            .collect()
    }

    fn normalize_language(code: &str) -> String {
        let lower = code.to_ascii_lowercase();
        match lower.as_str() {
            "eng" => "en",
            "fra" | "fre" => "fr",
            "spa" => "es",
            "deu" | "ger" => "de",
            "ita" => "it",
            "por" => "pt",
            "nld" | "dut" => "nl",
            "pol" => "pl",
            "rus" => "ru",
            "srp" => "sr",
            "swe" => "sv",
            "fin" => "fi",
            "dan" => "da",
            "nor" => "no",
            "ces" | "cze" => "cs",
            "hun" => "hu",
            "ron" | "rum" => "ro",
            "bul" => "bg",
            "hrv" => "hr",
            "est" => "et",
            "ell" | "gre" => "el",
            "isl" | "ice" => "is",
            "lav" => "lv",
            "lit" => "lt",
            "mlt" => "mt",
            "slk" | "slo" => "sk",
            "slv" => "sl",
            "tur" => "tr",
            "ukr" => "uk",
            "heb" => "he",
            "ara" => "ar",
            "jpn" => "ja",
            "kor" => "ko",
            "zho" | "chi" => "zh",
            "tha" => "th",
            "vie" => "vi",
            "ind" => "id",
            "msa" | "may" => "ms",
            "ben" => "bn",
            "hin" => "hi",
            "urd" => "ur",
            "fas" | "per" => "fa",
            "afr" => "af",
            "swa" => "sw",
            "zul" => "zu",
            "xho" => "xh",
            "kur" => "ku",
            "aze" => "az",
            "kat" | "geo" => "ka",
            "amh" => "am",
            "tam" => "ta",
            "tel" => "te",
            "kan" => "kn",
            "mal" => "ml",
            "guj" => "gu",
            "pan" => "pa",
            "ori" => "or",
            "mon" => "mn",
            "mya" | "bur" => "my",
            "lao" => "lo",
            "khm" => "km",
            _ => &lower,
        }
        .to_string()
    }

    /// Check whether a video is missing a selected language.
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

    /// Check missing subtitles using a folder listing.
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
                .filter(|path| {
                    !exclude_hearing_impaired || !Self::is_hearing_impaired_path(video_path, path)
                })
                .collect::<Vec<_>>();
        let has_generic = subtitle_files.iter().any(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(stem))
        });

        for lang in selected_languages {
            let has_language_specific = subtitle_files
                .iter()
                .any(|path| Self::matches_language(video_path, path, lang));
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
    fn discovery_includes_all_matching_formats() {
        let paths = ["Movie.en.ass", "Movie.en.srt", "Movie.srt", "Movie.ass"]
            .map(std::path::PathBuf::from);
        let mut found = SubtitleUtils::find_all_subtitle_files_in_listing(
            Path::new("Movie.mkv"),
            &["en".into()],
            &paths,
        );
        found.sort();
        let mut expected = paths.to_vec();
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn recognizes_upstream_type_suffix() {
        assert!(SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.[hi].en.srt")
        ));
        assert!(SubtitleUtils::matches_language(
            Path::new("Movie.mkv"),
            Path::new("Movie.[hi].en.srt"),
            "en"
        ));
        assert!(!SubtitleUtils::is_hearing_impaired_path(
            Path::new("Caption.2024.mkv"),
            Path::new("Caption.2024.en.srt")
        ));
        assert!(!SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.hi.srt")
        ));
    }

    #[test]
    fn embedded_languages_use_iso_aliases_without_guessing_regions() {
        let requested = ["es", "ja", "de", "en", "en-us", "xx"].map(str::to_string);
        assert_eq!(
            SubtitleUtils::parse_embedded_languages("0,spa\n1,jpn\n2,ger\n3,eng", &requested),
            vec!["es", "ja", "de", "en"]
        );
    }

    #[test]
    fn identifies_caption_markers_without_matching_regular_words() {
        assert!(SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.en.sdh.srt")
        ));
        assert!(SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.en.cc.srt")
        ));
        assert!(SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.en.[CC].srt")
        ));
        assert!(!SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.hi.srt")
        ));
        assert!(!SubtitleUtils::is_hearing_impaired_path(
            Path::new("Movie.mkv"),
            Path::new("Movie.en.srt")
        ));
        assert!(!SubtitleUtils::is_hearing_impaired_path(
            Path::new("The.History.mkv"),
            Path::new("The.History.srt")
        ));
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
