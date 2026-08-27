//! Updating, in the background and without asking.
//!
//! Sync lives in the menu bar and is running whether or not anybody is looking
//! at it, which is what makes a silent update the honest shape here: there is
//! no moment in a session that is a good one to interrupt with a question about
//! infrastructure. So the check, the download and the install all happen behind
//! the window, and the only thing a person is ever asked is whether to restart
//! now — and even that is offered rather than raised, as a menu item that
//! appears once there is something to restart into.
//!
//! **Nothing here is reachable from the webview.** The updater plugin's
//! commands are not in any capability, so a record's body cannot ask this
//! application to fetch and run a bundle. The whole flow is Rust, started once
//! from `setup`.
//!
//! The bundle is verified before it is installed: every artifact is signed with
//! a minisign key whose public half is compiled into this binary, and the
//! updater refuses anything that does not verify. That is what makes fetching
//! over plain HTTPS from a static file safe — the file says where to download
//! from, but it cannot say what is trusted.

use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt as _;

/// Look for an update, and install one if it is there.
///
/// Returns immediately; the work is a task. Called once, from `setup`, so a
/// launch is never waiting on a network request — an endpoint that is slow, or
/// a machine that is offline, must cost the window nothing.
pub fn in_the_background<R: Runtime>(app: &AppHandle<R>) {
    // Not in development, and this is ours to decide: the plugin checks in a
    // debug build like any other, and installing there would replace a bundle
    // that is not one — `tauri dev` runs a bare binary out of `target/`. The
    // rehearsal in docs/releasing.md is a release build for that reason.
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match install_if_there_is_one(&app).await {
            Ok(Some(version)) => {
                crate::tray::an_update_is_waiting(&app, &version);
                eprintln!("an update to {version} is installed and waiting for a restart");
            }
            Ok(None) => {}
            // Reported and dropped. Every reason this fails — no network, a
            // rate-limited endpoint, a manifest that is mid-publish — is
            // temporary and answered by the next launch, and none of them is
            // something to put in front of a person who did not ask.
            Err(error) => eprintln!("the update check did not finish: {error}"),
        }
    });
}

/// The whole flow, so the caller above is only the reporting.
///
/// Answers the version that was installed, or nothing when this is already the
/// newest one.
async fn install_if_there_is_one<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri_plugin_updater::Result<Option<String>> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(None);
    };
    let version = update.version.clone();
    // Both callbacks are required by the signature and neither has anywhere to
    // report to: this download has no progress bar, by design.
    update.download_and_install(|_, _| {}, || {}).await?;
    Ok(Some(version))
}
