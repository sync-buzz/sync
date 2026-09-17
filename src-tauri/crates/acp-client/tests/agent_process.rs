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

/// What the stub is asked to put inside the tool's answer.
///
/// Carried in the prompt and read back out of `rawOutput`, so what the test
/// proves is that the value crossed two pipes — not that both sides happen to
/// know the same constant.
const TOOL_MARKER: &str = "marker-8821";

#[tokio::test]
async fn a_tool_calls_own_answer_crosses_the_process_boundary() {
    let (mut agent, mut observed, session) = started().await;

    let answer = within_patience(
        "the turn that calls a tool",
        agent.connection().prompt(schema::PromptRequest::new(
            session.session_id.clone(),
            vec![schema::ContentBlock::Text(schema::TextContent::new(
                format!("tool:{TOOL_MARKER} please"),
            ))],
        )),
    )
    .await
    .expect("the turn ends");
    assert_eq!(answer.stop_reason, schema::StopReason::EndTurn);

    // Owned as they are read: a report borrows the update it came from, and
    // the updates are drained from the queue here.
    let mut reports = Vec::new();
    let mut prose = Vec::new();
    while let Ok(event) = observed.try_recv() {
        let Observed::Update(event) = event else {
            continue;
        };
        if let Some(report) = event.payload.tool_call() {
            reports.push((
                report.tool_call_id.0.to_string(),
                report.title.map(ToOwned::to_owned),
                report.status,
                report.raw_output.cloned(),
            ));
            continue;
        }
        // Edition 2021 here, so no let chain: this crate came over whole and
        // is kept checkable against the source it came from.
        if let SessionUpdatePayload::Known(update) = &event.payload {
            if let schema::SessionUpdate::AgentMessageChunk(chunk) = &**update {
                if let schema::ContentBlock::Text(text) = &chunk.content {
                    prose.push(text.text.clone());
                }
            }
        }
    }

    let [announced, finished] = reports.as_slice() else {
        panic!("the turn reports the call once and finishes it once, got {reports:?}");
    };

    // The announcement: no result yet, and that is an empty member rather than
    // a frame that never arrived.
    assert_eq!(announced.2, Some(schema::ToolCallStatus::Pending));
    assert_eq!(announced.3, None, "nothing has been returned yet");
    // Where a tool's name shows up on the wire differs by agent, so what is
    // pinned here is that the title arrives unchanged and reads back through
    // the naming its row declares.
    assert_eq!(
        McpToolNaming::Slash.parse(
            announced
                .1
                .as_deref()
                .expect("the call is announced by title"),
            &["sync"],
        ),
        Some(acp_client::McpToolName::new("sync", "sync_status")),
    );

    // The answer, and it is the tool's own.
    assert_eq!(finished.0, announced.0, "the same call, amended");
    assert_eq!(finished.2, Some(schema::ToolCallStatus::Completed));
    assert_eq!(
        finished.3,
        Some(serde_json::json!({ "servers": [{ "name": TOOL_MARKER }] })),
    );

    // The prose is there, it disagrees, and it was never consulted. This is
    // the whole reason the answer is taken from `rawOutput`: a client that
    // read the words would have reported an empty answer for a call that
    // returned one.
    assert!(!prose.is_empty(), "the stub also talks about the call");
    assert!(
        !prose.iter().any(|said| said.contains(TOOL_MARKER)),
        "the words never carry the answer: {prose:?}"
    );

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

/// The errand door through a real process: one turn, one tool, and the tool's
/// own answer.
///
/// The stub reads the tool's name back off the prompt, so what is proven here
/// is the round trip — the client renders `sync/…` for a Codex-spelled row, the
/// far end reads that same string, and the answer is recognised as the one that
/// was asked for.
#[tokio::test]
async fn an_errand_brings_back_what_the_tool_returned() {
    const MARKER: &str = "marker-4041";

    let errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        acp_client::McpToolName::new("sync", "sync_status"),
        serde_json::json!({ "marker": MARKER }),
    );

    let answer = within_patience(
        "the errand",
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect("the tool answers");

    assert_eq!(
        answer["called"], "sync/sync_status",
        "the far end was asked for the tool this row spells that way",
    );
    // The arguments crossed the pipe rather than having merely been sent: the
    // stub echoes the prompt it received, and the marker is in it.
    assert!(
        answer["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains(MARKER)),
        "the arguments did not reach the agent: {answer}",
    );
    // And the agent's own words, which said the opposite, were never consulted.
    assert!(
        !answer.to_string().contains("found nothing at all"),
        "the answer is the tool's, not the agent's: {answer}",
    );
}

/// The other outcome: a turn that talked and called nothing is a named refusal,
/// never the model's text.
#[tokio::test]
async fn a_turn_that_calls_nothing_is_refused_by_name() {
    let errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        acp_client::McpToolName::new("sync", "refuses"),
        serde_json::json!({}),
    );

    let error = within_patience(
        "the errand",
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect_err("a turn with no call in it has no answer to give");

    let acp_client::ErrandError::Refused(refusal) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(refusal, acp_client::Refusal::NotCalled);
    assert_eq!(refusal.code(), "tool_not_called");
}

/// A tool that ran and answered, under a title this client cannot read as a
/// name, through a real process. The answer is on the wire and it is still not
/// returned: only a call bearing the name that was asked for answers for the
/// turn, and this door would rather refuse than attribute by elimination.
#[tokio::test]
async fn an_answer_under_a_title_that_is_not_a_name_is_refused() {
    let errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        acp_client::McpToolName::new("sync", "unnamed_status"),
        serde_json::json!({}),
    );

    let error = within_patience(
        "the errand",
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect_err("a call this side cannot name is not this errand's answer");

    let acp_client::ErrandError::Refused(refusal) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(refusal, acp_client::Refusal::NotNamed);
    assert_eq!(refusal.code(), "tool_never_named");
}

/// And the third: a tool behind a permission nobody can give. This is what a
/// real flagship does today for anything it has not been told it may run, and
/// the refusal has to name that rather than blame the agent for calling
/// nothing.
#[tokio::test]
async fn a_tool_behind_a_permission_says_so_rather_than_blaming_the_agent() {
    let errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        acp_client::McpToolName::new("sync", "guarded_status"),
        serde_json::json!({}),
    );

    let error = within_patience(
        "the errand",
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect_err("a tool that was never allowed to run has not answered");

    let acp_client::ErrandError::Refused(refusal) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(refusal, acp_client::Refusal::PermissionNeeded);
}

/// And the same tool, behind the same permission, once somebody has agreed to
/// it: the request is answered inside the turn and the tool runs.
///
/// Through a process rather than against the handler alone, because what this
/// has to prove is the round trip — the agreement is a fact on this side, the
/// question comes off the wire, the answer goes back down it, and the far end
/// acts on the option that was chosen. A handler tested in isolation says
/// nothing about the last of those.
#[tokio::test]
async fn a_tool_somebody_agreed_to_runs_behind_its_permission() {
    let mut errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        acp_client::McpToolName::new("sync", "guarded_status"),
        serde_json::json!({}),
    );
    errand.consent = acp_client::Consent::Given;

    let answer = within_patience(
        "the errand",
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect("a tool that was allowed to run answers");

    assert_eq!(
        answer["called"], "sync/guarded_status",
        "the permission was answered for the tool that was agreed to",
    );
}

/// A turn that never ends is stopped, and the process with it.
///
/// The one bound an unwatched turn has. Driven with a patience of milliseconds
/// so the test measures the deadline firing rather than sitting out the real
/// one.
#[tokio::test]
async fn an_errand_that_never_ends_is_given_up_on() {
    let mut errand = acp_client::Errand::new(
        PathBuf::from("/tmp/acp-client-test"),
        // `slow` in the tool's name reaches the stub's waiting script, which
        // ends only on a cancel that an errand never sends.
        acp_client::McpToolName::new("sync", "slow_status"),
        serde_json::json!({}),
    );
    errand.patience = Duration::from_millis(500);

    let error = within(
        "the errand's own deadline",
        Duration::from_secs(10),
        acp_client::errand::run(&STUB, &options(), &errand),
    )
    .await
    .expect_err("a turn that never ended answered nothing");

    let acp_client::ErrandError::Refused(refusal) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(refusal.code(), "tool_call_overran");
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
