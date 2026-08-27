//! What of the engine's surface this server publishes.
//!
//! An allow-list, not a deny-list. The engine gains use cases on its own
//! cadence, and one it adds tomorrow stays closed until somebody opens it here
//! deliberately — the opposite way round, a release of the engine would widen
//! Sync's interface without anybody deciding to.
//!
//! Storage administration — migration, transport, `doctor` — is not published
//! at any point. Those are the window's, and an agent that could move a storage
//! is an agent that can lock a person out of their own memory.

use rmcp::model::Tool;
use serde_json::Value;

/// The engine tools a client of this server may reach, under the engine's own
/// names.
///
/// Reads only. Writing arrives as one product-level tool over the service's
/// `apply_transaction`, because that is where the conflict replay belongs — a
/// raw `apply_transaction` published here would make every client implement it
/// again.
pub const PUBLISHED: &[&str] = &[
    "memory_get_record",
    "memory_list_records",
    "memory_list_folders",
    "memory_list_types",
    "memory_read_content",
    "memory_search",
    "memory_backlinks",
    "memory_diff",
    "memory_schema_status",
];

/// Whether a name is one this server serves.
///
/// Asked before dispatch, so a tool the engine has but this server does not
/// publish is refused here rather than reached through a name a client guessed.
pub fn is_published(name: &str) -> bool {
    PUBLISHED.contains(&name)
}

/// The published tools, described the way the engine describes them.
///
/// The descriptions and schemas are not restated here on purpose: they are the
/// engine's, they change with it, and a second copy in Sync would be a second
/// copy to drift. `memory-hub-mcp::list_tools` already answers in MCP's own
/// shape — `{"tools": [{name, description, inputSchema}]}` — so this filters
/// and hands them on.
///
/// # Errors
///
/// Every way of misreading the catalogue is an error rather than a shorter
/// list. A server that answered `tools/list` with nothing because the engine's
/// shape moved under it would look, to every client, exactly like a project
/// that has no memory.
pub fn tools() -> Result<Vec<Tool>, CatalogueError> {
    let catalogue = memory_hub_mcp::list_tools();
    let entries = catalogue
        .get("tools")
        .and_then(Value::as_array)
        .ok_or(CatalogueError::Shape)?;
    let mut published = Vec::with_capacity(PUBLISHED.len());
    let mut seen = Vec::with_capacity(PUBLISHED.len());
    for entry in entries {
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !is_published(name) {
            continue;
        }
        seen.push(name);
        let tool: Tool = serde_json::from_value(entry.clone()).map_err(CatalogueError::Tool)?;
        published.push(tool);
    }
    let missing: Vec<&str> = PUBLISHED
        .iter()
        .copied()
        .filter(|name| !seen.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(CatalogueError::Missing(missing.join(", ")));
    }
    Ok(published)
}

/// Why the engine's catalogue could not be published as it stands.
#[derive(Debug)]
pub enum CatalogueError {
    /// `list_tools` did not answer with `{"tools": [...]}`.
    Shape,
    /// Something in it is not an MCP tool description.
    Tool(serde_json::Error),
    /// The allow-list names tools this engine does not have — it renamed or
    /// removed them. Said out loud, because the alternative is Sync's
    /// interface quietly shrinking on an engine upgrade.
    Missing(String),
}

impl std::fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape => f.write_str("the engine's tool catalogue is not `{tools: [...]}`"),
            Self::Tool(error) => {
                write!(f, "a tool in the engine's catalogue is unreadable: {error}")
            }
            Self::Missing(names) => {
                write!(f, "the engine no longer has these published tools: {names}")
            }
        }
    }
}

impl std::error::Error for CatalogueError {}
