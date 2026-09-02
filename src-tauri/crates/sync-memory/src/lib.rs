//! Sync's client for the memory-hub engine.
//!
//! memory-hub ships as a separate process and speaks MCP over stdio. Sync
//! bundles that binary, runs one long-lived session per open project, and talks
//! to it exclusively through [`MemoryClient`]. No other part of Sync opens
//! `refs/memory/*`, touches the `LanceDB` index, or loads an embedding model —
//! keeping that boundary is what makes a crash in the engine's heaviest
//! dependencies a reconnect rather than a lost window.
//!
//! The crate compiles and runs without Tauri: the desktop layer is one possible
//! caller, not where the logic lives.

mod client;
mod dto;
mod error;
/// Sync's domain, mapped onto the engine's envelopes.
///
/// Public because the daemon builds envelopes from it, and it stays here rather
/// than moving into `sync-mcp` — which is a reversal of what the move assumed,
/// and it is measured rather than argued. Of the 43 names `sync-mcp` reaches
/// for, 29 are ones this crate needs too: the shapes an answer comes back in,
/// the kind vocabulary, the constants a writer and a reader both spell. Moving
/// the other 14 out would not cut the file in half, it would leave them
/// importing their own foundations back across a crate boundary — a worse seam
/// than none.
///
/// What *has* moved is the code that uses this. The window builds no envelopes
/// at all: it asks the host channel for settings, documents and records, and is
/// handed them. The flat re-exports below are what it and the end-to-end tests
/// consume, and nothing more — a re-export nobody imports is a promise this
/// crate has not been asked to keep.
pub mod mapping;
/// Every operation of the host channel, written once for both of its clients.
pub mod operations;
pub mod pairing;
mod process;
mod protocol;

pub use client::{EngineInfo, MemoryClient};
pub use dto::{
    ContentView, Counts, EntityInput, FetchOutcome, FolderEntry, Handshake, InstalledExtension,
    LinkInput, Listing, MemoryPresence, ModelStatus, Overlap, ProjectSettings, RecordView,
    RemoteCheck, ScanOutcome, SearchOutcome, SyncState, ToolDeclaration, TransactionResult,
    TransportStatus, Version,
};
pub use error::{CommandError, MemoryError, MemoryErrorKind, Result};
pub use operations::{Effect, Operations, effect};
/// The environment variable the MCP server's bearer token travels in.
///
/// Named here because both sides need the same word and neither owns it: the
/// window puts the token in the environment it starts the server with, and the
/// server reads it back. An argument would have put it in `ps`.
pub const SERVER_TOKEN_VARIABLE: &str = "SYNC_MCP_TOKEN";

/// The environment variable this machine's own network identity travels in.
///
/// Thirty-two bytes in hex: the key an `iroh` endpoint is built from, and
/// therefore the name devices dial. It is minted and kept by the application,
/// in the keychain, and reaches the engine the same way the bearer token does
/// and for the same reason — an argument is readable by every process here
/// through `ps`, and a machine's identity is exactly what must not be.
///
/// Absent, the engine opens no network door. That is how *off* is spelled:
/// there is no flag to disagree with the key's absence.
pub const REMOTE_KEY_VARIABLE: &str = "SYNC_REMOTE_KEY";

pub use mapping::{
    Dependents, Document, DocumentEdits, ENTITY_KINDS, Entity, EntityKind, GUIDANCE_FIELD, Link,
    OWN_KINDS, RecordType, RecordsPage, TYPE_KIND, TypeDeclaration, content_hash, type_definitions,
    type_record,
};
pub use process::{BinarySource, EngineBinary, LaunchConfig, resolve_binary};
pub use protocol::{
    ATTACH, ATTEND, CHANNEL_VERSION, Connection, EXTENSION_FETCH, EXTENSION_FILE, EXTENSION_FORGET,
    EXTENSION_INSTALL, EXTENSION_LIST, EXTENSION_OCCASION, EXTENSION_REPOINT, MAX_FRAME_BYTES,
    METHODS, PROJECT_RESOURCE, PROJECTS, REGISTRY_CACHED, REGISTRY_INDEX, REGISTRY_LEDGER,
    REMOTE_DEVICES, REMOTE_GREETING, REMOTE_HELLO, REMOTE_IDLE, REVISION_RESOURCE, SCHEDULE_OFF,
    SCHEDULE_REMEMBER, SCHEDULE_SWITCH, TOOL_CALL, Transport, carried, tool_result,
};
