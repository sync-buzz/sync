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

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// The label the settings window is created under, and the one the frontend
/// reads to decide what to render.
pub const SETTINGS_LABEL: &str = "settings";

/// The section somebody was sent to, until the window has read it.
///
/// Held here rather than carried in the request that opens the window, because
/// the window may not exist yet: it is built hidden and reveals itself once it
/// has painted, so an event carrying the section would be shouted at a webview
/// with nothing listening. One value, read and taken by whoever arrives — the
/// first frame of a new window, or a window that was already open and got the
/// nudge below.
///
/// `None` is the ordinary state and means *wherever you were*: somebody who
/// pressed `⌘,` asked for settings, not for a particular one, and moving them
/// off the section they last used would be this answering a question they did
/// not ask.
#[derive(Default)]
pub struct AskedSection(Mutex<Option<String>>);

impl AskedSection {
    /// Remember where somebody was sent, if they were sent anywhere.
    ///
    /// A request naming nothing leaves a standing one alone. The two arrive in
    /// either order — a control that sends somebody to a section, and `⌘,`
    /// pressed a moment later — and the one that named a place is the one the
    /// person is waiting to see.
    fn remember(&self, section: Option<String>) {
        if section.is_none() {
            return;
        }
        if let Ok(mut asked) = self.0.lock() {
            *asked = section;
        }
    }

    /// Read it and clear it.
    ///
    /// Taken rather than read, because it is an errand somebody ran once. Left
    /// standing it would move that person off whatever they opened next time,
    /// which is a window that will not stay where it is put.
    fn take(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut asked| asked.take())
    }
}

/// What is emitted at the settings window when the value above has changed.
///
/// A bare nudge with nothing in it: what to show is in the state, and a payload
/// would be a second copy of it that could arrive in the other order.
const ASKED: &str = "settings:section";

/// Wide enough for a source list beside a column of settings, and no wider:
/// the window holds a list of agents and a list of extensions, and a settings
/// window that opens larger than its content reads as an empty one.
///
/// The width is set by the widest row this window actually draws rather than by
/// the prose, which is held to a measure and would fit in far less. That row is
/// an agent: a name over the path of the file the entry is written into, the
/// word for what state it is in, and a button. The path is the part that has no
/// natural length — it is somebody's home directory — and at 760 it was the
/// column that truncated first, which is the one thing on that row a person
/// acts on.
///
/// The height is set by the column beside it, and by the same test: nine
/// sections in four runs, one of them showing its parts, come to a little over
/// five hundred points. At 540 the last row sat under the bottom edge, so the
/// window opened on a list that was already scrolling — which reads as a list
/// with more in it than there is.
const WIDTH: f64 = 920.0;
const HEIGHT: f64 = 640.0;
const MIN_WIDTH: f64 = 760.0;
const MIN_HEIGHT: f64 = 520.0;

/// Open the settings window, or bring the open one forward.
///
/// `section` is where the person was sent, and it is a plain string on purpose:
/// which sections there are is decided in the settings window, and a list of
/// their names here would be a second one that goes stale the day somebody adds
/// a screen. A name this build does not have is simply not found, and the
/// window opens where it was.
///
/// The error is a message rather than a kind: there is one failure — the
/// platform refused the window — and nothing for the interface to branch on.
#[tauri::command]
pub fn settings_open<R: Runtime>(app: AppHandle<R>, section: Option<String>) -> Result<(), String> {
    // `try_state` rather than `state`, which panics when nothing manages it.
    // The only build that does not is one made in a test, and what it loses is
    // being sent to a section — which is what saying nothing already means, and
    // is not worth a crash on `⌘,`.
    if let Some(asked) = app.try_state::<AskedSection>() {
        asked.remember(section);
    }

    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        // Already built. It may be hidden — closing a window on macOS destroys
        // it, but a window can also be ordered out — so both are asked for.
        window.show().map_err(to_message)?;
        window.set_focus().map_err(to_message)?;
        // It is already mounted, so it will not read the state on its own. The
        // nudge is sent whether or not a section was named: a window told to
        // look finds `None` and stays where it is.
        let _ = app.emit_to(SETTINGS_LABEL, ASKED, ());
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

/// Which section the settings window was asked to show, once.
///
/// Taken rather than read: it is an errand somebody ran, and a value left
/// standing would move that person off whatever they opened next time.
///
/// # Errors
///
/// Never. A settings window that could not ask this would have nowhere to go
/// but the section it was already showing, which is what `None` says anyway.
#[tauri::command]
pub fn settings_section<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    Ok(app
        .try_state::<AskedSection>()
        .and_then(|asked| asked.take()))
}

fn to_message(error: tauri::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::AskedSection;

    /// The whole of what this state is for: an errand, run once.
    #[test]
    fn where_somebody_was_sent_is_read_once_and_then_forgotten() {
        let asked = AskedSection::default();
        asked.remember(Some("agents".to_owned()));

        assert_eq!(asked.take().as_deref(), Some("agents"));
        assert_eq!(
            asked.take(),
            None,
            "the window would be moved off the section somebody opened next",
        );
    }

    /// `⌘,` is not a request for a particular screen, and must not clear one
    /// that is: the two arrive in either order, and the person waiting is the
    /// one who pressed a control that named somewhere.
    #[test]
    fn opening_settings_without_naming_a_section_leaves_a_standing_ask_alone() {
        let asked = AskedSection::default();
        asked.remember(Some("agents".to_owned()));
        asked.remember(None);

        assert_eq!(asked.take().as_deref(), Some("agents"));
    }
}
