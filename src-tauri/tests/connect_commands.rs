#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Connecting an agent, driven the way the settings window drives it.
//!
//! Through Tauri's IPC with a mock runtime, against real files on disk — a
//! command that writes into somebody's configuration is a command whose test
//! should be able to open the file afterwards and read it.
//!
//! Nothing here writes. Every client Sync knows now keeps its configuration in
//! the person's home directory — one server on this machine, one entry per
//! client — so a test that connected would edit the configuration of whoever
//! ran it. What *can* be checked here is the half that only reads: the rows the
//! settings window shows, and the refusal for a name Sync does not know.
//!
//! The half that writes is checked in `connect.rs` itself, against text rather
//! than files: the six clients disagree about the shape of one entry, and that
//! disagreement needs no filesystem to be wrong.

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            sync_lib::connect::agents_list,
            sync_lib::connect::agent_connect,
            sync_lib::connect::agent_disconnect,
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
    fn read(body: InvokeResponseBody) -> Value {
        match body {
            InvokeResponseBody::Json(text) => {
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            }
            InvokeResponseBody::Raw(_) => panic!("these commands answer in JSON"),
        }
    }
    response.map(read)
}

/// The row for one client out of the list.
#[test]
fn every_client_is_listed_with_the_file_it_keeps_sync_in() {
    let (_app, webview) = app();

    let rows = invoke(&webview, "agents_list", json!({})).expect("the rows are read");
    let rows = rows.as_array().expect("a list of rows");
    assert_eq!(rows.len(), 7, "every client Sync knows: {rows:?}");

    for row in rows {
        assert_eq!(
            row["scope"], "installation",
            "no client is connected per project any more: {row}"
        );
        let configuration = row["configuration"].as_str().expect("a file");
        assert!(
            !configuration.starts_with(".mcp")
                && !configuration.starts_with(".cursor")
                && !configuration.starts_with(".vscode"),
            "and none of them is a file inside a repository: {configuration}"
        );
    }

    let named: Vec<&str> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    for known in [
        "claude-code",
        "codex-cli",
        "cursor",
        "grok-cli",
        "vscode",
        "claude-desktop",
        "zed",
    ] {
        assert!(named.contains(&known), "`{known}` is listed: {named:?}");
    }

    // The section reads the list in the order it arrives, so a row's heading is
    // the order rather than a sort in the window. Two copies of that ordering
    // would let the headings and the rows under them drift apart.
    let groups: Vec<&str> = rows
        .iter()
        .filter_map(|row| row["group"].as_str())
        .collect();
    let mut seen: Vec<&str> = Vec::new();
    for group in &groups {
        if seen.last() != Some(group) {
            assert!(
                !seen.contains(group),
                "`{group}` is listed in one run, not scattered: {groups:?}"
            );
            seen.push(group);
        }
    }
    assert_eq!(seen, ["command_line", "desktop", "editor"], "{groups:?}");
}

#[test]
fn an_agent_sync_does_not_know_is_refused_by_name() {
    let (_app, webview) = app();

    let refused = invoke(
        &webview,
        "agent_connect",
        json!({"agent": "some-other-editor"}),
    )
    .expect_err("an unknown agent is refused");
    assert_eq!(refused["kind"], "unknown_agent", "{refused}");
}
