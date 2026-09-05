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

/// What version of the host channel both ends are speaking.
///
/// One number for the channel rather than one per operation, and stated in the
/// handshake by each end. Until now nothing forced the question: the window and
/// the sidecar are built and released together, so a mismatch was a bundle
/// assembled wrong rather than a state anybody could be in. A client that
/// arrives through a store is months behind by construction — *old client, new
/// server* is its ordinary condition, not its edge — and a number added after
/// the first such client exists does nothing for the clients that shipped
/// without it.
///
/// Compatibility is equality and nothing cleverer. A range would be a promise
/// about which changes are safe, and nobody has had to keep that promise yet.
pub const CHANNEL_VERSION: u32 = 1;

/// The longest single message the host channel will read.
///
/// A line is read into memory before anything can look at it, so without a
/// ceiling one client with no newline in it is as much of this process's memory
/// as it cares to take, given away quietly. Eight mebibytes is far above any
/// message either end sends — a record body, a document, a base64 payload — and
/// far below what a connection may cost.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// The call that lists the projects this machine holds.
///
/// Answered before any project has been named, which is what makes it the one
/// call a connection can make while it still has nothing to say about *which*
/// project it is about. A client that cannot see the file system has no other
/// way to find out what there is; a client that can see it must not be asked
/// to, because a path is a thing to be typed wrong.
pub const PROJECTS: &str = "projects.list";

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

/// The call that hands the engine the secrets its network door will accept.
///
/// Only ever heard on the socket in the application's own directory. The set
/// lives in the keychain, and one module of the application is the only thing
/// that opens that — *how* a secret is kept and *who may ask for one* are kept
/// two questions with two answers, and an engine reading it would be a second
/// answer to the second. So the application reads it and states it here, and
/// the door holds it in memory for as long as it is open.
///
/// Refused on the network door itself, and that is the point of naming it here
/// rather than leaving it an ordinary operation: a device that could add a
/// device would be a device that cannot be revoked.
pub const REMOTE_DEVICES: &str = "remote.devices";

/// The first thing a device says on the network door, before anything else.
///
/// It carries the secret the person's own machine minted when they paired that
/// device. Nothing else on that connection is answered until it does, and a
/// connection that has not said it within [`REMOTE_GREETING`] is closed.
pub const REMOTE_HELLO: &str = "remote.hello";

/// How long a network connection has to prove who it is.
///
/// A connection that has said nothing is costing this process a task, a buffer
/// and a file descriptor for nothing, and the one caller this door exists for
/// sends its first line immediately. Ten seconds is far longer than a phone on
/// a bad connection needs and far shorter than a person would wait to notice.
pub const REMOTE_GREETING: std::time::Duration = std::time::Duration::from_secs(10);

/// How long an established network connection may say nothing at all.
///
/// A phone whose screen went off stops speaking without closing anything, and
/// the connection it leaves behind would otherwise be held until the process
/// ends. Ten minutes, because reconnecting is cheap and a person coming back to
/// a screen they left open should find it working.
pub const REMOTE_IDLE: std::time::Duration = std::time::Duration::from_secs(600);

/// What the engine asks the application to run, over [`ATTEND`].
///
/// The first request that travelled in that direction, and for a long time the
/// only one — [`SESSION_DROPPED`] is the other, and [`SESSION_EVENT`] goes that
/// way carrying no call at all. Named here beside the rest because both ends
/// spell every one of them, and a name spelled twice is a name that gets
/// renamed once.
pub const TOOL_CALL: &str = "extension.tool";

/// What a caller off this machine asks so that a package's request is made
/// *here*, over [`ATTEND`].
///
/// The engine carries it and reads none of it. What a package may reach is a
/// sentence in the manifest of the artefact installed on this machine, and the
/// secret it signs with is in this machine's keychain — so both the check and
/// the request belong on the machine the application is running on, and this
/// name is the whole of the engine's part in it.
///
/// It is deliberately not the door for a package that wants a secret *in its
/// hand*: there is no name here for that, and a build with no local Rust behind
/// it is refused by there being nothing to call.
pub const EXTENSION_FETCH: &str = "extension.fetch";

/// Everything one package's artefact is, asked of the machine holding it.
///
/// A phone draws a package and has none of it on disk. It reads what is
/// installed, it reads the files that make up the code, and it asks for a
/// package to be installed or forgotten — and every one of those is a fact
/// about the machine's artefact directory rather than about any project, which
/// is why they travel this way rather than as operations of the surface.
pub const EXTENSION_LIST: &str = "extension.list";
/// One file inside one installed artefact, as bytes.
pub const EXTENSION_FILE: &str = "extension.file";
/// Download what the registry named and install it, on the machine.
pub const EXTENSION_INSTALL: &str = "extension.install";
/// Stop serving an id on the machine.
pub const EXTENSION_FORGET: &str = "extension.forget";
/// Point an id back at the artefact it was serving, which is how an update is
/// rolled back.
pub const EXTENSION_REPOINT: &str = "extension.repoint";
/// What the registry says exists, fetched or from the cache beside it.
pub const REGISTRY_INDEX: &str = "registry.index";
/// What the last fetch left on the disk, asking nobody.
pub const REGISTRY_CACHED: &str = "registry.cached";
/// Every version one package has published.
pub const REGISTRY_LEDGER: &str = "registry.ledger";

/// Run the handler a package declared for an occasion, on the machine that
/// holds it.
///
/// Taking an extension on is three steps in one order — publish its types, run
/// this, write the declaration — and the middle one is a module evaluated in an
/// isolate beside the artefact. A window that is not on that machine asks for it
/// the same way it asks for anything else there.
pub const EXTENSION_OCCASION: &str = "extension.occasion";

/// What this installation decided about a project's clocks, as opposed to what
/// the project declares.
///
/// Kept beside the artefacts rather than in the project's memory, because it is
/// a decision about this machine: a clock runs where the packages are, and
/// switching one off is somebody saying *not on this computer* rather than
/// *not in this project*. So a phone reads and writes the computer's answer
/// rather than keeping one of its own — there is nothing on a phone for a clock
/// to run on.
pub const SCHEDULE_REMEMBER: &str = "schedule.remember";
/// Which of a project's clocks this machine has switched off.
pub const SCHEDULE_OFF: &str = "schedule.off";
/// Switch one clock on or off, on the machine that runs it.
pub const SCHEDULE_SWITCH: &str = "schedule.switch";

/// Talking to an agent, asked of the machine the agent runs on.
///
/// A conversation is a process the application raised and holds open. It is
/// not an operation of a project's memory and never touches one — what it is
/// about is a session key, a turn of text, a question waiting on an answer — so
/// these travel the way everything else about the machine travels: carried to
/// the application, which owns the process, the protocol and the transcript.
///
/// **Five of them name a project, and they name it as `project`.** On the
/// window's own door that is a path; on a device's it is a key the door
/// resolves through the registry before the application sees it, exactly as it
/// resolves the key on an operation. So the application is handed a path either
/// way and never learns which door its caller came through — which is what
/// keeps one function answering both.
pub const SESSION_CATALOG: &str = "session.catalog";
/// Everything running right now, across every project this machine holds.
pub const SESSION_LIVE: &str = "session.live";
/// Raise an agent and open a session in it. Names its project.
pub const SESSION_OPEN: &str = "session.open";
/// Run one turn. Answers when the prompt is on its way, not when the turn ends.
pub const SESSION_PROMPT: &str = "session.prompt";
/// Continue a conversation from before this launch. Names its project.
pub const SESSION_RESUME: &str = "session.resume";
/// The dormant conversations of one project. Names it.
pub const SESSION_REMEMBERED: &str = "session.remembered";
/// Stop offering a dormant conversation. Names its project.
pub const SESSION_FORGET_REMEMBERED: &str = "session.forget_remembered";
/// Give a conversation a name.
pub const SESSION_RENAME: &str = "session.rename";
/// Interrupt the turn that is running.
pub const SESSION_CANCEL: &str = "session.cancel";
/// End the agent's process, keeping what it said.
pub const SESSION_CLOSE: &str = "session.close";
/// End it and forget the pointer to it.
pub const SESSION_FORGET: &str = "session.forget";
/// Say which record a conversation was kept as.
pub const SESSION_KEPT_AS: &str = "session.kept_as";
/// The pointer for a kept record, where this machine holds one. Names its
/// project.
pub const SESSION_FOR_RECORD: &str = "session.for_record";
/// Put the agent in one of the modes it stated.
pub const SESSION_SET_MODE: &str = "session.set_mode";
/// Choose one of the options the agent stated for the session.
pub const SESSION_SET_OPTION: &str = "session.set_option";
/// Answer the question an agent stopped on.
pub const SESSION_PERMISSION_RESPOND: &str = "session.permission_respond";
/// Everything a session has said, read once, without watching it.
pub const SESSION_BACKLOG: &str = "session.backlog";

/// Watch a session: what it has said since a given point, then everything
/// after.
///
/// The one call of this family that is not finished when it is answered. The
/// door mints a number for the watch and puts it in the call, the application
/// holds it against the session, and every event afterwards travels back under
/// it as [`SESSION_EVENT`]. The application echoes the number in its answer, so
/// the device that asked learns what its events will arrive under without the
/// door having to reshape an answer it did not write.
///
/// `since` is the last sequence number the caller has already seen, or absent
/// for a caller that has seen nothing. It is what makes a connection that
/// dropped cost nothing: the device asks again from where it stopped and the
/// window above it sees an uninterrupted stream rather than its transcript
/// written twice.
pub const SESSION_SUBSCRIBE: &str = "session.subscribe";
/// Stop watching, by the number [`SESSION_SUBSCRIBE`] answered with. The
/// session goes on running.
pub const SESSION_UNSUBSCRIBE: &str = "session.unsubscribe";

/// What each agent adapter is, and whether this machine has it downloaded.
pub const AGENT_ADAPTERS: &str = "agent.adapters";
/// Download what the agents need, at the versions the machine's build pins.
pub const AGENT_ADAPTERS_PREPARE: &str = "agent.adapters_prepare";
/// Delete what was downloaded.
pub const AGENT_ADAPTERS_FORGET: &str = "agent.adapters_forget";

/// One thing a watched session said, on its way to the device watching it.
///
/// **The only message this channel carries that nobody asked for.** It travels
/// as a notification — no id, no answer — and it is the whole of what the
/// reverse direction on a device's connection is allowed to be: `{subscription,
/// event}` and nothing else. No call passes the other way. [`ATTEND`] inverts a
/// connection so that requests may be made on it, and that remains refused on
/// the network door for the reason it always was: answering in the
/// application's name is the application's alone.
///
/// It is written twice on its way — the application says it to the engine over
/// [`ATTEND`], and the engine says it to the device whose connection holds that
/// subscription — and it is the same message both times, which is why it has
/// one name.
pub const SESSION_EVENT: &str = "session.event";

/// The subscriptions whose device has gone, told to the application.
///
/// The application cannot see it happen: it writes events into a socket to an
/// engine that is still there, and the connection that ended is one further on.
/// So the engine says so, once per connection rather than once per event, and
/// the application lets go of the watchers named.
///
/// Without it a phone put in a pocket leaves a session serialising its every
/// word into a queue nobody drains, for as long as the conversation runs.
pub const SESSION_DROPPED: &str = "session.dropped";

/// Every call of that family, in one place a test can walk.
///
/// A slice rather than a `matches!` arm, and the reason is the test at the foot
/// of this file: a name added to the channel and left out of
/// [`effect`](crate::effect) is the one thing that list exists to prevent, and a
/// set nothing can enumerate cannot be checked against it. What the engine's own
/// door does for its operations, this does for the calls that are carried past
/// it.
pub const SESSIONS: &[&str] = &[
    SESSION_CATALOG,
    SESSION_LIVE,
    SESSION_OPEN,
    SESSION_PROMPT,
    SESSION_RESUME,
    SESSION_REMEMBERED,
    SESSION_FORGET_REMEMBERED,
    SESSION_RENAME,
    SESSION_CANCEL,
    SESSION_CLOSE,
    SESSION_FORGET,
    SESSION_KEPT_AS,
    SESSION_FOR_RECORD,
    SESSION_SET_MODE,
    SESSION_SET_OPTION,
    SESSION_PERMISSION_RESPOND,
    SESSION_BACKLOG,
    SESSION_SUBSCRIBE,
    SESSION_UNSUBSCRIBE,
    AGENT_ADAPTERS,
    AGENT_ADAPTERS_PREPARE,
    AGENT_ADAPTERS_FORGET,
];

/// Whether this call is about talking to an agent.
///
/// Asked by the network door, which does two things to this family that it does
/// to nothing else: it resolves the project key some of them carry, and it puts
/// the paths out of the answer before it goes to a device. Both are properties
/// of the family rather than of any one call, so the family is named once.
#[must_use]
pub fn about_a_session(method: &str) -> bool {
    SESSIONS.contains(&method)
}

/// Whether this call carries the project it is about, as `project`.
///
/// The rest of the family is addressed by a session key, which this machine
/// minted and which says nothing about where anything is. Naming the five that
/// are not is what lets the door resolve a key exactly once, in one place,
/// rather than in each of them.
#[must_use]
pub fn names_a_project(method: &str) -> bool {
    matches!(
        method,
        SESSION_OPEN
            | SESSION_RESUME
            | SESSION_REMEMBERED
            | SESSION_FORGET_REMEMBERED
            | SESSION_FOR_RECORD
    )
}

/// Whether a door carries this call to the application rather than answering
/// it.
///
/// One list rather than a condition in each door, because two doors read it and
/// they must agree: the engine's, which carries the call over [`ATTEND`], and
/// the application's, which refuses by name anything it was not asked to
/// answer. A name added to one and forgotten in the other is a call that
/// arrives somewhere with nothing to run it.
///
/// Everything here is about the machine rather than about the *contents* of a
/// project — an artefact on its disk, a registry it fetches, a secret in its
/// keychain, a process it raised — so none of them is an [`Operation`](crate)
/// and none of them is answered out of a project's memory.
///
/// Five of them do name a project, and it is worth saying why that is not the
/// same thing: a conversation is held *for* a repository and the agent is
/// raised *in* it, but nothing about it is read from or written to that
/// repository's memory. The project is where the agent works, not what the call
/// is about, which is why these are carried rather than dispatched.
#[must_use]
pub fn carried(method: &str) -> bool {
    about_a_session(method)
        || matches!(
            method,
            EXTENSION_FETCH
                | EXTENSION_LIST
                | EXTENSION_FILE
                | EXTENSION_INSTALL
                | EXTENSION_FORGET
                | EXTENSION_REPOINT
                | REGISTRY_INDEX
                | REGISTRY_CACHED
                | REGISTRY_LEDGER
                | EXTENSION_OCCASION
                | SCHEDULE_REMEMBER
                | SCHEDULE_OFF
                | SCHEDULE_SWITCH
        )
}

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
