//! The packages some agents need, fetched once instead of on every launch.
//!
//! Two of the five agents in the registry are reached through somebody else's
//! npm package: the row says `npx -y <package>`, and `npx` asks the registry
//! what that name resolves to **every time it runs**. So the cost is not only a
//! slow first conversation — it is a network round trip before every one of
//! them, and a package that can change under the application between two of
//! them without anything being said.
//!
//! Here the package is installed once, at a version that is written down, into
//! a directory belonging to this application, and its executable is run
//! directly. Nothing is installed globally and nothing is written anywhere the
//! person maintains themselves: an application that put things in a developer's
//! `node_modules` or in their global npm prefix would be leaving traces they did
//! not ask for and cannot see.
//!
//! # Why removal cannot simply follow uninstalling
//!
//! Installing an extension is a **project's** declaration — it is written into
//! the project's own record and travels with the repository. The package is the
//! **machine's**. So one project dropping Chat cannot be read as "nobody on this
//! machine needs the Claude adapter", and deleting on that signal would break a
//! second project that is using it.
//!
//! The way out is not bookkeeping across projects, which would mean opening
//! every registered project's memory to answer a question about a directory.
//! It is to make the deletion harmless: [`ensure`] is called before a session is
//! raised as well as at install, so an adapter that was removed while another
//! project still wanted it costs that project one slow launch — which is exactly
//! what every launch cost before this module existed — instead of a failure.

use std::path::{Path, PathBuf};

use acp_client::AcpMode;
use acp_client::registry::{self, AgentLaunchSpec};
use serde::Serialize;

/// The version of each adapter this build installs.
///
/// Exact and written down, which is the second half of what this module is for.
/// `npx -y <package>` runs whatever the registry answers today; a version here
/// means the adapter that was measured is the adapter that runs, and that
/// changing it is an edit somebody made rather than a Tuesday.
const PINNED: &[(&str, &str)] = &[("@agentclientprotocol/claude-agent-acp", "0.70.0")];

/// What is true of one agent's adapter on this machine.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterState {
    pub agent_id: String,
    pub package: String,
    pub version: String,
    /// Whether it is installed and ready to run without a fetch.
    pub ready: bool,
}

/// Where this application keeps what it downloaded.
///
/// Under the platform's application-data directory, so it is removed with the
/// application and is visible where a person would look for it.
fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-adapters")
}

/// The package an agent is reached through, if it is reached through one.
fn package_of(spec: &AgentLaunchSpec) -> Option<&'static str> {
    match spec.acp_mode {
        AcpMode::Adapter { package } => Some(package),
        _ => None,
    }
}

fn pinned_version(package: &str) -> Option<&'static str> {
    PINNED
        .iter()
        .find(|(name, _)| *name == package)
        .map(|(_, version)| *version)
}

/// The executable an installed adapter exposes.
///
/// Read out of the package's own manifest, never guessed and never picked from
/// the `.bin` directory by looking. npm hoists the executables of *every*
/// dependency into that one directory: installing this one adapter puts three
/// there, and two of them belong to libraries underneath it. Choosing by name,
/// or by order, launches somebody else's command — measured, not supposed.
///
/// The package name is not the command either: `@agentclientprotocol/claude-agent-acp`
/// declares `claude-agent-acp`. So the manifest's `bin` is the only thing that
/// answers, and it answers in two shapes — a string, or a map of names.
fn executable(data_dir: &Path, package: &str) -> Option<PathBuf> {
    let installed = root(data_dir).join("node_modules").join(package);
    let manifest = std::fs::read_to_string(installed.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest).ok()?;

    let command = match manifest.get("bin")? {
        // `"bin": "dist/index.js"` — the command takes the package's own name,
        // without its scope.
        serde_json::Value::String(_) => package.rsplit('/').next()?.to_owned(),
        // `"bin": { "claude-agent-acp": "dist/index.js" }` — named outright.
        serde_json::Value::Object(map) => map.keys().next()?.clone(),
        _ => return None,
    };

    let path = root(data_dir)
        .join("node_modules")
        .join(".bin")
        .join(command);
    path.is_file().then_some(path)
}

/// Every adapter this build knows about, and whether it is ready.
pub fn state(data_dir: &Path) -> Vec<AdapterState> {
    registry::ALL
        .iter()
        .filter_map(|spec| {
            let package = package_of(spec)?;
            let version = pinned_version(package)?;
            Some(AdapterState {
                agent_id: spec.id.to_owned(),
                package: package.to_owned(),
                version: version.to_owned(),
                ready: executable(data_dir, package).is_some(),
            })
        })
        .collect()
}

/// The executable for `spec`'s adapter, installing it first if it is missing.
///
/// `Ok(None)` for an agent that needs no adapter — every native row, and Codex,
/// whose bridge is compiled into this application.
///
/// # Errors
///
/// The reason as a sentence, when the package cannot be installed: no `npm` on
/// the machine, or the registry could not be reached.
pub fn ensure(data_dir: &Path, spec: &AgentLaunchSpec) -> Result<Option<PathBuf>, String> {
    let Some(package) = package_of(spec) else {
        return Ok(None);
    };
    let version = pinned_version(package)
        .ok_or_else(|| format!("no version of {package} is pinned by this build"))?;

    if let Some(found) = executable(data_dir, package) {
        return Ok(Some(found));
    }
    install(data_dir, package, version)?;
    executable(data_dir, package)
        .map(Some)
        .ok_or_else(|| format!("{package} installed without leaving an executable behind"))
}

/// Installs one package into this application's own directory.
///
/// The manifest written first is not a formality, it is the whole safety of
/// this function. `npm install` in a directory with no `package.json` **walks
/// up** looking for one and installs into whatever it finds — and on macOS the
/// application-data directory is two levels under `~/Library/Application
/// Support`, where some other application had left a manifest of its own. The
/// first version of this installed into that stranger's `node_modules`, wrote a
/// dependency into their file, and reported success while our own directory
/// stayed empty. A manifest here stops the walk at the first step.
fn install(data_dir: &Path, package: &str, version: &str) -> Result<(), String> {
    let prefix = root(data_dir);
    std::fs::create_dir_all(&prefix).map_err(|error| format!("{}: {error}", prefix.display()))?;

    let manifest = prefix.join("package.json");
    if !manifest.is_file() {
        std::fs::write(
            &manifest,
            "{\n  \"name\": \"sync-agent-adapters\",\n  \"private\": true,\n  \"description\": \"Adapters Sync downloaded for the agents it drives. Managed by Sync; safe to delete.\"\n}\n",
        )
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    }

    let npm = super::catalog::resolve("npm").ok_or_else(|| {
        "`npm` was not found on this machine, and it is what installs the adapter".to_owned()
    })?;

    let output = std::process::Command::new(npm)
        .current_dir(&prefix)
        .args([
            "install",
            &format!("{package}@{version}"),
            // The three things that make this quiet and reproducible: no audit
            // report nobody reads, no funding banner, and a resolution that
            // obeys the version above rather than a range.
            "--no-audit",
            "--no-fund",
            "--save-exact",
            // Belt as well as braces. The manifest above is what stops npm
            // walking up; this says where to install even if it somehow did.
            "--prefix",
            &prefix.to_string_lossy(),
        ])
        .env("PATH", super::catalog::search_path())
        .output()
        .map_err(|error| format!("npm could not be run: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    // npm's own last line says what went wrong far better than a status code.
    let said = String::from_utf8_lossy(&output.stderr);
    let reason = said
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or("npm failed without saying why");
    Err(format!("{package} could not be installed: {reason}"))
}

/// Deletes everything this module downloaded.
///
/// Safe to call at any time, including while a project elsewhere is still using
/// the adapter: [`ensure`] runs before every launch, so the cost of being wrong
/// is one slow start rather than a session that will not open.
///
/// # Errors
///
/// The reason as a sentence, when the directory is there and cannot be removed.
pub fn forget(data_dir: &Path) -> Result<(), String> {
    let prefix = root(data_dir);
    match std::fs::remove_dir_all(&prefix) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", prefix.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_agents_reached_through_a_package_have_an_adapter() {
        let described = state(&std::env::temp_dir().join("sync-adapter-state-test"));
        let ids: Vec<&str> = described.iter().map(|row| row.agent_id.as_str()).collect();
        assert_eq!(
            ids,
            ["claude"],
            "Codex is a bridge we compile in, and the rest are native"
        );
    }

    #[test]
    fn every_adapter_in_the_registry_has_a_version_written_down() {
        for spec in registry::ALL {
            if let Some(package) = package_of(spec) {
                assert!(
                    pinned_version(package).is_some(),
                    "{package} is reached through npm and this build pins no version of it"
                );
            }
        }
    }

    #[test]
    fn an_agent_that_needs_no_adapter_is_ready_without_anything_being_fetched() {
        let nowhere = std::env::temp_dir().join("sync-adapter-absent");
        for id in ["opencode", "grok", "gemini", "codex"] {
            let spec = registry::find(id).expect("the registry carries it");
            assert_eq!(
                ensure(&nowhere, spec),
                Ok(None),
                "{id} must not reach the network to be raised"
            );
        }
    }

    /// The command is read from the manifest, not chosen out of `.bin`.
    ///
    /// Installing the one adapter this build needs leaves three executables in
    /// that directory — `claude-agent-acp` and the two belonging to libraries
    /// beneath it. Anything that picks by looking picks one of those.
    #[test]
    fn the_command_comes_from_the_package_s_own_manifest() {
        let root_dir = std::env::temp_dir().join("sync-adapter-bin-test");
        let package = "@scope/agent-acp";
        let installed = root(&root_dir).join("node_modules").join(package);
        let bin = root(&root_dir).join("node_modules").join(".bin");
        std::fs::create_dir_all(&installed).expect("the package directory");
        std::fs::create_dir_all(&bin).expect("the bin directory");
        std::fs::write(
            installed.join("package.json"),
            r#"{"name":"@scope/agent-acp","bin":{"agent-acp":"dist/index.js"}}"#,
        )
        .expect("the manifest");
        // The alphabetically first entry, and the wrong answer.
        std::fs::write(bin.join("aaa-some-library"), "").expect("a hoisted command");
        std::fs::write(bin.join("agent-acp"), "").expect("the adapter's command");

        let found = executable(&root_dir, package).expect("the adapter resolves");
        assert_eq!(
            found.file_name().and_then(|name| name.to_str()),
            Some("agent-acp")
        );

        std::fs::remove_dir_all(&root_dir).expect("the test cleans up");
    }

    #[test]
    fn forgetting_what_was_never_downloaded_is_not_a_failure() {
        let nowhere = std::env::temp_dir().join("sync-adapter-never-there");
        assert_eq!(forget(&nowhere), Ok(()));
    }
}
