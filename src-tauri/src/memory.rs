//! The desktop adapter for project memory.
//!
//! Everything here is thin by design: parse the command's input, call
//! [`sync_memory`], map the result. The rules that decide *what* happens —
//! which binary runs, when to retry a conflict, what a locked project may
//! answer — live in that crate, which compiles and runs without Tauri.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sync_memory::{
    ContentView, Dependents, Document, DocumentEdits, FetchOutcome, FolderEntry, LaunchConfig,
    MemoryClient, MemoryError, MemoryPresence, RecordType, RecordsPage, ScanOutcome, SyncState,
    TransactionResult, TransportStatus,
};
use tauri::{AppHandle, Manager, Runtime, State};

/// The sidecar's name inside the bundle.
///
/// Tauri strips the target triple from `externalBin` entries when it bundles
/// them, so `binaries/sync-mcp-aarch64-apple-darwin` ships as this.
pub(crate) const BUNDLED_BINARY: &str = "sync-mcp";

/// Points a development build at a sidecar built from source, where no bundle
/// exists to take one from.
///
/// The same name the end-to-end tests read, because it names the same thing:
/// a `sync-mcp` binary, with the engine already inside it. Pointing it at a
/// `memory-hub` would name the engine alone, which does not speak this
/// window's channel.
pub(crate) const BINARY_OVERRIDE: &str = "SYNC_MCP_BINARY";

/// One live session per open project, opened on first use.
///
/// **A session is a connection, not an engine.** This installation runs one
/// `sync-mcp` for the whole machine and a session is a socket to it that has
/// said which project it is about, so four projects open cost four file
/// descriptors rather than four engines and four copies of the model. It is a
/// map because the client keeps one thing per
/// project that cannot be shared: the revision it expects its next write to be
/// against.
///
/// Sessions are kept for the life of the application. Opening one costs a
/// connect and a handshake, so there is nothing to gain from tearing one down.
///
/// Where no resident process was started — a test driving these commands with
/// no application around them — a session is a process of its own instead, and
/// that is the one arrangement where an engine per project is right: a test's
/// corpus is nobody else's.
///
/// **Two locks, and the shape is the point.** The outer one guards the map and
/// is held for the length of a lookup; the inner one guards a connection and is
/// held for the length of a call. A single lock over the map would have to be
/// the second — a call needs the client for as long as it runs — and then every
/// project waits behind whichever one is busy, which is a person watching a
/// list redraw because a different project is being written to.
///
/// The connection is `Option` so that opening one happens under its own lock
/// rather than the map's. Connecting starts a process and greets it; doing that
/// while holding the map would stop every project for as long as it takes, in
/// the one moment the window is least able to spare it.
#[derive(Default)]
pub struct MemorySessions {
    sessions: Mutex<HashMap<PathBuf, Arc<Mutex<Option<MemoryClient>>>>>,
}

/// A failure, in the shape the frontend branches on.
///
/// `kind` is the engine's stable vocabulary (`conflict`, `locked`,
/// `signing_not_configured`, …) plus this layer's own (`sidecar`, `protocol`,
/// `incompatible_interface`). The UI switches on it; the message is for people.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub kind: String,
    pub message: String,
    pub data: Value,
}

impl From<MemoryError> for CommandError {
    fn from(error: MemoryError) -> Self {
        match error {
            MemoryError::Domain {
                ref kind,
                ref message,
                ref data,
            } => Self {
                kind: kind.as_wire().to_owned(),
                message: message.clone(),
                data: data.clone(),
            },
            MemoryError::Sidecar(ref reason) => Self {
                kind: "sidecar".to_owned(),
                message: reason.clone(),
                data: Value::Null,
            },
            MemoryError::Protocol(_) | MemoryError::Io(_) => Self {
                kind: "protocol".to_owned(),
                message: error.to_string(),
                data: Value::Null,
            },
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// What the UI needs to describe the engine it is talking to.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSummary {
    pub binary: String,
    /// `installed` or `bundled` — the same engine either way, but updated by
    /// different people.
    pub source: String,
    pub version: String,
    pub project_id: String,
    pub revision: String,
    /// Which storage holds this project's records: `refs` for the Git objects
    /// Sync initialises, `folder` for a project some other client set up as
    /// files. `null` for an engine that did not say.
    pub records_backend: Option<String>,
    /// Whether the records are Git objects, and so whether diff, fetch and
    /// push mean anything here.
    ///
    /// Asked once, on the way in. A project keeping its records as files
    /// answers `unsupported` to every one of those, and a window that offers
    /// them anyway is a window explaining a refusal it could have avoided.
    pub records_are_git: bool,
    /// `null` when no embedding model is installed: search is FTS-only, which
    /// is a normal state and the UI should say so plainly.
    pub model_fingerprint: Option<String>,
}

pub use sync_memory::EntityInput;

/// One project's connection, opened on first use and shared by everything
/// that asks for that project.
type Session = Arc<Mutex<Option<MemoryClient>>>;

impl MemorySessions {
    /// Run an operation against a project's session, opening one if needed.
    ///
    /// **The waiting happens on a thread that exists to wait.** Tauri runs a
    /// command as an ordinary task on its async runtime, which has one worker
    /// per core, and every call here blocks until the engine answers — so a
    /// handful of slow calls would occupy every worker and stop commands that
    /// have nothing to do with memory: loading a package, opening a project,
    /// a clock coming round. Moving the blocking part to the blocking pool
    /// keeps those workers free, whatever memory is doing.
    ///
    /// `pub(crate)` because opening a folder as a project has to read and write
    /// one record before any screen exists to do it from — see
    /// [`crate::project`]. Nothing outside this crate gets a session.
    ///
    /// Two projects run at once; two calls to one project queue, because a
    /// session is one connection and its answers arrive in the order they were
    /// asked for. What this rules out is a call to *another* project waiting on
    /// that queue, which is what the window notices: a folder being deleted no
    /// longer holds up every other project's lists.
    pub(crate) async fn with_session<T, R: Runtime>(
        &self,
        app: &AppHandle<R>,
        project: &str,
        operation: impl FnOnce(&mut MemoryClient) -> Result<T, MemoryError> + Send + 'static,
    ) -> CommandResult<T>
    where
        T: Send + 'static,
    {
        let path = PathBuf::from(project);
        let session = self.session(&path)?;
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            run(session, || launch_config(&app, path), operation)
        })
        .await
        .map_err(|error| CommandError {
            kind: "protocol".to_owned(),
            message: format!("the memory call did not finish: {error}"),
            data: Value::Null,
        })?
    }

    /// The same call, made on the thread that asks for it.
    ///
    /// For a caller that cannot wait on a future: an extension handler answers
    /// inside a synchronous host call, and the isolate on the other side of it
    /// is waiting for the value rather than for a promise.
    pub(crate) fn with_session_here<T, R: Runtime>(
        &self,
        app: &AppHandle<R>,
        project: &str,
        operation: impl FnOnce(&mut MemoryClient) -> Result<T, MemoryError>,
    ) -> CommandResult<T> {
        let path = PathBuf::from(project);
        let session = self.session(&path)?;
        run(session, || launch_config(app, path), operation)
    }

    /// Find or make room for a project's session, holding the map no longer
    /// than the lookup.
    fn session(&self, project: &Path) -> CommandResult<Session> {
        let mut sessions = self.sessions.lock().map_err(|_| CommandError {
            kind: "protocol".to_owned(),
            message: "the memory session registry is poisoned".to_owned(),
            data: Value::Null,
        })?;
        Ok(Arc::clone(
            sessions.entry(project.to_path_buf()).or_default(),
        ))
    }
}

/// Hold one project's connection for the length of one call, opening it first
/// where this is the call that found it closed.
///
/// Where the engine lives is asked for rather than passed, because asking costs
/// an environment variable, the path of this executable and the log directory —
/// and a session that is already open needs none of them. Reading them anyway
/// would put three ways to fail in front of every call to a connection that is
/// working.
fn run<T>(
    session: Session,
    config: impl FnOnce() -> CommandResult<LaunchConfig>,
    operation: impl FnOnce(&mut MemoryClient) -> Result<T, MemoryError>,
) -> CommandResult<T> {
    let mut session = session.lock().map_err(|_| CommandError {
        kind: "protocol".to_owned(),
        message: "the memory session for this project is poisoned".to_owned(),
        data: Value::Null,
    })?;
    // A connection that failed to open leaves the slot empty rather than
    // remembering the failure, so the next call tries again. An engine that
    // was not installed yet is the ordinary case for that.
    if session.is_none() {
        *session = Some(MemoryClient::connect(config()?)?);
    }
    let client = session
        .as_mut()
        .expect("the session was opened above or the call returned");
    operation(client).map_err(CommandError::from)
}

/// Where the engine and its log live for this installation.
fn launch_config<R: Runtime>(app: &AppHandle<R>, project: PathBuf) -> CommandResult<LaunchConfig> {
    let override_binary = std::env::var_os(BINARY_OVERRIDE).map(PathBuf::from);
    let bundled = std::env::current_exe()
        .map_err(|error| CommandError {
            kind: "sidecar".to_owned(),
            message: format!("could not locate the application executable: {error}"),
            data: Value::Null,
        })?
        .parent()
        .map(|directory| directory.join(BUNDLED_BINARY))
        .ok_or_else(|| CommandError {
            kind: "sidecar".to_owned(),
            message: "the application executable has no parent directory".to_owned(),
            data: Value::Null,
        })?;
    let log_file = app
        .path()
        .app_log_dir()
        .map_err(|error| CommandError {
            kind: "sidecar".to_owned(),
            message: format!("could not resolve the log directory: {error}"),
            data: Value::Null,
        })?
        .join("memory.log");
    Ok(LaunchConfig {
        project,
        bundled,
        log_file,
        override_binary,
        // Where this installation's engine is, when it runs one. A window
        // reaches the process that already serves every project on the machine
        // rather than starting one of its own.
        //
        // `None` where no server was started, which is a test driving these
        // commands with no application around them. It is not a fall back for
        // production: a server that failed to start is reported, and a socket
        // named but unreachable is an error rather than a quiet return to one
        // engine per project.
        host_socket: app
            .try_state::<crate::server::RunningServer>()
            .and_then(|running| running.socket()),
    })
}

/// Open a project's memory and describe the engine serving it.
///
/// Every command here is an `async fn` that awaits
/// [`MemorySessions::with_session`], and the waiting it does is the point: a
/// search may rebuild the index and a fetch talks to a network, so the call
/// that blocks belongs on a thread kept for blocking rather than on one of the
/// few the runtime schedules everything else with.
///
/// The type corpus is brought up to date on open: the engine runs a strict
/// schema, so a record whose kind has no `__type__` definition is rejected at
/// write time. `publish_types` writes only when the store's corpus differs from
/// this build's, so opening the same project twice is a read both times.
#[tauri::command]
pub async fn memory_open<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<EngineSummary> {
    sessions
        .with_session(&app, &project, move |client| {
            client.publish_types()?;
            let info = client.info();
            Ok(EngineSummary {
                binary: info.binary.to_string_lossy().into_owned(),
                source: match info.source {
                    sync_memory::BinarySource::Override => "override".to_owned(),
                    sync_memory::BinarySource::Bundled => "bundled".to_owned(),
                },
                version: info.version.clone(),
                project_id: info.handshake.project_id.clone(),
                revision: client.revision().to_owned(),
                records_backend: info.handshake.backend.clone(),
                records_are_git: info.handshake.records_are_git(),
                model_fingerprint: info.handshake.model_fingerprint.clone(),
            })
        })
        .await
}

/// The states the UI has to render: lock, search mode, remote.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub revision: String,
    /// True while the engine process is gone and the next call will reconnect.
    pub reconnecting: bool,
    pub model: Value,
    pub transport: Value,
}

/// Read the states the UI renders, in one round trip.
#[tauri::command]
pub async fn memory_status<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<MemoryStatus> {
    sessions
        .with_session(&app, &project, move |client| {
            let reconnecting = !client.engine_is_alive();
            let model = client.model_status()?;
            let transport = client.transport_status()?;
            Ok(MemoryStatus {
                revision: client.revision().to_owned(),
                reconnecting,
                model: serde_json::to_value(model).unwrap_or(Value::Null),
                transport: serde_json::to_value(transport).unwrap_or(Value::Null),
            })
        })
        .await
}

/// The types the project holds, in the order the navigator lists them.
///
/// Read from the project's own corpus, marks included. Sync publishes one type
/// — `project`, without which the project's own record could not be written —
/// and everything else on this list is the project's own.
#[tauri::command]
pub async fn memory_types<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<Vec<RecordType>> {
    sessions
        .with_session(&app, &project, MemoryClient::list_types)
        .await
}

/// Add a type to the project's corpus.
///
/// `kind` is what the engine stores and what every record of this type carries;
/// `title` is what the window calls it; `icon` is the name of the mark it is
/// drawn with. The last two are kept inside the definition because no build can
/// know a type somebody invented here.
#[tauri::command]
pub async fn memory_type_create<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    kind: String,
    title: String,
    description: String,
    icon: String,
) -> CommandResult<Vec<RecordType>> {
    sessions
        .with_session(&app, &project, move |client| {
            client.create_type(&kind, &title, &description, &icon)?;
            client.list_types()
        })
        .await
}

/// Publish the types an extension brings, as one transaction.
///
/// Installing is all-or-nothing: a project holding three of an extension's five
/// types validates its records against a schema nobody chose. Republishing a
/// set that is already there writes nothing, which is what lets a project
/// declare its extensions and have them published on every machine that opens
/// it without a commit per open.
///
/// The definitions come from the caller because the catalogue is the window's,
/// not the engine's. What the engine enforces is the schema: a definition it
/// refuses is an extension that must not count as installed.
#[tauri::command]
pub async fn memory_extension_types_publish<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    types: Vec<ExtensionTypeInput>,
) -> CommandResult<Vec<RecordType>> {
    let types = serde_json::to_value(&types).unwrap_or(Value::Null);
    sessions
        .with_session(&app, &project, move |client| {
            client.publish_extension_types(&types)?;
            client.list_types()
        })
        .await
}

/// One type an extension publishes, as the catalogue states it.
///
/// The last three are what an extension brings and a type made in the window
/// does not have: the product fields its records carry, the relations they may
/// hold, and what an agent is told before it writes one. They travel as the
/// caller wrote them — the engine's schema is what says whether a declaration
/// is well formed, and a second opinion in this layer would be one more thing
/// to keep in step with an engine release.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionTypeInput {
    /// Prefixed with the extension's id, so two extensions cannot define the
    /// same kind differently.
    pub kind: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    /// Declarations by field name, as the engine's schema spells them.
    #[serde(default)]
    pub fields: Map<String, Value>,
    /// Relations by name: `{target, description}`, where `target` is a kind or
    /// `any`.
    #[serde(default)]
    pub relationships: Map<String, Value>,
    /// What an agent reads before writing a record of this type. Absent for a
    /// type that has nothing of its own to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Redefine a type the project holds.
///
/// `kind` names which one and is the one thing that cannot change: it is the
/// identifier every record of the type carries, and the store has no rename.
/// What travels is the same three answers the type was named with, so one form
/// asks them both times.
#[tauri::command]
pub async fn memory_type_update<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    kind: String,
    title: String,
    description: String,
    icon: String,
) -> CommandResult<Vec<RecordType>> {
    sessions
        .with_session(&app, &project, move |client| {
            client.update_type(&kind, &title, &description, &icon)?;
            client.list_types()
        })
        .await
}

/// What removing a type took with it.
///
/// The count is the answer to the question the confirmation asked, reported
/// back from the write rather than from the count the window showed before it:
/// the two differ if anything was written in between, and the one that happened
/// is the true one.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeRemoval {
    /// The corpus as it now stands.
    pub types: Vec<RecordType>,
    /// How many records of the type were deleted with its definition.
    pub removed: usize,
}

/// Remove a type and every record written as it.
///
/// Both halves or neither: a record whose kind has no definition is one the
/// engine's strict schema will not let anybody read or rewrite, so a definition
/// deleted on its own would strand everything written as it.
#[tauri::command]
pub async fn memory_type_delete<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    kind: String,
) -> CommandResult<TypeRemoval> {
    sessions
        .with_session(&app, &project, move |client| {
            let removed = client.delete_type(&kind)?;
            Ok(TypeRemoval {
                types: client.list_types()?,
                removed,
            })
        })
        .await
}

/// What attaching a folder produced: the corpus's types, and what the first
/// scan made of the files already in it.
///
/// Both halves matter to the window. The type is what the navigator lists; the
/// scan is what turned the documents on disk into records, and its unmatched
/// entries are the one part of attaching that a person has to answer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderAttachment {
    pub types: Vec<RecordType>,
    pub scan: ScanOutcome,
}

/// The type a folder becomes.
///
/// One argument rather than five, because they are one answer: a folder with no
/// type is nothing to write and a type with no folder is a different command.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderAttachmentInput {
    pub kind: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    /// A directory relative to the repository root.
    pub folder: String,
}

/// Attach a folder of the repository as a type of documents.
///
/// `folder` is a directory relative to the repository root, and every file in
/// it is a document of the type — images and PDFs included. There is no mask
/// any more: one hid a person's own files from them, and two types over one
/// folder were never expressible anyway, because a new file matching both masks
/// belongs to both.
///
/// Nothing is written into the folder. That is the engine's promise and the
/// reason the whole arrangement is worth having, so the window states it before
/// running this rather than leaving it to be discovered.
#[tauri::command]
pub async fn memory_folder_attach<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    attachment: FolderAttachmentInput,
) -> CommandResult<FolderAttachment> {
    sessions
        .with_session(&app, &project, move |client| {
            let scan = client.attach_folder(
                &attachment.kind,
                &attachment.title,
                &attachment.description,
                &attachment.icon,
                &attachment.folder,
            )?;
            Ok(FolderAttachment {
                types: client.list_types()?,
                scan,
            })
        })
        .await
}

/// The project's folders, from the records and from the working tree at once.
///
/// `folder` absent asks about the whole project; `""` asks about the root,
/// which is a folder like any other. `subtree` decides whether the region
/// reaches below the folder it names.
///
/// Read live and never cached here: an empty directory is a fact about one
/// working tree, and a remembered list would raise on one machine a folder that
/// does not exist on another.
#[tauri::command]
pub async fn memory_folders<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    folder: Option<String>,
    subtree: Option<bool>,
    kind: Option<String>,
) -> CommandResult<Vec<FolderEntry>> {
    sessions
        .with_session(&app, &project, move |client| {
            client.folders(folder.as_deref(), subtree.unwrap_or(false), kind.as_deref())
        })
        .await
}

/// Make a folder that nothing is in yet, under the type named by `kind`.
///
/// What a folder is differs by where that type keeps its documents, and the
/// engine decides it from the kind — this door does not branch, so the window
/// does not have to either.
#[tauri::command]
pub async fn memory_folder_create<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    folder: String,
    kind: String,
) -> CommandResult<TransactionResult> {
    sessions
        .with_session(&app, &project, move |client| {
            client.create_folder(&folder, &kind)
        })
        .await
}

/// Take a folder and everything filed under it, and say how many went.
///
/// Everything, whatever its type: a folder exists while something is in it, so
/// sparing one type's records would empty it rather than delete it.
#[tauri::command]
pub async fn memory_folder_delete<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    folder: String,
) -> CommandResult<usize> {
    sessions
        .with_session(&app, &project, move |client| client.delete_folder(&folder))
        .await
}

/// How many records a folder holds, at any depth and whatever their type.
///
/// What a confirmation asks before naming a number it is about to destroy.
#[tauri::command]
pub async fn memory_folder_toll<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    folder: String,
) -> CommandResult<usize> {
    sessions
        .with_session(&app, &project, move |client| client.folder_toll(&folder))
        .await
}

/// Rename a folder, moving every record filed under it in one transaction.
///
/// Where the documents are files the directory is renamed too, and the locators
/// follow it. A type's own storage root is refused: moving that is a change to
/// the type rather than a rename.
#[tauri::command]
pub async fn memory_folder_rename<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    from: String,
    to: String,
) -> CommandResult<TransactionResult> {
    sessions
        .with_session(&app, &project, move |client| {
            client.rename_folder(&from, &to)
        })
        .await
}

/// File one record in another folder. `""` is the root.
///
/// Whether a file moves with it is the engine's business, not this door's: a
/// record whose body is a repository file has a folder that *is* that file's
/// directory, and the engine moves both or neither.
#[tauri::command]
pub async fn memory_document_move<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
    folder: String,
) -> CommandResult<TransactionResult> {
    sessions
        .with_session(&app, &project, move |client| {
            client.move_document(&key, &folder)
        })
        .await
}

/// Reconcile every attached folder with the records, and report what moved.
///
/// When to call it is decided in `sync-memory`, which owns the reasoning; this
/// is the door.
#[tauri::command]
pub async fn memory_scan<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<ScanOutcome> {
    sessions
        .with_session(&app, &project, MemoryClient::scan)
        .await
}

/// Settle a file the scan could not attribute to a record.
///
/// `adopt` names the record the file turned out to be; omitting it says the
/// file is a document in its own right. `contentHash` travels from the scan
/// report rather than being recomputed, because nothing between the window and
/// the engine reads the working tree.
#[tauri::command]
pub async fn memory_unmatched_resolve<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    locator: String,
    content_hash: String,
    kind: String,
    adopt: Option<String>,
) -> CommandResult<ScanOutcome> {
    sessions
        .with_session(&app, &project, move |client| {
            client.resolve_unmatched(&locator, &content_hash, &kind, adopt.as_deref())?;
            // The answer is the corpus as it now stands: adopting one file may
            // settle another — the record it was competing with is no longer a
            // candidate — and a window redrawing from the old report would show a
            // question that has stopped being one.
            client.scan()
        })
        .await
}

/// What the Records column shows: counts over the whole corpus, and one page
/// of the selection.
///
/// `selection` is a listing query — `kind`, `freshness`, `limit`, `offset` —
/// and `hidden` names the kinds this window is not showing. The schema records
/// and the hidden kinds are excluded from both halves by the crate.
#[tauri::command]
pub async fn memory_records<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    selection: Value,
    hidden: Vec<String>,
) -> CommandResult<RecordsPage> {
    sessions
        .with_session(&app, &project, move |client| {
            client.records(&selection, &hidden)
        })
        .await
}

/// One record, whole: its Markdown body, its metadata and its product fields.
///
/// `null` when the key does not exist at the current revision.
#[tauri::command]
pub async fn memory_document<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
) -> CommandResult<Option<Document>> {
    sessions
        .with_session(&app, &project, move |client| client.document(&key))
        .await
}

/// Put a file into a type's storage, and answer with the record that names it.
///
/// The one route by which something that is not text reaches the working tree.
/// The bytes arrive as base64 because the protocol is JSON, and they are
/// decoded by the engine rather than here — the window has no filesystem and
/// the engine owns what a locator means.
///
/// The file lands in the **root of the storage**. Where a project keeps its
/// pictures is the project's arrangement, and an application that quietly
/// created an `assets/` folder would be making that arrangement on the team's
/// behalf, in their repository and in their diff.
#[tauri::command]
pub async fn memory_file_create<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    kind: String,
    name: String,
    content: String,
) -> CommandResult<Document> {
    sessions
        .with_session(&app, &project, move |client| {
            client.create_file_document(&kind, &name, &content)
        })
        .await
}

/// The bytes of a record whose content is a file, as the engine reports them.
///
/// Separate from `memory_document` on purpose. A record's *document* is what
/// the window shows and edits, and for anything that is not text there is
/// nothing to show and nothing to edit — carrying a PNG through that command
/// would put megabytes of base64 on every read of every record in a folder.
/// This one is asked for a single file, by whatever is about to draw it.
///
/// The answer says how to read itself: `utf-8`, `base64`, or `none` for a body
/// the engine did not fetch. Branching on that is not optional — a caller that
/// ignores it renders a picture as a page of base64.
#[tauri::command]
pub async fn memory_content<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
) -> CommandResult<ContentView> {
    sessions
        .with_session(&app, &project, move |client| client.read_content(&key))
        .await
}

/// Change what a patch names in one record, and answer with the record as
/// stored.
///
/// `edits` carries only what changed — a title, a body, tags, links, scope or
/// observed paths, the archive flag, product fields — and the command leaves
/// everything it is silent about exactly as the store holds it.
///
/// The answer is a read of what was just written rather than an echo of what was
/// sent: the window shows what the store holds, and the two differ the moment
/// the engine normalises anything. `null` would mean the record left the store
/// between the write and the read, which is an answer rather than a failure.
#[tauri::command]
pub async fn memory_document_update<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
    edits: DocumentEdits,
) -> CommandResult<Option<Document>> {
    sessions
        .with_session(&app, &project, move |client| {
            client.update_document(&key, &edits)?;
            client.document(&key)
        })
        .await
}

/// Create an empty record of one of the project's types, and answer with it.
///
/// The kind decides which fields the record must carry, and the definition the
/// project published decides what those are — so nothing about the shape of a
/// new record is this build's to choose. What travels is the kind and the title
/// somebody is about to type over.
#[tauri::command]
pub async fn memory_document_create<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    kind: String,
    title: String,
    // Where it goes. Absent files it where the type does by default: the root
    // of its storage, or no folder at all for a type whose documents are its
    // records.
    folder: Option<String>,
) -> CommandResult<Document> {
    sessions
        .with_session(&app, &project, move |client| {
            client.create_document(&kind, &title, folder.as_deref())
        })
        .await
}

/// The document that *is* a folder: opened if it exists, written if it does not.
///
/// This is how a folder gets a title and a text of its own. What it produces is
/// an ordinary record of an ordinary type, so its content is indexed and found
/// by search like any other document — nothing in the engine treats it
/// specially, which is exactly why it works.
#[tauri::command]
pub async fn memory_folder_describe<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    folder: String,
    kind: String,
) -> CommandResult<Document> {
    sessions
        .with_session(&app, &project, move |client| {
            client.describe_folder(&folder, &kind)
        })
        .await
}

/// Delete records by key, all of them or none.
///
/// One transaction, because a record deleted together with the ones that depend
/// on it is one decision: half of it applied is a corpus in a state nobody
/// chose.
#[tauri::command]
pub async fn memory_document_delete<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    keys: Vec<String>,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let result = client.delete_documents(&keys)?;
            Ok(serde_json::to_value(result).unwrap_or(Value::Null))
        })
        .await
}

/// What holds on to a record: the records that link to it, and the ones that
/// mention it in prose.
///
/// Two lists rather than one count, because deleting the first kind leaves a
/// link pointing at nothing while deleting the second would remove a sentence
/// about the record — and only the person deciding can weigh those.
#[tauri::command]
pub async fn memory_document_dependents<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
) -> CommandResult<Dependents> {
    sessions
        .with_session(&app, &project, move |client| client.dependents(&key))
        .await
}

/// List records with filters, sorting and paging.
#[tauri::command]
pub async fn memory_list<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    query: Value,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let listing = client.list_records(&query)?;
            Ok(serde_json::to_value(listing).unwrap_or(Value::Null))
        })
        .await
}

/// Search, reporting whether the answer came from FTS alone.
#[tauri::command]
pub async fn memory_search<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    query: Value,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let outcome = client.search(&query)?;
            Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
        })
        .await
}

/// Read one record by key.
#[tauri::command]
pub async fn memory_get<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    key: String,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let view = client.get_record(&key)?;
            Ok(serde_json::to_value(view).unwrap_or(Value::Null))
        })
        .await
}

/// Create or update entities in one transaction.
#[tauri::command]
pub async fn memory_save<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    entities: Vec<EntityInput>,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let result = client.save_entities(&entities)?;
            Ok(serde_json::to_value(result).unwrap_or(Value::Null))
        })
        .await
}

/// Delete entities by key, in one transaction.
///
/// The same door `memory_document_delete` goes through, rather than a raw one
/// beside it. A second delete that skipped the checks — a type definition goes
/// with its type, the project's own record is what the project is opened by —
/// would be a way for the window to do quietly what it refuses to do plainly.
#[tauri::command]
pub async fn memory_delete<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    keys: Vec<String>,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| {
            let result = client.delete_documents(&keys)?;
            Ok(serde_json::to_value(result).unwrap_or(Value::Null))
        })
        .await
}

/// Whether the project's memory is in step with its remote.
///
/// `ask_remote` is what decides whether opening a project waits on a network
/// call. The count of unpublished records is computed locally, so the header
/// has something true to say before anybody has been asked anything.
#[tauri::command]
pub async fn memory_sync_state<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    ask_remote: bool,
) -> CommandResult<SyncState> {
    sessions
        .with_session(&app, &project, move |client| client.sync_state(ask_remote))
        .await
}

/// Whether this repository's memory is here, still on a remote, or nowhere.
///
/// The flow that opens a project asks this before it offers to describe one:
/// an empty corpus is what a fresh clone and a brand-new project have in
/// common, and describing the first of them writes a `project` record that
/// will never merge with the one already on the remote.
#[tauri::command]
pub async fn memory_presence<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<MemoryPresence> {
    sessions
        .with_session(&app, &project, MemoryClient::presence)
        .await
}

/// Configure the memory remote, which is separate from the code `origin`.
#[tauri::command]
pub async fn memory_remote_set<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    url: String,
    refspec: Option<String>,
) -> CommandResult<TransportStatus> {
    sessions
        .with_session(&app, &project, move |client| {
            client.set_remote(&url, refspec.as_deref())
        })
        .await
}

/// Forget the memory remote.
#[tauri::command]
pub async fn memory_remote_remove<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<TransportStatus> {
    sessions
        .with_session(&app, &project, MemoryClient::remove_remote)
        .await
}

/// Fetch memory from the remote and merge it.
#[tauri::command]
pub async fn memory_fetch<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<FetchOutcome> {
    sessions
        .with_session(&app, &project, MemoryClient::fetch)
        .await
}

/// Put memory back where it stood, undoing what has happened since.
///
/// What a fetch is undone with: the revision to name is the
/// `localRevisionBefore` that fetch reported. Backwards along memory's own
/// history and nowhere else — a revision this project never passed through is
/// refused rather than reached.
#[tauri::command]
pub async fn memory_rewind<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    revision: String,
    expected: String,
) -> CommandResult<()> {
    sessions
        .with_session(&app, &project, move |client| {
            client.rewind(&revision, &expected)
        })
        .await
}

/// Push memory to the remote.
#[tauri::command]
pub async fn memory_push<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    force: bool,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| client.push(force))
        .await
}

/// Rebuild the search index.
#[tauri::command]
pub async fn memory_reindex<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, MemoryClient::reindex)
        .await
}

/// Catch memory up with code history, rebuilding after it was rewritten.
///
/// The engine reconciles before every write on its own, so this is reached for
/// one state only: history that was rebased, reset or replaced, which leaves
/// reconciliation on a commit the current history does not descend from and
/// every write refused with `diverged`. Nobody but the person at the window can
/// say that the new history is the real one, which is why `full_rebuild` is
/// asked for rather than assumed — it marks every record unverified, and a
/// claim last checked against a history that is gone has not been checked.
#[tauri::command]
pub async fn memory_reconcile<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    full_rebuild: bool,
) -> CommandResult<Value> {
    sessions
        .with_session(&app, &project, move |client| client.reconcile(full_rebuild))
        .await
}
