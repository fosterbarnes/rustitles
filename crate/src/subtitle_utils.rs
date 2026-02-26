//! Subtitle file utilities and language detection
//! 
//! This module provides functions for finding subtitle files, detecting
//! embedded subtitles, and handling language code conversions.

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
        let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Vec::new(),
        };
        let subtitle_extensions = ["srt", "sub", "ssa", "ass", "vtt"];
        let mut found_subtitles = Vec::new();
        
        crate::debug!("Searching for subtitle files for {} in {}", video_path.display(), folder.display());
        
        // Try language-specific first
        for lang in langs {
            for ext in &subtitle_extensions {
                let candidate = folder.join(format!("{}.{}.{}", stem, lang, ext));
                if candidate.exists() {
                    crate::debug!("Found language-specific subtitle: {}", candidate.display());
                    found_subtitles.push(candidate);
                    break; // Found one for this language, move to next
                }
            }
        }
        // Then try generic
        for ext in &subtitle_extensions {
            let candidate = folder.join(format!("{}.{}", stem, ext));
            if candidate.exists() {
                crate::debug!("Found generic subtitle: {}", candidate.display());
                found_subtitles.push(candidate);
                break; // Found one generic, stop
            }
        }
        
        if found_subtitles.is_empty() {
            crate::debug!("No subtitle files found for {}", video_path.display());
        } else {
            crate::debug!("Found {} subtitle files for {}", found_subtitles.len(), video_path.display());
        }
        
        found_subtitles
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

    /// Check for embedded subtitles using ffprobe
    pub fn has_embedded_subtitle(video_path: &std::path::Path, langs: &[String]) -> Option<String> {
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
                for line in stdout.lines() {
                    // Each line: index,language (e.g., 0,eng)
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 2 {
                        let lang = parts[1].trim().to_lowercase();
                        for req in langs {
                            // Accept both 2-letter and 3-letter codes
                            if lang == req.to_lowercase() || lang.starts_with(&req.to_lowercase()) {
                                return Some(Self::language_code_to_name(req).to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if a video is missing subtitles for any selected language
    pub fn video_missing_subtitle(video_path: &Path, selected_languages: &[String]) -> bool {
        if let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) {
            let folder = video_path.parent().unwrap_or_else(|| Path::new(""));
            
            // Check for common subtitle extensions
            let subtitle_extensions = ["srt", "sub", "ssa", "ass", "vtt"];
            
            // Check if any of the selected languages are missing
            for lang in selected_languages {
                let mut lang_found = false;
                
                // Check for language-specific patterns first (e.g., video.en.srt)
                for ext in &subtitle_extensions {
                    let subtitle_path = folder.join(format!("{}.{}.{}", stem, lang, ext));
                    if subtitle_path.exists() {
                        lang_found = true;
                        break;
                    }
                }
                
                // If language-specific not found, check basic pattern (e.g., video.srt)
                if !lang_found {
                    for ext in &subtitle_extensions {
                        let subtitle_path = folder.join(format!("{}.{}", stem, ext));
                        if subtitle_path.exists() {
                            lang_found = true;
                            break;
                        }
                    }
                }
                
                // If this language is missing, return true (missing subtitles)
                if !lang_found {
                    return true;
                }
            }
        }
        false // All selected languages have subtitles
    }
} 