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
