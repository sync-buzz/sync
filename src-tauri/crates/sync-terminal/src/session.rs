//! One terminal: the pty, the process on the far end of it, and the thread
//! that reads it.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::error::{Error, Result};
use crate::scrollback::{Scrollback, Tail};

/// How much a single read from the pty may bring back.
///
/// A screenful of colour is a few kilobytes; a build log arrives in as many
/// of these as it takes. Larger buys nothing, because the ring is what bounds
/// what is kept.
const READ_CHUNK: usize = 8 * 1024;

/// A lock that survives a panic on the other side of it.
///
/// Poisoning is the standard library reporting that some thread panicked while
/// holding this, and neither thing we hold has an invariant a panic could have
/// left half-applied: the ring is bytes and an offset. Refusing to read a
/// terminal because an unrelated thread fell over is a worse answer than
/// reading it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How large the screen is, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

impl Size {
    /// Neither dimension may be zero.
    ///
    /// The window measures the box it drew, and a box that has not been laid
    /// out yet — or that is sitting behind another tab — measures nothing. A
    /// pty of zero columns makes every program that divides by the width fault
    /// on its first line of output, so a measurement that cannot be true is
    /// raised to one instead of being passed on.
    fn to_pty(self) -> PtySize {
        PtySize {
            rows: self.rows.max(1),
            cols: self.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// How a process on the far end finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exit {
    pub code: u32,
    pub signal: Option<String>,
}

/// What to open.
#[derive(Debug, Clone)]
pub struct Opening {
    /// Where the shell starts. Refused if it is not a folder.
    pub cwd: PathBuf,
    pub size: Size,
    /// What to run instead of the person's login shell.
    ///
    /// Empty for the ordinary case. It is here because a test needs a process
    /// that says a known thing and exits, and because the login shell is a
    /// default rather than a rule.
    pub program: Vec<String>,
    /// Set on top of what the shell would inherit.
    pub env: Vec<(String, String)>,
}

/// What every reader of one terminal shares.
struct Shared {
    scrollback: Mutex<Scrollback>,
    /// The offset just past the last byte that has arrived.
    ///
    /// A watch rather than a broadcast of the bytes themselves. A broadcast
    /// channel drops for a receiver that falls behind, and bytes missing out of
    /// the middle of a terminal stream are a corrupted screen with nothing to
    /// say it happened. This says only *there is more*; what a reader is owed
    /// it takes from the ring, by an offset it names itself.
    progress: watch::Sender<u64>,
    exit: watch::Sender<Option<Exit>>,
}

/// One terminal.
pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    shared: Arc<Shared>,
}

// Neither a pty master nor a writer is `Debug`, and the layer above holds
// these inside things that are. What is worth printing is what a person would
// ask about anyway: how far it has got, and whether it is over.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("read", &*self.shared.progress.borrow())
            .field("exit", &*self.shared.exit.borrow())
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(crate) fn open(opening: &Opening, capacity: usize) -> Result<Self> {
        if !opening.cwd.is_dir() {
            return Err(Error::NoSuchFolder(opening.cwd.clone()));
        }

        let pair = native_pty_system()
            .openpty(opening.size.to_pty())
            .map_err(|source| Error::Open(source.to_string()))?;

        let mut command = if opening.program.is_empty() {
            // The person's login shell, resolved from `SHELL` and then from the
            // password database, and run *as* a login shell.
            //
            // Login matters more here than it looks. An application started
            // from the Dock inherits none of what a profile sets — a version
            // manager's shims, a package manager's prefix — so a shell that did
            // not read the profile is a shell whose `PATH` is missing the
            // tools the person installed, in a window that otherwise looks
            // exactly right.
            CommandBuilder::new_default_prog()
        } else {
            let mut builder = CommandBuilder::new(&opening.program[0]);
            builder.args(&opening.program[1..]);
            builder
        };
        command.cwd(&opening.cwd);
        // What the far end is told it is talking to. `xterm-256color` is what
        // the emulator in the window implements; claiming anything richer means
        // programs sending sequences that are drawn as text.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Sync");
        for (key, value) in &opening.env {
            command.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|source| Error::Open(source.to_string()))?;

        // **The slave is dropped here, and the order is load-bearing.** The
        // child has its own handle on it now; ours is a second one, and while
        // any handle is open the kernel has no reason to report the far end
        // closed. Keeping it would mean a shell that exits leaves a terminal
        // that never ends, a thread parked on a read that never returns, and
        // nothing anywhere to say why.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|source| Error::Open(source.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|source| Error::Open(source.to_string()))?;
        let killer = child.clone_killer();

        let shared = Arc::new(Shared {
            scrollback: Mutex::new(Scrollback::new(capacity)),
            progress: watch::Sender::new(0),
            exit: watch::Sender::new(None),
        });

        let pumping = Arc::clone(&shared);
        // A thread of the operating system rather than a task. `try_clone_reader`
        // hands back a blocking `Read`, and a shell is quiet nearly all of the
        // time — parked on it inside an async runtime, this would hold a worker
        // hostage for as long as nobody is typing.
        std::thread::Builder::new()
            .name("sync-terminal".into())
            .spawn(move || pump(reader, child, &pumping))
            .map_err(|source| Error::Open(source.to_string()))?;

        Ok(Self {
            master: pair.master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            shared,
        })
    }

    /// What was typed, on its way to the process.
    ///
    /// # Errors
    ///
    /// When the process has already ended, and when the pty refuses the write.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        if self.shared.exit.borrow().is_some() {
            return Err(Error::Ended);
        }
        let mut writer = lock(&self.writer);
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Tell the far end the screen changed size.
    ///
    /// This is not decoration: the kernel keeps the size, and changing it is
    /// what raises `SIGWINCH` — which is how a full-screen program learns to
    /// redraw itself rather than staying wrapped to a width that is gone.
    ///
    /// # Errors
    ///
    /// When the system refuses to set the size.
    pub fn resize(&self, size: Size) -> Result<()> {
        self.master
            .resize(size.to_pty())
            .map_err(|source| Error::Open(source.to_string()))
    }

    /// Everything said since an offset the caller names.
    #[must_use]
    pub fn since(&self, offset: u64) -> Tail {
        lock(&self.shared.scrollback).since(offset)
    }

    /// How the process finished, or `None` while it is still running.
    #[must_use]
    pub fn exit(&self) -> Option<Exit> {
        self.shared.exit.borrow().clone()
    }

    /// Wakes whenever there is more to read, and once more when it has ended.
    #[must_use]
    pub fn progress(&self) -> watch::Receiver<u64> {
        self.shared.progress.subscribe()
    }

    /// End it.
    ///
    /// This kills the shell. A process the person put in the background with
    /// `&` outlives it as an orphan, because what is signalled is the child
    /// rather than the process group it leads.
    pub fn close(&self) {
        let _ = lock(&self.killer).kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// Read the pty until the far end is gone, then reap.
fn pump(
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    shared: &Shared,
) {
    let mut buffer = [0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buffer) {
            // Zero on some systems, `EIO` on this one: when the last handle on
            // the far end goes, macOS reports reading a pty with no slave as an
            // error rather than as end of file. Both mean the same thing, and
            // treating the error as a fault would put a diagnostic in the log
            // every time somebody typed `exit`.
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let end = lock(&shared.scrollback).push(&buffer[..read]);
                shared.progress.send_replace(end);
            }
        }
    }

    let exit = match child.wait() {
        Ok(status) => Exit {
            code: status.exit_code(),
            signal: status.signal().map(ToOwned::to_owned),
        },
        // Waiting can only fail if the child was already reaped elsewhere, and
        // there is nobody else here to reap it. Reporting *ended* with a code
        // nobody can act on beats leaving the window waiting for ever.
        Err(_) => Exit {
            code: 0,
            signal: None,
        },
    };
    shared.exit.send_replace(Some(exit));
    // Wake the readers once more, so that whoever is waiting on there being
    // more notices there will not be.
    let end = lock(&shared.scrollback).end();
    shared.progress.send_replace(end);
}
