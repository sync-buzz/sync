//! Project-scoped MCP servers, called and listed through Sync rather than
//! connected to directly.
//!
//! An MCP server a project declares is treated the way an extension is: its
//! tools are discovered and listed in the project's record, and a call reaches
//! it through Sync's own dispatch — the same door an extension's tool is
//! called through — rather than by handing the agent a descriptor to connect
//! to on its own. Sync is the proxy: it holds the credentials, it routes the
//! call, and the agent never knows whether a tool answered from a package's
//! JavaScript or an MCP server's process.
//!
//! **Per-call, not pooled.** Every call connects, handshakes, asks one
//! question, and shuts down. See [`sync_mcp_client`] for the ceiling and the
//! upgrade path.
//!
//! **The crate knows no vault.** Secrets are resolved here, from the keychain,
//! because resolving them is the proxy's job and the proxy lives in the
//! application, not in the leaf crate that speaks the protocol.

use std::collections::HashMap;

use serde_json::Value;
use sync_memory::{McpTransportConfig, MemoryClient, Operations, ToolDeclaration};
use sync_vault::{Slot, Vault};
use tauri::{AppHandle, Manager, Runtime};

use crate::memory::MemorySessions;

/// The tools a project-scoped MCP server offers, for the window to write into
/// the project record beside an extension's own.
///
/// Answers an empty list when `id` is not a project-scoped MCP server — the
/// ordinary case of an extension being asked about — and also when the server
/// could not be reached: discovery is best-effort, and a server whose tools
/// could not be listed is still added, with its tools filled in on a later
/// refresh. The sentence a failure carries is the caller's to read or ignore.
#[tauri::command]
pub async fn mcp_list_tools<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    id: String,
) -> Result<Vec<ToolDeclaration>, String> {
    match list_tools(&app, &project, &id).await {
        Ok(Some(tools)) => Ok(tools),
        Ok(None) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Call one tool on a project-scoped MCP server.
///
/// `Ok(Some(value))` — the server answered, and the value is its result.
/// `Ok(None)` — `server` does not name a project-scoped MCP server, so the
/// caller falls back to the path an extension's tool takes (a JavaScript
/// handler, or the flagship). `Err` — it is an MCP server, and the call failed;
/// the sentence is for an agent or a person to read.
///
/// `asking` is the extension on whose behalf the call is made, or `None` for the
/// agent or the window itself. Consent is checked here, once, for the MCP path:
/// a project-scoped server is gated by the same agreement a flagship server is,
/// and the agent — which is the person, not a package — needs none.
pub async fn call_tool<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    server: &str,
    tool: &str,
    arguments: &Value,
    asking: Option<&str>,
) -> Result<Option<Value>, String> {
    let Some(transport) = declaration(app, project, server).await else {
        return Ok(None);
    };
    // The same agreement the flagship path reads: a package may call this
    // server only if a person said so on its card. The agent and the window are
    // the person, so `None` needs no agreement.
    if let Some(id) = asking
        && !crate::consent::allows(app, id, server)
    {
        return Err(format!(
            "Nobody has agreed that \"{id}\" may call `{server}`."
        ));
    }
    let secrets = resolve_secrets(server, &transport).await?;
    let arguments = arguments.as_object();
    let result = sync_mcp_client::call(&transport, &secrets, tool, arguments)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(result)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Call one tool on a project-scoped MCP server through a held connection.
///
/// The pooled counterpart to [`call_tool`]: the same declaration, consent and
/// secret resolution, answered by [`sync_mcp_client::Pool::call`] rather than
/// the per-call [`sync_mcp_client::call`]. A server whose process holds state
/// across calls — a browser a `navigate` opened — stays alive for the run the
/// pool covers, and the caller drains the pool when the run is over.
///
/// `Ok(None)` carries the same meaning: `server` does not name a project-scoped
/// MCP server, and the caller falls back to the extension path.
pub async fn call_tool_pooled<R: Runtime>(
    app: &AppHandle<R>,
    pool: &mut sync_mcp_client::Pool,
    project: &str,
    server: &str,
    tool: &str,
    arguments: &Value,
    asking: Option<&str>,
) -> Result<Option<Value>, String> {
    let Some(transport) = declaration(app, project, server).await else {
        return Ok(None);
    };
    if let Some(id) = asking
        && !crate::consent::allows(app, id, server)
    {
        return Err(format!(
            "Nobody has agreed that \"{id}\" may call `{server}`."
        ));
    }
    let secrets = resolve_secrets(server, &transport).await?;
    let result = pool
        .call(&transport, &secrets, server, tool, arguments.as_object())
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(result)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// List the tools a project-scoped MCP server offers, as the project record
/// carries them.
///
/// `Ok(Some(tools))` — the server answered, and the tools are spelled as
/// [`ToolDeclaration`]s, the same shape an extension's manifest declares.
/// `Ok(None)` — not a project-scoped MCP server. `Err` — it is one, and the
/// listing failed.
pub async fn list_tools<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    server: &str,
) -> Result<Option<Vec<ToolDeclaration>>, String> {
    let Some(transport) = declaration(app, project, server).await else {
        return Ok(None);
    };
    let secrets = resolve_secrets(server, &transport).await?;
    let tools = sync_mcp_client::list_tools(&transport, &secrets)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(
        tools
            .into_iter()
            .map(|tool| ToolDeclaration {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                input: Value::Object((*tool.input_schema).clone()),
            })
            .collect(),
    ))
}

/// The transport a project declares for `server`, when it declares one.
///
/// `None` when the project record carries no installed entry with that id and a
/// transport — which is the ordinary case of an extension being asked about,
/// and the signal to take the extension path.
///
/// A memory that cannot be read answers `None` too, and there is no error to
/// return because none would be the right one. This question is asked before
/// anybody knows whether `server` is an MCP server at all, so a failure here
/// would be spoken in place of the refusal the caller was about to give — a
/// package asking for a tool it never declared would be told the memory failed,
/// and whoever reads that sentence would go looking at the engine.
async fn declaration<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    server: &str,
) -> Option<McpTransportConfig> {
    // `try_state` rather than `state`, which panics. This runs before anybody
    // knows whether `server` is an MCP server, so an application without the
    // memory sessions managed — a test raising the command layer alone — would
    // take the panic in place of the refusal it was asking for.
    let sessions = app.try_state::<MemorySessions>()?;
    sessions
        .with_session(app, project, MemoryClient::project_settings)
        .await
        .ok()
        .flatten()
        .and_then(|settings| {
            settings
                .installed
                .into_iter()
                .find(|entry| entry.id == server)
        })
        .and_then(|entry| entry.transport.clone())
}

/// Resolve the secrets a transport names from the vault, keyed by the `secret`
/// name the transport carries.
///
/// A secret the vault does not hold is left out rather than failing the call:
/// the MCP client names it back as `MissingSecret`, which is a clearer sentence
/// than one from the keychain. A vault that refuses for any other reason is
/// treated the same way here — the ceiling is a store that distinguishes
/// *absent* from *refused*, and for now the call fails at the server with the
/// right name.
async fn resolve_secrets(
    id: &str,
    transport: &McpTransportConfig,
) -> Result<sync_mcp_client::ResolvedSecrets, String> {
    let owner = format!("mcp-{id}");
    let mut names: Vec<String> = match transport {
        McpTransportConfig::Stdio { env, .. } => env.iter().map(|e| e.secret.clone()).collect(),
        McpTransportConfig::Http { headers, .. } | McpTransportConfig::Sse { headers, .. } => {
            headers.iter().map(|h| h.secret.clone()).collect()
        }
    };
    names.sort_unstable();
    names.dedup();

    tauri::async_runtime::spawn_blocking(move || {
        let vault = Vault::system().map_err(|error| error.to_string())?;
        let mut secrets: HashMap<String, String> = HashMap::new();
        for name in &names {
            let slot = Slot::new(&owner, name).map_err(|error| error.to_string())?;
            if let Ok(value) = vault.read(&slot) {
                secrets.insert(name.clone(), value);
            }
        }
        Ok::<_, String>(sync_mcp_client::ResolvedSecrets::from(secrets))
    })
    .await
    .map_err(|error| error.to_string())?
}
