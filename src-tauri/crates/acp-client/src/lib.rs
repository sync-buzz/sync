//! An Agent Client Protocol (ACP) client over a child process's stdio.
//!
//! This crate is the client half of ACP and nothing else. It knows about
//! JSON-RPC frames, session updates, permission requests and how each measured
//! agent CLI is raised — and knows nothing about Sync: no `.sync/`, no Tauri,
//! no PTY, no application event type. The dependency arrow only ever points
//! into it.
//!
//! # Where the shapes come from
//!
//! Not from the protocol's documentation. Every shape here was checked against
//! frames captured off five agent CLIs running live,
//! because the documentation says how it should be and the frames say how it
//! is, and those differ. The message types themselves are the protocol authors'
//! own (`agent-client-protocol-schema`), re-exported as [`schema`]; the
//! transport, the launch table and the tolerance for divergence are here.
//!
//! # Shape of the thing
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use acp_client::{launch, registry, schema, AgentProfile, ClientHandler};
//! # async fn example<H: ClientHandler>(handler: H) -> acp_client::Result<()> {
//! let command = launch::command_for(&registry::OPENCODE, &launch::SpawnOptions::default());
//! let agent = launch::spawn(command, handler)?;
//!
//! let profile = AgentProfile::new(
//!     agent
//!         .connection()
//!         .initialize(schema::InitializeRequest::new(
//!             acp_client::SUPPORTED_PROTOCOL_VERSION,
//!         ))
//!         .await?,
//! );
//! assert!(profile.speaks_our_protocol_version());
//! # Ok(())
//! # }
//! ```
//!
//! The turn's output does not come back from `prompt` — it arrives on the
//! [`ClientHandler`] as `session/update` notifications while the call is still
//! in flight. `prompt` resolves with the turn's `stopReason`, after the last of
//! them.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod capabilities;
pub mod connection;
pub mod error;
pub mod handler;
pub mod launch;
pub mod registry;
pub mod tool_names;
pub mod update;

/// The Agent Client Protocol message types, version 1.
///
/// Re-exported so a consumer never has to depend on the schema crate directly
/// and cannot end up compiling against a different revision of it than this
/// client does.
pub use agent_client_protocol_schema::v1 as schema;
pub use agent_client_protocol_schema::ProtocolVersion;

pub use capabilities::{AgentProfile, SUPPORTED_PROTOCOL_VERSION};
pub use connection::{AgentConnection, DEFAULT_REQUEST_TIMEOUT};
pub use error::{Error, Result, RpcError};
pub use handler::ClientHandler;
pub use launch::{AgentProcess, SpawnOptions};
pub use registry::{AcpMode, AgentLaunchSpec, ModelPin, Verification};
pub use tool_names::{McpToolName, McpToolNaming};
pub use update::{SessionUpdateEvent, SessionUpdatePayload, UnrecognizedUpdate};
