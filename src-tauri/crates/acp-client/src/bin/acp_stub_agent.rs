//! A minimal ACP *agent* over stdio, for the one test that needs a process.
//!
//! It deliberately shares no code with the client in this crate — not the
//! framing, not the id correlation, not the dispatch. A stub built on the very
//! transport it is meant to exercise would agree with a bug in that transport
//! and the test would pass anyway.
//!
//! What it does is driven by the prompt text, so one binary covers every shape
//! the process test needs:
//!
//! | Prompt contains | Behaviour |
//! |---|---|
//! | `read:<path>`   | calls `fs/read_text_file` back, echoes the content, ends the turn |
//! | `write:<path>`  | calls `fs/write_text_file` back, ends the turn |
//! | `permission`    | calls `session/request_permission`, echoes the chosen option, ends the turn |
//! | `slow`          | streams one chunk, then waits for `session/cancel` |
//! | anything else   | echoes `PONG`, ends the turn |
//!
//! One behaviour is not driven by the prompt, because it happens before there
//! is one: `ACP_STUB_SILENT` names a method this stub takes and never answers,
//! while going on reading its stdin. That is the live-but-silent agent — the
//! shape a dying process cannot imitate, and the only one a deadline is for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Notify};

/// The session id this stub hands out. One session per process is enough.
const SESSION_ID: &str = "stub-session-1";

/// Names the one method this stub takes and never answers, if any.
const SILENT_METHOD: &str = "ACP_STUB_SILENT";

/// State shared by the read loop and the per-request tasks.
struct Stub {
    outgoing: mpsc::UnboundedSender<String>,
    /// Requests this stub sent to the client, awaiting their answers.
    pending: Mutex<HashMap<i64, oneshot::Sender<Value>>>,
    /// Bumped for each request the stub sends.
    next_id: Mutex<i64>,
    /// Raised by `session/cancel`.
    cancelled: Arc<Notify>,
}

impl Stub {
    fn send(&self, frame: &Value) {
        drop(self.outgoing.send(frame.to_string()));
    }

    fn respond(&self, id: &Value, result: &Value) {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn notify(&self, method: &str, params: &Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn chunk(&self, text: &str) {
        self.notify(
            "session/update",
            &json!({
                "sessionId": SESSION_ID,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": text },
                },
            }),
        );
    }

    /// Sends a request to the client and waits for its answer.
    async fn ask(&self, method: &str, params: Value) -> Option<Value> {
        let id = {
            let Ok(mut next) = self.next_id.lock() else {
                return None;
            };
            *next += 1;
            *next
        };

        let (tx, rx) = oneshot::channel();
        {
            let Ok(mut pending) = self.pending.lock() else {
                return None;
            };
            pending.insert(id, tx);
        }

        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));

        rx.await.ok()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(frame) = outgoing_rx.recv().await {
            if stdout.write_all(frame.as_bytes()).await.is_err()
                || stdout.write_all(b"\n").await.is_err()
                || stdout.flush().await.is_err()
            {
                break;
            }
        }
    });

    let stub = Arc::new(Stub {
        outgoing,
        pending: Mutex::new(HashMap::new()),
        next_id: Mutex::new(0),
        cancelled: Arc::new(Notify::new()),
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = frame.get("id").cloned();

        match (method, id) {
            (Some(method), Some(id)) => {
                tokio::spawn(handle_request(Arc::clone(&stub), method, id, frame));
            }
            (Some(method), None) => {
                if method == "session/cancel" {
                    stub.cancelled.notify_waiters();
                }
            }
            (None, Some(id)) => {
                // An answer to something this stub asked the client.
                let waiter = id
                    .as_i64()
                    .and_then(|id| stub.pending.lock().ok().and_then(|mut p| p.remove(&id)));
                if let Some(tx) = waiter {
                    drop(tx.send(frame.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
            (None, None) => {}
        }
    }

    drop(stub);
    drop(writer.await);
    Ok(())
}

/// Answers one client request.
async fn handle_request(stub: Arc<Stub>, method: String, id: Value, frame: Value) {
    if std::env::var(SILENT_METHOD).is_ok_and(|silent| silent == method) {
        // Deliberately no answer and no exit: the read loop is still running,
        // so the client sees a process that is up and simply says nothing.
        return;
    }

    match method.as_str() {
        "initialize" => stub.respond(
            &id,
            &json!({
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": true },
                "authMethods": [],
                "agentInfo": { "name": "acp-stub-agent", "version": "1.0.0" },
                // The child's own view of its environment. The spike proved
                // env arrival by reading a live process's environment rather
                // than by asking the model; this is the same idea, small.
                "_meta": { "stub/env": {
                    "ACP_STUB_MUST_NOT_SURVIVE": std::env::var("ACP_STUB_MUST_NOT_SURVIVE").ok(),
                    "ACP_STUB_MUST_SURVIVE": std::env::var("ACP_STUB_MUST_SURVIVE").ok(),
                } },
            }),
        ),
        "session/new" => {
            // Echo the session setup straight back through `_meta` so the test
            // can prove our `cwd` and `mcpServers` really arrived, rather than
            // trusting that they were sent.
            let params = frame.get("params").cloned().unwrap_or(Value::Null);
            stub.respond(
                &id,
                &json!({
                    "sessionId": SESSION_ID,
                    "_meta": { "stub/received": params },
                }),
            );
        }
        "session/prompt" => run_turn(&stub, &id, &frame).await,
        _ => stub.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("no such method: {method}") },
        })),
    }
}

/// Runs one turn, choosing its script from the prompt text.
async fn run_turn(stub: &Arc<Stub>, id: &Value, frame: &Value) {
    let text = prompt_text(frame);

    if text.contains("slow") {
        stub.chunk("1");
        // Ends only when the client cancels — which is exactly the acknowledgement
        // the protocol gives for `session/cancel`.
        stub.cancelled.notified().await;
        stub.respond(id, &json!({ "stopReason": "cancelled" }));
        return;
    }

    if let Some(path) = after(&text, "read:") {
        let answer = stub
            .ask(
                "fs/read_text_file",
                json!({ "sessionId": SESSION_ID, "path": path }),
            )
            .await;
        let content = answer
            .as_ref()
            .and_then(|answer| answer.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("<no content>");
        stub.chunk(content);
        stub.respond(id, &json!({ "stopReason": "end_turn" }));
        return;
    }

    if let Some(path) = after(&text, "write:") {
        stub.ask(
            "fs/write_text_file",
            json!({ "sessionId": SESSION_ID, "path": path, "content": "written by the stub" }),
        )
        .await;
        stub.chunk("written");
        stub.respond(id, &json!({ "stopReason": "end_turn" }));
        return;
    }

    if text.contains("permission") {
        let answer = stub
            .ask(
                "session/request_permission",
                json!({
                    "sessionId": SESSION_ID,
                    "toolCall": {
                        "toolCallId": "stub-call-1",
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
        let chosen = answer
            .as_ref()
            .and_then(|answer| answer.pointer("/outcome/optionId"))
            .and_then(Value::as_str)
            .unwrap_or("<none>");
        stub.chunk(chosen);
        stub.respond(id, &json!({ "stopReason": "end_turn" }));
        return;
    }

    stub.chunk("PONG");
    stub.respond(id, &json!({ "stopReason": "end_turn" }));
}

/// Concatenates the text blocks of a `session/prompt`.
fn prompt_text(frame: &Value) -> String {
    frame
        .pointer("/params/prompt")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// The rest of `text` after `marker`, up to the next space.
fn after(text: &str, marker: &str) -> Option<String> {
    let rest = text.split_once(marker)?.1;
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}
