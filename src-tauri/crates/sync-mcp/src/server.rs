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

use crate::application::Application;
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
    /// Sync, when it is on the other end.
    ///
    /// What runs an extension's tool: this process decides whether a call may
    /// be made and the application makes it, because everything a tool's body
    /// reaches — the keychain, the manifest's host list, the artefact — lives
    /// there. See [`crate::application`].
    ///
    /// Empty for a `sync-mcp` somebody started themselves, and then every tool
    /// call says so in words rather than hanging.
    application: Arc<Application>,
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
2. **Sync records**, gated by freshness. `fresh` means somebody read the code and found the \
   claim standing. `unverified` means nobody has said either way — usable, and worth \
   checking. `stale` and `invalid` mean the code moved under the claim: a flag, never a \
   fact, and not to be quoted until you have checked it.
3. **Loose files** — `README`, stray notes, comments. Lift what is authoritative into Sync.
4. **Your own memory** — a cache, not a source. Search Sync first; recall only what Sync \
   lacks. On conflict Sync wins, and never overwrite Sync from memory.

**Checking is a write.** Having read the code a record covers and found it true, write the \
record again with `verified: true`; it becomes `fresh` until the code under its \
`scope_paths` moves again. That is the only way anything here becomes trusted, so a session \
that read the code and said nothing leaves the next one doing the same reading. Where the \
claim no longer holds, the same write fixes it — a record that disagrees with the code is a \
record to correct, not evidence.

## Being asked for work is being asked for a record

Somebody who says *set a task*, *file that*, *note it down* or *plan this* is asking for a \
record in this project's memory, written with `sync_apply`. `sync_project` names the kinds \
this project holds, and one of them is the word they used. A list of your own, a sub-agent \
you start, or a line in your reply is none of it: those end with the conversation, and the \
request was for something that outlives it.

Work you are doing right now is the exception. That is done, not filed.

## The loop

1. **Orient** — `sync_project` on the project you are in.
2. **Research before acting** — `memory_search` the topic, and read what is already \
   recorded about the files in scope.
3. **Trust-check** every record you rely on, by the order above, and write back what the \
   check found — `verified: true` where it held, a correction where it did not.
4. **Record as you go** — write what became true with `sync_apply`, the moment it is true, \
   and correct what your change falsified. Say in one line what you wrote.

Be proactive: run this unprompted. What is not written down dies with the conversation.

## The branch is the person's, not yours

You are standing in somebody's working tree, and they may be working in it while you are. \
So the branch that is checked out is the branch you work in. Do not switch branches, make \
one, stash, or commit unless you were asked to: somebody who comes back to a branch they \
did not choose has lost whatever they were in the middle of, and no amount of correct work \
makes up for that.

A tree of its own is something work is *given*, never something an agent takes. Where you \
have been put in one, stay in it. Where you think the work needs one, say so and let the \
person decide — they may already be using the tree you would have made, or want it \
somewhere else, or want the work here after all.

The same rule is what keeps two agents out of each other's way, and it is the reason it is \
stated rather than left to sense: two of you in one tree are two of you editing the same \
files, and neither can see the other's half-finished edit.

## Name a record, never its key

A key is an address, not a name. `d-3ad25f` tells a reader nothing about what it points \
at, and nobody reading a sentence can open it. Every answer that hands you a key hands \
you the title and the kind beside it, so write the name and let the link carry the \
address:

    [the record's title](sync://<kind>/<key>)

This holds for what you say to a person exactly as much as for what you store — a \
message is Markdown too, and a bare key in one is a dead end for whoever reads it. \
Double brackets are not a link: they carry no kind, so nothing can route on one, and \
`sync_apply` refuses a write that spells a record with them. A key left bare in a code \
span comes back in the answer instead, with the link to write in its place.

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
        Self::over(Arc::new(projects), Arc::new(Application::new()))
    }

    /// Serve projects this process already holds.
    ///
    /// The door that shares. Where the host channel is open too, both doors are
    /// built over one [`Projects`], so a repository an agent asks about and one
    /// the window has open are the same memory rather than two.
    pub fn over(projects: Arc<Projects>, application: Arc<Application>) -> Self {
        Self {
            projects,
            application,
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
        let named = key.clone();

        // An extension's tool is the one call that leaves this process, and
        // the two halves are deliberately not in one `with_domain`. Reading the
        // project's record needs the engine; running the tool needs the
        // application and may sit there for twenty seconds waiting on somebody
        // else's API. Holding this project's memory across that would stop
        // every other call about the same repository — the window's included —
        // for the length of a stranger's request.
        if name == own::CALL {
            let asked = key.clone();
            let intent = project
                .with_domain(move |domain| own::intended(domain, &asked, &arguments))
                .await?;
            let call = match intent {
                Ok(intent) => {
                    let answer = self
                        .application
                        .call(
                            sync_memory::TOOL_CALL,
                            json!({
                                "project": project.path(),
                                "extension": intent.extension,
                                "tool": intent.tool,
                                "arguments": intent.arguments,
                            }),
                        )
                        .await;
                    crate::engine::as_tool_call(answer)
                }
                Err(refused) => crate::engine::as_tool_call(Err(refused)),
            };
            return Ok(name_the_project(call, &named));
        }

        let ours = own::is_ours(&name);
        let call = project
            .with_domain(move |domain| {
                if ours {
                    own::call(domain, &name, &arguments)
                } else {
                    domain.engine_tool(&name, &arguments)
                }
            })
            .await?;
        // Only the engine's answers. What this server's own tools say is
        // already what it decided to say — `sync_instructions` hands over a
        // package's prose because that is the whole of what it was called for,
        // and trimming its answer would leave the prose with nowhere to arrive.
        let call = if ours {
            call
        } else {
            without_package_prose(call)
        };
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
        if !published::is_published(&name) && !own::is_ours(&name) {
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

/// Take the packages' own prose back out of an answer that was not asked for it.
///
/// A project's record carries what each installed package tells an agent: the
/// prompt it published and the tools it declares, schemas and all. That is
/// where it belongs — the record travels with the repository, so a colleague
/// who cloned it is told the same thing — and it is served deliberately, one
/// package at a time, as `sync_instructions` with that package's topic.
///
/// What it must not be is *incidental*. A listing of thirty records that
/// happens to include the project's own arrives carrying every package's
/// instructions, several times the size of everything else in the answer, to an
/// agent that asked what kinds this project holds. The id and the version stay,
/// because they say a package is there; the prose goes, because there is a call
/// whose whole purpose is to hand it over.
///
/// Every answer is walked rather than the two that list records, and the reason
/// is that this is about a shape and not about a tool: whatever else comes to
/// carry a project's record — a search hit, a diff, something the engine grows
/// next year — carries it with the same member on it.
fn without_package_prose(mut call: ToolCall) -> ToolCall {
    if let Ok(content) = &mut call.result {
        strip_prose(content);
    }
    call
}

/// Every `installed` list in a value, trimmed to what a package *is*.
fn strip_prose(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Array(installed)) = object.get_mut("installed") {
                for package in installed.iter_mut() {
                    let Some(package) = package.as_object_mut() else {
                        continue;
                    };
                    package.remove("prompt");
                    // The names, so that what there is to call is still
                    // visible, and none of the descriptions or schemas: those
                    // are what the topic is for, and an agent calling a tool
                    // has read it.
                    if let Some(Value::Array(tools)) = package.get_mut("tools") {
                        for tool in tools.iter_mut() {
                            if let Some(name) = tool.get("name").cloned() {
                                *tool = name;
                            }
                        }
                    }
                }
            }
            for member in object.values_mut() {
                strip_prose(member);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_prose(item);
            }
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The one block every session reads without asking for it.
    ///
    /// It is where the two mistakes this check exists for were both made, and
    /// it is the worst place to make them: a topic is read by the agent that
    /// went looking, and this is read by all of them. See the same pair of
    /// tests in `own.rs` for what each one holds.
    #[test]
    fn the_instructions_name_tools_that_exist_and_states_that_do() {
        let unknown = own::names_that_are_not_tools(INSTRUCTIONS);
        assert!(
            unknown.is_empty(),
            "the instructions name tools this server does not publish: {unknown:?}"
        );
        for spelled in INSTRUCTIONS.split('`').skip(1).step_by(2) {
            assert_ne!(
                spelled.trim(),
                "valid",
                "freshness is `fresh` everywhere the engine and the window spell it"
            );
        }
    }
}
