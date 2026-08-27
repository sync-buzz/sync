//! Which agents this machine can actually raise, and where their executables
//! are.
//!
//! The launch registry in `acp-client` says how an agent is started. It cannot
//! say whether it is installed, because that is a fact about this machine and
//! not about the protocol — so the catalogue is the registry plus one probe per
//! row, and the probe is the reason this module exists.
//!
//! # The PATH a window does not have
//!
//! Every agent on a developer's machine is installed by something that edits a
//! login shell's profile: `~/.local/bin` for Claude Code, `~/.opencode/bin`,
//! `~/.grok/bin`, a node version manager's directory for `npx`. A bundled
//! `.app` launched from Finder inherits none of it — `launchd` hands it a
//! short, system-only PATH, and `Command::new("opencode")` fails with "no such
//! file or directory" on a machine where `opencode` plainly works.
//!
//! So the PATH is asked for rather than assumed: the person's own login shell
//! is run once, interactively, and its `PATH` is what every probe and every
//! launch uses. This is what editors do, and it is the difference between an
//! agent list that is empty in the shipped application and one that is right.

use std::path::PathBuf;
use std::sync::OnceLock;

use acp_client::registry::{self, AgentLaunchSpec};
use acp_client::{AcpMode, Verification};
use serde::Serialize;

/// One agent, as the window shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    /// Whether the executable was found on this machine. An agent that is not
    /// installed is still listed: "Codex is not installed" is an answer a
    /// person can act on, and an absence from the list is not.
    pub available: bool,
    /// What is missing, when it is not available.
    pub unavailable_reason: Option<String>,
    /// Whether a full turn was ever run against this agent for real.
    pub verified: bool,
    /// Why it is not, when it is not — the registry's own sentence.
    pub unverified_reason: Option<String>,
    /// `native`, `adapter` or `bridge`. Shown because it explains a slow first
    /// launch: an adapter is fetched by `npx` before a single frame is written.
    pub transport: String,
    /// Whether this agent takes a model when it is raised. The other way — the
    /// agent listing its models in protocol — cannot be known until a session
    /// exists, and arrives with the session's configuration.
    pub takes_model_at_launch: bool,
}

/// The PATH a login shell would have, or `None` where asking failed.
///
/// Asked once. The shell is run with `-ilc` so that an interactive profile —
/// where a version manager writes itself — is what answers.
fn login_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned());
        let output = std::process::Command::new(shell)
            .args(["-ilc", "printf %s \"$PATH\""])
            .output()
            .ok()?;
        let path = String::from_utf8(output.stdout).ok()?;
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        Some(path.to_owned())
    })
    .as_deref()
}

/// The PATH every probe and every launch in this module uses: the login shell's
/// when it could be read, and this process's own otherwise.
pub fn search_path() -> String {
    let inherited = std::env::var("PATH").unwrap_or_default();
    match login_path() {
        // Both, login shell first. A machine where the shell answered but the
        // agent was installed by something else still resolves.
        Some(login) if !inherited.is_empty() => format!("{login}:{inherited}"),
        Some(login) => login.to_owned(),
        None => inherited,
    }
}

/// Where `program` is on this machine, if anywhere.
///
/// [`registry::CURRENT_EXECUTABLE`] resolves to this application's own binary:
/// the Codex row enters our bundled bridge as a subcommand of ourselves, which
/// is what lets it work without the `.app` being on anybody's PATH.
pub fn resolve(program: &str) -> Option<PathBuf> {
    if program == registry::CURRENT_EXECUTABLE {
        return std::env::current_exe().ok();
    }
    let cwd = std::env::current_dir().ok()?;
    which::which_in(program, Some(search_path()), cwd).ok()
}

/// Every agent, with this machine's answer about each one.
pub fn descriptors() -> Vec<AgentDescriptor> {
    registry::ALL.iter().map(describe).collect()
}

/// The row behind an id, or `None` for an id this build does not carry.
pub fn spec(id: &str) -> Option<&'static AgentLaunchSpec> {
    registry::find(id)
}

fn describe(spec: &AgentLaunchSpec) -> AgentDescriptor {
    let found = resolve(spec.program);
    let (verified, unverified_reason) = match spec.verification {
        Verification::LiveFullCycle => (true, None),
        Verification::InitializeOnly { reason } => (false, Some(reason.to_owned())),
        // The enum is `#[non_exhaustive]`: a row proven some new way is not
        // proven by this build until somebody reads what the new way means.
        _ => (false, None),
    };

    AgentDescriptor {
        id: spec.id.to_owned(),
        name: display_name(spec),
        available: found.is_some(),
        unavailable_reason: found.is_none().then(|| missing(spec)),
        verified,
        unverified_reason,
        transport: match spec.acp_mode {
            AcpMode::Native => "native",
            AcpMode::Adapter { .. } => "adapter",
            AcpMode::Bridge { .. } => "bridge",
            _ => "unknown",
        }
        .to_owned(),
        takes_model_at_launch: spec.model_pin.is_some(),
    }
}

/// The row's name, with the backticks a documentation lint left in it taken
/// off. `display_name` is shown to a person, and `` `OpenCode` `` is not a name.
fn display_name(spec: &AgentLaunchSpec) -> String {
    spec.display_name.replace('`', "")
}

/// What to install, in the vocabulary of the thing that is missing rather than
/// of the row: an adapter is fetched by `npx`, so the absent program is node's,
/// not the agent's.
fn missing(spec: &AgentLaunchSpec) -> String {
    match spec.acp_mode {
        AcpMode::Adapter { package } => {
            format!(
                "`{}` was not found, and it is what runs {package}",
                spec.program
            )
        }
        _ => format!("`{}` was not found on this machine", spec.program),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_row_is_described() {
        let described = descriptors();
        assert_eq!(described.len(), registry::ALL.len());
        for row in registry::ALL {
            assert!(
                described.iter().any(|agent| agent.id == row.id),
                "{} is in the registry and not in the catalogue",
                row.id
            );
        }
    }

    #[test]
    fn a_name_shown_to_a_person_carries_no_markup() {
        for agent in descriptors() {
            assert!(
                !agent.name.contains('`'),
                "{} is shown as {:?}",
                agent.id,
                agent.name
            );
        }
    }

    #[test]
    fn gemini_says_why_it_is_unverified_and_the_proven_four_do_not() {
        let described = descriptors();
        let gemini = described
            .iter()
            .find(|agent| agent.id == "gemini")
            .expect("the registry carries Gemini");
        assert!(!gemini.verified);
        assert!(
            gemini.unverified_reason.is_some(),
            "an agent we do not vouch for has to say why"
        );

        for id in ["codex", "opencode", "grok", "claude"] {
            let agent = described
                .iter()
                .find(|agent| agent.id == id)
                .expect("the registry carries the four proven agents");
            assert!(agent.verified, "{id} was proven live");
            assert!(agent.unverified_reason.is_none());
        }
    }

    #[test]
    fn codex_resolves_to_our_own_binary_rather_than_to_a_path_entry() {
        // The bridge is a subcommand of this application, so the row is
        // available on any machine that is running us — including one where
        // nothing called `codex` is on the PATH at all.
        let codex = descriptors()
            .into_iter()
            .find(|agent| agent.id == "codex")
            .expect("the registry carries Codex");
        assert!(codex.available, "our own executable is always there");
        assert_eq!(codex.transport, "bridge");
    }

    #[test]
    fn the_search_path_is_never_narrower_than_this_process_s_own() {
        let inherited = std::env::var("PATH").unwrap_or_default();
        let searched = search_path();
        for entry in inherited.split(':').filter(|entry| !entry.is_empty()) {
            assert!(
                searched.split(':').any(|candidate| candidate == entry),
                "{entry} was dropped from the search path"
            );
        }
    }
}
