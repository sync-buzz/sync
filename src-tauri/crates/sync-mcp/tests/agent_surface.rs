#![allow(clippy::expect_used, clippy::unwrap_used)]

//! What an agent sees, driven as an agent drives it.
//!
//! A hand-written MCP session over the binary's stdin, because that is the only
//! thing that proves the surface: the unit an agent talks to is a process, and
//! a test that called the functions directly would prove they compose rather
//! than that they are published.
//!
//! The project is set up through the *window's* channel — `MemoryClient`, the
//! same client the application uses — because giving a repository memory is the
//! window's gesture and not the agent's. That asymmetry is the thing being
//! tested as much as any assertion below.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_sync-mcp");

/// An MCP session over one sidecar process.
struct Agent {
    child: Child,
    next_id: u64,
}

impl Agent {
    fn open(project: &Path) -> Self {
        let child = Command::new(BINARY)
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the sidecar starts");
        let mut agent = Self { child, next_id: 1 };
        agent.request(
            "initialize",
            &json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"},
            }),
        );
        agent.notify("notifications/initialized");
        agent
    }

    /// Open a server over every project a registry names.
    ///
    /// The door the window will use once it holds the server itself: one
    /// process, several projects, one model between them.
    fn over(registry: &Path) -> Self {
        let child = Command::new(BINARY)
            .arg("--registry")
            .arg(registry)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the sidecar starts");
        let mut agent = Self { child, next_id: 1 };
        agent.request(
            "initialize",
            &json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"},
            }),
        );
        agent.notify("notifications/initialized");
        agent
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let stdin = self.child.stdin.as_mut().expect("the sidecar takes input");
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .expect("the request is written");
        stdin.flush().expect("the request is sent");

        let stdout = self.child.stdout.as_mut().expect("the sidecar answers");
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).expect("the answer is readable");
            assert!(read > 0, "the sidecar closed while answering `{method}`");
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return message;
            }
        }
    }

    fn notify(&mut self, method: &str) {
        let stdin = self.child.stdin.as_mut().expect("the sidecar takes input");
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "method": method}))
            .expect("the notification is written");
        stdin.flush().expect("the notification is sent");
    }

    /// Call a tool and read its structured answer.
    fn call(&mut self, name: &str, arguments: &Value) -> Value {
        let answer = self.request("tools/call", &json!({"name": name, "arguments": arguments}));
        answer
            .get("result")
            .and_then(|result| result.get("structuredContent"))
            .cloned()
            .unwrap_or_else(|| panic!("`{name}` answered without structured content: {answer}"))
    }

    fn tools(&mut self) -> Vec<Value> {
        self.request("tools/list", &json!({"project": "PROBE"}))["result"]["tools"]
            .as_array()
            .expect("a list of tools")
            .clone()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A repository with no Sync memory in it.
fn repository() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("a temporary project");
    let status = Command::new("git")
        .arg("init")
        .arg(project.path())
        .output()
        .expect("git init");
    assert!(status.status.success(), "the fixture needs a repository");
    project
}

/// Give the repository memory, a project record and one type — through the
/// window's channel, which is the only thing that creates memory.
fn opened_as(
    project: &Path,
    name: &str,
    identifier: &str,
) -> (sync_memory::MemoryClient, tempfile::TempDir) {
    let logs = tempfile::tempdir().expect("a temporary log directory");
    let mut client = sync_memory::MemoryClient::connect(sync_memory::LaunchConfig {
        project: project.to_path_buf(),
        bundled: PathBuf::from(BINARY),
        log_file: logs.path().join("memory.log"),
        override_binary: None,
        host_socket: None,
    })
    .expect("the window opens the project");
    client
        .update_project(&sync_memory::ProjectSettings {
            name: name.to_owned(),
            identifier: identifier.to_owned(),
            description: String::new(),
            language: "en".to_owned(),
            installed: Vec::new(),
        })
        .expect("the project describes itself");
    client
        .create_type("decision", "Decision", "Decisions taken", "scale")
        .expect("a type is published");
    (client, logs)
}

fn opened_in_sync(project: &Path) -> (sync_memory::MemoryClient, tempfile::TempDir) {
    let logs = tempfile::tempdir().expect("a temporary log directory");
    let mut client = sync_memory::MemoryClient::connect(sync_memory::LaunchConfig {
        project: project.to_path_buf(),
        bundled: PathBuf::from(BINARY),
        log_file: logs.path().join("memory.log"),
        override_binary: None,
        host_socket: None,
    })
    .expect("the window opens the project");
    client
        .update_project(&sync_memory::ProjectSettings {
            name: "Probe".to_owned(),
            identifier: "PROBE".to_owned(),
            description: "A project to look at".to_owned(),
            language: "en".to_owned(),
            installed: vec![sync_memory::InstalledExtension {
                id: "acme.tracker".to_owned(),
                version: "1.2.0".to_owned(),
                prompt: Some(EXTENSION_PROMPT.to_owned()),
                // How the package was resolved — the bytes it was pinned to
                // and where they came from — is the window's half of the
                // record and reaches no agent. Left unsaid rather than filled
                // in with a hash nothing on this surface checks.
                integrity: None,
                source: None,
                tools: vec![sync_memory::ToolDeclaration {
                    name: "search_tickets".to_owned(),
                    description: "Finds tickets by their words".to_owned(),
                    input: json!({
                        "type": "object",
                        "properties": {"words": {"type": "string"}},
                    }),
                }],
            }],
        })
        .expect("the project describes itself");
    client
        .create_type("decision", "Decision", "Decisions taken", "scale")
        .expect("a type is published");
    // A kind Sync has never heard of, which is the ordinary case: the corpus
    // belongs to the project, and `EntityKind` is only what Sync ships.
    client
        .create_type("ticket", "Ticket", "Work coming in", "ticket")
        .expect("a type of the project's own is published");
    // And a kind the package brought, which is the ordinary shape of one: the
    // id, a dot, and a word the package chose. The prefix is the whole of what
    // joins a kind to the instructions describing it.
    client
        .create_type(
            "acme.tracker.watch",
            "Watch",
            "Something to keep an eye on",
            "eye",
        )
        .expect("a type an extension published");
    // The log directory is handed back with the client: dropping it would take
    // the engine's log file out from under a session that is still running.
    (client, logs)
}

#[test]
fn the_surface_is_the_curated_one_and_the_engine_s_administration_is_not_on_it() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let tools = agent.tools();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();

    for ours in ["sync_project", "sync_instructions", "sync_apply"] {
        assert!(names.contains(&ours), "`{ours}` is published: {names:?}");
    }
    // The refusal that matters. An agent that could move a storage is an agent
    // that can lock a person out of their own memory.
    for closed in [
        "memory_migrate_storage",
        "memory_doctor",
        "memory_apply_transaction",
    ] {
        assert!(
            !names.contains(&closed),
            "`{closed}` is not published: {names:?}"
        );
    }
    // Refused at the door as well as absent from the list: a name a client
    // guessed must not reach the engine because the list was only advisory.
    let refused = agent.request(
        "tools/call",
        &json!({"name": "memory_migrate_storage", "arguments": {}}),
    );
    assert!(
        refused.get("error").is_some(),
        "calling an unpublished tool is refused: {refused}"
    );
}

#[test]
fn every_tool_states_the_project_first_and_requires_it() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    for tool in agent.tools() {
        let name = tool["name"].as_str().expect("a name").to_owned();
        let schema = &tool["inputSchema"];
        if name == "sync_projects" {
            // The door. It is what an agent calls to learn the keys, so it
            // cannot be the tool that demands one.
            assert!(
                schema["properties"].get("project").is_none(),
                "`{name}` is where keys come from"
            );
            continue;
        }
        let properties = schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("`{name}` has properties"));
        assert_eq!(
            properties.keys().next().map(String::as_str),
            Some("project"),
            "`{name}` states the project first — it is what the rest of the call is about"
        );
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("`{name}` requires something"))
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            required.first(),
            Some(&"project"),
            "`{name}` requires the project: an argument that may be omitted is one that will be"
        );
    }
}

#[test]
fn a_call_without_a_project_is_refused_with_the_keys_to_use() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let refused = agent.call("sync_project", &json!({}));
    assert_eq!(
        refused["error"]["kind"], "unknown_project",
        "a call with no project is refused: {refused}"
    );
    let message = refused["error"]["message"]
        .as_str()
        .expect("a message a model can act on");
    assert!(
        message.contains("PROBE") && message.contains("sync_projects"),
        "the refusal names the keys and where to get them: {message}"
    );

    let wrong = agent.call("sync_project", &json!({"project": "NOT-A-PROJECT"}));
    assert_eq!(wrong["error"]["kind"], "unknown_project", "{wrong}");

    // A path is not a key, and is refused as one rather than resolved. Allowing
    // it would be allowing two names for one project, and the second one would
    // be the machine's rather than the project's.
    let by_path = agent.call(
        "sync_project",
        &json!({"project": project.path().to_string_lossy()}),
    );
    assert_eq!(by_path["error"]["kind"], "unknown_project", "{by_path}");
}

#[test]
fn the_door_lists_what_this_machine_answers_for() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let listed = agent.call("sync_projects", &json!({}));
    let projects = listed["projects"].as_array().expect("a list");
    assert_eq!(projects.len(), 1, "{listed}");
    assert_eq!(projects[0]["project"], "PROBE");
    assert_eq!(projects[0]["name"], "Probe");
}

#[test]
fn a_repository_nobody_opened_is_a_machine_answering_for_nothing() {
    // No `opened_in_sync`: this is a repository nobody has opened as a project.
    // It has no record, so it has no key — and a key is the only way to name a
    // project here. Deriving one from the folder would mint a name that
    // disagrees with the one the project gets the moment somebody describes it.
    let project = repository();
    let mut agent = Agent::open(project.path());

    let listed = agent.call("sync_projects", &json!({}));
    assert_eq!(
        listed["projects"].as_array().map(Vec::len),
        Some(0),
        "nothing is answered for: {listed}"
    );

    let refused = agent.call("sync_project", &json!({"project": "ANYTHING"}));
    assert_eq!(refused["error"]["kind"], "unknown_project", "{refused}");
    let message = refused["error"]["message"]
        .as_str()
        .expect("a message a model can act on");
    assert!(
        message.contains("open one in Sync"),
        "the refusal says what would make this repository answerable: {message}"
    );
}

/// What an extension tells an agent, as it would be written in a real one.
///
/// Short here on purpose — the test is about the text arriving whole, not about
/// what a vocabulary should say.
const EXTENSION_PROMPT: &str = "# Tracker\n\nOne ticket per thing that has to \
                                happen. Never two, and never a ticket about a ticket.";

#[test]
fn the_project_names_itself_its_kinds_and_what_it_is_composed_of() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let described = agent.call("sync_project", &json!({"project": "PROBE"}));
    assert_eq!(described["name"], "Probe");
    assert_eq!(described["language"], "en");
    assert_eq!(described["installed"][0]["id"], "acme.tracker");
    let kinds: Vec<&str> = described["kinds"]
        .as_array()
        .expect("kinds")
        .iter()
        .filter_map(|kind| kind["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"decision"), "the published type: {kinds:?}");

    // One topic per extension the project declares, so an agent can ask about
    // the part of the corpus an extension owns.
    let topics = agent.call("sync_instructions", &json!({"project": "PROBE"}));
    let names: Vec<&str> = topics["topics"]
        .as_array()
        .expect("topics")
        .iter()
        .filter_map(|topic| topic["topic"].as_str())
        .collect();
    assert!(
        names.contains(&"extension:acme.tracker"),
        "the installed extension has a topic: {names:?}"
    );
    assert!(names.contains(&"records"), "the built-in topics: {names:?}");
    assert!(
        topics["topics"]
            .as_array()
            .is_some_and(|listed| listed.iter().all(|topic| topic.get("body").is_none())),
        "the list says what there is to read, not the reading: {topics}"
    );

    // The extension's own words, as the project stored them. This server has no
    // view of the catalogue the extension came from, so anything else it said
    // here would be this build describing somebody else's vocabulary.
    let extension = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "extension:acme.tracker"}),
    );
    let body = extension["body"].as_str().expect("a body");
    assert!(
        body.starts_with(EXTENSION_PROMPT),
        "an extension's topic answers with what the extension published: {extension}"
    );

    // And with what it offers an agent to call, which is the half an agent
    // cannot guess: the full name, the sentence it decides on, and the schema
    // its arguments are checked against. Whole rather than summarised — a
    // description of a schema is a schema that disagrees with the one the
    // arguments are actually checked against.
    for said in [
        "acme.tracker.search_tickets",
        "Finds tickets by their words",
        "\"words\"",
    ] {
        assert!(
            body.contains(said),
            "the topic tells an agent what it can call: {said} is missing from {body}"
        );
    }
    // The names in orientation and nothing more: this is read on every session,
    // and a schema per tool here is paid for by every agent that never calls
    // one. What it buys is an agent knowing there is something to ask about.
    assert_eq!(
        described["installed"][0]["tools"],
        json!(["search_tickets"]),
        "orientation says what there is, by name: {described}"
    );
    assert!(
        described["installed"][0].get("prompt").is_none(),
        "and orientation does not carry it: a document per extension in the answer to \
         `what is this project` is paid for on every session: {described}"
    );

    let one = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "records"}),
    );
    assert!(
        one["body"].as_str().is_some_and(|body| body.len() > 100),
        "a topic answers with its body: {one}"
    );
    let missing = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "no-such-topic"}),
    );
    assert_eq!(missing["error"]["kind"], "invalid_argument");
}

#[test]
fn an_agent_writes_records_without_ever_seeing_an_envelope() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    // No transaction id, no expected revision, no envelope version and no
    // content digest. Everything the store needs and the agent cannot know.
    let written = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-written",
            "kind": "decision",
            "title": "Written by an agent",
            "content": "Through the product tool.",
            "tags": ["probe"],
            "scope_paths": ["src/"],
        }]}),
    );
    assert_eq!(written["changed_keys"][0], "d-written", "{written}");

    let revision = agent.call("sync_project", &json!({"project": "PROBE"}))["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let read = agent.call(
        "memory_get_record",
        &json!({"project": "PROBE", "key": "d-written", "revision": revision}),
    );
    let envelope = &read["record"]["envelope"];
    assert_eq!(envelope["title"], "Written by an agent");
    assert_eq!(envelope["source_paths"]["scope"][0], "src/");

    // The schema is what a type definition is for, and it is not published.
    let refused = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "delete": ["__type__/decision"]}),
    );
    assert_eq!(
        refused["error"]["kind"], "invalid_record",
        "a type definition goes with its type: {refused}"
    );

    // A key the store no longer holds is the state that was asked for, not a
    // failure and not "you named nothing".
    let gone = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "delete": ["never-existed"]}),
    );
    assert!(
        gone["changed_keys"]
            .as_array()
            .is_some_and(std::vec::Vec::is_empty),
        "deleting what is gone is a no-op: {gone}"
    );

    let deleted = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "delete": ["d-written"]}),
    );
    assert_eq!(deleted["changed_keys"][0], "d-written", "{deleted}");
}

/// A key is an address and a title is a name, and the write door is where the
/// difference is noticed: what comes back is the record that was named by its
/// address, and the exact link to name it by instead.
#[test]
fn a_record_named_by_its_key_comes_back_with_the_link_to_name_it_by() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-first",
            "kind": "decision",
            "title": "The one that was taken",
            "content": "It was taken.",
        }]}),
    );

    let written = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-second",
            "kind": "decision",
            "title": "The one after it",
            "content": "Supersedes `d-first`, which it names twice: `d-first`.",
        }]}),
    );
    let bare = written["bare_keys"]
        .as_array()
        .unwrap_or_else(|| panic!("the write says what it noticed: {written}"));
    assert_eq!(
        bare.len(),
        1,
        "one record to name, not one report a mention: {written}"
    );
    assert_eq!(bare[0]["key"], "d-first");
    assert_eq!(bare[0]["written_in"], "d-second");
    assert_eq!(
        bare[0]["write_instead"],
        "[The one that was taken](sync://decision/d-first)"
    );

    // The write landed regardless. A transaction is not thrown away over prose.
    assert_eq!(written["changed_keys"][0], "d-second", "{written}");

    // Written as it asks for, there is nothing to say and nothing is said.
    let named = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-third",
            "kind": "decision",
            "title": "The one that names the first properly",
            "content": "Supersedes [The one that was taken](sync://decision/d-first).",
        }]}),
    );
    assert!(
        named["bare_keys"].is_null(),
        "a write with nothing to report says nothing: {named}"
    );

    // A record naming a sibling of the same transaction is answered from the
    // transaction: neither of them is in the store yet.
    let together = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [
            {
                "key": "d-fourth",
                "kind": "decision",
                "title": "The one that names its sibling",
                "content": "Rests on `d-fifth`.",
            },
            {
                "key": "d-fifth",
                "kind": "decision",
                "title": "The sibling",
                "content": "Nothing here names anything.",
            },
        ]}),
    );
    assert_eq!(
        together["bare_keys"][0]["write_instead"], "[The sibling](sync://decision/d-fifth)",
        "{together}"
    );
}

/// The other spelling, and the other answer: double brackets carry no kind, so
/// nothing can follow one, and the door refuses rather than advises. What makes
/// that affordable is that nothing is guessed — the spelling is wrong whatever
/// it points at.
#[test]
fn a_write_spelling_a_record_in_double_brackets_is_refused() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-first",
            "kind": "decision",
            "title": "The one that was taken",
            "content": "It was taken.",
        }]}),
    );

    let refused = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-bracketed",
            "kind": "decision",
            "title": "The one that pointed nowhere",
            "content": "Supersedes [[d-first]].",
        }]}),
    );
    assert_eq!(refused["error"]["kind"], "invalid_argument", "{refused}");
    let message = refused["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("a refusal says why: {refused}"));
    assert!(
        message.contains("[The one that was taken](sync://decision/d-first)"),
        "the refusal carries the link to write instead: {message}"
    );
    assert!(
        message.contains("Nothing was written"),
        "the refusal says the transaction did not land: {message}"
    );
    assert_eq!(refused["error"]["data"]["wikilinks"][0]["where"], "content");

    // Refused means refused: the record is not in the store under half of what
    // was sent. A door that reported and wrote would leave the dead end behind.
    let revision = agent.call("sync_project", &json!({"project": "PROBE"}))["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let read = agent.call(
        "memory_get_record",
        &json!({"project": "PROBE", "key": "d-bracketed", "revision": revision}),
    );
    assert!(
        read["record"].is_null() || read.get("error").is_some(),
        "the refused record was not written: {read}"
    );

    // A title and a field are read as well as a body: they are what a reader
    // meets first, and a dead end in one is the same dead end.
    let by_title = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-titled",
            "kind": "decision",
            "title": "After [[d-first]]",
            "content": "Nothing here names anything.",
        }]}),
    );
    assert_eq!(by_title["error"]["data"]["wikilinks"][0]["where"], "title");

    // Nothing answers to this one, and it is refused all the same: the spelling
    // is what is wrong, and a link to nowhere is not what was missing.
    let nowhere = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-nowhere",
            "kind": "decision",
            "title": "The one that named a stranger",
            "content": "Rests on [[d-never-existed]].",
        }]}),
    );
    assert_eq!(nowhere["error"]["kind"], "invalid_argument", "{nowhere}");
    assert!(
        nowhere["error"]["data"]["wikilinks"][0]["write_instead"].is_null(),
        "there is no link to suggest for a key nothing answers to: {nowhere}"
    );

    // And a document about the spelling remains writable, because quoting a
    // syntax is what code spans and fences are for.
    let quoted = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "d-quoting",
            "kind": "decision",
            "title": "The one that explains the rule",
            "content": "Do not write `[[d-first]]`.",
        }]}),
    );
    assert_eq!(quoted["changed_keys"][0], "d-quoting", "{quoted}");
}

#[test]
fn a_kind_the_project_invented_is_a_kind_an_agent_can_write() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    // The surface must not contradict itself: a kind `sync_project` names is a
    // kind `sync_apply` takes. It did not, once — `kind` was typed as the
    // eleven Sync ships definitions for, so a type somebody created in the
    // window was advertised and then refused.
    let kinds: Vec<String> = agent.call("sync_project", &json!({"project": "PROBE"}))["kinds"]
        .as_array()
        .expect("kinds")
        .iter()
        .filter_map(|kind| kind["kind"].as_str().map(str::to_owned))
        .collect();
    assert!(kinds.iter().any(|kind| kind == "ticket"), "{kinds:?}");

    let written = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{
            "key": "t-one",
            "kind": "ticket",
            "title": "A ticket",
            "content": "Of a kind this build has never heard of.",
        }]}),
    );
    assert_eq!(written["changed_keys"][0], "t-one", "{written}");

    // And a kind the project does *not* hold is refused by the engine's schema,
    // which is the right refuser: it knows what definitions exist.
    let refused = agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [{"key": "n-one", "kind": "no-such-kind", "title": "T", "content": "C"}]}),
    );
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("schema")),
        "the schema refuses an unknown kind: {refused}"
    );
}

/// One server, two projects, and no leakage between them.
///
/// The point of the whole arrangement: a machine's projects share a process and
/// a model, and stay entirely separate corpora. What keeps them apart is the
/// key on every call — there is no session state to get out of step, so a call
/// naming `LEFT` cannot be affected by anything a call naming `RIGHT` did.
#[test]
fn one_server_answers_for_two_projects_and_keeps_them_apart() {
    let left = repository();
    let right = repository();
    let _left = opened_as(left.path(), "Left", "LEFT");
    let _right = opened_as(right.path(), "Right", "RIGHT");

    let registry = tempfile::tempdir().expect("a temporary directory");
    let file = registry.path().join("registered-projects.json");
    std::fs::write(
        &file,
        json!([
            {"path": left.path(), "name": "Left", "identifier": "LEFT"},
            {"path": right.path(), "name": "Right", "identifier": "RIGHT"},
        ])
        .to_string(),
    )
    .expect("a registry is written");

    let mut agent = Agent::over(&file);

    let listed = agent.call("sync_projects", &json!({}));
    let keys: Vec<&str> = listed["projects"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|held| held["project"].as_str())
        .collect();
    assert_eq!(keys, vec!["LEFT", "RIGHT"], "{listed}");

    assert_eq!(
        agent.call("sync_project", &json!({"project": "LEFT"}))["name"],
        "Left"
    );
    assert_eq!(
        agent.call("sync_project", &json!({"project": "right"}))["name"],
        "Right",
        "a key is a name written one way, not two"
    );

    // Written in one, and absent from the other.
    let written = agent.call(
        "sync_apply",
        &json!({
            "project": "LEFT",
            "save": [{
                "key": "d-left",
                "kind": "decision",
                "title": "Only in Left",
                "content": "",
            }],
        }),
    );
    assert_eq!(
        written["project"], "LEFT",
        "the answer says whose it is: {written}"
    );

    let elsewhere = agent.call(
        "memory_get_record",
        &json!({"project": "RIGHT", "key": "d-left"}),
    );
    assert!(
        elsewhere["record"].is_null() || elsewhere.get("error").is_some(),
        "a record written in one project is not in the other: {elsewhere}"
    );
}

/// The two writes that keeping a conversation is, against a type that declares
/// fields — driven through the window's channel, which is what performs them.
///
/// This is a regression, and the fault it holds down cost a shipped feature:
/// Chat published `chat.conversation` with a required field called `folder`,
/// which is a member of every record's envelope. Nothing refused the
/// definition, and from then on keeping a conversation failed twice over and
/// silently — the new record for missing a required field the window is not
/// allowed to send, and the write after it for naming an envelope member. The
/// corpus held no kept conversation at all, and no screen ever said why.
#[test]
fn a_conversation_is_kept_in_the_two_writes_the_window_makes() {
    let project = repository();
    let (mut client, _logs) = opened_as(project.path(), "Kept", "KEPT");

    let published = client
        .publish_extension_types(&json!([{
            "kind": "chat.conversation",
            "title": "Conversation",
            "description": "A conversation with an agent, kept on purpose.",
            "icon": "message-square",
            "fields": {
                "agent": {"type": "string", "required": true},
                "workdir": {"type": "string", "required": true},
                "tokens": {"type": "integer"},
                "complete": {"type": "boolean"},
            },
        }]))
        .expect("the extension's type is published");
    assert!(published, "a type the project did not hold is written");

    // What the window does first: an empty record of the type. It carries no
    // fields of its own, so the required ones have to be filled from the
    // definition — a record missing one is a record the strict schema rejects.
    let created = client
        .create_document("chat.conversation", "Talking about the parser", None)
        .expect("an empty conversation record is created");

    // And then everything about it, fields and all.
    let edits = sync_memory::DocumentEdits {
        content: Some("### You\n\nWhy is it slow?".to_owned()),
        fields: Some(
            json!({
                "agent": "Claude Code",
                "workdir": "/tmp/kept",
                "tokens": 812,
                "complete": true,
            })
            .as_object()
            .expect("an object")
            .clone(),
        ),
        ..Default::default()
    };
    client
        .update_document(&created.key, &edits)
        .expect("the transcript and the facts about it are written");

    let stored = client
        .document(&created.key)
        .expect("the record is read back")
        .expect("and it is there");
    assert_eq!(stored.kind, "chat.conversation");
    assert_eq!(stored.title, "Talking about the parser");
    assert_eq!(stored.content, "### You\n\nWhy is it slow?");
    assert_eq!(stored.fields["agent"], "Claude Code");
    assert_eq!(
        stored.fields["workdir"], "/tmp/kept",
        "the working directory is a product field of its own, not the envelope's `folder`"
    );
    assert_eq!(stored.fields["tokens"], 812);
    assert_eq!(stored.fields["complete"], true);
}

/// A type naming one of the envelope's own members is refused where it is
/// published, rather than at every write of it forever after.
#[test]
fn a_type_that_declares_an_envelope_member_is_not_published() {
    let project = repository();
    let (mut client, _logs) = opened_as(project.path(), "Kept", "KEPT");

    let refused = client
        .publish_extension_types(&json!([{
            "kind": "chat.conversation",
            "title": "Conversation",
            "description": "A conversation with an agent, kept on purpose.",
            "icon": "message-square",
            "fields": {
                "agent": {"type": "string", "required": true},
                "folder": {"type": "string", "required": true},
            },
        }]))
        .expect_err("a type the store could never hold is not published");
    let said = refused.to_string();
    assert!(
        said.contains("folder"),
        "the refusal names the field, because that is the whole of the fix: {said}"
    );

    // And nothing of it landed: an extension whose type is unwritable must not
    // count as installed.
    let types = client.list_types().expect("the types are read");
    assert!(
        !types.iter().any(|kind| kind.kind == "chat.conversation"),
        "the definition was refused, so the project does not hold it"
    );
}

/// **The catalogue grows by one name, whatever a project's extensions offer.**
///
/// This is the whole reason there is a dispatcher rather than a tool per
/// contribution: every entry in this list is paid for in tokens by every agent
/// on every turn, including the ones that will never call it. A project with
/// four extensions offering three tools each would put twelve descriptions and
/// twelve schemas in front of an agent that asked about none of them.
#[test]
fn what_extensions_offer_is_one_name_in_the_catalogue_rather_than_one_each() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let names: Vec<String> = agent
        .tools()
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();

    assert!(
        names.iter().any(|name| name == "sync_call"),
        "the one door onto extensions is published: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("acme.tracker")),
        "and what the extension offers is not in the catalogue: {names:?}"
    );
}

/// A tool of an extension the project does not have is refused by name, and the
/// refusal says which project was asked.
///
/// An agent reads one answer and has to be able to act on it: *this project*
/// has no such extension is a different fact from *no such tool exists*, and
/// only the first tells it to look at `sync_project`.
#[test]
fn a_tool_of_an_extension_this_project_does_not_have_is_refused_naming_both() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let answer = agent.request(
        "tools/call",
        &json!({
            "name": "sync_call",
            "arguments": {"project": "PROBE", "tool": "other.notes.search"},
        }),
    );
    let said = answer.to_string();

    assert!(
        said.contains("PROBE"),
        "the refusal names the project: {said}"
    );
    assert!(
        said.contains("other.notes"),
        "and the extension that is not here: {said}"
    );
    assert!(
        said.contains("sync_project"),
        "and where to read what this project does have: {said}"
    );
}

/// A name the extension does not offer is refused with what it does offer, and
/// with the topic that describes each one.
///
/// The compensation for a catalogue that carries one name: a client cannot
/// check against a schema it was never given, so this side has to answer better
/// than the client's own check would have.
#[test]
fn a_tool_an_extension_does_not_offer_is_refused_with_the_ones_it_does() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let answer = agent.request(
        "tools/call",
        &json!({
            "name": "sync_call",
            "arguments": {"project": "PROBE", "tool": "acme.tracker.file_ticket"},
        }),
    );
    let said = answer.to_string();

    assert!(
        said.contains("acme.tracker.search_tickets"),
        "the refusal names what is offered instead: {said}"
    );
    assert!(
        said.contains("extension:acme.tracker"),
        "and the topic that says what each takes: {said}"
    );
}

/// Arguments that do not match the schema its author published are refused
/// before anything runs, and the refusal says what was wrong with them.
#[test]
fn arguments_that_do_not_fit_the_package_s_own_schema_are_refused() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let answer = agent.request(
        "tools/call",
        &json!({
            "name": "sync_call",
            "arguments": {
                "project": "PROBE",
                "tool": "acme.tracker.search_tickets",
                // The package declared this a string.
                "arguments": {"words": 41},
            },
        }),
    );
    let said = answer.to_string();

    assert!(
        said.contains("words"),
        "the refusal names the argument that was wrong: {said}"
    );
    assert!(
        said.contains("extension:acme.tracker"),
        "and where the whole schema is stated: {said}"
    );
}

/// A call that passes every check reaches for Sync — and says so plainly when
/// Sync is not there.
///
/// This process is started by the application in the product and by this test
/// on its own, so "nobody is attending" is the honest state here. What is being
/// tested is that it is *said*, at once: a tool call that hung until its
/// patience ran out would be a minute of an agent's time spent on an answer
/// this process had from the first instant.
#[test]
fn a_call_with_no_application_behind_it_says_so_rather_than_hanging() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let answer = agent.request(
        "tools/call",
        &json!({
            "name": "sync_call",
            "arguments": {
                "project": "PROBE",
                "tool": "acme.tracker.search_tickets",
                "arguments": {"words": "a ticket"},
            },
        }),
    );
    let said = answer.to_string();

    assert!(
        said.contains("Sync is not on the other end"),
        "the refusal says what is missing rather than blaming the package: {said}"
    );
}

/// Two things about a record that are not its text: where it is filed away, and
/// whether anybody has checked it.
///
/// A write states the whole record, which is what makes an unstated archive
/// flag dangerous — correcting a sentence in a record somebody put away would
/// bring it back into every listing, and nothing would say so. So the flag is
/// read off the store when the write is silent about it, and only a write that
/// names it moves it.
///
/// Freshness is stated the other way round, and deliberately: a text that
/// changed is a claim nobody has checked since. Silence there means
/// `unverified`, and `fresh` is reachable only by saying you read the code.
#[test]
fn an_archive_survives_a_write_and_a_check_has_to_be_claimed() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let write = |agent: &mut Agent, record: Value| {
        agent.call("sync_apply", &json!({"project": "PROBE", "save": [record]}))
    };
    let envelope = |agent: &mut Agent| {
        let revision = agent.call("sync_project", &json!({"project": "PROBE"}))["revision"]
            .as_str()
            .expect("a revision")
            .to_owned();
        agent.call(
            "memory_get_record",
            &json!({"project": "PROBE", "key": "d-standing", "revision": revision}),
        )["record"]["envelope"]
            .clone()
    };

    write(
        &mut agent,
        json!({
            "key": "d-standing",
            "kind": "decision",
            "title": "What was decided",
            "content": "The first version.",
            "scope_paths": ["src/"],
            "archived": true,
        }),
    );
    let stored = envelope(&mut agent);
    assert_eq!(stored["archive"]["archived"], true, "{stored}");
    assert_eq!(
        stored["freshness"]["state"], "unverified",
        "a write that claims nothing has checked nothing: {stored}"
    );

    // The sentence moved and the archive did not, because the write said
    // nothing about it.
    write(
        &mut agent,
        json!({
            "key": "d-standing",
            "kind": "decision",
            "title": "What was decided",
            "content": "The second version.",
            "scope_paths": ["src/"],
        }),
    );
    let stored = envelope(&mut agent);
    assert_eq!(stored["content"], "The second version.");
    assert_eq!(
        stored["archive"]["archived"], true,
        "an unstated flag leaves the record filed as it was: {stored}"
    );

    write(
        &mut agent,
        json!({
            "key": "d-standing",
            "kind": "decision",
            "title": "What was decided",
            "content": "The second version.",
            "scope_paths": ["src/"],
            "archived": false,
            "verified": true,
        }),
    );
    let stored = envelope(&mut agent);
    assert_eq!(stored["archive"]["archived"], false, "{stored}");
    assert_eq!(
        stored["freshness"]["state"], "fresh",
        "the one route to fresh is somebody saying they read the code: {stored}"
    );

    // Nothing to have read, so nothing to claim — and nothing that would ever
    // take the flag off again.
    let refused = write(
        &mut agent,
        json!({
            "key": "d-unscoped",
            "kind": "decision",
            "title": "A claim about nothing in particular",
            "content": "No paths.",
            "verified": true,
        }),
    );
    assert_eq!(refused["error"]["kind"], "invalid_argument", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|said| said.contains("scope_paths")),
        "the refusal says what to add: {refused}"
    );
}

/// A package's instructions are served, never scattered.
///
/// The project's own record carries what every installed package tells an
/// agent, because that record travels with the repository and a colleague who
/// cloned it has to be told the same thing. What must not happen is that text
/// arriving in answers nobody asked it for: a listing of records that happens
/// to include the project's own would otherwise carry every package's document
/// to an agent that asked what this project holds.
#[test]
fn a_package_s_prose_arrives_through_its_topic_and_nowhere_else() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let revision = agent.call("sync_project", &json!({"project": "PROBE"}))["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();

    for (tool, arguments) in [
        (
            "memory_list_records",
            json!({"project": "PROBE", "metadata_only": true, "limit": 50}),
        ),
        (
            "memory_get_record",
            json!({"project": "PROBE", "key": "PROBE", "revision": revision}),
        ),
        (
            "memory_search",
            json!({"project": "PROBE", "query": "probe"}),
        ),
    ] {
        let answer = agent.call(tool, &arguments).to_string();
        assert!(
            !answer.contains(EXTENSION_PROMPT),
            "{tool} carried a package's instructions to somebody who asked for records: {answer}"
        );
        assert!(
            !answer.contains("Finds tickets by their words"),
            "{tool} carried a tool's description as well: {answer}"
        );
    }

    // Still there, in the one call whose purpose is to hand it over.
    let topic = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "extension:acme.tracker"}),
    );
    assert!(
        topic["body"]
            .as_str()
            .is_some_and(|body| body.contains(EXTENSION_PROMPT)),
        "the topic is where a package speaks: {topic}"
    );
}

/// What joins a kind to the instructions that describe it.
///
/// A project is a list of kinds and a list of packages, and without the join
/// they are two lists: an agent asked to write a Watch has no way to know whose
/// topic says what a good one looks like. The prefix is the join, and it is
/// said rather than left to be inferred — including in the hint beside each
/// topic, which names what the package brought rather than the package.
#[test]
fn orientation_says_which_package_brought_which_kind() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let described = agent.call("sync_project", &json!({"project": "PROBE"}));
    let package = &described["installed"][0];
    assert_eq!(
        package["kinds"],
        json!(["acme.tracker.watch"]),
        "{described}"
    );
    assert_eq!(
        package["instructions"], "extension:acme.tracker",
        "orientation says where to read about it: {described}"
    );
    assert!(
        described["next"]
            .as_str()
            .is_some_and(|next| next.contains("instructions")),
        "and says to read it before writing one: {described}"
    );

    let topics = agent.call("sync_instructions", &json!({"project": "PROBE"}));
    let hint = topics["topics"]
        .as_array()
        .expect("topics")
        .iter()
        .find(|topic| topic["topic"] == "extension:acme.tracker")
        .and_then(|topic| topic["when"].as_str())
        .expect("a hint for the package's topic")
        .to_owned();
    assert!(
        hint.contains("Watch"),
        "the hint is in the words somebody would ask in, not the package's id: {hint}"
    );
}

/// A type's own definition is what says how to write one of its records.
///
/// The fields, what each takes, which are required, the relations it may carry,
/// and the guidance its author wrote for the moment before the first write.
/// None of it is reachable anywhere else on this surface — the engine's
/// catalogue answers with how *many* fields a type has — so a writer that
/// cannot read it here finds a field name out from a refusal.
#[test]
fn a_topic_states_the_schema_of_every_kind_it_covers() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    let brought = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "extension:acme.tracker"}),
    );
    let body = brought["body"].as_str().expect("a body").to_owned();
    assert!(
        body.contains("acme.tracker.watch") && body.contains("Watch"),
        "the package's topic covers the kinds it brought: {body}"
    );

    // The kinds nobody's package brought have no topic of their own, and are
    // described in the one that says so.
    let topics = agent.call("sync_instructions", &json!({"project": "PROBE"}));
    let listed: Vec<&str> = topics["topics"]
        .as_array()
        .expect("topics")
        .iter()
        .filter_map(|topic| topic["topic"].as_str())
        .collect();
    assert!(listed.contains(&"kinds"), "{listed:?}");

    let own = agent.call(
        "sync_instructions",
        &json!({"project": "PROBE", "topic": "kinds"}),
    );
    let body = own["body"].as_str().expect("a body").to_owned();
    assert!(
        body.contains("ticket") && body.contains("decision"),
        "the project's own kinds are described where a writer is looking: {body}"
    );
    assert!(
        !body.contains("acme.tracker.watch"),
        "and a package's kinds are described by the package: {body}"
    );
}

/// Filing a record, and describing the place it is filed in.
///
/// Both are members of the write rather than calls of their own, which is the
/// part worth holding: a package's instructions say so in those words, and an
/// agent that found them missing would go looking for a `folders.create` that
/// has never existed on this surface.
#[test]
fn a_record_is_filed_and_its_folder_described_by_the_same_write() {
    let project = repository();
    let _sync = opened_in_sync(project.path());
    let mut agent = Agent::open(project.path());

    agent.call(
        "sync_apply",
        &json!({"project": "PROBE", "save": [
            {
                "key": "d-filed",
                "kind": "decision",
                "title": "Filed where it belongs",
                "content": "In a folder.",
                "folder": "releases",
            },
            {
                "key": "d-about-the-folder",
                "kind": "decision",
                "title": "What goes in releases",
                "content": "Anything a release turns on.",
                "folder": "releases",
                "is_folder": true,
            },
        ]}),
    );

    let folders = agent.call(
        "memory_list_folders",
        &json!({"project": "PROBE", "kind": "decision"}),
    );
    let listed = folders["folders"]
        .as_array()
        .expect("folders")
        .iter()
        .find(|folder| folder["path"] == "releases")
        .cloned()
        .unwrap_or_else(|| panic!("the folder the write named: {folders}"));
    assert_eq!(
        listed["described_by"], "d-about-the-folder",
        "the record that is the folder says so on the write that filed it: {listed}"
    );
}
