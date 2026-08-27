#![allow(clippy::expect_used, clippy::unwrap_used)]

//! A package reaching a project, through the commands the window calls.
//!
//! This is the chain the extension migration turns on, and every link in it was
//! a constant in the window until now: a folder somebody is writing becomes a
//! package, the package's own files say what it publishes, and the project's
//! memory ends up holding those definitions. Nothing here names an extension —
//! the vocabulary comes out of files the test wrote a moment earlier, which is
//! the whole point.
//!
//! Two tests, because they fail for different reasons and only one needs an
//! engine. The first is about the boundary between the window and the desktop
//! layer, where a new field goes missing without an error: it asserts on the
//! JSON that actually crosses. The second carries that same JSON on into the
//! engine and reads back what landed.

use std::path::Path;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

mod common;

/// The id each test's fixture package installs under.
///
/// Fixed rather than random so that a run interrupted halfway leaves one stale
/// pointer that the next run overwrites, instead of a directory that fills up —
/// and **one per test**, which is the half of the rule this file was missing.
/// The artefact store is this machine's and every test in the binary shares it,
/// so two tests installing under one name and forgetting it are two tests
/// racing: the second `extension_forget` finds the pointer already gone and
/// fails, in a different run each time. It failed about one full workspace run
/// in two, and passed every time this file was run on its own. The third test
/// below already knew this; the first two did not.
const CROSSES: &str = "probe-vocabulary";
const LANDS: &str = "probe-vocabulary-lands";

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(sync_lib::memory::MemorySessions::default())
        .invoke_handler(tauri::generate_handler![
            sync_lib::extensions::extension_install_folder,
            sync_lib::extensions::extension_list,
            sync_lib::extensions::extension_forget,
            sync_lib::memory::memory_open,
            sync_lib::memory::memory_extension_types_publish,
            sync_lib::memory::memory_types,
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

/// A package that draws nothing: two types and a prompt, and no code at all.
///
/// The case worth fixturing, because it is the one an extension system usually
/// gets wrong. An extension is not necessarily a screen, and this one's whole
/// contribution reaches a project without a line of it being executed.
fn package(root: &Path, id: &str) {
    write(
        root.join("manifest.json"),
        &json!({
            "manifestVersion": 1,
            "id": id,
            "version": "1.0.0",
            "name": "Probe vocabulary",
            "engines": { "syncApi": "^1.0" },
            "capabilities": ["records"],
            "types": ["types/decision.json", "types/question.json"],
            "prompt": "prompt/instructions.md"
        })
        .to_string(),
    );

    write(
        root.join("types/decision.json"),
        &json!({
            "kind": format!("{id}.decision"),
            "title": "Decision",
            "description": "A choice that was made.",
            "icon": "signpost",
            "guidance": "One decision per record.",
            "relationships": {
                "references": { "target": "any", "description": "What it rests on." }
            }
        })
        .to_string(),
    );

    write(
        root.join("types/question.json"),
        &json!({
            "kind": format!("{id}.question"),
            "title": "Question",
            "description": "Something nobody has settled yet.",
            "icon": "circle-help",
            "fields": {
                "status": {
                    "type": "enum",
                    "values": ["open", "answered"],
                    "required": true,
                    "default": "open",
                    "description": "Whether it is still a fork."
                }
            }
        })
        .to_string(),
    );

    write(
        root.join("prompt/instructions.md"),
        "# Write it down\n\nOne claim per record.\n",
    );
}

fn write(at: std::path::PathBuf, content: &str) {
    std::fs::create_dir_all(at.parent().expect("a parent")).expect("a directory");
    std::fs::write(at, content).expect("writes");
}

/// The fixture, installed and found in the list the window reads.
fn installed(webview: &WebviewWindow<MockRuntime>, folder: &Path, id: &str) -> Value {
    invoke(
        webview,
        "extension_install_folder",
        json!({ "path": folder.to_string_lossy() }),
    )
    .expect("the folder installs");

    let listed = invoke(webview, "extension_list", json!({})).expect("the list reads");
    listed
        .as_array()
        .expect("a list")
        .iter()
        .find(|entry| entry["manifest"]["id"] == id)
        .cloned()
        .expect("the package this test just installed is in it")
}

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temp project");
    let output = std::process::Command::new("git")
        .arg("init")
        .arg(directory.path())
        .output()
        .expect("git init");
    assert!(output.status.success());
    directory
}

#[test]
fn what_a_package_publishes_crosses_to_the_window_whole() {
    let folder = tempfile::tempdir().expect("a directory");
    package(folder.path(), CROSSES);
    let (_app, webview) = app();

    let entry = installed(&webview, folder.path(), CROSSES);

    assert!(
        entry["defect"].is_null(),
        "nothing is wrong with it: {entry}"
    );
    assert!(
        entry["ui"].is_null(),
        "it ships no module, and that is an answer rather than an omission: {entry}"
    );
    assert!(
        entry["styles"].is_null(),
        "and no stylesheet either, for the same reason: {entry}"
    );
    assert_eq!(entry["pointer"]["source"], "folder");
    assert!(
        entry["pointer"]["integrity"].is_null(),
        "a folder has no fixed content to hash: {entry}"
    );
    assert_eq!(
        entry["prompt"].as_str(),
        Some("# Write it down\n\nOne claim per record.\n"),
        "the prose is carried whole rather than summarised"
    );

    // The part that would go missing quietly. Each of these is a field an
    // author wrote in a file, and the window forwards them to the engine
    // untouched — so a member dropped in serialisation is a declaration that
    // silently stops existing.
    let types = entry["types"].as_array().expect("the types it publishes");
    assert_eq!(types.len(), 2, "{entry}");

    let question = types
        .iter()
        .find(|type_| type_["kind"] == format!("{CROSSES}.question"))
        .expect("the question type");
    assert_eq!(question["title"], "Question");
    assert_eq!(question["icon"], "circle-help");
    assert_eq!(question["fields"]["status"]["default"], "open");
    assert_eq!(question["fields"]["status"]["values"][1], "answered");

    let decision = types
        .iter()
        .find(|type_| type_["kind"] == format!("{CROSSES}.decision"))
        .expect("the decision type");
    assert_eq!(decision["guidance"], "One decision per record.");
    assert_eq!(decision["relationships"]["references"]["target"], "any");

    invoke(&webview, "extension_forget", json!({ "id": CROSSES })).expect("it is forgotten");
}

#[test]
fn the_vocabulary_a_package_carries_lands_in_the_project() {
    if !common::sidecar_is_available() {
        eprintln!("{}", common::NO_SIDECAR);
        return;
    }
    let folder = tempfile::tempdir().expect("a directory");
    package(folder.path(), LANDS);
    let project = repository();
    let project_path = project.path().to_string_lossy().into_owned();
    let (_app, webview) = app();

    let entry = installed(&webview, folder.path(), LANDS);
    invoke(&webview, "memory_open", json!({ "project": project_path }))
        .expect("opening memory succeeds");

    // Verbatim, exactly as the window forwards it: the point of the package's
    // files being written in the engine's own shape is that nothing translates
    // them on the way, so a test that reshaped them here would be testing a
    // path the product does not take.
    invoke(
        &webview,
        "memory_extension_types_publish",
        json!({ "project": project_path, "types": entry["types"] }),
    )
    .expect("the types publish");

    let types = invoke(&webview, "memory_types", json!({ "project": project_path }))
        .expect("the types read back");
    let held = types.as_array().expect("a list");

    let question = held
        .iter()
        .find(|type_| type_["kind"] == format!("{LANDS}.question"))
        .expect("the question type is in the project");
    assert_eq!(question["title"], "Question");
    assert_eq!(
        question["fields"]["status"]["values"][0], "open",
        "the declaration landed as written: {question}"
    );

    let decision = held
        .iter()
        .find(|type_| type_["kind"] == format!("{LANDS}.decision"))
        .expect("the decision type is in the project");
    assert_eq!(
        decision["guidance"], "One decision per record.",
        "what an agent is told before writing one travels with the definition: {decision}"
    );
    assert_eq!(decision["relationships"]["references"]["target"], "any");

    invoke(&webview, "extension_forget", json!({ "id": LANDS })).expect("it is forgotten");
}

/// A package that draws carries its own rules, and the URL for them crosses.
///
/// Its own, because the window's stylesheet holds only what the window's own
/// source uses: Tailwind generates the classes it finds, the build reads `src`,
/// and a package is not in it. Every utility an extension used that the shell
/// did not happen to use as well produced no rule at all — and nothing anywhere
/// said so, which is how Chat lost its proportions for a fortnight while every
/// file anybody opened looked correct.
///
/// The assertion is on the JSON rather than on the Rust value because this is
/// the boundary a new field goes missing at without an error: `styles` reaching
/// the window is the whole of what makes the sheet load.
///
/// Its own id rather than either of the two above, because a pointer is keyed by id in a store
/// this machine shares between runs — two tests installing different packages
/// under one name is each of them reading the other's manifest.
#[test]
fn a_stylesheet_crosses_to_the_window_as_a_url() {
    const DRAWS: &str = "probe-styles";

    let folder = tempfile::tempdir().expect("a directory");
    let root = folder.path();
    write(
        root.join("manifest.json"),
        &json!({
            "manifestVersion": 1,
            "id": DRAWS,
            "version": "1.0.0",
            "name": "Probe styles",
            "engines": { "syncApi": "^1.0" },
            "areas": [{ "id": "probe", "label": "Probe", "frame": "browse" }],
            "ui": "ui/index.js",
            "styles": "ui/index.css",
        })
        .to_string(),
    );
    write(root.join("ui/index.js"), "export default () => ({});\n");
    write(
        root.join("ui/index.css"),
        ".gap-1\\.5{gap:calc(var(--spacing)*1.5)}\n",
    );

    let (_app, webview) = app();
    invoke(
        &webview,
        "extension_install_folder",
        json!({ "path": root.to_string_lossy() }),
    )
    .expect("the folder installs");

    let listed = invoke(&webview, "extension_list", json!({})).expect("the list reads");
    let entry = listed
        .as_array()
        .expect("a list")
        .iter()
        .find(|entry| entry["manifest"]["id"] == DRAWS)
        .cloned()
        .expect("the package this test just installed is in it");

    assert_eq!(
        entry["styles"].as_str(),
        Some(format!("syncext://{DRAWS}/ui/index.css").as_str()),
        "the window never builds this string itself: {entry}"
    );
    assert_eq!(
        entry["ui"].as_str(),
        Some(format!("syncext://{DRAWS}/ui/index.js").as_str()),
        "and the module is served the same way: {entry}"
    );

    invoke(&webview, "extension_forget", json!({ "id": DRAWS })).expect("it is forgotten");
}
