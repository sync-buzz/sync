//! A handler's own door out, over the real network.
//!
//! Ignored by default, for the reason `sync-extensions`' `live_net.rs` is:
//! `cargo test` is a statement about this code, and this file is a statement
//! about GitHub. It earns its place because everything beside it proves what is
//! *refused* — a door that answered nothing at all would pass every one of
//! those tests.
//!
//! What it is really about is the shape of the answer. A handler `await`s, the
//! thread of its isolate sits inside Rust until the response arrives, and the
//! promise has to settle to what came back rather than to `{}` — which is what
//! a promise handed across unsettled serialises as, with no error anywhere.
//!
//! ```text
//! cargo test --test live_handler_net -- --ignored --nocapture
//! ```
#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::{Value, json};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// The host to read from, and it is the one the Issues package declares.
const DECLARED: &str = "api.github.com";
const ID: &str = "probe-handlers-live-net";

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(sync_lib::memory::MemorySessions::default())
        .manage(sync_lib::work::WorkFile::default())
        .invoke_handler(tauri::generate_handler![
            sync_lib::extensions::extension_install_folder,
            sync_lib::extensions::extension_forget,
            sync_lib::handlers::extension_handler_call,
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
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("a local origin"),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: tauri::http::header::HeaderMap::new(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );
    response.map(|body| match body {
        tauri::ipc::InvokeResponseBody::Json(text) => {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        }
        tauri::ipc::InvokeResponseBody::Raw(bytes) => {
            Value::String(String::from_utf8_lossy(&bytes).into_owned())
        }
    })
}

fn write(at: std::path::PathBuf, content: &str) {
    std::fs::create_dir_all(at.parent().expect("a parent")).expect("a directory");
    std::fs::write(at, content).expect("writes");
}

/// Reads a public repository with the permission that names the host.
///
/// `rust-lang/rust` because it is public, old, and certain to answer a machine
/// with no account. The assertions are on the shape of what came back rather
/// than on anything anybody wrote in it.
#[test]
#[ignore = "talks to GitHub"]
fn a_handler_that_awaited_a_request_answers_what_came_back() {
    let folder = tempfile::tempdir().expect("a directory");
    write(
        folder.path().join("manifest.json"),
        &json!({
            "manifestVersion": 1,
            "id": ID,
            "version": "1.0.0",
            "name": "Live network probe",
            "engines": { "syncApi": "^2.0" },
            "capabilities": ["background", "net"],
            "net": { "hosts": [DECLARED] },
            "service": "service/index.js",
            "lifecycle": { "installed": "probe.installed" },
        })
        .to_string(),
    );
    write(
        folder.path().join("service/index.js"),
        &format!(
            r#"
            export default function register() {{
              return {{
                "probe.installed": async () => {{
                  const answer = JSON.parse(__syncHost__("net.fetch", JSON.stringify({{
                    url: "https://{DECLARED}/repos/rust-lang/rust",
                    headers: {{ "user-agent": "Sync" }},
                  }})));
                  return {{ status: answer.status, ok: answer.ok, named: JSON.parse(answer.body).full_name }};
                }},
              }};
            }}
            "#
        ),
    );

    let (_app, webview) = app();
    invoke(
        &webview,
        "extension_install_folder",
        json!({ "path": folder.path().to_string_lossy() }),
    )
    .expect("the folder installs");

    let answer = invoke(
        &webview,
        "extension_handler_call",
        json!({
            "project": "/nowhere",
            "id": ID,
            "occasion": "installed",
            "payload": {},
        }),
    )
    .expect("the handler ran");

    assert_eq!(answer["status"], json!(200), "GitHub answered: {answer}");
    assert_eq!(answer["ok"], json!(true));
    assert_eq!(
        answer["named"],
        json!("rust-lang/rust"),
        "the body settled into the handler's answer rather than an empty object: {answer}"
    );

    invoke(&webview, "extension_forget", json!({ "id": ID })).expect("it is forgotten");
}
