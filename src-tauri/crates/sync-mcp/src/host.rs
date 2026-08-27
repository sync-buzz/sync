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
use serde_json::Value;
use sync_memory::{METHODS, MemoryError, Result};

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
        methods.sort_unstable();
        methods
    }

    /// Run one call.
    ///
    /// # Errors
    ///
    /// `unsupported` when nothing answers to that name — said plainly rather
    /// than as a silence the caller has to time out on.
    pub fn dispatch(&self, project: &Path, method: &str, params: &Value) -> Result<Value> {
        // Answered by the surface rather than by an operation, because it is a
        // question about the surface. The window asks it once on connecting: a
        // bundled sidecar older than the window is a mismatch worth finding at
        // the handshake instead of at the first call that is not there.
        if method == METHODS {
            return Ok(Value::from(self.methods()));
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
