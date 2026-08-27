use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Why a memory operation failed, in the terms the UI has to act on.
///
/// memory-hub answers domain failures with a stable `kind` string; this enum is
/// that contract made explicit, so a caller cannot forget a case the interface
/// documents. Anything unrecognised lands in [`MemoryErrorKind::Other`] with the
/// original string preserved — a newer engine may know kinds this build does
/// not, and that is not a reason to lose the message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MemoryErrorKind {
    /// A write raced another writer on the same key. Both revisions are in
    /// `data`; refresh and replay.
    Conflict,
    /// A record did not satisfy its `__type__` definition.
    InvalidRecord,
    /// An argument was missing or malformed — a bug in this client.
    InvalidArgument,
    /// The transaction id was already used. Retrying with a fresh id is safe;
    /// reusing one is not.
    TransactionReused,
    /// The search index is unusable. Offer a reindex.
    Index,
    /// Push was blocked by the stale-record policy.
    PushBlocked,
    /// No memory remote is configured.
    NoRemoteConfigured,
    /// Code history moved in a way reconciliation cannot follow without an
    /// explicit full rebuild.
    Diverged,
    /// The engine speaks a memory interface major this build does not.
    IncompatibleMemoryInterface,
    /// This repository holds no Sync memory. Not a failure to report: opening
    /// the project in Sync is what creates memory, and that is the answer to
    /// it. Sync's own, now that the engine opens whatever storage its host
    /// named — the agent surface refuses in these words rather than giving a
    /// repository memory because an agent connected to it.
    NotInitialised,
    /// The engine understood the request and this project's storage cannot do
    /// it — diff and transport against records that are
    /// files rather than Git objects. A fact about the project, not a fault.
    Unsupported,
    /// A kind this build does not know about.
    #[serde(untagged)]
    Other(String),
}

impl MemoryErrorKind {
    #[must_use]
    pub fn from_wire(kind: &str) -> Self {
        match kind {
            "conflict" => Self::Conflict,
            "invalid_record" => Self::InvalidRecord,
            "invalid_argument" => Self::InvalidArgument,
            "transaction_reused" => Self::TransactionReused,
            "index" => Self::Index,
            "push_blocked" => Self::PushBlocked,
            "no_remote_configured" => Self::NoRemoteConfigured,
            "diverged" => Self::Diverged,
            "incompatible_memory_interface" => Self::IncompatibleMemoryInterface,
            "not_initialised" => Self::NotInitialised,
            "unsupported" => Self::Unsupported,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The wire spelling, so a round-trip through this enum is lossless.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Conflict => "conflict",
            Self::InvalidRecord => "invalid_record",
            Self::InvalidArgument => "invalid_argument",
            Self::TransactionReused => "transaction_reused",
            Self::Index => "index",
            Self::PushBlocked => "push_blocked",
            Self::NoRemoteConfigured => "no_remote_configured",
            Self::Diverged => "diverged",
            Self::IncompatibleMemoryInterface => "incompatible_memory_interface",
            Self::NotInitialised => "not_initialised",
            Self::Unsupported => "unsupported",
            Self::Other(kind) => kind,
        }
    }
}

/// Anything that can go wrong between Sync and the memory engine.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// The engine reported a domain failure.
    #[error("{message}")]
    Domain {
        kind: MemoryErrorKind,
        message: String,
        /// Structured detail from the engine: conflicting keys, the revision it
        /// expected, a recovery action.
        data: Value,
    },

    /// The sidecar could not be found, started, or kept alive.
    #[error("memory engine unavailable: {0}")]
    Sidecar(String),

    /// The engine answered something this client cannot parse. A protocol
    /// mismatch rather than a domain failure.
    #[error("memory engine returned an unusable response: {0}")]
    Protocol(String),

    #[error("memory engine I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl MemoryError {
    /// Build a domain error from the engine's `{kind, message, data}` shape.
    #[must_use]
    pub fn domain(kind: &str, message: impl Into<String>, data: Value) -> Self {
        Self::Domain {
            kind: MemoryErrorKind::from_wire(kind),
            message: message.into(),
            data,
        }
    }

    /// The failure kind, when this is a domain failure the UI can act on.
    #[must_use]
    pub const fn kind(&self) -> Option<&MemoryErrorKind> {
        match self {
            Self::Domain { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// Whether this is the engine saying the project has never been
    /// initialised — the one failure that is answered by doing something
    /// rather than by telling somebody.
    #[must_use]
    pub fn is_not_initialised(&self) -> bool {
        matches!(
            self,
            Self::Domain {
                kind: MemoryErrorKind::NotInitialised,
                ..
            }
        )
    }

    /// Whether replaying the operation against a fresh revision could succeed.
    ///
    /// Only a same-key conflict qualifies: everything else either needs the
    /// user or is a bug in the request.
    #[must_use]
    pub fn is_retryable_conflict(&self) -> bool {
        matches!(
            self,
            Self::Domain {
                kind: MemoryErrorKind::Conflict,
                ..
            }
        )
    }
}

pub type Result<T> = std::result::Result<T, MemoryError>;
