//! The inference sidecar client: spawns `sync-inference-server`, speaks to it
//! over stdio, and reconnects when it dies.
//!
//! The window's [`ask`](crate::models::ask) reaches a model through this
//! client rather than running inference in its own process, for the same reason
//! the memory engine is a sidecar: a crash in native inference is a reconnect
//! rather than a lost window. The lifecycle mirrors the memory sidecar's —
//! started on first use, held for the application's life, and restarted when a
//! call finds it gone.
//!
//! # One connection, one call at a time
//!
//! The sidecar holds one model and answers one request at a time, so the
//! connection is behind a [`Mutex`]. A handler that asks while another is
//! answering waits; this is the right behaviour for a model that runs in
//! milliseconds, and the wrong one for one that runs in seconds — but the
//! second is a model that should not be an auxiliary model.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::Value;
use sync_inference::ipc::{CHANNEL_VERSION, HandshakeParams, Request, Response, RunParams};
use tauri::{AppHandle, Runtime};

/// The sidecar's name inside the bundle. Tauri strips the target triple from
/// `externalBin` entries, so `binaries/sync-inference-server-aarch64-apple-darwin`
/// ships as this.
const BUNDLED_BINARY: &str = "sync-inference-server";

/// Points a development build at a sidecar built from source, where no bundle
/// exists to take one from. The same name the memory sidecar reads, for the
/// same reason: a `target/debug/sync-inference-server` binary, built from this
/// tree.
const BINARY_OVERRIDE: &str = "SYNC_INFERENCE_BINARY";

/// One live sidecar, held in [`InferenceSidecar`] for the application's life.
struct Connection {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// The inference sidecar, held as Tauri state.
///
/// `Default` because the window builds its state before the sidecar is needed,
/// and the process is not started until the first call asks for it.
#[derive(Default)]
pub struct InferenceSidecar {
    connection: Mutex<Option<Connection>>,
}

impl InferenceSidecar {
    /// Run one task through the sidecar: start it if needed, send the request,
    /// read the answer.
    ///
    /// **Two kinds of failure, and only one drops the connection.** An I/O
    /// failure — the process is dead, the pipe is broken — drops the
    /// connection so the next call reconnects. A response failure — the sidecar
    /// answered `{"error": ...}` — keeps the connection, because the process is
    /// alive and the error is about this request, not about the pipe. Collapsing
    /// the two would turn a model that fails to load into a crash loop: each
    /// call would reconnect, try the same load, and fail the same way.
    ///
    /// # Errors
    ///
    /// When the sidecar cannot be started, when the pipe is broken, or when the
    /// sidecar answers an error. The string is the handler's message.
    pub fn run<R: Runtime>(
        &self,
        _app: &AppHandle<R>,
        model_path: &str,
        runtime: &str,
        task: &str,
        input: &Value,
        engine_path: Option<&str>,
    ) -> Result<Value, String> {
        let mut guard = self.connection.lock().map_err(|e| e.to_string())?;

        // Start the sidecar if this is the first call, or if the last call left
        // it dead.
        if guard.is_none() {
            *guard = Some(start()?);
        }

        let connection = guard.as_mut().expect("the connection was opened above");

        let params = serde_json::to_value(RunParams {
            model_path: model_path.to_owned(),
            runtime: runtime.to_owned(),
            task: task.to_owned(),
            input: input.clone(),
            engine_path: engine_path.map(str::to_owned),
        })
        .map_err(|e| format!("could not encode the request: {e}"))?;

        // Send and receive. An I/O failure here means the process is dead —
        // drop the connection so the next call starts fresh.
        let response = match send(
            connection,
            Request {
                method: "run".to_owned(),
                params,
            },
        ) {
            Ok(response) => response,
            Err(io_error) => {
                if let Some(mut dead) = guard.take() {
                    let _ = dead.child.kill();
                }
                return Err(io_error);
            }
        };

        // A response error is about this request, not about the pipe. The
        // connection stays open for the next call.
        if let Some(error) = response.error {
            return Err(error.message);
        }

        Ok(response.result)
    }
}

/// Start the sidecar and perform the handshake.
fn start() -> Result<Connection, String> {
    let binary = resolve_binary()?;

    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not start `{}`: {e}", binary.display()))?;

    let stdin = child.stdin.take().ok_or("the sidecar opened no stdin")?;
    let stdout = child.stdout.take().ok_or("the sidecar opened no stdout")?;
    let mut connection = Connection {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    };

    handshake(&mut connection)?;
    Ok(connection)
}

/// Resolve where the sidecar binary is: an override for a dev build, or the
/// binary beside this executable for a bundled one.
fn resolve_binary() -> Result<std::path::PathBuf, String> {
    if let Some(path) = std::env::var_os(BINARY_OVERRIDE).map(std::path::PathBuf::from) {
        return Ok(path);
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("could not locate the application executable: {e}"))?;
    Ok(exe
        .parent()
        .ok_or("the application executable has no parent directory")?
        .join(BUNDLED_BINARY))
}

/// Send the handshake and check the channel version.
fn handshake(connection: &mut Connection) -> Result<(), String> {
    let request = Request {
        method: "handshake".to_owned(),
        params: serde_json::to_value(HandshakeParams {
            channel: CHANNEL_VERSION,
        })
        .unwrap_or_default(),
    };
    let response = send(connection, request)?;
    if let Some(error) = response.error {
        return Err(format!("handshake failed: {}", error.message));
    }
    Ok(())
}

/// Send one request and read one response. The protocol is line-delimited
/// JSON: one line in, one line out.
fn send(connection: &mut Connection, request: Request) -> Result<Response, String> {
    let line = serde_json::to_string(&request).map_err(|e| format!("could not encode: {e}"))?;
    connection
        .stdin
        .write_all(line.as_bytes())
        .map_err(|e| format!("could not send to the sidecar: {e}"))?;
    connection
        .stdin
        .write_all(b"\n")
        .map_err(|e| format!("could not send to the sidecar: {e}"))?;
    connection
        .stdin
        .flush()
        .map_err(|e| format!("could not flush to the sidecar: {e}"))?;

    let mut answer = String::new();
    connection
        .stdout
        .read_line(&mut answer)
        .map_err(|e| format!("could not read from the sidecar: {e}"))?;
    if answer.is_empty() {
        return Err("the sidecar closed without answering".to_owned());
    }
    serde_json::from_str(&answer).map_err(|e| format!("the sidecar answered un-readable JSON: {e}"))
}
