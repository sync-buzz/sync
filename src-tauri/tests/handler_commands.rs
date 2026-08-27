//! Calling a package's handler, through the command the window will call.
//!
//! The crate's own tests prove that a handler runs, is stopped and reports.
//! These prove the thing above it: that a package installed on this machine is
//! resolved, that its module is read out of the artefact, that an occasion it
//! declares nothing for is an answer rather than a failure, and that a refusal
//! reaches the handler in words it can catch.
//!
//! They drive the real command over Tauri's IPC rather than calling the
//! function, because the shape of what crosses is part of what is being tested —
//! a field that does not survive `invoke` is a field that does not exist.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use serde_json::{Value, json};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

/// One id per test, fixed rather than random.
///
/// Fixed so that a run interrupted halfway leaves one stale pointer the next
/// run overwrites, instead of a directory that fills up. **Per test** because
/// the artefact store is this machine's and every test in the binary shares it:
/// two tests installing one id from two temporary folders is a race, and it
/// reads as `No such file or directory` when the first folder is removed while
/// the second test is still reading it. Sharing one id here cost half an hour
/// and five tests that failed in a different order every run.
const RUNS: &str = "probe-handlers-runs";
const SILENT: &str = "probe-handlers-silent";
const RUNAWAY: &str = "probe-handlers-runaway";
const REFUSED: &str = "probe-handlers-refused";
const MISMATCHED: &str = "probe-handlers-mismatched";
const OFFERS: &str = "probe-handlers-offers";
const UNPAID: &str = "probe-handlers-unpaid";
const ORDERS: &str = "probe-handlers-orders";
const KINDS: &str = "probe-handlers-kinds";
const SILENT_ORDER: &str = "probe-handlers-silent-order";
const UNNAMED: &str = "probe-handlers-unnamed";
const BLANK: &str = "probe-handlers-blank";

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

/// A package with handlers and no screen — the case that has to work, because
/// it is the one an extension with nothing to draw is made of.
fn package(root: &Path, id: &str, handlers: &str) {
    // Running code with no screen is something a person agrees to, and a
    // package that ships handlers has to ask for it. Nothing more than that:
    // the capability to order work is asked for by name, by the two tests that
    // are about it.
    asking(root, id, &["background"], handlers);
}

fn asking(root: &Path, id: &str, capabilities: &[&str], handlers: &str) {
    write(
        root.join("manifest.json"),
        &json!({
            "manifestVersion": 1,
            "id": id,
            "version": "1.0.0",
            "name": "Handler probe",
            "engines": { "syncApi": "^2.0" },
            "capabilities": capabilities,
            "service": "service/index.js",
            "lifecycle": { "installed": "probe.installed" },
        })
        .to_string(),
    );
    write(root.join("service/index.js"), handlers);
}

/// A handler that orders work and answers with whatever came back.
fn orders(order: &str) -> String {
    format!(
        r#"
        export default function register() {{
          return {{
            "probe.installed": () => {{
              try {{ return {{ key: __syncHost__("work.order", JSON.stringify({order})) }}; }}
              catch (error) {{ return {{ refused: String(error) }}; }}
            }},
          }};
        }}
        "#
    )
}

const ORDINARY: &str = r#"
    export default function register() {
      return {
        "probe.installed": (payload) => ({ ran: true, for: payload.project }),
      };
    }
"#;

fn install(webview: &WebviewWindow<MockRuntime>, folder: &Path) {
    invoke(
        webview,
        "extension_install_folder",
        json!({ "path": folder.to_string_lossy() }),
    )
    .expect("the folder installs");
}

fn called(
    webview: &WebviewWindow<MockRuntime>,
    id: &str,
    occasion: &str,
    payload: Value,
) -> Result<Value, Value> {
    invoke(
        webview,
        "extension_handler_call",
        json!({
            "project": "/nowhere",
            "id": id,
            "occasion": occasion,
            "payload": payload,
        }),
    )
}

/// **Two lists, one truth.** `handlers.rs` holds `OFFERED` — the names it says
/// it answers — and `@sync-buzz/extension-api/service` states the same list a
/// second time in TypeScript, because a package is built against a published
/// contract rather than against Sync's source. That copy is allowed to be
/// behind; what is not allowed is for *this* build to advertise a name its own
/// match does not answer, because then an author is told something exists and
/// finds out at three in the morning that it does not.
///
/// So each offered name is called for a project that is not there. The engine
/// refuses every one of them — there is no such project — and that is the
/// point: an engine's refusal is not a permission refusal, and only the second
/// one names the function as something a handler may not do.
#[test]
fn every_function_this_build_offers_is_one_it_answers() {
    let folder = tempfile::tempdir().expect("a directory");
    package(
        folder.path(),
        OFFERS,
        r#"
        export default function register() {
          return {
            "probe.installed": () => {
              const said = {};
              for (const name of ["memory.record", "memory.list", "memory.content", "work.order", "memory.invented"]) {
                try { __syncHost__(name, JSON.stringify({ key: "k" })); said[name] = "answered"; }
                catch (error) { said[name] = String(error); }
              }
              return said;
            },
          };
        }
        "#,
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer =
        called(&webview, OFFERS, "installed", json!({})).expect("the handler itself did not fail");
    let said = answer.as_object().expect("an object of answers");
    const REFUSAL: &str = "is not something a handler may do";

    for offered in [
        "memory.record",
        "memory.list",
        "memory.content",
        "work.order",
    ] {
        let reply = said[offered].as_str().unwrap_or_default();
        assert!(
            !reply.contains(REFUSAL),
            "`{offered}` is advertised and not answered: {reply}"
        );
    }
    let invented = said["memory.invented"].as_str().unwrap_or_default();
    assert!(
        invented.contains(REFUSAL),
        "a name nothing answers should be refused by name: {invented}"
    );
    assert!(
        invented.contains("memory.record") && invented.contains("memory.content"),
        "and the refusal should say what is offered instead: {invented}"
    );

    invoke(&webview, "extension_forget", json!({ "id": OFFERS })).expect("it is forgotten");
}

#[test]
fn a_handler_runs_from_the_package_on_disk() {
    let folder = tempfile::tempdir().expect("a directory");
    package(folder.path(), RUNS, ORDINARY);
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, RUNS, "installed", json!({ "project": "Sync" }))
        .expect("the handler runs");
    assert_eq!(
        answer,
        json!({ "ran": true, "for": "Sync" }),
        "what it returned crosses whole"
    );
}

/// The ordinary case for almost every package: it declares nothing for this
/// occasion, and that is an answer rather than something going wrong.
#[test]
fn an_occasion_a_package_says_nothing_about_is_not_a_failure() {
    let folder = tempfile::tempdir().expect("a directory");
    package(folder.path(), SILENT, ORDINARY);
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer =
        called(&webview, SILENT, "removed", json!({})).expect("nothing to call is not a failure");
    assert!(answer.is_null(), "got {answer}");
}

/// A handler that hangs fails its own call, and the refusal says which package
/// it was — which the crate underneath could not have known.
#[test]
fn a_runaway_handler_is_stopped_and_the_package_is_named() {
    let folder = tempfile::tempdir().expect("a directory");
    package(
        folder.path(),
        RUNAWAY,
        r#"
            export default function register() {
              return { "probe.installed": () => { while (true) {} } };
            }
        "#,
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let began = std::time::Instant::now();
    let refusal = called(&webview, RUNAWAY, "installed", json!({})).expect_err("it never returns");
    let took = began.elapsed();

    let said = refusal.as_str().expect("a refusal in words");
    assert!(
        said.contains(RUNAWAY),
        "which package it was comes first: {said}"
    );
    assert!(said.contains("stopped"), "{said}");
    assert!(
        took < std::time::Duration::from_secs(30),
        "the limit is the host's and it was applied: {took:?}"
    );
}

/// Nothing is ambient. A handler asking for something this build does not offer
/// hears why, and can carry on — a refusal is something a package is written
/// against, not a crash.
#[test]
fn what_a_handler_may_not_do_is_refused_in_words_it_can_catch() {
    let folder = tempfile::tempdir().expect("a directory");
    package(
        folder.path(),
        REFUSED,
        r#"
            export default function register() {
              return {
                "probe.installed": () => {
                  try { __syncHost__("net.fetch", "{}"); return { caught: false }; }
                  catch (error) { return { caught: true, said: String(error) }; }
                },
              };
            }
        "#,
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, REFUSED, "installed", json!({})).expect("the handler runs");
    assert_eq!(answer["caught"], json!(true), "got {answer}");
    assert!(
        answer["said"]
            .as_str()
            .expect("the refusal's words")
            .contains("not something a handler may do"),
        "the handler hears why rather than that something failed: {answer}"
    );
}

/// The manifest and the module are related by nothing a compiler can see, and
/// this is what that disagreement looks like when it reaches a person.
#[test]
fn a_manifest_naming_a_handler_the_module_lacks_says_which_name() {
    let folder = tempfile::tempdir().expect("a directory");
    package(
        folder.path(),
        MISMATCHED,
        r#"
            export default function register() {
              return { "probe.something-else": () => ({}) };
            }
        "#,
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let refusal = called(&webview, MISMATCHED, "installed", json!({})).expect_err("they disagree");
    let said = refusal.as_str().expect("a refusal in words");
    assert!(said.contains("probe.installed"), "{said}");
    assert!(said.contains("disagree"), "{said}");
}

#[test]
fn a_package_this_machine_does_not_have_is_named_rather_than_ignored() {
    let (_app, webview) = app();
    let refusal = invoke(
        &webview,
        "extension_handler_call",
        json!({
            "project": "/nowhere",
            "id": "probe-never-installed",
            "occasion": "installed",
            "payload": {},
        }),
    )
    .expect_err("there is nothing to run");
    assert!(
        refusal
            .as_str()
            .expect("words")
            .contains("probe-never-installed"),
        "{refusal}"
    );
}

/// The real control sample, from where it actually lies on disk.
///
/// Run deliberately, because it depends on a folder outside this repository:
///
/// ```text
/// cargo test --test handler_commands -- --ignored --nocapture
/// ```
///
/// It answers one question and no other: does the Rust half — resolve the
/// package, read its service module out of the folder, run the handler — work
/// on the package a person would actually install? Everything above it is the
/// window's, and this says nothing about that.
#[test]
#[ignore = "reads ../../hello-ext, run it deliberately"]
fn the_real_control_sample_runs() {
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../hello-ext")
        .canonicalize()
        .expect("the sample is where this test says it is");

    let (_app, webview) = app();
    invoke(
        &webview,
        "extension_install_folder",
        json!({ "path": sample.to_string_lossy() }),
    )
    .expect("the folder installs");

    let answer = invoke(
        &webview,
        "extension_handler_call",
        json!({
            "project": "/nowhere",
            "id": "hello",
            "occasion": "installed",
            "payload": { "project": { "name": "Demo" }, "version": "0.1.0" },
        }),
    )
    .expect("the handler runs");

    println!("hello answered: {answer}");
    assert_eq!(answer["greeted"], json!("Demo"));
    assert_eq!(
        answer["node"],
        json!(false),
        "it ran in an isolate, not in something richer"
    );
    assert_eq!(answer["browser"], json!(false));
}

/// **The one capability the manifest cannot give away.** `background` and
/// `schedule` are visible in the file and refused when it is read; whether a
/// handler orders work is inside the JavaScript, so the only place it can be
/// enforced is the call. A package that ships handlers and asked only for
/// `background` has agreed to code running, and not to money being spent.
#[test]
fn ordering_work_without_asking_to_is_refused_by_name() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        UNPAID,
        &["background"],
        &orders(
            r#"{ kind: "agent.session", agent: "claude", title: "A probe", prompt: { text: "go" }, onInterrupted: "wait" }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, UNPAID, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(
        refused.contains("work.agent"),
        "the refusal names the capability to add: {refused}"
    );
    assert!(
        refused.contains("asleep"),
        "and says why it is asked for separately: {refused}"
    );
    assert!(
        answer.get("key").is_none(),
        "and nothing was ordered: {answer}"
    );

    invoke(&webview, "extension_forget", json!({ "id": UNPAID })).expect("it is forgotten");
}

/// Past the capability, an order is still checked against what this build can
/// actually perform — and each refusal names what exists rather than only what
/// does not, so a handler's author can act on it without reading Sync's source.
///
/// The order that *succeeds* is not here. It raises a real agent, which on a
/// machine without the adapter means `npm install` — so what a test can prove
/// ends at the last refusal, and the rest was watched on a running application.
#[test]
fn an_order_this_build_cannot_perform_is_refused_naming_what_it_can() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        ORDERS,
        &["background", "work.agent"],
        &orders(
            r#"{ kind: "agent.session", agent: "hal", title: "A probe", prompt: { text: "go" }, onInterrupted: "continue" }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, ORDERS, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(
        refused.contains("`hal`") && refused.contains("claude"),
        "an agent nobody has is refused naming the ones this build knows: {refused}"
    );

    invoke(&webview, "extension_forget", json!({ "id": ORDERS })).expect("it is forgotten");
}

/// The other half of the same sentence, and the reason `kind` is a registry
/// with one entry rather than a guess at five.
#[test]
fn a_kind_of_work_that_does_not_exist_is_refused_naming_the_one_that_does() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        KINDS,
        &["background", "work.agent"],
        &orders(
            r#"{ kind: "agent.swarm", agent: "claude", title: "A probe", prompt: { text: "go" }, onInterrupted: "wait" }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, KINDS, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(
        refused.contains("agent.swarm") && refused.contains("agent.session"),
        "{refused}"
    );

    invoke(&webview, "extension_forget", json!({ "id": KINDS })).expect("it is forgotten");
}

/// A conversation nobody named is one the list names after the words a handler
/// wrote to an agent — which reads exactly like something a person typed.
///
/// Two refusals rather than one, because they are two mistakes: a package that
/// forgot the field, and a package that filled it with nothing. The second is
/// the one a validator usually lets through.
#[test]
fn an_order_that_does_not_say_what_to_call_the_conversation_is_refused() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        UNNAMED,
        &["background", "work.agent"],
        &orders(
            r#"{ kind: "agent.session", agent: "claude", prompt: { text: "go" }, onInterrupted: "wait" }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, UNNAMED, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(refused.contains("title"), "{refused}");

    invoke(&webview, "extension_forget", json!({ "id": UNNAMED })).expect("it is forgotten");
}

#[test]
fn a_title_of_nothing_at_all_is_refused_like_a_missing_one() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        BLANK,
        &["background", "work.agent"],
        &orders(
            r#"{ kind: "agent.session", agent: "claude", title: "   ", prompt: { text: "go" }, onInterrupted: "wait" }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer = called(&webview, BLANK, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(
        refused.contains("a title"),
        "the refusal says what is missing and why only the package can supply it: {refused}"
    );

    invoke(&webview, "extension_forget", json!({ "id": BLANK })).expect("it is forgotten");
}

/// An order with no choice about interruption is not an order. There is no
/// default because neither answer is right for both cases (§6.4), so the
/// refusal has to arrive rather than a guess.
#[test]
fn an_order_that_does_not_say_what_to_do_if_it_is_interrupted_is_refused() {
    let folder = tempfile::tempdir().expect("a directory");
    asking(
        folder.path(),
        SILENT_ORDER,
        &["background", "work.agent"],
        &orders(
            r#"{ kind: "agent.session", agent: "claude", title: "A probe", prompt: { text: "go" } }"#,
        ),
    );
    let (_app, webview) = app();
    install(&webview, folder.path());

    let answer =
        called(&webview, SILENT_ORDER, "installed", json!({})).expect("the handler itself ran");
    let refused = answer["refused"].as_str().unwrap_or_default();
    assert!(
        refused.contains("onInterrupted"),
        "the refusal names the field that is missing: {refused}"
    );

    invoke(&webview, "extension_forget", json!({ "id": SILENT_ORDER })).expect("it is forgotten");
}
