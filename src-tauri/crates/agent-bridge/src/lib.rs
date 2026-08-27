//! Provider-neutral process bridges for agent clients.
//!
//! The public seam is ACP over byte streams. A provider adapter translates its
//! native process protocol behind that seam. The first adapter is Codex's
//! official `app-server`; native ACP agents do not need translation.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod codex;

pub use codex::{run_codex_stdio, serve_codex, CodexOptions};

/// A bridge failure with enough context for a CLI to report it on stderr.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Starting or talking to the provider process failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A JSON-RPC frame could not be encoded.
    #[error("could not encode {side} JSON-RPC frame: {source}")]
    Encode {
        side: &'static str,
        source: serde_json::Error,
    },
    /// The provider process did not expose the expected stdio pipe.
    #[error("Codex app-server did not expose {0}")]
    MissingPipe(&'static str),
    /// The bridge was launched outside a Tokio runtime where one was required.
    #[error("could not create the bridge runtime: {0}")]
    Runtime(std::io::Error),
}

/// Result alias for bridge operations.
pub type Result<T> = std::result::Result<T, Error>;
