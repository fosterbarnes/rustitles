//! Application configuration constants.

use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::time::Duration;

/// The current application version.
pub const APP_VERSION: &str = "2.4.0";

/// Video file extensions supported by scans.
pub static VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "mpeg", "mpg", "webm", "m4v", "3gp", "3g2", "asf",
    "mts", "m2ts", "ts", "vob", "ogv", "rm", "rmvb", "divx", "f4v", "mxf", "mp2", "mpv", "dat",
    "tod", "vro", "drc", "mng", "qt", "yuv", "viv", "amv", "nsv", "svi", "mpe", "mpv2", "m2v",
    "m1v", "m2p", "trp", "tp", "ps", "evo", "ogm", "ogx", "mod", "rec", "dvr-ms", "pva", "wtv",
    "m4p", "3gpp", "3gpp2",
];

/// Lowercase video extensions for quick lookup.
pub static VIDEO_EXTENSIONS_SET: Lazy<HashSet<&'static str>> =
    Lazy::new(|| VIDEO_EXTENSIONS.iter().copied().collect());

/// Plex local-extras folder names.
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

/// Default concurrent downloads.
pub static DEFAULT_CONCURRENT_DOWNLOADS: usize = 25;

/// Maximum concurrent downloads.
pub static MAX_CONCURRENT_DOWNLOADS: usize = 100;

/// Default window size.
pub static WINDOW_SIZE: [f32; 2] = [1000.0, 700.0];

/// Minimum window size.
pub static MIN_WINDOW_SIZE: [f32; 2] = [600.0, 461.0];

/// Minimum startup splash time.
pub const MIN_SPLASH_DURATION: Duration = Duration::from_millis(2700);

/// Delay before the splash fades out.
pub const SPLASH_FADE_WAIT: Duration = Duration::from_millis(100);

/// Splash fade-out time.
pub const SPLASH_FADE_DURATION: Duration = Duration::from_millis(500);

/// Delay before the main UI appears.
pub const SPLASH_POST_FADE_WAIT: Duration = Duration::from_millis(100);
