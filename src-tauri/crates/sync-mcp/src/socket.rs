//! The host channel's door, for the window and for the clock.
//!
//! The second door on the resident process, and the one Sync itself comes
//! through. What it serves is [`crate::host::Host`] — the same dispatcher the
//! stdio door serves, unchanged, because `host.rs` never knew its transport.
//!
//! # Why a socket rather than the port the agents use
//!
//! The MCP door listens on a fixed TCP port, and a port already taken is
//! reported rather than stepped around — `lib.rs` treats a server that did not
//! start as survivable precisely because the window did not depend on it. Put
//! the window's memory on that port and whatever else is listening on 41847
//! takes the whole product down with it.
//!
//! A socket in the application's own directory cannot collide with anything,
//! and its permissions are the whole of its access control. The token exists
//! because an agent is configured with a URL; the window is configured with
//! nothing and needs none.
//!
//! # One connection, one project
//!
//! A connection names its project once, with `project.attach`, and every
//! message after that is **exactly** what the stdio door has always carried —
//! same methods, same params, same errors. That is deliberate: putting the
//! project on every call would have meant taking it off the params again before
//! the operations saw it, and threading it through the one piece of per-project
//! state the client keeps — the revision it expects its next write to be
//! against. A connection is a file descriptor, not an engine, so a window with
//! four projects open holds four of them against one process.
//!
//! # What runs where
//!
//! Every dispatch reaches Git, `LanceDB` and possibly a model, so it goes to a
//! blocking thread. The alternative is a call in one project stalling the
//! runtime that the agents' door is sharing.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use sync_memory::ATTACH;

use crate::host::Host;

/// Serve the host channel on `path` until the process ends.
///
/// # Errors
///
/// When the socket cannot be bound — including when another process is already
/// listening on it, which is a machine running two copies of Sync rather than a
/// state to recover from.
pub async fn serve(host: Arc<Host>, path: PathBuf) -> io::Result<()> {
    let listener = bind(&path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let host = Arc::clone(&host);
        // Per connection, because one window has several projects open and a
        // call in one of them must not be behind a call in another.
        tokio::spawn(async move {
            if let Err(error) = attend(&host, stream).await {
                // A connection ending is ordinary — the window closed a project
                // — so this is only worth a line when it ended for a reason.
                if error.kind() != io::ErrorKind::UnexpectedEof {
                    eprintln!("a host connection ended: {error}");
                }
            }
        });
    }
}

/// Take the socket, refusing to steal one another process is using.
///
/// A socket file outlives the process that made it, so binding means deciding
/// what an existing one is. Connecting to it is the only way to tell: an answer
/// means somebody is alive and this process must not take their door away, and
/// a refusal means the file is what a crash left behind.
fn bind(path: &Path) -> io::Result<UnixListener> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "another process is already serving the host channel on {}",
                    path.display()
                ),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let listener = UnixListener::bind(path)?;
    // The permissions are the access control, so they are set rather than left
    // to whatever umask this process was started with.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    // Written beside the door, because whoever wants this door next has to be
    // able to take it. A socket says somebody is there and not who, and Sync's
    // rule is that this process ends when Sync does — so the next Sync needs a
    // way to end one that outlived its own.
    let _ = std::fs::write(pid_file(path), std::process::id().to_string());
    Ok(listener)
}

/// Where the process serving `socket` writes its own process id.
#[must_use]
pub fn pid_file(socket: &Path) -> PathBuf {
    socket.with_extension("pid")
}

/// Read one connection to its end, answering every line.
async fn attend(host: &Arc<Host>, stream: UnixStream) -> io::Result<()> {
    let (reading, mut writing) = stream.into_split();
    let mut lines = BufReader::new(reading).lines();
    let mut attached: Option<PathBuf> = None;

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        // Answered, not fatal, exactly as the stdio door answers it: a line
        // that is not JSON is a caller's mistake, and ending the connection
        // over it would report the same class of mistake two different ways.
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            write(&mut writing, &malformed()).await?;
            continue;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        if method == ATTACH {
            let answer = match params.get("path").and_then(Value::as_str) {
                Some(path) => {
                    attached = Some(PathBuf::from(path));
                    json!({"jsonrpc": "2.0", "id": id, "result": {"attached": path}})
                }
                None => refusal(&id, "`project.attach` needs the path of a project"),
            };
            write(&mut writing, &answer).await?;
            continue;
        }

        let Some(project) = attached.clone() else {
            // Said rather than answered from a guess. There is no default
            // project and inventing one would answer a call meant for one
            // repository out of another.
            let answer = refusal(
                &id,
                "this connection has not said which project it is about — call `project.attach` first",
            );
            write(&mut writing, &answer).await?;
            continue;
        };

        let host = Arc::clone(host);
        let method = method.to_owned();
        // Off the runtime: a dispatch reaches Git, LanceDB and possibly a
        // model, and the agents' door shares these threads.
        let answered =
            tokio::task::spawn_blocking(move || host.dispatch(&project, &method, &params))
                .await
                .map_err(|error| {
                    io::Error::other(format!("a host call could not be run: {error}"))
                })?;

        let answer = match answered {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            // The engine's own `kind` travels in `data`, so the window can tell
            // a stale revision from a locked project without reading prose.
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32000,
                "message": error.to_string(),
                "data": {"kind": error.kind().map(sync_memory::MemoryErrorKind::as_wire)},
            }}),
        };
        write(&mut writing, &answer).await?;
    }
    Ok(())
}

async fn write(stream: &mut tokio::net::unix::OwnedWriteHalf, answer: &Value) -> io::Result<()> {
    stream.write_all(format!("{answer}\n").as_bytes()).await?;
    stream.flush().await
}

/// There is no id to answer with, so it is `null`, which is what JSON-RPC says
/// to do.
fn malformed() -> Value {
    json!({"jsonrpc": "2.0", "id": Value::Null, "error": {
        "code": -32700,
        "message": "the request could not be read as JSON",
    }})
}

fn refusal(id: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": -32000,
        "message": message,
        "data": {"kind": Value::Null},
    }})
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A socket file left by a process that is gone is what a crash leaves, and
    /// refusing to start over it would mean a crash costs somebody a restart of
    /// their machine's memory rather than of the application.
    #[test]
    fn a_socket_left_by_a_dead_process_is_taken_over() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");
        std::fs::write(&path, b"not a socket").expect("a file in the way");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        assert!(bind(&path).is_ok(), "a stale socket was not cleared");
    }

    /// Two copies of Sync on one machine is a different matter: taking the door
    /// would leave the first one's window talking to nothing, with nothing
    /// anywhere saying why.
    #[test]
    fn a_socket_another_process_is_serving_is_not_stolen() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the first process took it");

        let error = bind(&path).expect_err("the second process took a door in use");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        drop(held);
    }

    /// Whoever holds the door says who they are, because the next Sync has to
    /// be able to take it from a process that outlived the application that
    /// started it.
    #[test]
    fn the_door_names_the_process_holding_it() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the door opened");

        let written = std::fs::read_to_string(pid_file(&path)).expect("a pid beside the socket");
        assert_eq!(
            written.trim().parse::<u32>().expect("a number"),
            std::process::id()
        );
        drop(held);
    }

    #[test]
    fn the_door_is_readable_by_nobody_else() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the door opened");

        let mode = std::fs::metadata(&path)
            .expect("the socket is there")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the permissions are this door's whole access control"
        );
        drop(held);
    }
}
