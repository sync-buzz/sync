//! The engine, as one call.
//!
//! Everything above this speaks in tool names and JSON, exactly as the window's
//! client did when the engine was a separate process. That is deliberate: the
//! domain code that is moving in here was written against a `call(tool,
//! arguments)` and does not need to learn a new shape to change which side of
//! the process boundary it lives on.
//!
//! What is gone is the boundary itself. There is no framing, no supervision and
//! no restart-on-crash below this: a panic in the engine is a panic in this
//! process. That is the trade the linked engine makes, and it is why the window
//! keeps supervising *this* binary.

use std::path::PathBuf;
use std::sync::Arc;

use memory_hub_mcp::{
    EmbeddingProvider, RecordsIn, Session, ToolCall, ToolCallFailure, ToolFailure,
};
use serde_json::{Value, json};
use sync_memory::{MemoryError, Result};

/// One project's memory, in process.
pub struct Engine {
    session: Session,
}

impl Engine {
    /// Open the memory of the project rooted at `project`, using `embeddings`
    /// for the vector channel.
    ///
    /// The provider is handed in rather than resolved here, because one process
    /// now holds several projects and a model resolved per project is a model
    /// loaded per project — the largest thing either side holds, multiplied by
    /// how many projects somebody has open. `None` says this machine has no
    /// model: search runs on full text alone, and the session will not go
    /// looking for a GGUF of its own afterwards.
    ///
    /// Records go in Git's own metadata, and that is Sync's answer rather than
    /// the engine's: a project here is a Git repository, so memory kept in Git
    /// objects travels with it, versions itself, pushes to the same remote and
    /// puts nothing in the working tree for somebody to wonder about. The
    /// engine will not guess, and this is the one place the guess is not
    /// needed.
    pub fn open(project: PathBuf, embeddings: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        Self {
            session: Session::with_provider(project, RecordsIn::GitMetadata, embeddings),
        }
    }

    /// Run one engine tool and answer with its content.
    ///
    /// Reconciliation before a write and index synchronisation after one happen
    /// inside `Session::call` — see `memory-hub-mcp`. What is dropped here is
    /// what this caller has no client to tell: the record notices and the
    /// revision move. When the host channel grows notifications, they are read
    /// from the same [`ToolCall`] rather than recomputed.
    ///
    /// # Errors
    ///
    /// A tool that failed becomes a [`MemoryError`] carrying the engine's own
    /// `kind`, so a caller can still tell a stale revision from a locked
    /// project without reading prose.
    pub fn call(&mut self, tool: &str, arguments: &Value) -> Result<Value> {
        let ToolCall { result, .. } = self.run(tool, arguments);
        result.map_err(|failure| match failure {
            ToolCallFailure::Rpc(error) => refusal(None, error.message, error.data),
            ToolCallFailure::Tool(error) => refusal(Some(error.kind), error.message, error.data),
        })
    }

    /// Run one engine tool and hand back what the engine said, untouched.
    ///
    /// For the one caller that republishes the engine's answer rather than
    /// acting on it: the MCP surface passes a tool's result — including the
    /// shape of its refusal — straight through to the agent, under the engine's
    /// own name. Restating it in our vocabulary and then restating it back
    /// would be two translations to keep in step for no gain.
    pub fn run(&mut self, tool: &str, arguments: &Value) -> ToolCall {
        self.session.call(tool, arguments)
    }
}

impl Engine {
    /// The project's current staged revision.
    ///
    /// A resource rather than a tool, in the engine as in MCP — so it is read
    /// through the session's resource door rather than dressed up as a call.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Protocol`] when the resource carries something
    /// other than a revision, which would mean this build and the engine
    /// disagree about their own interface.
    pub fn revision(&mut self) -> Result<String> {
        self.resource(sync_memory::REVISION_RESOURCE)?
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                MemoryError::Protocol("the revision resource carried no revision".to_owned())
            })
    }

    /// Read one of the engine's resources, already unwrapped.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Protocol`] when the resource carries something
    /// other than a JSON body, which would mean this build and the engine
    /// disagree about their own interface.
    pub fn resource(&mut self, uri: &str) -> Result<Value> {
        let answer = self
            .session
            .read_resource(&json!({"uri": uri}))
            .map_err(|error| refusal(None, error.message, error.data))?;
        let text = answer
            .get("contents")
            .and_then(Value::as_array)
            .and_then(|contents| contents.first())
            .and_then(|entry| entry.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| MemoryError::Protocol("resource returned no content".to_owned()))?;
        serde_json::from_str(text)
            .map_err(|error| MemoryError::Protocol(format!("unreadable resource body: {error}")))
    }
}

/// Turn the engine's refusal into one of ours, keeping the name it gave.
///
/// The engine names what went wrong twice: once as the shape of the refusal and
/// once, inside, as the thing that actually happened. The inner name is what a
/// caller acts on — `not_initialised` is a project to initialise, while the
/// `invalid_request` wrapping it is only a category — so it wins when it is
/// there. Reading the outer one instead is how a project with no memory looked
/// like a malformed request.
fn refusal(kind: Option<String>, message: String, data: Value) -> MemoryError {
    let named = data
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(kind)
        .unwrap_or_else(|| "invalid_request".to_owned());
    MemoryError::domain(&named, message, data)
}

/// Dress one of our own answers as the engine dresses its own.
///
/// Sync's tools sit beside the engine's on the same surface, and a client
/// should not be able to tell which half answered from the shape of the answer.
/// The engine reports a failed tool as a *successful* call carrying a `kind`, a
/// message and data; so does this.
///
/// `revision_changed` and `changed` are left empty. A caller that acts on them
/// is acting on what the engine reported about its own write, and these tools
/// either do not write or report their own result — `sync_apply` answers with
/// the revision it landed on.
pub fn as_tool_call(answer: Result<Value>) -> ToolCall {
    ToolCall {
        result: answer.map_err(|error| {
            ToolCallFailure::Tool(ToolFailure {
                kind: error
                    .kind()
                    .map_or_else(|| "internal".to_owned(), |kind| kind.as_wire().to_owned()),
                message: error.to_string(),
                data: match &error {
                    MemoryError::Domain { data, .. } => data.clone(),
                    _ => Value::Null,
                },
            })
        }),
        revision_changed: false,
        changed: Vec::new(),
    }
}
