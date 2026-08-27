#![allow(clippy::expect_used, clippy::unwrap_used)]

//! What a list of conversations says about who started them.
//!
//! One question, driven through Tauri's IPC rather than by calling the
//! function: **does the source survive `invoke`**. It has to be asked that way
//! because nothing on this boundary fails loudly. `serde` renames on one side
//! and TypeScript describes the other, neither knows about the first, and a
//! field spelled differently arrives as `undefined` — so the list goes back to
//! saying every conversation was somebody's, with nothing anywhere reporting a
//! problem.
//!
//! No agent is raised. A session is a value, and `Sessions` is a registry of
//! them, so the row can be produced from a session that was never connected to
//! anything — which is the whole reason this is affordable as a test at all.
//! Raising an agent for real means `npm install` on whatever machine runs the
//! suite, and that is watched on a running application instead.
//!
//! There is no test that a caller cannot claim a source it did not earn,
//! because no caller can: the field reaches a session through `Session::new`,
//! which is reached through `sessions::open`, and the only caller that passes
//! one is `raise_for_work` — which builds it from the package and handler the
//! host itself resolved. No command takes the field, so a window cannot say a
//! conversation was ordered by an extension.

use serde_json::Value;
use sync_lib::sessions::live::{Session, Sessions, Source};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

fn app() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(Sessions::default())
        .invoke_handler(tauri::generate_handler![sync_lib::sessions::session_live])
        .build(mock_context(noop_assets()))
        .expect("the mock application builds");
    let webview = WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("a webview to invoke from");
    (app, webview)
}

fn listed(webview: &WebviewWindow<MockRuntime>) -> Value {
    let response = get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: "session_live".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("a local origin"),
            body: tauri::ipc::InvokeBody::Json(Value::Null),
            headers: tauri::http::header::HeaderMap::new(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect("the list answers");
    match response {
        tauri::ipc::InvokeResponseBody::Json(text) => {
            serde_json::from_str(&text).expect("a list of rows")
        }
        tauri::ipc::InvokeResponseBody::Raw(_) => panic!("the list crosses as JSON"),
    }
}

/// The field step 5 exists for, read back the way the window reads it.
///
/// `src/lib/agent-sessions/client.ts` declares `source?: SessionSource` with
/// `extension`, `handler` and `about`. These are those names, and nothing but
/// this run of the real command produces them.
#[test]
fn a_conversation_an_extension_ordered_says_so_across_the_boundary() {
    let (app, webview) = app();
    let sessions = tauri::Manager::state::<Sessions>(&app);
    sessions.insert(Session::new(
        sessions.mint_key(),
        "claude".to_owned(),
        "Claude Code".to_owned(),
        std::env::temp_dir(),
        Some(Source {
            work: "w1787673512158-1".to_owned(),
            extension_id: "issues".to_owned(),
            extension_name: "Issues".to_owned(),
            handler: "issues.poll".to_owned(),
            about: Some("issue-4c1a".to_owned()),
        }),
    ));

    let rows = listed(&webview);
    let row = &rows[0];
    assert_eq!(
        row["source"]["extensionId"], "issues",
        "the window filters its own orders by this, and reads it under this name: {row}"
    );
    assert_eq!(row["source"]["handler"], "issues.poll", "{row}");
    assert_eq!(
        row["source"]["work"], "w1787673512158-1",
        "and which of that handler's orders it was, which is all three rows of a busy          handler have to tell them apart: {row}"
    );
    assert_eq!(row["source"]["about"], "issue-4c1a", "{row}");
}

/// And the ordinary case, which is most of them.
///
/// A person's conversation carries **no** `source` member rather than a null
/// one, because the window's type says the field is optional. A `null` here
/// would still read as absent in TypeScript, so this is not about correctness
/// so much as about the two sides agreeing on what a conversation nobody
/// ordered looks like — and about not spending a member on every row to say
/// nothing.
#[test]
fn a_conversation_a_person_started_says_nothing_about_a_source() {
    let (app, webview) = app();
    let sessions = tauri::Manager::state::<Sessions>(&app);
    sessions.insert(Session::new(
        sessions.mint_key(),
        "claude".to_owned(),
        "Claude Code".to_owned(),
        std::env::temp_dir(),
        None,
    ));

    let row = listed(&webview)[0].clone();
    assert!(
        row.get("source").is_none(),
        "absent, not null, and not an empty object: {row}"
    );
    assert_eq!(
        row["agentId"], "claude",
        "and the rest of the row is unaffected: {row}"
    );
}

/// Both kinds in one list, which is what Chat is actually handed.
///
/// The list is the only place a session nobody in this window started is
/// visible at all, so the two have to arrive together and be told apart. This
/// is that, and it is also what an extension does to find its own work: match
/// on `source.extension` and ignore the rest.
#[test]
fn one_list_carries_both_and_they_can_be_told_apart() {
    let (app, webview) = app();
    let sessions = tauri::Manager::state::<Sessions>(&app);
    for source in [
        None,
        Some(Source {
            work: "w1-0".to_owned(),
            extension_id: "issues".to_owned(),
            extension_name: "Issues".to_owned(),
            handler: "issues.poll".to_owned(),
            about: None,
        }),
        Some(Source {
            work: "w1-1".to_owned(),
            extension_id: "digest".to_owned(),
            extension_name: "Digest".to_owned(),
            handler: "digest.nightly".to_owned(),
            about: None,
        }),
    ] {
        sessions.insert(Session::new(
            sessions.mint_key(),
            "claude".to_owned(),
            "Claude Code".to_owned(),
            std::env::temp_dir(),
            source,
        ));
    }

    let rows = listed(&webview);
    let rows = rows.as_array().expect("a list");
    assert_eq!(rows.len(), 3);

    let mine: Vec<&Value> = rows
        .iter()
        .filter(|row| row["source"]["extensionId"] == "issues")
        .collect();
    assert_eq!(
        mine.len(),
        1,
        "an extension finds its own orders with no second call: {rows:?}"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.get("source").is_none())
            .count(),
        1,
        "and what a person started is still there beside them"
    );
}
