//! Renderer selection and startup crash recovery.
//!
//! winit cannot recreate its event loop in one process, so a failed renderer is
//! retried by relaunching the app with the other renderer.

use crate::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

const STATE_FILE: &str = "rustitles_startup.json";
const RENDERER_ARG: &str = "--renderer=";
const RETRY_ARG: &str = "--renderer-retry";
const RENDERER_ENV: &str = "RUSTITLES_RENDERER";
const ISSUES_URL: &str = "https://github.com/fosterbarnes/rustitles/issues";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RendererKind {
    Glow,
    Wgpu,
}

impl RendererKind {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "glow" | "opengl" | "gl" => Some(Self::Glow),
            "wgpu" => Some(Self::Wgpu),
            _ => None,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Glow => Self::Wgpu,
            Self::Wgpu => Self::Glow,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Glow => "glow",
            Self::Wgpu => "wgpu",
        }
    }

    fn platform_default() -> Self {
        // Metal is reliable on macOS; OpenGL matches 2.3 everywhere else.
        if cfg!(target_os = "macos") {
            Self::Wgpu
        } else {
            Self::Glow
        }
    }
}

impl From<RendererKind> for eframe::Renderer {
    fn from(kind: RendererKind) -> Self {
        match kind {
            RendererKind::Glow => eframe::Renderer::Glow,
            RendererKind::Wgpu => eframe::Renderer::Wgpu,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
struct StartupState {
    renderer: RendererKind,
    /// False until the renderer has drawn a frame.
    confirmed: bool,
}

/// Pick a renderer: CLI, env, saved state, then platform default.
fn choose(
    cli: Option<RendererKind>,
    env: Option<RendererKind>,
    saved: Option<StartupState>,
) -> (RendererKind, &'static str) {
    if let Some(kind) = cli {
        return (kind, "command line");
    }
    if let Some(kind) = env {
        return (kind, RENDERER_ENV);
    }
    match saved {
        Some(state) if state.confirmed => (state.renderer, "last working renderer"),
        Some(state) => (
            state.renderer.other(),
            "previous launch failed before its first frame",
        ),
        None => (RendererKind::platform_default(), "platform default"),
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);
static FRAMES: AtomicUsize = AtomicUsize::new(0);
static CONFIRMED: AtomicBool = AtomicBool::new(false);

fn state_path() -> Option<PathBuf> {
    crate::settings::Settings::get_path()
        .ok()
        .map(|path| path.with_file_name(STATE_FILE))
}

fn load_state() -> Option<StartupState> {
    let content = std::fs::read_to_string(state_path()?).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_state(state: StartupState) {
    let Some(path) = state_path() else { return };
    let result = serde_json::to_vec(&state)
        .map_err(std::io::Error::other)
        .and_then(|bytes| crate::helper_functions::Utils::write_atomic(&path, &bytes));
    if let Err(error) = result {
        warn!(
            "Failed to save startup state to {}: {}",
            path.display(),
            error
        );
    }
}

/// Choose the renderer for this launch and mark the attempt as unconfirmed.
pub fn begin() -> RendererKind {
    let args: Vec<String> = std::env::args().collect();
    let cli = args
        .iter()
        .find_map(|arg| arg.strip_prefix(RENDERER_ARG))
        .and_then(RendererKind::parse);
    let env = std::env::var(RENDERER_ENV)
        .ok()
        .and_then(|value| RendererKind::parse(&value));
    let saved = load_state();
    let (kind, reason) = choose(cli, env, saved);
    if saved.is_some_and(|state| !state.confirmed) && cli.is_none() && env.is_none() {
        warn!(
            "Last launch with the {} renderer never drew a frame; switching to {}",
            kind.other().as_str(),
            kind.as_str()
        );
    }
    info!("Using {} renderer ({})", kind.as_str(), reason);
    CURRENT.store(kind as u8, Ordering::Relaxed);
    save_state(StartupState {
        renderer: kind,
        confirmed: false,
    });
    kind
}

fn current() -> RendererKind {
    if CURRENT.load(Ordering::Relaxed) == RendererKind::Wgpu as u8 {
        RendererKind::Wgpu
    } else {
        RendererKind::Glow
    }
}

/// Call once per UI pass; the second pass means a frame was presented.
pub fn frame_rendered() {
    if FRAMES.fetch_add(1, Ordering::Relaxed) == 1 {
        CONFIRMED.store(true, Ordering::Relaxed);
        save_state(StartupState {
            renderer: current(),
            confirmed: true,
        });
        info!("{} renderer confirmed working", current().as_str());
    }
}

/// Whether this launch has presented a frame.
pub fn is_confirmed() -> bool {
    CONFIRMED.load(Ordering::Relaxed)
}

/// Handle a failure before the first frame: relaunch once with the other renderer.
/// Returns false when no relaunch happened and the caller should report the error.
pub fn relaunch_with_other_renderer(reason: &str) -> bool {
    if std::env::args().any(|arg| arg == RETRY_ARG) {
        error!("Both renderers failed to start; last error: {}", reason);
        return false;
    }
    let next = current().other();
    error!(
        "The {} renderer failed to start: {}. Relaunching with {}",
        current().as_str(),
        reason,
        next.as_str()
    );
    save_state(StartupState {
        renderer: next,
        confirmed: false,
    });
    let spawned = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .arg(format!("{RENDERER_ARG}{}", next.as_str()))
            .arg(RETRY_ARG)
            .spawn()
    });
    match spawned {
        Ok(_) => true,
        Err(error) => {
            error!("Failed to relaunch Rustitles: {}", error);
            false
        }
    }
}

/// Show a blocking error dialog that points at the log.
pub fn show_fatal_dialog(reason: &str) {
    let log_path = crate::helper_functions::Utils::get_log_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unavailable".to_string());
    // wgpu appends an empty debug map to some errors.
    let reason = reason.trim_end_matches(": {}");
    let message = format!(
        "Rustitles hit an error and has to close.\n\n{reason}\n\nLog file:\n{log_path}\n\n\
         Please attach the log when reporting this at {ISSUES_URL}"
    );
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Rustitles")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(renderer: RendererKind, confirmed: bool) -> Option<StartupState> {
        Some(StartupState {
            renderer,
            confirmed,
        })
    }

    #[test]
    fn explicit_choices_win_over_saved_state() {
        let saved = state(RendererKind::Glow, true);
        assert_eq!(
            choose(Some(RendererKind::Wgpu), Some(RendererKind::Glow), saved).0,
            RendererKind::Wgpu
        );
        assert_eq!(
            choose(None, Some(RendererKind::Wgpu), saved).0,
            RendererKind::Wgpu
        );
    }

    #[test]
    fn unconfirmed_renderer_switches_to_the_other() {
        assert_eq!(
            choose(None, None, state(RendererKind::Glow, false)).0,
            RendererKind::Wgpu
        );
        assert_eq!(
            choose(None, None, state(RendererKind::Wgpu, false)).0,
            RendererKind::Glow
        );
    }

    #[test]
    fn confirmed_renderer_is_kept_and_default_applies_without_state() {
        assert_eq!(
            choose(None, None, state(RendererKind::Wgpu, true)).0,
            RendererKind::Wgpu
        );
        assert_eq!(choose(None, None, None).0, RendererKind::platform_default());
    }

    #[test]
    fn parses_renderer_names() {
        assert_eq!(RendererKind::parse(" OpenGL "), Some(RendererKind::Glow));
        assert_eq!(RendererKind::parse("wgpu"), Some(RendererKind::Wgpu));
        assert_eq!(RendererKind::parse("vulkan"), None);
    }
}
