//! The projects one server answers for, and the name each answers to.
//!
//! A server used to be a project: the process was started over one directory
//! and every call meant that one. It is a machine now, and the difference shows
//! up in every tool's schema — [`PROJECT_ARGUMENT`] is required everywhere,
//! because the alternative is a default, and a default is how a call meant for
//! one project quietly answers from another.
//!
//! Each project holds its own lock. One lock over all of them would make two
//! agents working on two projects wait for each other for no reason other than
//! that somebody put them in the same process.
//!
//! **The list is not settled when the process starts.** A machine gains a
//! project whenever somebody opens one in the window, and this server outlives
//! the window that spawned it — so a list read once at start is a snapshot of
//! whenever the server happened to be launched, and the answer to "which
//! projects are there" would be wrong for every project opened since. The
//! registry file is re-read when it has changed, and a project already open
//! stays open across the re-read: its memory holds a repository and a loaded
//! model, and reopening it to answer questions it was already answering would
//! charge somebody else's new project to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};
use std::time::SystemTime;

use memory_hub_mcp::EmbeddingProvider;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::Domain;

/// The argument every tool takes first: which project the call is about.
pub const PROJECT_ARGUMENT: &str = "project";

/// A project this machine answers for, as the window registered it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registered {
    /// The repository root.
    pub path: PathBuf,
    /// What the window calls it.
    pub name: String,
    /// What agents call it here.
    pub identifier: String,
}

/// What a file looked like when it was last read.
///
/// The length as well as the time, because a registry gaining a project and
/// losing another within one clock tick would otherwise read as no change at
/// all. Two cheap fields from one `stat`, and neither is a hash of a file this
/// server would then be reading on every call to answer a question about.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    length: u64,
}

impl Stamp {
    /// What `path` looks like now, or nothing when it cannot be looked at.
    fn of(path: &Path) -> Option<Self> {
        let found = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: found.modified().ok(),
            length: found.len(),
        })
    }
}

/// The file a list of projects is read from, and when it was read.
struct Source {
    path: PathBuf,
    /// Held so a project registered later opens with the same model as the
    /// ones that were there at the start, rather than resolving a second copy.
    embeddings: Option<Arc<dyn EmbeddingProvider>>,
    read: Mutex<Option<Stamp>>,
}

/// What this process holds open, and what the registry calls it.
///
/// Two maps, and the split is the point: **a name belongs to the registry
/// entry, not to the open memory.** A project renamed in the window is the same
/// memory under a new name, and a project the host door opened has a memory and
/// no name at all until the machine registers it.
///
/// [`Self::open`] is keyed by path and is the invariant everything else rests
/// on: a repository has one memory, so it gets one [`Domain`] in this process
/// no matter which door asked for it. Two sessions over one repository would
/// mean two indexes, two views of the revision, and writes racing inside a
/// single process.
#[derive(Default)]
struct Held {
    /// What the registry calls each path it names — and therefore exactly what
    /// an agent may reach.
    naming: BTreeMap<PathBuf, Registered>,
    /// Every memory this process has open, by path.
    ///
    /// A superset of [`Self::naming`]'s keys. The host door may be handed a
    /// path the registry has never heard of, because the window reads a
    /// project's own record to find the identifier it will register by — so a
    /// door that served only registered projects could not open one.
    open: BTreeMap<PathBuf, Arc<Project>>,
}

/// Every project a server answers for.
pub struct Projects {
    /// Behind a lock because the list outlives the read that produced it: see
    /// the module's note on why a server started on Saturday must not still be
    /// answering Saturday's question on Monday.
    entries: RwLock<Held>,
    /// Where to look again. `None` for a list that came from nowhere a file
    /// can be re-read from — the single-project door, and the empty one.
    source: Option<Source>,
    /// Handed to every [`Project`] this holds, so that one being written to is
    /// said in one place however the writer got in.
    moves: Arc<Moves>,
}

/// Where this process says that a project's memory has moved.
///
/// Set after the projects are opened rather than taken as an argument to every
/// constructor, for the reason [`crate::application::Application::attend`] is
/// the same shape: the channel this is written on does not exist yet when the
/// projects are opened, and a `sync-mcp` somebody started in a terminal never
/// gains one at all. Shared by every [`Project`] rather than copied into each,
/// so a project opened before the channel arrived is heard from after it does
/// — the alternative is the projects open at start-up being the only ones
/// nobody ever hears about.
#[derive(Default)]
pub struct Moves {
    said: Mutex<Option<Told>>,
}

/// What is done about a project's memory having moved.
///
/// A closure rather than a named channel, so that this file states *that* a
/// move is reported and stays ignorant of how — which is what keeps the
/// vocabulary of the wire out of the type that holds every project open.
type Told = Box<dyn Fn(&Path) + Send + Sync>;

impl Moves {
    /// Say that the memory of the project at `path` is not what it was.
    ///
    /// Silent where nobody is listening, and silent through a poisoned lock:
    /// what a caller would do with either is nothing, and the call that has
    /// just succeeded is not made wrong by a notice that could not go out.
    fn say(&self, path: &Path) {
        if let Ok(said) = self.said.lock()
            && let Some(said) = said.as_ref()
        {
            said(path);
        }
    }
}

impl Projects {
    /// Answer for `registered`, with one vector model between them all.
    ///
    /// The provider is resolved by the caller and cloned into each project.
    /// Resolved per project it would be *loaded* per project, and the model is
    /// the largest thing this process holds — a machine with eight projects
    /// would pay for eight copies of one file.
    ///
    /// This list never changes. [`Projects::registered`] is the one that does.
    #[must_use]
    pub fn over(
        registered: Vec<Registered>,
        embeddings: Option<&Arc<dyn EmbeddingProvider>>,
    ) -> Self {
        let moves = Arc::<Moves>::default();
        Self {
            entries: RwLock::new(opened(registered, embeddings, &moves)),
            source: None,
            moves,
        }
    }

    /// Say where a project's memory moving is to be reported.
    ///
    /// Called once, after the channel that carries it exists — which is why
    /// this is a method rather than an argument. Nothing is reported before it,
    /// and nothing is reported at all by a process nobody attended.
    pub fn report_moves_to(&self, said: impl Fn(&Path) + Send + Sync + 'static) {
        if let Ok(mut held) = self.moves.said.lock() {
            *held = Some(Box::new(said));
        }
    }

    /// Answer for the projects a registry file names, now and as it gains them.
    ///
    /// The first read is the caller's to fail on: a server told to serve a
    /// registry it cannot read has been misconfigured, and starting anyway
    /// would answer "this machine has no projects" to everyone who asked. Every
    /// later read is silent by the same argument turned around — the file
    /// becoming unreadable while the server runs is not a reason to forget the
    /// projects it is already serving.
    ///
    /// # Errors
    ///
    /// When the registry cannot be read, or is not a list of projects.
    pub fn registered(
        path: PathBuf,
        embeddings: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Stamped before the read, not after: a write landing between the two
        // leaves a stamp older than the file, which costs one re-read. The
        // other order would record a file that was never read.
        let stamp = Stamp::of(&path);
        let text = std::fs::read_to_string(&path)?;
        let listed: Vec<Registered> = serde_json::from_str(&text)?;
        let moves = Arc::<Moves>::default();
        Ok(Self {
            entries: RwLock::new(opened(listed, embeddings.as_ref(), &moves)),
            source: Some(Source {
                path,
                embeddings,
                read: Mutex::new(stamp),
            }),
            moves,
        })
    }

    /// Where the window keeps this installation's settings, if this process
    /// serves a registry at all.
    ///
    /// Derived from the registry's own path rather than taken as a second
    /// argument, and the derivation is sound because there is one such
    /// directory: `registered-projects.json` is written by the window into its
    /// configuration directory, and so is everything else the window decides —
    /// `mcp-server.json`, `voice.json`. A process serving one project from the
    /// command line has no registry and therefore no answer here, which is
    /// correct: nobody set a preference for it.
    #[must_use]
    pub fn configuration(&self) -> Option<&Path> {
        self.source.as_ref().and_then(|source| source.path.parent())
    }

    /// Read the registry again, if it has changed since it was last read.
    ///
    /// Called before every answer about which projects there are, because there
    /// is no other moment: nothing tells this process that somebody opened a
    /// project in the window. A `stat` per call is what that costs, against a
    /// call that is about to reach Git and a vector store.
    ///
    /// Silent on failure, and deliberately: what a caller would do with "the
    /// registry moved" is nothing, and the list already held is still the best
    /// answer available. The stamp is left alone in that case, so the next call
    /// tries again rather than trusting a read that did not happen.
    fn refresh(&self) {
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let Some(found) = Stamp::of(&source.path) else {
            return;
        };
        let mut read = source.read.lock().unwrap_or_else(PoisonError::into_inner);
        if *read == Some(found) {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&source.path) else {
            return;
        };
        let Ok(listed) = serde_json::from_str::<Vec<Registered>>(&text) else {
            return;
        };
        *read = Some(found);
        self.adopt(listed, source.embeddings.as_ref());
    }

    /// Make the held list the one `listed` names, keeping what is already open.
    ///
    /// A project whose entry is unchanged keeps the [`Project`] it had, so its
    /// memory is not reopened and whatever it has loaded stays loaded. One
    /// whose path moved is a different project under the same name and is
    /// opened afresh; one no longer named is dropped, because a server that
    /// went on answering for a project the machine has forgotten would be the
    /// same staleness in the other direction.
    fn adopt(&self, listed: Vec<Registered>, embeddings: Option<&Arc<dyn EmbeddingProvider>>) {
        let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
        let was = std::mem::take(&mut entries.naming);
        for project in listed {
            // Opened already, on whichever account: the memory is kept and only
            // the name is written. A project the host door opened a moment ago
            // is *the same repository* the registry has just named, and
            // reopening it would throw away whatever it had loaded and leave
            // two sessions over one repository while both handles lived.
            entries.open.entry(project.path.clone()).or_insert_with(|| {
                Arc::new(Project::over(project.path.clone(), embeddings, &self.moves))
            });
            entries.naming.insert(project.path.clone(), project);
        }
        // A project the machine has forgotten stops being answered for, and its
        // memory is let go with its name — a server still holding a repository
        // nobody registered would be the same staleness in the other direction.
        // What the *host* door opened is not touched: it was never named here,
        // so it was never forgotten either.
        for path in was.into_keys() {
            if !entries.naming.contains_key(&path) {
                entries.open.remove(&path);
            }
        }
    }

    /// The held list, whether or not a previous call panicked over it.
    ///
    /// Unlike a project's memory, this is a map of handles: a panic while it
    /// was locked cannot have left it half-written into a state nobody can
    /// describe, so recovering is honest rather than hopeful.
    fn entries(&self) -> RwLockReadGuard<'_, Held> {
        self.entries.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Answer for the one project a session was pointed at.
    ///
    /// The stdio door, where a process is still started over a directory. The
    /// key is read from the project's own record rather than derived from the
    /// folder: it is the same key everyone else who opened that repository
    /// uses, and a second way of arriving at it would be a second answer.
    ///
    /// A directory whose memory names no project answers for nothing. There is
    /// no key to call it by, and inventing one here would mint a name that
    /// disagrees with the one the project gets the moment somebody describes
    /// it in the window.
    #[must_use]
    pub fn just(project: PathBuf, embeddings: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        let mut domain = Domain::open(project.clone(), embeddings);
        // The revision first, because reading the corpus without it is reading
        // a session that has not met the project yet. `ensure_revision` rather
        // than `ensure_initialised`: an agent connecting to a repository has
        // decided nothing about whether it keeps memory, so this reads and does
        // not create.
        let moves = Arc::<Moves>::default();
        let Ok(Some(settings)) = domain
            .ensure_revision()
            .and_then(|()| domain.project_settings())
        else {
            return Self {
                entries: RwLock::new(Held::default()),
                source: None,
                moves,
            };
        };
        // The memory is handed over as it stands rather than reopened: it has
        // already read the revision and the settings, which is the work this
        // door exists to have done before the first call.
        Self {
            entries: RwLock::new(Held {
                naming: BTreeMap::from([(
                    project.clone(),
                    Registered {
                        path: project.clone(),
                        name: settings.name,
                        identifier: settings.identifier,
                    },
                )]),
                open: BTreeMap::from([(
                    project.clone(),
                    Arc::new(Project {
                        path: project,
                        domain: Mutex::new(domain),
                        moves: Arc::clone(&moves),
                    }),
                )]),
            }),
            source: None,
            moves,
        }
    }

    /// The project answering to `key`, if one does.
    ///
    /// Case does not matter. An identifier is upper-case by construction, so
    /// `sync` and `SYNC` are the same name written two ways rather than two
    /// names — refusing the second would be pedantry, not strictness.
    #[must_use]
    pub fn holding(&self, key: &str) -> Option<Arc<Project>> {
        self.refresh();
        // Through the naming, and only through it. A project this process
        // opened because the host door was handed its path has no name here,
        // and an agent reaching it would be reaching past the registry that
        // decides what an agent may reach.
        let entries = self.entries();
        let path = entries
            .naming
            .values()
            .find(|named| named.identifier.eq_ignore_ascii_case(key))
            .map(|named| &named.path)?;
        entries.open.get(path).map(Arc::clone)
    }

    /// The project rooted at `path`, opening its memory if this process has not
    /// been asked about it before.
    ///
    /// The host door, where the caller is Sync itself and names a path rather
    /// than an identifier. It does not consult the registry, and that is the
    /// point: the opening flow reads a project's record to *find* the
    /// identifier it will register by, so a door that served only registered
    /// projects could not open one.
    ///
    /// Answers with whatever is already open under that path — registered or
    /// adopted — so one repository has one memory in this process however it
    /// was reached.
    #[must_use]
    pub fn at(&self, path: &Path, embeddings: Option<&Arc<dyn EmbeddingProvider>>) -> Arc<Project> {
        self.refresh();
        if let Some(open) = self.entries().open.get(path).map(Arc::clone) {
            return open;
        }
        let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
        // Looked for again under the write lock. Two calls about the same
        // unknown path can both miss the read above, and the loser of that race
        // must get the winner's handle rather than open a second memory over
        // the same repository.
        Arc::clone(
            entries.open.entry(path.to_owned()).or_insert_with(|| {
                Arc::new(Project::over(path.to_owned(), embeddings, &self.moves))
            }),
        )
    }

    /// The key a registered project is called by, given where it is.
    ///
    /// The reverse of [`Self::holding`], and it exists for one caller: the
    /// answer to a device. A device names a project by key and is told where
    /// nothing is, so an answer carrying a directory would be handing back the
    /// one thing the call it answers was not allowed to say. `None` for a path
    /// this machine has not registered — a working tree somebody made, most
    /// often — which the caller reads as *not a project*, never as *the project
    /// with no name*.
    #[must_use]
    pub fn key_at(&self, path: &Path) -> Option<String> {
        self.refresh();
        self.entries()
            .naming
            .values()
            .find(|named| named.path == path)
            .map(|named| named.identifier.clone())
    }

    /// The keys, in the order they are listed.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.refresh();
        let mut keys: Vec<String> = self
            .entries()
            .naming
            .values()
            .map(|named| named.identifier.clone())
            .collect();
        // **By name, and said here rather than left to the map.** The map is
        // keyed by path now, and a path is a temporary directory as often as it
        // is a home directory — so an order taken from it is an order that
        // changes for no reason a reader could see. This list is read by a
        // person and by an agent choosing between projects, and both are owed
        // the same order every time.
        keys.sort_by_key(|key| key.to_uppercase());
        keys
    }

    /// Every project, as the door lists them.
    #[must_use]
    pub fn listed(&self) -> Value {
        self.refresh();
        // Ordered by name, for the reason [`Self::keys`] gives.
        let mut named: Vec<Registered> = self.entries().naming.values().cloned().collect();
        named.sort_by_key(|held| held.identifier.to_uppercase());
        let listed: Vec<Value> = named
            .iter()
            .map(|held| {
                json!({
                    "project": held.identifier,
                    "name": held.name,
                    "path": held.path,
                })
            })
            .collect();
        json!({
            "projects": listed,
        })
    }
}

/// Whether a call moved the memory, given where it stood either side of it.
///
/// A function rather than the expression written inline, because the asymmetry
/// in it is the whole rule and it is worth being able to state the three cases
/// separately. `None` before is a project nobody had read yet: the read that
/// gives it a revision is somebody opening it, not something happening to it,
/// and counted as a move it would tell every window watching that repository to
/// read it again every time another one was opened.
fn moved(before: Option<&str>, after: Option<&str>) -> bool {
    before.is_some_and(|before| after != Some(before))
}

/// Open every project in `listed`, keyed the way [`Projects`] keys them.
fn opened(
    listed: Vec<Registered>,
    embeddings: Option<&Arc<dyn EmbeddingProvider>>,
    moves: &Arc<Moves>,
) -> Held {
    let mut held = Held::default();
    for project in listed {
        held.open.insert(
            project.path.clone(),
            Arc::new(Project::over(project.path.clone(), embeddings, moves)),
        );
        held.naming.insert(project.path.clone(), project);
    }
    held
}

/// One repository's memory, open in this process.
///
/// It carries no name. What a project is called is the registry's — see
/// [`Held`] — and a memory that also held a name would be a second place for
/// one to be wrong: a project renamed in the window is the same memory, and a
/// project the host door opened has no name at all until the machine registers
/// it.
pub struct Project {
    path: PathBuf,
    /// Behind a mutex because running a tool takes `&mut`, and one project's
    /// memory serialises its own calls anyway — pretending otherwise here would
    /// only move the contention.
    domain: Mutex<Domain>,
    /// Where a call that moved this memory is reported. Shared with every other
    /// project in this process — see [`Moves`].
    moves: Arc<Moves>,
}

impl Project {
    /// Open the memory of the repository rooted at `path`.
    ///
    /// One constructor for both doors, because there is nothing different to
    /// do: the provider is the one this process already resolved, so a project
    /// reached either way costs no second copy of the model.
    fn over(
        path: PathBuf,
        embeddings: Option<&Arc<dyn EmbeddingProvider>>,
        moves: &Arc<Moves>,
    ) -> Self {
        Self {
            path: path.clone(),
            domain: Mutex::new(Domain::open(path, embeddings.cloned())),
            moves: Arc::clone(moves),
        }
    }

    /// Where this project is on disk.
    ///
    /// Read by the one caller that has to name it to somebody else: a tool call
    /// travels to the application, which knows projects by their path rather
    /// than by the key an agent used.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Do one thing with this project's memory, off the async runtime's
    /// threads.
    ///
    /// Every call is blocking — it reaches Git, `LanceDB` and possibly a
    /// model — so it belongs on a blocking thread. What the async runtime buys
    /// is that the transport keeps reading while a call runs: a search loading
    /// a model for the first time no longer means the connection is deaf, and
    /// on this server it also means a slow call in one project does not stop
    /// the others.
    ///
    /// # Errors
    ///
    /// A poisoned lock means a previous call panicked while holding this
    /// project's memory. Its state is no longer known, and answering from it
    /// would be answering from wreckage.
    pub async fn with_domain<T, F>(self: &Arc<Self>, work: F) -> Result<T, McpError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Domain) -> T + Send + 'static,
    {
        let project = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            project
                .holding(work)
                .map_err(|reason| McpError::internal_error(reason, None))
        })
        .await
        .map_err(|error| {
            McpError::internal_error(format!("memory call could not be run: {error}"), None)
        })?
    }

    /// Hold this project's memory for one call, and say so if the call moved
    /// it.
    ///
    /// **Every door goes through here**, and that is the whole of why it
    /// exists. The window's operations, an agent's tools and a device's calls
    /// are three ways into one [`Domain`], and a door deciding for itself
    /// whether the memory had moved would be a second answer to that question,
    /// with a third arriving the day somebody adds a door. Put here it cannot
    /// be forgotten: a door that reaches a project's memory at all reaches it
    /// through this.
    ///
    /// The revision is compared either side of the call **under the one lock
    /// that answered it**, so nothing can land between the write and the
    /// reading of it. Asking the engine instead would be a round trip after
    /// every read as well as a different question — what the store holds, not
    /// what this call did.
    ///
    /// A first read is not a move. [`Domain::seen_revision`] answers `None`
    /// until a project has been read at all, and opening one is not something
    /// happening to it: counted as a move, the first call of every session
    /// would tell every window watching that repository to read it again.
    ///
    /// The notice goes out after the lock is let go, which is not tidiness —
    /// whoever is told may reach straight back into this project, and a notice
    /// sent under the lock would be that caller waiting on the call it was told
    /// about.
    ///
    /// # Errors
    ///
    /// The sentence to tell whoever asked, ready to be wrapped in whichever
    /// failure that door speaks. It means a previous call panicked while
    /// holding this project's memory: its state is no longer known, and
    /// answering from it would be answering from wreckage.
    pub fn holding<T>(&self, work: impl FnOnce(&mut Domain) -> T) -> Result<T, String> {
        let (outcome, moved) = {
            let mut domain = self.domain()?;
            let before = domain.seen_revision().map(ToOwned::to_owned);
            let outcome = work(&mut domain);
            let moved = moved(before.as_deref(), domain.seen_revision());
            (outcome, moved)
        };
        if moved {
            self.moves.say(&self.path);
        }
        Ok(outcome)
    }

    /// This project's memory, held until the guard is dropped.
    ///
    /// The one place that decides what a poisoned lock means, so that both
    /// doors say the same sentence about it rather than each inventing one.
    fn domain(&self) -> Result<MutexGuard<'_, Domain>, String> {
        self.domain.lock().map_err(|_| {
            format!(
                "the memory of `{}` is no longer usable — restart the server",
                self.path.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const A: &str = r#"[{"path": "/w/a", "name": "A", "identifier": "A"}]"#;
    const A_AND_B: &str = r#"[{"path": "/w/a", "name": "A", "identifier": "A"},
                              {"path": "/w/b", "name": "B", "identifier": "B"}]"#;
    const A_ELSEWHERE: &str = r#"[{"path": "/w/moved", "name": "A", "identifier": "A"}]"#;

    /// A registry file holding `body`, and the directory it lives in.
    ///
    /// The directory is returned with it because dropping it deletes the file,
    /// and a registry that vanished mid-test would be testing the wrong thing.
    fn registry(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a directory to write in");
        let path = dir.path().join("registered-projects.json");
        std::fs::write(&path, body).expect("the registry was written");
        (dir, path)
    }

    /// The invariant the whole two-door arrangement rests on: a repository has
    /// one memory in this process, whichever door asked for it. Two would mean
    /// two indexes, two views of the revision, and writes racing inside a
    /// single process.
    #[test]
    fn one_repository_is_one_memory_however_it_was_reached() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path, None).expect("the registry was read");

        let by_key = projects.holding("A").expect("the agent's door answers");
        let by_path = projects.at(Path::new("/w/a"), None);

        assert!(
            Arc::ptr_eq(&by_key, &by_path),
            "the host door opened a second memory over a repository the agent's door already held"
        );
    }

    /// The two doors have different policies, and this is the difference. The
    /// window has to read a project's own record to find the identifier it will
    /// register by, so the host door serves a path the registry has never heard
    /// of — and an agent must not reach that project until the machine says it
    /// answers for it.
    #[test]
    fn a_path_the_registry_does_not_name_is_served_to_the_host_and_not_to_an_agent() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path, None).expect("the registry was read");

        let adopted = projects.at(Path::new("/w/fresh"), None);
        assert!(
            Arc::ptr_eq(&adopted, &projects.at(Path::new("/w/fresh"), None)),
            "asking twice opened it twice"
        );
        assert_eq!(
            projects.keys(),
            ["A"],
            "a project the machine has not registered is not one it answers for"
        );
        assert!(
            projects.holding("FRESH").is_none(),
            "an agent reached past the registry that decides what an agent may reach"
        );
    }

    /// What happens between the window reading a project and registering it.
    /// The handle has to survive that, or the opening flow pays for reopening
    /// the memory it is in the middle of using.
    #[test]
    fn a_project_registered_after_the_host_door_opened_it_keeps_the_memory_it_had() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");

        let before = projects.at(Path::new("/w/b"), None);
        std::fs::write(&path, A_AND_B).expect("the registry was rewritten");

        let after = projects.holding("B").expect("B is answered for now");
        assert!(
            Arc::ptr_eq(&before, &after),
            "registering a project reopened memory the host door already had open"
        );
        assert!(
            Arc::ptr_eq(&before, &projects.at(Path::new("/w/b"), None)),
            "and the host door was handed a different one afterwards"
        );
    }

    #[test]
    fn a_project_registered_after_the_server_started_is_answered_for() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");
        assert_eq!(projects.keys(), ["A"]);

        std::fs::write(&path, A_AND_B).expect("the registry was rewritten");
        assert_eq!(projects.keys(), ["A", "B"]);
        assert!(
            projects.holding("b").is_some(),
            "B could not be called by name"
        );
    }

    /// The reason the list is merged rather than rebuilt: an open project holds
    /// a repository and whatever it has loaded, and somebody else registering a
    /// project is no reason to make it load again.
    #[test]
    fn a_project_that_was_already_open_stays_open() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");
        let before = projects.holding("A").expect("A is answered for");

        std::fs::write(&path, A_AND_B).expect("the registry was rewritten");
        let after = projects.holding("A").expect("A is still answered for");

        assert!(
            Arc::ptr_eq(&before, &after),
            "A was reopened when B arrived"
        );
    }

    #[test]
    fn a_project_whose_path_moved_is_opened_afresh() {
        let (_dir, path) = registry(A);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");
        let before = projects.holding("A").expect("A is answered for");

        std::fs::write(&path, A_ELSEWHERE).expect("the registry was rewritten");
        let after = projects.holding("A").expect("A is still answered for");

        assert!(
            !Arc::ptr_eq(&before, &after),
            "the same name over another directory kept the old memory"
        );
    }

    #[test]
    fn a_project_the_machine_forgot_stops_being_answered_for() {
        let (_dir, path) = registry(A_AND_B);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");
        assert_eq!(projects.keys(), ["A", "B"]);

        std::fs::write(&path, A).expect("the registry was rewritten");
        assert_eq!(projects.keys(), ["A"]);
        assert!(projects.holding("B").is_none(), "B outlived its entry");
    }

    /// A registry that stops being readable is not a reason to forget what is
    /// already being served.
    #[test]
    fn an_unreadable_registry_leaves_the_list_it_had() {
        let (_dir, path) = registry(A_AND_B);
        let projects = Projects::registered(path.clone(), None).expect("the registry was read");

        std::fs::remove_file(&path).expect("the registry was removed");
        assert_eq!(projects.keys(), ["A", "B"]);

        std::fs::write(&path, "{ not a list").expect("the registry was rewritten");
        assert_eq!(projects.keys(), ["A", "B"]);
    }

    #[test]
    fn a_list_with_no_file_behind_it_is_not_looked_for() {
        let projects = Projects::over(Vec::new(), None);
        assert!(projects.keys().is_empty(), "got {:?}", projects.keys());
    }

    #[test]
    fn a_registry_that_cannot_be_read_at_all_is_refused_rather_than_emptied() {
        let dir = tempfile::tempdir().expect("a directory to write in");
        assert!(Projects::registered(dir.path().join("absent.json"), None).is_err());
    }

    /// The three cases of "did this call move the memory", stated apart.
    ///
    /// The middle one is the one that would be got wrong: a project nobody had
    /// read yet gains a revision on the first call that needs it, and that is
    /// somebody opening it rather than anything happening to it. Reported as a
    /// move, opening a project in one window would tell every other window
    /// watching that repository to read it all again.
    #[test]
    fn a_first_reading_is_not_the_memory_moving() {
        assert!(!moved(None, None), "nothing has been read");
        assert!(!moved(None, Some("abc")), "the project was opened");
        assert!(!moved(Some("abc"), Some("abc")), "the call read something");
        assert!(moved(Some("abc"), Some("def")), "the call wrote something");
    }

    /// Nothing is said until somebody says where to say it, and a process
    /// nobody attended never gains one. That is the ordinary state of a
    /// `sync-mcp` somebody started in a terminal, and it must not be a failure.
    #[test]
    fn a_move_with_nobody_listening_is_silent() {
        let moves = Moves::default();
        moves.say(Path::new("/w/a"));
    }

    /// What is reported is the project's own path, so that a window holding
    /// several open can tell which of them changed under it.
    #[test]
    fn a_move_names_the_project_it_happened_to() {
        let projects = Projects::over(Vec::new(), None);
        let heard = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let telling = Arc::clone(&heard);
        projects.report_moves_to(move |path| {
            telling
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(path.to_owned());
        });

        projects.moves.say(Path::new("/w/a"));
        projects.moves.say(Path::new("/w/b"));

        assert_eq!(
            *heard.lock().unwrap_or_else(PoisonError::into_inner),
            vec![PathBuf::from("/w/a"), PathBuf::from("/w/b")]
        );
    }

    /// A project this process has never read is not reported when something
    /// asks it a question — which is what the first call of every session is.
    #[test]
    fn opening_a_project_is_not_reported_as_a_change_to_it() {
        let projects = Projects::over(Vec::new(), None);
        let heard = Arc::new(Mutex::new(0_usize));
        let telling = Arc::clone(&heard);
        projects.report_moves_to(move |_| {
            *telling.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        });

        let held = projects.at(Path::new("/w/never-read"), None);
        // A call that does not touch the memory at all, which is as far as a
        // test with no repository behind it can go — and it is the case that
        // matters: the revision was never read, so there is nothing to compare.
        held.holding(|_| ()).expect("the memory was not poisoned");

        assert_eq!(
            *heard.lock().unwrap_or_else(PoisonError::into_inner),
            0,
            "a project that has never been read was reported as having changed"
        );
    }
}
