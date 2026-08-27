//! Translating MCP tool names between our vocabulary and each agent's.
//!
//! The same MCP tool does not have the same name twice. One `sync_status`,
//! served by one server called `sync`, was seen in live `tool_call` frames as
//! all four of:
//!
//! | Agent    | On the wire              |
//! |----------|--------------------------|
//! | Claude   | `mcp__sync__sync_status` |
//! | Codex    | `sync/sync_status`       |
//! | `OpenCode` | `sync_sync_status`       |
//! | Grok     | `sync__sync_status`      |
//!
//! Anything that addresses a tool by name — recognising our own calls in the
//! event stream, pre-approving a server's tools, filtering — has to go through
//! here, or it works on exactly one agent.
//!
//! Both directions come off one function, [`McpToolNaming::prefix`]: rendering
//! is prefix + tool, parsing is strip the prefix. They cannot drift apart.

/// A tool as *we* name it: which MCP server serves it, and its name there.
///
/// This is the canonical form. The wire spellings above are all renderings of
/// it, and none of them is the name itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpToolName {
    /// The MCP server's name, as we gave it in `session/new`.
    pub server: String,
    /// The tool's name, as the server itself publishes it.
    pub tool: String,
}

impl McpToolName {
    /// Builds a canonical tool name.
    #[must_use]
    pub fn new(server: impl Into<String>, tool: impl Into<String>) -> Self {
        Self {
            server: server.into(),
            tool: tool.into(),
        }
    }
}

/// How one agent spells MCP tool names.
///
/// Each variant is a measurement off a live `tool_call` frame, not a reading of
/// anyone's documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum McpToolNaming {
    /// `mcp__<server>__<tool>` — measured on Claude.
    McpDoubleUnderscore,
    /// `<server>/<tool>` — measured on Codex.
    Slash,
    /// `<server>_<tool>` — measured on `OpenCode`.
    Underscore,
    /// `<server>__<tool>` — measured on Grok.
    DoubleUnderscore,
}

impl McpToolNaming {
    /// The prefix this agent puts in front of a tool from `server`.
    ///
    /// The single source both directions are derived from.
    #[must_use]
    pub fn prefix(self, server: &str) -> String {
        match self {
            Self::McpDoubleUnderscore => format!("mcp__{server}__"),
            Self::Slash => format!("{server}/"),
            Self::Underscore => format!("{server}_"),
            Self::DoubleUnderscore => format!("{server}__"),
        }
    }

    /// Our name → this agent's name.
    #[must_use]
    pub fn render(self, name: &McpToolName) -> String {
        format!("{}{}", self.prefix(&name.server), name.tool)
    }

    /// This agent's name → our name.
    ///
    /// `servers` is the set of MCP servers we handed the agent in
    /// `session/new`, and it is required rather than inferred: `sync_sync_status`
    /// splits into `sync` + `sync_status` only because we know a server called
    /// `sync` exists. Guessing the split from the string alone would be a coin
    /// toss on every underscore.
    ///
    /// Returns `None` when the name belongs to none of `servers` — an agent's
    /// own built-in tool, which is not ours to translate.
    #[must_use]
    pub fn parse<S: AsRef<str>>(self, wire: &str, servers: &[S]) -> Option<McpToolName> {
        servers.iter().find_map(|server| {
            let server = server.as_ref();
            let tool = wire.strip_prefix(&self.prefix(server))?;
            if tool.is_empty() {
                return None;
            }
            Some(McpToolName::new(server, tool))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four spellings exactly as they were captured off the wire, against
    /// the naming each agent was measured to use.
    const OBSERVED: [(McpToolNaming, &str); 4] = [
        (McpToolNaming::McpDoubleUnderscore, "mcp__sync__sync_status"),
        (McpToolNaming::Slash, "sync/sync_status"),
        (McpToolNaming::Underscore, "sync_sync_status"),
        (McpToolNaming::DoubleUnderscore, "sync__sync_status"),
    ];

    #[test]
    fn renders_every_observed_spelling() {
        let ours = McpToolName::new("sync", "sync_status");
        for (naming, wire) in OBSERVED {
            assert_eq!(naming.render(&ours), wire, "rendering for {naming:?}");
        }
    }

    #[test]
    fn parses_every_observed_spelling_back() {
        let ours = McpToolName::new("sync", "sync_status");
        for (naming, wire) in OBSERVED {
            assert_eq!(
                naming.parse(wire, &["sync"]),
                Some(ours.clone()),
                "parsing for {naming:?}"
            );
        }
    }

    #[test]
    fn a_tool_that_is_not_ours_does_not_get_claimed() {
        // Codex's own `review`, `OpenCode`'s `bash` — an agent's built-ins share
        // the stream with ours and must come back as "not one of mine".
        for (naming, _) in OBSERVED {
            assert_eq!(naming.parse("read_file", &["sync"]), None, "{naming:?}");
        }
    }

    #[test]
    fn a_different_servers_tool_is_not_ours_either() {
        // `git-sync` and `sync` are both real server names we have used.
        let naming = McpToolNaming::McpDoubleUnderscore;
        assert_eq!(naming.parse("mcp__other__status", &["sync"]), None);
        assert_eq!(
            naming.parse("mcp__git-sync__status", &["sync", "git-sync"]),
            Some(McpToolName::new("git-sync", "status"))
        );
    }

    #[test]
    fn the_prefix_alone_is_not_a_tool() {
        for (naming, _) in OBSERVED {
            let prefix = naming.prefix("sync");
            assert_eq!(naming.parse(&prefix, &["sync"]), None, "{naming:?}");
        }
    }

    #[test]
    fn underscore_and_double_underscore_do_not_answer_for_each_other() {
        // Grok's `sync__sync_status` under `OpenCode`'s single-underscore rule
        // would yield the tool `_sync_status`, which no server publishes. The
        // schemes are per-agent for exactly this reason, so the test pins that
        // each one reads its own spelling and reports the other's honestly.
        assert_eq!(
            McpToolNaming::Underscore.parse("sync__sync_status", &["sync"]),
            Some(McpToolName::new("sync", "_sync_status")),
        );
        assert_eq!(
            McpToolNaming::DoubleUnderscore.parse("sync_sync_status", &["sync"]),
            None,
        );
    }

    #[test]
    fn round_trips_through_every_naming() {
        let ours = McpToolName::new("sync", "sync_doc_create");
        for (naming, _) in OBSERVED {
            let wire = naming.render(&ours);
            assert_eq!(
                naming.parse(&wire, &["sync"]),
                Some(ours.clone()),
                "{naming:?}"
            );
        }
    }
}
