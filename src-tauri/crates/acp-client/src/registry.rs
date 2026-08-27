//! How each agent CLI is raised into ACP — as data.
//!
//! None of this is derivable from the protocol. `initialize` tells you what an
//! agent can do only once it is already running, and nothing anywhere tells you
//! that Grok's ACP server hides behind `agent stdio` while `OpenCode`'s is
//! `acp`, that Codex needs our bridge to its official app-server protocol, or
//! that the Claude adapter refuses to start while `CLAUDECODE` is set in its
//! environment.
//!
//! So it is a table, and it stays a table: a row per agent, read by code that
//! never branches on which row it is holding. Adding an agent is a row; the
//! launch path does not change. (That is also what keeps the agent-agnostic
//! runtime constraint intact — these names are data this module publishes, not
//! conditions the code tests.)
//!
//! Every row is a measurement from the live spike, and rows differ in how far
//! that measurement got — see [`Verification`].

use crate::tool_names::McpToolNaming;

/// One agent CLI and everything needed to raise it into an ACP conversation.
///
/// Constructible from outside the crate on purpose: a row is a written-down
/// measurement, and a consumer that has measured an agent this table does not
/// carry — or a test that needs a stub — should be able to write one down too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunchSpec {
    /// Stable identifier for this row.
    pub id: &'static str,
    /// Name for a human.
    pub display_name: &'static str,
    /// The program to run. A bare name on purpose: resolving it against the
    /// user's `PATH` (or a version manager's shims) is the caller's job, and
    /// this crate has no business looking at the filesystem.
    pub program: &'static str,
    /// Arguments that put the program into ACP mode.
    pub args: &'static [&'static str],
    /// Environment variables that must be *removed* from the child's
    /// environment before it starts.
    pub unset_env: &'static [&'static str],
    /// Arguments that put this agent under no approval policy and no sandbox,
    /// for a session that answers permission requests itself
    /// (`SYNC:s-acp-permission-policy`). Passed only when the caller asks for
    /// full access ([`crate::launch::SpawnOptions::full_access`]) — an agent
    /// whose owner wants to be consulted must keep its own policy.
    ///
    /// Empty is an ordinary value and the common one: an agent that needs no
    /// argument to work in its workspace, or one whose flags were never
    /// measured. Answering the request is what restores the agent's reach;
    /// these arguments are what stop the agent's own sandbox from refusing
    /// underneath the answer.
    pub full_access_args: &'static [&'static str],
    /// Whether the CLI speaks ACP itself or needs translation in front.
    pub acp_mode: AcpMode,
    /// How this agent spells MCP tool names, when that was measured.
    /// `None` means no `tool_call` frame was ever seen from it.
    pub tool_naming: Option<McpToolNaming>,
    /// How this agent is told which model to run, at launch. `None` means it
    /// cannot be told at launch at all — see [`ModelPin`].
    pub model_pin: Option<ModelPin>,
    /// How far this row was proven, live.
    pub verification: Verification,
}

impl AgentLaunchSpec {
    /// Whether the full `initialize` → `session/new` → `session/prompt` →
    /// `StopReason` cycle was seen working on this agent.
    ///
    /// The gate a caller wants before offering an agent as a working choice.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self.verification, Verification::LiveFullCycle)
    }
}

/// How an agent takes a model id when it is raised.
///
/// A per-row measurement like everything else here, and it has to be: the four
/// proven agents disagree completely, and the PTY path's `--model` flag exists
/// on none of them. Each variant below was read off the agent itself — its own
/// `--help`, or its own source — not inferred from a neighbour.
///
/// `None` on a row is a measurement too, and a load-bearing one: it says the
/// agent takes no model at launch by any means found, so a caller with a model
/// to pass knows it has nowhere to put it rather than quietly dropping it.
/// `OpenCode` (`opencode acp` offers no model flag; `-m` belongs to `run`) and
/// Grok (`grok agent stdio` knows only debug flags) are both that case. Both
/// advertise their models in-protocol instead, which is a different mechanism
/// and a different piece of work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelPin {
    /// Two argv items: `flag`, then `key=<id>`.
    ///
    /// Codex app-server takes its whole configuration this way — its `--help`
    /// spells the example out as `-c model="o3"`, an override of what would
    /// otherwise come from `~/.codex/config.toml`.
    ConfigArg {
        /// The flag that introduces one override.
        flag: &'static str,
        /// The configuration key to set.
        key: &'static str,
    },
    /// One environment variable, set to the id verbatim.
    ///
    /// The Claude adapter's own resolution puts this first: "Model priority
    /// (highest to lowest): 1. `ANTHROPIC_MODEL` environment variable".
    Env {
        /// The variable's name.
        name: &'static str,
    },
}

/// Whether the CLI speaks ACP natively or through a translation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AcpMode {
    /// The CLI has its own ACP server.
    Native,
    /// A third-party adapter speaks ACP and drives the CLI behind it. The
    /// adapter is the thing that gets launched, which is why its package name
    /// is recorded here and not in a comment.
    Adapter {
        /// The npm package that provides the adapter.
        package: &'static str,
    },
    /// A bridge owned by the caller translates a provider-native protocol to
    /// ACP. Unlike an adapter package, it uses the user's installed CLI.
    Bridge {
        /// Stable bridge implementation name for diagnostics.
        name: &'static str,
    },
}

/// Registry sentinel resolved by an embedding application to its own binary.
///
/// Codex uses this to enter the bundled `agent-bridge` subcommand without
/// assuming that the app bundle is also installed on the shell's `PATH`.
pub const CURRENT_EXECUTABLE: &str = "@current-executable";

/// How far this row was proven against the real CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verification {
    /// `initialize` → `session/new` → `session/prompt` → `StopReason` all
    /// observed live, plus `session/cancel` and `session/load`.
    LiveFullCycle,
    /// The launch command works and `initialize` was answered, but the cycle
    /// was not completed, for the stated reason.
    InitializeOnly {
        /// Why it stopped there. Kept as data so a caller can say why an agent
        /// is not on offer instead of silently omitting it.
        reason: &'static str,
    },
}

/// Codex through our bridge to its official app-server protocol.
pub const CODEX: AgentLaunchSpec = AgentLaunchSpec {
    id: "codex",
    display_name: "Codex CLI",
    program: CURRENT_EXECUTABLE,
    args: &["agent-bridge", "codex"],
    unset_env: &[],
    // The same two settings the PTY path spelled as `--sandbox
    // danger-full-access --ask-for-approval never`, in the only vocabulary this
    // app-server has: its `--help` documents `-c key=value` as an override of what
    // `~/.codex/config.toml` would otherwise say, and the value is read as TOML
    // with a fall back to the literal string. Codex is the one row that needs
    // this — under its own default policy it can neither write nor run, and
    // answering its permission request does not change that.
    full_access_args: &[
        "-c",
        "approval_policy=never",
        "-c",
        "sandbox_mode=danger-full-access",
    ],
    acp_mode: AcpMode::Bridge {
        name: "codex-app-server",
    },
    tool_naming: Some(McpToolNaming::Slash),
    // Its own `--help`: `-c model="o3"`.
    model_pin: Some(ModelPin::ConfigArg {
        flag: "-c",
        key: "model",
    }),
    verification: Verification::LiveFullCycle,
};

/// `OpenCode`, natively.
pub const OPENCODE: AgentLaunchSpec = AgentLaunchSpec {
    id: "opencode",
    display_name: "`OpenCode`",
    program: "opencode",
    args: &["acp"],
    unset_env: &[],
    // Nothing measured: it works in its own workspace once the request is
    // answered.
    full_access_args: &[],
    acp_mode: AcpMode::Native,
    tool_naming: Some(McpToolNaming::Underscore),
    // `opencode acp` takes no model; `-m/--model` belongs to `opencode run`.
    model_pin: None,
    verification: Verification::LiveFullCycle,
};

/// Grok Build CLI, natively — but under `agent stdio`, not `acp`. `grok acp`
/// tries to raise a TUI and dies on a machine with no controlling terminal.
pub const GROK: AgentLaunchSpec = AgentLaunchSpec {
    id: "grok",
    display_name: "Grok Build CLI",
    program: "grok",
    args: &["agent", "stdio"],
    unset_env: &[],
    // `grok agent stdio` knows only debug flags — there is nothing to pass.
    full_access_args: &[],
    acp_mode: AcpMode::Native,
    tool_naming: Some(McpToolNaming::DoubleUnderscore),
    // `grok agent stdio` knows only debug flags. It names its models in its
    // own `initialize` answer instead (`_meta.modelState`).
    model_pin: None,
    verification: Verification::LiveFullCycle,
};

/// Claude Code, through its ACP adapter.
///
/// `CLAUDECODE` must go: the adapter refuses to start inside another Claude
/// Code session, and it refuses *late* — `initialize` succeeds and nothing
/// looks wrong until `session/new` comes back `Internal error`. `CLAUDE_CODE_ENTRYPOINT`
/// is cleared with it because that is the environment the working live run was
/// made in; only `CLAUDECODE` is named by the adapter's own error message.
pub const CLAUDE: AgentLaunchSpec = AgentLaunchSpec {
    id: "claude",
    display_name: "Claude Code",
    program: "npx",
    args: &["-y", "@agentclientprotocol/claude-agent-acp"],
    unset_env: &["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"],
    // The adapter carries the PTY path's `--permission-mode` no further than
    // the request it raises, which the session now answers.
    full_access_args: &[],
    acp_mode: AcpMode::Adapter {
        package: "@agentclientprotocol/claude-agent-acp",
    },
    tool_naming: Some(McpToolNaming::McpDoubleUnderscore),
    // The adapter reads this before any of its own sources.
    model_pin: Some(ModelPin::Env {
        name: "ANTHROPIC_MODEL",
    }),
    verification: Verification::LiveFullCycle,
};

/// Gemini CLI, natively.
///
/// The launch command is proven — it answers `initialize` with a well-formed
/// ACP frame. Everything past that was refused by Google's server, not by the
/// protocol: the same account is turned away outside ACP too, by a plain
/// `gemini -p`. The row exists so that the day the account tier changes,
/// nothing has to be written; and the flag exists so that until then, a caller
/// can decline to offer it.
pub const GEMINI: AgentLaunchSpec = AgentLaunchSpec {
    id: "gemini",
    display_name: "Gemini CLI",
    program: "gemini",
    args: &["--acp"],
    unset_env: &[],
    // Never measured: no turn ever ran to need it.
    full_access_args: &[],
    acp_mode: AcpMode::Native,
    tool_naming: None,
    // Never measured: the account is refused before a turn ever runs.
    model_pin: None,
    verification: Verification::InitializeOnly {
        reason: "the Google account tier is refused by the server, not by the protocol: \
                 `session/new` returns UNSUPPORTED_CLIENT and a plain `gemini -p` is \
                 refused the same way",
    },
};

/// Every agent this client has been run against.
///
/// Order is the order they were measured in; nothing reads it as a ranking.
pub const ALL: &[AgentLaunchSpec] = &[CODEX, OPENCODE, GROK, CLAUDE, GEMINI];

/// Looks a row up by [`AgentLaunchSpec::id`].
#[must_use]
pub fn find(id: &str) -> Option<&'static AgentLaunchSpec> {
    ALL.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_can_be_found_by_its_own_id() {
        for spec in ALL {
            assert_eq!(find(spec.id), Some(spec), "{} is not findable", spec.id);
        }
        assert_eq!(
            find("goose"),
            None,
            "an agent we never ran must not be here"
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = ALL.iter().map(|spec| spec.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two rows share an id");
    }

    #[test]
    fn the_four_agents_that_completed_a_live_cycle_are_the_verified_ones() {
        let verified: Vec<&str> = ALL
            .iter()
            .filter(|spec| spec.is_verified())
            .map(|spec| spec.id)
            .collect();
        assert_eq!(verified, ["codex", "opencode", "grok", "claude"]);
    }

    #[test]
    fn gemini_carries_its_reason_as_data_not_as_absence() {
        let gemini = find("gemini").expect("gemini is a row");
        assert!(!gemini.is_verified());
        let Verification::InitializeOnly { reason } = gemini.verification else {
            panic!("gemini reached initialize and no further");
        };
        assert!(
            reason.contains("account"),
            "the reason must say it is the account, not the protocol"
        );
    }

    #[test]
    fn the_claude_adapter_row_clears_claudecode() {
        let claude = find("claude").expect("claude is a row");
        assert!(
            claude.unset_env.contains(&"CLAUDECODE"),
            "without this the adapter dies at session/new, long after initialize looked fine"
        );
    }

    #[test]
    fn no_other_row_clears_environment_it_does_not_need_to() {
        for spec in ALL.iter().filter(|spec| spec.id != "claude") {
            assert!(
                spec.unset_env.is_empty(),
                "{} clears environment nothing measured asked for",
                spec.id
            );
        }
    }

    #[test]
    fn every_verified_row_knows_how_its_agent_spells_tool_names() {
        for spec in ALL.iter().filter(|spec| spec.is_verified()) {
            assert!(
                spec.tool_naming.is_some(),
                "{} completed a live cycle, so its tool_call frames were seen",
                spec.id
            );
        }
    }

    #[test]
    fn the_four_verified_agents_use_four_different_namings() {
        let mut namings: Vec<McpToolNaming> = ALL
            .iter()
            .filter(|spec| spec.is_verified())
            .filter_map(|spec| spec.tool_naming)
            .collect();
        let count = namings.len();
        namings.dedup_by(|a, b| a == b);
        namings.sort_by_key(|naming| format!("{naming:?}"));
        namings.dedup();
        assert_eq!(
            namings.len(),
            count,
            "the whole point of the table is that no two of them agree"
        );
    }

    #[test]
    fn adapter_rows_launch_the_adapter_they_name() {
        for spec in ALL {
            if let AcpMode::Adapter { package } = spec.acp_mode {
                assert!(
                    spec.args.contains(&package),
                    "{} claims the {package} adapter but does not launch it",
                    spec.id
                );
            }
        }
    }
}
