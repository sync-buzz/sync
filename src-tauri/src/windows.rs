//! The windows a person works in, and how the second one is made.
//!
//! Sync opens one window at launch and can open any number after it. A window
//! holds a project and nothing outside itself — the areas, the selection and
//! the columns are the window's, and the project is opened into it — so two
//! windows are two projects side by side rather than one interface drawn
//! twice. That is the whole reason this file exists: everything about a second
//! window is a label and a place to put it, and both are decided here.
//!
//! The label matters beyond bookkeeping. A capability is granted per label, so
//! a window built under a name the capability does not cover is a window whose
//! webview cannot show itself — `capabilities/default.json` names `main` and
//! `main-*`, and the labels below are spelled to match.
//!
//! Every window is built from the configured one in `tauri.conf.json`. Copying
//! its size, its material and its title bar into this file would be the same
//! window described in two places, and the second description would be the one
//! that goes stale.

use tauri::utils::config::WindowConfig;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

/// The label the window Tauri opens with carries, and the one a lone window
/// keeps: a person with one window open has `main`, whatever order the windows
/// before it were closed in.
pub const FIRST: &str = "main";

/// What the windows after it are called. The number is the first one free
/// rather than a running count, so closing `main-2` and opening another gives
/// `main-2` again instead of climbing forever.
const FOLLOWING: &str = "main-";

/// How far a new window sits from the one it was opened over.
///
/// A window placed exactly on top of another looks like the click did nothing,
/// and one placed far away looks like it belongs to something else. This is the
/// step the system itself cascades documents by.
const CASCADE: f64 = 28.0;

/// Open a window on nothing — the welcome screen, where a project is chosen.
///
/// # Errors
///
/// Reports what the platform refused. A window that could not be created is the
/// whole of the failure: nothing else was changed on the way to it.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let mut config = configured(app);
    config.label = free_label(app);
    match cascade(app) {
        Some((x, y)) => {
            config.x = Some(x);
            config.y = Some(y);
            config.center = false;
        }
        // Nowhere on this screen for the cascade to continue into, which is
        // what a person tiling windows to the corner has done. Centred is the
        // one answer that is never off the screen.
        None if !app.webview_windows().is_empty() => config.center = true,
        None => {}
    }

    let window = WebviewWindowBuilder::from_config(app, &config)?.build()?;
    // Before the window shows itself rather than after: a Dock icon that
    // arrives once the window is already up is a window that opened behind
    // everything else.
    follow_the_windows(app);
    Ok(window)
}

/// Open a window from the menu bar's File menu.
///
/// The error is a message rather than a kind, as the settings window's is:
/// there is one failure — the platform refused the window — and nothing for the
/// interface to branch on.
#[tauri::command]
pub fn window_new<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    open(&app).map(|_| ()).map_err(|error| error.to_string())
}

/// Name a window after the project it has open.
///
/// A command rather than `setTitle` from the window itself, because on macOS
/// naming a window is not only naming it: AppKit re-lays the title bar out and
/// the traffic lights go back where the system would have put them — measured,
/// the moment the title is set, the buttons jump from the configured 28pt inset
/// to the standard 9pt one. Tao holds them where `tauri.conf.json` asked for
/// only from the view's `drawRect`, which nothing calls after a rename, so they
/// stayed visibly out of place until the window was next resized.
///
/// Both halves therefore happen here, in that order, on one trip to the main
/// thread — and the title is set through AppKit rather than through
/// `WebviewWindow::set_title`, which is the whole trick: Tao's is asynchronous
/// even when it is already on the main thread (`DispatchQueue::main`), so a
/// re-inset written after it ran *before* the rename it was correcting.
///
/// The error is a message rather than a kind: there is one failure, and nothing
/// for the interface to branch on.
#[tauri::command]
pub fn window_named<R: Runtime>(window: WebviewWindow<R>, title: String) -> Result<(), String> {
    let inset = configured(window.app_handle()).traffic_light_position;
    let named = window.clone();

    window
        .run_on_main_thread(move || {
            #[cfg(target_os = "macos")]
            name_and_keep_the_traffic_lights(&named, &title, inset);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = inset;
                if let Err(error) = named.set_title(&title) {
                    eprintln!("the window could not be named: {error}");
                }
            }
        })
        .map_err(|error| error.to_string())
}

/// Name the window and put the traffic lights back, in that order and without
/// yielding the main thread between the two.
///
/// The inset arithmetic is the same Tao does from `drawRect`, deliberately: it
/// is idempotent — the spacing is read back off the buttons — so Tao's next
/// redraw lands them in exactly this place rather than moving them again.
///
/// Every failure here is a window whose title bar is not shaped the way this
/// expects, which is not a thing to report: the buttons are simply left alone.
#[cfg(target_os = "macos")]
fn name_and_keep_the_traffic_lights<R: Runtime>(
    window: &WebviewWindow<R>,
    title: &str,
    inset: Option<tauri::utils::config::LogicalPosition>,
) {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use objc2_foundation::NSString;

    let Ok(pointer) = window.ns_window() else {
        return;
    };
    // SAFETY: the pointer is this window's own `NSWindow`, and this runs on the
    // main thread — `run_on_main_thread` is what got here.
    let window: &NSWindow = unsafe { &*pointer.cast::<NSWindow>() };
    window.setTitle(&NSString::from_str(title));

    let Some(inset) = inset else {
        return;
    };
    let (Some(close), Some(miniaturize), Some(zoom)) = (
        window.standardWindowButton(NSWindowButton::CloseButton),
        window.standardWindowButton(NSWindowButton::MiniaturizeButton),
        window.standardWindowButton(NSWindowButton::ZoomButton),
    ) else {
        return;
    };
    // SAFETY: reading a view's superview is unsafe only in that it must happen
    // on the main thread, which is where this is.
    let container = unsafe { close.superview().and_then(|view| view.superview()) };
    let Some(container) = container else {
        return;
    };

    // The container is shortened to the buttons' new bottom edge and pinned to
    // the top of the window: it is what the title bar drags by, and one left at
    // its own height would be a drag region that no longer matches the buttons.
    let height = close.frame().size.height + inset.y;
    let mut frame = container.frame();
    frame.size.height = height;
    frame.origin.y = window.frame().size.height - height;
    container.setFrame(frame);

    let spacing = miniaturize.frame().origin.x - close.frame().origin.x;
    for (step, button) in [close, miniaturize, zoom].into_iter().enumerate() {
        let mut origin = button.frame().origin;
        origin.x = inset.x + (step as f64) * spacing;
        button.setFrameOrigin(origin);
    }
}

/// Bring Sync forward: the window that is already open, or a new one.
///
/// What the menu bar item's icon does, and what the Dock icon does when Sync is
/// running with every window closed. Both are a person asking for the
/// application rather than for another window of it, so an open window is
/// answered with itself — a second window nobody asked for would be a click
/// that quietly made work.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = frontmost(app) {
        // All three, because a window can be away in three ways: closed to the
        // Dock, ordered out, or simply behind something.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    if let Err(error) = open(app) {
        eprintln!("a window could not be opened: {error}");
    }
}

/// Show a Dock icon when there is a window, and none when there is not.
///
/// An application with no windows and a Dock icon is an application that looks
/// stuck; `Accessory` is what the system calls a thing that lives in the menu
/// bar, and it goes back to `Regular` the moment there is a window to belong to
/// a Dock icon again.
///
/// macOS only, because it is the only platform where the two states have names.
pub fn follow_the_windows<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let policy = if app.webview_windows().is_empty() {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The window as `tauri.conf.json` describes it, which every window is a copy
/// of. An installation whose configuration names no window is not one this can
/// invent a better answer for than the defaults.
fn configured<R: Runtime>(app: &AppHandle<R>) -> WindowConfig {
    app.config()
        .app
        .windows
        .first()
        .cloned()
        .unwrap_or_default()
}

/// The first label no window is using.
fn free_label<R: Runtime>(app: &AppHandle<R>) -> String {
    if app.get_webview_window(FIRST).is_none() {
        return FIRST.to_owned();
    }

    let mut number = 2;
    loop {
        let label = format!("{FOLLOWING}{number}");
        if app.get_webview_window(&label).is_none() {
            return label;
        }
        number += 1;
    }
}

/// Where a new window goes: one step down and across from the window it is
/// being opened over, or `None` when that step would put it off the screen.
///
/// The window it steps from is the focused one where there is one. Where there
/// is not — the Dock menu is used while Sync is in the background, and nothing
/// of it is focused then — it is the window furthest into the cascade already,
/// so the next one continues the run rather than landing back on top of it.
fn cascade<R: Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    let from = frontmost(app)?;
    let scale = from.scale_factor().ok()?;
    let at = from.outer_position().ok()?.to_logical::<f64>(scale);
    let size = from.outer_size().ok()?.to_logical::<f64>(scale);

    let screen = from.current_monitor().ok()??;
    let corner = screen.position().to_logical::<f64>(screen.scale_factor());
    let extent = screen.size().to_logical::<f64>(screen.scale_factor());

    let (x, y) = (at.x + CASCADE, at.y + CASCADE);
    // The title bar is enough of the window to be worth keeping on the screen;
    // asking for the whole of it would refuse the cascade for a window a person
    // had deliberately hung off the bottom edge.
    let fits = x + size.width <= corner.x + extent.width && y <= corner.y + extent.height - CASCADE;
    fits.then_some((x, y))
}

/// The window a person is looking at, or the one a new window should step from.
fn frontmost<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let windows = project_windows(app);
    if let Some(focused) = windows
        .iter()
        .find(|window| window.is_focused().unwrap_or(false))
    {
        return Some(focused.clone());
    }

    windows.into_iter().max_by_key(|window| {
        window
            .outer_position()
            .map(|at| i64::from(at.x) + i64::from(at.y))
            .unwrap_or(i64::MIN)
    })
}

/// Every window that holds a project, which is every window but the settings
/// one: settings belong to the installation, and a person who closed their last
/// project window and left settings open has no project window open.
fn project_windows<R: Runtime>(app: &AppHandle<R>) -> Vec<WebviewWindow<R>> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label == FIRST || label.starts_with(FOLLOWING))
        .map(|(_, window)| window)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The labels are the one thing here that another file has to agree with:
    /// `capabilities/default.json` grants `main` and `main-*`, and a window
    /// built under anything else is one whose webview cannot show itself. So
    /// the sequence is asserted rather than assumed.
    #[test]
    fn windows_are_labelled_main_and_then_main_2() {
        let app = tauri::test::mock_app();
        let handle = app.handle();

        assert_eq!(
            free_label(handle),
            "main",
            "the first window is the main one"
        );

        let first = open(handle).expect("a window");
        assert_eq!(first.label(), "main");

        let second = open(handle).expect("a second window");
        assert_eq!(second.label(), "main-2");

        let third = open(handle).expect("a third window");
        assert_eq!(third.label(), "main-3");
    }

    /// Settings is not a project window, so a person with only settings open
    /// has none — and asking for a window has to make one rather than hand
    /// back the settings window under another name.
    #[test]
    fn the_settings_window_is_not_a_project_window() {
        let app = tauri::test::mock_app();
        let handle = app.handle();
        crate::settings::settings_open(handle.clone()).expect("the settings window");

        assert!(
            project_windows(handle).is_empty(),
            "settings is the installation's window, not a project's"
        );
        assert_eq!(free_label(handle), "main");
    }
}
