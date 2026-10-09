//! The panel a person calls from anywhere, and the project it answers for.
//!
//! One key, pressed in whatever application is in front, puts a line in front
//! of somebody and takes what they type to an agent. Everything about *what a
//! line means* is the console's and is not repeated here: this file is the
//! surface and the key, and the line it carries goes through the same reading
//! every line in a project window goes through.
//!
//! # Why it is not a window
//!
//! A window takes the application in front of somebody away from them. That is
//! correct for a window — it is where the work is — and wrong for this: a
//! person who presses the key while reading a page wants to say one sentence
//! and go back to the page, and an application that activated to hear it has
//! already interrupted the thing it was asked to help with.
//!
//! So this is an `NSPanel` with `NonactivatingPanel`, which is the one
//! arrangement in this system where a surface takes the keyboard without taking
//! the front. Tauri builds an `NSWindow` and cannot be asked for the other
//! class, so the window it built is subclassed — `tauri-nspanel`, which is the
//! same `objc2` already in this tree.
//!
//! # Why the key is a position rather than a character
//!
//! `Code::Slash` is a place on the keyboard, not the character printed on it.
//! A shortcut registered as a character would be a shortcut that moves when
//! somebody changes layout, and the one thing a key like this has to be is
//! always in the same place — a person reaches for it without looking, in the
//! middle of working in another application. Under a Latin layout the key reads
//! `⌘/`; under a Cyrillic one the same key reads `⌘.`, and it is the same key.
//!
//! # Why the project is frozen when the key is pressed
//!
//! A line has to go somewhere, and [`crate::windows::foremost`] answers *which
//! project is somebody looking at* by asking which window has the focus. The
//! panel takes the focus the moment it opens, so the question has to be asked
//! before that and the answer kept: a panel that asked after opening would be
//! asking about itself.
//!
//! `None` is an ordinary answer and not a failure — it is what an application
//! sitting in the menu bar with no window open says. The panel then asks a
//! person which project they mean before it takes a line, because a line with
//! no project is a line with nowhere to go.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{
    AppHandle, Emitter as _, Manager as _, PhysicalPosition, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt as _, Modifiers, Shortcut};

use crate::project::RecentProject;

/// The label the panel is built under, and the one the frontend reads to decide
/// that this document is the panel rather than a project window.
pub const LABEL: &str = "ask";

/// Said at the panel when it is opened, so the line is cleared and the caret
/// taken.
///
/// A bare nudge with nothing in it, as the settings window's is: what the panel
/// is to say for is in the state below, and a payload would be a second copy of
/// it that could arrive in the other order.
const OPENED: &str = "ask:opened";

/// As wide as a sentence somebody dictates to an agent, and no wider.
///
/// The surface is one line of input with what it could become listed under it,
/// which is a column of short rows rather than a page — the width is set by the
/// longest thing in that column, an address with a key in it. Past this the
/// line reads as an empty document, which is the one thing it must not look
/// like: it is a place to say something, not a place to write.
const WIDTH: f64 = 720.0;

/// What it opens at: the line, and nothing under it yet.
///
/// The panel is resized to its content from the frontend ([`ask_height`]), so
/// this is the first frame rather than the shape. It is stated here all the
/// same because the window is built before anything has been typed into it, and
/// a panel built at the height of a full list would open as a rectangle of
/// nothing while the list is still empty.
const HEIGHT: f64 = 72.0;

/// How far down the screen the panel sits, as a share of its height.
///
/// Not centred. A surface centred vertically reads as a dialogue that has
/// stopped everything until it is answered, which is what a modal is for; this
/// one sits in the upper third, where this system has put the thing you type
/// into since Spotlight — above the middle, with the answer growing downwards
/// into the space under it.
const FROM_TOP: f64 = 0.18;

/// How round the panel is, in points.
///
/// Stated here as well as in the token layer because two different things draw
/// the same corner: AppKit rounds the *material* and the shadow under it, and
/// the stylesheet rounds the surface drawn on top. A disagreement between them
/// is a bright seam along the top two corners, which is the one place a surface
/// like this is looked at most. It is `--radius-xl`, which is what the slab
/// inside it uses.
const CORNER: f64 = 15.0;

// The class the window is subclassed into, and the three answers that make it a
// panel rather than a window.
#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(AskPanel {
        config: {
            // It takes the keyboard — that is the whole purpose — and never
            // becomes the main window: the main window is where a person's work
            // is, and a panel claiming to be it is a panel the system would
            // start treating as the application.
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// The project the panel is speaking for: what somebody chose, and what was in
/// front otherwise.
///
/// **Two values rather than one, and that is the whole of the fix.** A single
/// one was overwritten by every press of the key — the panel reads what window
/// is in front each time it opens, so a project somebody had chosen out of the
/// list survived exactly until they pressed the key again. The two answer
/// different questions and must not share a slot: *what did they say* outranks
/// *what were they looking at*, and only the second changes under them.
///
/// Neither is taken on reading, unlike the settings window's section: the panel
/// is opened and closed many times against the same project, and a value that
/// emptied itself on the first read would leave the second line with nowhere to
/// go.
#[derive(Default)]
pub struct Asking(Mutex<Chosen>);

/// What the two halves hold at any moment.
#[derive(Default)]
struct Chosen {
    /// What somebody picked out of the list. Nothing until they ever have.
    said: Option<PathBuf>,
    /// What was in front when the key was last pressed.
    seen: Option<PathBuf>,
}

impl Asking {
    /// What the window in front was, which the key reads on every press.
    fn noticed(&self, project: Option<PathBuf>) {
        if let Ok(mut asking) = self.0.lock() {
            asking.seen = project;
        }
    }

    /// What somebody chose, which stands until they choose again.
    fn chose(&self, project: PathBuf) {
        if let Ok(mut asking) = self.0.lock() {
            asking.said = Some(project);
        }
    }

    fn read(&self) -> Option<PathBuf> {
        let asking = self.0.lock().ok()?;
        asking.said.clone().or_else(|| asking.seen.clone())
    }
}

/// The key, as a place on the keyboard rather than as a character on it.
fn shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::SUPER), Code::Slash)
}

/// Build the panel and register the key.
///
/// Both at launch, and the panel is built now rather than when the key is first
/// pressed: a webview takes a few hundred milliseconds to come up, and a
/// surface that is asked for by a keystroke has to be there on that keystroke.
/// What it costs is one hidden webview for the life of the process, which is
/// what every application with a surface like this pays.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = build(app) {
        // Reported rather than fatal, like the menu bar item: a panel that
        // could not be made is a key that does nothing, and the application is
        // still the thing agents talk to.
        eprintln!("the panel could not be created: {error}");
        return;
    }

    if let Err(error) = app.global_shortcut().register(shortcut()) {
        // The commonest cause is another application holding the same key, and
        // there is nothing to do about it from here.
        eprintln!("the panel's shortcut could not be registered: {error}");
    }
}

/// The window, subclassed into a panel and left hidden.
fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Ask Sync")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        // The panel draws its own surface — a rounded slab with a shadow under
        // it — so the window behind it holds no colour of its own. Without this
        // the corners are drawn over an opaque rectangle.
        .transparent(true)
        .shadow(true)
        // The material, asked for at build time so the panel opens with it
        // already applied rather than acquiring it a frame later.
        //
        // This is the whole of the panel's background. A surface over another
        // application has to say *there is something behind me* — that is what
        // keeps it from reading as a screenshot pasted over somebody's work —
        // and the only thing on this system that can say it is the window
        // server: `backdrop-filter` blurs the page under an element, not the
        // desktop under the window. `Popover` is the material this system uses
        // for exactly this surface, and it follows both appearances and
        // reduced transparency without being told.
        .effects(tauri::utils::config::WindowEffectsConfig {
            effects: vec![tauri::utils::WindowEffect::Popover],
            // Always the active one. The panel is only ever on screen while it
            // has the keyboard, so a material that dimmed when the window
            // behind it was active would be dimmed the whole time.
            state: Some(tauri::utils::WindowEffectState::Active),
            radius: Some(CORNER),
            color: None,
        })
        .visible(false)
        // It is not a window a person switches to. A panel in the window menu,
        // in the Dock's window list or under `⌘\`` would be an entry for
        // something that exists for one keystroke at a time.
        .skip_taskbar(true)
        .build()?;

    #[cfg(target_os = "macos")]
    subclass(&window)?;

    Ok(())
}

/// Make the window AppKit built into the class this surface needs.
///
/// Everything in here is the difference between a panel and a window, and all
/// of it is this platform's. Where Sync runs without AppKit the surface is an
/// ordinary window that floats — it steals the front, which is a worse panel
/// and not a missing one.
#[cfg(target_os = "macos")]
fn subclass<R: Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    use tauri_nspanel::WebviewWindowExt as _;
    use tauri_nspanel::objc2_app_kit::NSWindowStyleMask;
    use tauri_nspanel::{CollectionBehavior, PanelLevel};

    let panel = window.to_panel::<AskPanel<R>>()?;

    // The one flag the class exists for: the panel takes the keyboard and the
    // application in front of somebody stays in front. Added rather than set,
    // so Tauri's own structural styles survive it.
    if let Err(error) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
        eprintln!("the panel refused the non-activating style: {error}");
    }

    // Above ordinary windows but below the menu bar and the system's own
    // panels: it is a surface over somebody's work, not over the system.
    panel.set_level(PanelLevel::Floating.into());

    // Wherever the person is. A panel managed by Spaces would be tied to the
    // desktop it was opened on, which for a key pressed from any application is
    // the wrong desktop about as often as the right one — and
    // `full_screen_auxiliary` is what lets it draw over an application that has
    // taken the whole screen.
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );

    Ok(())
}

/// Whether the surface is up, taking the keyboard without taking the front.
#[cfg(target_os = "macos")]
fn showing<R: Runtime>(app: &AppHandle<R>) -> bool {
    use tauri_nspanel::ManagerExt as _;

    app.get_webview_panel(LABEL)
        .is_ok_and(|panel| panel.is_visible())
}

#[cfg(not(target_os = "macos"))]
fn showing<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// Show it and give it the keyboard.
#[cfg(target_os = "macos")]
fn reveal<R: Runtime>(app: &AppHandle<R>) {
    use tauri_nspanel::ManagerExt as _;

    if let Ok(panel) = app.get_webview_panel(LABEL) {
        panel.show_and_make_key();
    }
}

#[cfg(not(target_os = "macos"))]
fn reveal<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Put it away, leaving whatever is underneath with the keyboard.
#[cfg(target_os = "macos")]
fn conceal<R: Runtime>(app: &AppHandle<R>) {
    use tauri_nspanel::ManagerExt as _;

    if let Ok(panel) = app.get_webview_panel(LABEL) {
        panel.hide();
    }
}

#[cfg(not(target_os = "macos"))]
fn conceal<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// Show the panel, or put it away when it is already up.
///
/// The key is a toggle rather than an opener because that is what a key pressed
/// twice means. Pressed while the panel is up it is somebody changing their
/// mind, and reaching for Escape to undo a keystroke is a worse answer than the
/// same key again.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    // A panel is AppKit, and the shortcut's handler does not run on the main
    // thread. Touching a window from anywhere else on this platform is
    // undefined rather than slow.
    let _ = app.clone().run_on_main_thread(move || {
        if showing(&app) {
            conceal(&app);
            return;
        }

        // Before the panel takes the focus, which is the whole reason this is
        // read here and not from the frontend.
        if let Some(asking) = app.try_state::<Asking>() {
            asking.noticed(crate::windows::foremost(&app));
        }

        place(&app);
        reveal(&app);
        // After showing, so a panel that is being opened for the first time has
        // a webview to hear it.
        let _ = app.emit_to(LABEL, OPENED, ());
    });
}

/// Put the panel on the screen the person is working on.
///
/// The screen under the pointer rather than the main one. A key pressed from
/// another application says nothing about which display that application is on,
/// and the pointer is the only thing on this machine that does — somebody
/// typing on the left-hand screen has their hand on the mouse that is there.
fn place<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };

    let screen = app
        .cursor_position()
        .ok()
        .and_then(|at| app.monitor_from_point(at.x, at.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());

    let Some(screen) = screen else {
        // No display answered, which is not a reason to leave the panel
        // wherever it was last.
        let _ = window.center();
        return;
    };

    let Ok(size) = window.outer_size() else {
        let _ = window.center();
        return;
    };

    let area = screen.size();
    let origin = screen.position();
    let x = origin.x + (i64::from(area.width) - i64::from(size.width)) as i32 / 2;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a share of a screen's height in pixels is far inside an i32"
    )]
    let y = origin.y + (f64::from(area.height) * FROM_TOP) as i32;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Put the panel away.
///
/// Asked for by the panel itself — Escape, a line that has been taken, a click
/// outside it — rather than decided here. What closes it is a question about the
/// surface, and the surface is the frontend's.
#[tauri::command]
pub fn ask_dismiss<R: Runtime>(app: AppHandle<R>) {
    let _ = app.clone().run_on_main_thread(move || conceal(&app));
}

/// The project the panel is speaking for, as the panel needs to show it.
///
/// `None` means nobody had a project open when the key was pressed, and the
/// panel asks before it takes a line. The name travels with the path because
/// the panel draws it: a path is what the machine needs and a name is what a
/// person recognises, and the two are kept together everywhere else in this
/// application for the same reason.
///
/// # Errors
///
/// Never. A panel that could not read this shows the list of projects, which is
/// what `None` already means.
#[tauri::command]
pub fn ask_project<R: Runtime>(app: AppHandle<R>) -> Result<Option<RecentProject>, String> {
    let Some(path) = app.try_state::<Asking>().and_then(|asking| asking.read()) else {
        return Ok(None);
    };

    // Named from the recent list rather than from the folder, so the panel says
    // what the rest of the application says. A project that has fallen off that
    // list — eight entries — is named by its folder, which is what it was
    // called before anybody renamed it.
    let name = crate::project::recent_projects_load(app)
        .into_iter()
        .find(|entry| entry.path == path.to_string_lossy())
        .map(|entry| entry.name)
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    Ok(Some(RecentProject {
        path: path.to_string_lossy().into_owned(),
        name,
    }))
}

/// Speak for this project from now on.
///
/// What somebody chose in the panel, which outranks what was in front when the
/// key was pressed: they said it out loud, and the other was inferred. It is
/// kept rather than used once — a person who switched the panel to a project
/// means the next line too.
///
/// # Errors
///
/// Never. The value is held in memory and nothing about writing it can fail.
#[tauri::command]
pub fn ask_use<R: Runtime>(app: AppHandle<R>, project: String) -> Result<(), String> {
    if let Some(asking) = app.try_state::<Asking>() {
        asking.chose(PathBuf::from(project));
    }
    Ok(())
}

/// Make the panel exactly as tall as what it is showing.
///
/// Not a convenience. The window is transparent, so everything it covers and
/// does not draw is a region where a click lands on nothing — a dead band over
/// somebody else's application. A panel the height of its content has no such
/// band, which is also what makes clicking outside it reach the thing
/// underneath.
///
/// # Errors
///
/// Reports what the platform refused, which the panel has nothing to do about
/// beyond saying so.
#[tauri::command]
pub fn ask_height<R: Runtime>(app: AppHandle<R>, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    window
        .set_size(tauri::LogicalSize::new(WIDTH, height.max(HEIGHT)))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Asking;

    /// The panel is opened against the same project many times, so the value is
    /// read rather than taken — the opposite of the settings window's errand.
    #[test]
    fn what_the_panel_speaks_for_survives_being_read() {
        let asking = Asking::default();
        asking.noticed(Some(PathBuf::from("/tmp/project")));

        assert_eq!(asking.read(), Some(PathBuf::from("/tmp/project")));
        assert_eq!(
            asking.read(),
            Some(PathBuf::from("/tmp/project")),
            "the second line typed into the panel would have nowhere to go",
        );
    }

    /// Nobody with a project open is the ordinary state of an application in the
    /// menu bar, and the panel has to be able to say so.
    #[test]
    fn nothing_in_front_is_an_answer_rather_than_a_failure() {
        let asking = Asking::default();
        assert_eq!(asking.read(), None);

        asking.noticed(Some(PathBuf::from("/tmp/project")));
        asking.noticed(None);
        assert_eq!(
            asking.read(),
            None,
            "a panel opened with every window closed must not speak for whatever was open last",
        );
    }

    /// The defect this pair of values exists to prevent: the key is pressed, the
    /// window in front is read again, and what somebody chose is still what the
    /// panel speaks for.
    #[test]
    fn what_somebody_chose_outlasts_every_later_press_of_the_key() {
        let asking = Asking::default();
        asking.noticed(Some(PathBuf::from("/tmp/one")));
        asking.chose(PathBuf::from("/tmp/two"));

        asking.noticed(Some(PathBuf::from("/tmp/three")));
        asking.noticed(None);

        assert_eq!(
            asking.read(),
            Some(PathBuf::from("/tmp/two")),
            "a project chosen out of the list lasted until the next keystroke",
        );
    }

    /// And choosing again replaces it, which is the only thing that may.
    #[test]
    fn choosing_again_is_what_moves_it() {
        let asking = Asking::default();
        asking.chose(PathBuf::from("/tmp/one"));
        asking.chose(PathBuf::from("/tmp/two"));

        assert_eq!(asking.read(), Some(PathBuf::from("/tmp/two")));
    }
}
