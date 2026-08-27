//! The server itself: one MCP interface over every project this machine keeps.
//!
//! One process, several projects, one model. What makes that possible is that
//! nothing here is per-connection: a connection carries no project, so two
//! agents talking to the same server are not two servers, and the vector model
//! is resolved once and shared by every project rather than loaded per session.
//!
//! Which project a call is about is therefore the call's own business — see
//! [`crate::projects`] for why that is an argument and never a default.

use std::borrow::Cow;
use std::future::{self, Future};
use std::sync::Arc;

use memory_hub_mcp::ToolCall;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer};
use serde_json::{Map, Value, json};

use crate::contributed::Registry;
use crate::projects::{PROJECT_ARGUMENT, Projects};
use crate::{own, published};

/// One server, over every project it was given.
///
/// Cloned per connection by the HTTP transport, which is why both fields are
/// shared handles: a clone is another way in to the same projects, never
/// another copy of them.
#[derive(Clone)]
pub struct SyncMcp {
    projects: Arc<Projects>,
    /// The tools extensions contributed to this server.
    ///
    /// Empty, and stays empty until something contributes: the runtime that
    /// will is a later stage. The door is here so the day it arrives it walks
    /// through one rather than cuts one — see [`crate::contributed`].
    contributed: Arc<Registry>,
}

/// What every session is told before it does anything.
///
/// The one block of prose a client reads without being asked for it, so it
/// carries the contract rather than a tour: which project a call is about,
/// what this memory is *for*, and what outranks what when two answers
/// disagree. Everything narrower — how to write a spec, how freshness works —
/// is `sync_instructions`, which costs a call and is read when the area comes
/// up rather than in every session.
///
/// Hosts truncate long instruction blocks, so the order is the priority: the
/// rule that makes a call legal comes first, the vocabulary second, the
/// habits last.
const INSTRUCTIONS: &str = "\
Sync holds each project's memory: decisions, constraints, observations, questions, \
specs and docs that outlive a conversation.

## Every call names a project

One server serves every project on this machine, so `project` is the first argument of \
every tool and is never omitted. It is the project's own key — `sync_projects` lists them \
— and never a path. There is no default: a call without a key is refused rather than \
answered from somewhere.

Start a session with `sync_projects`, then `sync_project` for the one you are working in: \
it names the kinds that project holds, which is the vocabulary every `kind` argument is \
drawn from. Kinds differ between projects.

## Truth order

Higher wins on conflict; lower fills gaps.

1. **Live code** — the final arbiter for any claim about the code.
2. **Sync records**, gated by freshness: `valid` and `unverified` are usable; `stale` and \
   `invalid` are a flag, never a fact — check them against the code before acting.
3. **Loose files** — `README`, stray notes, comments. Lift what is authoritative into Sync.
4. **Your own memory** — a cache, not a source. Search Sync first; recall only what Sync \
   lacks. On conflict Sync wins, and never overwrite Sync from memory.

## The loop

1. **Orient** — `sync_project` on the project you are in.
2. **Research before acting** — `memory_search` the topic, and read what is already \
   recorded about the files in scope.
3. **Trust-check** every record you rely on, by the order above.
4. **Record as you go** — write what became true with `sync_apply`, the moment it is true, \
   and correct what your change falsified. Say in one line what you wrote.

Be proactive: run this unprompted. What is not written down dies with the conversation.

**Never write a secret.** A project's memory travels with its repository. Name where a \
secret lives, never its value.";

impl SyncMcp {
    /// Whether an agent connected here may make the machine talk.
    ///
    /// The person's own answer, read from the file the window writes and never
    /// cached: an agent that connected while it was allowed must stop being
    /// able to the moment somebody switches it off, and a value held in this
    /// process would go on saying yes until the sidecar was restarted.
    fn may_speak(&self) -> bool {
        self.projects
            .configuration()
            .is_some_and(|directory| sync_voice::preference(directory).agents)
    }

    /// Serve `projects`.
    pub fn new(projects: Projects) -> Self {
        Self::over(Arc::new(projects))
    }

    /// Serve projects this process already holds.
    ///
    /// The door that shares. Where the host channel is open too, both doors are
    /// built over one [`Projects`], so a repository an agent asks about and one
    /// the window has open are the same memory rather than two.
    pub fn over(projects: Arc<Projects>) -> Self {
        Self {
            projects,
            contributed: Arc::new(Registry::new()),
        }
    }

    /// Everything this server publishes, as one list.
    ///
    /// Sync, and reached from the trait the way [`Self::run`] is, because
    /// nothing in a catalogue waits: the engine's tools are read off its
    /// published set, ours are a constant, and a contribution is already in
    /// memory. `list_tools` is the async door onto it and holds no logic of
    /// its own.
    ///
    /// # Errors
    ///
    /// The engine's own catalogue is the one part of this that can fail to be
    /// read, and it fails the whole list rather than half of it: a client that
    /// cannot trust the shape of the list takes none of it.
    fn catalogue(&self) -> Result<ListToolsResult, McpError> {
        let mut tools = published::tools().map_err(|error| {
            McpError::internal_error(
                format!("the engine's tool catalogue is unreadable: {error}"),
                None,
            )
        })?;
        // Read per catalogue rather than held: somebody moving the switch on
        // Sync's Voice page changes what this server publishes, without the
        // sidecar being restarted.
        tools.extend(own::tools(self.may_speak()));
        tools.extend(self.contributed.tools());
        for tool in &mut tools {
            if tool.name != own::PROJECTS && tool.name != own::SPEAK {
                require_project(tool);
            }
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    /// Run one tool, whichever half of the surface owns it.
    ///
    /// The project is taken off the arguments before anything else sees them:
    /// below this line a call looks exactly as it did when a process was one
    /// project, and the engine is never handed an argument it has no field for.
    async fn run(&self, name: String, mut arguments: Value) -> Result<ToolCall, McpError> {
        if name == own::PROJECTS {
            let listed = self.projects.listed();
            return Ok(crate::engine::as_tool_call(Ok(listed)));
        }
        // Before the project is taken off the arguments, because speakers
        // belong to a machine rather than to a repository: `sync_speak` is the
        // second tool here with no `project`, and the only one that touches no
        // memory at all.
        if name == own::SPEAK {
            return Ok(crate::engine::as_tool_call(own::speak(
                self.projects.configuration(),
                &arguments,
            )));
        }
        let key = match take_project(&mut arguments) {
            Ok(key) => key,
            Err(refusal) => return Ok(crate::engine::as_tool_call(Err(self.explain(&refusal)))),
        };
        let Some(project) = self.projects.holding(&key) else {
            return Ok(crate::engine::as_tool_call(Err(self.explain(&format!(
                "no project answers to `{key}` on this machine"
            )))));
        };
        let contributed = Arc::clone(&self.contributed);
        let named = key.clone();
        let call = project
            .with_domain(move |domain| {
                if own::is_ours(&name) {
                    own::call(domain, &contributed, &name, &arguments)
                } else if contributed.holds(&name) {
                    crate::engine::as_tool_call(contributed.call(domain, &name, &arguments))
                } else {
                    domain.engine_tool(&name, &arguments)
                }
            })
            .await?;
        Ok(name_the_project(call, &named))
    }

    /// A refusal that says how to call correctly, with the keys to call with.
    ///
    /// Told rather than implied. An agent that got `invalid_params` would have
    /// to guess whether it named the wrong project, called a tool that has
    /// none, or mistyped — and it would guess by trying again, which is the
    /// expensive way to read documentation.
    fn explain(&self, what_went_wrong: &str) -> sync_memory::MemoryError {
        let known = self.projects.keys();
        let named = if known.is_empty() {
            "This machine answers for no projects yet — open one in Sync first.".to_owned()
        } else {
            format!("Projects on this machine: {}.", known.join(", "))
        };
        sync_memory::MemoryError::domain(
            "unknown_project",
            format!(
                "{what_went_wrong}. Every tool takes `{PROJECT_ARGUMENT}` as its first argument, \
                 and it is the project's own key rather than a path. {named} `{}` lists them.",
                own::PROJECTS
            ),
            json!({"known": known}),
        )
    }
}

impl ServerHandler for SyncMcp {
    /// The revisions this server can actually answer for.
    ///
    /// `rmcp` defaults this to every revision it has a name for, which is not
    /// the same list as the ones it can serve: `2026-07-28` is in the default
    /// and requires every `tools/list` to carry a cache hint the SDK has no
    /// field for. A client offering it was told yes, then read an answer with
    /// the hint missing — and a client that cannot trust the shape of the list
    /// takes none of it, so the whole server arrived with zero tools and no
    /// error anywhere in the handshake.
    ///
    /// Narrowed to what can be served, an unknown revision negotiates down to
    /// [`ServerInfo`]'s own version instead of being promised.
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
        ])
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sync", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send {
        future::ready(self.catalogue())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        // Refused here rather than in the engine: a tool this server does not
        // publish is not a tool, whatever the engine thinks of the name.
        if !published::is_published(&name) && !own::is_ours(&name) && !self.contributed.holds(&name)
        {
            return Err(McpError::invalid_params(
                format!("no tool named `{name}`"),
                None,
            ));
        }
        let arguments = request.arguments.map_or_else(|| json!({}), Value::Object);
        let call = self.run(name, arguments).await?;
        // `Complete`: no tool here pauses for the client or hands back a task
        // to poll.
        Ok(into_result(call).into())
    }
}

/// Take the project key off a call's arguments.
///
/// # Errors
///
/// A missing key is refused rather than defaulted. The client knows which
/// directory it is working in and could be asked, but an argument that may be
/// left out is one that will be left out — and the answer would then come from
/// whichever project the server picked, which is the one failure nobody checks
/// for because it looks like an answer.
fn take_project(arguments: &mut Value) -> Result<String, String> {
    let Some(object) = arguments.as_object_mut() else {
        return Err("this call carried no arguments".to_owned());
    };
    match object.remove(PROJECT_ARGUMENT) {
        Some(Value::String(key)) if !key.trim().is_empty() => Ok(key.trim().to_owned()),
        Some(Value::String(_)) | None => Err(format!("this call named no `{PROJECT_ARGUMENT}`")),
        Some(other) => Err(format!(
            "`{PROJECT_ARGUMENT}` has to be a project key, and this call sent {other}"
        )),
    }
}

/// State the project argument in a tool's schema, first and required.
///
/// Written into the engine's own schemas rather than described beside them. A
/// model fills in what a schema asks for; a sentence in a description asking
/// for one more field is a sentence it may summarise away.
fn require_project(tool: &mut Tool) {
    let schema = Arc::make_mut(&mut tool.input_schema);
    let described = json!({
        "type": "string",
        "description": "Which project this call is about: its key, as `sync_projects` lists \
                        them. Not a path, and never omitted — there is no default project.",
    });
    // First, because it answers "where do I look" and everything else in the
    // call is a question about that answer. `preserve_order` is what makes the
    // position mean anything; without it this would land alphabetically.
    let properties = schema
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut();
    if let Some(properties) = properties {
        let mut ordered = Map::with_capacity(properties.len() + 1);
        ordered.insert(PROJECT_ARGUMENT.to_owned(), described);
        for (name, value) in std::mem::take(properties) {
            ordered.insert(name, value);
        }
        *properties = ordered;
    }
    let required = schema
        .entry("required")
        .or_insert_with(|| json!([]))
        .as_array_mut();
    if let Some(required) = required
        && !required.iter().any(|name| name == PROJECT_ARGUMENT)
    {
        required.insert(0, json!(PROJECT_ARGUMENT));
    }
}

/// Say which project answered, in the answer itself.
///
/// Cheap insurance against the failure this design trades for: a key names one
/// project and looks exactly like a key naming another, so an answer that did
/// not say whose it was would be indistinguishable from the right one.
fn name_the_project(mut call: ToolCall, key: &str) -> ToolCall {
    if let Ok(content) = &mut call.result
        && let Some(object) = content.as_object_mut()
    {
        object.insert(PROJECT_ARGUMENT.to_owned(), json!(key));
    }
    call
}

/// Turn what the engine did into what MCP says.
///
/// A tool that failed is an answer, not a protocol error: `isError` with the
/// engine's own `kind`, message and data, so a client can tell a stale revision
/// from a locked project without parsing prose.
fn into_result(call: ToolCall) -> CallToolResult {
    let (payload, mut result) = match call.result {
        Ok(content) => {
            let result = CallToolResult::success(vec![ContentBlock::text(content.to_string())]);
            (content, result)
        }
        Err(failure) => {
            let payload = match failure {
                memory_hub_mcp::ToolCallFailure::Rpc(error) => json!({"error": {
                    "kind": error.data.get("kind").cloned().unwrap_or_else(|| json!("invalid_request")),
                    "message": error.message,
                    "data": error.data,
                }}),
                memory_hub_mcp::ToolCallFailure::Tool(error) => json!({"error": {
                    "kind": error.kind,
                    "message": error.message,
                    "data": error.data,
                }}),
            };
            let result = CallToolResult::error(vec![ContentBlock::text(payload.to_string())]);
            (payload, result)
        }
    };
    // Both forms, the way the engine answers its own clients: the text is what
    // a model reads, the structured copy is what a program reads, and neither
    // has to parse the other.
    result.structured_content = Some(payload);
    result
}
