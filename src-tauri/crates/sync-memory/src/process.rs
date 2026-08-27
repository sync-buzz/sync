//! Finding, starting and supervising the `sync-mcp` sidecar.
//!
//! The engine is inside it now, so what is supervised is Sync's own binary:
//! a crash in ggml or `LanceDB` still takes down a process rather than the
//! window, which is the whole reason the boundary is here.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value;

use crate::error::{MemoryError, Result};
use crate::protocol::Transport;

/// Where the engine binary came from.
///
/// Worth reporting: "you are running the copy that shipped" and "you are
/// running one somebody pointed this window at" behave the same and are built
/// by different people, and a support conversation starts with knowing which
/// is which.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinarySource {
    /// A binary named deliberately, by the override — see
    /// [`LaunchConfig::override_binary`].
    Override,
    /// The copy bundled inside the application.
    Bundled,
}

/// A resolved engine binary.
#[derive(Clone, Debug)]
pub struct EngineBinary {
    pub path: PathBuf,
    pub source: BinarySource,
    pub version: String,
}

/// How to find and launch the engine.
pub struct LaunchConfig {
    /// The repository whose memory this session serves.
    pub project: PathBuf,
    /// The bundled sidecar, as resolved by the host application.
    pub bundled: PathBuf,
    /// Where the engine's stderr goes. It carries ggml/llama noise and panics;
    /// it is not part of the protocol and must never be shown as-is.
    pub log_file: PathBuf,
    /// A sidecar to run instead of the bundled one, named deliberately.
    ///
    /// **Named, never searched for**, and that is a reversal worth stating. A
    /// binary on `PATH` used to win by default, because the sidecar was then
    /// `memory-hub`: an engine somebody installs on purpose, which keeps a
    /// registry of its consumers, and shadowing a newer one they chose would
    /// have been Sync overruling them on their own machine.
    ///
    /// `sync-mcp` is none of those things. It is Sync's own binary with the
    /// engine linked inside it, and nothing is left to catch a mismatch — no
    /// registry, no version handshake, only the method list, which an older
    /// build with the same operation names passes. A stray copy on `PATH` from
    /// a `cargo install` somebody ran once would have silently served a
    /// different engine than the one this window shipped with, and nothing
    /// would have said so.
    pub override_binary: Option<PathBuf>,
    /// The resident process's host channel, when this installation runs one.
    ///
    /// Present, this client connects to it and names its project. Absent, it
    /// starts a process of its own — which is what a test does, and what the
    /// single-project door has always done.
    ///
    /// A path that is set and unreachable is an error rather than a fall back
    /// to a private process: falling back would put the machine silently back
    /// to one engine per project, and nothing would say so.
    pub host_socket: Option<PathBuf>,
}

/// Resolve which binary to run.
///
/// The bundled copy, unless an override names another. Whether the one chosen
/// answers what this window calls is settled at the greeting, by the method
/// list — the only check left now that the engine is linked into the sidecar
/// rather than installed alongside it.
///
/// The override is an error when it cannot run, never a quiet fall back to the
/// bundled copy: somebody who names a binary is testing *that* binary, and
/// running a different one instead would answer a question they did not ask.
///
/// # Errors
///
/// Returns [`MemoryError::Sidecar`] when the chosen binary cannot be run.
pub fn resolve_binary(config: &LaunchConfig) -> Result<EngineBinary> {
    let (path, source, whose) = match config.override_binary.as_ref() {
        Some(path) => (path.clone(), BinarySource::Override, "the memory engine"),
        None => (
            config.bundled.clone(),
            BinarySource::Bundled,
            "the bundled memory engine",
        ),
    };
    let version = probe_version(&path).ok_or_else(|| {
        MemoryError::Sidecar(format!("{whose} at {} could not be run", path.display()))
    })?;
    Ok(EngineBinary {
        path,
        source,
        version,
    })
}

/// A running sidecar and its stdio.
pub struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Sidecar {
    /// Start `sync-mcp --host` for one project.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Sidecar`] when the process cannot be spawned or
    /// does not expose the stdio the protocol needs.
    pub fn spawn(binary: &Path, config: &LaunchConfig) -> Result<Self> {
        let log = open_log(&config.log_file)?;
        let mut child = Command::new(binary)
            .arg("--host")
            .arg(&config.project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|error| {
                MemoryError::Sidecar(format!(
                    "could not start the memory engine at {}: {error}",
                    binary.display()
                ))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| MemoryError::Sidecar("the engine exposed no stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MemoryError::Sidecar("the engine exposed no stdout".to_owned()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Whether the process is still running.
    ///
    /// A dead engine and a busy one look identical from a blocked read, so the
    /// distinction comes from here rather than from a timeout.
    ///
    /// # Errors
    ///
    /// Returns the failure from waiting on the child process.
    pub fn is_alive(&mut self) -> io::Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }

    /// The engine's process id.
    ///
    /// Exposed so a caller can act on this exact process — a test killing
    /// "every sync-mcp" would take out the engines of every other test
    /// running beside it.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// How the engine exited, when it has.
    ///
    /// Included in the restart message so a supervision log says "exited with
    /// status 101" rather than only "the stream ended".
    pub fn exit_status(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl Transport for Sidecar {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.stdin, message)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()
    }

    fn receive(&mut self) -> io::Result<Option<Value>> {
        read_message(&mut self.stdout)
    }
}

/// A connection to the process that serves this whole machine.
///
/// Not a process this client owns — that is the difference the whole
/// arrangement turns on. Sync runs one `sync-mcp`, and a window with four
/// projects open holds four of *these* against it rather than four engines.
/// Dropping one closes a file descriptor and nothing else.
pub struct Resident {
    /// Kept so the connection can be made again after it breaks, and so that
    /// "is anything serving" can be asked without writing on this one.
    path: PathBuf,
    writing: UnixStream,
    reading: BufReader<UnixStream>,
}

impl Resident {
    /// Open a connection to the host channel at `path`.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Sidecar`] when nothing is listening — which is the
    /// resident process being down, and is the caller's to answer by starting
    /// it. Deliberately **not** a fall back to a process of this client's own:
    /// that would put the machine quietly back to one engine per project, which
    /// is the arrangement this exists to end.
    pub fn connect(path: &Path) -> Result<Self> {
        let writing = reach(path)?;
        let reading = writing.try_clone().map_err(|error| {
            MemoryError::Sidecar(format!("the connection could not be read from: {error}"))
        })?;
        Ok(Self {
            path: path.to_owned(),
            writing,
            reading: BufReader::new(reading),
        })
    }

    /// Whether anything is still serving on this socket.
    ///
    /// One attempt, unlike [`reach`]: this asks about now, not about a process
    /// that might be on its way up.
    ///
    /// Asked by connecting rather than by writing on the connection in hand: a
    /// caller asks this *because* something looks wrong, and a probe that
    /// disturbed the channel it is reporting on would be its own answer.
    fn is_serving(&self) -> bool {
        UnixStream::connect(&self.path).is_ok()
    }
}

/// How long to keep trying to reach the resident process.
///
/// Not politeness: the application starts the process and opens a window in the
/// same breath, so the first project can be opened before the socket is bound.
/// Failing there would make a launch race decide whether somebody's memory
/// works, which is the kind of defect that reproduces on one machine in ten.
const REACH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
const REACH_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// Connect, allowing for a process that is still starting.
fn reach(path: &Path) -> Result<UnixStream> {
    let deadline = std::time::Instant::now() + REACH_DEADLINE;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if std::time::Instant::now() >= deadline => {
                return Err(MemoryError::Sidecar(format!(
                    "nothing is serving memory on {} after {} seconds: {error}",
                    path.display(),
                    REACH_DEADLINE.as_secs()
                )));
            }
            Err(_) => std::thread::sleep(REACH_PAUSE),
        }
    }
}

impl Transport for Resident {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writing, message)?;
        self.writing.write_all(b"\n")?;
        self.writing.flush()
    }

    fn receive(&mut self) -> io::Result<Option<Value>> {
        read_message(&mut self.reading)
    }
}

/// How a client reaches the engine.
///
/// Two, and they are not interchangeable arrangements of one thing: one is a
/// process this client started and is responsible for, the other is a
/// connection to the process this machine runs. Everything above this is
/// written against [`Transport`] and does not know which it has.
pub enum Channel {
    /// A process of this client's own. What a test drives, and what a machine
    /// with no resident process falls back to when it is asked to.
    Own(Sidecar),
    /// A connection to the process that serves the machine.
    Resident(Resident),
}

impl Channel {
    /// The engine's process id, when this client is the one that started it.
    ///
    /// `None` for the resident process. It is not this client's process, and
    /// the one caller for this — a test simulating a crash — must not be handed
    /// a pid it would then kill out from under every other project on the
    /// machine.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Own(sidecar) => Some(sidecar.pid()),
            Self::Resident(_) => None,
        }
    }

    /// Whether there is still an engine to talk to.
    ///
    /// # Errors
    ///
    /// Returns the failure from waiting on a child process.
    pub fn is_alive(&mut self) -> io::Result<bool> {
        match self {
            Self::Own(sidecar) => sidecar.is_alive(),
            Self::Resident(resident) => Ok(resident.is_serving()),
        }
    }

    /// How the engine exited, when this client owned it and it has.
    pub fn exit_status(&mut self) -> Option<std::process::ExitStatus> {
        match self {
            Self::Own(sidecar) => sidecar.exit_status(),
            // A connection that broke says nothing about how, and inventing a
            // status would be putting a number on a sentence that does not have
            // one.
            Self::Resident(_) => None,
        }
    }
}

impl Transport for Channel {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        match self {
            Self::Own(sidecar) => sidecar.send(message),
            Self::Resident(resident) => resident.send(message),
        }
    }

    fn receive(&mut self) -> io::Result<Option<Value>> {
        match self {
            Self::Own(sidecar) => sidecar.receive(),
            Self::Resident(resident) => resident.receive(),
        }
    }
}

/// One JSON message off a line-delimited stream, skipping blank lines.
///
/// Shared by both channels because it is the same framing: the resident door
/// answers exactly what the single-project door answers, and a second copy of
/// this loop is a second place for the two to drift apart.
fn read_message(reading: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reading.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.trim().is_empty() {
            continue;
        }
        return serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // Closing stdin is how the protocol says "we are done"; the engine
        // exits on EOF. Killing is the fallback for one that does not.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn open_log(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

/// Ask a binary for its version, which doubles as "can this actually run here".
fn probe_version(binary: &Path) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_missing_bundled_binary_is_an_actionable_failure_not_a_panic() {
        let directory = tempfile::tempdir().unwrap();
        let config = LaunchConfig {
            project: directory.path().to_path_buf(),
            bundled: directory.path().join("does-not-exist"),
            log_file: directory.path().join("logs/memory.log"),
            override_binary: None,
            host_socket: None,
        };

        let error = resolve_binary(&config).unwrap_err();

        assert!(matches!(error, MemoryError::Sidecar(_)));
    }

    /// A stand-in for a sidecar: all the resolver asks of a binary is that it
    /// runs and reports a version.
    fn fake_engine(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "#!/bin/sh\necho 'sync-mcp 9.9.9'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn an_override_is_run_instead_of_the_bundled_copy() {
        let directory = tempfile::tempdir().unwrap();
        let named = directory.path().join("named/sync-mcp");
        let bundled = directory.path().join("bundled/sync-mcp");
        fake_engine(&named);
        fake_engine(&bundled);

        let resolved = resolve_binary(&LaunchConfig {
            project: directory.path().to_path_buf(),
            bundled: bundled.clone(),
            log_file: directory.path().join("memory.log"),
            override_binary: Some(named.clone()),
            host_socket: None,
        })
        .unwrap();

        assert_eq!(resolved.source, BinarySource::Override);
        assert_eq!(resolved.path, named);
    }

    #[test]
    fn an_override_that_cannot_run_is_a_failure_rather_than_the_bundled_copy() {
        let directory = tempfile::tempdir().unwrap();
        let bundled = directory.path().join("bundled/sync-mcp");
        fake_engine(&bundled);

        let error = resolve_binary(&LaunchConfig {
            project: directory.path().to_path_buf(),
            bundled,
            log_file: directory.path().join("memory.log"),
            override_binary: Some(directory.path().join("named/does-not-exist")),
            host_socket: None,
        })
        .unwrap_err();

        assert!(
            matches!(error, MemoryError::Sidecar(_)),
            "a named binary that cannot run is said out loud, not silently replaced"
        );
    }

    #[test]
    fn the_bundled_copy_is_used_when_nothing_overrides_it() {
        let directory = tempfile::tempdir().unwrap();
        let bundled = directory.path().join("bundled/sync-mcp");
        fake_engine(&bundled);

        let resolved = resolve_binary(&LaunchConfig {
            project: directory.path().to_path_buf(),
            bundled: bundled.clone(),
            log_file: directory.path().join("memory.log"),
            override_binary: None,
            host_socket: None,
        })
        .unwrap();

        assert_eq!(resolved.source, BinarySource::Bundled);
        assert_eq!(resolved.path, bundled);
    }

    #[test]
    fn the_log_file_and_its_directory_are_created_on_demand() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("nested/logs/memory.log");

        open_log(&log).unwrap();

        assert!(log.is_file(), "stderr has somewhere to go from the start");
    }
}
