//! The window's channel.
//!
//! Not MCP, and deliberately so: an MCP tool is a piece of a
//! model's prompt — it costs the agent context and has to explain itself in a
//! voice written for a model. The window's operations owe none of that. They
//! are calls with typed results made by a caller that already knows what it is
//! doing, and putting them in the tool model would commit us to writing
//! model-facing prose forever for things no model should reach.
//!
//! So they are not hidden tools. They are not tools. The agent's connection has
//! no route to them at all.
//!
//! Nothing here knows its transport. Today the window spawns this process and
//! owns both pipes, so the channel is stdio; when a resident process earns its
//! keep, the same dispatcher answers over a socket without changing.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use memory_hub_mcp::EmbeddingProvider;
use serde_json::{Value, json};
use sync_memory::{CHANNEL_VERSION, METHODS, MemoryError, PROJECTS, Result};

use crate::domain::Domain;
use crate::projects::Projects;

mod operations;

/// One product operation, named on the wire.
///
/// A trait rather than a match arm so the set is data: the built-ins register
/// themselves at start-up, and an extension that contributes an operation in a
/// later stage registers one the same way, without this file learning its name.
pub trait Operation: Send + Sync {
    /// What the window calls it. Dotted, subject first — `types.list`, not
    /// `list_types` — so the set reads as a surface rather than a pile.
    fn name(&self) -> &'static str;

    /// Whether this operation needs the project's memory to be readable.
    ///
    /// True for almost everything, and the default says so. The exceptions
    /// describe the engine rather than the corpus, and a person reaches them
    /// *because* the memory is not readable: demanding a revision for those
    /// would be demanding to read the memory in order to make it readable.
    fn needs_memory(&self) -> bool {
        true
    }

    /// Run it against the project's memory.
    ///
    /// The body is a few lines: read what the call carries, hand it to
    /// [`Domain`], answer with what came back. Everything that knows a
    /// decision from a document lives there, so an operation stays an
    /// adapter — which is what keeps adding one cheap.
    ///
    /// # Errors
    ///
    /// Returns whatever the domain refused, unchanged.
    fn run(&self, domain: &mut Domain, params: &Value) -> Result<Value>;
}

/// The window's surface, over every project this process holds.
///
/// It owns no memory of its own. A call names the project it is about and the
/// surface asks [`Projects`] for it, which is what lets one process serve the
/// window, the clock and every agent from one set of open repositories and one
/// loaded model.
pub struct Host {
    projects: Arc<Projects>,
    /// The model this process resolved, handed to a project opened on demand so
    /// that reaching a new one costs no second copy of it.
    embeddings: Option<Arc<dyn EmbeddingProvider>>,
    operations: BTreeMap<&'static str, Box<dyn Operation>>,
}

impl Host {
    /// Open the surface over the projects this process holds.
    ///
    /// Infallible, because a project whose memory cannot be read yet is exactly
    /// the case the window has to be told about in words. The refusal arrives
    /// as the answer to the first call that needed a corpus, carrying the
    /// engine's own `kind`; a constructor that refused instead would end the
    /// process, and a closed pipe names nothing.
    pub fn over(projects: Arc<Projects>, embeddings: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        let mut host = Self {
            projects,
            embeddings,
            operations: BTreeMap::new(),
        };
        for operation in operations::operations() {
            host.register(operation);
        }
        host
    }

    /// Add an operation to the surface.
    ///
    /// Public because the surface is meant to grow from outside this module —
    /// the built-ins use the same door an extension will.
    pub fn register(&mut self, operation: Box<dyn Operation>) {
        self.operations.insert(operation.name(), operation);
    }

    /// Every method this surface answers, in order — including the one that
    /// answers this. A list that leaves itself out is a list the caller cannot
    /// trust to be the whole of it.
    #[must_use]
    pub fn methods(&self) -> Vec<&'static str> {
        let mut methods: Vec<&'static str> = self.operations.keys().copied().collect();
        methods.push(METHODS);
        methods.push(PROJECTS);
        methods.sort_unstable();
        methods
    }

    /// Whether this call can be answered before a project has been named.
    ///
    /// Asked by a door before it demands a project of its caller, so that the
    /// two questions a connection has to ask first — what do you answer, and
    /// what have you got — are not refused for being asked first.
    #[must_use]
    pub fn answers_without_project(method: &str) -> bool {
        matches!(method, METHODS | PROJECTS)
    }

    /// Answer the one call every connection makes first.
    ///
    /// It carries two things and they answer two different questions. The list
    /// says what this surface can do, which is what catches a sidecar older
    /// than the window that asked. The number says what the *channel* is, which
    /// is what catches the pair being incompatible at all — a client from a
    /// store is months behind by construction, and it must be told so in a
    /// sentence rather than left to fail on the first call whose shape moved.
    ///
    /// A caller that states no version is answered rather than refused. The
    /// number is only enforceable where both ends state it, and the answer
    /// carries it either way, so a caller that says nothing can still read what
    /// it is talking to.
    fn handshake(&self, params: &Value) -> Result<Value> {
        if let Some(stated) = params.get("channel").and_then(Value::as_u64) {
            let ours = u64::from(CHANNEL_VERSION);
            if stated != ours {
                let older = if stated < ours { "caller" } else { "engine" };
                return Err(MemoryError::domain(
                    "unsupported",
                    format!(
                        "this channel is version {ours} and the caller speaks {stated} — the {older} is the older of the two"
                    ),
                    Value::Null,
                ));
            }
        }
        Ok(json!({"channel": CHANNEL_VERSION, "methods": self.methods()}))
    }

    /// Where the project called `key` is, if this machine holds one.
    ///
    /// The registry is the whole of the answer. A caller that names a project
    /// this machine has not registered is told so by name, and nothing goes
    /// near the file system on its behalf — which is the difference between a
    /// key and a path, and the reason a connection from elsewhere gets only the
    /// former.
    #[must_use]
    pub fn project_named(&self, key: &str) -> Option<std::path::PathBuf> {
        self.projects
            .holding(key)
            .map(|project| project.path().to_owned())
    }

    /// Run one call.
    ///
    /// # Errors
    ///
    /// `unsupported` when nothing answers to that name — said plainly rather
    /// than as a silence the caller has to time out on.
    pub fn dispatch(&self, project: &Path, method: &str, params: &Value) -> Result<Value> {
        // Answered by the surface rather than by an operation, and both for the
        // same reason: an [`Operation`] is handed a [`Domain`], which is one
        // project's memory, and neither of these is about a project. The window
        // asks the first on connecting — a bundled sidecar older than the window
        // is a mismatch worth finding at the handshake instead of at the first
        // call that is not there — and a client with no file system in front of
        // it asks the second before it can ask anything else at all.
        if method == METHODS {
            return self.handshake(params);
        }
        if method == PROJECTS {
            return Ok(self.projects.listed());
        }
        let Some(operation) = self.operations.get(method) else {
            return Err(MemoryError::domain(
                "unsupported",
                format!("no operation named `{method}`"),
                Value::Null,
            ));
        };
        let held = self.projects.at(project, self.embeddings.as_ref());
        let mut domain = held
            .domain()
            .map_err(|reason| MemoryError::domain("unusable", reason, Value::Null))?;
        // Here rather than in each operation, and asked of the operation rather
        // than assumed: the answer is a property of what is being called, and
        // thirty-eight copies of one `?` is how a rule stops being one.
        if operation.needs_memory() {
            domain.ensure_initialised()?;
        }
        operation.run(&mut domain, params)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn surface() -> Host {
        Host::over(Arc::new(Projects::over(Vec::new(), None)), None)
    }

    /// The handshake carries both halves of what a caller has to know: what
    /// this surface answers, and what channel it is answering on.
    #[test]
    fn the_handshake_states_the_channel_and_the_surface() {
        let answered = surface()
            .handshake(&json!({"channel": CHANNEL_VERSION}))
            .expect("the versions agree");
        assert_eq!(answered["channel"], CHANNEL_VERSION);
        assert!(
            answered["methods"]
                .as_array()
                .expect("a list")
                .iter()
                .any(|method| method == METHODS),
            "the list leaves itself out: {answered}"
        );
    }

    /// A caller behind this engine and a caller ahead of it are two different
    /// sentences, and both have to name which side is old. "Incompatible" alone
    /// leaves a person with a working application and no idea what to update.
    #[test]
    fn a_caller_at_another_version_is_told_which_side_is_old() {
        let older = surface()
            .handshake(&json!({"channel": u64::from(CHANNEL_VERSION) - 1}))
            .expect_err("an older caller");
        assert!(
            older.to_string().contains("the caller is the older"),
            "{older}"
        );

        let newer = surface()
            .handshake(&json!({"channel": u64::from(CHANNEL_VERSION) + 1}))
            .expect_err("a newer caller");
        assert!(
            newer.to_string().contains("the engine is the older"),
            "{newer}"
        );
    }

    /// A key is resolved through the registry and nowhere else. That is the
    /// difference between a key and a path: one names something this machine
    /// agreed to hold, the other names a directory.
    #[test]
    fn a_key_is_answered_from_the_registry_and_an_unknown_one_is_not_answered_at_all() {
        let host = Host::over(
            Arc::new(Projects::over(
                vec![crate::projects::Registered {
                    path: std::path::PathBuf::from("/w/a"),
                    name: "A".to_owned(),
                    identifier: "A".to_owned(),
                }],
                None,
            )),
            None,
        );

        assert_eq!(
            host.project_named("A"),
            Some(std::path::PathBuf::from("/w/a"))
        );
        // Not a path derived from the name, not a folder looked for: nothing.
        assert_eq!(host.project_named("B"), None);
    }

    /// The list is answered by the surface, so it is answered whatever project
    /// the caller has named — including none.
    #[test]
    fn the_projects_are_listed_without_a_project_being_named() {
        let host = Host::over(
            Arc::new(Projects::over(
                vec![crate::projects::Registered {
                    path: std::path::PathBuf::from("/w/a"),
                    name: "A".to_owned(),
                    identifier: "A".to_owned(),
                }],
                None,
            )),
            None,
        );

        assert!(Host::answers_without_project(PROJECTS));
        let listed = host
            .dispatch(Path::new(""), PROJECTS, &json!({}))
            .expect("the list needs no project");
        assert_eq!(listed["projects"][0]["project"], "A");
    }

    /// Every operation says what it does to the project, and the list that says
    /// so is in the crate both clients read.
    ///
    /// The check is here rather than there because only this side knows what
    /// the surface actually holds. A client off this machine replays a call
    /// whose answer never arrived, and it may only do that for a call that
    /// wrote nothing — so an operation added here and left out of that list is
    /// a duplicate record on somebody's phone, arriving once in a while, with
    /// nothing on either end saying why.
    #[test]
    fn every_operation_says_whether_it_writes() {
        let unclassified: Vec<&str> = surface()
            .methods()
            .into_iter()
            .filter(|method| sync_memory::effect(method).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "these operations are not named in `sync_memory::effect`: {unclassified:?}"
        );
    }

    /// A caller that states nothing is answered, and the answer is where it
    /// finds the number it did not send.
    #[test]
    fn a_caller_that_states_no_version_is_answered_with_ours() {
        let answered = surface()
            .handshake(&json!({}))
            .expect("nothing was claimed, so nothing disagrees");
        assert_eq!(answered["channel"], CHANNEL_VERSION);
    }
}
