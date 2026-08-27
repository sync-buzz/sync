//! Bidirectional JSON-RPC 2.0 over a byte stream, and the ACP methods on top.
//!
//! The seam is the byte stream, not the process: [`AgentConnection::new`]
//! takes any reader/writer pair, so the whole protocol can be exercised over
//! an in-memory duplex and only one test in this crate has to raise a real
//! child process.
//!
//! # Frames
//!
//! Newline-delimited JSON, one message per line, in both directions — the form
//! every agent measured in the live spike actually speaks. Anything on the
//! agent's stdout that is not a JSON object is logged and skipped rather than
//! treated as a protocol failure: adapters and CLIs print banners and warnings
//! there, and a banner must not take the session down.
//!
//! # Delivery order
//!
//! Responses to our requests and `session/update` notifications share one
//! ordered path, so a turn's `stopReason` can never arrive before that turn's
//! last chunk. Requests *from* the agent are handled off that path — see
//! [`ClientHandler`] for what that does and does not promise.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol_schema::v1 as schema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::error::{Error, Result, RpcError};
use crate::handler::ClientHandler;
use crate::update::decode_session_update;

/// The JSON-RPC version string every frame carries.
const JSONRPC_VERSION: &str = "2.0";

/// How long a control request waits for its answer.
///
/// Deliberately generous. The first launch of an adapter may pull its package
/// through `npx` before a single frame is written, and a tighter window would
/// cut a cold start off just as it was working. What it defends against is not
/// slowness but silence: an agent that is up and never answers used to park the
/// caller forever, and forever is the one duration that has no defence.
///
/// One value for all five control methods on purpose — none of them is a
/// judgement call the way a turn is, so splitting them would be a table nobody
/// could say how to fill in. `session/prompt` is not bounded by it at all; see
/// [`AgentConnection::prompt`].
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

/// What a waiting caller gets handed once its answer arrives. A dropped sender
/// means the connection died with the request in flight, which the caller
/// reads as [`Error::Closed`].
type PendingReply = oneshot::Sender<std::result::Result<serde_json::Value, RpcError>>;

/// State shared between the caller-facing connection and its background tasks.
struct Shared {
    /// Serialized frames on their way to the agent's stdin.
    outgoing: mpsc::UnboundedSender<Outgoing>,
    /// Requests we have sent and not yet been answered, keyed by our own id.
    pending: Mutex<HashMap<i64, PendingReply>>,
    /// Our request ids. Monotonic, never reused within a connection.
    next_id: AtomicI64,
    /// Set once the agent's stdout has ended, so a request raised afterwards
    /// fails immediately instead of waiting for an answer that cannot come.
    closed: AtomicBool,
    /// How long a control request waits before the agent is given up on.
    request_timeout: Duration,
    /// How many sessions have been established on this connection — answered
    /// `session/new` and `session/load`, counted after the fact.
    ///
    /// Zero is the whole point: it says the agent has never been given work of
    /// the user's, and that is what makes ending its process over an overrun
    /// safe. From one upwards, silence may be nothing worse than the agent
    /// being busy with a turn.
    sessions: AtomicUsize,
    /// Set once a control request overran its deadline. Kept apart from
    /// `closed` because the two are different reports: this one says the agent
    /// is up and silent, and every later request has to say so too.
    expired: AtomicBool,
    /// Turns `true` with `expired`, so whoever owns the agent's process can
    /// kill it. Only [`crate::launch`] listens; a connection built on a plain
    /// duplex has no process to reap.
    expiry: watch::Sender<bool>,
}

/// A live ACP connection to one agent.
///
/// Not `Clone` on purpose: dropping it tears the connection down, so shared
/// ownership goes through an [`Arc`], where that meaning stays exact.
pub struct AgentConnection {
    shared: Arc<Shared>,
    tasks: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for AgentConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConnection")
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

impl Drop for AgentConnection {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl AgentConnection {
    /// Starts a connection over `reader` (the agent's stdout) and `writer`
    /// (the agent's stdin), delivering everything the agent sends to `handler`.
    ///
    /// Spawns three tokio tasks — read, write, ordered delivery — which are
    /// aborted when the returned connection is dropped. Must be called from
    /// within a tokio runtime.
    ///
    /// Control requests are bounded by [`DEFAULT_REQUEST_TIMEOUT`]; use
    /// [`AgentConnection::with_request_timeout`] to say otherwise.
    pub fn new<R, W, H>(reader: R, writer: W, handler: H) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        H: ClientHandler,
    {
        Self::with_request_timeout(reader, writer, handler, DEFAULT_REQUEST_TIMEOUT)
    }

    /// As [`AgentConnection::new`], with the control-request deadline given
    /// rather than defaulted.
    ///
    /// The deadline is a parameter and not only a constant so that a test can
    /// watch it fire in milliseconds instead of sitting out the real two
    /// minutes — which is the difference between the behaviour being covered
    /// and being asserted about in a comment.
    pub fn with_request_timeout<R, W, H>(
        reader: R,
        writer: W,
        handler: H,
        request_timeout: Duration,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        H: ClientHandler,
    {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Outgoing>();
        let (delivery_tx, delivery_rx) = mpsc::unbounded_channel::<Delivery>();

        let shared = Arc::new(Shared {
            outgoing: outgoing_tx,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            closed: AtomicBool::new(false),
            request_timeout,
            sessions: AtomicUsize::new(0),
            expired: AtomicBool::new(false),
            expiry: watch::Sender::new(false),
        });
        let handler = Arc::new(handler);

        let tasks = vec![
            tokio::spawn(write_loop(writer, outgoing_rx)),
            tokio::spawn(read_loop(
                reader,
                delivery_tx,
                Arc::clone(&shared),
                Arc::clone(&handler),
            )),
            tokio::spawn(delivery_loop(delivery_rx, Arc::clone(&shared), handler)),
        ];

        Self { shared, tasks }
    }

    /// Whether the agent's stdout has ended. Once true, every request fails
    /// with [`Error::Closed`].
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::SeqCst)
    }

    // --- Client → agent -----------------------------------------------------

    /// `initialize` — the first frame of any connection.
    ///
    /// # Errors
    ///
    /// [`Error::Rpc`] if the agent refuses, [`Error::Closed`] if it died,
    /// [`Error::MalformedResponse`] if its answer is not an initialize result.
    pub async fn initialize(
        &self,
        request: schema::InitializeRequest,
    ) -> Result<schema::InitializeResponse> {
        self.request(schema::AGENT_METHOD_NAMES.initialize, &request)
            .await
    }

    /// `authenticate` — pick one of the `authMethods` from `initialize`.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`].
    pub async fn authenticate(
        &self,
        request: schema::AuthenticateRequest,
    ) -> Result<schema::AuthenticateResponse> {
        self.request(schema::AGENT_METHOD_NAMES.authenticate, &request)
            .await
    }

    /// `session/new` — opens a session with our `cwd` and our `mcpServers`.
    ///
    /// This is where session identity stops being a property of the process
    /// and becomes a parameter of the call: the `env` of each MCP server in
    /// the request reaches that server's own process.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`].
    pub async fn new_session(
        &self,
        request: schema::NewSessionRequest,
    ) -> Result<schema::NewSessionResponse> {
        let response = self
            .request(schema::AGENT_METHOD_NAMES.session_new, &request)
            .await?;
        self.session_established();
        Ok(response)
    }

    /// `session/load` — reattaches to a session by id on a fresh connection.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`].
    pub async fn load_session(
        &self,
        request: schema::LoadSessionRequest,
    ) -> Result<schema::LoadSessionResponse> {
        let response = self
            .request(schema::AGENT_METHOD_NAMES.session_load, &request)
            .await?;
        // A reattached session is as established as a new one: from here the
        // agent may be carrying work of the user's.
        self.session_established();
        Ok(response)
    }

    /// `session/prompt` — runs one turn, resolving with its `stopReason`.
    ///
    /// The turn's output arrives meanwhile as `session/update` notifications on
    /// the handler; this call resolves after the last of them.
    ///
    /// The only call here that carries no deadline, deliberately: a turn is the
    /// agent working, and a working agent may legitimately take tens of
    /// minutes. A wall clock on it would end honest work, which is a worse
    /// failure than the one deadlines are for. A turn is bounded by two other
    /// things instead — the agent dying ([`Error::Closed`]) and the user
    /// cancelling ([`AgentConnection::cancel`]).
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`].
    pub async fn prompt(&self, request: schema::PromptRequest) -> Result<schema::PromptResponse> {
        self.request_without_deadline(schema::AGENT_METHOD_NAMES.session_prompt, &request)
            .await
    }

    /// `session/prompt`, plus a receipt that fires only after its frame has been
    /// flushed to the agent. The receiver is dropped on a write failure.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::prompt`].
    pub async fn prompt_with_dispatch(
        &self,
        request: schema::PromptRequest,
        dispatched: oneshot::Sender<()>,
    ) -> Result<schema::PromptResponse> {
        self.call(
            schema::AGENT_METHOD_NAMES.session_prompt,
            &request,
            None,
            Some(dispatched),
        )
        .await
    }

    /// `session/set_mode` — switches the session's mode.
    ///
    /// Only some agents advertise modes at all; the caller is expected to have
    /// read `session/new`'s `modes` before calling.
    ///
    /// Deadlined like every control request, but by the time a mode is switched
    /// a session exists, so overrunning costs this call and nothing else — the
    /// agent may simply be mid-turn, and a mode that did not change is a far
    /// smaller loss than a turn that was killed for it.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`], plus [`Error::Timeout`].
    pub async fn set_session_mode(
        &self,
        request: schema::SetSessionModeRequest,
    ) -> Result<schema::SetSessionModeResponse> {
        self.request(schema::AGENT_METHOD_NAMES.session_set_mode, &request)
            .await
    }

    /// `session/set_config_option` — sets one of the options the session
    /// advertised.
    ///
    /// This is how a model is chosen on an agent that offers the choice in
    /// protocol rather than at launch, and it is the only mechanism that is the
    /// same across agents: `session/new` answers with `configOptions`, one of
    /// which has the category `model`, and its `id` comes back here. The launch
    /// registry's [`crate::ModelPin`] is the other half — the agents that take a
    /// model only as an argument or an environment variable — and a caller
    /// needs both, because which one an agent offers is the agent's decision
    /// and not ours.
    ///
    /// Deadlined and forgiving of overrun for the same reason as
    /// [`AgentConnection::set_session_mode`]: a session already exists, and an
    /// option that did not change costs less than a turn killed for it.
    ///
    /// # Errors
    ///
    /// As [`AgentConnection::initialize`], plus [`Error::Timeout`].
    pub async fn set_config_option(
        &self,
        request: schema::SetSessionConfigOptionRequest,
    ) -> Result<schema::SetSessionConfigOptionResponse> {
        self.request(
            schema::AGENT_METHOD_NAMES.session_set_config_option,
            &request,
        )
        .await
    }

    /// `session/cancel` — a notification, so it returns as soon as the frame is
    /// queued rather than waiting for an answer.
    ///
    /// The acknowledgement is the in-flight [`AgentConnection::prompt`]
    /// resolving with `StopReason::Cancelled`.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] if the notification cannot be serialized, or
    /// [`Error::Closed`] if the connection is already gone.
    pub fn cancel(&self, notification: &schema::CancelNotification) -> Result<()> {
        self.notify(schema::AGENT_METHOD_NAMES.session_cancel, notification)
    }

    // --- Plumbing -----------------------------------------------------------

    /// Sends a control request and waits for its answer, no longer than the
    /// connection's deadline.
    async fn request<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: &P,
    ) -> Result<T> {
        self.call(method, params, Some(self.shared.request_timeout), None)
            .await
    }

    /// Sends a request and waits for its answer for as long as it takes. Only
    /// `session/prompt` is entitled to this — see [`AgentConnection::prompt`].
    async fn request_without_deadline<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: &P,
    ) -> Result<T> {
        self.call(method, params, None, None).await
    }

    /// Sends a request and waits for its answer, optionally under a deadline.
    async fn call<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: &P,
        deadline: Option<Duration>,
        dispatched: Option<oneshot::Sender<()>>,
    ) -> Result<T> {
        let params =
            serde_json::to_value(params).map_err(|source| Error::Encode { method, source })?;

        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        let frame = serde_json::to_string(&OutgoingFrame::Request {
            jsonrpc: JSONRPC_VERSION,
            id,
            method,
            params: &params,
        })
        .map_err(|source| Error::Encode { method, source })?;

        let (tx, rx) = oneshot::channel();
        // Registering before the send is what makes an answer that comes back
        // faster than this task is rescheduled still find its waiter.
        self.register_pending(method, id, tx)?;

        if self
            .shared
            .outgoing
            .send(Outgoing { frame, dispatched })
            .is_err()
        {
            self.shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
            return Err(Error::Closed);
        }

        // A dropped sender means the connection died with the request in
        // flight — the only way this receiver errors.
        let answer = match deadline {
            Some(deadline) => {
                let Ok(answer) = tokio::time::timeout(deadline, rx).await else {
                    return Err(self.overran(method, deadline));
                };
                answer
            }
            None => rx.await,
        };

        let payload = answer
            .map_err(|_| Error::Closed)?
            .map_err(|source| Error::Rpc { method, source })?;

        serde_json::from_value(payload.clone()).map_err(|source| Error::MalformedResponse {
            method,
            payload,
            source,
        })
    }

    /// Sends a notification. Nothing comes back, by definition.
    fn notify<P: Serialize>(&self, method: &'static str, params: &P) -> Result<()> {
        let params =
            serde_json::to_value(params).map_err(|source| Error::Encode { method, source })?;
        let frame = serde_json::to_string(&OutgoingFrame::Notification {
            jsonrpc: JSONRPC_VERSION,
            method,
            params: &params,
        })
        .map_err(|source| Error::Encode { method, source })?;

        self.shared
            .outgoing
            .send(Outgoing {
                frame,
                dispatched: None,
            })
            .map_err(|_| Error::Closed)
    }

    /// Files a waiter under `id`, refusing if the connection has already ended.
    ///
    /// The refusal is checked on both sides of the insert: before, so a request
    /// raised on a dead connection fails at once, and after, so a request that
    /// raced the reader's EOF does not sit in a map nobody will ever drain.
    fn register_pending(&self, method: &'static str, id: i64, tx: PendingReply) -> Result<()> {
        if let Some(refusal) = self.refusal(method) {
            return Err(refusal);
        }
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.insert(id, tx);
        if let Some(refusal) = self.refusal(method) {
            pending.remove(&id);
            return Err(refusal);
        }
        Ok(())
    }

    /// Why a request cannot be raised at all, if it cannot.
    ///
    /// An expired connection answers with the same [`Error::Timeout`] the first
    /// caller got rather than [`Error::Closed`]: the reason a call cannot be
    /// made is still the agent's silence, and collapsing that into "closed"
    /// would lose the one thing the user needs to be told.
    fn refusal(&self, method: &'static str) -> Option<Error> {
        if self.shared.expired.load(Ordering::SeqCst) {
            return Some(Error::Timeout {
                method,
                timeout: self.shared.request_timeout,
            });
        }
        self.is_closed().then_some(Error::Closed)
    }

    /// Turns a request's overrun into its error, and settles what else the
    /// overrun costs.
    ///
    /// The frame was taken and never answered: the agent is up and silent. What
    /// that is worth depends on whether it has been given anything of the
    /// user's yet.
    ///
    /// With no session on this connection, silence means the agent never came
    /// up. Nothing is under way, and nobody holds a session to close it by
    /// later — so the connection is given up on and its process killed, or it
    /// would outlive the application's interest in it entirely.
    ///
    /// With a session established, the same silence may be nothing worse than
    /// the agent being busy with a turn: a mode switch can arrive mid-turn.
    /// Only this call fails then. Ending the process would take the user's work
    /// with it, and the session it belongs to can still close it later.
    fn overran(&self, method: &'static str, timeout: Duration) -> Error {
        if self.shared.sessions.load(Ordering::SeqCst) == 0 {
            self.expire();
        }
        Error::Timeout { method, timeout }
    }

    /// Records that a session now exists on this connection.
    ///
    /// Counted after the answer, never before: a `session/new` that is still in
    /// flight has established nothing, and it is exactly that window the
    /// deadline defends.
    fn session_established(&self) {
        self.shared.sessions.fetch_add(1, Ordering::SeqCst);
    }

    /// Gives the connection up after a control request overran its deadline.
    ///
    /// Everything still waiting is released, every later request is refused
    /// without going near the agent, and the overrun is announced so whoever
    /// owns the agent's process can kill it.
    fn expire(&self) {
        self.shared.expired.store(true, Ordering::SeqCst);
        self.shared.closed.store(true, Ordering::SeqCst);
        self.shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        // The previous value is of no interest; the announcement is.
        let _ = self.shared.expiry.send_replace(true);
    }

    /// A receiver that turns `true` once a control request overran its
    /// deadline.
    ///
    /// Crate-internal: outside this crate nobody owns the agent's process, and
    /// the only thing to do with this signal is kill it.
    pub(crate) fn expiry(&self) -> watch::Receiver<bool> {
        self.shared.expiry.subscribe()
    }
}

/// A frame on its way out. Serialized by hand rather than through the protocol
/// crate's envelope types because notifications and requests differ only by the
/// presence of `id`, and `serde` says that most plainly here.
#[derive(Serialize)]
#[serde(untagged)]
enum OutgoingFrame<'a> {
    Request {
        jsonrpc: &'static str,
        id: i64,
        method: &'static str,
        params: &'a serde_json::Value,
    },
    Notification {
        jsonrpc: &'static str,
        method: &'static str,
        params: &'a serde_json::Value,
    },
    Response {
        jsonrpc: &'static str,
        id: &'a serde_json::Value,
        result: &'a serde_json::Value,
    },
    ErrorResponse {
        jsonrpc: &'static str,
        id: &'a serde_json::Value,
        error: &'a RpcError,
    },
}

/// A frame as it arrives. Every field is optional because which ones are
/// present is exactly what tells the four kinds of frame apart.
#[derive(Deserialize)]
struct IncomingFrame {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

/// Something to hand on in the order the agent wrote it.
enum Delivery {
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
    Response {
        id: i64,
        outcome: std::result::Result<serde_json::Value, RpcError>,
    },
}

struct Outgoing {
    frame: String,
    dispatched: Option<oneshot::Sender<()>>,
}

/// Drains queued frames onto the agent's stdin, one line each.
async fn write_loop<W>(mut writer: W, mut outgoing: mpsc::UnboundedReceiver<Outgoing>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    while let Some(outgoing) = outgoing.recv().await {
        if let Err(error) = writer.write_all(outgoing.frame.as_bytes()).await {
            tracing::warn!(%error, "ACP write failed; agent stdin is gone");
            break;
        }
        if let Err(error) = writer.write_all(b"\n").await {
            tracing::warn!(%error, "ACP write failed; agent stdin is gone");
            break;
        }
        if let Err(error) = writer.flush().await {
            tracing::warn!(%error, "ACP flush failed; agent stdin is gone");
            break;
        }
        if let Some(dispatched) = outgoing.dispatched {
            let _ = dispatched.send(());
        }
    }
}

/// Reads the agent's stdout, classifies each frame, and routes it.
async fn read_loop<R, H>(
    reader: R,
    delivery: mpsc::UnboundedSender<Delivery>,
    shared: Arc<Shared>,
    handler: Arc<H>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    H: ClientHandler,
{
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "ACP read failed; treating the agent as gone");
                break;
            }
        }

        // Lossy on purpose: a byte sequence that is not UTF-8 cannot be a
        // protocol frame, and it must not end the session either. It falls
        // through to the parse below and is logged as noise.
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }

        let frame = match serde_json::from_str::<IncomingFrame>(text) {
            Ok(frame) => frame,
            Err(error) => {
                // Adapters and CLIs write banners and warnings to stdout.
                tracing::debug!(%error, line = %text, "non-protocol line on agent stdout");
                continue;
            }
        };

        match (frame.method, frame.id) {
            // Notification: a method, no id.
            (Some(method), None) => {
                if delivery
                    .send(Delivery::Notification {
                        method,
                        params: frame.params,
                    })
                    .is_err()
                {
                    break;
                }
            }
            // Request from the agent: a method and an id. Handled off the
            // ordered path so a handler that waits for a human cannot stall
            // another session's stream.
            (Some(method), Some(id)) => {
                tokio::spawn(answer_agent_request(
                    Arc::clone(&shared),
                    Arc::clone(&handler),
                    method,
                    id,
                    frame.params,
                ));
            }
            // Response to one of ours: an id, no method.
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else {
                    tracing::warn!(?id, "agent answered with an id this client never issued");
                    continue;
                };
                let outcome = match frame.error {
                    Some(error) => Err(error),
                    None => Ok(frame.result.unwrap_or(serde_json::Value::Null)),
                };
                if delivery.send(Delivery::Response { id, outcome }).is_err() {
                    break;
                }
            }
            (None, None) => {
                tracing::debug!(line = %text, "JSON on agent stdout that is not a JSON-RPC frame");
            }
        }
    }

    // Dropping `delivery` is what tells the delivery loop to shut down and
    // fail everyone still waiting.
    drop(delivery);
}

/// Delivers notifications and answers in the order the agent wrote them.
async fn delivery_loop<H>(
    mut deliveries: mpsc::UnboundedReceiver<Delivery>,
    shared: Arc<Shared>,
    handler: Arc<H>,
) where
    H: ClientHandler,
{
    while let Some(delivery) = deliveries.recv().await {
        match delivery {
            Delivery::Notification { method, params } => {
                deliver_notification(&handler, method, params).await;
            }
            Delivery::Response { id, outcome } => {
                let waiter = shared
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                // A missing waiter means the caller stopped waiting, or the
                // agent answered an id it was never given. Both deserve a line
                // and neither deserves ending the connection over.
                if let Some(tx) = waiter {
                    drop(tx.send(outcome));
                } else {
                    tracing::warn!(id, "answer to a request this client is not awaiting");
                }
            }
        }
    }

    // The agent is gone. Nobody will ever be answered, so say so now rather
    // than leaving callers parked forever: dropping each sender surfaces as
    // `Error::Closed` on the awaiting side.
    shared.closed.store(true, Ordering::SeqCst);
    shared
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Routes one notification to the handler.
async fn deliver_notification<H: ClientHandler>(
    handler: &Arc<H>,
    method: String,
    params: Option<serde_json::Value>,
) {
    if method != schema::CLIENT_METHOD_NAMES.session_update {
        handler.unhandled_notification(&method, params).await;
        return;
    }

    let Some(params) = params else {
        handler.unhandled_notification(&method, None).await;
        return;
    };

    match decode_session_update(params.clone()) {
        Ok(event) => handler.session_update(event).await,
        Err(error) => {
            // No `sessionId`, so there is nothing to attribute the update to.
            tracing::warn!(%error, "session/update without a usable sessionId");
            handler.unhandled_notification(&method, Some(params)).await;
        }
    }
}

/// Runs one agent → client request through the handler and writes the answer.
async fn answer_agent_request<H: ClientHandler>(
    shared: Arc<Shared>,
    handler: Arc<H>,
    method: String,
    id: serde_json::Value,
    params: Option<serde_json::Value>,
) {
    let names = schema::CLIENT_METHOD_NAMES;
    let outcome = if method == names.session_request_permission {
        typed_call(params, |request| async move {
            handler.request_permission(request).await
        })
        .await
    } else if method == names.fs_read_text_file {
        typed_call(params, |request| async move {
            handler.read_text_file(request).await
        })
        .await
    } else if method == names.fs_write_text_file {
        typed_call(params, |request| async move {
            handler.write_text_file(request).await
        })
        .await
    } else {
        handler.unhandled_request(&method, params).await
    };

    let frame = match &outcome {
        Ok(result) => serde_json::to_string(&OutgoingFrame::Response {
            jsonrpc: JSONRPC_VERSION,
            id: &id,
            result,
        }),
        Err(error) => serde_json::to_string(&OutgoingFrame::ErrorResponse {
            jsonrpc: JSONRPC_VERSION,
            id: &id,
            error,
        }),
    };

    match frame {
        Ok(frame) => drop(shared.outgoing.send(Outgoing {
            frame,
            dispatched: None,
        })),
        Err(error) => {
            // The handler produced something unserializable. The agent is
            // waiting on this id, so it still has to be answered.
            tracing::warn!(%error, %method, "could not encode the answer to an agent request");
            let fallback = RpcError::internal("client could not encode its answer");
            if let Ok(frame) = serde_json::to_string(&OutgoingFrame::ErrorResponse {
                jsonrpc: JSONRPC_VERSION,
                id: &id,
                error: &fallback,
            }) {
                drop(shared.outgoing.send(Outgoing {
                    frame,
                    dispatched: None,
                }));
            }
        }
    }
}

/// Deserializes a request's params, runs `call`, and re-serializes its answer.
async fn typed_call<P, T, F, Fut>(
    params: Option<serde_json::Value>,
    call: F,
) -> std::result::Result<serde_json::Value, RpcError>
where
    P: DeserializeOwned,
    T: Serialize,
    F: FnOnce(P) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, RpcError>>,
{
    let request: P = serde_json::from_value(params.unwrap_or(serde_json::Value::Null))
        .map_err(RpcError::invalid_params)?;
    let response = call(request).await?;
    serde_json::to_value(response).map_err(RpcError::internal)
}
