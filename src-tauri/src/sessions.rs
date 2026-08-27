//! Talking to agents.
//!
//! This is the other half of the word from [`crate::connect`], and the two are
//! deliberately separate. There, an agent is something that connects **to**
//! Sync: we write our MCP server into its configuration file and it reaches us
//! on its own. Here, an agent is something Sync **drives**: we raise its
//! process, speak ACP down its stdin and read the conversation back. Four of the
//! seven clients listed over there — Claude Desktop, Cursor, VS Code, Zed — can
//! never appear here, because they are applications and editors rather than
//! processes with a protocol on their standard input. Zed in particular is an
//! ACP *client*: it is on our side of the wire, not the other one.
//!
//! # What this layer is for
//!
//! It is the seam an extension talks to agents through, and the reason it
//! exists is that every agent is different and an extension must not have to
//! know how. The differences are absorbed below it, in `acp-client`: the launch
//! table is data, an `initialize` answer says what an agent can do, an update
//! variant nobody models still arrives intact, and one MCP tool spelled four
//! ways resolves to one name. What an extension sees is one shape.
//!
//! # What it deliberately does not do
//!
//! It does not interpret the conversation. Updates cross to the window as the
//! agent wrote them, and are assembled there. A canon in Rust would have to
//! decide what every unknown variant means, which is precisely the decision
//! that goes stale as the agents diverge.

pub mod adapters;
pub mod catalog;
pub mod event;
pub mod live;
pub mod remembered;

use std::path::PathBuf;
use std::sync::Arc;

use acp_client::{AgentProfile, launch, schema};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, Runtime};

use crate::project::{ProjectError, configuration_file};
use adapters::AdapterState;
use catalog::AgentDescriptor;
use event::{PastedImage, SessionEvent, Status};
use live::{Pasted, Session, SessionHandler, Sessions, Source};
use remembered::{Remembered, Store};

/// A session as the window lists it — enough to say what is running and to
/// offer to stop it, without reading the conversation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub key: String,
    pub agent_id: String,
    pub agent_name: String,
    /// What the conversation is called: the first words of it, or whatever
    /// somebody renamed it to. `None` before anything has been said, which is
    /// when the agent's name is the only thing there is to call it.
    pub title: Option<String>,
    pub cwd: String,
    pub status: Status,
    pub opened_at_ms: u64,
    /// Whether the agent said at `initialize` that it reads images. What the
    /// window asks before offering to paste one — offering a gesture the agent
    /// will refuse is worse than saying it has none.
    pub accepts_images: bool,
    /// Who asked for this conversation, when it was not a person.
    ///
    /// The row is where it has to be, because this list is the only place a
    /// session nobody in this window started is visible at all. `null` is a
    /// person, and it is the ordinary answer.
    ///
    /// Read from the session in memory rather than from the file the order was
    /// written to: this list is polled every few seconds by every window, and a
    /// file read per poll would be paid by every conversation to answer for the
    /// few that have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// Everything one session has said so far, read in one go.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBacklog {
    pub events: Vec<SessionEvent>,
    /// How many events fell off the front of the history before this read.
    pub dropped: u64,
}

/// What opening a session answered with.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedSession {
    pub key: String,
    pub agent_name: String,
    /// What the agent said about itself in `initialize`.
    pub agent_version: Option<String>,
    /// The session's configuration as the agent stated it, the model among it.
    /// `None` from an agent that offers no configuration in protocol.
    pub configuration: Option<serde_json::Value>,
    /// The modes the agent said it works in, and the one it is in — its own
    /// `{ currentModeId, availableModes }`. `None` from an agent with no modes.
    pub modes: Option<serde_json::Value>,
}

/// Where this application keeps what it downloaded.
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
    app.path()
        .app_data_dir()
        .map_err(|error| ProjectError::new("agent_adapter", error.to_string()))
}

/// What each adapter is, and whether it is ready to run without a fetch.
///
/// # Errors
///
/// [`ProjectError`] only when the machine cannot say where application data
/// lives.
#[tauri::command(async)]
pub fn agent_adapters<R: Runtime>(app: AppHandle<R>) -> Result<Vec<AdapterState>, ProjectError> {
    Ok(adapters::state(&data_dir(&app)?))
}

/// Downloads everything the agents need, at the versions this build pins.
///
/// Called when the extension that talks to agents is installed, so that the
/// first conversation is not the thing that pays for it. It is not required:
/// a machine that was offline at install still works, one launch more slowly.
///
/// # Errors
///
/// [`ProjectError`] naming what could not be installed and why — most often no
/// `npm` on the machine, or no network.
#[tauri::command(async)]
pub async fn agent_adapters_prepare<R: Runtime>(app: AppHandle<R>) -> Result<(), ProjectError> {
    let dir = data_dir(&app)?;
    for spec in acp_client::registry::ALL {
        adapters::ensure(&dir, spec)
            .map_err(|reason| ProjectError::new("agent_adapter", reason))?;
    }
    Ok(())
}

/// Deletes what was downloaded.
///
/// Called when the extension is removed. Deliberately not guarded by whether
/// another project still wants it: the check before a launch reinstates it, so
/// the worst this can do is make one conversation slow to start.
///
/// # Errors
///
/// [`ProjectError`] when the directory is there and cannot be removed.
#[tauri::command(async)]
pub async fn agent_adapters_forget<R: Runtime>(app: AppHandle<R>) -> Result<(), ProjectError> {
    adapters::forget(&data_dir(&app)?).map_err(|reason| ProjectError::new("agent_adapter", reason))
}

/// Every agent, and whether this machine can raise it.
///
/// # Errors
///
/// None: an agent that is not installed is a row that says so.
#[tauri::command(async)]
pub fn session_catalog() -> Vec<AgentDescriptor> {
    catalog::descriptors()
}

/// Everything running right now, across every extension.
///
/// The window needs this to answer two questions a person will ask about a
/// process it started on their behalf: is it still going, and how do I stop it.
/// Neither is answerable from a screen that has been unmounted, which is
/// exactly when it matters.
///
/// # Errors
///
/// None.
#[tauri::command(async)]
pub fn session_live(sessions: State<'_, Sessions>) -> Vec<SessionRow> {
    let mut rows: Vec<SessionRow> = sessions.all().iter().map(row).collect();
    rows.sort_by_key(|row| row.opened_at_ms);
    rows
}

/// Raises an agent and opens a session in it.
///
/// # Errors
///
/// [`ProjectError`] when the agent is unknown or not installed, when its
/// process will not start, or when it refuses `initialize` or `session/new`.
#[tauri::command(async)]
pub async fn session_open<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    agent_id: String,
    cwd: String,
    model: Option<String>,
) -> Result<OpenedSession, ProjectError> {
    open(
        &app,
        &sessions,
        &agent_id,
        &PathBuf::from(&cwd),
        model,
        None,
    )
    .await
    .map(|(_, opened)| opened)
}

/// Opening a session, for callers that need the session and not only the answer.
///
/// Its own function because there are two of them now and there was one. The
/// window opens a session and wants what to draw; work ordered by a handler
/// opens one and wants the session itself, to say something into
/// ([`crate::work`]). Two paths that each raised an agent would be two places
/// deciding what a failed launch leaves behind, and this is the decision:
/// the session stays in the registry with its reason on it.
async fn open<R: Runtime>(
    app: &AppHandle<R>,
    sessions: &Sessions,
    agent_id: &str,
    cwd: &std::path::Path,
    model: Option<String>,
    // Who asked. `None` is the window, where a person is the answer by
    // construction. It is taken here rather than set on the session afterwards
    // so that a poll landing between the insert and the raise cannot see a row
    // that is briefly nobody's.
    source: Option<Source>,
) -> Result<(Arc<Session>, OpenedSession), ProjectError> {
    let spec = catalog::spec(agent_id)
        .ok_or_else(|| ProjectError::new("agent_unknown", format!("no agent called {agent_id}")))?;
    let program = catalog::resolve(spec.program).ok_or_else(|| {
        ProjectError::new(
            "agent_missing",
            format!("`{}` was not found on this machine", spec.program),
        )
    })?;

    let session = Session::new(
        sessions.mint_key(),
        agent_id.to_owned(),
        spec.display_name.replace('`', ""),
        cwd.to_path_buf(),
        source,
    );
    sessions.insert(Arc::clone(&session));

    match raise(app, spec, program, model, None, &session, cwd).await {
        // No pointer yet, deliberately. A session that has been opened and not
        // spoken in is not a conversation, and an agent does not necessarily
        // keep one: writing a pointer here filled the list with rows that had
        // no name to show and no session left to load. The first thing said is
        // what makes it a conversation, and that is where it is written down.
        Ok(opened) => Ok((session, opened)),
        Err(error) => {
            // A session that failed to open stays in the registry with its
            // reason attached rather than vanishing: the window asked for an
            // agent and is owed an answer about it. It is removed when the
            // screen closes it, like any other.
            session.set_status(Status::Failed, Some(error.message.clone()));
            Err(error)
        }
    }
}

/// Raises an agent for work an extension ordered, with no window involved.
///
/// The same path the window takes, and that is the point rather than a
/// convenience: a session opened at three in the morning is an ordinary session
/// — it is in the registry, it is listed by [`session_live`], it writes the
/// same pointer, and a person who opens a window finds it there. Nothing here
/// is a second kind of conversation.
///
/// No model is passed. A package does not choose one: the agents that take a
/// model on the launch have a pinned default, and the rest state theirs in
/// protocol, where the choice belongs to whoever is looking at the session.
///
/// # Errors
///
/// [`ProjectError`] when the agent is unknown or not installed, when its
/// process will not start, or when it refuses `initialize` or `session/new`.
pub(crate) async fn raise_for_work<R: Runtime>(
    app: &AppHandle<R>,
    agent_id: &str,
    cwd: &std::path::Path,
    source: Source,
) -> Result<Arc<Session>, ProjectError> {
    let sessions = app.state::<Sessions>();
    open(app, &sessions, agent_id, cwd, None, Some(source))
        .await
        .map(|(session, _)| session)
}

/// The part of opening that can fail, so the caller has one place to record why.
async fn raise<R: Runtime>(
    app: &AppHandle<R>,
    spec: &'static acp_client::AgentLaunchSpec,
    program: PathBuf,
    model: Option<String>,
    // The agent's own id for a session it already holds, when this is a
    // conversation being continued rather than a new one.
    resume: Option<String>,
    session: &Arc<Session>,
    cwd: &std::path::Path,
) -> Result<OpenedSession, ProjectError> {
    // An adapter is fetched at install, not here — but "not here" cannot mean
    // "assume it is there": the directory belongs to the machine and the install
    // belonged to one project, so another project may have dropped it. Checking
    // costs a directory read, and being wrong costs the slow launch this whole
    // mechanism exists to avoid rather than a session that will not open.
    let adapter = adapters::ensure(&data_dir(app)?, spec)
        .map_err(|reason| ProjectError::new("agent_adapter", reason))?;

    // The model only rides in on the launch when this agent takes one that way.
    // The others advertise theirs in protocol, and the session's configuration
    // is where that choice is made — see `session_set_model`.
    let options = launch::SpawnOptions {
        // The adapter's own executable when there is one, so nothing asks the
        // npm registry what the package name means on the way to every turn.
        program: Some(adapter.clone().unwrap_or(program)),
        // The row's arguments are how it *fetches*; an adapter already fetched
        // is run with none of them.
        args: adapter.is_some().then(Vec::new),
        cwd: Some(cwd.to_path_buf()),
        // The agent inherits a PATH that can actually find node, git and the
        // rest of what it shells out to. A bundled `.app` has none of that.
        env: vec![("PATH".to_owned(), catalog::search_path())],
        model: model.filter(|_| spec.model_pin.is_some()),
        // Sync answers the agent's permission requests, so the agent is raised
        // with its own approvals left alone rather than turned off. The two have
        // to agree: an agent launched with approvals disabled would show
        // questions about things it can already do.
        full_access: false,
    };

    let handler = SessionHandler::new(session);
    let command = launch::command_for(spec, &options);
    let process = launch::spawn(command, handler)
        .map_err(|error| ProjectError::new("agent_start", error.to_string()))?;
    let connection = Arc::clone(process.connection());
    session.adopt(process, Arc::clone(&connection), spec.tool_naming);

    let initialized = connection
        .initialize(schema::InitializeRequest::new(
            acp_client::SUPPORTED_PROTOCOL_VERSION,
        ))
        .await
        .map_err(|error| failed(session, "agent_initialize", &error))?;
    let profile = AgentProfile::new(initialized);
    session.set_accepts_images(profile.accepts_images());

    // A session the agent already has, or a new one. Loading is the whole of
    // continuing a conversation: the agent replays what was said as ordinary
    // `session/update` notifications and answers when it has finished, so what
    // the window ends up with is the transcript it had, rebuilt by the only
    // thing that still holds it.
    // Two answers, taken together because the protocol gives them together and
    // an agent may state either without the other: the configuration is what
    // can be chosen, the modes are how the agent is being asked to behave.
    // Reading only the first was this build's answer for a while, and it is why
    // Claude Code's Plan mode — which arrives in this very response — had no
    // way of reaching the window at all.
    let (configuration, modes) = match resume {
        Some(previous) => {
            let previous = schema::SessionId::new(previous);
            // Everything that arrives until this call answers is the agent
            // repeating itself, not saying something new. The window needs the
            // difference for the person's own words — see `SessionEvent::Update`.
            session.set_replaying(true);
            let loaded = connection
                .load_session(schema::LoadSessionRequest::new(
                    previous.clone(),
                    cwd.to_path_buf(),
                ))
                .await;
            session.set_replaying(false);
            let loaded = loaded.map_err(|error| failed(session, "agent_session_load", &error))?;
            session.remember_session(previous);
            (
                stated(loaded.config_options.as_ref()),
                stated(loaded.modes.as_ref()),
            )
        }
        None => {
            let opened = connection
                .new_session(schema::NewSessionRequest::new(cwd.to_path_buf()))
                .await
                .map_err(|error| failed(session, "agent_session", &error))?;
            session.remember_session(opened.session_id.clone());
            (
                stated(opened.config_options.as_ref()),
                stated(opened.modes.as_ref()),
            )
        }
    };
    if let Some(options) = configuration.clone() {
        session.state_configuration(options);
    }
    if let Some(state) = modes.clone() {
        session.state_modes(state);
    }
    session.set_status(Status::Ready, None);

    Ok(OpenedSession {
        key: session.key.clone(),
        agent_name: session.agent_name.clone(),
        agent_version: profile.agent_version().map(ToOwned::to_owned),
        configuration,
        modes,
    })
}

/// One of the agent's own answers as the value the window is handed, or `None`
/// where the agent gave none.
///
/// A function rather than the expression written twice: `session/new` and
/// `session/load` answer with the same two members, and a build that read one
/// of them differently in one of the two branches would give a reopened
/// conversation a different set of controls from the one it had.
fn stated<T: Serialize>(held: Option<&T>) -> Option<serde_json::Value> {
    held.and_then(|value| serde_json::to_value(value).ok())
}

/// Watches a session: everything so far, then everything after.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session.
#[tauri::command(async)]
pub fn session_subscribe(
    sessions: State<'_, Sessions>,
    key: String,
    events: Channel<SessionEvent>,
) -> Result<u64, ProjectError> {
    Ok(lookup(&sessions, &key)?.subscribe(events))
}

/// Writes down where a conversation is, so it outlives this run.
///
/// Best effort, deliberately. A pointer that could not be written costs the
/// ability to continue the conversation after a restart; refusing to open the
/// conversation over it would cost the conversation itself, which is the larger
/// of the two by a wide margin.
fn remember<R: Runtime>(app: &AppHandle<R>, session: &Arc<Session>) {
    // Nothing said, nothing to point at. A session opened and abandoned has no
    // name to list it under and is not necessarily kept by the agent either, so
    // a pointer to one is a row that can be neither read nor reopened — which
    // is exactly what filled the list the first time this shipped.
    if !session.spoke() {
        return;
    }
    let Some(acp_session) = session.acp_session() else {
        return;
    };
    let Ok(path) = configuration_file(app, remembered::FILE) else {
        return;
    };
    let project = session.cwd.to_string_lossy().into_owned();
    let mut store = Store::read(&path);
    store.remember(
        &project,
        Remembered {
            acp_session: acp_session.0.to_string(),
            agent_id: session.agent_id.clone(),
            agent_name: session.agent_name.clone(),
            cwd: project.clone(),
            title: session.title(),
            opened_at_ms: session.opened_at_ms,
            last_seen_ms: event::now_ms(),
            source: session.source.clone(),
            record_key: None,
        },
    );
    let _ = store.write(&path);
}

/// The conversations this machine can ask an agent to hand back and is not
/// already running, for one project.
///
/// **Dormant only.** A conversation that is running is already a row of
/// [`session_live`], and the two lists are joined here rather than in the
/// window because this is the only side that can join them: a live row is
/// keyed by this run's own key, a pointer by the agent's session id, and the
/// window is given the second of those for no other purpose.
///
/// What comes back is therefore exactly "the conversations from before this
/// launch", and continuing one is [`session_resume`].
///
/// # Errors
///
/// [`ProjectError`] when the configuration directory cannot be resolved.
#[tauri::command(async)]
pub fn session_remembered<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    project: String,
) -> Result<Vec<Remembered>, ProjectError> {
    let path = configuration_file(&app, remembered::FILE)?;
    let running: std::collections::HashSet<String> = sessions
        .all()
        .iter()
        .filter_map(|session| session.acp_session())
        .map(|id| id.0.to_string())
        .collect();
    Ok(Store::read(&path)
        .of_project(&project)
        .into_iter()
        .filter(|held| !running.contains(&held.acp_session))
        .collect())
}

/// Stops offering a conversation. What the agent holds is untouched.
///
/// The pointer is this machine's note that a conversation can be asked for
/// back, and it can outlive the thing it points at: an agent prunes its own
/// sessions, and a session it has dropped is one no amount of asking will
/// return. Without this, such a row could be neither continued nor removed.
///
/// # Errors
///
/// [`ProjectError`] when the configuration directory cannot be written.
#[tauri::command(async)]
pub fn session_forget_remembered<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    acp_session: String,
) -> Result<(), ProjectError> {
    let path = configuration_file(&app, remembered::FILE)?;
    let mut store = Store::read(&path);
    store.forget(&project, &acp_session);
    store.write(&path)
}

/// Continues a conversation: raises its agent and asks for the session back.
///
/// The agent replays what was said as ordinary updates and answers when it has
/// finished, so the session this opens arrives with its transcript already in
/// it. What comes back is a *new* key in this run — the conversation is the
/// same one, and the pointer that named it is the same pointer.
///
/// # Errors
///
/// [`ProjectError`]: `agent_forgotten` when this machine holds no pointer for
/// the session, `agent_moved` when the directory it was held in is not the one
/// being opened, `agent_missing` when its agent is not installed here, and
/// `agent_session_load` when the agent no longer holds the session — which is
/// the answer that cannot be known before asking, and the caller's cue to
/// continue from a kept transcript instead.
#[tauri::command(async)]
pub async fn session_resume<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    project: String,
    acp_session: String,
) -> Result<OpenedSession, ProjectError> {
    let path = configuration_file(&app, remembered::FILE)?;
    let held = Store::read(&path)
        .get(&project, &acp_session)
        .cloned()
        .ok_or_else(|| {
            ProjectError::new(
                "agent_forgotten",
                "this machine has no record of that conversation".to_owned(),
            )
        })?;

    // The directory is checked rather than trusted. The same repository cloned
    // somewhere else is a different working tree, and an agent asked to resume
    // into it would be answering about files that are not the ones it read.
    if held.cwd != project {
        return Err(ProjectError::new(
            "agent_moved",
            format!(
                "that conversation was held in {}, not in {project}",
                held.cwd
            ),
        ));
    }

    let spec = catalog::spec(&held.agent_id).ok_or_else(|| {
        ProjectError::new(
            "agent_unknown",
            format!("no agent called {}", held.agent_id),
        )
    })?;
    let program = catalog::resolve(spec.program).ok_or_else(|| {
        ProjectError::new(
            "agent_missing",
            format!("`{}` was not found on this machine", spec.program),
        )
    })?;

    let cwd = PathBuf::from(&project);
    let session = Session::new(
        sessions.mint_key(),
        held.agent_id.clone(),
        spec.display_name.replace('`', ""),
        cwd.clone(),
        // Carried by the pointer this resume was read from, which is what makes
        // §6.3 true across a restart: the session that was raised for this
        // conversation held its source in memory, and that memory ended with
        // the process.
        held.source.clone(),
    );
    // The name it already had. Nothing the agent replays carries one, and a
    // conversation coming back under a different title is a different
    // conversation as far as the person reading the list is concerned.
    if let Some(title) = held.title.as_deref() {
        session.set_title(title);
    }
    sessions.insert(Arc::clone(&session));

    match raise(
        &app,
        spec,
        program,
        None,
        Some(held.acp_session.clone()),
        &session,
        &cwd,
    )
    .await
    {
        Ok(opened) => {
            remember(&app, &session);
            Ok(opened)
        }
        Err(error) => {
            session.set_status(Status::Failed, Some(error.message.clone()));
            Err(error)
        }
    }
}

/// Says which record a conversation was kept as, so that the record can be
/// continued later on this machine.
///
/// The link lives here rather than in the record: a record travels with the
/// repository, and an agent's session id means nothing wherever it lands.
///
/// # Errors
///
/// [`ProjectError`] when the configuration directory cannot be written.
#[tauri::command(async)]
pub fn session_kept_as<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    key: String,
    record_key: String,
) -> Result<bool, ProjectError> {
    let Some(session) = sessions.get(&key) else {
        return Ok(false);
    };
    let Some(acp_session) = session.acp_session() else {
        return Ok(false);
    };
    let path = configuration_file(&app, remembered::FILE)?;
    let project = session.cwd.to_string_lossy().into_owned();
    let mut store = Store::read(&path);
    let linked = store.kept_as(&project, &acp_session.0, &record_key);
    store.write(&path)?;
    Ok(linked)
}

/// The pointer for a kept record, when this machine holds one.
///
/// `None` is the ordinary answer and not a failure: the record was written on
/// another machine, or by somebody else, or this machine has since forgotten
/// the conversation. It is what tells the window to offer continuing from the
/// transcript rather than from the agent.
///
/// # Errors
///
/// [`ProjectError`] when the configuration directory cannot be resolved.
#[tauri::command(async)]
pub fn session_for_record<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    record_key: String,
) -> Result<Option<Remembered>, ProjectError> {
    let path = configuration_file(&app, remembered::FILE)?;
    Ok(Store::read(&path)
        .for_record(&project, &record_key)
        .cloned())
}

/// Everything a session has said, read once, without watching it.
///
/// The window subscribes to the conversation it has open and to no other, so
/// this is how a command that acts on a *row* reaches that row's words. The
/// answer carries the same count of dropped events a subscription reports —
/// a transcript that quietly begins in the middle reads as the whole of it.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session.
#[tauri::command(async)]
pub fn session_backlog(
    sessions: State<'_, Sessions>,
    key: String,
) -> Result<SessionBacklog, ProjectError> {
    let (events, dropped) = lookup(&sessions, &key)?.backlog();
    Ok(SessionBacklog { events, dropped })
}

/// Stops watching. The session goes on running.
///
/// # Errors
///
/// None: unsubscribing from a session that has already gone is not a failure.
#[tauri::command(async)]
pub fn session_unsubscribe(sessions: State<'_, Sessions>, key: String) {
    if let Some(session) = sessions.get(&key) {
        session.unsubscribe();
    }
}

/// Runs one turn.
///
/// Returns as soon as the prompt is on its way, not when the turn ends: a turn
/// is the agent working and may take tens of minutes, and its output arrives on
/// the subscription meanwhile. What ends it is a status event.
///
/// `attachments` are absolute paths to files, and they cross the wire as
/// **resource links** rather than as bytes. That is the one thing every agent
/// must accept — the protocol requires resource-link support in prompts, while
/// image content is a capability most do not advertise — and it is also the
/// only shape this application can honestly send: Sync never read the file. It
/// names one, and the agent, which is already running in the project's own
/// folder, opens it itself.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, or the session is not open.
#[tauri::command(async)]
pub async fn session_prompt<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    key: String,
    text: String,
    attachments: Vec<String>,
    images: Vec<PastedContent>,
) -> Result<(), ProjectError> {
    let session = lookup(&sessions, &key)?;
    // Asked before anything is kept, and asked again by `send` when the turn is
    // actually built. A closed session refused here costs nothing; refused only
    // at the end, it would already have spent this conversation's image budget
    // on a message that was never sent.
    ready(&session)?;
    if !images.is_empty() && !session.accepts_images() {
        return Err(ProjectError::new(
            "agent_no_images",
            format!("{} does not accept images in a prompt", session.agent_name),
        ));
    }

    // Decoded before anything is kept or sent, so a message with one unreadable
    // picture in it fails without leaving the other two behind.
    let mut decoded = Vec::with_capacity(images.len());
    for image in images {
        let bytes = BASE64
            .decode(image.data.as_bytes())
            .map_err(|_| ProjectError::new("image_unreadable", "that image could not be read"))?;
        decoded.push(Pasted {
            name: image.name,
            mime_type: image.mime_type,
            bytes,
        });
    }

    // Re-encoded from the bytes rather than passed through as it arrived, so
    // what the agent is sent and what this session holds are one thing spelled
    // once. The string that came in was only ever evidence of the bytes.
    let sent: Vec<schema::ImageContent> = decoded
        .iter()
        .map(|image| {
            schema::ImageContent::new(BASE64.encode(&image.bytes), image.mime_type.clone())
        })
        .collect();
    let described: Vec<(String, String, u64)> = decoded
        .iter()
        .map(|image| {
            (
                image.name.clone(),
                image.mime_type.clone(),
                image.bytes.len() as u64,
            )
        })
        .collect();

    let ids = session.keep_pasted(decoded).map_err(|held| {
        // Two different sentences, because they are two different problems. A
        // conversation that is already full is one a person can act on by
        // starting another; one picture too large for an empty conversation is
        // not, and telling them it is "already holding 0 MB" would be nonsense
        // dressed as an explanation.
        let message = if held == 0 {
            format!(
                "that is larger than the {} of images one conversation may hold",
                megabytes(live::PASTED_LIMIT_BYTES)
            )
        } else {
            format!(
                "this conversation is already holding {} of pasted images, which is all it may hold",
                megabytes(held)
            )
        };
        ProjectError::new("images_too_large", message)
    })?;
    let kept: Vec<PastedImage> = ids
        .into_iter()
        .zip(described)
        .map(|(id, (name, mime_type, bytes))| PastedImage {
            id,
            name,
            mime_type,
            bytes,
        })
        .collect();

    send(
        &app,
        &session,
        Turn {
            text,
            attachments,
            sent,
            kept,
        },
    )
}

/// One turn, in the two spellings its images need.
///
/// `sent` is what crosses to the agent and `kept` is what the transcript holds,
/// and they are the same pictures twice because the protocol wants base64 and a
/// session that held base64 would be holding a third more than the picture
/// weighs. A turn with no images has neither, which is every turn work orders:
/// a handler has no clipboard.
#[derive(Debug, Default)]
pub(crate) struct Turn {
    pub text: String,
    pub attachments: Vec<String>,
    pub sent: Vec<schema::ImageContent>,
    pub kept: Vec<PastedImage>,
}

/// Says one turn into an open session and answers as soon as it is on its way.
///
/// The one place a turn is built, for the window's prompt and for work an
/// extension ordered alike. What differs between them is what is in the turn;
/// what must not differ is any of this — that the question is recorded before
/// it is sent, that saying something is what writes the conversation's pointer,
/// and that the turn itself runs on its own task because it may take tens of
/// minutes and every result of it arrives as an event.
///
/// # Errors
///
/// [`ProjectError`] when the session is not open.
pub(crate) fn send<R: Runtime>(
    app: &AppHandle<R>,
    session: &Arc<Session>,
    turn: Turn,
) -> Result<(), ProjectError> {
    let (connection, acp_session) = ready(session)?;
    // Recorded before it is sent, so the transcript holds it whether or not the
    // agent ever answers — and so a screen that comes back to this session is
    // handed the question along with the answer.
    session.record_prompt(turn.text.clone(), turn.attachments.clone(), turn.kept);
    // The first thing said is what names the conversation, so the pointer
    // written when the session opened is holding `null` until now. Rewritten
    // here rather than only at open: a conversation the list could only call
    // "Untitled" is one nobody can pick out of it after a restart.
    remember(app, session);
    session.set_status(Status::Working, None);
    let request =
        schema::PromptRequest::new(acp_session, blocks(turn.text, &turn.attachments, turn.sent));

    // The turn runs on its own task so this command can answer now. Nothing is
    // lost by that: every result of the turn is an event on the subscription.
    let watching = Arc::clone(session);
    tauri::async_runtime::spawn(async move {
        match connection.prompt(request).await {
            Ok(response) => {
                let reason = serde_json::to_value(response.stop_reason)
                    .ok()
                    .and_then(|value| value.as_str().map(ToOwned::to_owned));
                watching.set_status(Status::Ready, reason);
            }
            Err(error) => {
                // The agent's stderr is the only account of a process that died
                // without saying anything in protocol.
                let mut detail = error.to_string();
                let tail = watching.recent_stderr().await;
                if !tail.is_empty() {
                    detail.push_str(&format!(" — {}", tail.join(" ")));
                }
                watching.set_status(Status::Failed, Some(detail));
            }
        }
    });
    Ok(())
}

/// Renames a conversation.
///
/// Nothing keys on this and no record is written from it, so a name here is
/// free in the way a type's name is free and its identifier is not: it is what
/// the row says and what is offered as the title when the conversation is kept.
/// An empty name clears the one there is, and the next thing said derives
/// another.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session.
#[tauri::command(async)]
pub fn session_rename<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    key: String,
    title: String,
) -> Result<(), ProjectError> {
    let session = lookup(&sessions, &key)?;
    session.set_title(&title);
    // The name is what the conversation is picked out of a list by, so the
    // pointer that outlives this run has to carry the new one.
    remember(&app, &session);
    Ok(())
}

/// One pasted image, for the window to draw.
///
/// Answered as base64 rather than pushed on the subscription, because a
/// history is replayed whole to every screen that comes back to a conversation
/// and a picture in it would be paid for on every one of them. Asked for once,
/// when something is about to draw it.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, or the session no longer
/// holds that image — which is what every id becomes once the conversation is
/// deleted.
#[tauri::command(async)]
pub fn session_image(
    sessions: State<'_, Sessions>,
    key: String,
    id: String,
) -> Result<PastedView, ProjectError> {
    let image = lookup(&sessions, &key)?
        .pasted(&id)
        .ok_or_else(|| ProjectError::new("image_unknown", "that image is no longer held"))?;
    Ok(PastedView {
        mime_type: image.mime_type,
        data: BASE64.encode(&image.bytes),
    })
}

/// A pasted image on its way back out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedView {
    pub mime_type: String,
    /// Base64, without a `data:` prefix. What wraps it is the window's own
    /// business, and a prefix here would be this layer deciding it is going
    /// into an `img`.
    pub data: String,
}

/// Asks the agent to stop the turn it is running.
///
/// A notification, so it is acknowledged by the turn ending with `cancelled`
/// rather than by an answer to this call.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, or the session is not open.
#[tauri::command(async)]
pub fn session_cancel(sessions: State<'_, Sessions>, key: String) -> Result<(), ProjectError> {
    let session = lookup(&sessions, &key)?;
    let (connection, acp_session) = ready(&session)?;
    connection
        .cancel(&schema::CancelNotification::new(acp_session))
        .map_err(|error| ProjectError::new("agent_cancel", error.to_string()))
}

/// Answers a permission question. `option_id` of `None` withdraws it, which the
/// agent hears as the turn being cancelled.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, or the question is not open —
/// which is what a second answer to the same question is.
#[tauri::command(async)]
pub fn session_permission_respond(
    sessions: State<'_, Sessions>,
    key: String,
    request_id: u64,
    option_id: Option<String>,
) -> Result<(), ProjectError> {
    let session = lookup(&sessions, &key)?;
    let chosen = option_id.map(schema::PermissionOptionId::from);
    if session.answer(request_id, chosen) {
        Ok(())
    } else {
        Err(ProjectError::new(
            "permission_settled",
            "that question is no longer open",
        ))
    }
}

/// Chooses one of the options the session advertised — the model among them.
///
/// This is the mechanism every agent that offers a choice in protocol uses, and
/// it is the only one that is the same across agents. The others take a model
/// when they are raised and cannot change it afterwards; their sessions carry no
/// configuration, and the window offers no picker rather than a broken one.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, the session is not open, or
/// the agent refuses the value.
#[tauri::command(async)]
pub async fn session_set_option(
    sessions: State<'_, Sessions>,
    key: String,
    config_id: String,
    value_id: String,
) -> Result<serde_json::Value, ProjectError> {
    let session = lookup(&sessions, &key)?;
    let (connection, acp_session) = ready(&session)?;
    let restated = connection
        .set_config_option(schema::SetSessionConfigOptionRequest::new(
            acp_session,
            config_id,
            schema::SessionConfigOptionValue::value_id(value_id),
        ))
        .await
        .map_err(|error| ProjectError::new("agent_option", error.to_string()))?;

    // The answer is the whole option set again rather than an acknowledgement:
    // choosing one option may change what the others offer, so it replaces.
    let options = serde_json::to_value(&restated.config_options)
        .map_err(|error| ProjectError::new("agent_option", error.to_string()))?;
    session.state_configuration(options.clone());
    Ok(options)
}

/// Puts the session into one of the modes the agent advertised.
///
/// The other half of what an agent states when a session opens, and the half
/// this build had no way of acting on. For Claude Code it is Plan, Accept Edits
/// and Default — the choice a person makes several times an hour and the one
/// they came to the window to make.
///
/// The mode state is restated here rather than left to arrive. `set_mode`
/// answers with nothing at all — unlike `set_config_option`, which hands back
/// the whole option set — and an agent that then sends no `current_mode_update`
/// would leave the control showing the mode that is no longer current. Saying
/// it locally is safe because the call has already succeeded: what is written
/// down is what the agent has agreed to.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, when its agent is not up, or
/// when the agent refuses the mode.
#[tauri::command(async)]
pub async fn session_set_mode(
    sessions: State<'_, Sessions>,
    key: String,
    mode_id: String,
) -> Result<serde_json::Value, ProjectError> {
    let session = lookup(&sessions, &key)?;
    let (connection, acp_session) = ready(&session)?;
    connection
        .set_session_mode(schema::SetSessionModeRequest::new(
            acp_session,
            mode_id.clone(),
        ))
        .await
        .map_err(|error| ProjectError::new("agent_mode", error.to_string()))?;

    // The list the agent gave is kept as it gave it and only the current id
    // moves. Inventing a list here — or dropping the one held because the
    // answer carried none — would replace the agent's own account of what it
    // can do with this build's guess at it.
    let mut state = session
        .modes()
        .unwrap_or_else(|| serde_json::json!({ "availableModes": [] }));
    if let Some(object) = state.as_object_mut() {
        object.insert(
            "currentModeId".to_owned(),
            serde_json::Value::String(mode_id),
        );
    }
    session.state_modes(state.clone());
    Ok(state)
}

/// Stops a session's agent, and keeps the conversation.
///
/// Stopping and deleting are two commands because they are two intentions. An
/// agent is a process that is spending a person's money, and ending it is
/// urgent; what it said is a thing they may still be reading, and taking that
/// away as a side effect of stopping the process would be the application
/// deciding they were finished with it. The row stays, marked ended, until
/// [`session_forget`].
///
/// # Errors
///
/// Never in practice: stopping something already stopped is what was asked for.
/// The result is there because an async command holding a borrow of the
/// application's state has to have one.
#[tauri::command(async)]
pub async fn session_close(sessions: State<'_, Sessions>, key: String) -> Result<(), ProjectError> {
    if let Some(session) = sessions.get(&key) {
        session.end(None).await;
    }
    Ok(())
}

/// Deletes a conversation, stopping its agent first if it is still running.
///
/// Nothing is written anywhere, so this is the whole of it: a conversation lives
/// for as long as the application does and no longer. That is a decision rather
/// than an omission — a transcript is not a claim about the project, and filling
/// its memory with unreviewed ones would make the corpus worth less.
///
/// # Errors
///
/// Never in practice, for the same reason as [`session_close`].
#[tauri::command(async)]
pub async fn session_forget(
    sessions: State<'_, Sessions>,
    key: String,
) -> Result<(), ProjectError> {
    if let Some(session) = sessions.remove(&key) {
        session.end(None).await;
    }
    Ok(())
}

/// Ends every session. Called when the application is going away, so that no
/// agent outlives the window that raised it.
pub async fn close_all(sessions: &Sessions) {
    for session in sessions.all() {
        sessions.remove(&session.key);
        session.end(Some("Sync closed".to_owned())).await;
    }
}

fn row(session: &Arc<Session>) -> SessionRow {
    SessionRow {
        key: session.key.clone(),
        agent_id: session.agent_id.clone(),
        agent_name: session.agent_name.clone(),
        title: session.title(),
        cwd: session.cwd.to_string_lossy().into_owned(),
        status: session.status(),
        opened_at_ms: session.opened_at_ms,
        accepts_images: session.accepts_images(),
        source: session.source.clone(),
    }
}

/// What one turn is made of: what was said, and what was attached to it.
///
/// The text leads, because it is the thing being asked; the attachments follow
/// it, as the material it is about. A turn with nothing typed carries no text
/// block at all rather than an empty one — a prompt whose first block is an
/// empty string is a prompt that says nothing, and dropping it is not the same
/// as saying nothing.
fn blocks(
    text: String,
    attachments: &[String],
    images: Vec<schema::ImageContent>,
) -> Vec<schema::ContentBlock> {
    let mut blocks = Vec::with_capacity(attachments.len() + images.len() + 1);
    if !text.trim().is_empty() {
        blocks.push(schema::ContentBlock::Text(schema::TextContent::new(text)));
    }
    for path in attachments {
        blocks.push(schema::ContentBlock::ResourceLink(resource_link(path)));
    }
    for image in images {
        blocks.push(schema::ContentBlock::Image(image));
    }
    blocks
}

/// A pasted image on its way in, as the window spells it.
///
/// `data` is base64 and stays base64 all the way to the agent: that is the
/// protocol's own spelling of an image, so decoding it here would be work done
/// only to encode it again. The bytes are decoded once, to be kept — a session
/// holding base64 would be holding a third more than the picture weighs, and
/// could not answer how big it is without arithmetic.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedContent {
    pub name: String,
    pub mime_type: String,
    /// The image itself, base64, without a `data:` prefix.
    pub data: String,
}

/// A size in a sentence a person reads.
fn megabytes(bytes: usize) -> String {
    format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// One attached file, as the protocol's own reference to something readable.
///
/// The name is the file's own, because that is what an agent will echo back and
/// what a person will recognise; the URI is the absolute path as a `file:` URL,
/// because the agent is a separate process and a relative path would be
/// resolved against its own directory rather than against this window's idea of
/// where the file was.
fn resource_link(path: &str) -> schema::ResourceLink {
    let file = std::path::Path::new(path);
    let name = file.file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut link = schema::ResourceLink::new(name, file_uri(path));
    if let Some(mime) = mime_type(file) {
        link = link.mime_type(mime.to_owned());
    }
    link
}

/// An absolute path as a `file:` URL.
///
/// Percent-encoded by hand rather than by a crate, because the whole of the
/// job is the characters a path may hold that a URL may not, and this is the
/// only place in the application that needs it. A space is the one that turns
/// up daily; the rest are here so that the daily one is not a special case.
fn file_uri(path: &str) -> String {
    const UNRESERVED: &str = "-._~!$&'()*+,;=:@";
    let mut uri = String::from("file://");
    for byte in path.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || UNRESERVED.contains(ch) || ch == '/' {
            uri.push(ch);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// What kind of file it is, from its extension.
///
/// Stated when it is one of the kinds a person attaches on purpose, and left
/// unsaid otherwise: `mimeType` is optional in the protocol, and an agent that
/// is told `application/octet-stream` has been told something less true than
/// nothing. The extension is the only evidence available — this application
/// does not open the file.
fn mime_type(path: &std::path::Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

fn lookup(sessions: &State<'_, Sessions>, key: &str) -> Result<Arc<Session>, ProjectError> {
    sessions
        .get(key)
        .ok_or_else(|| ProjectError::new("session_unknown", format!("no session called {key}")))
}

/// The two things every call on an open session needs, or the reason there are
/// none.
fn ready(
    session: &Arc<Session>,
) -> Result<(Arc<acp_client::AgentConnection>, schema::SessionId), ProjectError> {
    match (session.connection(), session.acp_session()) {
        (Some(connection), Some(id)) => Ok((connection, id)),
        _ => Err(ProjectError::new(
            "session_closed",
            "that session is not open",
        )),
    }
}

/// Records a failure on the session and states it as the window's error, so the
/// screen watching the session and the call that asked for it hear the same
/// sentence rather than two accounts of one event.
fn failed(session: &Arc<Session>, kind: &str, error: &acp_client::Error) -> ProjectError {
    let message = error.to_string();
    session.set_status(Status::Failed, Some(message.clone()));
    ProjectError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field the window does not read is a field that is not there.
    ///
    /// Nothing on this boundary fails loudly: `serde` renames on one side and
    /// TypeScript describes the other, and neither knows about the first, so a
    /// field spelled differently arrives as `undefined` and the row simply says
    /// less than it used to. This is the assertion that says which spelling the
    /// window was written against.
    #[test]
    fn a_row_crosses_under_the_names_the_window_reads() {
        let row = SessionRow {
            key: "s0".to_owned(),
            agent_id: "opencode".to_owned(),
            agent_name: "OpenCode".to_owned(),
            title: Some("Why does reconcile run twice?".to_owned()),
            cwd: "/tmp/project".to_owned(),
            status: Status::Working,
            opened_at_ms: 1234,
            accepts_images: true,
            source: None,
        };

        assert_eq!(
            serde_json::to_string(&row).expect("a row serialises"),
            r#"{"key":"s0","agentId":"opencode","agentName":"OpenCode","title":"Why does reconcile run twice?","cwd":"/tmp/project","status":"working","openedAtMs":1234,"acceptsImages":true}"#,
            "a conversation a person started says nothing about a source, rather than saying null"
        );
    }

    /// The same boundary, for the field that is the whole of step 5.
    ///
    /// It is asserted separately because the two answers are different shapes:
    /// a person's conversation carries no `source` member at all, and one an
    /// extension ordered carries the object the window filters and draws by. A
    /// row that spelled either of them differently would arrive as `undefined`
    /// and Chat would quietly go back to saying every conversation was
    /// somebody's.
    #[test]
    fn a_row_says_who_ordered_it_when_it_was_not_a_person() {
        let row = SessionRow {
            key: "s1".to_owned(),
            agent_id: "claude".to_owned(),
            agent_name: "Claude Code".to_owned(),
            title: None,
            cwd: "/tmp/project".to_owned(),
            status: Status::Working,
            opened_at_ms: 1234,
            accepts_images: true,
            source: Some(Source {
                work: "w1-0".to_owned(),
                extension_id: "issues".to_owned(),
                extension_name: "Issues".to_owned(),
                handler: "issues.poll".to_owned(),
                about: Some("issue-4c1a".to_owned()),
            }),
        };

        let json = serde_json::to_value(&row).expect("a row serialises");
        assert_eq!(json["source"]["work"], "w1-0");
        assert_eq!(json["source"]["extensionId"], "issues");
        assert_eq!(
            json["source"]["extensionName"], "Issues",
            "the name a heading is drawn from, so no catalogue is needed to draw one"
        );
        assert_eq!(json["source"]["handler"], "issues.poll");
        assert_eq!(json["source"]["about"], "issue-4c1a");
    }

    #[test]
    fn a_turn_carries_what_was_said_and_then_what_was_attached() {
        let built = blocks(
            "What is wrong with this?".to_owned(),
            &["/tmp/a shot.png".to_owned()],
            Vec::new(),
        );
        assert_eq!(built.len(), 2, "the text, then the file");

        let json = serde_json::to_value(&built).expect("the blocks serialise");
        assert_eq!(json[0]["type"], "text");
        assert_eq!(json[0]["text"], "What is wrong with this?");
        assert_eq!(json[1]["type"], "resource_link");
        assert_eq!(json[1]["name"], "a shot.png");
        assert_eq!(
            json[1]["uri"], "file:///tmp/a%20shot.png",
            "a space in a path is not a space in a URL",
        );
        assert_eq!(json[1]["mimeType"], "image/png");
    }

    #[test]
    fn a_turn_with_nothing_typed_is_the_attachment_alone() {
        let built = blocks("   ".to_owned(), &["/tmp/shot.png".to_owned()], Vec::new());
        assert_eq!(built.len(), 1, "an empty text block says nothing twice");

        let json = serde_json::to_value(&built).expect("the blocks serialise");
        assert_eq!(json[0]["type"], "resource_link");
    }

    #[test]
    fn a_file_of_a_kind_nobody_can_name_is_sent_without_a_kind() {
        let json = serde_json::to_value(blocks(
            "look".to_owned(),
            &["/tmp/notes.unknownext".to_owned()],
            Vec::new(),
        ))
        .expect("the blocks serialise");

        assert!(
            json[1].get("mimeType").is_none(),
            "guessing is worse than saying nothing: {json}",
        );
    }

    #[test]
    fn an_image_crosses_back_under_the_names_the_window_reads() {
        let view = PastedView {
            mime_type: "image/png".to_owned(),
            data: "aGVsbG8=".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&view).expect("a view serialises"),
            r#"{"mimeType":"image/png","data":"aGVsbG8="}"#
        );
    }

    #[test]
    fn a_conversation_nobody_has_spoken_in_carries_a_null_title() {
        let row = SessionRow {
            key: "s0".to_owned(),
            agent_id: "opencode".to_owned(),
            agent_name: "OpenCode".to_owned(),
            title: None,
            cwd: "/tmp/project".to_owned(),
            status: Status::Ready,
            opened_at_ms: 0,
            accepts_images: false,
            source: None,
        };

        // Null rather than absent: the window distinguishes "not named" from
        // "named", and an omitted key would make the two read alike.
        assert!(
            serde_json::to_string(&row)
                .expect("a row serialises")
                .contains(r#""title":null"#)
        );
    }
}
