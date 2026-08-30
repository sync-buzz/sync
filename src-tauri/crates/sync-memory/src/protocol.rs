//! MCP JSON-RPC framing, and nothing above it.
//!
//! One line of JSON per message, request ids allocated here, notifications
//! separated from responses. Everything that knows what a *record* is lives a
//! layer up.

use std::io;

use serde_json::{Value, json};

use crate::error::{MemoryError, Result};

/// The revision resource every client subscribes to. A notification carries
/// only this URI: the revision itself must be re-read, never inferred from the
/// notification payload.
pub const REVISION_RESOURCE: &str = "memory://revision/current";

/// What the engine says about the project it is serving — the same shape the
/// handshake carries, re-readable after something changes it.
pub const PROJECT_RESOURCE: &str = "memory://project";

/// What the host channel answers about itself.
///
/// Asked once, on connecting: a bundled sidecar that does not answer what this
/// window calls is a bundle assembled wrong, and the handshake is where that
/// should be found.
pub const METHODS: &str = "methods.list";

/// The call that tells a host connection which project it is about.
///
/// Only meaningful on the resident process's socket, where one process serves
/// every project on the machine: the connection says its project once and every
/// message after it is what the single-project door has always carried. Named
/// here rather than in either door, because both read it and two copies of one
/// name is how a rename breaks the half nobody edited.
pub const ATTACH: &str = "project.attach";

/// The call that turns a host connection into the channel *back*.
///
/// Every other message on this socket goes one way — the window asks, the
/// engine answers. This one inverts a connection: after it, the engine writes
/// requests and the application answers them, on the same line-framed JSON-RPC
/// and matched by the same `id`.
///
/// It exists because a tool an agent calls has to run **in the application**,
/// where the permissions, the artefact and the keychain are. The engine is a
/// child process that Sync spawns and outlives, so the direction of the pipe
/// and the direction of a call are not the same question.
///
/// A separate connection rather than an inversion of an attached one, and that
/// is the whole reason it is its own call: a connection with a project on it
/// exists only while somebody has that project open, and an agent calls a tool
/// with no window anywhere.
pub const ATTEND: &str = "host.attend";

/// What the engine asks the application to run, over [`ATTEND`].
///
/// The one request that travels in that direction today. Named here beside the
/// others because both ends spell it, and a name spelled twice is a name that
/// gets renamed once.
pub const TOOL_CALL: &str = "extension.tool";

/// A bidirectional line-delimited JSON channel to the engine.
///
/// Implemented by the sidecar's stdio in production and by an in-process fake
/// in tests, which is the only reason this is a trait.
pub trait Transport: Send {
    /// Write one JSON message. The transport appends the newline.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O failure.
    fn send(&mut self, message: &Value) -> io::Result<()>;

    /// Read one JSON message, or `None` at end of stream.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O failure.
    fn receive(&mut self) -> io::Result<Option<Value>>;
}

/// A JSON-RPC conversation over a [`Transport`].
pub struct Connection<T: Transport> {
    transport: T,
    next_id: u64,
    /// Resource URIs the engine said changed while we were waiting for a
    /// response. Drained by the client, which then re-reads them.
    pending_updates: Vec<String>,
}

impl<T: Transport> Connection<T> {
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: 1,
            pending_updates: Vec::new(),
        }
    }

    /// Send a request and read until its response arrives.
    ///
    /// Notifications that arrive in between are collected rather than
    /// discarded: a mutation's `resources/updated` frequently overtakes the
    /// response to the very call that caused it.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Protocol`] for a malformed or errored response,
    /// [`MemoryError::Sidecar`] when the stream ends, and
    /// [`MemoryError::Io`] for transport failures.
    pub fn request(&mut self, method: &str, params: &Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.transport
            .send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .map_err(|error| died_or_io(method, error))?;
        loop {
            let Some(message) = self
                .transport
                .receive()
                .map_err(|error| died_or_io(method, error))?
            else {
                return Err(MemoryError::Sidecar(format!(
                    "the engine closed its output while answering `{method}`"
                )));
            };
            if let Some(uri) = notification_uri(&message) {
                self.pending_updates.push(uri);
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                // A response to a request we no longer care about (a retry
                // after a restart, say). Nothing to do with it but move on.
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(rpc_error(method, error));
            }
            return message.get("result").cloned().ok_or_else(|| {
                MemoryError::Protocol(format!("`{method}` returned neither result nor error"))
            });
        }
    }

    /// Send a notification, which by definition has no response.
    ///
    /// # Errors
    ///
    /// Returns the transport failure.
    pub fn notify(&mut self, method: &str, params: &Value) -> Result<()> {
        self.transport
            .send(&json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }))
            .map_err(|error| died_or_io(method, error))?;
        Ok(())
    }

    /// The underlying transport, for the few questions only it can answer —
    /// chiefly whether the process behind it is still alive.
    pub const fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Take the resource updates seen so far.
    pub fn take_updates(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_updates)
    }
}

/// Read a domain failure out of a tool result.
///
/// The engine reports these as a *successful* JSON-RPC response carrying
/// `isError: true` and `structuredContent.error`, because the call itself
/// worked — it is the operation that failed. Callers branch on `kind`.
///
/// # Errors
///
/// Returns the domain failure when the result carries one.
pub fn tool_result(result: &Value) -> Result<Value> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let error = result
            .get("structuredContent")
            .and_then(|content| content.get("error"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let kind = error
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the memory engine reported a failure without a message");
        let data = error.get("data").cloned().unwrap_or_else(|| json!({}));
        return Err(MemoryError::domain(kind, message, data));
    }
    result.get("structuredContent").cloned().ok_or_else(|| {
        MemoryError::Protocol("tool result carried no structured content".to_owned())
    })
}

/// Classify a transport failure as a dead engine or a genuine I/O problem.
///
/// Writing to a process that has exited gives `BrokenPipe`, and reading from
/// one gives `UnexpectedEof` — both mean "the engine is gone", which is a
/// transient condition the client recovers from by restarting it. Reporting
/// them as I/O errors instead would surface a crash to the user as an
/// unexplained failure.
fn died_or_io(method: &str, error: io::Error) -> MemoryError {
    match error.kind() {
        io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionReset => {
            MemoryError::Sidecar(format!("the engine is gone (during `{method}`): {error}"))
        }
        _ => MemoryError::Io(error),
    }
}

/// The URI a `notifications/resources/updated` message names, if that is what
/// this message is.
fn notification_uri(message: &Value) -> Option<String> {
    if message.get("id").is_some() {
        return None;
    }
    if message.get("method").and_then(Value::as_str)? != "notifications/resources/updated" {
        return None;
    }
    message
        .get("params")
        .and_then(|params| params.get("uri"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Turn a JSON-RPC error into ours, keeping the engine's `data.kind` when it
/// has one — protocol-level failures use the same stable vocabulary.
fn rpc_error(method: &str, error: &Value) -> MemoryError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the memory engine rejected the request");
    let data = error.get("data").cloned().unwrap_or_else(|| json!({}));
    let kind = data.get("kind").and_then(Value::as_str).map(str::to_owned);
    match kind {
        Some(kind) => MemoryError::domain(&kind, message, data),
        None => MemoryError::Protocol(format!("`{method}` failed: {message}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::collections::VecDeque;

    /// A transport that replays scripted messages and records what was sent.
    struct Scripted {
        sent: Vec<Value>,
        inbound: VecDeque<Value>,
    }

    impl Transport for Scripted {
        fn send(&mut self, message: &Value) -> io::Result<()> {
            self.sent.push(message.clone());
            Ok(())
        }

        fn receive(&mut self) -> io::Result<Option<Value>> {
            Ok(self.inbound.pop_front())
        }
    }

    #[test]
    fn a_notification_arriving_before_the_response_is_kept_not_dropped() {
        let mut connection = Connection::new(Scripted {
            sent: Vec::new(),
            inbound: VecDeque::from(vec![
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/updated",
                    "params": {"uri": REVISION_RESOURCE}
                }),
                json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
            ]),
        });

        let result = connection.request("tools/call", &json!({})).unwrap();

        assert_eq!(result, json!({"ok": true}));
        assert_eq!(
            connection.take_updates(),
            vec![REVISION_RESOURCE.to_owned()],
            "the revision changed and the client has to re-read it"
        );
    }

    #[test]
    fn a_broken_pipe_is_a_dead_engine_not_an_io_error() {
        struct Dead;
        impl Transport for Dead {
            fn send(&mut self, _message: &Value) -> io::Result<()> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
            }
            fn receive(&mut self) -> io::Result<Option<Value>> {
                Ok(None)
            }
        }

        let error = Connection::new(Dead)
            .request("tools/call", &json!({}))
            .unwrap_err();

        assert!(
            matches!(error, MemoryError::Sidecar(_)),
            "writing to an exited process means restart it, not report I/O: {error:?}"
        );
    }

    #[test]
    fn a_closed_stream_is_a_sidecar_failure_not_a_hang() {
        let mut connection = Connection::new(Scripted {
            sent: Vec::new(),
            inbound: VecDeque::new(),
        });

        let error = connection.request("tools/call", &json!({})).unwrap_err();

        assert!(matches!(error, MemoryError::Sidecar(_)));
    }

    #[test]
    fn a_tool_error_becomes_a_domain_failure_with_its_kind() {
        let result = json!({
            "isError": true,
            "structuredContent": {"error": {
                "kind": "conflict",
                "message": "same-key conflict",
                "data": {"keys": ["note-1"]}
            }}
        });

        let error = tool_result(&result).unwrap_err();

        assert!(error.is_retryable_conflict());
        assert_eq!(
            error.kind().unwrap().as_wire(),
            "conflict",
            "the kind survives the round trip"
        );
    }

    #[test]
    fn an_unknown_kind_is_preserved_rather_than_flattened() {
        let result = json!({
            "isError": true,
            "structuredContent": {"error": {
                "kind": "some_future_kind",
                "message": "from a newer engine"
            }}
        });

        let error = tool_result(&result).unwrap_err();

        assert_eq!(error.kind().unwrap().as_wire(), "some_future_kind");
    }
}
