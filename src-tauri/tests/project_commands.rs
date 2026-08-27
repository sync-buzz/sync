#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Opening a folder, invoked the way the frontend invokes it.
//!
//! `src/lib/project/client.ts` calls `invoke("project_probe", { path })`,
//! `invoke("project_initialize_repository", { path })` and the two settings
//! commands. This drives the same path through Tauri's IPC with a mock runtime,
//! against real folders on disk, so the answers the opening flow branches on —
//! in a repository or not, opened before or not — are covered by something
//! other than clicking through the interface.

mod common;

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// An application with the project commands registered, exactly as `run()` does.
fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(sync_lib::memory::MemorySessions::default())
        .invoke_handler(tauri::generate_handler![
            sync_lib::project::project_probe,
            sync_lib::project::project_initialize_repository,
            sync_lib::project::project_settings_load,
            sync_lib::project::project_settings_save,
        ])
        .build(mock_context(noop_assets()))
        .expect("the mock application should build");
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the mock window should build");
    (app, window)
}

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, arguments: Value) -> Value {
    let response = get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.to_owned(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("a local origin"),
            body: InvokeBody::Json(arguments),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        },
    );
    match response.expect("the command should succeed") {
        InvokeResponseBody::Json(json) => serde_json::from_str(&json).expect("a JSON response"),
        InvokeResponseBody::Raw(_) => panic!("the project commands answer in JSON"),
    }
}

/// The same call, for the answers that come back as a refusal.
///
/// [`invoke`] unwraps, because every other test here asks for something that
/// works. A refusal travels on the error callback rather than the response one,
/// so it needs a way in of its own rather than a panic.
fn invoke_refusal(window: &WebviewWindow<MockRuntime>, command: &str, arguments: Value) -> Value {
    let response = get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.to_owned(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("a local origin"),
            body: InvokeBody::Json(arguments),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        },
    );
    response.expect_err("the command should be refused")
}

fn git_is_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The settings commands talk to a real sidecar, the same way the memory tests
/// do. Without one they say so and pass rather than failing on a machine that
/// simply has not built it.
fn engine_is_available() -> bool {
    common::sidecar_is_available()
}

#[test]
fn probing_a_plain_folder_reports_no_repository() {
    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");

    let probe = invoke(
        &window,
        "project_probe",
        json!({ "path": path.to_string_lossy() }),
    );

    assert_eq!(probe["repositoryRoot"], Value::Null);
    assert_eq!(probe["name"], name_of(&path));
}

#[test]
fn initializing_makes_the_folder_its_own_repository() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }

    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");

    let probe = invoke(
        &window,
        "project_initialize_repository",
        json!({ "path": path.to_string_lossy() }),
    );

    assert_eq!(probe["repositoryRoot"], probe["path"]);
    assert!(path.join(".git").exists());
}

#[test]
fn a_folder_inside_a_repository_reports_the_repository_root() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }

    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("a real path");
    invoke(
        &window,
        "project_initialize_repository",
        json!({ "path": root.to_string_lossy() }),
    );

    let nested = root.join("packages/app");
    std::fs::create_dir_all(&nested).expect("the nested folder should be created");

    let probe = invoke(
        &window,
        "project_probe",
        json!({ "path": nested.to_string_lossy() }),
    );

    assert_eq!(probe["repositoryRoot"], root.to_string_lossy().as_ref());
    assert_ne!(probe["path"], probe["repositoryRoot"]);
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .expect("a temporary folder has a name")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_repository_with_no_project_record_has_nothing_to_answer_with() {
    if !git_is_installed() || !engine_is_available() {
        eprintln!("skipping: git is not installed, or there is no sync-mcp built");
        return;
    }

    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");
    invoke(
        &window,
        "project_initialize_repository",
        json!({ "path": path.to_string_lossy() }),
    );

    let probe = invoke(
        &window,
        "project_settings_load",
        json!({ "project": path.to_string_lossy() }),
    );

    assert_eq!(probe["settings"], Value::Null);
    assert_eq!(
        probe["memoryError"],
        Value::Null,
        "memory answered; it just had nothing to say: {probe}"
    );
}

/// An identifier is written once and edited never, so a value the window would
/// not have derived is refused rather than tidied up: there is no later moment
/// at which somebody could unlearn it.
#[test]
fn a_project_is_refused_an_identifier_the_window_would_not_have_derived() {
    if !git_is_installed() || !engine_is_available() {
        eprintln!("skipping: git or the engine is unavailable");
        return;
    }
    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");
    Command::new("git")
        .arg("init")
        .arg(&path)
        .output()
        .expect("git init");

    for refused in ["atlas", "ATLAS-", "ATLAS ONE", ""] {
        let error = invoke_refusal(
            &window,
            "project_settings_save",
            json!({
                "project": path.to_string_lossy(),
                "settings": {
                    "name": "Atlas",
                    "identifier": refused,
                    "description": "",
                    "language": "en"
                },
            }),
        );
        assert_eq!(
            error["kind"], "invalid_identifier",
            "`{refused}` should be refused, got {error}"
        );
    }
}

#[test]
fn what_a_new_project_is_asked_is_what_it_answers_with_next_time() {
    if !git_is_installed() || !engine_is_available() {
        eprintln!("skipping: git is not installed, or there is no sync-mcp built");
        return;
    }

    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");
    invoke(
        &window,
        "project_initialize_repository",
        json!({ "path": path.to_string_lossy() }),
    );

    invoke(
        &window,
        "project_settings_save",
        json!({
            "project": path.to_string_lossy(),
            "settings": {
                "name": "Atlas",
                "identifier": "ATLAS",
                "description": "The engine.",
                "language": "de"
            },
        }),
    );

    let probe = invoke(
        &window,
        "project_settings_load",
        json!({ "project": path.to_string_lossy() }),
    );

    assert_eq!(probe["settings"]["name"], "Atlas");
    assert_eq!(probe["settings"]["identifier"], "ATLAS");
    assert_eq!(probe["settings"]["description"], "The engine.");
    assert_eq!(probe["settings"]["language"], "de");
}

/// What a project declares it is composed of, written and read back whole.
///
/// The prompt is the member worth a test of its own. It is the only way an
/// extension reaches an agent — the MCP server has no view of the catalogue the
/// extension came from — and it crosses the same three vocabularies every other
/// new field crosses on the way in. A member none of the layers between models
/// is dropped in silence, and the save still answers success.
#[test]
fn a_project_declares_its_extensions_and_what_each_tells_an_agent() {
    if !git_is_installed() || !engine_is_available() {
        eprintln!("skipping: git is not installed, or there is no sync-mcp built");
        return;
    }

    let (_app, window) = app();
    let folder = tempfile::tempdir().expect("a temporary folder");
    let path = folder.path().canonicalize().expect("a real path");
    invoke(
        &window,
        "project_initialize_repository",
        json!({ "path": path.to_string_lossy() }),
    );

    invoke(
        &window,
        "project_settings_save",
        json!({
            "project": path.to_string_lossy(),
            "settings": {
                "name": "Atlas",
                "identifier": "ATLAS",
                "description": "The engine.",
                "language": "en",
                "installed": [
                    {
                        "id": "records",
                        "version": "1.0.0",
                        "integrity": "9c327d33c74bf97dffb5ff59c52f3a0f30f26bf738e92ed20bbd21d08d33eecc",
                        "source": "file",
                    },
                    {
                        "id": "project-memory",
                        "version": "1.0.0",
                        "prompt": "# Project memory\n\nSeven kinds of claim.",
                        "source": "folder",
                    },
                ],
            },
        }),
    );

    let probe = invoke(
        &window,
        "project_settings_load",
        json!({ "project": path.to_string_lossy() }),
    );
    let installed = &probe["settings"]["installed"];

    assert_eq!(installed[0]["id"], "records");
    assert!(
        installed[0].get("prompt").is_none(),
        "an extension with nothing to say stores nothing: {installed}"
    );
    assert_eq!(installed[1]["id"], "project-memory");
    assert_eq!(
        installed[1]["prompt"], "# Project memory\n\nSeven kinds of claim.",
        "the prompt is what reaches the agent, so it has to survive the round trip: {installed}"
    );

    // The two that make the declaration a lockfile rather than a wish. A
    // version can be re-tagged; a digest cannot, so what a colleague resolves
    // on their own machine is the same bytes rather than the same number.
    assert_eq!(
        installed[0]["integrity"],
        "9c327d33c74bf97dffb5ff59c52f3a0f30f26bf738e92ed20bbd21d08d33eecc",
        "the digest is what a re-tagged release is caught by: {installed}"
    );
    assert_eq!(installed[0]["source"], "file");

    // A folder has no fixed content to hash, and that absence is stored as an
    // absence rather than as an empty string somebody could mistake for one.
    assert_eq!(installed[1]["source"], "folder");
    assert!(
        installed[1].get("integrity").is_none(),
        "a package being written in has nothing to lock to: {installed}"
    );
}
