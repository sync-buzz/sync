//! ACP server backed by the official Codex app-server protocol.
//!
//! Both protocols are newline-delimited JSON-RPC over stdio, but their turn
//! models differ: ACP keeps `session/prompt` open until the turn ends, while
//! Codex answers `turn/start` immediately and later emits `turn/completed`.
//! This module owns that correlation so an ACP caller never learns it exists.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::{Error, Result};

const MIN_CODEX_VERSION: (u64, u64, u64) = (0, 147, 0);
const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How to start the Codex process behind the bridge.
#[derive(Debug, Clone)]
pub struct CodexOptions {
    /// Resolved Codex executable, or the bare `codex` name.
    pub program: PathBuf,
    /// Codex `-c key=value` overrides. Kept verbatim so model, approval and
    /// sandbox policy remain owned by Codex rather than duplicated here.
    pub config_overrides: Vec<String>,
}

impl Default for CodexOptions {
    fn default() -> Self {
        Self {
            program: PathBuf::from("codex"),
            config_overrides: Vec::new(),
        }
    }
}

/// Run the bridge on this process's stdio and a child `codex app-server`.
///
/// # Errors
///
/// Returns an error when Codex cannot be started or either protocol stream
/// cannot be read or written.
pub fn run_codex_stdio(options: CodexOptions) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    runtime.block_on(async move {
        let mut command = Command::new(&options.program);
        command.args(["app-server", "--stdio"]);
        for value in &options.config_overrides {
            command.args(["-c", value]);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn()?;
        let codex_writer = child.stdin.take().ok_or(Error::MissingPipe("stdin"))?;
        let codex_reader = child.stdout.take().ok_or(Error::MissingPipe("stdout"))?;

        let result = serve_codex(
            tokio::io::stdin(),
            tokio::io::stdout(),
            codex_reader,
            codex_writer,
        )
        .await;
        drop(child.kill().await);
        result
    })
}

/// Translate between an ACP client and a Codex app-server connection.
///
/// The stream seam is public so the complete bridge is testable with two
/// in-memory duplexes. Production is only a process-spawn wrapper around it.
///
/// # Errors
///
/// Returns the first I/O or JSON encoding failure on either side.
pub async fn serve_codex<AR, AW, CR, CW>(
    acp_reader: AR,
    mut acp_writer: AW,
    codex_reader: CR,
    mut codex_writer: CW,
) -> Result<()>
where
    AR: AsyncRead + Unpin,
    AW: AsyncWrite + Unpin,
    CR: AsyncRead + Unpin,
    CW: AsyncWrite + Unpin,
{
    let mut acp_lines = BufReader::new(acp_reader).lines();
    let mut codex_lines = BufReader::new(codex_reader).lines();
    let mut state = State::default();

    loop {
        tokio::select! {
            line = acp_lines.next_line() => {
                let Some(line) = line? else { break };
                let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                    tracing::debug!(%line, "skipping non-JSON ACP input");
                    continue;
                };
                for outgoing in state.handle_acp(&frame) {
                    match outgoing {
                        Outgoing::Acp(frame) => write_frame(&mut acp_writer, "ACP", &frame).await?,
                        Outgoing::Codex(frame) => write_frame(&mut codex_writer, "Codex", &frame).await?,
                    }
                }
            }
            line = codex_lines.next_line() => {
                let Some(line) = line? else { break };
                let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                    tracing::debug!(%line, "skipping non-JSON Codex output");
                    continue;
                };
                for outgoing in state.handle_codex(&frame) {
                    match outgoing {
                        Outgoing::Acp(frame) => write_frame(&mut acp_writer, "ACP", &frame).await?,
                        Outgoing::Codex(frame) => write_frame(&mut codex_writer, "Codex", &frame).await?,
                    }
                }
            }
        }
    }
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    side: &'static str,
    frame: &Value,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(frame).map_err(|source| Error::Encode { side, source })?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug)]
enum Outgoing {
    Acp(Value),
    Codex(Value),
}

#[derive(Debug)]
enum PendingCodex {
    Initialize { acp_id: Value },
    NewSession { acp_id: Value },
    LoadSession { acp_id: Value, session_id: String },
    StartTurn { acp_id: Value, session_id: String },
    Ignore,
}

#[derive(Debug)]
struct PendingPrompt {
    acp_id: Value,
    session_id: String,
}

#[derive(Debug, Clone, Copy)]
enum ApprovalKind {
    Command,
    FileChange,
    Permissions,
    LegacyPatch,
    LegacyCommand,
    McpTool,
}

#[derive(Debug)]
struct PendingApproval {
    codex_id: Value,
    kind: ApprovalKind,
    requested_permissions: Option<Value>,
}

#[derive(Debug, Default)]
struct State {
    next_codex_id: i64,
    next_acp_id: i64,
    pending_codex: HashMap<i64, PendingCodex>,
    prompts: HashMap<String, PendingPrompt>,
    active_turn_by_session: HashMap<String, String>,
    pending_approvals: HashMap<i64, PendingApproval>,
}

impl State {
    fn handle_acp(&mut self, frame: &Value) -> Vec<Outgoing> {
        if frame.get("method").is_some() {
            return self.acp_call(frame);
        }
        self.acp_response(frame)
    }

    fn acp_call(&mut self, frame: &Value) -> Vec<Outgoing> {
        let method = frame["method"].as_str().unwrap_or_default();
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        let id = frame.get("id").cloned();

        match method {
            "initialize" => id.map_or_else(Vec::new, |acp_id| {
                let codex_id = self.codex_request_id(PendingCodex::Initialize { acp_id });
                vec![Outgoing::Codex(request(
                    codex_id,
                    "initialize",
                    json!({
                        "clientInfo": { "name": "agent-bridge", "version": BRIDGE_VERSION },
                        "capabilities": { "experimentalApi": true }
                    }),
                ))]
            }),
            "session/new" => id.map_or_else(Vec::new, |acp_id| {
                let codex_id = self.codex_request_id(PendingCodex::NewSession { acp_id });
                let mut thread =
                    json!({ "cwd": params.get("cwd").cloned().unwrap_or(Value::Null) });
                let servers = codex_mcp_servers(params.get("mcpServers"));
                if !servers.is_empty() {
                    thread["config"] = json!({ "mcp_servers": servers });
                }
                vec![Outgoing::Codex(request(codex_id, "thread/start", thread))]
            }),
            "session/load" => id.map_or_else(Vec::new, |acp_id| {
                let session_id = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let codex_id = self.codex_request_id(PendingCodex::LoadSession {
                    acp_id,
                    session_id: session_id.clone(),
                });
                let mut thread = json!({
                    "threadId": session_id,
                    "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                });
                let servers = codex_mcp_servers(params.get("mcpServers"));
                if !servers.is_empty() {
                    thread["config"] = json!({ "mcp_servers": servers });
                }
                vec![Outgoing::Codex(request(codex_id, "thread/resume", thread))]
            }),
            "session/prompt" => id.map_or_else(Vec::new, |acp_id| {
                let session_id = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let codex_id = self.codex_request_id(PendingCodex::StartTurn {
                    acp_id,
                    session_id: session_id.clone(),
                });
                vec![Outgoing::Codex(request(
                    codex_id,
                    "turn/start",
                    json!({
                        "threadId": session_id,
                        "input": codex_input(params.get("prompt")),
                    }),
                ))]
            }),
            "session/cancel" => {
                let session_id = params["sessionId"].as_str().unwrap_or_default();
                let Some(turn_id) = self.active_turn_by_session.get(session_id).cloned() else {
                    return Vec::new();
                };
                let codex_id = self.codex_request_id(PendingCodex::Ignore);
                vec![Outgoing::Codex(request(
                    codex_id,
                    "turn/interrupt",
                    json!({ "threadId": session_id, "turnId": turn_id }),
                ))]
            }
            _ => id.map_or_else(Vec::new, |id| {
                vec![Outgoing::Acp(error_response(
                    id,
                    -32601,
                    format!("method not supported by Codex bridge: {method}"),
                    None,
                ))]
            }),
        }
    }

    fn acp_response(&mut self, frame: &Value) -> Vec<Outgoing> {
        let Some(id) = frame.get("id").and_then(Value::as_i64) else {
            return Vec::new();
        };
        let Some(pending) = self.pending_approvals.remove(&id) else {
            return Vec::new();
        };
        let option = frame
            .pointer("/result/outcome/optionId")
            .and_then(Value::as_str)
            .unwrap_or("reject-once");
        let result = approval_result(pending.kind, option, pending.requested_permissions.as_ref());
        vec![Outgoing::Codex(json!({
            "jsonrpc": "2.0",
            "id": pending.codex_id,
            "result": result,
        }))]
    }

    fn handle_codex(&mut self, frame: &Value) -> Vec<Outgoing> {
        if frame.get("method").is_some() {
            if frame.get("id").is_some() {
                return self.codex_request(frame);
            }
            return self.codex_notification(frame);
        }
        self.codex_response(frame)
    }

    fn codex_response(&mut self, frame: &Value) -> Vec<Outgoing> {
        let Some(id) = frame.get("id").and_then(Value::as_i64) else {
            return Vec::new();
        };
        let Some(pending) = self.pending_codex.remove(&id) else {
            return Vec::new();
        };
        if let Some(error) = frame.get("error") {
            let acp_id = match pending {
                PendingCodex::Initialize { acp_id }
                | PendingCodex::NewSession { acp_id }
                | PendingCodex::LoadSession { acp_id, .. }
                | PendingCodex::StartTurn { acp_id, .. } => Some(acp_id),
                PendingCodex::Ignore => None,
            };
            return acp_id.map_or_else(Vec::new, |acp_id| {
                vec![Outgoing::Acp(json!({
                    "jsonrpc": "2.0",
                    "id": acp_id,
                    "error": error,
                }))]
            });
        }
        let result = frame.get("result").cloned().unwrap_or_else(|| json!({}));

        match pending {
            PendingCodex::Initialize { acp_id } => {
                let user_agent = result["userAgent"].as_str().unwrap_or_default();
                if !supported_codex(user_agent) {
                    return vec![Outgoing::Acp(error_response(
                        acp_id,
                        -32000,
                        format!(
                            "Codex {user_agent:?} is too old; agent-bridge requires 0.147.0 or newer"
                        ),
                        None,
                    ))];
                }
                vec![
                    Outgoing::Codex(notification("initialized", json!({}))),
                    Outgoing::Acp(json!({
                        "jsonrpc": "2.0",
                        "id": acp_id,
                        "result": {
                            "protocolVersion": 1,
                            "agentCapabilities": {
                                "loadSession": true,
                                "promptCapabilities": { "image": true, "audio": false, "embeddedContext": true },
                                "mcpCapabilities": { "http": false, "sse": false }
                            },
                            "authMethods": [],
                            "agentInfo": { "name": "agent-bridge-codex", "title": "Codex", "version": BRIDGE_VERSION },
                            "_meta": { "codexUserAgent": user_agent }
                        }
                    })),
                ]
            }
            PendingCodex::NewSession { acp_id } => {
                let thread_id = result.pointer("/thread/id").and_then(Value::as_str);
                match thread_id {
                    Some(thread_id) => vec![Outgoing::Acp(success_response(
                        acp_id,
                        json!({ "sessionId": thread_id }),
                    ))],
                    None => vec![Outgoing::Acp(error_response(
                        acp_id,
                        -32603,
                        "Codex thread/start returned no thread id",
                        Some(result),
                    ))],
                }
            }
            PendingCodex::LoadSession { acp_id, session_id } => {
                resumed_session_response(acp_id, &session_id, result)
            }
            PendingCodex::StartTurn { acp_id, session_id } => {
                let turn_id = result.pointer("/turn/id").and_then(Value::as_str);
                match turn_id {
                    Some(turn_id) => {
                        self.active_turn_by_session
                            .insert(session_id.clone(), turn_id.to_owned());
                        self.prompts
                            .insert(turn_id.to_owned(), PendingPrompt { acp_id, session_id });
                        Vec::new()
                    }
                    None => vec![Outgoing::Acp(error_response(
                        acp_id,
                        -32603,
                        "Codex turn/start returned no turn id",
                        Some(result),
                    ))],
                }
            }
            PendingCodex::Ignore => Vec::new(),
        }
    }

    fn codex_notification(&mut self, frame: &Value) -> Vec<Outgoing> {
        let method = frame["method"].as_str().unwrap_or_default();
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = params["threadId"].as_str().unwrap_or_default();
        if session_id.is_empty() {
            return Vec::new();
        }

        match method {
            "item/agentMessage/delta" => update(
                session_id,
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": params["delta"] },
                }),
            ),
            "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => update(
                session_id,
                json!({
                    "sessionUpdate": "agent_thought_chunk",
                    "content": { "type": "text", "text": params["delta"] },
                }),
            ),
            "item/started" => tool_update(session_id, params.get("item"), false),
            "item/completed" => tool_update(session_id, params.get("item"), true),
            "turn/plan/updated" => {
                let entries = params["plan"]
                    .as_array()
                    .map(|steps| {
                        steps.iter().map(|step| json!({
                            "content": step["step"],
                            "priority": "medium",
                            "status": plan_status(step["status"].as_str().unwrap_or_default()),
                        })).collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                update(
                    session_id,
                    json!({ "sessionUpdate": "plan", "entries": entries }),
                )
            }
            "thread/tokenUsage/updated" => {
                let used = params
                    .pointer("/tokenUsage/total/totalTokens")
                    .cloned()
                    .unwrap_or(json!(0));
                let size = params
                    .pointer("/tokenUsage/modelContextWindow")
                    .cloned()
                    .unwrap_or(json!(0));
                update(
                    session_id,
                    json!({ "sessionUpdate": "usage_update", "used": used, "size": size }),
                )
            }
            "turn/completed" => self.turn_completed(&params),
            _ => Vec::new(),
        }
    }

    fn turn_completed(&mut self, params: &Value) -> Vec<Outgoing> {
        let Some(turn_id) = params.pointer("/turn/id").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(prompt) = self.prompts.remove(turn_id) else {
            return Vec::new();
        };
        self.active_turn_by_session.remove(&prompt.session_id);
        match params.pointer("/turn/status").and_then(Value::as_str) {
            Some("completed") => vec![Outgoing::Acp(success_response(
                prompt.acp_id,
                json!({ "stopReason": "end_turn" }),
            ))],
            Some("interrupted") => vec![Outgoing::Acp(success_response(
                prompt.acp_id,
                json!({ "stopReason": "cancelled" }),
            ))],
            status => {
                let detail = params
                    .pointer("/turn/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex turn failed");
                vec![Outgoing::Acp(error_response(
                    prompt.acp_id,
                    -32603,
                    format!("{detail} ({})", status.unwrap_or("unknown status")),
                    params.pointer("/turn/error").cloned(),
                ))]
            }
        }
    }

    fn codex_request(&mut self, frame: &Value) -> Vec<Outgoing> {
        let method = frame["method"].as_str().unwrap_or_default();
        if method == "mcpServer/elicitation/request" {
            return self.codex_mcp_elicitation(frame);
        }
        let Some(kind) = approval_kind(method) else {
            return vec![Outgoing::Codex(error_response(
                frame["id"].clone(),
                -32601,
                format!("agent-bridge does not handle Codex request {method}"),
                None,
            ))];
        };
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = params["threadId"]
            .as_str()
            .or_else(|| params["conversationId"].as_str())
            .unwrap_or_default();
        let tool_call_id = params["itemId"]
            .as_str()
            .or_else(|| params["callId"].as_str())
            .unwrap_or("codex-approval");
        let title = approval_title(kind, &params);
        let locations = approval_locations(kind, &params);
        let acp_id = self.next_acp_request_id();
        self.pending_approvals.insert(
            acp_id,
            PendingApproval {
                codex_id: frame["id"].clone(),
                kind,
                requested_permissions: params.get("permissions").cloned(),
            },
        );
        vec![Outgoing::Acp(request(
            acp_id,
            "session/request_permission",
            json!({
                "sessionId": session_id,
                "toolCall": {
                    "toolCallId": tool_call_id,
                    "title": title,
                    "kind": match kind { ApprovalKind::FileChange | ApprovalKind::LegacyPatch => "edit", _ => "execute" },
                    "locations": locations,
                },
                "options": [
                    { "optionId": "allow-once", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "allow-session", "name": "Allow always", "kind": "allow_always" },
                    { "optionId": "reject-once", "name": "Deny", "kind": "reject_once" },
                    { "optionId": "reject-session", "name": "Deny and stop", "kind": "reject_always" }
                ]
            }),
        ))]
    }

    fn codex_mcp_elicitation(&mut self, frame: &Value) -> Vec<Outgoing> {
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        if !is_mcp_tool_approval(&params) {
            return vec![Outgoing::Codex(success_response(
                frame["id"].clone(),
                json!({ "action": "decline", "content": null, "_meta": null }),
            ))];
        }

        let session_id = params["threadId"].as_str().unwrap_or_default();
        let server = params["serverName"].as_str().unwrap_or("mcp");
        let tool = params
            .pointer("/_meta/tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let title = format!("{server}/{tool}");
        let message = params["message"].as_str().unwrap_or(&title);
        let raw_input = params
            .pointer("/_meta/tool_params")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut options =
            vec![json!({ "optionId": "allow-once", "name": "Allow", "kind": "allow_once" })];
        if mcp_approval_supports(&params, "session") {
            options.push(json!({
                "optionId": "allow-session",
                "name": "Allow for this session",
                "kind": "allow_always"
            }));
        }
        if mcp_approval_supports(&params, "always") {
            options.push(json!({
                "optionId": "allow-always",
                "name": "Always allow",
                "kind": "allow_always"
            }));
        }
        options.push(json!({
            "optionId": "cancel",
            "name": "Cancel",
            "kind": "reject_once"
        }));

        let acp_id = self.next_acp_request_id();
        self.pending_approvals.insert(
            acp_id,
            PendingApproval {
                codex_id: frame["id"].clone(),
                kind: ApprovalKind::McpTool,
                requested_permissions: None,
            },
        );
        vec![Outgoing::Acp(request(
            acp_id,
            "session/request_permission",
            json!({
                "sessionId": session_id,
                "toolCall": {
                    "toolCallId": format!("mcp-approval-{acp_id}"),
                    "title": title,
                    "kind": "execute",
                    "rawInput": raw_input,
                    "_meta": { "message": message }
                },
                "options": options
            }),
        ))]
    }

    fn codex_request_id(&mut self, pending: PendingCodex) -> i64 {
        self.next_codex_id += 1;
        self.pending_codex.insert(self.next_codex_id, pending);
        self.next_codex_id
    }

    fn next_acp_request_id(&mut self) -> i64 {
        if self.next_acp_id < 1_000_000 {
            self.next_acp_id = 1_000_000;
        }
        self.next_acp_id += 1;
        self.next_acp_id
    }
}

fn resumed_session_response(acp_id: Value, session_id: &str, result: Value) -> Vec<Outgoing> {
    match result.pointer("/thread/id").and_then(Value::as_str) {
        Some(resumed_id) if resumed_id == session_id => {
            // The conversation, before the answer. `session/load` in ACP is not
            // an acknowledgement — it is the agent replaying what was said as
            // ordinary `session/update` notifications, and the response is what
            // marks the end of the replay. This used to answer `{}` and send
            // nothing, so Codex got its own context back and the window got a
            // conversation with nothing in it.
            let mut replay = replayed_turns(session_id, &result);
            replay.push(Outgoing::Acp(success_response(acp_id, json!({}))));
            replay
        }
        Some(resumed_id) => vec![Outgoing::Acp(error_response(
            acp_id,
            -32603,
            format!("Codex resumed thread {resumed_id}, expected {session_id}"),
            Some(result),
        ))],
        None => vec![Outgoing::Acp(error_response(
            acp_id,
            -32603,
            "Codex thread/resume returned no thread id",
            Some(result),
        ))],
    }
}

/// What was said in a resumed thread, as the updates a live turn would have
/// sent.
///
/// `thread/resume` answers with the whole history already — turns oldest first,
/// each with `itemsView: "full"` — so nothing else has to be asked for. What
/// this does is translate: a stored `ThreadItem` is the *finished* form of what
/// the live path receives as a stream of deltas, so a message replays as one
/// chunk rather than as the dozens it originally arrived in. The window folds
/// chunks into blocks and cannot tell the difference.
///
/// Items with no reading here are skipped rather than guessed at. A web search
/// or an image view is not a thing this window draws, and inventing a tool call
/// for one would put a row in the transcript that never existed.
fn replayed_turns(session_id: &str, result: &Value) -> Vec<Outgoing> {
    let Some(turns) = result.pointer("/thread/turns").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut replay = Vec::new();
    for turn in turns {
        let Some(items) = turn.get("items").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            replay.extend(replayed_item(session_id, item));
        }
    }
    replay
}

fn replayed_item(session_id: &str, item: &Value) -> Vec<Outgoing> {
    match item["type"].as_str() {
        // The person's own words. They reach the window from the agent only on
        // a replay: a message somebody types is recorded by Sync as it is sent,
        // and the agent's echo of it during a live turn is the same sentence
        // twice.
        Some("userMessage") => {
            let said = text_blocks(item.get("content"));
            if said.is_empty() {
                return Vec::new();
            }
            update(
                session_id,
                json!({
                    "sessionUpdate": "user_message_chunk",
                    "content": { "type": "text", "text": said },
                }),
            )
        }
        Some("agentMessage") => {
            let said = item["text"].as_str().unwrap_or_default();
            if said.is_empty() {
                return Vec::new();
            }
            update(
                session_id,
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": said },
                }),
            )
        }
        // The summary rather than the raw content, and it is what the live path
        // shows too: `item/reasoning/summaryTextDelta` is the stream this is the
        // finished form of.
        Some("reasoning") => {
            let thought = joined_strings(item.get("summary"));
            let thought = if thought.is_empty() {
                text_blocks(item.get("content"))
            } else {
                thought
            };
            if thought.is_empty() {
                return Vec::new();
            }
            update(
                session_id,
                json!({
                    "sessionUpdate": "agent_thought_chunk",
                    "content": { "type": "text", "text": thought },
                }),
            )
        }
        // Two updates, because that is what the window saw the first time: the
        // call, then how it ended. `tool_update` is the live path's own
        // translation and is reused rather than restated — a second spelling of
        // "what is a tool call" would drift from the first.
        Some("commandExecution" | "fileChange" | "mcpToolCall") => {
            let mut both = tool_update(session_id, Some(item), false);
            both.extend(tool_update(session_id, Some(item), true));
            both
        }
        _ => Vec::new(),
    }
}

/// The text of a content array — `[{"type": "text", "text": …}]` — joined.
fn text_blocks(content: Option<&Value>) -> String {
    content
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// An array of plain strings, joined as paragraphs. Codex's reasoning summary.
fn joined_strings(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

#[allow(clippy::needless_pass_by_value)]
fn request(id: impl Into<Value>, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.into(), "method": method, "params": params })
}

#[allow(clippy::needless_pass_by_value)]
fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

#[allow(clippy::needless_pass_by_value)]
fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

#[allow(clippy::needless_pass_by_value)]
fn error_response(id: Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message.into() });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

#[allow(clippy::needless_pass_by_value)]
fn update(session_id: &str, body: Value) -> Vec<Outgoing> {
    vec![Outgoing::Acp(notification(
        "session/update",
        json!({ "sessionId": session_id, "update": body }),
    ))]
}

fn codex_input(prompt: Option<&Value>) -> Vec<Value> {
    prompt
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| match block["type"].as_str() {
            Some("text") => Some(json!({ "type": "text", "text": block["text"] })),
            Some("image") => block["data"].as_str().map(|data| {
                let mime = block["mimeType"].as_str().unwrap_or("image/png");
                json!({ "type": "image", "url": format!("data:{mime};base64,{data}") })
            }),
            _ => None,
        })
        .collect()
}

fn codex_mcp_servers(servers: Option<&Value>) -> serde_json::Map<String, Value> {
    let mut translated = serde_json::Map::new();
    for server in servers.and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = server["name"].as_str() else {
            continue;
        };
        let Some(command) = server["command"].as_str() else {
            continue;
        };
        let env = server["env"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((entry["name"].as_str()?.to_owned(), entry["value"].clone())))
            .collect::<serde_json::Map<_, _>>();
        translated.insert(
            name.replace('-', "_"),
            json!({
                "command": command,
                "args": server["args"].as_array().cloned().unwrap_or_default(),
                "env": env,
            }),
        );
    }
    translated
}

fn tool_update(session_id: &str, item: Option<&Value>, completed: bool) -> Vec<Outgoing> {
    let Some(item) = item else { return Vec::new() };
    let Some(item_id) = item["id"].as_str() else {
        return Vec::new();
    };
    if !matches!(
        item["type"].as_str(),
        Some("commandExecution" | "fileChange" | "mcpToolCall")
    ) {
        return Vec::new();
    }
    if completed {
        let status = match item["status"].as_str() {
            Some("failed" | "declined") => "failed",
            _ => "completed",
        };
        return update(
            session_id,
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": item_id, "status": status }),
        );
    }
    let (title, kind, raw_input) = match item["type"].as_str() {
        Some("commandExecution") => (
            item["command"].as_str().unwrap_or("command").to_owned(),
            "execute",
            item["command"].clone(),
        ),
        Some("fileChange") => (
            "Apply file changes".to_owned(),
            "edit",
            item["changes"].clone(),
        ),
        Some("mcpToolCall") => (
            format!(
                "{}/{}",
                item["server"].as_str().unwrap_or("mcp"),
                item["tool"].as_str().unwrap_or("tool")
            ),
            "other",
            item["arguments"].clone(),
        ),
        _ => return Vec::new(),
    };
    update(
        session_id,
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": item_id,
            "title": title,
            "kind": kind,
            "status": "pending",
            "rawInput": raw_input,
        }),
    )
}

fn plan_status(status: &str) -> &'static str {
    match status {
        "inProgress" => "in_progress",
        "completed" => "completed",
        _ => "pending",
    }
}

fn approval_kind(method: &str) -> Option<ApprovalKind> {
    match method {
        "item/commandExecution/requestApproval" => Some(ApprovalKind::Command),
        "item/fileChange/requestApproval" => Some(ApprovalKind::FileChange),
        "item/permissions/requestApproval" => Some(ApprovalKind::Permissions),
        "applyPatchApproval" => Some(ApprovalKind::LegacyPatch),
        "execCommandApproval" => Some(ApprovalKind::LegacyCommand),
        _ => None,
    }
}

fn approval_title(kind: ApprovalKind, params: &Value) -> String {
    params["command"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            params["command"].as_array().map(|parts| {
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .or_else(|| params["reason"].as_str().map(str::to_owned))
        .unwrap_or_else(|| match kind {
            ApprovalKind::FileChange | ApprovalKind::LegacyPatch => "Apply file changes".to_owned(),
            ApprovalKind::Permissions => "Grant additional permissions".to_owned(),
            ApprovalKind::Command | ApprovalKind::LegacyCommand => "Run command".to_owned(),
            ApprovalKind::McpTool => "Run MCP tool".to_owned(),
        })
}

fn approval_locations(kind: ApprovalKind, params: &Value) -> Vec<Value> {
    match kind {
        ApprovalKind::FileChange => params["grantRoot"]
            .as_str()
            .into_iter()
            .map(|path| json!({ "path": path }))
            .collect(),
        ApprovalKind::LegacyPatch => params["fileChanges"]
            .as_object()
            .into_iter()
            .flat_map(|files| files.keys())
            .map(|path| json!({ "path": path }))
            .collect(),
        ApprovalKind::McpTool => Vec::new(),
        _ => params["cwd"]
            .as_str()
            .into_iter()
            .map(|path| json!({ "path": path }))
            .collect(),
    }
}

fn approval_result(kind: ApprovalKind, option: &str, requested: Option<&Value>) -> Value {
    let allow = matches!(option, "allow-once" | "allow-session");
    let persistent = option == "allow-session";
    match kind {
        ApprovalKind::Command | ApprovalKind::FileChange => json!({
            "decision": if allow {
                if persistent { "acceptForSession" } else { "accept" }
            } else if option == "reject-session" { "cancel" } else { "decline" }
        }),
        ApprovalKind::LegacyPatch | ApprovalKind::LegacyCommand => {
            let decision = if allow {
                json!(if persistent {
                    "approved_for_session"
                } else {
                    "approved"
                })
            } else if option == "reject-session" {
                json!("abort")
            } else {
                json!({ "denied": { "rejection": "Denied by user" } })
            };
            json!({ "decision": decision })
        }
        ApprovalKind::Permissions => json!({
            "permissions": if allow { requested.cloned().unwrap_or_else(|| json!({})) } else { json!({}) },
            "scope": if persistent { "session" } else { "turn" },
        }),
        ApprovalKind::McpTool => match option {
            "allow-once" => {
                json!({ "action": "accept", "content": null, "_meta": null })
            }
            "allow-session" => json!({
                "action": "accept",
                "content": null,
                "_meta": { "persist": "session" }
            }),
            "allow-always" => json!({
                "action": "accept",
                "content": null,
                "_meta": { "persist": "always" }
            }),
            _ => json!({ "action": "cancel", "content": null, "_meta": null }),
        },
    }
}

fn is_mcp_tool_approval(params: &Value) -> bool {
    if params["mode"].as_str() != Some("form")
        || params
            .pointer("/_meta/codex_approval_kind")
            .and_then(Value::as_str)
            != Some("mcp_tool_call")
    {
        return false;
    }
    params["requestedSchema"].is_null()
        || params["requestedSchema"].as_object().is_some_and(|schema| {
            schema.get("type").and_then(Value::as_str) == Some("object")
                && schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(serde_json::Map::is_empty)
        })
}

fn mcp_approval_supports(params: &Value, expected: &str) -> bool {
    match params.pointer("/_meta/persist") {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

fn supported_codex(user_agent: &str) -> bool {
    version_from_user_agent(user_agent).is_some_and(|version| version >= MIN_CODEX_VERSION)
}

fn version_from_user_agent(user_agent: &str) -> Option<(u64, u64, u64)> {
    user_agent
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|part| {
            let mut numbers = part.split('.').map(str::parse::<u64>);
            Some((
                numbers.next()?.ok()?,
                numbers.next()?.ok()?,
                numbers.next()?.ok()?,
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_reads_the_app_server_user_agent() {
        assert!(supported_codex("codex_cli_rs/0.147.0"));
        assert!(supported_codex("codex-cli/1.2.3 (macos)"));
        assert!(!supported_codex("codex_cli_rs/0.146.9"));
        assert!(!supported_codex("unknown"));
    }

    #[test]
    fn acp_mcp_environment_becomes_codex_config() {
        let servers = codex_mcp_servers(Some(&json!([{
            "name": "git-sync",
            "command": "/Applications/Sync.app/Contents/MacOS/git-sync",
            "args": ["mcp", "--root", "/work/repo"],
            "env": [{ "name": "SYNC_AGENT_SLUG", "value": "rust-impl" }]
        }])));
        assert_eq!(
            servers["git_sync"]["command"],
            json!("/Applications/Sync.app/Contents/MacOS/git-sync")
        );
        assert_eq!(
            servers["git_sync"]["env"]["SYNC_AGENT_SLUG"],
            json!("rust-impl")
        );
    }

    #[test]
    fn all_five_codex_approval_methods_are_normalized() {
        for (method, result_pointer, expected) in [
            (
                "item/commandExecution/requestApproval",
                "/result/decision",
                json!("acceptForSession"),
            ),
            (
                "item/fileChange/requestApproval",
                "/result/decision",
                json!("acceptForSession"),
            ),
            (
                "item/permissions/requestApproval",
                "/result/scope",
                json!("session"),
            ),
            (
                "applyPatchApproval",
                "/result/decision",
                json!("approved_for_session"),
            ),
            (
                "execCommandApproval",
                "/result/decision",
                json!("approved_for_session"),
            ),
        ] {
            let mut state = State::default();
            let request = json!({
                "jsonrpc": "2.0",
                "id": 77,
                "method": method,
                "params": {
                    "threadId": "thread-1",
                    "itemId": "item-1",
                    "permissions": { "network": { "enabled": true } }
                }
            });
            let permission = state.codex_request(&request);
            let Outgoing::Acp(permission) = &permission[0] else {
                panic!("{method} must become an ACP request");
            };
            assert_eq!(permission["method"], json!("session/request_permission"));

            let answer = state.acp_response(&json!({
                "jsonrpc": "2.0",
                "id": permission["id"],
                "result": { "outcome": { "outcome": "selected", "optionId": "allow-session" } }
            }));
            let Outgoing::Codex(answer) = &answer[0] else {
                panic!("{method} must receive a Codex response");
            };
            assert_eq!(answer["id"], json!(77));
            assert_eq!(answer.pointer(result_pointer), Some(&expected), "{method}");
        }
    }

    #[test]
    fn mcp_tool_approval_elicitation_is_normalized() {
        let mut state = State::default();
        let permission = state.codex_request(&json!({
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
                    "persist": ["session", "always"]
                }
            }
        }));
        let Outgoing::Acp(permission) = &permission[0] else {
            panic!("MCP tool approval must become an ACP permission request");
        };
        assert_eq!(permission["method"], json!("session/request_permission"));
        assert_eq!(
            permission["params"]["toolCall"]["title"],
            json!("sync/sync_projects")
        );
        assert_eq!(permission["params"]["toolCall"]["rawInput"], json!({}));
        assert_eq!(
            permission["params"]["toolCall"]["_meta"]["message"],
            json!("Allow Sync to list this project's memory?")
        );
        assert_eq!(
            permission["params"]["options"][1]["optionId"],
            json!("allow-session")
        );
        assert_eq!(
            permission["params"]["options"][2]["optionId"],
            json!("allow-always")
        );

        let answer = state.acp_response(&json!({
            "jsonrpc": "2.0",
            "id": permission["id"],
            "result": { "outcome": { "outcome": "selected", "optionId": "allow-session" } }
        }));
        let Outgoing::Codex(answer) = &answer[0] else {
            panic!("MCP tool approval must receive a Codex response");
        };
        assert_eq!(answer["id"], json!(78));
        assert_eq!(answer["result"]["action"], json!("accept"));
        assert_eq!(answer["result"]["content"], Value::Null);
        assert_eq!(answer["result"]["_meta"]["persist"], json!("session"));
    }

    #[test]
    fn unsupported_mcp_elicitation_is_declined() {
        let mut state = State::default();
        let answer = state.codex_request(&json!({
            "jsonrpc": "2.0",
            "id": 79,
            "method": "mcpServer/elicitation/request",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "serverName": "sync",
                "mode": "form",
                "message": "Enter a value",
                "requestedSchema": {
                    "type": "object",
                    "properties": { "value": { "type": "string" } }
                },
                "_meta": null
            }
        }));
        let Outgoing::Codex(answer) = &answer[0] else {
            panic!("unsupported elicitations must receive a Codex response");
        };
        assert_eq!(answer["result"]["action"], json!("decline"));
        assert_eq!(answer["result"]["content"], Value::Null);
    }

    #[test]
    fn completed_message_and_reasoning_items_do_not_become_tool_updates() {
        for item_type in ["agentMessage", "reasoning"] {
            let item = json!({ "id": "not-a-tool", "type": item_type, "status": "completed" });
            assert!(tool_update("thread-1", Some(&item), true).is_empty());
        }
    }

    #[test]
    fn plan_and_cancel_keep_the_acp_session_identity() {
        let mut state = State::default();
        let plan = state.codex_notification(&json!({
            "method": "turn/plan/updated",
            "params": {
                "threadId": "thread-1",
                "plan": [{ "step": "Run tests", "status": "inProgress" }]
            }
        }));
        let Outgoing::Acp(plan) = &plan[0] else {
            panic!("plan must reach ACP");
        };
        assert_eq!(plan["params"]["sessionId"], json!("thread-1"));
        assert_eq!(
            plan["params"]["update"]["entries"][0]["status"],
            json!("in_progress")
        );

        state
            .active_turn_by_session
            .insert("thread-1".to_owned(), "turn-1".to_owned());
        let cancel = state.handle_acp(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": "thread-1" }
        }));
        let Outgoing::Codex(cancel) = &cancel[0] else {
            panic!("cancel must reach Codex");
        };
        assert_eq!(cancel["method"], json!("turn/interrupt"));
        assert_eq!(cancel["params"]["turnId"], json!("turn-1"));
    }
}
