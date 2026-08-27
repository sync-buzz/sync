//! The menu bar item, and what keeps the application alive behind it.
//!
//! Sync serves agents whether or not anybody is looking at it, so closing the
//! last window has to mean "put it away" rather than "stop". That is the whole
//! reason this file exists: without it, the server would end the moment a
//! person closed a window they were finished with.
//!
//! The windows it puts back are `crate::windows`' — this file knows how to ask
//! for one and nothing about how one is made.

use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt as _;

/// Put Sync in the menu bar.
///
/// # Errors
///
/// Reports whatever the platform refused, which is the item itself failing to
/// be created — the application still runs, and this is reported rather than
/// fatal.
pub fn install(app: &App) -> tauri::Result<()> {
    let menu = menu(app, None)?;

    TrayIconBuilder::with_id("sync")
        // The glyph on nothing, not the application icon.
        //
        // A template image is a *silhouette*: macOS keeps the alpha channel,
        // throws the colour away and repaints it for a light or dark bar. The
        // application icon cannot serve as one — its plate is opaque edge to
        // edge, so its silhouette is a filled square, which is what the bar
        // showed. `trayTemplate@2x.png` is the same glyph drawn on nothing, at
        // the density a Retina bar draws; `pnpm icons` writes it from the same
        // source SVG as every other icon, so the mark cannot drift from the
        // one on the plate.
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/trayTemplate@2x.png"
        ))?)
        .icon_as_template(true)
        .tooltip("Sync — memory for your projects")
        .menu(&menu)
        // The primary click is the window, not the menu.
        //
        // Sync has a window and the icon is how a person gets back to it, so
        // the click that costs nothing to make is spent on the thing they came
        // for. The menu is still one click away under the secondary button,
        // which is where this system keeps what an icon can do besides its
        // obvious thing.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(clicked)
        .on_menu_event(chosen)
        .build(app)?;
    Ok(())
}

/// The menu behind the icon.
///
/// `waiting` names the version an installed update will start into, and is the
/// only thing that varies: when there is one, the menu grows a restart item at
/// the top. It is built rather than enabled — a permanently greyed "Restart to
/// Update" is a promise the menu makes at every launch and keeps almost never,
/// while an item that is simply not there until it means something says
/// nothing false.
fn menu<R: Runtime, M: Manager<R>>(app: &M, waiting: Option<&str>) -> tauri::Result<Menu<R>> {
    let open = MenuItem::with_id(app, "open", "Open Sync", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("Cmd+,"))?;
    let at_login = CheckMenuItem::with_id(
        app,
        "at-login",
        "Start at Login",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Sync", true, Some("Cmd+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &at_login,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    if let Some(version) = waiting {
        let restart = MenuItem::with_id(
            app,
            "restart",
            format!("Restart to Update to {version}"),
            true,
            None::<&str>,
        )?;
        menu.insert_items(&[&restart, &PredefinedMenuItem::separator(app)?], 0)?;
    }
    Ok(menu)
}

/// Say, in the menu, that a downloaded update is in place.
///
/// The whole of what the background updater ever puts in front of anybody. It
/// offers rather than interrupts: the next launch is the new version whether or
/// not this is ever clicked, and Sync is an application people leave running,
/// so "restart now" is worth having somewhere.
pub fn an_update_is_waiting<R: Runtime>(app: &AppHandle<R>, version: &str) {
    let Some(tray) = app.tray_by_id("sync") else {
        return;
    };
    match menu(app, Some(version)) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => eprintln!("the menu could not say an update is waiting: {error}"),
    }
}

/// Do what the menu was asked for.
fn chosen<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => crate::windows::show(app),
        "settings" => {
            let _ = crate::settings::settings_open(app.clone());
        }
        "at-login" => {
            // Read back rather than remembered: the tick is the system's answer
            // about the login item, and the system is where a person may also
            // have turned it off.
            let launcher = app.autolaunch();
            let _ = if launcher.is_enabled().unwrap_or(false) {
                launcher.disable()
            } else {
                launcher.enable()
            };
        }
        // The one thing the background updater ever asks of anybody, and it
        // asks by being available rather than by interrupting.
        "restart" => app.restart(),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Answer a click on the icon with the window.
///
/// Only the release, and only of the primary button: a press is not yet a
/// click — a person can still slide off the icon — and the secondary button is
/// the menu's, which the system puts up without asking this.
fn clicked<R: Runtime>(tray: &tauri::tray::TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        crate::windows::show(tray.app_handle());
    }
}
