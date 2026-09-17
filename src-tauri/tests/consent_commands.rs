#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Agreements, driven the way the package's page and the settings window drive
//! them.
//!
//! Through Tauri's IPC with a mock runtime, because both things worth asking
//! here happen at that boundary and neither is visible from inside Rust.
//!
//! **Do the arguments survive `invoke`.** A member spelled one way in `serde`
//! and another in TypeScript arrives as nothing at all, and an agreement
//! assembled from nothing is a row that can never match: every call the row
//! somebody pressed meant to allow would go on being refused, with no error
//! anywhere to say why.
//! An empty server is exactly that row, which is why it is refused rather than
//! kept.
//!
//! **Does an agreement outlive the application.** The second half of the test
//! below builds a second application over the same files and asks it, which is
//! the closest a suite gets to quitting and reopening — an agreement held in
//! memory would pass every assertion up to that line and fail from there.
//!
//! No agent is raised and none can be: nothing here calls a tool. What is under
//! test is the record of what somebody agreed to, which is written and read
//! with no agent anywhere.

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// The id this test agrees on behalf of.
///
/// Nothing on this machine serves it, which is deliberate: an agreement names a
/// package rather than requiring one, so the one thing this must not need is an
/// artefact on the disk.
const PANEL: &str = "probe-consent-panel";

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            sync_lib::consent::tool_consent_status,
            sync_lib::consent::tool_consent_grant,
            sync_lib::consent::tool_consent_revoke,
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

/// What that package may reach, as the list reports it.
fn servers_of(listed: &Value, id: &str) -> Vec<String> {
    listed
        .as_array()
        .expect("a list of packages")
        .iter()
        .find(|held| held["id"] == id)
        .map(|held| {
            held["servers"]
                .as_array()
                .expect("a list of servers")
                .iter()
                .map(|server| server.as_str().expect("a server key").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The whole round: a row on a page agrees, the settings window reads it back, a
/// second application still has it, and withdrawing one leaves the other
/// standing.
///
/// One test rather than four, because the four share one file on this machine
/// and four tests sharing one file are four tests racing each other. It cleans
/// up after itself for the same reason the neighbouring suite forgets what it
/// installed: what is written here is written where the application writes, not
/// into a temporary directory.
#[test]
fn what_somebody_agreed_to_is_what_a_later_launch_allows() {
    let (_app, webview) = app();

    // Before agreeing to anything, because the file is this machine's own and a
    // run that failed half way through left its rows in it. Withdrawing is the
    // only way to start from nothing: agreeing is additive, so a leftover row
    // would be read as part of what this test just asked for.
    forget(&webview);

    let listed = invoke(
        &webview,
        "tool_consent_grant",
        json!({ "extension": PANEL, "server": "somewhere" }),
    )
    .expect("a card may agree");
    assert_eq!(
        servers_of(&listed, PANEL),
        vec!["somewhere"],
        "the agreement came back as it was given: {listed}",
    );

    // A second application over the same files, which is what quitting and
    // reopening leaves behind.
    let (_second, reopened) = app();
    let listed = invoke(&reopened, "tool_consent_status", json!({})).expect("the list is drawn");
    assert_eq!(
        servers_of(&listed, PANEL),
        vec!["somewhere"],
        "an agreement did not survive the application: {listed}",
    );
    let row = listed
        .as_array()
        .expect("a list")
        .iter()
        .find(|held| held["id"] == PANEL)
        .expect("the package that was agreed to");
    assert_eq!(
        row["installed"], false,
        "nothing serves this id, and the row says so rather than hiding: {row}",
    );
    assert_eq!(
        row["name"], PANEL,
        "with no package to read a name from, the id is what a person sees: {row}",
    );

    // A second server is a second agreement and not a replacement of the first.
    // The rows are pressed one at a time, so a second one that quietly took the
    // first away would be a withdrawal nothing on the screen asked for.
    let listed = invoke(
        &reopened,
        "tool_consent_grant",
        json!({ "extension": PANEL, "server": "elsewhere" }),
    )
    .expect("another server may be agreed to");
    assert_eq!(
        servers_of(&listed, PANEL),
        vec!["somewhere", "elsewhere"],
        "the first agreement is still standing: {listed}",
    );

    let listed = invoke(
        &reopened,
        "tool_consent_revoke",
        json!({ "extension": PANEL, "server": "elsewhere" }),
    )
    .expect("an agreement may be withdrawn");
    assert_eq!(
        servers_of(&listed, PANEL),
        vec!["somewhere"],
        "one was withdrawn and the other was not: {listed}",
    );

    forget(&reopened);
    let listed = invoke(&reopened, "tool_consent_status", json!({})).expect("the list is drawn");
    assert!(
        servers_of(&listed, PANEL).is_empty(),
        "the test leaves this machine as it found it: {listed}",
    );
}

/// Withdraw everything this test could have agreed to, whether it did or not.
///
/// Both servers by name rather than a loop over what is listed: the point is to
/// leave nothing of this test's behind, and reading the list first would make
/// the cleanup depend on the thing that just failed.
fn forget(webview: &WebviewWindow<MockRuntime>) {
    for server in ["somewhere", "elsewhere"] {
        invoke(
            webview,
            "tool_consent_revoke",
            json!({ "extension": PANEL, "server": server }),
        )
        .expect("withdrawing something nobody agreed to is not a failure");
    }
}

/// An agreement that names no server is refused rather than stored.
///
/// Stored, it would be a row nothing can ever match and nobody can read as a
/// mistake — the card would look like it had agreed to something, and every
/// call would go on being refused.
#[test]
fn an_agreement_that_names_nothing_is_refused() {
    let (_app, webview) = app();

    let refused = invoke(
        &webview,
        "tool_consent_grant",
        json!({ "extension": PANEL, "server": "" }),
    )
    .expect_err("an agreement to reach nothing is not an agreement");

    assert_eq!(
        refused["kind"], "nothing_named",
        "a card branches on the kind, not on the sentence: {refused}"
    );
}
