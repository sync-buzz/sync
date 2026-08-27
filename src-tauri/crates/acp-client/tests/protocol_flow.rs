//! The protocol itself, driven frame by frame over an in-memory duplex.
//!
//! No process is started here. Every agent frame is written by the test, so
//! nothing depends on timing, and the assertions are about what actually
//! crossed the wire rather than about how long something took.
// In a test, an `expect` on a fixture is the failure report: if the captured
// frames stop being readable, the panic names which one and why.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use acp_client::{schema, Error, SessionUpdatePayload};
use serde_json::json;
use support::{wire, wire_with_capacity, wire_with_timeout, within, within_patience, Observed};
use tokio::sync::Semaphore;

/// The control deadline the tests below inject in place of the shipped two
/// minutes. Short enough to sit through, long enough that a frame written and
/// read over a duplex arrives well inside it.
const DEADLINE: Duration = Duration::from_millis(100);

/// How long those tests give the client to do what the deadline itself must
/// make it do. An order of magnitude over [`DEADLINE`] and far under the
/// suite's patience: with the deadline taken out of the client, the failure has
/// to arrive as a named red rather than as a suite that hangs.
const DEADLINE_PATIENCE: Duration = Duration::from_secs(3);

/// The text of an `agent_message_chunk`, or a panic saying what it was instead.
fn chunk_text(observed: Observed) -> String {
    let Observed::Update(event) = observed else {
        panic!("expected a session/update, got {observed:?}");
    };
    let SessionUpdatePayload::Known(update) = event.payload else {
        panic!("expected a typed update");
    };
    let schema::SessionUpdate::AgentMessageChunk(chunk) = *update else {
        panic!("expected an agent_message_chunk");
    };
    let schema::ContentBlock::Text(text) = chunk.content else {
        panic!("expected text content");
    };
    text.text
}

#[tokio::test]
async fn the_full_cycle_reaches_a_stop_reason() {
    let mut wired = wire();

    let call = async {
        let init = wired
            .connection
            .initialize(schema::InitializeRequest::new(
                acp_client::SUPPORTED_PROTOCOL_VERSION,
            ))
            .await?;
        let session = wired
            .connection
            .new_session(schema::NewSessionRequest::new("/tmp/project"))
            .await?;
        let prompt = wired
            .connection
            .prompt(schema::PromptRequest::new(
                session.session_id.clone(),
                vec![schema::ContentBlock::Text(schema::TextContent::new("hi"))],
            ))
            .await?;
        Ok::<_, Error>((init, session, prompt))
    };

    let agent = async {
        let (id, params) = wired.agent.expect_request("initialize").await;
        assert_eq!(params["protocolVersion"], json!(1));
        wired
            .agent
            .respond(
                id,
                json!({ "protocolVersion": 1, "agentCapabilities": { "loadSession": true } }),
            )
            .await;

        let (id, params) = wired.agent.expect_request("session/new").await;
        assert_eq!(params["cwd"], json!("/tmp/project"));
        wired.agent.respond(id, json!({ "sessionId": "s-1" })).await;

        let (id, _) = wired.agent.expect_request("session/prompt").await;
        wired
            .agent
            .update(
                "s-1",
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "PONG" },
                }),
            )
            .await;
        wired
            .agent
            .respond(id, json!({ "stopReason": "end_turn" }))
            .await;
    };

    // Boxed because this one future holds the whole cycle's state — three
    // request/response pairs and the stub agent driving them — and the
    // protocol types are large enough that clippy calls it out on the stack.
    let (answer, ()) = Box::pin(within_patience("the full cycle", futures_join(call, agent))).await;
    let (init, session, prompt) = answer.expect("the cycle completed");

    assert_eq!(init.protocol_version, acp_client::ProtocolVersion::V1);
    assert_eq!(session.session_id.0.as_ref(), "s-1");
    assert_eq!(prompt.stop_reason, schema::StopReason::EndTurn);

    let observed = wired
        .observed
        .recv()
        .await
        .expect("the chunk was delivered");
    assert_eq!(chunk_text(observed), "PONG");
}

#[tokio::test]
async fn prompt_dispatch_is_reported_after_the_frame_crosses_the_wire() {
    // Smaller than the request frame: the writer cannot finish until the fake
    // agent actually reads, which makes an early queue-only receipt observable.
    let wired = wire_with_capacity(16);
    let mut agent = wired.agent;
    let (dispatched_tx, mut dispatched_rx) = tokio::sync::oneshot::channel();
    let call = tokio::spawn(async move {
        wired
            .connection
            .prompt_with_dispatch(
                schema::PromptRequest::new(
                    "s-1",
                    vec![schema::ContentBlock::Text(schema::TextContent::new("hi"))],
                ),
                dispatched_tx,
            )
            .await
    });

    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut dispatched_rx)
            .await
            .is_err(),
        "dispatch was reported while the frame was only queued"
    );
    let (id, _) = agent.expect_request("session/prompt").await;
    within_patience("prompt dispatch receipt", dispatched_rx)
        .await
        .expect("the flushed prompt reports dispatch");
    assert!(!call.is_finished(), "dispatch waited for the turn to end");

    agent.respond(id, json!({ "stopReason": "end_turn" })).await;
    assert_eq!(
        call.await
            .expect("prompt task")
            .expect("prompt response")
            .stop_reason,
        schema::StopReason::EndTurn
    );
}

#[tokio::test]
async fn the_stop_reason_never_overtakes_the_last_chunk() {
    // The upper layer renders chunks and then closes the turn. If the answer to
    // `session/prompt` could reach the caller before an update the agent wrote
    // earlier, the turn would close on a half-rendered message.
    //
    // Simply reading the queue after the turn does not test this: the frames
    // arrive in order either way, so an implementation that resolves responses
    // off the ordered path passes that check as long as delivery happens to
    // have caught up. What discriminates is holding delivery open. With the
    // gate shut, an ordered client cannot produce the answer at all, because
    // the answer is queued behind the chunk that is waiting.
    let mut wired = wire();
    let gate = Arc::new(Semaphore::new(0));
    wired.handler.hold_updates(Arc::clone(&gate)).await;

    let mut agent = wired.agent;
    let script = tokio::spawn(async move {
        let (id, _) = agent.expect_request("session/prompt").await;
        for text in ["one", "two", "three"] {
            agent
                .update(
                    "s-1",
                    json!({
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": text },
                    }),
                )
                .await;
        }
        agent.respond(id, json!({ "stopReason": "end_turn" })).await;
        agent
    });

    let mut call = Box::pin(wired.connection.prompt(schema::PromptRequest::new(
        "s-1",
        vec![schema::ContentBlock::Text(schema::TextContent::new("hi"))],
    )));

    // The window is long enough for every frame above to be written, read and
    // parsed — so the answer is sitting in the client, and the only thing
    // keeping it from the caller is the chunk still waiting on the gate.
    let too_early = tokio::time::timeout(Duration::from_millis(300), &mut call).await;
    assert!(
        too_early.is_err(),
        "the turn closed while its own first chunk had not been delivered yet"
    );

    gate.add_permits(3);
    let answer = within_patience("the turn", &mut call)
        .await
        .expect("the turn ends");
    assert_eq!(answer.stop_reason, schema::StopReason::EndTurn);

    let mut delivered = Vec::new();
    while let Ok(observed) = wired.observed.try_recv() {
        delivered.push(chunk_text(observed));
    }
    assert_eq!(delivered, ["one", "two", "three"]);

    drop(script.await.expect("the agent script finished"));
}

#[tokio::test]
async fn cancel_ends_the_turn_with_stop_reason_cancelled() {
    let mut wired = wire();

    let call = wired.connection.prompt(schema::PromptRequest::new(
        "s-1",
        vec![schema::ContentBlock::Text(schema::TextContent::new(
            "count to 200",
        ))],
    ));

    let agent = async {
        let (prompt_id, _) = wired.agent.expect_request("session/prompt").await;
        wired
            .agent
            .update(
                "s-1",
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "1" },
                }),
            )
            .await;

        // The client cancels mid-turn; `session/cancel` is a notification, so
        // there is no id and nothing to answer.
        wired
            .connection
            .cancel(&schema::CancelNotification::new("s-1"))
            .expect("the connection is open");

        let frame = wired.agent.next_frame().await;
        assert_eq!(frame["method"], json!("session/cancel"));
        assert_eq!(frame["params"]["sessionId"], json!("s-1"));
        assert!(frame.get("id").is_none(), "a notification carries no id");

        // The agent acknowledges the only way the protocol allows: by ending
        // the turn that was already in flight.
        wired
            .agent
            .respond(prompt_id, json!({ "stopReason": "cancelled" }))
            .await;
    };

    let (answer, ()) = within_patience("the cancelled turn", futures_join(call, agent)).await;
    assert_eq!(answer.unwrap().stop_reason, schema::StopReason::Cancelled);
}

#[tokio::test]
async fn the_agent_gets_answered_when_it_asks_for_permission() {
    let mut wired = wire();

    wired
        .agent
        .request(
            7,
            "session/request_permission",
            json!({
                "sessionId": "s-1",
                "toolCall": {
                    "toolCallId": "call-1",
                    "title": "printf 'OK' > /tmp/outside/probe.txt",
                    "kind": "execute",
                },
                "options": [
                    { "optionId": "reject", "name": "Deny", "kind": "reject_once" },
                    { "optionId": "allow", "name": "Allow Once", "kind": "allow_once" },
                ],
            }),
        )
        .await;

    let answer = within_patience("the permission answer", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(7));
    assert_eq!(answer["result"]["outcome"]["outcome"], json!("selected"));
    assert_eq!(answer["result"]["outcome"]["optionId"], json!("allow"));

    let observed = within_patience("the handler to see it", wired.observed.recv())
        .await
        .expect("the handler saw it");
    let Observed::Permission(request) = observed else {
        panic!("expected a permission request, got {observed:?}");
    };
    assert_eq!(request.session_id.0.as_ref(), "s-1");
    assert_eq!(request.options.len(), 2);
}

#[tokio::test]
async fn the_agent_gets_answered_when_it_reads_a_file() {
    // Only Grok was measured calling this. It cannot read a skill without it.
    let mut wired = wire();
    wired
        .handler
        .set_read_answer(Ok("BLUEHERON-7719".to_owned()))
        .await;

    wired
        .agent
        .request(
            1,
            "fs/read_text_file",
            json!({ "sessionId": "s-1", "path": "/tmp/project/skill.md" }),
        )
        .await;

    let answer = within_patience("the file contents", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(1));
    assert_eq!(answer["result"]["content"], json!("BLUEHERON-7719"));

    let observed = within_patience("the handler to see it", wired.observed.recv())
        .await
        .expect("the handler saw it");
    let Observed::Read(request) = observed else {
        panic!("expected a read, got {observed:?}");
    };
    assert_eq!(request.path.to_string_lossy(), "/tmp/project/skill.md");
}

#[tokio::test]
async fn the_agent_gets_answered_when_it_writes_a_file() {
    let mut wired = wire();

    wired
        .agent
        .request(
            2,
            "fs/write_text_file",
            json!({
                "sessionId": "s-1",
                "path": "/tmp/project/out.txt",
                "content": "OK",
            }),
        )
        .await;

    let answer = within_patience("the write acknowledgement", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(2));
    assert!(answer.get("error").is_none(), "the write succeeded");

    let observed = within_patience("the handler to see it", wired.observed.recv())
        .await
        .expect("the handler saw it");
    let Observed::Write(request) = observed else {
        panic!("expected a write, got {observed:?}");
    };
    assert_eq!(request.content, "OK");
}

#[tokio::test]
async fn a_handler_that_declines_answers_with_an_error_not_with_silence() {
    // An agent waiting on a request it never gets answered hangs forever.
    let mut wired = wire();
    wired
        .handler
        .set_read_answer(Err(acp_client::RpcError::new(-32000, "no such file")))
        .await;

    wired
        .agent
        .request(
            3,
            "fs/read_text_file",
            json!({ "sessionId": "s-1", "path": "/nope" }),
        )
        .await;

    let answer = within_patience("the refusal", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(3));
    assert_eq!(answer["error"]["code"], json!(-32000));
    assert_eq!(answer["error"]["message"], json!("no such file"));
}

#[tokio::test]
async fn a_client_method_we_do_not_implement_is_refused_not_ignored() {
    let mut wired = wire();

    wired
        .agent
        .request(4, "terminal/create", json!({ "sessionId": "s-1" }))
        .await;

    let answer = within_patience("the method-not-found", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(4));
    assert_eq!(answer["error"]["code"], json!(-32601));
}

#[tokio::test]
async fn a_request_with_params_the_method_cannot_take_is_refused() {
    let mut wired = wire();

    wired
        .agent
        .request(5, "fs/read_text_file", json!({ "nothing": "useful" }))
        .await;

    let answer = within_patience("the invalid-params", wired.agent.next_frame()).await;
    assert_eq!(answer["id"], json!(5));
    assert_eq!(answer["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn a_banner_on_the_agents_stdout_does_not_end_the_session() {
    // Adapters and CLIs print to stdout. Treating that as a protocol failure
    // would kill sessions for a cosmetic reason.
    let mut wired = wire();

    let call = wired.connection.initialize(schema::InitializeRequest::new(
        acp_client::SUPPORTED_PROTOCOL_VERSION,
    ));

    let agent = async {
        let (id, _) = wired.agent.expect_request("initialize").await;
        wired
            .agent
            .write_line("npm warn exec package not found")
            .await;
        wired.agent.write_line("").await;
        wired.agent.write_line("{ not json at all").await;
        wired
            .agent
            .respond(id, json!({ "protocolVersion": 1 }))
            .await;
    };

    let (answer, ()) =
        within_patience("initialize past the noise", futures_join(call, agent)).await;
    assert_eq!(
        answer.unwrap().protocol_version,
        acp_client::ProtocolVersion::V1
    );
}

#[tokio::test]
async fn an_unknown_notification_reaches_the_handler_instead_of_vanishing() {
    let mut wired = wire();

    wired
        .agent
        .notify("x.ai/hook_fired", json!({ "event": "pre_tool_use" }))
        .await;

    let observed = within_patience("the unknown notification", wired.observed.recv())
        .await
        .expect("delivered");
    let Observed::UnhandledNotification { method, params } = observed else {
        panic!("expected an unhandled notification, got {observed:?}");
    };
    assert_eq!(method, "x.ai/hook_fired");
    assert_eq!(params.unwrap()["event"], json!("pre_tool_use"));
}

#[tokio::test]
async fn a_session_update_without_a_session_id_is_surfaced_not_dropped() {
    let mut wired = wire();

    wired
        .agent
        .notify(
            "session/update",
            json!({ "update": { "sessionUpdate": "agent_message_chunk" } }),
        )
        .await;

    let observed = within_patience("the unroutable update", wired.observed.recv())
        .await
        .expect("delivered");
    assert!(
        matches!(observed, Observed::UnhandledNotification { ref method, .. } if method == "session/update"),
        "got {observed:?}"
    );
}

#[tokio::test]
async fn an_agent_error_comes_back_as_a_typed_refusal() {
    let mut wired = wire();

    let call = wired
        .connection
        .new_session(schema::NewSessionRequest::new("/tmp/project"));

    let agent = async {
        let (id, _) = wired.agent.expect_request("session/new").await;
        wired
            .agent
            .respond_error(id, -32000, "This client is no longer supported")
            .await;
    };

    let (answer, ()) = within_patience("the refusal", futures_join(call, agent)).await;
    let Err(Error::Rpc { method, source }) = answer else {
        panic!("expected a typed RPC refusal, got {answer:?}");
    };
    assert_eq!(method, "session/new");
    assert_eq!(source.code, -32000);
    assert_eq!(source.message, "This client is no longer supported");
}

#[tokio::test]
async fn an_answer_that_is_not_the_shape_the_method_promises_is_not_swallowed() {
    let mut wired = wire();

    let call = wired
        .connection
        .new_session(schema::NewSessionRequest::new("/tmp/project"));

    let agent = async {
        let (id, _) = wired.agent.expect_request("session/new").await;
        wired
            .agent
            .respond(id, json!({ "notASessionId": true }))
            .await;
    };

    let (answer, ()) = within_patience("the malformed answer", futures_join(call, agent)).await;
    let Err(Error::MalformedResponse {
        method, payload, ..
    }) = answer
    else {
        panic!("expected a malformed-response error, got {answer:?}");
    };
    assert_eq!(method, "session/new");
    assert_eq!(payload["notASessionId"], json!(true));
}

#[tokio::test]
async fn a_request_in_flight_when_the_agent_dies_fails_instead_of_hanging() {
    let mut wired = wire();

    let call = wired
        .connection
        .prompt(schema::PromptRequest::new("s-1", vec![]));

    let agent = async {
        let _ = wired.agent.expect_request("session/prompt").await;
        wired.agent.hang_up().await;
    };

    let (answer, ()) = within_patience("the failure", futures_join(call, agent)).await;
    assert!(
        matches!(answer, Err(Error::Closed)),
        "expected Error::Closed, got {answer:?}"
    );
}

#[tokio::test]
async fn a_request_raised_after_the_agent_died_fails_at_once() {
    let wired = wire();
    wired.agent.hang_up().await;

    // Let the reader observe the EOF.
    within_patience("the connection to notice the EOF", async {
        while !wired.connection.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await;

    let answer = wired
        .connection
        .prompt(schema::PromptRequest::new("s-1", vec![]))
        .await;
    assert!(
        matches!(answer, Err(Error::Closed)),
        "expected Error::Closed, got {answer:?}"
    );
}

#[tokio::test]
async fn a_control_request_to_a_silent_agent_ends_on_the_deadline() {
    let mut wired = wire_with_timeout(DEADLINE);

    let call = wired.connection.initialize(schema::InitializeRequest::new(
        acp_client::SUPPORTED_PROTOCOL_VERSION,
    ));

    // The agent takes the frame and answers nothing. It has not died — its
    // stdout is open — so nothing else in the client will ever release the
    // caller.
    let agent = async {
        let _ = wired.agent.expect_request("initialize").await;
    };

    let (answer, ()) = within(
        "the deadline to end initialize",
        DEADLINE_PATIENCE,
        futures_join(call, agent),
    )
    .await;

    let Err(Error::Timeout { method, timeout }) = answer else {
        panic!("expected a deadline failure, got {answer:?}");
    };
    assert_eq!(method, schema::AGENT_METHOD_NAMES.initialize);
    assert_eq!(timeout, DEADLINE);
}

#[tokio::test]
async fn a_call_after_the_deadline_fails_without_reaching_the_agent() {
    let mut wired = wire_with_timeout(DEADLINE);

    let call = wired.connection.initialize(schema::InitializeRequest::new(
        acp_client::SUPPORTED_PROTOCOL_VERSION,
    ));
    let agent = async {
        let _ = wired.agent.expect_request("initialize").await;
    };
    let (first, ()) = within(
        "the deadline to end initialize",
        DEADLINE_PATIENCE,
        futures_join(call, agent),
    )
    .await;
    assert!(
        matches!(first, Err(Error::Timeout { .. })),
        "the connection has to expire first, got {first:?}"
    );

    let second = within(
        "the refusal of the next call",
        DEADLINE_PATIENCE,
        wired
            .connection
            .new_session(schema::NewSessionRequest::new("/tmp/acp-client-test")),
    )
    .await;
    let Err(Error::Timeout { method, .. }) = second else {
        panic!("expected the same deadline failure, got {second:?}");
    };
    assert_eq!(method, schema::AGENT_METHOD_NAMES.session_new);

    // What proves it failed fast is not a stopwatch: a refused call never
    // reaches the wire at all, while one that waited its deadline out again
    // would have written its frame immediately.
    assert!(
        tokio::time::timeout(DEADLINE * 3, wired.agent.next_frame())
            .await
            .is_err(),
        "a call on an expired connection must not be written to the agent"
    );
}

#[tokio::test]
async fn a_deadline_inside_a_live_session_costs_only_that_call() {
    let mut wired = wire_with_timeout(DEADLINE);

    // A session first: from here on the agent may be carrying the user's work,
    // and no overrun is worth taking that down.
    let opening = wired
        .connection
        .new_session(schema::NewSessionRequest::new("/tmp/acp-client-test"));
    let agent = async {
        let (id, _) = wired.agent.expect_request("session/new").await;
        wired.agent.respond(id, json!({ "sessionId": "s-1" })).await;
    };
    let (session, ()) = within_patience("session/new", futures_join(opening, agent)).await;
    assert_eq!(
        session.expect("the session opens").session_id.0.as_ref(),
        "s-1"
    );

    // Now a control call the agent is too busy to answer.
    let switch = wired
        .connection
        .set_session_mode(schema::SetSessionModeRequest::new("s-1", "plan"));
    let busy = async {
        let _ = wired.agent.expect_request("session/set_mode").await;
    };
    let (answer, ()) = within(
        "the deadline to end set_mode",
        DEADLINE_PATIENCE,
        futures_join(switch, busy),
    )
    .await;
    let Err(Error::Timeout { method, .. }) = answer else {
        panic!("expected a deadline failure, got {answer:?}");
    };
    assert_eq!(method, schema::AGENT_METHOD_NAMES.session_set_mode);

    // And the connection is untouched: the price of the overrun was the mode
    // that did not change, not the session. The next call goes through.
    assert!(!wired.connection.is_closed(), "the connection stays open");
    let again = wired
        .connection
        .set_session_mode(schema::SetSessionModeRequest::new("s-1", "plan"));
    let answering = async {
        let (id, _) = wired.agent.expect_request("session/set_mode").await;
        wired.agent.respond(id, json!({})).await;
    };
    let (second, ()) = within_patience("the next set_mode", futures_join(again, answering)).await;
    assert!(
        second.is_ok(),
        "a call after an overrun in a live session must still work, got {second:?}"
    );
}

#[tokio::test]
async fn a_turn_is_not_ended_by_the_control_deadline() {
    let mut wired = wire_with_timeout(DEADLINE);

    let call = wired
        .connection
        .prompt(schema::PromptRequest::new("s-1", vec![]));

    let agent = async {
        let (id, _) = wired.agent.expect_request("session/prompt").await;
        // Well past the control deadline. A turn is the agent working, and
        // work outlasting a control exchange is the normal case, not a fault.
        tokio::time::sleep(DEADLINE * 5).await;
        wired
            .agent
            .respond(id, json!({ "stopReason": "end_turn" }))
            .await;
    };

    let (answer, ()) = within_patience("the slow turn", futures_join(call, agent)).await;
    assert_eq!(
        answer.expect("the turn ends").stop_reason,
        schema::StopReason::EndTurn
    );
}

#[tokio::test]
async fn cancel_gets_out_while_a_request_is_still_unanswered() {
    let mut wired = wire();

    let call = wired
        .connection
        .prompt(schema::PromptRequest::new("s-1", vec![]));

    let agent = async {
        let _ = wired.agent.expect_request("session/prompt").await;
        // Nothing ever answers that prompt. `session/cancel` is the way a user
        // gets out of exactly this, so it must not be queued behind the caller
        // that is still waiting.
        wired
            .connection
            .cancel(&schema::CancelNotification::new("s-1"))
            .expect("the connection is open");

        let frame = wired.agent.next_frame().await;
        assert_eq!(frame["method"], json!("session/cancel"));
        assert!(frame.get("id").is_none(), "a notification carries no id");
    };

    // The turn is deliberately held open: whichever half finishes has to be the
    // agent's, because the prompt cannot resolve at all.
    let outcome = within_patience(
        "the cancel to reach the agent",
        futures::future::select(Box::pin(call), Box::pin(agent)),
    )
    .await;
    assert!(
        matches!(outcome, futures::future::Either::Right(_)),
        "the cancel reached the agent while the prompt was still waiting"
    );
}

#[tokio::test]
async fn set_mode_and_load_session_travel_with_their_parameters() {
    let mut wired = wire();

    let call = async {
        let mode = wired
            .connection
            .set_session_mode(schema::SetSessionModeRequest::new("s-1", "default"))
            .await?;
        let loaded = wired
            .connection
            .load_session(schema::LoadSessionRequest::new("s-1", "/tmp/project"))
            .await?;
        Ok::<_, Error>((mode, loaded))
    };

    let agent = async {
        let (id, params) = wired.agent.expect_request("session/set_mode").await;
        assert_eq!(params["sessionId"], json!("s-1"));
        assert_eq!(params["modeId"], json!("default"));
        wired.agent.respond(id, json!({})).await;

        let (id, params) = wired.agent.expect_request("session/load").await;
        assert_eq!(params["sessionId"], json!("s-1"));
        assert_eq!(params["cwd"], json!("/tmp/project"));
        wired.agent.respond(id, json!({})).await;
    };

    let (answer, ()) = within_patience("both calls", futures_join(call, agent)).await;
    answer.expect("both methods answered");
}

#[tokio::test]
async fn session_new_carries_our_cwd_and_our_mcp_servers_with_their_env() {
    // The whole point of the transport change: session identity stops being a
    // property of the process and becomes a parameter of this call.
    let mut wired = wire();

    let request =
        schema::NewSessionRequest::new("/tmp/project").mcp_servers(vec![schema::McpServer::Stdio(
            schema::McpServerStdio::new("sync", "/Applications/Sync.app/Contents/MacOS/git-sync")
                .args(vec!["mcp".to_owned(), "--root".to_owned()])
                .env(vec![schema::EnvVariable::new(
                    "SYNC_AGENT_SLUG",
                    "rust-impl-b",
                )]),
        )]);

    let call = wired.connection.new_session(request);
    let agent = async {
        let (id, params) = wired.agent.expect_request("session/new").await;
        assert_eq!(params["cwd"], json!("/tmp/project"));

        let server = &params["mcpServers"][0];
        assert_eq!(server["name"], json!("sync"));
        assert_eq!(
            server["command"],
            json!("/Applications/Sync.app/Contents/MacOS/git-sync")
        );
        assert_eq!(server["args"], json!(["mcp", "--root"]));
        assert_eq!(
            server["env"],
            json!([{ "name": "SYNC_AGENT_SLUG", "value": "rust-impl-b" }])
        );
        // A stdio server carries no `type` discriminator on the wire — that is
        // the form all four agents accepted.
        assert!(server.get("type").is_none(), "server frame: {server}");

        wired.agent.respond(id, json!({ "sessionId": "s-1" })).await;
    };

    let (answer, ()) = within_patience("session/new", futures_join(call, agent)).await;
    assert_eq!(answer.unwrap().session_id.0.as_ref(), "s-1");
}

/// The model list, and choosing from it, are one mechanism and it is the
/// protocol's own.
///
/// This is the half of "pick a model" the launch registry cannot answer:
/// `ModelPin` covers the agents that take a model as an argument or an
/// environment variable, and it is `None` for the ones that offer the choice
/// in protocol instead. Those answer `session/new` with `configOptions`, one of
/// them categorised `model`, and take the chosen id back on
/// `session/set_config_option`. A client that read only the registry would
/// offer no models at all on exactly the agents that have most to offer.
#[tokio::test]
async fn a_model_is_listed_by_the_session_and_chosen_through_the_protocol() {
    let mut wired = wire();

    let opening = wired
        .connection
        .new_session(schema::NewSessionRequest::new("/tmp/project"));
    let agent = async {
        let (id, _) = wired.agent.expect_request("session/new").await;
        wired
            .agent
            .respond(
                id,
                json!({
                    "sessionId": "s-1",
                    "configOptions": [{
                        "id": "model",
                        "name": "Model",
                        "category": "model",
                        "type": "select",
                        "currentValue": "sonnet",
                        "options": [
                            { "value": "sonnet", "name": "Claude Sonnet" },
                            { "value": "opus", "name": "Claude Opus" },
                        ],
                    }],
                }),
            )
            .await;
    };
    let (session, ()) = within_patience("session/new", futures_join(opening, agent)).await;

    let options = session
        .expect("the session opened")
        .config_options
        .expect("the agent advertised its configuration");
    let model = options
        .iter()
        .find(|option| option.category == Some(schema::SessionConfigOptionCategory::Model))
        .expect("one of the options is the model");
    let schema::SessionConfigKind::Select(select) = &model.kind else {
        panic!("a model is chosen from a list");
    };
    assert_eq!(select.current_value.0.as_ref(), "sonnet");

    // And the choice goes back the same way on every agent that offers it.
    let choosing = wired
        .connection
        .set_config_option(schema::SetSessionConfigOptionRequest::new(
            "s-1",
            model.id.clone(),
            schema::SessionConfigOptionValue::value_id("opus"),
        ));
    let answering = async {
        let (id, params) = wired
            .agent
            .expect_request("session/set_config_option")
            .await;
        assert_eq!(params["configId"], json!("model"));
        assert_eq!(params["value"], json!("opus"));
        // The answer is the whole option set again, not an acknowledgement:
        // choosing one option may change what the others offer, so the agent
        // restates them and the client replaces rather than patches.
        wired
            .agent
            .respond(
                id,
                json!({
                    "configOptions": [{
                        "id": "model",
                        "name": "Model",
                        "category": "model",
                        "type": "select",
                        "currentValue": "opus",
                        "options": [
                            { "value": "sonnet", "name": "Claude Sonnet" },
                            { "value": "opus", "name": "Claude Opus" },
                        ],
                    }],
                }),
            )
            .await;
    };
    let (chosen, ()) = within_patience(
        "session/set_config_option",
        futures_join(choosing, answering),
    )
    .await;
    let restated = chosen.expect("the model was accepted");
    let schema::SessionConfigKind::Select(select) = &restated.config_options[0].kind else {
        panic!("a model is chosen from a list");
    };
    assert_eq!(
        select.current_value.0.as_ref(),
        "opus",
        "the agent's restated options carry the choice"
    );
}

/// Runs both halves of a test concurrently.
///
/// The client call and the agent script have to interleave: the call does not
/// resolve until the agent answers, and the agent cannot answer a frame that
/// has not been written yet.
async fn futures_join<A, B>(
    a: impl std::future::Future<Output = A>,
    b: impl std::future::Future<Output = B>,
) -> (A, B) {
    futures::future::join(a, b).await
}
