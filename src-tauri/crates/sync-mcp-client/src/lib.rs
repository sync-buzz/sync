//! Speaking MCP as a client, to a server a project declares.
//!
//! The shell's other MCP half — [`sync-mcp`] — *publishes* a server: an agent
//! connects to it. This crate is the opposite direction: a project *declares*
//! an MCP server (a stdio command or an HTTP endpoint, with credentials in the
//! vault), and this crate is how the shell reaches it as a client to call a
//! tool or list the tools on offer.
//!
//! **Per-call, not pooled.** Every call connects, does the handshake, asks one
//! question, and shuts down. The cost is the latency of a handshake on every
//! call; the gain is that there is no connection state to keep consistent with
//! a project record that can change between two calls. A server worth keeping
//! warm is worth a pool for, and the upgrade path is a held connection keyed by
//! the declaration — but that is not this version.
//!
//! **The crate knows no vault.** A call arrives with its secrets already
//! resolved into a map, keyed by the name the transport carries. Resolving them
//! — reading `mcp-{id}/{secret}` from the keychain — is the command layer's
//! business, because the keychain is a side effect and this crate is the half
//! that can be tested without one. A missing secret is an error here; the
//! command layer decides whether that means *skip the server* or *fail the
//! call*.
//!
//! [`sync-mcp`]: crate

use std::collections::HashMap;

use http::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject, ListToolsResult, Tool};
use rmcp::service::{self, RoleClient, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransportConfig, StreamableHttpClientWorker,
};
use sync_memory::{McpEnvSecret, McpHeaderSecret, McpTransportConfig};
use tokio::process::Command;

/// The secrets a declared MCP server needs, resolved from the vault and handed
/// to a call.
///
/// Keyed by the `secret` name the transport carries — the same string the
/// project record stores and the vault reads under `mcp-{id}/{secret}`. The
/// crate never sees the vault; it sees the values, and only the ones the caller
/// resolved.
#[derive(Debug, Default)]
pub struct ResolvedSecrets(pub HashMap<String, String>);

impl ResolvedSecrets {
    /// Build the map from the `(secret-name, value)` pairs the vault returned.
    #[must_use]
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        Self(pairs.into_iter().collect())
    }

    fn get(&self, secret: &str) -> Option<&str> {
        self.0.get(secret).map(String::as_str)
    }
}

impl From<HashMap<String, String>> for ResolvedSecrets {
    fn from(map: HashMap<String, String>) -> Self {
        Self(map)
    }
}

/// What can go wrong reaching a declared MCP server.
///
/// Each variant is a sentence a person or an agent reads, because a call that
/// fails is a call somebody asked for and the reason it did not happen is the
/// whole of the answer.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("the MCP server's stdio command could not be started: {0}")]
    Spawn(String),
    #[error("the MCP server would not complete the handshake: {0}")]
    Handshake(String),
    #[error("the MCP server did not answer: {0}")]
    Call(String),
    #[error("the secret `{0}` was not resolved from the vault")]
    MissingSecret(String),
    #[error("the MCP transport could not be built: {0}")]
    Transport(String),
}

/// Call one tool on a declared MCP server.
///
/// Connects, does the `initialize` handshake, sends `tools/call`, and shuts
/// down — one round trip, one connection, no state kept between calls. The
/// result is the server's own answer, carried whole: the content blocks, the
/// structured result and the error flag are the server's to fill and the
/// caller's to read.
///
/// # Errors
///
/// [`McpClientError::MissingSecret`] when a secret the transport names was not
/// in `secrets`; the other variants when the server could not be reached, would
/// not shake hands, or refused the call.
pub async fn call(
    transport: &McpTransportConfig,
    secrets: &ResolvedSecrets,
    tool: &str,
    arguments: Option<&JsonObject>,
) -> Result<CallToolResult, McpClientError> {
    let service = connect(transport, secrets).await?;
    let mut params = CallToolRequestParams::new(tool.to_owned());
    if let Some(arguments) = arguments {
        params = params.with_arguments(arguments.clone());
    }
    let result = service.call_tool(params).await.map_err(|error| {
        tracing::warn!(%error, tool, "MCP tools/call failed");
        McpClientError::Call(error.to_string())
    })?;
    let _ = service.cancel().await;
    Ok(result)
}

/// List the tools a declared MCP server offers.
///
/// The same round trip as [`call`], answered with the server's tool catalogue.
/// A project record does not carry an MCP server's tools the way it carries a
/// package's declarations — an MCP server discovers them at run time — so this
/// is how a caller that wants them gets them.
///
/// # Errors
///
/// See [`call`]; the failure modes are the same up to the final request.
pub async fn list_tools(
    transport: &McpTransportConfig,
    secrets: &ResolvedSecrets,
) -> Result<Vec<Tool>, McpClientError> {
    let service = connect(transport, secrets).await?;
    let ListToolsResult { tools, .. } = service.list_tools(None).await.map_err(|error| {
        tracing::warn!(%error, "MCP tools/list failed");
        McpClientError::Call(error.to_string())
    })?;
    let _ = service.cancel().await;
    Ok(tools)
}

/// A held connection to a declared MCP server, reused across calls.
///
/// The per-call [`call`] connects, asks, and shuts down — the honest ceiling
/// when nothing is worth keeping warm. A server whose process holds state a
/// caller needs across calls (a browser a `navigate` opened, a session a
/// `login` started) is worth a held connection for, and this is it: one
/// [`RunningService`] per server, keyed by the name the caller knows it by,
/// kept until the caller says the run is over via [`Pool::drain`].
///
/// **The secrets and transport a service was built from are not re-checked on
/// a reused call.** A pool lives for one run of one caller, and within a run
/// neither changes; the moment that stops being true is the moment a pool is
/// the wrong shape and a re-declaration on every call is. The caller that
/// holds the pool is the caller that decides its lifetime.
///
/// **Errors evict.** A call that fails to answer loses the service it was
/// made on: the next call to that server reconnects, because a service that
/// could not answer once is not one to ask again.
pub struct Pool {
    by_server: HashMap<String, RunningService<RoleClient, ()>>,
}

impl Pool {
    /// An empty pool. The caller that builds it owns the run it covers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_server: HashMap::new(),
        }
    }

    /// Call one tool on one server, reusing a held connection where one was
    /// made and making one where none was.
    ///
    /// `server` is the key the pool holds the connection under — the same name
    /// the caller uses to declare the server, not anything the server itself
    /// publishes. The first call to a name connects and stores; later calls to
    /// the same name reuse.
    ///
    /// # Errors
    ///
    /// See [`call`]; the failure modes are the same, with the addition that a
    /// reused service which fails to answer is evicted so the next call
    /// reconnects rather than asking a dead one again.
    pub async fn call(
        &mut self,
        transport: &McpTransportConfig,
        secrets: &ResolvedSecrets,
        server: &str,
        tool: &str,
        arguments: Option<&JsonObject>,
    ) -> Result<CallToolResult, McpClientError> {
        use std::collections::hash_map::Entry;
        // `connect` is awaited before the entry is taken, so the map is not
        // borrowed across the spawn; the entry is held only for the insert and
        // the call, which is the shortest hold that still reuses.
        let service = match self.by_server.entry(server.to_owned()) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(slot) => slot.insert(connect(transport, secrets).await?),
        };
        let mut params = CallToolRequestParams::new(tool.to_owned());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments.clone());
        }
        match service.call_tool(params).await {
            Ok(result) => Ok(result),
            Err(error) => {
                tracing::warn!(%error, tool, server, "pooled MCP tools/call failed");
                // The borrow `service` took of the map ends with the call, so
                // the entry can be taken back out and cancelled.
                if let Some(svc) = self.by_server.remove(server) {
                    let _ = svc.cancel().await;
                }
                Err(McpClientError::Call(error.to_string()))
            }
        }
    }

    /// Drop every held connection, shutting each server process down.
    ///
    /// The caller calls this when its run is over — a handler returning, a
    /// workflow finishing — so a server worth keeping warm for the length of a
    /// run does not outlive it. A stdio server's process (and the browser it
    /// holds) goes here; an HTTP server's client connection closes here, and
    /// the remote server is untouched, as it should be.
    pub async fn drain(&mut self) {
        for (_, svc) in self.by_server.drain() {
            let _ = svc.cancel().await;
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

/// Connect to a declared server and answer the running service, handshake done.
///
/// The match is the one place the three transports differ: stdio spawns a
/// child, HTTP and SSE both open a streamable HTTP client. The handshake is
/// common, because `serve_client` does it before it answers — a server that
/// would not initialise is refused here rather than failing on the first call.
async fn connect(
    transport: &McpTransportConfig,
    secrets: &ResolvedSecrets,
) -> Result<RunningService<RoleClient, ()>, McpClientError> {
    match transport {
        McpTransportConfig::Stdio { command, args, env } => {
            let mut cmd = Command::new(command);
            cmd.args(args);
            for McpEnvSecret { name, secret } in env {
                cmd.env(name, resolve(secrets, secret)?);
            }
            let child = TokioChildProcess::new(cmd)
                .map_err(|error| McpClientError::Spawn(error.to_string()))?;
            service::serve_client((), child)
                .await
                .map_err(|error| McpClientError::Handshake(error.to_string()))
        }
        McpTransportConfig::Http { url, headers } => http(url, headers, secrets).await,
        McpTransportConfig::Sse { url, headers } => {
            // ponytail: the old pure-SSE transport (an `/sse` endpoint with a
            // separate POST back) is not separately supported; both HTTP and
            // SSE declarations reach a streamable HTTP client. Every modern
            // server speaks streamable HTTP, and a legacy SSE-only server is
            // the ceiling — add a dedicated SSE client transport if one is met.
            http(url, headers, secrets).await
        }
    }
}

/// The HTTP/SSE half, shared by both variants.
async fn http(
    url: &str,
    headers: &[McpHeaderSecret],
    secrets: &ResolvedSecrets,
) -> Result<RunningService<RoleClient, ()>, McpClientError> {
    let config = StreamableHttpClientTransportConfig::with_uri(url)
        .custom_headers(build_headers(headers, secrets)?);
    let worker = StreamableHttpClientWorker::new(reqwest::Client::default(), config);
    service::serve_client((), worker)
        .await
        .map_err(|error| McpClientError::Handshake(error.to_string()))
}

/// Resolve a transport's header secrets into the HTTP headers a request sends.
///
/// A header's scheme, when present, prefixes the value — `Bearer` is the common
/// case, and `Authorization: Bearer <token>` is what it builds. Absent means
/// the value is sent verbatim.
fn build_headers(
    headers: &[McpHeaderSecret],
    secrets: &ResolvedSecrets,
) -> Result<HashMap<HeaderName, HeaderValue>, McpClientError> {
    let mut out = HashMap::new();
    for McpHeaderSecret {
        name,
        secret,
        scheme,
    } in headers
    {
        let value = resolve(secrets, secret)?;
        let written = match scheme {
            Some(scheme) if !scheme.is_empty() => format!("{scheme} {value}"),
            _ => value.to_owned(),
        };
        out.insert(
            HeaderName::try_from(name)
                .map_err(|error| McpClientError::Transport(error.to_string()))?,
            HeaderValue::from_str(&written)
                .map_err(|error| McpClientError::Transport(error.to_string()))?,
        );
    }
    Ok(out)
}

/// Look up one secret, or name the one that is missing.
fn resolve<'a>(secrets: &'a ResolvedSecrets, secret: &str) -> Result<&'a str, McpClientError> {
    secrets
        .get(secret)
        .ok_or_else(|| McpClientError::MissingSecret(secret.to_owned()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A scheme prefixes the value the way `Authorization: Bearer <token>`
    /// expects; its absence sends the value verbatim. Getting this wrong is the
    /// one mistake a server never tells you about — it just refuses.
    #[test]
    fn a_scheme_prefixes_the_value_and_its_absence_sends_it_verbatim() {
        let secrets = ResolvedSecrets::from_pairs([
            ("token".to_owned(), "abc123".to_owned()),
            ("key".to_owned(), "raw".to_owned()),
        ]);

        let with_scheme = build_headers(
            &[McpHeaderSecret {
                name: "Authorization".to_owned(),
                secret: "token".to_owned(),
                scheme: Some("Bearer".to_owned()),
            }],
            &secrets,
        )
        .expect("built");
        assert_eq!(
            with_scheme
                .get(&HeaderName::from_static("authorization"))
                .map(HeaderValue::to_str)
                .and_then(Result::ok),
            Some("Bearer abc123"),
        );

        let without = build_headers(
            &[McpHeaderSecret {
                name: "X-Key".to_owned(),
                secret: "key".to_owned(),
                scheme: None,
            }],
            &secrets,
        )
        .expect("built");
        assert_eq!(
            without
                .get(&HeaderName::from_static("x-key"))
                .map(HeaderValue::to_str)
                .and_then(Result::ok),
            Some("raw"),
        );
    }

    /// A missing secret names itself in the error, because *which one* is the
    /// whole of what a person or a call site can fix.
    #[test]
    fn a_missing_secret_names_itself() {
        let secrets = ResolvedSecrets::default();
        let error = resolve(&secrets, "github-token").expect_err("no secret resolves");
        assert!(matches!(
            error,
            McpClientError::MissingSecret(name) if name == "github-token"
        ));
    }

    /// An empty scheme is the same as no scheme — a header that arrives as
    /// `Bearer ` with nothing after it is worse than useless, and a scheme set
    /// to the empty string by a sloppy descriptor is treated as absent.
    #[test]
    fn an_empty_scheme_is_treated_as_absent() {
        let secrets = ResolvedSecrets::from_pairs([("token".to_owned(), "v".to_owned())]);
        let headers = build_headers(
            &[McpHeaderSecret {
                name: "Authorization".to_owned(),
                secret: "token".to_owned(),
                scheme: Some(String::new()),
            }],
            &secrets,
        )
        .expect("built");
        assert_eq!(
            headers
                .get(&HeaderName::from_static("authorization"))
                .map(HeaderValue::to_str)
                .and_then(Result::ok),
            Some("v"),
        );
    }
}
