//! Configuration constants and settings for the Rustitles subtitle downloader
//! 
//! This module contains application-wide configuration values including
//! supported file formats, download limits, and UI settings.

/// The current application version (keep in sync with Cargo.toml)
pub const APP_VERSION: &str = "2.2.1";

/// Supported video file extensions for subtitle scanning
pub static VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "mpeg", "mpg", "webm", "m4v",
    "3gp", "3g2", "asf", "mts", "m2ts", "ts", "vob", "ogv", "rm", "rmvb", 
    "divx", "f4v", "mxf", "mp2", "mpv", "dat", "tod", "vro", "drc", "mng", 
    "qt", "yuv", "viv", "amv", "nsv", "svi", "mpe", "mpv2", "m2v", "m1v", 
    "m2p", "trp", "tp", "ps", "evo", "ogm", "ogx", "mod", "rec", "dvr-ms", 
    "pva", "wtv", "m4p", "m4b", "m4r", "m4a", "3gpp", "3gpp2"
];

/// Default concurrent downloads
pub static DEFAULT_CONCURRENT_DOWNLOADS: usize = 25;

/// Maximum concurrent downloads
pub static MAX_CONCURRENT_DOWNLOADS: usize = 100;

/// Default window size
pub static WINDOW_SIZE: [f32; 2] = [800.0, 580.0];

/// Minimum window size
pub static MIN_WINDOW_SIZE: [f32; 2] = [600.0, 461.0]; 