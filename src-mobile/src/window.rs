//! The window's commands, answered from a computer.
//!
//! The document on this phone is the one the Mac shows, and it asks for what it
//! needs by the same names: `memory_records`, `memory_save`, `extension_fetch`.
//! On the Mac those names reach an engine on the same machine. Here each one is
//! a question put over the host channel and answered where there is something
//! to answer it with.
//!
//! **Nothing here spells an operation or its parameters.** That is
//! [`sync_memory::Operations`], written once for both clients, so a member
//! renamed on the channel is a compiler error in this file rather than a call
//! that comes back empty on whichever application was not rebuilt. What this
//! file decides is a different question, and the only one it decides: which of
//! the window's commands a phone may ask at all.
//!
//! # Refusal is what is absent
//!
//! A command that is not here is not answered by a stub explaining itself — it
//! is not registered, and Tauri refuses it by name. That is deliberate. The
//! alternative is a list of every command the Mac has, kept in step by hand, and
//! a command added there and forgotten here would then be *answered* — with a
//! sentence saying it cannot be done, which is a phone claiming to know
//! something about a command nobody taught it.
//!
//! What is absent, and why:
//!
//! - **The file system.** Choosing a folder, probing one, registering a
//!   project, the recent list. A phone names a project by a key from the
//!   computer's registry and has no directory to offer.
//! - **This machine's own arrangements.** Windows, the menu bar, the server's
//!   port, the keychain, worktrees, voices. Every one of them is about the
//!   machine the window is running on, and that machine is this phone.
//! - **Agents.** A session is a conversation held open by the application that
//!   started it, and carrying one across a network is its own piece of work.

use serde::Serialize;
use serde_json::Value;
use sync_memory::{
    CommandError, ContentView, Dependents, Document, DocumentEdits, EntityInput, FetchOutcome,
    FolderEntry, MemoryPresence, Operations as _, RecordType, RecordsPage, ScanOutcome, SyncState,
    TransportStatus,
};
use tauri::State;

use crate::channel::{Asking, Asks, Channel};

/// What a command here answers with when it does not answer with nothing.
type Answered<T> = Result<T, CommandError>;

/// Every command this phone answers, in the one list Tauri is given.
///
/// A macro because `generate_handler!` needs the names where it is written and
/// they are implemented here, and a set kept in two places is a command that
/// exists and is not reachable. What the application passes in are the commands
/// about the phone itself, which belong beside the connection rather than here.
macro_rules! commands {
    ($($also:path),* $(,)?) => {
        tauri::generate_handler![
            $($also,)*
            crate::window::projects_registered,
            crate::window::memory_open,
            crate::window::memory_status,
            crate::window::memory_types,
            crate::window::memory_type_create,
            crate::window::memory_type_update,
            crate::window::memory_type_delete,
            crate::window::memory_extension_types_publish,
            crate::window::memory_folder_attach,
            crate::window::memory_scan,
            crate::window::memory_unmatched_resolve,
            crate::window::memory_folders,
            crate::window::memory_folder_create,
            crate::window::memory_folder_describe,
            crate::window::memory_folder_delete,
            crate::window::memory_folder_toll,
            crate::window::memory_folder_rename,
            crate::window::memory_document_move,
            crate::window::memory_records,
            crate::window::memory_document,
            crate::window::memory_content,
            crate::window::memory_file_create,
            crate::window::memory_document_update,
            crate::window::memory_document_create,
            crate::window::memory_document_delete,
            crate::window::memory_document_dependents,
            crate::window::memory_list,
            crate::window::memory_search,
            crate::window::memory_get,
            crate::window::memory_save,
            crate::window::memory_delete,
            crate::window::memory_sync_state,
            crate::window::memory_presence,
            crate::window::memory_remote_set,
            crate::window::memory_remote_remove,
            crate::window::memory_fetch,
            crate::window::memory_push,
            crate::window::memory_reindex,
            crate::window::memory_reconcile,
            crate::window::project_settings_load,
            crate::window::project_settings_save,
            crate::window::extension_list,
            crate::window::extension_fetch,
            crate::window::registry_index,
            crate::window::registry_cached_index,
            crate::window::registry_ledger,
            crate::window::extension_install_registry,
            crate::window::extension_forget,
            crate::window::extension_repoint,
            crate::window::extension_handler_call,
            crate::window::schedule_remember,
            crate::window::schedule_switched_off,
            crate::window::schedule_switch,
        ]
    };
}
pub(crate) use commands;

// ── The projects there are ──────────────────────────────────────────────────

/// One project of the computer's registry, as the window lists it.
///
/// The same three members the Mac's own registry entry carries, and `path` is
/// the one worth stopping on: on the Mac it is a directory, and here it is the
/// key. The window treats it as neither — it is the handle a project is asked
/// about by, passed back unread in every call that follows — and the phone is
/// where a handle that is not a path is true.
///
/// A phone is never told where a project is. The door does not send it and
/// there is no operation that would take it.
#[derive(Debug, Serialize)]
pub struct KnownProject {
    path: String,
    name: String,
    identifier: String,
}

/// The projects the computer holds.
///
/// Answered before any project has been named — it is the one call a connection
/// can make while it still has nothing to say about *which* project — and it is
/// what a phone opens onto.
#[tauri::command(async)]
pub fn projects_registered(channel: State<'_, Channel>) -> Answered<Vec<KnownProject>> {
    let listed = channel.ask(sync_memory::PROJECTS, &serde_json::json!({}))?;
    let listed = listed
        .get("projects")
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError {
            kind: "protocol".to_owned(),
            message: "the computer listed no projects and did not say so".to_owned(),
            data: Value::Null,
        })?;
    Ok(listed
        .iter()
        .filter_map(|project| {
            let key = project.get("project").and_then(Value::as_str)?;
            Some(KnownProject {
                path: key.to_owned(),
                name: project
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_owned(),
                identifier: key.to_owned(),
            })
        })
        .collect())
}

// ── The project's memory ────────────────────────────────────────────────────

/// What the window is told about the engine answering for a project.
///
/// The Mac fills this from a binary it started and can see. A phone can see
/// none of that: the engine is on somebody's computer, and the honest members
/// are the ones the project itself answers with. `source` says so in a word,
/// which is why it has a third value rather than borrowing one of the Mac's.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSummary {
    binary: String,
    source: String,
    version: String,
    project_id: String,
    revision: String,
    records_backend: Option<String>,
    records_are_git: bool,
    model_fingerprint: Option<String>,
}

/// Open a project's memory and describe what is answering for it.
#[tauri::command(async)]
pub fn memory_open(project: String, channel: State<'_, Channel>) -> Answered<EngineSummary> {
    Ok(opening(&*channel, &project)?)
}

/// The three calls opening a project is made of, in the order it needs them.
///
/// Its own function because it is the only command here that is more than one
/// call, and the order is the part worth holding: the types Sync needs for its
/// own records are published before anything is read, because the store runs a
/// strict schema and a project whose `project` record has no definition is one
/// nothing can write. The revision and the description come after, and describe
/// the store as it stands once that is true.
fn opening(asks: &dyn Asks, project: &str) -> sync_memory::Result<EngineSummary> {
    let mut asking = Asking::about(asks, project);
    asking.publish_types()?;
    let revision = asking.read_revision()?;
    let handshake = asking.describe()?;
    Ok(EngineSummary {
        // What a phone cannot know, said as nothing rather than as a guess:
        // the engine is a binary on somebody else's machine, and this window
        // has never seen it. `source` carries that in a word.
        binary: String::new(),
        source: "channel".to_owned(),
        version: format!(
            "{}.{}",
            handshake.memory_interface_version.major, handshake.memory_interface_version.minor
        ),
        project_id: handshake.project_id.clone(),
        revision,
        records_backend: handshake.backend.clone(),
        records_are_git: handshake.records_are_git(),
        model_fingerprint: handshake.model_fingerprint.clone(),
    })
}

/// The states the window renders: search mode and remote.
///
/// `reconnecting` is the Mac's question about a process it started, and this
/// phone has no process to ask about. A connection that is down does not reach
/// this at all — the call fails, and the failure is what the window shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    revision: String,
    reconnecting: bool,
    model: Value,
    transport: Value,
}

#[tauri::command(async)]
pub fn memory_status(project: String, channel: State<'_, Channel>) -> Answered<MemoryStatus> {
    let mut asking = channel.about(&project);
    let revision = asking.read_revision()?;
    let model = asking.model_status()?;
    let transport = asking.transport_status()?;
    Ok(MemoryStatus {
        revision,
        reconnecting: false,
        model: serde_json::to_value(model).unwrap_or(Value::Null),
        transport: serde_json::to_value(transport).unwrap_or(Value::Null),
    })
}

#[tauri::command(async)]
pub fn memory_types(project: String, channel: State<'_, Channel>) -> Answered<Vec<RecordType>> {
    Ok(channel.about(&project).list_types()?)
}

#[tauri::command(async)]
pub fn memory_type_create(
    project: String,
    kind: String,
    title: String,
    description: String,
    icon: String,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel
        .about(&project)
        .create_type(&kind, &title, &description, &icon)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_type_update(
    project: String,
    kind: String,
    title: String,
    description: String,
    icon: String,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel
        .about(&project)
        .update_type(&kind, &title, &description, &icon)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_type_delete(
    project: String,
    kind: String,
    channel: State<'_, Channel>,
) -> Answered<usize> {
    Ok(channel.about(&project).delete_type(&kind)?)
}

#[tauri::command(async)]
pub fn memory_extension_types_publish(
    project: String,
    types: Value,
    channel: State<'_, Channel>,
) -> Answered<bool> {
    Ok(channel.about(&project).publish_extension_types(&types)?)
}

#[tauri::command(async)]
pub fn memory_folder_attach(
    project: String,
    kind: String,
    title: String,
    description: String,
    icon: String,
    folder: String,
    channel: State<'_, Channel>,
) -> Answered<ScanOutcome> {
    Ok(channel
        .about(&project)
        .attach_folder(&kind, &title, &description, &icon, &folder)?)
}

#[tauri::command(async)]
pub fn memory_scan(project: String, channel: State<'_, Channel>) -> Answered<ScanOutcome> {
    Ok(channel.about(&project).scan()?)
}

#[tauri::command(async)]
pub fn memory_unmatched_resolve(
    project: String,
    locator: String,
    content_hash: String,
    kind: String,
    adopt: Option<String>,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).resolve_unmatched(
        &locator,
        &content_hash,
        &kind,
        adopt.as_deref(),
    )?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_folders(
    project: String,
    folder: Option<String>,
    subtree: bool,
    kind: Option<String>,
    channel: State<'_, Channel>,
) -> Answered<Vec<FolderEntry>> {
    Ok(channel
        .about(&project)
        .folders(folder.as_deref(), subtree, kind.as_deref())?)
}

#[tauri::command(async)]
pub fn memory_folder_create(
    project: String,
    folder: String,
    kind: String,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).create_folder(&folder, &kind)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_folder_describe(
    project: String,
    folder: String,
    kind: String,
    channel: State<'_, Channel>,
) -> Answered<Document> {
    Ok(channel.about(&project).describe_folder(&folder, &kind)?)
}

#[tauri::command(async)]
pub fn memory_folder_delete(
    project: String,
    folder: String,
    channel: State<'_, Channel>,
) -> Answered<usize> {
    Ok(channel.about(&project).delete_folder(&folder)?)
}

#[tauri::command(async)]
pub fn memory_folder_toll(
    project: String,
    folder: String,
    channel: State<'_, Channel>,
) -> Answered<usize> {
    Ok(channel.about(&project).folder_toll(&folder)?)
}

#[tauri::command(async)]
pub fn memory_folder_rename(
    project: String,
    from: String,
    to: String,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).rename_folder(&from, &to)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_document_move(
    project: String,
    key: String,
    folder: String,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).move_document(&key, &folder)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_records(
    project: String,
    selection: Value,
    hidden: Vec<String>,
    channel: State<'_, Channel>,
) -> Answered<RecordsPage> {
    Ok(channel.about(&project).records(&selection, &hidden)?)
}

#[tauri::command(async)]
pub fn memory_document(
    project: String,
    key: String,
    channel: State<'_, Channel>,
) -> Answered<Option<Document>> {
    Ok(channel.about(&project).document(&key)?)
}

#[tauri::command(async)]
pub fn memory_content(
    project: String,
    key: String,
    channel: State<'_, Channel>,
) -> Answered<ContentView> {
    Ok(channel.about(&project).read_content(&key)?)
}

#[tauri::command(async)]
pub fn memory_file_create(
    project: String,
    kind: String,
    name: String,
    content: String,
    channel: State<'_, Channel>,
) -> Answered<Document> {
    Ok(channel
        .about(&project)
        .create_file_document(&kind, &name, &content)?)
}

#[tauri::command(async)]
pub fn memory_document_update(
    project: String,
    key: String,
    edits: DocumentEdits,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).update_document(&key, &edits)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_document_create(
    project: String,
    kind: String,
    title: String,
    folder: Option<String>,
    channel: State<'_, Channel>,
) -> Answered<Document> {
    Ok(channel
        .about(&project)
        .create_document(&kind, &title, folder.as_deref())?)
}

#[tauri::command(async)]
pub fn memory_document_delete(
    project: String,
    keys: Vec<String>,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).delete_documents(&keys)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_document_dependents(
    project: String,
    key: String,
    channel: State<'_, Channel>,
) -> Answered<Dependents> {
    Ok(channel.about(&project).dependents(&key)?)
}

#[tauri::command(async)]
pub fn memory_list(project: String, query: Value, channel: State<'_, Channel>) -> Answered<Value> {
    let listing = channel.about(&project).list_records(&query)?;
    Ok(serde_json::to_value(listing).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_search(
    project: String,
    query: Value,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let outcome = channel.about(&project).search(&query)?;
    Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_get(project: String, key: String, channel: State<'_, Channel>) -> Answered<Value> {
    let view = channel.about(&project).get_record(&key)?;
    Ok(serde_json::to_value(view).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_save(
    project: String,
    entities: Vec<EntityInput>,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).save_entities(&entities)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_delete(
    project: String,
    keys: Vec<String>,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).delete_documents(&keys)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn memory_sync_state(
    project: String,
    ask_remote: bool,
    channel: State<'_, Channel>,
) -> Answered<SyncState> {
    Ok(channel.about(&project).sync_state(ask_remote)?)
}

#[tauri::command(async)]
pub fn memory_presence(project: String, channel: State<'_, Channel>) -> Answered<MemoryPresence> {
    Ok(channel.about(&project).presence()?)
}

#[tauri::command(async)]
pub fn memory_remote_set(
    project: String,
    url: String,
    refspec: Option<String>,
    channel: State<'_, Channel>,
) -> Answered<TransportStatus> {
    Ok(channel
        .about(&project)
        .set_remote(&url, refspec.as_deref())?)
}

#[tauri::command(async)]
pub fn memory_remote_remove(
    project: String,
    channel: State<'_, Channel>,
) -> Answered<TransportStatus> {
    Ok(channel.about(&project).remove_remote()?)
}

#[tauri::command(async)]
pub fn memory_fetch(project: String, channel: State<'_, Channel>) -> Answered<FetchOutcome> {
    Ok(channel.about(&project).fetch()?)
}

#[tauri::command(async)]
pub fn memory_push(project: String, force: bool, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.about(&project).push(force)?)
}

#[tauri::command(async)]
pub fn memory_reindex(project: String, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.about(&project).reindex()?)
}

#[tauri::command(async)]
pub fn memory_reconcile(
    project: String,
    full_rebuild: bool,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    Ok(channel.about(&project).reconcile(full_rebuild)?)
}

// ── The project's own record ────────────────────────────────────────────────

#[tauri::command(async)]
pub fn project_settings_load(project: String, channel: State<'_, Channel>) -> Answered<Value> {
    let settings = channel.about(&project).project_settings()?;
    Ok(serde_json::to_value(settings).unwrap_or(Value::Null))
}

#[tauri::command(async)]
pub fn project_settings_save(
    project: String,
    settings: sync_memory::ProjectSettings,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    let written = channel.about(&project).update_project(&settings)?;
    Ok(serde_json::to_value(written).unwrap_or(Value::Null))
}

// ── Packages ────────────────────────────────────────────────────────────────

/// The packages the computer serves.
///
/// Not *the packages on this phone*, and the difference is the whole model:
/// installing writes an artefact on a machine and a declaration into the
/// project, and the phone is neither. What it draws is what the computer has,
/// fetched file by file over the channel as the window imports it.
#[tauri::command(async)]
pub fn extension_list(channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(sync_memory::EXTENSION_LIST, &serde_json::json!({}))?)
}

/// What the registry says exists, fetched by the computer.
///
/// The fetch belongs there and not here: it is conditional on an ETag, it
/// leaves a cache beside the artefacts, and what it answers decides what that
/// same machine will download and check. A phone asking the registry directly
/// would be a second reader of it, with its own cache and its own idea of what
/// exists.
#[tauri::command(async)]
pub fn registry_index(channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(sync_memory::REGISTRY_INDEX, &serde_json::json!({}))?)
}

/// What the computer's last fetch left on its disk, asking nobody.
#[tauri::command(async)]
pub fn registry_cached_index(channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(sync_memory::REGISTRY_CACHED, &serde_json::json!({}))?)
}

/// Every version one package has published.
#[tauri::command(async)]
pub fn registry_ledger(id: String, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(sync_memory::REGISTRY_LEDGER, &serde_json::json!({"id": id}))?)
}

/// Install what the registry named — on the computer, which is the only place
/// an artefact means anything.
///
/// The types it publishes and the project's declaration of it are written by
/// the window afterwards, through the memory commands above, exactly as on a
/// Mac. So installing from a phone is the same act as installing from the
/// desktop rather than a second kind of install, and what a person ends up with
/// is one inventory instead of two to keep in step.
#[tauri::command(async)]
pub fn extension_install_registry(artefact: Value, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::EXTENSION_INSTALL,
        &serde_json::json!({"artefact": artefact}),
    )?)
}

/// Stop serving an id on the computer. The artefact and its records stay.
#[tauri::command(async)]
pub fn extension_forget(id: String, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::EXTENSION_FORGET,
        &serde_json::json!({"id": id}),
    )?)
}

/// Point an id back at the artefact it was serving, which is how an update that
/// failed halfway is undone.
#[tauri::command(async)]
pub fn extension_repoint(pointer: Value, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::EXTENSION_REPOINT,
        &serde_json::json!({"pointer": pointer}),
    )?)
}

/// Run the handler a package declared for an occasion.
///
/// Taking an extension on is three steps in one order, and this is the middle
/// one: its types are published first so the handler may write records of them,
/// and the project's declaration is written last so a handler that refuses
/// leaves a project that never took the extension on. The order is the window's
/// and does not change here — only where the module is evaluated, which is
/// beside the artefact and never on this phone.
#[tauri::command(async)]
pub fn extension_handler_call(
    project: String,
    id: String,
    occasion: String,
    payload: Value,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::EXTENSION_OCCASION,
        &serde_json::json!({
            "project": project, "id": id, "occasion": occasion, "payload": payload,
        }),
    )?)
}

// ── Clocks ──────────────────────────────────────────────────────────────────

/// Tell the computer which packages this project declares, so its clocks know
/// what to run.
///
/// The computer's answer and not this phone's, and there is nothing to decide
/// about that: a clock ticks where the packages are, with no window open, and a
/// phone in somebody's pocket is not where anything runs.
#[tauri::command(async)]
pub fn schedule_remember(
    project: String,
    extensions: Vec<String>,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::SCHEDULE_REMEMBER,
        &serde_json::json!({"project": project, "extensions": extensions}),
    )?)
}

/// Which of a project's clocks the computer has switched off.
#[tauri::command(async)]
pub fn schedule_switched_off(project: String, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::SCHEDULE_OFF,
        &serde_json::json!({"project": project}),
    )?)
}

/// Switch one clock, on the computer that runs it.
#[tauri::command(async)]
pub fn schedule_switch(
    project: String,
    id: String,
    on: bool,
    channel: State<'_, Channel>,
) -> Answered<Value> {
    Ok(channel.ask(
        sync_memory::SCHEDULE_SWITCH,
        &serde_json::json!({"project": project, "id": id, "on": on}),
    )?)
}

/// A package's own network request, made on the computer.
///
/// It travels as itself rather than as an operation of a project: what a
/// package may reach is a sentence in the manifest of the artefact installed on
/// that machine, and the secret it signs with is in that machine's keychain, so
/// both the check and the request belong there. This phone names neither.
#[tauri::command(async)]
pub fn extension_fetch(id: String, request: Value, channel: State<'_, Channel>) -> Answered<Value> {
    Ok(carrying(&*channel, &id, &request)?)
}

/// The call a package's request travels as.
///
/// Its own function because it is the one call here that names no project, and
/// that is the whole of what is worth checking about it: a package is not a
/// project, and a key put in this call would be a phone claiming the request
/// belongs to whatever it happens to be looking at.
fn carrying(asks: &dyn Asks, id: &str, request: &Value) -> sync_memory::Result<Value> {
    asks.ask(
        sync_memory::EXTENSION_FETCH,
        &serde_json::json!({"id": id, "request": request}),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Mutex;

    use serde_json::json;
    use sync_memory::{MemoryError, Operations as _};

    use super::{carrying, opening};
    use crate::channel::{Asking, Asks};

    /// A computer that writes down what it was asked and says what it was told
    /// to say.
    struct Heard {
        asked: Mutex<Vec<(String, serde_json::Value)>>,
        answers: Mutex<std::collections::VecDeque<sync_memory::Result<serde_json::Value>>>,
    }

    impl Heard {
        fn answering_each(answers: Vec<sync_memory::Result<serde_json::Value>>) -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                answers: Mutex::new(answers.into()),
            }
        }

        fn answering(answer: sync_memory::Result<serde_json::Value>) -> Self {
            Self::answering_each(vec![answer])
        }

        fn saying(answer: serde_json::Value) -> Self {
            Self::answering(Ok(answer))
        }

        fn once(&self) -> (String, serde_json::Value) {
            let asked = self.asked.lock().expect("nothing panicked");
            assert_eq!(asked.len(), 1, "one call was made: {asked:?}");
            asked[0].clone()
        }
    }

    impl Asks for Heard {
        fn ask(
            &self,
            method: &str,
            params: &serde_json::Value,
        ) -> sync_memory::Result<serde_json::Value> {
            self.asked
                .lock()
                .expect("nothing panicked")
                .push((method.to_owned(), params.clone()));
            self.answers
                .lock()
                .expect("nothing panicked")
                .pop_front()
                .expect("the test has an answer for every call")
        }
    }

    /// A read: the operation is named by the shared vocabulary and the project
    /// travels in the call, because the door keeps no memory of which project a
    /// connection is about.
    #[test]
    fn a_read_goes_out_as_its_operation_with_the_project_named() {
        let heard = Heard::saying(json!({"revision": "abc", "record": {"key": "d-1"}}));
        let read = Asking::about(&heard, "SYNC")
            .get_record("d-1")
            .expect("the computer answered");

        let (method, params) = heard.once();
        assert_eq!(method, "records.get");
        assert_eq!(params["key"], "d-1");
        assert_eq!(params["project"], "SYNC");
        assert_eq!(read.record.expect("a record")["key"], "d-1");
    }

    /// A write, and the same two things are true of it. The point of checking
    /// one of each is that neither is spelled here: both come from the trait
    /// the two applications share.
    #[test]
    fn a_write_goes_out_as_its_operation_with_the_project_named() {
        let heard = Heard::saying(json!({"revision": "abc", "written": []}));
        drop(
            Asking::about(&heard, "SYNC")
                .save_entities(&[])
                .expect("the computer answered"),
        );

        let (method, params) = heard.once();
        assert_eq!(method, "records.save");
        assert_eq!(params["entities"], json!([]));
        assert_eq!(params["project"], "SYNC");
    }

    /// A package's request names the package and no project.
    ///
    /// What may be reached is a sentence in the manifest of the artefact on the
    /// computer, so the call carries the package's id and the request and
    /// nothing else — a project put in it here would be this phone answering a
    /// question the computer asks of the manifest.
    #[test]
    fn a_package_s_request_names_the_package_and_no_project() {
        let heard = Heard::saying(json!({"status": 200}));
        drop(
            carrying(
                &heard,
                "buzz.records",
                &json!({"url": "https://example.test"}),
            )
            .expect("the computer answered"),
        );

        let (method, params) = heard.once();
        assert_eq!(method, "extension.fetch");
        assert_eq!(params["id"], "buzz.records");
        assert_eq!(
            params.as_object().expect("an object").len(),
            2,
            "the phone added something to a call it only carries: {params}"
        );
    }

    /// Opening a project publishes what Sync needs before it reads anything.
    ///
    /// The order is the whole of it. The store runs a strict schema, so a
    /// project whose own record has no type definition is one nothing can
    /// write — and reading the revision first would describe a store that is
    /// about to change under the answer.
    #[test]
    fn opening_a_project_publishes_before_it_reads() {
        let heard = Heard::answering_each(vec![
            Ok(json!(false)),
            Ok(json!("97a64dc")),
            Ok(json!({
                "memoryInterfaceVersion": {"major": 2, "minor": 1},
                "storeVersion": {"major": 1, "minor": 0},
                "envelopeVersion": {},
                "indexVersion": {"major": 1, "minor": 0},
                "installationId": "i",
                "projectId": "SYNC",
                "projectPath": "/somebody/else/s/disk",
                "backend": "refs"
            })),
        ]);
        let opened = opening(&heard, "SYNC").expect("the computer answered");

        let asked = heard.asked.lock().expect("nothing panicked");
        let named: Vec<&str> = asked.iter().map(|(method, _)| method.as_str()).collect();
        assert_eq!(
            named,
            ["types.publish", "project.revision", "project.describe"]
        );
        assert!(
            asked.iter().all(|(_, params)| params["project"] == "SYNC"),
            "a call went out without the project on it: {asked:?}"
        );
        assert_eq!(opened.revision, "97a64dc");
        assert_eq!(opened.project_id, "SYNC");
        assert!(opened.records_are_git);
        // The engine is on somebody else's machine and this window has never
        // seen it. Saying so beats naming a binary that is not there.
        assert_eq!(opened.source, "channel");
        assert_eq!(opened.binary, "");
        assert_eq!(opened.version, "2.1");
    }

    /// A refusal arrives as the computer said it, kind and all.
    ///
    /// The window branches on the kind — a conflict is offered a reread, an
    /// unreadable record is not — so a client that flattened this into a
    /// sentence would leave every screen with nothing to do but apologise.
    #[test]
    fn a_refusal_keeps_the_kind_the_computer_gave_it() {
        let heard = Heard::answering(Err(MemoryError::domain(
            "conflict",
            "someone else wrote that record",
            json!({"expected": "abc"}),
        )));
        let refused = Asking::about(&heard, "SYNC")
            .save_entities(&[])
            .expect_err("the computer refused");

        let refused = sync_memory::CommandError::from(refused);
        assert_eq!(refused.kind, "conflict");
        assert_eq!(refused.message, "someone else wrote that record");
        assert_eq!(refused.data["expected"], "abc");
    }
}
