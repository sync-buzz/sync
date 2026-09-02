#![allow(clippy::expect_used, clippy::unwrap_used)]

//! End to end against **one** resident process, the way Sync actually runs.
//!
//! Every other test in this crate gives its client a process of its own, which
//! is the arrangement a test wants and the one the product no longer has: Sync
//! runs a single `sync-mcp` for the machine and the window connects to it, so a
//! machine with four projects open holds four connections rather than four
//! engines.
//!
//! That path had no coverage at all when it was written. These tests are it,
//! and the thing they are really for is the claim the whole arrangement rests
//! on: **two projects, one process, and neither one's memory leaking into the
//! other's.**
//!
//! The binary is taken from `SYNC_MCP_BINARY` or from where this workspace
//! builds one, exactly as `sidecar_smoke` does. Without one they report why and
//! pass, so a developer who has not built the sidecar does not see a red suite.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sync_memory::{
    ENTITY_KINDS, Entity, EntityKind, LaunchConfig, MemoryClient, Operations as _, type_definitions,
};

fn engine_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SYNC_MCP_BINARY") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let staged = std::fs::read_dir(root.join("binaries"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sync-mcp-"))
        });
    staged
        .into_iter()
        .chain([
            root.join("target/release/sync-mcp"),
            root.join("target/debug/sync-mcp"),
        ])
        .find(|path| path.is_file())
}

/// The one process, and everything that has to outlive it being talked to.
struct Resident {
    child: Child,
    /// The write end of the engine's standard input, held for as long as this
    /// fixture lives — exactly as Sync holds it. Dropping it is how a parent
    /// tells the engine it is finished with it.
    leash: Option<std::process::ChildStdin>,
    socket: PathBuf,
    /// Held because dropping it deletes the socket and the registry.
    home: tempfile::TempDir,
}

impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start one `sync-mcp` serving `projects`, with both doors open.
///
/// The port is asked for by binding one and letting go, because a fixed one
/// would collide with whatever else this machine is running — including another
/// copy of this test.
///
/// **On a leash, as Sync starts it.** The engine ends when the pipe on its
/// standard input closes, so this fixture holds one for as long as the test
/// runs — and a test that panics leaves no engine behind, which is the same
/// property the application relies on.
fn resident(binary: &Path, projects: &[(&str, &Path)]) -> Resident {
    let home = tempfile::tempdir().expect("a directory to serve from");
    let registry = home.path().join("registered-projects.json");
    let listed: Vec<serde_json::Value> = projects
        .iter()
        .map(|(identifier, path)| {
            serde_json::json!({"path": path, "name": identifier, "identifier": identifier})
        })
        .collect();
    std::fs::write(
        &registry,
        serde_json::to_string(&listed).expect("a registry"),
    )
    .expect("the registry is written");

    let address = {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        taken.local_addr().expect("its address")
    };
    let socket = home.path().join("host.sock");
    let child = Command::new(binary)
        .arg("--registry")
        .arg(&registry)
        .arg("--http")
        .arg(address.to_string())
        .arg("--socket")
        .arg(&socket)
        .arg("--exit-when-orphaned")
        .env("SYNC_MCP_TOKEN", "resident-channel-test")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the resident process starts");

    let mut child = child;
    let leash = child.stdin.take();
    let held = Resident {
        child,
        leash,
        socket,
        home,
    };
    held.wait_until_serving();
    held
}

impl Resident {
    fn wait_until_serving(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(&self.socket).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the host channel never opened on {}", self.socket.display());
    }

    /// A client reaching this process about one project, the way a window does.
    fn client(&self, project: &Path, binary: &Path, log_file: PathBuf) -> MemoryClient {
        MemoryClient::connect(LaunchConfig {
            project: project.to_path_buf(),
            bundled: binary.to_path_buf(),
            log_file,
            override_binary: None,
            host_socket: Some(self.socket.clone()),
        })
        .expect("the client reaches the resident process and handshakes")
    }
}

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temp project");
    let status = Command::new("git")
        .arg("init")
        .arg(directory.path())
        .output()
        .expect("git init");
    assert!(status.status.success(), "the fixture needs a repository");
    directory
}

/// One record, in the shape the strict schema takes.
///
/// The same shape `sidecar_smoke` writes, and deliberately: these tests are
/// about which process answers, not about what a kind requires, and a fixture
/// that argued with the schema would fail for a reason that has nothing to do
/// with what is being tested.
fn note(key: &str, title: &str) -> Entity {
    let mut extensions = serde_json::Map::new();
    extensions.insert("status".to_owned(), serde_json::json!("todo"));
    Entity {
        key: key.to_owned(),
        kind: EntityKind::Spec.as_str().to_owned(),
        title: title.to_owned(),
        content: format!("{title}.\n"),
        tags: Vec::new(),
        links: Vec::new(),
        paths_observed: Vec::new(),
        scope_paths: vec!["src/".to_owned()],
        extensions,
        folder: None,
        is_folder: false,
        archived: false,
        verified: false,
    }
}

/// The strict schema refuses a kind with no `__type__` record, so a project
/// publishes its types before its first record — exactly as Sync does on open.
fn seeded(client: &mut MemoryClient) {
    client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
}

/// How many `sync-mcp` processes are this test's own.
///
/// By the socket it was started with rather than by name: the developer running
/// this has Sync open, and counting every `sync-mcp` on the machine would count
/// theirs.
fn engines_serving(socket: &Path) -> usize {
    let listed = Command::new("ps")
        .args(["-Ao", "args="])
        .output()
        .expect("ps runs");
    BufReader::new(listed.stdout.as_slice())
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.contains("sync-mcp") && line.contains(&*socket.to_string_lossy()))
        .count()
}

/// The claim the whole arrangement rests on. Two projects, two clients, and
/// **one** process — where before there would have been two engines, each with
/// its own copy of the model.
#[test]
fn two_projects_are_two_connections_and_one_process() {
    let Some(binary) = engine_binary() else {
        eprintln!("no sync-mcp binary; skipping");
        return;
    };
    let logs = tempfile::tempdir().expect("temp logs");
    let one = repository();
    let two = repository();
    let held = resident(&binary, &[("ONE", one.path()), ("TWO", two.path())]);

    let mut first = held.client(one.path(), &binary, logs.path().join("one.log"));
    let mut second = held.client(two.path(), &binary, logs.path().join("two.log"));

    assert_eq!(
        engines_serving(&held.socket),
        1,
        "two projects were open and the machine was running more than one engine"
    );
    assert_eq!(
        first.engine_pid(),
        None,
        "a client that did not start the engine must not hand anybody its pid"
    );
    assert!(first.engine_is_alive() && second.engine_is_alive());
}

/// A connection names its project once, and everything after it is that
/// project's. Two connections to one process must not be able to read each
/// other's corpus — the failure this would be is somebody's decision showing up
/// in a colleague's repository.
#[test]
fn one_process_keeps_two_projects_apart() {
    let Some(binary) = engine_binary() else {
        eprintln!("no sync-mcp binary; skipping");
        return;
    };
    let logs = tempfile::tempdir().expect("temp logs");
    let one = repository();
    let two = repository();
    let held = resident(&binary, &[("ONE", one.path()), ("TWO", two.path())]);

    let mut first = held.client(one.path(), &binary, logs.path().join("one.log"));
    let mut second = held.client(two.path(), &binary, logs.path().join("two.log"));

    seeded(&mut first);
    seeded(&mut second);
    first
        .apply(
            "one",
            &[note("s-only-in-one", "Only the first project knows this").to_put()],
        )
        .expect("the first project takes a write");

    // `Ok` with nothing in it, not `Err`: a key that is not there is an answer
    // rather than a failure, so the assertion is about the record and not about
    // the call.
    let here = first
        .get_record("s-only-in-one")
        .expect("the project that was written to answers");
    let there = second
        .get_record("s-only-in-one")
        .expect("the other project answers");
    assert!(
        here.record.is_some(),
        "the project that was written to cannot read its own record"
    );
    assert!(
        there.record.is_none(),
        "a record written in one project was readable from another on the same process"
    );
    // The sharper claim, and the one that could not be true by accident: the
    // two connections are looking at two different memories, each with its own
    // history.
    assert_ne!(
        here.revision, there.revision,
        "both projects answered from one revision, so they are one corpus"
    );

    second
        .apply(
            "two",
            &[note("s-only-in-two", "Only the second project knows this").to_put()],
        )
        .expect("the second project takes a write");
    assert!(
        first
            .get_record("s-only-in-two")
            .expect("the first project answers")
            .record
            .is_none(),
        "the second project's write reached the first"
    );
}

/// The window is not the only caller: a connection that has not said which
/// project it is about must be told so rather than answered from a guess. There
/// is no default project, and inventing one would answer a call meant for one
/// repository out of another.
#[test]
fn a_connection_that_named_no_project_is_refused() {
    use std::io::Write as _;

    let Some(binary) = engine_binary() else {
        eprintln!("no sync-mcp binary; skipping");
        return;
    };
    let one = repository();
    let held = resident(&binary, &[("ONE", one.path())]);

    let mut stream =
        std::os::unix::net::UnixStream::connect(&held.socket).expect("the door is there");
    stream
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"types.list\",\"params\":{}}\n")
        .expect("the request is sent");
    let mut answer = String::new();
    BufReader::new(&stream)
        .read_line(&mut answer)
        .expect("an answer");

    let answer: serde_json::Value = serde_json::from_str(&answer).expect("readable JSON");
    let said = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(
        said.contains("project.attach"),
        "the refusal has to say what to do about it: {said}"
    );
}

/// **The rule the whole arrangement is held to: the engine lives exactly as
/// long as the application.**
///
/// Not by being killed — anything can fail to kill anything — but by holding a
/// pipe that the operating system closes when its holder ends, however it ends.
/// This is what a crash, a `kill -9` and a development reload all look like from
/// the engine's side, and each of them left one running before this existed.
///
/// Why it must: the engine holds a port, a socket, an open repository per
/// project and a loaded model, and none of that is somebody's machine to spend
/// on an application they have closed.
#[test]
fn the_engine_ends_when_whatever_started_it_ends() {
    let Some(binary) = engine_binary() else {
        eprintln!("no sync-mcp binary; skipping");
        return;
    };
    let one = repository();
    let mut held = resident(&binary, &[("ONE", one.path())]);
    assert!(
        held.child
            .try_wait()
            .expect("the engine can be waited on")
            .is_none(),
        "the engine was not running to begin with"
    );

    // Nothing is killed. The pipe is simply let go, which is what the operating
    // system does for a parent that is no longer there.
    held.leash.take();

    let deadline = Instant::now() + Duration::from_secs(10);
    let ended = loop {
        if held
            .child
            .try_wait()
            .expect("the engine can be waited on")
            .is_some()
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        ended,
        "the engine outlived the process holding its pipe, which is how a machine collects orphans"
    );
    assert!(
        std::os::unix::net::UnixStream::connect(&held.socket).is_err(),
        "the door was still answering after the engine ended"
    );
}

/// A window outlives the process it talks to — an update replaces the binary, a
/// crash takes it down — and what a person must not have to do about it is
/// anything. The next call reconnects, re-attaches and carries on.
#[test]
fn a_client_reconnects_when_the_resident_process_is_replaced() {
    let Some(binary) = engine_binary() else {
        eprintln!("no sync-mcp binary; skipping");
        return;
    };
    let logs = tempfile::tempdir().expect("temp logs");
    let one = repository();
    let mut held = resident(&binary, &[("ONE", one.path())]);
    let mut client = held.client(one.path(), &binary, logs.path().join("one.log"));

    seeded(&mut client);
    client
        .apply(
            "before",
            &[note("s-before-the-crash", "Written before the process died").to_put()],
        )
        .expect("the first write lands");

    // Killed the way a crash would, and started again on the same socket, which
    // is what an update does.
    let _ = held.child.kill();
    let _ = held.child.wait();
    let restarted = Command::new(&binary)
        .arg("--registry")
        .arg(held.home.path().join("registered-projects.json"))
        .arg("--http")
        .arg({
            let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
            taken.local_addr().expect("its address").to_string()
        })
        .arg("--socket")
        .arg(&held.socket)
        .arg("--exit-when-orphaned")
        .env("SYNC_MCP_TOKEN", "resident-channel-test")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the replacement starts");
    let mut restarted = restarted;
    held.leash = restarted.stdin.take();
    held.child = restarted;
    held.wait_until_serving();

    // No user action, no lost write, and the connection is about the same
    // project it was about before — which only holds because the reconnect
    // says `project.attach` again.
    let view = client
        .get_record("s-before-the-crash")
        .expect("the client recovered on its own");
    assert!(
        view.record.is_some(),
        "the record written before the process died came back empty"
    );
}
