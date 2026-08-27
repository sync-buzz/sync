#![allow(clippy::expect_used, clippy::unwrap_used)]

//! What stands in the HTTP door, tested from outside it.
//!
//! Raw requests over a socket rather than through a client library, because the
//! checks are about headers a well-behaved client would never send: an `Origin`
//! only a browser sets, a `Host` naming somebody else's domain, a token that is
//! nearly right. A client that made those requests hard to send would be
//! testing the client.
//!
//! The registry is empty on purpose. Nothing here depends on a project — the
//! door is answered before any project is reached, and giving it one would make
//! this test wait for an engine to prove something about a header.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_sync-mcp");
const TOKEN: &str = "a-token-this-test-invented";

/// A server on a port of its own, killed when the test ends.
struct Door {
    child: Child,
    address: SocketAddr,
    _registry: tempfile::TempDir,
}

impl Door {
    fn open() -> Self {
        let registry = tempfile::tempdir().expect("a temporary directory");
        let file = registry.path().join("registered-projects.json");
        std::fs::write(&file, "[]").expect("an empty registry");

        // Asked for by binding one and letting go: a fixed port would collide
        // with whatever else this machine is running, including another copy of
        // this test.
        let address = {
            let taken = TcpListener::bind("127.0.0.1:0").expect("a free port");
            taken.local_addr().expect("its address")
        };

        let child = Command::new(BINARY)
            .arg("--registry")
            .arg(&file)
            .arg("--http")
            .arg(address.to_string())
            .env("SYNC_MCP_TOKEN", TOKEN)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server starts");

        let door = Self {
            child,
            address,
            _registry: registry,
        };
        door.wait_until_listening();
        door
    }

    fn wait_until_listening(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(&self.address, Duration::from_millis(200)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the server never started listening on {}", self.address);
    }

    /// Send one request and answer with its status line.
    fn knock(&self, headers: &[(&str, String)], body: &str) -> String {
        let mut stream =
            TcpStream::connect(self.address).expect("the door is there to be knocked on");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("a deadline");
        let mut request = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n", body.len());
        for (name, value) in headers {
            let _ = writeln!(request, "{name}: {value}\r");
        }
        request.push_str("Connection: close\r\n\r\n");
        request.push_str(body);
        stream
            .write_all(request.as_bytes())
            .expect("the request is sent");

        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    }
}

impl Drop for Door {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn initialize() -> String {
    initialize_offering("2025-06-18")
}

/// An `initialize` from a client that speaks `version`.
fn initialize_offering(version: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"},
        },
    })
    .to_string()
}

fn ordinary(door: &Door) -> Vec<(&'static str, String)> {
    vec![
        ("Host", door.address.to_string()),
        ("Content-Type", "application/json".to_owned()),
        ("Accept", "application/json, text/event-stream".to_owned()),
    ]
}

#[test]
fn a_client_with_the_token_is_served() {
    let door = Door::open();
    let mut headers = ordinary(&door);
    headers.push(("Authorization", format!("Bearer {TOKEN}")));
    let answer = door.knock(&headers, &initialize());
    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "the door opens for the token it was started with: {answer}"
    );
    assert!(
        answer.contains("protocolVersion"),
        "and what is behind it is the same server: {answer}"
    );
}

/// A revision this server cannot serve is negotiated down, never echoed.
///
/// `rmcp` will agree to any revision it has a name for, and its names run ahead
/// of what it can answer: `2026-07-28` wants a cache hint on every `tools/list`
/// that the SDK has no field for. Agreeing costs nothing at the handshake and
/// everything after it — the client reads a list whose shape its own revision
/// forbids, distrusts the lot, and attaches a server with no tools on it. No
/// status says so, so this is the only place it can be seen.
#[test]
fn a_revision_this_server_cannot_serve_is_not_agreed_to() {
    let door = Door::open();
    let mut headers = ordinary(&door);
    headers.push(("Authorization", format!("Bearer {TOKEN}")));
    let answer = door.knock(&headers, &initialize_offering("2026-07-28"));
    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "the offer itself is fine, it is the answer that matters: {answer}"
    );
    assert!(
        !answer.contains("2026-07-28"),
        "a revision it cannot serve must not be promised back: {answer}"
    );
    assert!(
        answer.contains("2025-11-25"),
        "it falls back to the newest revision it can serve: {answer}"
    );
}

/// And a revision it can serve is still answered with that revision.
#[test]
fn a_revision_this_server_can_serve_is_agreed_to() {
    let door = Door::open();
    let mut headers = ordinary(&door);
    headers.push(("Authorization", format!("Bearer {TOKEN}")));
    for offered in ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"] {
        let answer = door.knock(&headers, &initialize_offering(offered));
        assert!(
            answer.contains(offered),
            "`{offered}` is on the list this server publishes: {answer}"
        );
    }
}

#[test]
fn a_request_without_the_token_is_refused() {
    let door = Door::open();
    let answer = door.knock(&ordinary(&door), &initialize());
    assert!(
        answer.starts_with("HTTP/1.1 401"),
        "no token, no answer: {answer}"
    );

    let mut nearly = ordinary(&door);
    nearly.push(("Authorization", format!("Bearer {TOKEN}x")));
    let answer = door.knock(&nearly, &initialize());
    assert!(
        answer.starts_with("HTTP/1.1 401"),
        "and nearly right is wrong: {answer}"
    );
}

#[test]
fn a_page_in_a_browser_is_refused_even_holding_the_token() {
    let door = Door::open();
    let mut from_a_page = ordinary(&door);
    from_a_page.push(("Authorization", format!("Bearer {TOKEN}")));
    from_a_page.push(("Origin", "https://example.com".to_owned()));
    let answer = door.knock(&from_a_page, &initialize());
    assert!(
        answer.starts_with("HTTP/1.1 403"),
        "a page that learned the token is still a page: {answer}"
    );
}

#[test]
fn a_name_resolved_to_this_machine_is_refused() {
    let door = Door::open();
    let mut rebound = ordinary(&door);
    rebound[0] = ("Host", format!("attacker.example:{}", door.address.port()));
    rebound.push(("Authorization", format!("Bearer {TOKEN}")));
    let answer = door.knock(&rebound, &initialize());
    assert!(
        answer.starts_with("HTTP/1.1 403"),
        "the socket says this machine, the Host says otherwise: {answer}"
    );
}

#[test]
fn a_door_onto_the_network_does_not_open_at_all() {
    let registry = tempfile::tempdir().expect("a temporary directory");
    let file = registry.path().join("registered-projects.json");
    std::fs::write(&file, "[]").expect("an empty registry");

    let refused = Command::new(BINARY)
        .arg("--registry")
        .arg(&file)
        .arg("--http")
        .arg("0.0.0.0:41847")
        .env("SYNC_MCP_TOKEN", TOKEN)
        .output()
        .expect("the server runs");
    assert!(
        !refused.status.success(),
        "an address off the loopback is refused rather than narrowed"
    );

    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("loopback"),
        "and it says why, because the alternative is publishing every project: {said}"
    );
}

#[test]
fn a_door_without_a_token_does_not_open_at_all() {
    let registry = tempfile::tempdir().expect("a temporary directory");
    let file = registry.path().join("registered-projects.json");
    std::fs::write(&file, "[]").expect("an empty registry");

    let refused = Command::new(BINARY)
        .arg("--registry")
        .arg(&file)
        .arg("--http")
        .arg("127.0.0.1:0")
        .env_remove("SYNC_MCP_TOKEN")
        .output()
        .expect("the server runs");
    assert!(
        !refused.status.success(),
        "a server with no token would be every project on a port"
    );
}
