//! The single owner of every conversation with the memory engine.
//!
//! Nothing else in Sync opens `refs/memory/*`, touches `LanceDB`, or loads an
//! embedding model: those live behind the sidecar, and the sidecar lives behind
//! this type. What the rest of the application sees is typed methods and typed
//! failures.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::dto::Handshake;
use crate::error::{MemoryError, Result};
use crate::operations::{Operations, parse};
use crate::process::{BinarySource, Channel, EngineBinary, LaunchConfig, Resident, Sidecar};
use crate::protocol::Connection;

/// What this window will not start without.
///
/// Not the whole surface — only the operations the window reaches on the paths
/// a person takes first. A sidecar missing one of these cannot open a project,
/// so saying so at the handshake beats failing at the third screen.
const REQUIRED_METHODS: &[&str] = &[
    "project.describe",
    "project.revision",
    "types.list",
    "records.load",
    "records.apply",
    "documents.get",
    "project.settings",
    "project.update",
    // On the first path of all: the flow that opens a project asks this before
    // it offers to describe one. A sidecar that cannot answer would skip the
    // check in silence and let a fresh clone be described a second time, which
    // is the one outcome the check exists to prevent.
    "engine.presence",
];

/// What Sync knows about the sidecar serving this project.
#[derive(Clone, Debug)]
pub struct EngineInfo {
    pub binary: PathBuf,
    pub source: BinarySource,
    pub version: String,
    pub handshake: Handshake,
}

/// A live session against one project's memory.
pub struct MemoryClient {
    config: LaunchConfig,
    binary: EngineBinary,
    connection: Connection<Channel>,
    info: EngineInfo,
    /// The last revision this client observed, and what `expected_revision`
    /// carries on the next write.
    revision: String,
    /// Set when the engine said the revision changed. The UI drains this and
    /// re-reads; the notification payload itself is never trusted as the value.
    revision_dirty: bool,
    /// How many transaction ids this session has handed out. Part of what makes
    /// each one name a single attempt — see [`Self::next_transaction_id`].
    transactions: u64,
}

impl MemoryClient {
    /// Start a session: resolve the binary, launch it, greet it.
    ///
    /// Greeting costs one round trip and a resource read, so this is done
    /// lazily on first use rather than pre-warmed.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Sidecar`] when the sidecar cannot be found,
    /// started, or does not answer what this window calls, and whatever the
    /// engine refused when the project's memory cannot be read.
    pub fn connect(config: LaunchConfig) -> Result<Self> {
        // Resolved either way. The version and where the binary came from are
        // what the settings window reports about this installation's engine,
        // and they are as true of the process serving the machine as of one
        // this client started — the resident process was resolved the same way.
        let binary = crate::process::resolve_binary(&config)?;
        let connection = Connection::new(Self::channel(&binary, &config)?);
        let info = EngineInfo {
            binary: binary.path.clone(),
            source: binary.source,
            version: binary.version.clone(),
            handshake: Handshake::default(),
        };
        let mut client = Self {
            config,
            binary,
            connection,
            info,
            revision: String::new(),
            revision_dirty: false,
            transactions: 0,
        };
        client.greet()?;
        Ok(client)
    }

    /// Reach the engine, however this installation is arranged.
    ///
    /// One place, because [`Self::connect`] and [`Self::restart`] must make the
    /// same choice: a reconnect that fell back to a private process would put a
    /// machine back to one engine per project, one restart at a time, with
    /// nothing saying so.
    fn channel(binary: &EngineBinary, config: &LaunchConfig) -> Result<Channel> {
        match config.host_socket.as_ref() {
            Some(path) => Resident::connect(path).map(Channel::Resident),
            None => Sidecar::spawn(&binary.path, config).map(Channel::Own),
        }
    }

    fn greet(&mut self) -> Result<()> {
        // The resident process serves every project on the machine, so a
        // connection to it says which one it is about before it asks anything.
        // Here rather than beside the connect, because a reconnect has to say it
        // again: the process that answers afterwards has never heard of this
        // client.
        if self.config.host_socket.is_some() {
            let project = self.config.project.clone();
            self.request_once(crate::protocol::ATTACH, &json!({"path": project}))?;
        }
        // `request_once`, never `request`: this runs inside `restart`, and a
        // handshake that restarted on failure would restart to greet, greet to
        // fail, and fail to restart — a loop with a process spawned at every
        // turn. A replacement that cannot answer is an error, not another try.
        // The number goes out with the question rather than beside it: a
        // handshake is the one place both ends are certain to speak, and a
        // version agreed anywhere else is a version one of them can skip.
        let answered = self.request_once(
            crate::protocol::METHODS,
            &json!({"channel": crate::protocol::CHANNEL_VERSION}),
        )?;
        let answered = channel_methods(&answered)?;
        let missing: Vec<&str> = REQUIRED_METHODS
            .iter()
            .copied()
            .filter(|method| !answered.iter().any(|name| name == method))
            .collect();
        if !missing.is_empty() {
            return Err(MemoryError::Sidecar(format!(
                "the sidecar does not answer {} — it is older than this window",
                missing.join(", ")
            )));
        }
        // The revision before the handshake, and the order is load-bearing.
        // Reading the revision is what gives a project with no memory one, and
        // a handshake taken from a project with nothing declared names no
        // backend — so every question about what this store can do would be
        // answered from before it existed.
        self.revision = self.read_current_revision()?;
        self.info.handshake = parse(self.request_once("project.describe", &json!({}))?)?;
        Ok(())
    }

    #[must_use]
    pub const fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// The revision this client last observed.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Whether the engine reported a revision change that has not been re-read.
    ///
    /// The notification is a hint, not a value: callers invalidate their caches
    /// and re-read, they do not take a revision from a notification payload.
    #[must_use]
    pub fn revision_is_stale(&mut self) -> bool {
        self.drain_updates();
        self.revision_dirty
    }

    /// The engine's process id, when this client is the one that started it.
    ///
    /// `None` against the resident process, and the `Option` is the point: the
    /// one caller for this is a test simulating a crash, and handing it the pid
    /// of the process serving the whole machine would have it kill the memory of
    /// every other project running beside it.
    pub fn engine_pid(&mut self) -> Option<u32> {
        self.connection.transport_mut().pid()
    }

    /// Whether the engine process is still running.
    ///
    /// A UI showing a transient "reconnecting" state asks this; a blocked read
    /// cannot tell a dead engine from a busy one.
    pub fn engine_is_alive(&mut self) -> bool {
        self.connection.transport_mut().is_alive().unwrap_or(false)
    }

    /// Re-read the current revision and clear the stale flag.
    ///
    /// The sidecar reads it from the engine rather than answering from what it
    /// last wrote, so this sees a revision moved by anything else on the
    /// machine — a `git pull`, a second window, the engine's own CLI.
    ///
    /// # Errors
    ///
    /// Returns the engine or transport failure.
    pub fn refresh_revision(&mut self) -> Result<String> {
        let revision = self.read_revision()?;
        self.revision.clone_from(&revision);
        self.revision_dirty = false;
        Ok(revision)
    }

    /// A transaction id no other attempt will reuse.
    ///
    /// The engine refuses a reused id, which is what makes a retry after a lost
    /// response safe rather than a silent double write — so an id has to name
    /// *this attempt*, not the operation. The revision it was built against and
    /// a per-session counter are enough for that: a repeat of the same attempt
    /// is the one case where reusing an id is correct, and it is the case the
    /// engine's own replay handles.
    pub fn next_transaction_id(&mut self, prefix: &str) -> String {
        self.transactions += 1;
        format!("{prefix}-{}-{}", self.revision, self.transactions)
    }

    // ── Plumbing ────────────────────────────────────────────────────────────

    fn request_once(&mut self, method: &str, params: &Value) -> Result<Value> {
        let answer = self.connection.request(method, params)?;
        self.drain_updates();
        // The daemon owns the revision now, and says it in every answer that
        // has one. Taking it here keeps the window current without a read after
        // every write — and without the window guessing what its own write did.
        if let Some(revision) = answer.get("revision").and_then(Value::as_str) {
            revision.clone_into(&mut self.revision);
        }
        Ok(answer)
    }

    /// Replace a dead sidecar with a live one, restoring the session state the
    /// engine does not persist: the handshake and the revision subscription.
    fn restart(&mut self, reason: &str) -> Result<()> {
        // The exit status is worth carrying into the message: "exited with
        // status 101" is a different conversation from "the stream ended".
        let exit = self.connection.transport_mut().exit_status();
        let detail = match exit {
            Some(status) => format!("{reason}; the engine exited with {status}"),
            None => reason.to_owned(),
        };
        let channel = Self::channel(&self.binary, &self.config).map_err(|error| {
            MemoryError::Sidecar(format!("the memory engine stopped ({detail}) and {error}"))
        })?;
        self.connection = Connection::new(channel);
        // The replacement is greeted the same way the first one was: a restart
        // that skipped the check would be a window trusting a process it never
        // asked anything of. Greeting also re-reads the revision, which may
        // have moved while we were gone.
        self.greet()?;
        self.revision_dirty = false;
        Ok(())
    }

    /// The project's revision, read from the engine.
    ///
    /// `request_once`, never `request`: this runs inside [`Self::greet`], which
    /// runs inside [`Self::restart`]. Going through the restarting form would
    /// let a replacement that starts and then dies restart to read, read to
    /// fail, and fail to restart — recursion with a process spawned at every
    /// turn. A replacement that cannot answer is an error, not another try.
    fn read_current_revision(&mut self) -> Result<String> {
        parse(self.request_once("project.revision", &json!({}))?)
    }

    /// Notice anything the sidecar said between answers.
    ///
    /// Nothing yet: the host channel answers, it does not announce. When it
    /// grows notifications — the daemon already knows which calls moved the
    /// revision — this is where the window learns of them, and
    /// `revision_is_stale` starts telling the truth again.
    fn drain_updates(&mut self) {
        let _ = self.connection.take_updates();
    }
}

/// The channel's operations, carried to the sidecar this window started.
///
/// The whole of what this client adds to them is here: a sidecar that died
/// between two calls is a transient condition rather than something to ask a
/// person about, so it is restarted, greeted and the call replayed. Only a
/// second failure is reported.
///
/// What it sends is not a tool call: the window's channel is `sync-mcp`'s own
/// surface, and an operation there has no description, no schema and no
/// presence in anything an agent can list.
impl Operations for MemoryClient {
    fn request(&mut self, method: &str, params: &Value) -> Result<Value> {
        match self.request_once(method, params) {
            Err(MemoryError::Sidecar(reason)) => {
                self.restart(&reason)?;
                self.request_once(method, params)
            }
            other => other,
        }
    }
}

/// Resource bodies arrive as JSON text inside `contents[0].text`.
/// What the handshake says this window is talking to.
///
/// Three answers and each is a different sentence. A handshake with no version
/// on it is an engine from before the channel had one, which is the shape a
/// stale bundle takes. A version that is not ours names which of the two is
/// behind — the sentence a person can act on, rather than a call failing later
/// on a shape that moved. Anything else is the list of what it answers.
fn channel_methods(answer: &Value) -> Result<Vec<String>> {
    let Some(stated) = answer.get("channel").and_then(Value::as_u64) else {
        return Err(MemoryError::Sidecar(
            "the sidecar's handshake states no channel version — it is older than this window"
                .to_owned(),
        ));
    };
    let ours = u64::from(crate::protocol::CHANNEL_VERSION);
    if stated != ours {
        let older = if stated < ours { "sidecar" } else { "window" };
        return Err(MemoryError::Sidecar(format!(
            "this window speaks channel version {ours} and the sidecar speaks {stated} — the {older} is the older of the two"
        )));
    }
    parse(answer.get("methods").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// The handshake as it is answered today.
    #[test]
    fn a_handshake_at_this_version_is_the_list_it_carries() {
        let named = channel_methods(&json!({
            "channel": crate::protocol::CHANNEL_VERSION,
            "methods": ["types.list", "records.load"],
        }))
        .expect("the versions agree");
        assert_eq!(named, vec!["types.list", "records.load"]);
    }

    /// An engine from before the channel had a number. The bare list is what it
    /// answered, and the message has to name what is old rather than report an
    /// unreadable response.
    #[test]
    fn a_handshake_with_no_version_says_the_sidecar_is_the_older_one() {
        let refused = channel_methods(&json!(["types.list"])).expect_err("no version was stated");
        assert!(
            refused.to_string().contains("older than this window"),
            "the message names which side is behind: {refused}"
        );
    }

    /// Both directions of the mismatch, because both happen: a window updated
    /// past its bundled engine, and a resident process from a newer install.
    #[test]
    fn a_version_that_is_not_ours_names_the_older_side() {
        let ours = u64::from(crate::protocol::CHANNEL_VERSION);
        let behind = channel_methods(&json!({"channel": ours + 1, "methods": []}))
            .expect_err("the sidecar speaks a later version");
        assert!(
            behind.to_string().contains("the window is the older"),
            "{behind}"
        );

        let ahead = channel_methods(&json!({"channel": 0, "methods": []}))
            .expect_err("the sidecar speaks an earlier version");
        assert!(
            ahead.to_string().contains("the sidecar is the older"),
            "{ahead}"
        );
    }
}
