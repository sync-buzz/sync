//! What the command tests need before they can reach a real sidecar.
//!
//! Shared by both command suites because both have the same problem: the
//! commands resolve their sidecar the way the application does, and the way
//! the application does is "beside the running executable" — which, for a test
//! executable, is `target/debug/deps`, where nothing stages one.

use std::path::PathBuf;
use std::sync::Once;

/// The variable `memory.rs` reads to find a sidecar built from source.
///
/// It names a **`sync-mcp`**, with the engine already linked inside it. A
/// `memory-hub` binary is the engine alone and does not speak this window's
/// channel, which is what this used to be pointed at.
const BINARY_OVERRIDE: &str = "SYNC_MCP_BINARY";

/// Point the commands at a sidecar, and say whether there is one to point at.
///
/// An override already in the environment is left alone — CI sets one, and so
/// does anybody testing a particular build. Otherwise the sidecar this
/// workspace built is found and named, so a developer who has built one does
/// not get a suite that passes by doing nothing.
///
/// The write is behind a [`Once`] that every reader passes through first.
/// Setting an environment variable while another thread reads the environment
/// is undefined, and there is no such thread here: no test reaches the commands
/// without calling this and waiting for it to finish.
pub fn sidecar_is_available() -> bool {
    static STAGED: Once = Once::new();
    STAGED.call_once(|| {
        if std::env::var_os(BINARY_OVERRIDE).is_some() {
            return;
        }
        if let Some(path) = built_sidecar() {
            // SAFETY: the only readers are the commands these tests invoke, and
            // every one of them is reached through this function, which blocks
            // until the write has happened.
            unsafe { std::env::set_var(BINARY_OVERRIDE, path) };
        }
    });
    std::env::var_os(BINARY_OVERRIDE).is_some_and(|path| PathBuf::from(path).is_file())
}

/// Why the suite was skipped, in the words that say what to do about it.
#[allow(
    dead_code,
    reason = "each test binary uses a different part of this module"
)]
pub const NO_SIDECAR: &str = "skipping: no sync-mcp binary — build one with `cargo build -p sync-mcp`, or set SYNC_MCP_BINARY";

/// The sidecar this workspace built, wherever it put it.
///
/// **The newest of them, not the first found.** These tests do not build
/// anything — building this one links the engine and takes minutes, which is
/// not a cost a test run may impose — so what they can do is refuse to prefer a
/// binary that is older than one somebody has just made. Ordering by location
/// was what this did before, and it made a stale release build shadow a debug
/// build from a minute ago: the suite then tested an engine that had never
/// heard of the field being added, and said so as a mismatched value rather
/// than as a stale binary.
///
/// A copy in `binaries/` is what a bundle would ship, and it wins ties for that
/// reason: it is staged deliberately, and the two builds under `target/` are
/// whatever the last command happened to leave.
fn built_sidecar() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let staged = std::fs::read_dir(root.join("binaries"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sync-mcp-"))
        });
    staged
        .into_iter()
        .chain([
            root.join("target/release/sync-mcp"),
            root.join("target/debug/sync-mcp"),
        ])
        .filter(|path| path.is_file())
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|about| about.modified())
                .ok()
        })
}
