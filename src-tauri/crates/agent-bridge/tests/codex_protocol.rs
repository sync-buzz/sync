#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use agent_bridge::serve_codex;
use serde_json::{json, Value};
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};

type Reader = BufReader<ReadHalf<tokio::io::DuplexStream>>;
type Writer = WriteHalf<tokio::io::DuplexStream>;

fn harness() -> (Reader, Writer, Reader, Writer, tokio::task::JoinHandle<()>) {
    let (acp_peer, acp_bridge) = tokio::io::duplex(64 * 1024);
    let (codex_peer, codex_bridge) = tokio::io::duplex(64 * 1024);
    let (acp_read, acp_write) = split(acp_peer);
    let (bridge_acp_read, bridge_acp_write) = split(acp_bridge);
    let (codex_read, codex_write) = split(codex_peer);
    let (bridge_codex_read, bridge_codex_write) = split(codex_bridge);
    let task = tokio::spawn(async move {
        serve_codex(
            bridge_acp_read,
            bridge_acp_write,
            bridge_codex_read,
            bridge_codex_write,
        )
        .await
        .expect("bridge serves both streams");
    });
    (
        BufReader::new(acp_read),
        acp_write,
        BufReader::new(codex_read),
        codex_write,
        task,
    )
}

async fn send(writer: &mut Writer, frame: Value) {
    writer
        .write_all(format!("{frame}\n").as_bytes())
        .await
        .expect("frame written");
}

async fn recv(reader: &mut Reader) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("frame arrived in time")
        .expect("frame read");
    serde_json::from_str(&line).expect("JSON-RPC frame")
}

async fn initialize(
    acp_reader: &mut Reader,
    acp_writer: &mut Writer,
    codex_reader: &mut Reader,
    codex_writer: &mut Writer,
) {
    send(
        acp_writer,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": 1 } }),
    )
    .await;
    let request = recv(codex_reader).await;
    assert_eq!(request["method"], json!("initialize"));
    send(
        codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "userAgent": "codex_cli_rs/0.147.0",
                "codexHome": "/tmp/codex",
                "platformFamily": "unix",
                "platformOs": "macos"
            }
        }),
    )
    .await;
    assert_eq!(recv(codex_reader).await["method"], json!("initialized"));
    let response = recv(acp_reader).await;
    assert_eq!(response["id"], json!(1));
    assert_eq!(response["result"]["protocolVersion"], json!(1));
    assert_eq!(
        response["result"]["agentInfo"]["name"],
        json!("agent-bridge-codex")
    );
}

#[tokio::test]
async fn full_acp_turn_is_backed_by_async_codex_turn() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": "/work/repo",
                "mcpServers": [{
                    "name": "sync",
                    "command": "/Applications/Sync.app/Contents/MacOS/git-sync",
                    "args": ["mcp", "--root", "/work/repo"],
                    "env": [{ "name": "SYNC_AGENT_SLUG", "value": "rust-impl" }]
                }]
            }
        }),
    )
    .await;
    let start_thread = recv(&mut codex_reader).await;
    assert_eq!(start_thread["method"], json!("thread/start"));
    assert_eq!(start_thread["params"]["cwd"], json!("/work/repo"));
    assert_eq!(
        start_thread["params"]["config"]["mcp_servers"]["sync"]["env"]["SYNC_AGENT_SLUG"],
        json!("rust-impl")
    );
    send(
        &mut codex_writer,
        json!({ "jsonrpc": "2.0", "id": start_thread["id"], "result": { "thread": { "id": "thread-1" } } }),
    )
    .await;
    assert_eq!(
        recv(&mut acp_reader).await["result"]["sessionId"],
        json!("thread-1")
    );

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {
                "sessionId": "thread-1",
                "prompt": [{ "type": "text", "text": "Reply PONG" }]
            }
        }),
    )
    .await;
    let start_turn = recv(&mut codex_reader).await;
    assert_eq!(start_turn["method"], json!("turn/start"));
    assert_eq!(
        start_turn["params"]["input"][0]["text"],
        json!("Reply PONG")
    );
    send(
        &mut codex_writer,
        json!({ "jsonrpc": "2.0", "id": start_turn["id"], "result": { "turn": { "id": "turn-1", "status": "inProgress", "items": [] } } }),
    )
    .await;
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "method": "item/agentMessage/delta",
            "params": { "threadId": "thread-1", "turnId": "turn-1", "itemId": "msg-1", "delta": "PONG" }
        }),
    )
    .await;
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "method": "turn/completed",
            "params": { "threadId": "thread-1", "turn": { "id": "turn-1", "status": "completed", "items": [] } }
        }),
    )
    .await;

    let update = recv(&mut acp_reader).await;
    assert_eq!(update["method"], json!("session/update"));
    assert_eq!(update["params"]["update"]["content"]["text"], json!("PONG"));
    let completed = recv(&mut acp_reader).await;
    assert_eq!(completed["id"], json!(3));
    assert_eq!(completed["result"]["stopReason"], json!("end_turn"));

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn acp_load_resumes_the_same_codex_thread() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "session/load",
            "params": { "sessionId": "thread-1", "cwd": "/work/repo", "mcpServers": [] }
        }),
    )
    .await;
    let resume = recv(&mut codex_reader).await;
    assert_eq!(resume["method"], json!("thread/resume"));
    assert_eq!(resume["params"]["threadId"], json!("thread-1"));

    // What Codex actually answers: the whole thread, turns oldest first, every
    // one of them `itemsView: "full"`. Shaped after a real `thread/resume` off
    // `codex-cli 0.144.5` — the field names are the app-server's, not ours.
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": resume["id"],
            "result": { "thread": { "id": "thread-1", "turns": [
                {
                    "id": "turn-1",
                    "status": "completed",
                    "startedAt": 1_787_302_394_i64,
                    "itemsView": "full",
                    "items": [
                        {"type": "userMessage", "id": "i-1",
                         "content": [{"type": "text", "text": "Why is it slow?"}]},
                        {"type": "reasoning", "id": "i-2",
                         "summary": ["**Reading the parser**", "**Timing it**"]},
                        {"type": "commandExecution", "id": "i-3",
                         "command": "cargo bench", "status": "completed"},
                        {"type": "agentMessage", "id": "i-4",
                         "text": "The parser re-reads the file each pass."}
                    ]
                }
            ]}}
        }),
    )
    .await;

    // The replay comes first and the response is what ends it. This is the
    // whole of the fix: the old bridge answered `{}` and sent nothing, so Codex
    // remembered the conversation and the window did not.
    let said = recv(&mut acp_reader).await;
    assert_eq!(said["method"], json!("session/update"));
    assert_eq!(
        said["params"]["update"]["sessionUpdate"],
        json!("user_message_chunk"),
        "the person's own words reach the window only on a replay: {said}"
    );
    assert_eq!(
        said["params"]["update"]["content"]["text"],
        json!("Why is it slow?")
    );

    let thought = recv(&mut acp_reader).await;
    assert_eq!(
        thought["params"]["update"]["sessionUpdate"],
        json!("agent_thought_chunk")
    );
    assert_eq!(
        thought["params"]["update"]["content"]["text"],
        json!("**Reading the parser**\n\n**Timing it**"),
        "the summary joins as paragraphs, which is what the live delta stream builds"
    );

    // A tool call replays as the two updates the window saw the first time.
    let call = recv(&mut acp_reader).await;
    assert_eq!(
        call["params"]["update"]["sessionUpdate"],
        json!("tool_call")
    );
    assert_eq!(call["params"]["update"]["title"], json!("cargo bench"));
    assert_eq!(call["params"]["update"]["kind"], json!("execute"));
    let settled = recv(&mut acp_reader).await;
    assert_eq!(
        settled["params"]["update"]["sessionUpdate"],
        json!("tool_call_update")
    );
    assert_eq!(settled["params"]["update"]["status"], json!("completed"));
    assert_eq!(settled["params"]["update"]["toolCallId"], json!("i-3"));

    let answered = recv(&mut acp_reader).await;
    assert_eq!(
        answered["params"]["update"]["sessionUpdate"],
        json!("agent_message_chunk")
    );
    assert_eq!(
        answered["params"]["update"]["content"]["text"],
        json!("The parser re-reads the file each pass.")
    );

    // And only then the answer, which is what says the replay is over.
    assert_eq!(recv(&mut acp_reader).await["result"], json!({}));

    task.abort();
    let _ = task.await;
}

/// A thread with no history replays nothing and still answers.
///
/// The empty case is not the old behaviour by another name: it is a thread that
/// genuinely has nothing in it, and the difference between "no turns" and "the
/// bridge does not do replay" is the whole of the previous fault.
#[tokio::test]
async fn a_resumed_thread_with_no_turns_answers_without_replaying() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "session/load",
            "params": { "sessionId": "thread-1", "cwd": "/work/repo", "mcpServers": [] }
        }),
    )
    .await;
    let resume = recv(&mut codex_reader).await;
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": resume["id"],
            "result": { "thread": { "id": "thread-1", "turns": [] } }
        }),
    )
    .await;
    assert_eq!(recv(&mut acp_reader).await["result"], json!({}));

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn codex_approval_round_trips_through_acp_permission() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "call-1",
                "startedAtMs": 1,
                "command": "touch /tmp/probe",
                "cwd": "/work/repo"
            }
        }),
    )
    .await;
    let permission = recv(&mut acp_reader).await;
    assert_eq!(permission["method"], json!("session/request_permission"));
    assert_eq!(
        permission["params"]["toolCall"]["title"],
        json!("touch /tmp/probe")
    );
    assert_eq!(
        permission["params"]["options"][1]["kind"],
        json!("allow_always")
    );

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": permission["id"],
            "result": { "outcome": { "outcome": "selected", "optionId": "allow-session" } }
        }),
    )
    .await;
    let answer = recv(&mut codex_reader).await;
    assert_eq!(answer["id"], json!(77));
    assert_eq!(answer["result"]["decision"], json!("acceptForSession"));

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn codex_mcp_tool_approval_round_trips_through_acp_permission() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 78,
            "method": "mcpServer/elicitation/request",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "serverName": "sync",
                "mode": "form",
                "message": "Allow Sync to list this project's memory?",
                "requestedSchema": { "type": "object", "properties": {} },
                "_meta": {
                    "codex_approval_kind": "mcp_tool_call",
                    "tool_name": "sync_projects",
                    "tool_params": {},
                    "persist": "session"
                }
            }
        }),
    )
    .await;
    let permission = recv(&mut acp_reader).await;
    assert_eq!(permission["method"], json!("session/request_permission"));
    assert_eq!(
        permission["params"]["toolCall"]["title"],
        json!("sync/sync_projects")
    );

    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": permission["id"],
            "result": { "outcome": { "outcome": "selected", "optionId": "allow-once" } }
        }),
    )
    .await;
    let answer = recv(&mut codex_reader).await;
    assert_eq!(answer["id"], json!(78));
    assert_eq!(answer["result"]["action"], json!("accept"));
    assert_eq!(answer["result"]["content"], Value::Null);
    assert_eq!(answer["result"]["_meta"], Value::Null);

    task.abort();
    let _ = task.await;
}

/// A picture Codex made reaches the window, live and on a replay alike.
///
/// The item's shape is the app-server's own, read off
/// `codex app-server generate-json-schema` for `codex-cli 0.144.5`:
/// `imageGeneration` carries `result` — base64 with no prefix, the same field
/// the model's `image_generation_call` returns — beside a `savedPath` Codex
/// wrote it to. Both paths are asserted in one test on purpose: they are one
/// translation, and a test for only one of them would go green while a
/// resumed conversation lost every picture in it.
#[tokio::test]
async fn a_generated_image_becomes_acp_image_content_live_and_on_a_replay() {
    let (mut acp_reader, mut acp_writer, mut codex_reader, mut codex_writer, task) = harness();
    initialize(
        &mut acp_reader,
        &mut acp_writer,
        &mut codex_reader,
        &mut codex_writer,
    )
    .await;

    // Live. `item/started` fires for the same id before there is an image, and
    // must produce nothing: a chunk carrying an empty string is a picture that
    // cannot be drawn, and it would end the block the agent was writing.
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "method": "item/started",
            "params": { "threadId": "thread-1", "item": {
                "type": "imageGeneration", "id": "img-1",
                "status": "inProgress", "revisedPrompt": null, "result": ""
            }}
        }),
    )
    .await;
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": { "threadId": "thread-1", "item": {
                "type": "imageGeneration", "id": "img-1",
                "status": "completed",
                "revisedPrompt": "A red circle on white",
                "result": "aGVsbG8=",
                "savedPath": "/work/repo/.codex/img-1.png"
            }}
        }),
    )
    .await;

    let drawn = recv(&mut acp_reader).await;
    assert_eq!(drawn["method"], json!("session/update"));
    assert_eq!(
        drawn["params"]["update"]["sessionUpdate"],
        json!("agent_message_chunk"),
        "a picture is what the agent answered with, so it belongs in its message: {drawn}"
    );
    assert_eq!(drawn["params"]["update"]["content"]["type"], json!("image"));
    assert_eq!(
        drawn["params"]["update"]["content"]["data"],
        json!("aGVsbG8="),
        "the base64 crosses as it arrived — a `data:` prefix added here decodes to nothing"
    );
    assert_eq!(
        drawn["params"]["update"]["content"]["mimeType"],
        json!("image/png")
    );

    // On a replay, out of `thread/resume`'s own turns. The same picture, and it
    // has to read identically: a conversation that came back without it would
    // be a different conversation.
    send(
        &mut acp_writer,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "session/load",
            "params": { "sessionId": "thread-1", "cwd": "/work/repo", "mcpServers": [] }
        }),
    )
    .await;
    let resume = recv(&mut codex_reader).await;
    send(
        &mut codex_writer,
        json!({
            "jsonrpc": "2.0",
            "id": resume["id"],
            "result": { "thread": { "id": "thread-1", "turns": [{
                "id": "turn-1",
                "status": "completed",
                "itemsView": "full",
                "items": [{
                    "type": "imageGeneration", "id": "img-1",
                    "status": "completed",
                    "revisedPrompt": "A red circle on white",
                    "result": "aGVsbG8=",
                    "savedPath": "/work/repo/.codex/img-1.jpeg"
                }]
            }]}}
        }),
    )
    .await;

    let replayed = recv(&mut acp_reader).await;
    assert_eq!(
        replayed["params"]["update"]["sessionUpdate"],
        json!("agent_message_chunk")
    );
    assert_eq!(
        replayed["params"]["update"]["content"]["data"],
        json!("aGVsbG8=")
    );
    assert_eq!(
        replayed["params"]["update"]["content"]["mimeType"],
        json!("image/jpeg"),
        "the media type is read off the file Codex saved beside it: {replayed}"
    );
    assert_eq!(recv(&mut acp_reader).await["result"], json!({}));

    task.abort();
    let _ = task.await;
}
