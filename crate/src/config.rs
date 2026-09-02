//! Configuration constants and settings for the Rustitles subtitle downloader
//!
//! This module contains application-wide configuration values including
//! supported file formats, download limits, and UI settings.

use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::time::Duration;

/// The current application version (keep in sync with .version/version, crate/src/version.txt, crate/Cargo.toml)
pub const APP_VERSION: &str = "2.4.0";

/// Supported video file extensions for subtitle scanning
pub static VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "mpeg", "mpg", "webm", "m4v", "3gp", "3g2", "asf",
    "mts", "m2ts", "ts", "vob", "ogv", "rm", "rmvb", "divx", "f4v", "mxf", "mp2", "mpv", "dat",
    "tod", "vro", "drc", "mng", "qt", "yuv", "viv", "amv", "nsv", "svi", "mpe", "mpv2", "m2v",
    "m1v", "m2p", "trp", "tp", "ps", "evo", "ogm", "ogx", "mod", "rec", "dvr-ms", "pva", "wtv",
    "m4p", "3gpp", "3gpp2",
];

/// Lowercase video extensions for O(1) case-insensitive lookup
pub static VIDEO_EXTENSIONS_SET: Lazy<HashSet<&'static str>> =
    Lazy::new(|| VIDEO_EXTENSIONS.iter().copied().collect());

/// Plex local-extras folder names (matched case-insensitively)
pub static EXTRAS_FOLDER_NAMES: &[&str] = &[
    "Behind The Scenes",
    "Deleted Scenes",
    "Featurettes",
    "Interviews",
    "Scenes",
    "Shorts",
    "Trailers",
    "Other",
];

/// Default concurrent downloads
pub static DEFAULT_CONCURRENT_DOWNLOADS: usize = 25;

/// Maximum concurrent downloads
pub static MAX_CONCURRENT_DOWNLOADS: usize = 100;

/// Default window size
pub static WINDOW_SIZE: [f32; 2] = [1000.0, 700.0];

/// Minimum window size
pub static MIN_WINDOW_SIZE: [f32; 2] = [600.0, 461.0];

/// Minimum time the startup splash stays on screen before the main UI.
pub const MIN_SPLASH_DURATION: Duration = Duration::from_millis(2700);

/// Extra hold after the splash is allowed to dismiss, before fade-out starts.
pub const SPLASH_FADE_WAIT: Duration = Duration::from_millis(100);

/// Splash fade-out length.
pub const SPLASH_FADE_DURATION: Duration = Duration::from_millis(500);

/// Extra hold after fade-out finishes, before the main UI appears.
pub const SPLASH_POST_FADE_WAIT: Duration = Duration::from_millis(100);
