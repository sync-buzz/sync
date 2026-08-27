#![allow(clippy::expect_used, clippy::unwrap_used)]

//! The commands, invoked the way the frontend invokes them.
//!
//! `src/lib/memory/client.ts` calls `invoke("memory_open", { project })` and
//! friends. This drives the same path through Tauri's IPC with a mock runtime:
//! argument names are converted from camelCase, the command runs, and the
//! result comes back as JSON. It is the piece between the typed client and the
//! engine that nothing else covers.
//!
//! Needs a real `sync-mcp`; without one the tests say so and pass, the same way
//! `sync-memory`'s end-to-end tests do. The sidecar is found where this
//! workspace builds it, so a suite that is quietly testing nothing says which
//! command would give it something to test.

use std::process::Command;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

mod common;

/// An application with the memory commands registered, exactly as `run()` does.
fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(sync_lib::memory::MemorySessions::default())
        .invoke_handler(tauri::generate_handler![
            sync_lib::memory::memory_open,
            sync_lib::memory::memory_status,
            sync_lib::memory::memory_save,
            sync_lib::memory::memory_search,
            sync_lib::memory::memory_document,
            sync_lib::memory::memory_document_update,
            sync_lib::memory::memory_type_create,
            sync_lib::memory::memory_types,
            sync_lib::memory::memory_extension_types_publish,
            sync_lib::memory::memory_reindex,
            sync_lib::memory::memory_document_create,
            sync_lib::memory::memory_document_delete,
            sync_lib::memory::memory_document_dependents,
            sync_lib::memory::memory_folders,
            sync_lib::memory::memory_folder_rename,
            sync_lib::memory::memory_document_move,
        ])
        .build(mock_context(noop_assets()))
        .expect("the mock application builds");
    let webview = WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("a webview to invoke from");
    (app, webview)
}

fn invoke(
    webview: &WebviewWindow<MockRuntime>,
    command: &str,
    args: Value,
) -> Result<Value, Value> {
    let response = get_ipc_response(
        webview,
        InvokeRequest {
            cmd: command.to_owned(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("a local origin"),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );
    response.map(|body| match body {
        InvokeResponseBody::Json(text) => {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        }
        InvokeResponseBody::Raw(bytes) => {
            Value::String(String::from_utf8_lossy(&bytes).into_owned())
        }
    })
}

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temp project");
    let output = Command::new("git")
        .arg("init")
        .arg(directory.path())
        .output()
        .expect("git init");
    assert!(output.status.success());
    directory
}

/// Publish a type the way a project does, through the command the type sheet
/// calls.
///
/// Nothing can be written until this has happened: Sync publishes exactly one
/// definition of its own — `project` — and every other kind a project stores is
/// the project's own, created here or by an agent. A test that wrote a record
/// without one is testing a corpus no project has.
fn publish_type(webview: &WebviewWindow<MockRuntime>, project: &str, kind: &str, title: &str) {
    invoke(
        webview,
        "memory_type_create",
        json!({
            "project": project,
            "kind": kind,
            "title": title,
            "description": format!("{title}, for a test."),
            "icon": "signpost",
        }),
    )
    .expect("the type is published");
}

#[test]
fn the_frontend_can_open_memory_write_and_search_through_the_commands() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    let summary = invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");
    assert!(
        summary["version"].as_str().is_some_and(|v| !v.is_empty()),
        "the engine reports itself: {summary}"
    );

    publish_type(&webview, &project_path, "spec", "Spec");

    // camelCase on the wire, snake_case in Rust — the conversion Tauri does is
    // part of the contract the typed client relies on.
    let write = invoke(
        &webview,
        "memory_save",
        json!({
            "project": project_path,
            "entities": [{
                "key": "s-first",
                "kind": "spec",
                "title": "Written from the frontend",
                "content": "The command layer carried this all the way down.",
                "fields": {"status": "todo"}
            }]
        }),
    )
    .expect("writing succeeds");
    assert_eq!(write["changed_keys"][0], "s-first");

    // A store this new has no inverted index yet, and the engine answers a search
    // against one with `index` rather than with no results. Building it here is
    // what makes the assertion below about the command layer rather than about
    // how many records happen to exist.
    invoke(&webview, "memory_reindex", json!({"project": project_path})).expect("the index builds");

    let found = invoke(
        &webview,
        "memory_search",
        json!({"project": project_path, "query": {"query": "frontend", "limit": 10}}),
    )
    .expect("search succeeds");
    assert!(
        found["hits"]
            .as_array()
            .is_some_and(|hits| hits.iter().any(|hit| hit["id"] == "s-first")),
        "the record written through the command is findable through it: {found}"
    );

    let status = invoke(&webview, "memory_status", json!({"project": project_path}))
        .expect("status succeeds");
    assert!(
        status["model"]["mode"] == "fts" || status["model"]["mode"] == "hybrid",
        "the UI is told how search will answer: {status}"
    );
}

#[test]
fn the_editor_saves_through_the_command_and_reads_back_what_it_wrote() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");
    publish_type(&webview, &project_path, "decision", "Decision");
    invoke(
        &webview,
        "memory_save",
        json!({
            "project": project_path,
            "entities": [{
                "key": "d-editor",
                "kind": "decision",
                "title": "Before the edit",
                "content": "The old body.",
                "scope_paths": ["src/components/editor/"],
                "tags": ["shell"],
            }]
        }),
    )
    .expect("writing succeeds");

    // What `updateMemoryDocument` sends, under the names it sends it: a key
    // and a patch of what changed, nothing else.
    let written = invoke(
        &webview,
        "memory_document_update",
        json!({
            "project": project_path,
            "key": "d-editor",
            "edits": {
                "title": "After the edit",
                "content": "# The new body\n",
            },
        }),
    )
    .expect("the save succeeds");

    assert_eq!(written["title"], "After the edit");
    assert_eq!(written["content"], "# The new body\n");
    assert_eq!(
        written["scope"][0], "src/components/editor/",
        "the answer is the record as stored, and the edit kept its scope"
    );
    assert_eq!(written["tags"][0], "shell");

    let read = invoke(
        &webview,
        "memory_document",
        json!({"project": project_path, "key": "d-editor"}),
    )
    .expect("reading it back succeeds");
    assert_eq!(read["content"], "# The new body\n");

    // The panel beside the editor sends the same command with a different patch,
    // and the body somebody is typing is not part of it.
    let archived = invoke(
        &webview,
        "memory_document_update",
        json!({
            "project": project_path,
            "key": "d-editor",
            "edits": {"tags": ["shell", "editor"], "archived": true},
        }),
    )
    .expect("the metadata edit succeeds");
    assert_eq!(archived["content"], "# The new body\n");
    assert_eq!(archived["tags"], json!(["shell", "editor"]));
    assert_eq!(archived["archived"], true);

    let refused = invoke(
        &webview,
        "memory_document_update",
        json!({
            "project": project_path,
            "key": "__type__/decision",
            "edits": {"title": "Renamed", "content": "prose"},
        }),
    )
    .expect_err("a type definition is not a document");
    assert_eq!(
        refused["kind"], "invalid_record",
        "and the refusal carries a kind the window branches on: {refused}"
    );
}

#[test]
fn the_window_creates_a_record_asks_what_holds_it_and_deletes_it() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");
    publish_type(&webview, &project_path, "note", "Note");

    let created = invoke(
        &webview,
        "memory_document_create",
        json!({"project": project_path, "kind": "note", "title": "Untitled note"}),
    )
    .expect("the record is created");
    let key = created["key"].as_str().expect("a key").to_owned();
    assert!(key.starts_with("note-"), "the key says its kind: {key}");
    assert_eq!(created["title"], "Untitled note");
    assert_eq!(created["content"], "");

    let holding = invoke(
        &webview,
        "memory_document_dependents",
        json!({"project": project_path, "key": key}),
    )
    .expect("the store answers");
    assert_eq!(holding["links"], json!([]));
    assert_eq!(holding["mentions"], json!([]));

    invoke(
        &webview,
        "memory_document_delete",
        json!({"project": project_path, "keys": [key]}),
    )
    .expect("the record is deleted");

    let gone = invoke(
        &webview,
        "memory_document",
        json!({"project": project_path, "key": key}),
    )
    .expect("the read succeeds");
    assert!(gone.is_null(), "a deleted record reads back as nothing");
}

/// An extension's vocabulary, published and read back through the real channel.
///
/// The point of the test is the reading back. A type an extension brings is
/// more than four strings — it declares the fields its records carry, the
/// relations they may hold, and what an agent is told before writing one — and
/// every one of those crosses three vocabularies on the way in: camelCase from
/// the window, camelCase again over the host channel, snake_case in the
/// engine's own schema. Every layer between deserializes forgivingly, so a
/// member none of them models is dropped in silence and the publish still
/// answers success. Nothing but reading the value back catches that.
#[test]
fn an_extension_publishes_a_vocabulary_and_the_project_holds_all_of_it() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");

    invoke(
        &webview,
        "memory_extension_types_publish",
        json!({
            "project": project_path,
            "types": [{
                "kind": "example.question",
                "title": "Question",
                "description": "An unresolved fork.",
                "icon": "circle-help",
                "guidance": "Raise one before asking a person, not after.",
                "fields": {
                    "status": {
                        "type": "enum",
                        "values": ["open", "answered"],
                        "required": true,
                        "default": "open",
                    },
                    "answer": {"type": "text", "required": false},
                },
                "relationships": {
                    "answered_by": {"target": "any", "description": "What settled it."},
                },
            }],
        }),
    )
    .expect("the vocabulary is published");

    let types = invoke(&webview, "memory_types", json!({"project": project_path}))
        .expect("the corpus answers with its types");
    let published = types
        .as_array()
        .expect("a list of types")
        .iter()
        .find(|entry| entry["kind"] == "example.question")
        .expect("the type the extension published");

    assert_eq!(published["title"], "Question");
    assert_eq!(
        published["guidance"], "Raise one before asking a person, not after.",
        "what an agent is told travels with the type, not with the build"
    );
    assert_eq!(
        published["fields"]["status"]["values"],
        json!(["open", "answered"]),
        "an enumeration arrives whole: a field the engine cannot validate against \
         is a field it would refuse a record for"
    );
    assert_eq!(published["fields"]["status"]["default"], "open");
    assert_eq!(published["fieldCount"], 2);
    assert_eq!(published["relationships"]["answered_by"]["target"], "any");

    // The engine validates against what was published rather than against what
    // the window remembers, which is the whole reason the declaration has to
    // arrive intact.
    let created = invoke(
        &webview,
        "memory_document_create",
        json!({"project": project_path, "kind": "example.question", "title": "Which way?"}),
    )
    .expect("a record of the published type is written");
    assert!(
        created["key"]
            .as_str()
            .is_some_and(|key| key.starts_with("question-")),
        "the key names the kind rather than the extension: {}",
        created["key"]
    );
    assert_eq!(
        created["fields"]["status"], "open",
        "a new record starts at the default the declaration states"
    );
}

/// The three folder commands, in the order a person uses them: file a record
/// somewhere, see the folder that now exists, rename it.
#[test]
fn the_window_files_a_record_sees_the_folder_and_renames_it() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");
    publish_type(&webview, &project_path, "note", "Note");

    let created = invoke(
        &webview,
        "memory_document_create",
        json!({"project": project_path, "kind": "note", "title": "Where storage goes"}),
    )
    .expect("the record is created");
    let key = created["key"].as_str().expect("a key").to_owned();
    assert!(
        created["folder"].is_null(),
        "a record nobody filed is in no folder, and the root is that absence"
    );

    invoke(
        &webview,
        "memory_document_move",
        json!({"project": project_path, "key": key, "folder": "decisions/storage"}),
    )
    .expect("the record is filed");

    let filed = invoke(
        &webview,
        "memory_document",
        json!({"project": project_path, "key": key}),
    )
    .expect("the read succeeds");
    assert_eq!(filed["folder"], json!("decisions/storage"));
    assert_eq!(
        filed["isFolder"],
        json!(false),
        "being filed in a folder is not being one"
    );

    // Folders are implicit: this one exists because a record is in it, and it
    // is known from the records rather than from any directory on disk.
    let folders = invoke(&webview, "memory_folders", json!({"project": project_path}))
        .expect("the folders are listed");
    let storage = folders
        .as_array()
        .expect("a list")
        .iter()
        .find(|entry| entry["path"] == json!("decisions/storage"))
        .expect("the folder the record is in");
    assert_eq!(storage["inRecords"], json!(true));
    assert_eq!(storage["inStorage"], json!(false));
    assert_eq!(storage["records"], json!(1));
    assert!(
        storage["describedBy"].is_null(),
        "nobody has given this folder a record of its own"
    );

    invoke(
        &webview,
        "memory_folder_rename",
        json!({"project": project_path, "from": "decisions/storage", "to": "decisions/persistence"}),
    )
    .expect("the folder is renamed");

    let moved = invoke(
        &webview,
        "memory_document",
        json!({"project": project_path, "key": key}),
    )
    .expect("the read succeeds");
    assert_eq!(
        moved["folder"],
        json!("decisions/persistence"),
        "renaming the folder carried the record in it"
    );
    assert_eq!(moved["key"], json!(key), "and broke no link doing it");
}

/// The engine refuses a move it cannot make honestly, and the refusal has to
/// arrive as something the window can put in front of a person.
#[test]
fn a_move_the_engine_refuses_arrives_as_a_failure_and_not_as_silence() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    invoke(&webview, "memory_open", json!({"project": project_path}))
        .expect("opening memory succeeds");
    publish_type(&webview, &project_path, "note", "Note");
    let created = invoke(
        &webview,
        "memory_document_create",
        json!({"project": project_path, "kind": "note", "title": "A note"}),
    )
    .expect("the record is created");
    let key = created["key"].as_str().expect("a key").to_owned();

    let failure = invoke(
        &webview,
        "memory_document_move",
        json!({"project": project_path, "key": key, "folder": ""}),
    )
    .expect_err("moving a record to where it already is is not a move");
    assert_eq!(failure["kind"], json!("invalid_argument"));
    assert!(
        failure["message"].as_str().is_some_and(|it| !it.is_empty()),
        "a refusal a person can read: {failure}"
    );
}

#[test]
fn a_failure_reaches_the_frontend_as_a_kind_it_can_branch_on() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let (_app, webview) = app();

    // A path that is not a repository: the engine cannot serve it.
    let directory = tempfile::tempdir().expect("temp dir");
    let error = invoke(
        &webview,
        "memory_open",
        json!({"project": directory.path().to_string_lossy()}),
    )
    .expect_err("a project without a repository fails");

    assert!(
        error["kind"].as_str().is_some_and(|kind| !kind.is_empty()),
        "failures carry a machine-readable kind, not just prose: {error}"
    );
    assert!(
        error["message"].as_str().is_some_and(|m| !m.is_empty()),
        "and a message for people"
    );
}
