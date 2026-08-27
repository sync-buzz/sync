//! The one test in this crate that raises a process.
//!
//! Everything else runs the protocol over an in-memory duplex. This one exists
//! because a duplex cannot prove the parts that only a real child has: that the
//! command a registry row describes actually starts, that stdin and stdout are
//! wired the right way round, and that the environment a row insists on
//! clearing is really absent from the child.
//!
//! Its counterpart is `src/bin/acp_stub_agent.rs`, which shares no code with
//! the client — a stub built on the transport under test would agree with a bug
//! in it.
// In a test, an `expect` on a fixture is the failure report: if the captured
// frames stop being readable, the panic names which one and why.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use acp_client::registry::{AcpMode, AgentLaunchSpec, Verification};
use acp_client::{launch, schema, McpToolNaming, SessionUpdatePayload};
use support::{within, within_patience, Observed, TestHandler};

/// A registry row pointing at the stub binary cargo just built.
///
/// The row is built the same way every real row is, so the launch path under
/// test is the production one — only the program differs.
const STUB: AgentLaunchSpec = AgentLaunchSpec {
    id: "stub",
    display_name: "Stub agent",
    program: "acp-stub-agent",
    args: &[],
    unset_env: &["ACP_STUB_MUST_NOT_SURVIVE"],
    // The stub has no policy of its own to be told about.
    full_access_args: &[],
    acp_mode: AcpMode::Native,
    tool_naming: Some(McpToolNaming::Slash),
    // The stub answers whatever it is asked; nothing about a model to pass.
    model_pin: None,
    verification: Verification::LiveFullCycle,
};

fn options() -> launch::SpawnOptions {
    launch::SpawnOptions {
        program: Some(PathBuf::from(env!("CARGO_BIN_EXE_acp-stub-agent"))),
        ..launch::SpawnOptions::default()
    }
}

/// The text of the next `agent_message_chunk` the handler saw.
async fn next_chunk(observed: &mut tokio::sync::mpsc::UnboundedReceiver<Observed>) -> String {
    loop {
        let Some(event) = within_patience("a session/update", observed.recv()).await else {
            panic!("the connection ended before a chunk arrived");
        };
        let Observed::Update(event) = event else {
            continue;
        };
        let SessionUpdatePayload::Known(update) = event.payload else {
            panic!("the stub sends only typed variants");
        };
        if let schema::SessionUpdate::AgentMessageChunk(chunk) = *update {
            let schema::ContentBlock::Text(text) = chunk.content else {
                panic!("the stub sends text");
            };
            return text.text;
        }
    }
}

/// Starts the stub and runs `initialize` + `session/new` against it.
async fn started() -> (
    launch::AgentProcess,
    tokio::sync::mpsc::UnboundedReceiver<Observed>,
    schema::NewSessionResponse,
) {
    let (handler, observed) = TestHandler::new();
    let command = launch::command_for(&STUB, &options());
    let agent = launch::spawn(command, handler).expect("the stub binary starts");

    let init = within_patience(
        "initialize",
        agent
            .connection()
            .initialize(schema::InitializeRequest::new(
                acp_client::SUPPORTED_PROTOCOL_VERSION,
            )),
    )
    .await
    .expect("the stub answers initialize");
    assert_eq!(init.protocol_version, acp_client::ProtocolVersion::V1);

    let session = within_patience(
        "session/new",
        agent.connection().new_session(
            schema::NewSessionRequest::new("/tmp/acp-client-test").mcp_servers(vec![
                schema::McpServer::Stdio(
                    schema::McpServerStdio::new("sync", "/usr/local/bin/git-sync")
                        .args(vec!["mcp".to_owned()])
                        .env(vec![schema::EnvVariable::new("SYNC_AGENT_SLUG", "stub")]),
                ),
            ]),
        ),
    )
    .await
    .expect("the stub answers session/new");

    (agent, observed, session)
}

#[tokio::test]
async fn the_full_cycle_runs_against_a_real_process() {
    let (mut agent, mut observed, session) = started().await;

    let answer = within_patience(
        "session/prompt",
        agent.connection().prompt(schema::PromptRequest::new(
            session.session_id.clone(),
            vec![schema::ContentBlock::Text(schema::TextContent::new(
                "say hello",
            ))],
        )),
    )
    .await
    .expect("the stub answers session/prompt");

    assert_eq!(answer.stop_reason, schema::StopReason::EndTurn);
    assert_eq!(next_chunk(&mut observed).await, "PONG");

    agent.kill().await.expect("the stub can be stopped");
}

#[tokio::test]
async fn our_cwd_and_mcp_servers_reach_the_process() {
    // The stub echoes the `session/new` params it received. What is asserted
    // here is arrival at the other end of a pipe, not that we sent it.
    let (mut agent, _observed, session) = started().await;

    let meta = session.meta.as_ref().expect("the stub echoes what it got");
    let meta = serde_json::to_value(meta).expect("meta is JSON");
    let received = &meta["stub/received"];

    assert_eq!(received["cwd"], serde_json::json!("/tmp/acp-client-test"));
    assert_eq!(received["mcpServers"][0]["name"], serde_json::json!("sync"));
    assert_eq!(
        received["mcpServers"][0]["env"],
        serde_json::json!([{ "name": "SYNC_AGENT_SLUG", "value": "stub" }])
    );

    agent.kill().await.expect("the stub can be stopped");
}

#[tokio::test]
async fn the_process_can_ask_the_client_to_read_a_file() {
    let (mut agent, mut observed, session) = started().await;
    // Time-boxed on purpose: a client that stops answering the agent's
    // `fs/read_text_file` leaves the turn hanging, and a hanging test reports
    // nothing. This way the failure is named.
    let answer = within_patience(
        "the turn that reads a file",
        agent.connection().prompt(schema::PromptRequest::new(
            session.session_id.clone(),
            vec![schema::ContentBlock::Text(schema::TextContent::new(
                "read:/tmp/skill.md now",
            ))],
        )),
    )
    .await
    .expect("the turn ends");
    assert_eq!(answer.stop_reason, schema::StopReason::EndTurn);

    // The handler's default answer, echoed back by the agent — so the request
    // crossed the pipe, was answered, and the answer crossed back.
    let mut saw_read = false;
    let mut chunk = None;
    while let Ok(event) = observed.try_recv() {
        match event {
            Observed::Read(request) => {
                assert_eq!(request.path.to_string_lossy(), "/tmp/skill.md");
                saw_read = true;
            }
            Observed::Update(_) => chunk = Some(event_text(event)),
            _ => {}
        }
    }
    assert!(
        saw_read,
        "the agent's fs/read_text_file reached the handler"
    );
    assert_eq!(chunk.as_deref(), Some("contents from the client"));

    agent.kill().await.expect("the stub can be stopped");
}

#[tokio::test]
async fn the_process_can_ask_the_client_for_permission() {
    let (mut agent, mut observed, session) = started().await;
    // Time-boxed for the same reason as the read turn above.
    let answer = within_patience(
        "the turn that asks permission",
        agent.connection().prompt(schema::PromptRequest::new(
            session.session_id.clone(),
            vec![schema::ContentBlock::Text(schema::TextContent::new(
                "needs permission",
            ))],
        )),
    )
    .await
    .expect("the turn ends");
    assert_eq!(answer.stop_reason, schema::StopReason::EndTurn);

    let mut saw_permission = false;
    let mut chunk = None;
    while let Ok(event) = observed.try_recv() {
        match event {
            Observed::Permission(request) => {
                assert_eq!(request.options.len(), 2);
                saw_permission = true;
            }
            Observed::Update(_) => chunk = Some(event_text(event)),
            _ => {}
        }
    }
    assert!(saw_permission, "the permission request reached the handler");
    // The handler picks the allow option; the agent echoes what it was told.
    assert_eq!(chunk.as_deref(), Some("allow"));

    agent.kill().await.expect("the stub can be stopped");
}

#[tokio::test]
async fn cancel_reaches_the_process_and_ends_the_turn() {
    let (mut agent, mut observed, session) = started().await;

    let turn = agent.connection().prompt(schema::PromptRequest::new(
        session.session_id.clone(),
        vec![schema::ContentBlock::Text(schema::TextContent::new(
            "be slow please",
        ))],
    ));

    let cancel = async {
        // The first chunk proves the turn is genuinely under way, so the
        // cancellation lands mid-turn rather than before it started.
        assert_eq!(next_chunk(&mut observed).await, "1");
        agent
            .connection()
            .cancel(&schema::CancelNotification::new(session.session_id.clone()))
            .expect("the connection is open");
    };

    let (answer, ()) =
        within_patience("the cancelled turn", futures::future::join(turn, cancel)).await;
    assert_eq!(
        answer.expect("the turn ends").stop_reason,
        schema::StopReason::Cancelled
    );

    agent.kill().await.expect("the stub can be stopped");
}

#[tokio::test]
async fn the_child_sees_the_environment_the_row_and_the_caller_agreed_on() {
    // The row-level assertion is a unit test on the built command; this one is
    // about the process that actually started, and it is read out of the
    // child's own environment rather than out of our command builder.
    //
    // Both directions are needed. That a variable the row clears is gone is
    // the `CLAUDECODE` case — a failure that would otherwise show up only at
    // `session/new`, long after `initialize` looked fine. That a variable the
    // caller set is present is the case the whole transport change rests on:
    // session identity travels as a parameter, and it has to arrive.
    let options = launch::SpawnOptions {
        env: vec![
            // The row clears this one; the caller setting it must not win.
            ("ACP_STUB_MUST_NOT_SURVIVE".to_owned(), "1".to_owned()),
            ("ACP_STUB_MUST_SURVIVE".to_owned(), "marker-4417".to_owned()),
        ],
        ..options()
    };

    let (handler, _observed) = TestHandler::new();
    let command = launch::command_for(&STUB, &options);
    let mut agent = launch::spawn(command, handler).expect("the stub binary starts");

    let init = within_patience(
        "initialize",
        agent
            .connection()
            .initialize(schema::InitializeRequest::new(
                acp_client::SUPPORTED_PROTOCOL_VERSION,
            )),
    )
    .await
    .expect("the stub answers");

    let meta = init
        .meta
        .as_ref()
        .expect("the stub reports its environment");
    let env = &serde_json::to_value(meta).expect("meta is JSON")["stub/env"];
    assert_eq!(
        env["ACP_STUB_MUST_NOT_SURVIVE"],
        serde_json::Value::Null,
        "the row clears this one, so no caller can put it in the child"
    );
    assert_eq!(
        env["ACP_STUB_MUST_SURVIVE"],
        serde_json::json!("marker-4417"),
        "everything else the caller sets has to arrive"
    );

    agent.kill().await.expect("the stub can be stopped");
}

/// The control deadline the silence test injects in place of the shipped two
/// minutes.
const STUB_DEADLINE: Duration = Duration::from_millis(150);

/// How long that test waits for something the deadline has to cause. Wide
/// enough that a loaded box does not fail on process scheduling, short enough
/// that removing the deadline — or the kill — reads as this test going red by
/// name rather than as a suite that never finishes.
const STUB_DEADLINE_PATIENCE: Duration = Duration::from_secs(5);

#[tokio::test]
async fn a_process_that_goes_silent_is_given_up_on_and_stopped() {
    // The one shape a duplex cannot make: a real process that is up, reading
    // its stdin, and answering nothing. Its stdout never closes, so none of the
    // client's other exits — EOF, a dropped sender — can ever fire.
    let options = launch::SpawnOptions {
        env: vec![("ACP_STUB_SILENT".to_owned(), "initialize".to_owned())],
        ..options()
    };
    let (handler, _observed) = TestHandler::new();
    let command = launch::command_for(&STUB, &options);
    let mut agent = launch::spawn_with_request_timeout(command, handler, STUB_DEADLINE)
        .expect("the stub binary starts");

    let answer = within(
        "the deadline to end initialize",
        STUB_DEADLINE_PATIENCE,
        agent
            .connection()
            .initialize(schema::InitializeRequest::new(
                acp_client::SUPPORTED_PROTOCOL_VERSION,
            )),
    )
    .await;
    let Err(acp_client::Error::Timeout { method, timeout }) = answer else {
        panic!("expected a deadline failure, got {answer:?}");
    };
    assert_eq!(method, schema::AGENT_METHOD_NAMES.initialize);
    assert_eq!(timeout, STUB_DEADLINE);

    // The process is the point. Nothing about a silent agent ends it on its
    // own — its stdin is still open and it is still reading — so an exit status
    // appearing at all is the proof that the client killed it, and the status
    // says it was killed rather than asked to leave.
    //
    // Asked rather than awaited: `wait` would hold the process and the reaper
    // could not take it back, which is a deadlock this test found the first
    // time it was written.
    let status = within("the agent to be stopped", STUB_DEADLINE_PATIENCE, async {
        loop {
            if let Some(status) = agent.try_wait().await.expect("the child can be asked") {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        !status.success(),
        "the agent was killed after the deadline, not left to exit: {status:?}"
    );
}

#[tokio::test]
async fn a_program_that_does_not_exist_fails_with_the_program_named() {
    let options = launch::SpawnOptions {
        program: Some(PathBuf::from("/nonexistent/acp-agent-that-is-not-there")),
        ..launch::SpawnOptions::default()
    };
    let (handler, _observed) = TestHandler::new();
    let command = launch::command_for(&STUB, &options);

    let error = launch::spawn(command, handler).expect_err("nothing to start");
    let acp_client::Error::Spawn { program, .. } = error else {
        panic!("expected a spawn failure, got {error:?}");
    };
    assert_eq!(program, "/nonexistent/acp-agent-that-is-not-there");
}

/// The text of an update event, for the loops above.
fn event_text(event: Observed) -> String {
    let Observed::Update(event) = event else {
        panic!("not an update");
    };
    let SessionUpdatePayload::Known(update) = event.payload else {
        panic!("the stub sends only typed variants");
    };
    let schema::SessionUpdate::AgentMessageChunk(chunk) = *update else {
        panic!("expected an agent_message_chunk");
    };
    let schema::ContentBlock::Text(text) = chunk.content else {
        panic!("the stub sends text");
    };
    text.text
}

/// A provisioned adapter is run directly, and the row's fetching arguments go.
///
/// The Claude row's arguments are `npx -y <package>`: they are not *how the
/// adapter is configured*, they are *how it is fetched*. An embedder that has
/// already fetched it has to be able to drop them, or the launch it paid to
/// avoid happens anyway.
#[test]
fn given_arguments_replace_the_row_s_own_and_the_rest_of_the_row_still_applies() {
    let options = launch::SpawnOptions {
        args: Some(vec!["--stdio".to_owned()]),
        model: Some("claude-opus-5".to_owned()),
        ..launch::SpawnOptions::default()
    };
    let command = launch::command_for(&acp_client::registry::CLAUDE, &options);

    let args: Vec<&str> = command
        .get_args()
        .map(|arg| arg.to_str().expect("utf-8"))
        .collect();
    assert_eq!(
        args,
        ["--stdio"],
        "the row's own fetching arguments are gone"
    );

    // The model pin is not an argument about fetching, so it survives — and on
    // this row it is an environment variable in the first place.
    let model = command
        .get_envs()
        .find(|(name, _)| *name == std::ffi::OsStr::new("ANTHROPIC_MODEL"))
        .and_then(|(_, value)| value)
        .and_then(|value| value.to_str());
    assert_eq!(model, Some("claude-opus-5"));

    // And so does the clearing that makes this row work at all.
    assert!(
        command
            .get_envs()
            .any(|(name, value)| name == std::ffi::OsStr::new("CLAUDECODE") && value.is_none()),
        "CLAUDECODE must still be cleared: the adapter refuses, and refuses late"
    );
}
