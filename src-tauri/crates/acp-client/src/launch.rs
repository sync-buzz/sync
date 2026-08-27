//! Raising an agent process and connecting to its stdio.
//!
//! Kept apart from [`crate::connection`] on purpose: the connection's seam is a
//! reader/writer pair, so everything about the protocol is testable without a
//! process. This module is the only place a process exists.
//!
//! Resolving `program` to an actual executable is *not* done here. Version
//! managers, shims and app bundles make that a question about the user's
//! machine, and this crate does not look at filesystems; pass a resolved path
//! through [`SpawnOptions::program`] when the bare name will not do.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

use crate::connection::{AgentConnection, DEFAULT_REQUEST_TIMEOUT};
use crate::error::{Error, Result};
use crate::handler::ClientHandler;
use crate::registry::{AgentLaunchSpec, ModelPin};

/// How many trailing stderr lines are kept for diagnosis.
const STDERR_RING: usize = 20;

/// Per-launch adjustments to a registry row.
#[derive(Debug, Default, Clone)]
pub struct SpawnOptions {
    /// Resolved path to the executable, when the row's bare `program` name is
    /// not enough — a node version manager's `npx`, a bundled binary, a test
    /// double.
    pub program: Option<PathBuf>,
    /// Arguments in place of the row's own.
    ///
    /// For a caller that has already provisioned what the row would otherwise
    /// fetch. The Claude row is `npx -y @agentclientprotocol/claude-agent-acp`,
    /// which asks the registry on **every** launch and runs whatever it answers;
    /// an embedder that installed that package itself, at a version it recorded,
    /// runs its executable directly and passes nothing. Overriding rather than
    /// appending, because the row's arguments are how the row fetches — keeping
    /// them would fetch anyway.
    ///
    /// `None` leaves the row's arguments exactly as measured, which is what
    /// every launch did before this field existed. The row's `full_access_args`
    /// and its model pin still apply: those are what to run *with*, not what to
    /// run.
    pub args: Option<Vec<String>>,
    /// Working directory for the agent process. Note this is *not* the
    /// session's `cwd`, which travels in `session/new`.
    pub cwd: Option<PathBuf>,
    /// Environment variables to set on the child, on top of the inherited
    /// environment. Applied before the row's `unset_env`, so a row that clears
    /// a variable wins over an attempt to set it — the row is there because
    /// the agent breaks with it set.
    pub env: Vec<(String, String)>,
    /// Which model the agent should run, when the caller has one to ask for.
    ///
    /// Delivered the way this row says it takes one
    /// ([`AgentLaunchSpec::model_pin`]) and **verbatim**: an id this crate does
    /// not recognise is still the caller's id, and an agent refusing it says
    /// far more than a silent correction would. `None` leaves the agent on its
    /// own default — what every launch did before this field existed. A row
    /// that takes no model at launch cannot use this; ask `model_pin` before
    /// setting it if that difference matters to the caller.
    pub model: Option<String>,
    /// Whether this session answers the agent's permission requests itself, in
    /// which case the row's [`AgentLaunchSpec::full_access_args`] go on the
    /// command line (`SYNC:s-acp-permission-policy`).
    ///
    /// It is a flag rather than an always-on row property because the two must
    /// agree: a session whose owner asked to be consulted would otherwise be
    /// raised with its approvals already turned off, and every card it showed
    /// would be a question about something the agent could do regardless.
    pub full_access: bool,
}

/// Builds the child command for `spec`.
///
/// Pure: it touches nothing outside the returned command, which is what lets a
/// test read back exactly what would have been run.
#[must_use]
pub fn command_for(spec: &AgentLaunchSpec, options: &SpawnOptions) -> std::process::Command {
    let program = options
        .program
        .clone()
        .unwrap_or_else(|| PathBuf::from(spec.program));

    let mut command = std::process::Command::new(program);
    match &options.args {
        Some(given) => command.args(given),
        None => command.args(spec.args),
    };

    // What the session already decided about permissions, said to the agent in
    // its own launch vocabulary. A row with nothing to say here adds nothing.
    if options.full_access {
        command.args(spec.full_access_args);
    }

    // The model rides in on whichever mechanism this agent actually has. Both
    // go on before the row's own `unset_env` below, for the same reason the
    // caller's environment does.
    if let (Some(model), Some(pin)) = (options.model.as_deref(), spec.model_pin) {
        match pin {
            ModelPin::ConfigArg { flag, key } => {
                command.args([flag, &format!("{key}={model}")]);
            }
            ModelPin::Env { name } => {
                command.env(name, model);
            }
        }
    }

    for (name, value) in &options.env {
        command.env(name, value);
    }
    // Last, so a row that must clear a variable cannot be undone by a caller
    // setting it: `CLAUDECODE` present means no session at all.
    for name in spec.unset_env {
        command.env_remove(name);
    }

    if let Some(cwd) = &options.cwd {
        command.current_dir(cwd);
    }

    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command
}

/// A running agent process and the connection to it.
#[derive(Debug)]
pub struct AgentProcess {
    /// Shared with the task that reaps the agent when its connection gives up
    /// on it — see [`reap_on_expiry`]. That task holds it weakly, so this
    /// handle still decides when the process is dropped.
    child: Arc<tokio::sync::Mutex<Child>>,
    connection: Arc<AgentConnection>,
    stderr: Arc<Mutex<VecDeque<String>>>,
}

impl AgentProcess {
    /// The ACP connection to this process.
    ///
    /// Shared rather than borrowed: one agent process holds several sessions,
    /// and a turn in flight on one of them must not stop another session from
    /// being prompted or cancelled. Clone the [`Arc`] into each task that needs
    /// to talk; the process handle stays here for stopping it.
    #[must_use]
    pub fn connection(&self) -> &Arc<AgentConnection> {
        &self.connection
    }

    /// The agent's last few stderr lines.
    ///
    /// Agents explain their refusals here, and the explanation often arrives
    /// well before the protocol-level failure it causes. The stream is always
    /// drained into the tracing log at `debug`, so the pipe cannot fill; this
    /// is the same content, kept for whoever has to report the failure.
    #[must_use]
    pub fn recent_stderr(&self) -> Vec<String> {
        self.stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Kills the agent and reaps it.
    ///
    /// # Errors
    ///
    /// The OS error, if the kill or the wait fails.
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.lock().await.kill().await
    }

    /// Waits for the agent to exit on its own.
    ///
    /// The limit of that: waiting holds the process, and [`reap_on_expiry`]
    /// cannot take it back. If a control request overruns its deadline while
    /// this is waiting, the kill happens once the wait is over — so use it only
    /// where the agent really is expected to leave by itself. To ask whether it
    /// has already gone, use [`AgentProcess::try_wait`], which does not hold on
    /// to anything.
    ///
    /// # Errors
    ///
    /// The OS error, if waiting fails.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.lock().await.wait().await
    }

    /// The agent's exit status if it has already exited, without waiting for
    /// it.
    ///
    /// # Errors
    ///
    /// The OS error, if the status cannot be read.
    pub async fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.lock().await.try_wait()
    }
}

/// Raises `command` and connects to its stdio.
///
/// Control requests are bounded by [`DEFAULT_REQUEST_TIMEOUT`]; use
/// [`spawn_with_request_timeout`] to say otherwise.
///
/// # Errors
///
/// [`Error::Spawn`] when the process could not be started — most often the
/// program is not on `PATH`, which is the case this crate deliberately does not
/// try to fix for the caller.
pub fn spawn<H: ClientHandler>(command: std::process::Command, handler: H) -> Result<AgentProcess> {
    spawn_with_request_timeout(command, handler, DEFAULT_REQUEST_TIMEOUT)
}

/// As [`spawn`], with the control-request deadline given rather than defaulted.
///
/// # Errors
///
/// As [`spawn`].
///
/// # Panics
///
/// Never in practice: the three pipes are taken exactly once, immediately after
/// [`command_for`] configured all three as pipes.
pub fn spawn_with_request_timeout<H: ClientHandler>(
    command: std::process::Command,
    handler: H,
    request_timeout: Duration,
) -> Result<AgentProcess> {
    let program = command.get_program().to_string_lossy().into_owned();

    let mut command = Command::from(command);
    command.kill_on_drop(true);

    let mut child = command.spawn().map_err(|source| Error::Spawn {
        program: program.clone(),
        source,
    })?;

    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        // Only reachable if a caller hands in a command whose stdio is not
        // piped, which `command_for` guarantees it is.
        return Err(Error::Spawn {
            program,
            source: std::io::Error::other("agent process was started without piped stdio"),
        });
    };

    let ring = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING)));
    tokio::spawn(drain_stderr(stderr, Arc::clone(&ring)));

    let connection = Arc::new(AgentConnection::with_request_timeout(
        stdout,
        stdin,
        handler,
        request_timeout,
    ));

    let child = Arc::new(tokio::sync::Mutex::new(child));
    // An agent that overran its deadline is up and not talking, so nothing
    // will ever close its stdout and `kill_on_drop` will not fire until the
    // caller lets go of this handle — which, in the case this defends against,
    // is when the application exits. The process has to be ended here.
    tokio::spawn(reap_on_expiry(connection.expiry(), Arc::downgrade(&child)));

    Ok(AgentProcess {
        child,
        connection,
        stderr: ring,
    })
}

/// Kills the agent once its connection reports a deadline overrun.
///
/// Holds the process weakly and watches a channel the connection owns, so the
/// task ends by itself when either side is dropped instead of keeping a process
/// alive that nobody is talking to any more.
async fn reap_on_expiry(mut expiry: watch::Receiver<bool>, child: Weak<tokio::sync::Mutex<Child>>) {
    // An error means the connection was dropped, which is an ordinary teardown
    // and not an overrun: the caller's own handle reaps the process then.
    while expiry.changed().await.is_ok() {
        if !*expiry.borrow_and_update() {
            continue;
        }
        let Some(child) = child.upgrade() else {
            return;
        };
        if let Err(error) = child.lock().await.kill().await {
            tracing::warn!(%error, "could not stop the agent after its deadline overran");
        }
        return;
    }
}

/// Reads the agent's stderr so the pipe cannot fill, logging each line and
/// keeping the tail for [`AgentProcess::recent_stderr`].
async fn drain_stderr(stderr: tokio::process::ChildStderr, ring: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(line = %line, "agent stderr");
        let mut ring = ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ring.len() == STDERR_RING {
            ring.pop_front();
        }
        ring.push_back(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    fn env_of(command: &std::process::Command) -> Vec<(String, Option<String>)> {
        command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn a_row_becomes_exactly_its_own_command_line() {
        let command = command_for(&registry::GROK, &SpawnOptions::default());
        assert_eq!(command.get_program(), "grok");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["agent", "stdio"]);
    }

    #[test]
    fn the_claude_row_removes_claudecode_from_the_child() {
        let command = command_for(&registry::CLAUDE, &SpawnOptions::default());
        // `None` is how std records a removal, as opposed to an assignment.
        assert!(
            env_of(&command).contains(&("CLAUDECODE".to_owned(), None)),
            "CLAUDECODE must be removed, not merely left alone"
        );
    }

    #[test]
    fn a_caller_cannot_set_a_variable_the_row_has_to_clear() {
        // The order matters: the adapter refuses to start with CLAUDECODE set,
        // so a caller passing it through must not win.
        let options = SpawnOptions {
            env: vec![("CLAUDECODE".to_owned(), "1".to_owned())],
            ..SpawnOptions::default()
        };
        let command = command_for(&registry::CLAUDE, &options);
        let claudecode: Vec<_> = env_of(&command)
            .into_iter()
            .filter(|(name, _)| name == "CLAUDECODE")
            .collect();
        assert_eq!(claudecode, [("CLAUDECODE".to_owned(), None)]);
    }

    #[test]
    fn a_rows_own_environment_still_reaches_the_child() {
        let options = SpawnOptions {
            env: vec![("SYNC_AGENT_SLUG".to_owned(), "rust-impl-b".to_owned())],
            ..SpawnOptions::default()
        };
        let command = command_for(&registry::OPENCODE, &options);
        assert!(env_of(&command)
            .contains(&("SYNC_AGENT_SLUG".to_owned(), Some("rust-impl-b".to_owned()))));
    }

    /// The Codex bridge forwards generic configuration overrides to the
    /// official app-server, exactly as `codex --help` spells them.
    #[test]
    fn codex_takes_the_model_as_a_configuration_override() {
        let options = SpawnOptions {
            model: Some("gpt-5.6-sol".to_owned()),
            ..SpawnOptions::default()
        };
        let command = command_for(&registry::CODEX, &options);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            ["agent-bridge", "codex", "-c", "model=gpt-5.6-sol"],
            "the row's own arguments come first and the override after them",
        );
        // And nowhere else: a model in the environment would be a second,
        // silent channel for the same decision.
        assert!(env_of(&command).is_empty());
    }

    /// The Claude adapter reads its model from the environment before any of
    /// its own sources, so that is where the id goes.
    #[test]
    fn claude_takes_the_model_in_its_own_environment_variable() {
        let options = SpawnOptions {
            model: Some("claude-opus-5".to_owned()),
            ..SpawnOptions::default()
        };
        let command = command_for(&registry::CLAUDE, &options);
        assert!(env_of(&command).contains(&(
            "ANTHROPIC_MODEL".to_owned(),
            Some("claude-opus-5".to_owned()),
        )));
        // The row's arguments are untouched — this agent has no override flag.
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-y", "@agentclientprotocol/claude-agent-acp"]);
    }

    /// Neither of these can be told a model when it is raised, and that is a
    /// measurement, not an omission: inventing a flag for them would produce a
    /// command line the agent rejects.
    #[test]
    fn a_row_that_takes_no_model_at_launch_is_raised_unchanged() {
        for row in [&registry::OPENCODE, &registry::GROK] {
            let plain = command_for(row, &SpawnOptions::default());
            let asked = command_for(
                row,
                &SpawnOptions {
                    model: Some("some-model".to_owned()),
                    ..SpawnOptions::default()
                },
            );
            assert_eq!(
                asked.get_args().collect::<Vec<_>>(),
                plain.get_args().collect::<Vec<_>>(),
                "{}'s arguments must not grow a model",
                row.id,
            );
            assert_eq!(env_of(&asked), env_of(&plain), "{}", row.id);
        }
    }

    /// No model asked for is the case every launch was in before this existed:
    /// the agent stays on its own default.
    #[test]
    fn asking_for_no_model_leaves_the_command_as_the_row_wrote_it() {
        let command = command_for(&registry::CODEX, &SpawnOptions::default());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["agent-bridge", "codex"]);
        assert!(env_of(&command).is_empty());
    }

    /// An id this crate has never heard of still arrives untouched. The agent
    /// refusing it by name is the useful failure; a correction here would hide
    /// which id was actually asked for.
    ///
    /// The fixture is deliberately something any well-meaning normalisation
    /// would change — outer spaces, mixed case, a space inside — because an id
    /// that survives trimming and lower-casing unchanged proves nothing about
    /// whether either happened. Both mechanisms are asserted, so neither can
    /// start tidying up on its own.
    #[test]
    fn the_model_id_travels_verbatim() {
        let asked = "  OpenAI Sol 5.6  ";
        let options = SpawnOptions {
            model: Some(asked.to_owned()),
            ..SpawnOptions::default()
        };

        let command = command_for(&registry::CODEX, &options);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.last(), Some(&format!("model={asked}").as_ref()));

        let claude = command_for(&registry::CLAUDE, &options);
        assert!(env_of(&claude).contains(&("ANTHROPIC_MODEL".to_owned(), Some(asked.to_owned()))));
    }

    #[test]
    fn a_resolved_program_path_replaces_the_bare_name() {
        let options = SpawnOptions {
            program: Some(PathBuf::from("/opt/node22/bin/npx")),
            ..SpawnOptions::default()
        };
        let command = command_for(&registry::CODEX, &options);
        assert_eq!(command.get_program(), "/opt/node22/bin/npx");
        // The bridge arguments are the row's, not the caller's, and stay.
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["agent-bridge", "codex"]);
    }
}
