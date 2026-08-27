//! The settings window.
//!
//! Settings belong to the installation, not to a project and not to the window
//! showing one: which agents this Mac connects to Sync, how its server is
//! reached, and which extensions it has. Nothing here is about a project any
//! more — one server answers for every project, so connecting an agent is a
//! gesture of this Mac's rather than of whatever window it was done from. On macOS that is a window of its own — the one every native application
//! opens with `⌘,` — rather than a sheet, which the shell reserves for what
//! configures the window it slides out of.
//!
//! It is one webview on the same document as the main window. Which of the two
//! a document is showing is decided by the window's label rather than by a
//! route: the frontend is a static export, so a second route would be a second
//! HTML file that has to resolve identically under the dev server and inside
//! the bundle, and a label answers the same question without that.
//!
//! The window is built hidden and revealed by the frontend once it has painted,
//! for the reason the main window is: a window that appears before its first
//! frame is a flash of nothing.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// The label the settings window is created under, and the one the frontend
/// reads to decide what to render.
pub const SETTINGS_LABEL: &str = "settings";

/// Wide enough for a source list beside a column of settings, and no wider:
/// the window holds a list of agents and a list of extensions, and a settings
/// window that opens larger than its content reads as an empty one.
const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 540.0;
const MIN_WIDTH: f64 = 640.0;
const MIN_HEIGHT: f64 = 420.0;

/// Open the settings window, or bring the open one forward.
///
/// The error is a message rather than a kind: there is one failure — the
/// platform refused the window — and nothing for the interface to branch on.
#[tauri::command]
pub fn settings_open<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        // Already built. It may be hidden — closing a window on macOS destroys
        // it, but a window can also be ordered out — so both are asked for.
        window.show().map_err(to_message)?;
        window.set_focus().map_err(to_message)?;
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
        .title("Settings")
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .visible(false)
        .build()
        .map_err(to_message)?;

    Ok(())
}

fn to_message(error: tauri::Error) -> String {
    error.to_string()
}
