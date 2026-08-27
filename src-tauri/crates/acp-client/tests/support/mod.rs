//! A hand-driven agent on the other end of an in-memory duplex.
//!
//! Nothing here starts a process. The client's seam is a reader/writer pair, so
//! the whole protocol can be driven frame by frame from the test body, which
//! makes every timing in these tests deterministic: the agent says something
//! only when the test tells it to.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use acp_client::{schema, AgentConnection, ClientHandler, RpcError, SessionUpdateEvent};
use async_trait::async_trait;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::sync::{mpsc, Mutex, Semaphore};

/// How long any wait in a test may take before it is a failure rather than a
/// slow machine.
///
/// Deliberately far above what the work costs — the whole suite runs in under a
/// second on an idle box. It is sized for a loaded one: at a load average near
/// 30, with another crate's build running alongside, a five-second budget was
/// observed timing out on nothing worse than process startup. The only thing a
/// long budget costs is the time a genuinely broken build takes to say so.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// Awaits `future`, failing the test if it takes longer than [`PATIENCE`].
///
/// A test that hangs says nothing; a test that times out with a name says
/// which step never happened.
/// Awaits the future here rather than through [`within`]: delegating adds a
/// frame to every caller's future, and some of the ones in this suite are
/// already near the size the workspace lints at.
pub async fn within_patience<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(PATIENCE, future)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// Awaits `future`, failing the test if it takes longer than `limit`.
///
/// For the waits whose whole point is that something happens *soon*. [`PATIENCE`]
/// is sized so that a loaded box does not fail on scheduling alone, which makes
/// it the wrong instrument for asserting that a deadline fired: with the
/// deadline gone the test has to go red by name in seconds, not park the suite.
pub async fn within<T>(
    what: &str,
    limit: Duration,
    future: impl std::future::Future<Output = T>,
) -> T {
    tokio::time::timeout(limit, future)
        .await
        .unwrap_or_else(|_| panic!("timed out after {limit:?} waiting for {what}"))
}

/// Something the client handed to its handler, in the order it arrived.
///
/// Boxed variants: the protocol request types differ by hundreds of bytes, and
/// every one of these travels through a channel.
#[derive(Debug)]
pub enum Observed {
    Update(Box<SessionUpdateEvent>),
    Permission(Box<schema::RequestPermissionRequest>),
    Read(Box<schema::ReadTextFileRequest>),
    Write(Box<schema::WriteTextFileRequest>),
    UnhandledNotification {
        method: String,
        params: Option<serde_json::Value>,
    },
}

/// What the handler should answer an `fs/read_text_file` with.
pub type ReadAnswer = Result<String, RpcError>;

/// A [`ClientHandler`] that records everything and answers from canned values.
pub struct TestHandler {
    events: mpsc::UnboundedSender<Observed>,
    read_answer: Mutex<ReadAnswer>,
    /// When set, each `session/update` costs one permit before it is recorded,
    /// so a test can hold the ordered delivery path open and watch what does
    /// (and does not) get past it.
    update_gate: Mutex<Option<Arc<Semaphore>>>,
}

impl TestHandler {
    /// Builds a handler and the queue its observations arrive on.
    #[must_use]
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Observed>) {
        let (events, rx) = mpsc::unbounded_channel();
        let handler = Arc::new(Self {
            events,
            read_answer: Mutex::new(Ok("contents from the client".to_owned())),
            update_gate: Mutex::new(None),
        });
        (handler, rx)
    }

    /// Sets what the next `fs/read_text_file` is answered with.
    pub async fn set_read_answer(&self, answer: ReadAnswer) {
        *self.read_answer.lock().await = answer;
    }

    /// Makes every later `session/update` wait for a permit from `gate`.
    ///
    /// Handing out permits is how a test decides when a given update is
    /// allowed to be delivered.
    pub async fn hold_updates(&self, gate: Arc<Semaphore>) {
        *self.update_gate.lock().await = Some(gate);
    }

    fn record(&self, observed: Observed) {
        // The receiver outlives every test that cares; a closed queue only
        // means the test already finished.
        drop(self.events.send(observed));
    }
}

#[async_trait]
impl ClientHandler for TestHandler {
    async fn session_update(&self, event: SessionUpdateEvent) {
        let gate = self.update_gate.lock().await.clone();
        if let Some(gate) = gate {
            // `forget` so each update costs a permit rather than borrowing one.
            if let Ok(permit) = gate.acquire().await {
                permit.forget();
            }
        }
        self.record(Observed::Update(Box::new(event)));
    }

    async fn request_permission(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Result<schema::RequestPermissionResponse, RpcError> {
        let chosen = request
            .options
            .iter()
            .find(|option| {
                matches!(
                    option.kind,
                    schema::PermissionOptionKind::AllowOnce
                        | schema::PermissionOptionKind::AllowAlways
                )
            })
            .map(|option| option.option_id.clone());
        self.record(Observed::Permission(Box::new(request)));

        Ok(schema::RequestPermissionResponse::new(match chosen {
            Some(option_id) => schema::RequestPermissionOutcome::Selected(
                schema::SelectedPermissionOutcome::new(option_id),
            ),
            None => schema::RequestPermissionOutcome::Cancelled,
        }))
    }

    async fn read_text_file(
        &self,
        request: schema::ReadTextFileRequest,
    ) -> Result<schema::ReadTextFileResponse, RpcError> {
        let answer = self.read_answer.lock().await.clone();
        self.record(Observed::Read(Box::new(request)));
        answer.map(schema::ReadTextFileResponse::new)
    }

    async fn write_text_file(
        &self,
        request: schema::WriteTextFileRequest,
    ) -> Result<schema::WriteTextFileResponse, RpcError> {
        self.record(Observed::Write(Box::new(request)));
        Ok(schema::WriteTextFileResponse::new())
    }

    async fn unhandled_notification(&self, method: &str, params: Option<serde_json::Value>) {
        self.record(Observed::UnhandledNotification {
            method: method.to_owned(),
            params,
        });
    }
}

/// The agent side of the duplex, driven a frame at a time by the test.
pub struct FakeAgent {
    reader: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    writer: WriteHalf<tokio::io::DuplexStream>,
}

impl FakeAgent {
    /// The next frame the client sent. Panics if the client hung up.
    pub async fn next_frame(&mut self) -> serde_json::Value {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).await.expect("read");
        assert!(read > 0, "the client closed its side of the connection");
        serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("the client wrote a non-JSON line {line:?}: {e}"))
    }

    /// The next frame, required to be a request for `method`; returns its id.
    pub async fn expect_request(&mut self, method: &str) -> (serde_json::Value, serde_json::Value) {
        let frame = self.next_frame().await;
        assert_eq!(frame["method"], json!(method), "unexpected frame: {frame}");
        assert_eq!(frame["jsonrpc"], json!("2.0"), "frame: {frame}");
        (frame["id"].clone(), frame["params"].clone())
    }

    /// Writes a line to the client verbatim — including lines that are not
    /// JSON at all, which agents really do emit.
    pub async fn write_line(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.expect("write");
        self.writer.write_all(b"\n").await.expect("write");
        self.writer.flush().await.expect("flush");
    }

    /// Sends one JSON frame.
    pub async fn send(&mut self, frame: serde_json::Value) {
        self.write_line(&frame.to_string()).await;
    }

    /// Answers a client request successfully.
    pub async fn respond(&mut self, id: serde_json::Value, result: serde_json::Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await;
    }

    /// Answers a client request with a JSON-RPC error.
    pub async fn respond_error(&mut self, id: serde_json::Value, code: i64, message: &str) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }))
        .await;
    }

    /// Sends a notification.
    pub async fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }

    /// Sends a `session/update` for `session`.
    pub async fn update(&mut self, session: &str, update: serde_json::Value) {
        self.notify(
            "session/update",
            json!({ "sessionId": session, "update": update }),
        )
        .await;
    }

    /// Sends a request from the agent to the client.
    pub async fn request(&mut self, id: i64, method: &str, params: serde_json::Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
    }

    /// Closes the agent's stdout, as a dying process would.
    pub async fn hang_up(mut self) {
        self.writer.shutdown().await.expect("shutdown");
        drop(self);
    }
}

/// A connected client and the fake agent facing it.
pub struct Wired {
    pub connection: AgentConnection,
    pub agent: FakeAgent,
    pub handler: Arc<TestHandler>,
    pub observed: mpsc::UnboundedReceiver<Observed>,
}

/// Wires a client to a hand-driven agent over an in-memory duplex.
#[must_use]
pub fn wire() -> Wired {
    wire_with_timeout_and_capacity(acp_client::DEFAULT_REQUEST_TIMEOUT, 64 * 1024)
}

/// As [`wire`], with the control-request deadline the client should use.
///
/// A test that wants to watch the deadline fire injects a short one here; the
/// alternative is sitting out the shipped two minutes, which is no test at all.
#[must_use]
pub fn wire_with_timeout(request_timeout: Duration) -> Wired {
    wire_with_timeout_and_capacity(request_timeout, 64 * 1024)
}

#[must_use]
pub fn wire_with_capacity(capacity: usize) -> Wired {
    wire_with_timeout_and_capacity(acp_client::DEFAULT_REQUEST_TIMEOUT, capacity)
}

fn wire_with_timeout_and_capacity(request_timeout: Duration, capacity: usize) -> Wired {
    let (client_io, agent_io) = tokio::io::duplex(capacity);
    let (client_read, client_write) = tokio::io::split(client_io);
    let (agent_read, agent_write) = tokio::io::split(agent_io);

    let (handler, observed) = TestHandler::new();
    let connection = AgentConnection::with_request_timeout(
        client_read,
        client_write,
        Arc::clone(&handler),
        request_timeout,
    );

    Wired {
        connection,
        agent: FakeAgent {
            reader: BufReader::new(agent_read),
            writer: agent_write,
        },
        handler,
        observed,
    }
}
