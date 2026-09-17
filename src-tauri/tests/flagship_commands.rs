#![allow(clippy::expect_used, clippy::unwrap_used)]

//! The data door, driven the way the window drives it.
//!
//! Through Tauri's IPC with a mock runtime, because the two things worth asking
//! here are both about the boundary and neither is visible from inside Rust.
//!
//! **Does the ask survive `invoke`.** A member spelled one way in `serde` and
//! another in TypeScript arrives as nothing at all, with no error anywhere —
//! `flagship_call` would then be asked for a tool with no name and would refuse
//! for the wrong reason. What separates the two cases is the shape of the
//! rejection: a command whose arguments did not deserialize is refused by Tauri
//! with a sentence, and one that ran is refused by this application with a
//! `kind` on it.
//!
//! **Does the turn stay out of the conversations.** The registry those are
//! drawn from is managed here and read back afterwards through the same command
//! the window reads it with.
//!
//! No agent is raised, and none can be: every ask below names a server no
//! configuration has, and that is refused before anything is launched. Raising
//! one for real would mean an agent CLI installed on whatever machine runs the
//! suite, and it is watched on a running application instead.

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// A server name no agent's configuration can hold, so that the refusal below
/// never depends on what the machine running the suite has installed.
const NO_SUCH_SERVER: &str = "a-server-nobody-has-3f9c1e";

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(sync_lib::sessions::live::Sessions::default())
        .invoke_handler(tauri::generate_handler![
            sync_lib::flagship::flagship_call,
            sync_lib::sessions::session_live,
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

/// One ask, in the spelling the window sends.
fn ask() -> Value {
    json!({
        "project": "/tmp",
        "ask": {
            "server": NO_SUCH_SERVER,
            "tool": "status",
            "arguments": { "verbose": true },
        },
    })
}

#[test]
fn the_ask_crosses_the_boundary_and_the_refusal_is_named() {
    let (_app, webview) = app();

    let refused = invoke(&webview, "flagship_call", ask())
        .expect_err("no machine has that server, so this cannot be an answer");

    // An object with a `kind` is this application refusing. A string would mean
    // the arguments never deserialized, and the window would be branching on a
    // sentence that has nothing to do with what it asked for.
    let kind = refused
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the refusal carries no kind: {refused}"));
    assert!(
        !kind.is_empty(),
        "an unnamed refusal is one nothing can branch on",
    );
    assert!(
        refused
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| !message.is_empty()),
        "the refusal has nothing to show anybody: {refused}",
    );
}

/// An ask missing a member is refused by Tauri before the command runs — which
/// is what makes the assertion above mean something. Without this, a rejection
/// carrying a `kind` and one carrying a sentence would be indistinguishable to
/// a reader of that test.
#[test]
fn an_ask_that_is_not_one_never_reaches_the_command() {
    let (_app, webview) = app();

    let refused = invoke(&webview, "flagship_call", json!({ "project": "/tmp" }))
        .expect_err("an ask with no tool in it is not an ask");
    assert!(
        refused.get("kind").is_none(),
        "this should never have reached the command: {refused}",
    );
}

/// The criterion the whole door is shaped around: the turn is not a
/// conversation, and nothing about it joins the list of them.
///
/// By construction rather than by a filter — the command reaches neither the
/// registry nor the file of resumable conversations — so what this pins is that
/// the construction has not quietly changed. Someone routing the errand through
/// `sessions::open` for the adapter handling it already has would turn this
/// red.
#[test]
fn a_call_leaves_the_conversations_alone() {
    let (_app, webview) = app();

    let before = invoke(&webview, "session_live", json!({})).expect("the list answers");
    assert_eq!(before, json!([]), "nothing has been opened yet");

    let _ = invoke(&webview, "flagship_call", ask());

    let after = invoke(&webview, "session_live", json!({})).expect("the list answers");
    assert_eq!(after, json!([]), "the errand joined the conversations");
}
