//! Terminals: who may ask for one, and what the window is told.
//!
//! The process, the pty and the tail of what was said are in `sync-terminal`,
//! which knows nothing of this application. What is decided here is the half a
//! crate must not decide for itself: whether the package asking has the
//! capability, which project the terminal belongs to, and how bytes cross into
//! the window.
//!
//! # Where the check is, and what stands in for it after
//!
//! The **capability** is read when a terminal is opened, and only then. Reading
//! it means resolving the package on disk and parsing its manifest, and doing
//! that per keystroke would put a file read behind every character somebody
//! types.
//!
//! Every call after that names the package too, and the crate refuses a
//! terminal to anybody but the one that raised it. That is as strong as reading
//! the manifest again and costs a string comparison: a terminal can only exist
//! because somebody with the capability opened it, so *the same somebody* is
//! the whole of what a second reading would establish.
//!
//! **It has to be this way round rather than trusting the name.** A terminal's
//! name is a counter, so a package that asked for nothing could otherwise guess
//! one and write into a shell somebody else opened — and writing into a shell
//! is running a command in it.
//!
//! What this does not do is hold apart two packages that lie about which they
//! are. Every extension in this window shares one origin, so what separates
//! them is the door each was handed rather than a wall the webview puts between
//! them — which is true of `net` and `vault` as well.
//!
//! # A terminal belongs to the project, not to the section that opened it
//!
//! An area is mounted on first visit and never unmounted, but it can be left,
//! hidden or reloaded, and none of those is a reason for a build to stop. So
//! the owner is the project: closing that closes them, and nothing else does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use sync_terminal::{Opening, Session, Size, TerminalId, TerminalRow, Terminals};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, Runtime, State};

use sync_extensions::manifest::TERMINAL_CAPABILITY;

use crate::extensions::permitted;

/// Which watcher of each terminal is the current one.
///
/// **A terminal is watched by one screen at a time**, which is what a terminal
/// is anywhere else on this system and what the window needs: one tile draws
/// one of them. Watching again retires whoever was watching before, so a
/// section that re-attaches after a reload leaves nothing behind — without
/// this, every reattachment would add a task that wakes on every byte and
/// encodes it for a channel nobody is reading.
#[derive(Default)]
pub struct Watchers(Mutex<HashMap<TerminalId, u64>>);

impl Watchers {
    /// Become the current watcher of a terminal, and answer to what.
    fn claim(&self, id: &TerminalId) -> u64 {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let turn = held.entry(id.clone()).or_default();
        *turn += 1;
        *turn
    }

    /// Whether a watcher is still the current one.
    fn holds(&self, id: &TerminalId, turn: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            == Some(&turn)
    }
}

/// What the window is told, as it happens.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent {
    /// Bytes the process wrote.
    ///
    /// Base64 rather than an array of numbers. The same bytes as JSON numbers
    /// are four times the size on the wire and arrive as a JavaScript array
    /// that has to be walked and copied; a terminal that prints a build log
    /// would spend the window's frame budget on the shape of the message. Not
    /// a string either: a chunk can end in the middle of a character, and the
    /// lossy conversion that hides that replaces the tail with a question mark
    /// the next chunk cannot repair.
    Output {
        from: u64,
        to: u64,
        /// Something was dropped between what was asked for and what came back.
        ///
        /// Told rather than smoothed over: what the window draws after a gap is
        /// its own decision, and bytes missing from the middle of a terminal
        /// stream are a corrupted screen with nothing to say why.
        gapped: bool,
        base64: String,
    },
    /// The process finished. Nothing follows this.
    Ended { code: u32, signal: Option<String> },
    /// The terminal was closed while somebody was watching it.
    Gone,
}

/// Raise a terminal in a project's folder.
///
/// # Errors
///
/// When the package did not ask for the capability, when the folder is not one,
/// or when the system refuses to open a pty.
#[tauri::command(async)]
pub async fn terminal_open<R: Runtime>(
    app: AppHandle<R>,
    extension: String,
    project: String,
    cwd: String,
    size: Size,
) -> Result<TerminalId, String> {
    let checking = app.clone();
    let asking = extension.clone();
    // `permitted` reads the disk, so it goes to the blocking pool — the same
    // road `extension_fetch` takes to the same function.
    tauri::async_runtime::spawn_blocking(move || {
        permitted(&checking, &asking, TERMINAL_CAPABILITY)
    })
    .await
    .map_err(|error| format!("the terminal did not open: {error}"))??;

    let opening = Opening {
        cwd: PathBuf::from(cwd),
        size,
        // The person's login shell, and nothing said about it here. What to run
        // is the crate's default rather than this layer's choice, so that the
        // window cannot be asked to run a command by whoever is talking to it.
        program: Vec::new(),
        env: Vec::new(),
    };
    app.state::<Terminals>()
        .open(&project, &extension, &opening)
        .map_err(|error| error.to_string())
}

/// What was typed, on its way to the process.
///
/// # Errors
///
/// When nothing is open under that name, or the process has ended.
#[tauri::command(async)]
pub fn terminal_write(
    terminals: State<'_, Terminals>,
    extension: String,
    id: TerminalId,
    data: String,
) -> Result<(), String> {
    terminals
        .with(&id, &extension, |session| session.write(data.as_bytes()))
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// The screen changed size, and the far end has to hear about it.
///
/// # Errors
///
/// When nothing is open under that name, or the system refuses the size.
#[tauri::command(async)]
pub fn terminal_resize(
    terminals: State<'_, Terminals>,
    extension: String,
    id: TerminalId,
    size: Size,
) -> Result<(), String> {
    terminals
        .with(&id, &extension, |session| session.resize(size))
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Watch a terminal from an offset the caller names: everything since, then
/// everything after.
///
/// Answers as soon as the watching has started rather than when it is over.
///
/// # Errors
///
/// When nothing is open under that name.
#[tauri::command(async)]
pub fn terminal_watch<R: Runtime>(
    app: AppHandle<R>,
    extension: String,
    id: TerminalId,
    from: u64,
    events: Channel<TerminalEvent>,
) -> Result<(), String> {
    // Established before the task starts, so that watching something that is
    // not there is a refusal the caller can act on rather than a `Gone` it has
    // to distinguish from a terminal that closed a moment later.
    let mut progress = app
        .state::<Terminals>()
        .with(&id, &extension, Session::progress)
        .map_err(|error| error.to_string())?;
    let turn = app.state::<Watchers>().claim(&id);

    tauri::async_runtime::spawn(async move {
        let mut offset = from;
        loop {
            // Somebody else took this terminal's screen. Said nothing to the
            // channel on the way out: whoever replaced this watcher is already
            // drawing, and a `Gone` arriving now would be read as the terminal
            // having ended.
            if !app.state::<Watchers>().holds(&id, turn) {
                return;
            }

            // Read *before* draining, and the order is the whole of the
            // correctness here. The reading thread sets the exit only once it
            // has stopped reading, so an exit seen before this drain means
            // everything the process ever said is already in the ring. Read
            // after, and a chunk that landed between the two would be delivered
            // to nobody.
            let ended = match app
                .state::<Terminals>()
                .with(&id, &extension, Session::exit)
            {
                Ok(ended) => ended,
                Err(_) => {
                    let _ = events.send(TerminalEvent::Gone);
                    return;
                }
            };

            let Ok(tail) = app
                .state::<Terminals>()
                .with(&id, &extension, |session| session.since(offset))
            else {
                let _ = events.send(TerminalEvent::Gone);
                return;
            };

            if !tail.bytes.is_empty() {
                let event = TerminalEvent::Output {
                    from: tail.from,
                    to: tail.to,
                    gapped: tail.is_gapped(offset),
                    base64: BASE64.encode(&tail.bytes),
                };
                offset = tail.to;
                // A send that fails is the window having gone away. The process
                // is unaffected, which is the point of it living where it does.
                if events.send(event).is_err() {
                    return;
                }
            }

            if let Some(exit) = ended {
                let _ = events.send(TerminalEvent::Ended {
                    code: exit.code,
                    signal: exit.signal,
                });
                return;
            }

            if progress.changed().await.is_err() {
                return;
            }
        }
    });

    Ok(())
}

/// What a project has open.
#[tauri::command(async)]
pub fn terminal_list(
    terminals: State<'_, Terminals>,
    extension: String,
    project: String,
) -> Vec<TerminalRow> {
    terminals.list(&project, &extension)
}

/// End one.
#[tauri::command(async)]
pub fn terminal_close(terminals: State<'_, Terminals>, extension: String, id: TerminalId) {
    terminals.close(&id, &extension);
}

/// End everything a project has open.
///
/// Called when the project is closed. It is here rather than left to the window
/// closing its terminals one by one because the window that is going away is
/// the one that would have to remember.
#[tauri::command(async)]
pub fn terminal_close_project(terminals: State<'_, Terminals>, extension: String, project: String) {
    terminals.close_owned_by(&project, &extension);
}
