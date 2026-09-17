//! The client half of the conversation: what the agent asks *us*.
//!
//! ACP is bidirectional. Besides answering our requests, an agent sends
//! notifications we consume (`session/update`) and requests we must answer
//! (`session/request_permission`, `fs/read_text_file`, `fs/write_text_file`).
//! This trait is the whole seam between the protocol and whatever sits above
//! it — implement it and the connection has everywhere to deliver.
//!
//! Only Grok was measured calling `fs/*` at all; Claude, Codex and `OpenCode`
//! reach files with their own tools. The methods are still required rather
//! than optional, because "the agent may never call it" is not the same fact
//! as "the client may answer it wrong", and Grok cannot read a skill without
//! them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_client_protocol_schema::v1 as schema;
use async_trait::async_trait;

use crate::error::RpcError;
use crate::update::SessionUpdateEvent;

/// Everything an ACP agent can send towards the client.
///
/// Ordering guarantee, and its limit: `session_update` calls and the answers
/// to our own requests are delivered in the order the agent wrote them, so the
/// `stopReason` of a turn can never overtake that turn's last message chunk.
/// Agent *requests* (the other three methods) are handled concurrently and
/// carry no ordering relation to the updates around them — a permission
/// request for a tool call may reach you before the `tool_call` update that
/// describes it. That is deliberate: a request handler may sit waiting for a
/// human, and one session's prompt must not stall another session's stream.
#[async_trait]
pub trait ClientHandler: Send + Sync + 'static {
    /// A `session/update` notification arrived.
    ///
    /// Keep it quick — this call is on the ordered delivery path, so time
    /// spent here delays every later update and response on the connection.
    /// Forwarding into a channel is the intended shape.
    async fn session_update(&self, event: SessionUpdateEvent);

    /// The agent asks the user to approve an operation.
    ///
    /// # Errors
    ///
    /// Return an [`RpcError`] to answer the agent with a JSON-RPC error rather
    /// than an outcome. Declining the operation is *not* an error — that is a
    /// `RequestPermissionOutcome`.
    async fn request_permission(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Result<schema::RequestPermissionResponse, RpcError>;

    /// The agent asks the client to read a text file on its behalf.
    ///
    /// # Errors
    ///
    /// Return an [`RpcError`] when the file cannot be read; the agent expects
    /// to be told, not to be handed empty content.
    async fn read_text_file(
        &self,
        request: schema::ReadTextFileRequest,
    ) -> Result<schema::ReadTextFileResponse, RpcError>;

    /// The agent asks the client to write a text file on its behalf.
    ///
    /// # Errors
    ///
    /// Return an [`RpcError`] when the write cannot be performed.
    async fn write_text_file(
        &self,
        request: schema::WriteTextFileRequest,
    ) -> Result<schema::WriteTextFileResponse, RpcError>;

    /// A notification we have no typed route for — an agent extension, or a
    /// method from a protocol revision newer than this client.
    ///
    /// The default drops it after a `debug` line. Override to observe them.
    async fn unhandled_notification(&self, method: &str, params: Option<serde_json::Value>) {
        tracing::debug!(method, ?params, "unhandled ACP notification from agent");
    }

    /// A request we have no typed route for.
    ///
    /// The default answers `-32601 method not found`, which is what the agent
    /// needs to hear: an unanswered request leaves it waiting forever. Override
    /// only to implement a method this client does not model — never to answer
    /// something you cannot actually do.
    ///
    /// # Errors
    ///
    /// The default implementation always errors, by design.
    async fn unhandled_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, RpcError> {
        tracing::debug!(method, ?params, "unhandled ACP request from agent");
        Err(RpcError::method_not_found(method))
    }
}

/// Lets `Arc<H>` stand in wherever a handler is wanted, so one handler can be
/// shared by the connection and by whoever built it.
#[async_trait]
impl<H: ClientHandler> ClientHandler for Arc<H> {
    async fn session_update(&self, event: SessionUpdateEvent) {
        (**self).session_update(event).await;
    }

    async fn request_permission(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Result<schema::RequestPermissionResponse, RpcError> {
        (**self).request_permission(request).await
    }

    async fn read_text_file(
        &self,
        request: schema::ReadTextFileRequest,
    ) -> Result<schema::ReadTextFileResponse, RpcError> {
        (**self).read_text_file(request).await
    }

    async fn write_text_file(
        &self,
        request: schema::WriteTextFileRequest,
    ) -> Result<schema::WriteTextFileResponse, RpcError> {
        (**self).write_text_file(request).await
    }

    async fn unhandled_notification(&self, method: &str, params: Option<serde_json::Value>) {
        (**self).unhandled_notification(method, params).await;
    }

    async fn unhandled_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, RpcError> {
        (**self).unhandled_request(method, params).await
    }
}

// ---------------------------------------------------------------------------
// The answers every handler gives the same way.
// ---------------------------------------------------------------------------

/// Resolves a path an agent named, refusing anything outside `root`.
///
/// The agent is a separate program and its requests are input rather than
/// instructions: a turn opened on one directory must not be a way to read a
/// file in another. Resolved against the real directory, so that a symlink or a
/// `..` cannot step out of it.
///
/// Here rather than beside the handler that asks it, because the rule belongs
/// to raising a turn rather than to any one thing done with one: a second
/// spelling of *is this path inside that directory* is the kind of pair that
/// agrees for a year and then disagrees about one symlink, and the crate that
/// raises the turns is where the one spelling goes.
///
/// **As much of the path as exists is resolved, and the rest has to be ordinary
/// names.** A write may name a file that is not there — that is what a write
/// is — and it may name directories that are not there either, which the
/// handler answering it creates. A rule that could only resolve what already
/// exists would refuse `docs/new/notes.md` as *outside this directory*, which
/// is both a refusal and a lie about where the path pointed. What the remainder
/// is held to instead is that every component of it is a plain name: `..`
/// reaches [`Path::file_name`] as `None` and is refused there, which is what
/// keeps a path from walking back out of the part that did resolve.
///
/// # Errors
///
/// [`RpcError::invalid_params`] naming the path, for anything that does not
/// land inside `root`. The agent is told rather than left waiting: an
/// unanswered request keeps it believing this side is about to hand it
/// something.
pub fn contained(root: &Path, named: &str) -> Result<PathBuf, RpcError> {
    let requested = PathBuf::from(named);
    let absolute = if requested.is_absolute() {
        requested
    } else {
        root.join(requested)
    };
    let outside = || RpcError::invalid_params(format!("{named} is outside this turn's directory"));

    let root = root.canonicalize().map_err(|_| outside())?;

    // Up to the nearest ancestor that is really there, keeping what was walked
    // past. The loop ends: every path has an ancestor that resolves or a parent
    // that is `None`, and the second is refused.
    let mut unmade = Vec::new();
    let mut walking = absolute.as_path();
    let anchor = loop {
        if let Ok(resolved) = walking.canonicalize() {
            break resolved;
        }
        // `None` for a component that is not a plain name — a path ending in
        // `..`, or a root with nothing above it.
        let name = walking.file_name().ok_or_else(outside)?;
        unmade.push(name.to_owned());
        walking = walking.parent().ok_or_else(outside)?;
    };

    let mut landing = anchor;
    for name in unmade.iter().rev() {
        landing.push(name);
    }
    if landing.starts_with(&root) {
        Ok(landing)
    } else {
        Err(outside())
    }
}

/// The option that allows this one operation and nothing after it.
///
/// `allow_always` is deliberately not taken by any unattended turn in this
/// build, even for something a person agreed to for good. It is remembered by
/// the agent, in a store this installation cannot read, show or clear — so the
/// agreement would outlive its withdrawal here and nothing on this side would
/// know. Measured, and worse than the general case: Claude's own *Always
/// Allow* carries `lifetime: persistent, storage: project_local`, which writes
/// a rule into the person's project.
///
/// `None` where the agent offered no once-only *yes*: an agent whose only way
/// of saying yes is forever gets no from here.
#[must_use]
pub(crate) fn allow_once(
    options: &[schema::PermissionOption],
) -> Option<schema::PermissionOptionId> {
    options
        .iter()
        .find(|option| matches!(option.kind, schema::PermissionOptionKind::AllowOnce))
        .map(|option| option.option_id.clone())
}

/// The narrowest *no* the agent offered.
///
/// Narrowest so that nothing is remembered on its side about a question this
/// side was never able to put to anybody. Where it offers no such option the
/// turn is cancelled, which is the protocol's word for the truth here.
#[must_use]
pub(crate) fn narrowest_no(
    options: &[schema::PermissionOption],
) -> schema::RequestPermissionOutcome {
    let rejected = options
        .iter()
        .find(|option| matches!(option.kind, schema::PermissionOptionKind::RejectOnce))
        .map(|option| option.option_id.clone());

    match rejected {
        Some(option_id) => schema::RequestPermissionOutcome::Selected(
            schema::SelectedPermissionOutcome::new(option_id),
        ),
        None => schema::RequestPermissionOutcome::Cancelled,
    }
}

/// The answer that lets one operation happen.
#[must_use]
pub(crate) fn allowed(option_id: schema::PermissionOptionId) -> schema::RequestPermissionOutcome {
    schema::RequestPermissionOutcome::Selected(schema::SelectedPermissionOutcome::new(option_id))
}
