//! Extension packages: what one is, how it is checked, and where it lives.
//!
//! Tauri-free on purpose, like `sync-memory` beside it. Nothing here knows
//! about a window, a webview or a command — it takes paths and answers with
//! data, so it is testable from plain `cargo test` and usable from anything
//! that later needs to read what a machine has installed, including a CLI.
//!
//! What is deliberately *not* here: whether an extension is compatible with
//! this build. That is decided against `SYNC_API_VERSION`, which is declared on
//! the surface it describes — `src/lib/extension-api/version.ts` — and a second
//! copy of that number in Rust would be a second answer to a question with one.
//! This crate hands the manifest over; the host decides.
//!
//! The four pieces, in the order a package meets them:
//!
//! - [`manifest`] — what a package says about itself, and what is refused.
//! - [`archive`] — the `.syncext` file, its hashes, and its signature.
//! - [`store`] — the artefact directory, and the pointer that says what serves
//!   an id right now.
//! - [`vocabulary`] — the types it publishes and what it tells an agent, read
//!   out of the artefact rather than out of a constant in the window.
//!
//! And beside them the two that dial out, both here rather than in the webview
//! so that reaching anywhere is a property of something written down rather
//! than of a page: the window's `connect-src` is not widened by a byte for
//! either of them.
//!
//! - [`registry`] — what exists anywhere. Its hosts are compiled in, because
//!   where this application looks for extensions is a fact about the build.
//! - [`net`] — what one installed package may read. Its hosts are the
//!   package's own, out of its manifest and onto the card somebody installed it
//!   from, because what an extension reaches is a fact about that extension.

pub mod archive;
pub mod manifest;
pub mod net;
pub mod registry;
pub mod store;
pub mod vocabulary;

pub use archive::{Archive, ArchiveError, SignatureState, digest_of};
pub use manifest::{
    AGENT_TOOLS_CAPABILITY, Manifest, ManifestError, NET_CAPABILITY, NET_WRITE_CAPABILITY, Net,
    SUPPORTED_MANIFEST_VERSION, Secret, Tool,
};
pub use net::{
    Method as NetMethod, NetError, Part as NetPart, Request as NetRequest, Response as NetResponse,
};
pub use registry::{Artefact, Fetched, Index, Ledger, Listed, Registry, RegistryError, Release};
pub use store::{Installed, Pointer, Source, Store, StoreError};
pub use vocabulary::{TypeDefinition, VocabularyError, read_prompt, read_types};
