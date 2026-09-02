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
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, Runtime};

use crate::project::{ProjectError, configuration_file};
use adapters::AdapterState;
use catalog::AgentDescriptor;
use event::{PastedImage, SessionEvent, Status};
use live::{About, HeldImage, Place, Session, SessionHandler, Sessions, Source};
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
    /// The project this conversation belongs to.
    ///
    /// Beside [`Self::cwd`] rather than instead of it, because they answer two
    /// questions: this is whose conversation it is, and `cwd` is where the
    /// agent is working. They differ exactly when the work is being done in a
    /// disposable tree — and a screen that filtered its own conversations by
    /// `cwd` would lose every one of those the moment it was made, which is
    /// what it did before this field existed.
    pub project: String,
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
    /// The record this conversation is being held under, when there is one.
    ///
    /// The field a list groups by, and the reason it is here rather than inside
    /// [`Self::source`]: a conversation somebody opened from a task has no
    /// orderer and is still about that task. Grouping by who asked leaves every
    /// one of those in the same undifferentiated heap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<About>,
    /// The disposable tree this conversation is being held in, when it is not
    /// being held in the project's own.
    ///
    /// On the row because this list is the only place an unattended
    /// conversation is visible at all, and because the two gestures a tree
    /// offers — name the work, throw it away — need the path it is at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<crate::worktree::Worktree>,
    /// The agent's own id for this session, once the agent has given one.
    ///
    /// Here so that a live row and a pointer can be spoken about in one
    /// vocabulary. A pointer has always been addressed by this id, a live row
    /// by this run's key, and the two were never comparable — which was fine
    /// while the only question was *is this one running*, answered on this side
    /// in [`session_remembered`]. It stops being fine the moment a conversation
    /// names another one: the parent of a live child may itself be live, and a
    /// window given two incomparable identities cannot say which row it is.
    ///
    /// `None` until the agent answers `session/new`, and on a session whose
    /// agent never rose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_session: Option<String>,
    /// The conversation this one was delegated from, by the agent's id for it.
    ///
    /// Read against [`Self::acp_session`] of the other rows, whichever half of
    /// the list they came from. A parent nothing in the list names is drawn as
    /// no parent at all rather than as a row with something missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
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
    // The project. Named `cwd` because that is what it has always been called
    // on this call and renaming it would move a surface for no gain — where the
    // agent is actually raised is answered below.
    cwd: String,
    model: Option<String>,
    // Where to work: the project itself when this is absent, a fresh tree, or
    // one that is already there. A person's choice and nothing else's —
    // `docs/background.md` §9 does not promise a sandbox, so a tree buys the
    // right to throw the work away rather than any kind of safety.
    worktree: Option<crate::worktree::Choice>,
    // What this conversation stands under: a record, another conversation, both
    // or neither.
    under: Under,
) -> Result<OpenedSession, ProjectError> {
    let project = PathBuf::from(&cwd);
    let made = match worktree {
        Some(choice) => Some(crate::worktree::resolve(
            &app,
            &project,
            &sessions.mint_key(),
            choice,
        )?),
        None => None,
    };
    let place = Place {
        project: project.clone(),
        worktree: made.clone(),
    };
    let opened = open(&app, &sessions, &agent_id, place, model, None, under).await;

    // A tree made for a conversation that never opened is a directory nobody
    // will ever look at: the agent did not start, so nothing was written in it
    // and nothing is lost by removing it. Quiet, and only what this call made —
    // a tree that was chosen rather than created belongs to whoever made it.
    if opened.is_err()
        && let Some(tree) = made
    {
        let _ =
            crate::worktree::worktree_discard(project.to_string_lossy().into_owned(), tree.path);
    }

    opened.map(|(_, opened)| opened)
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
    place: Place,
    model: Option<String>,
    // Who asked. `None` is the window, where a person is the answer by
    // construction. It is taken here rather than set on the session afterwards
    // so that a poll landing between the insert and the raise cannot see a row
    // that is briefly nobody's.
    source: Option<Source>,
    // What this one stands under, set with the same timing and for the same
    // reason: a row that appeared in the list before it knew where it belonged
    // would be a row that changes group under somebody reading it.
    under: Under,
) -> Result<(Arc<Session>, OpenedSession), ProjectError> {
    let spec = catalog::spec(agent_id)
        .ok_or_else(|| ProjectError::new("agent_unknown", format!("no agent called {agent_id}")))?;
    // Where a delegated conversation's heading comes from. Both facts are the
    // parent's, and taking them here rather than from the caller is what makes
    // them unforgeable: whoever asked for this session — a window, a handler,
    // an agent through a package's tool — cannot put a conversation under a
    // record it does not belong to, because it is not asked.
    let Under { about, parent } = under;
    let (source, about) = match parent.as_deref() {
        None => (source, about),
        Some(parent) => descent(app, sessions, &place.project, parent)?,
    };
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
        place,
        source,
        about,
        parent,
    );
    // Both entrances pass through here — a window sending a command block and a
    // package ordering on an agent's behalf — so this is where the two are made
    // the same thing. Anything that read the debt off *how* the conversation
    // was opened would be a rule that held for one of them.
    if session.parent.is_some() {
        session.owe();
    }
    sessions.insert(Arc::clone(&session));

    let cwd = session.cwd.clone();
    match raise(app, spec, program, model, None, &session, &cwd).await {
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

/// What a conversation stands under, as whoever opens one may state it.
///
/// One value rather than two arguments travelling together, which is the
/// bargain [`Place`] already makes for the other pair: these are two answers to
/// one question — where does this conversation belong — set at the same moment,
/// never edited afterwards, and read together by everything that draws the
/// list.
///
/// **Who ordered it is deliberately not here.** A source is composed by the
/// host out of an order it wrote down, and a shape arriving from the webview
/// carrying one would be a window able to sign its work as a package's.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Under {
    /// The record this conversation is being opened under, when a screen opened
    /// it from one. Only the caller can answer it: a person pressing `Send to
    /// agent` on a task is standing in the task, and nothing below that line
    /// can find it out afterwards.
    #[serde(default)]
    pub about: Option<About>,
    /// The conversation this one is being delegated from, by the agent's own id
    /// for it.
    ///
    /// Where it is given, [`Self::about`] is ignored and the parent's is used:
    /// a delegated conversation is filed where its parent is, so a caller
    /// cannot file work under a record it has nothing to do with.
    #[serde(default)]
    pub parent: Option<String>,
}

/// How deep a chain of delegated conversations may go.
///
/// Two, counted as conversations rather than as delegations: somebody's own
/// conversation may delegate, and what it delegated may not. The number is
/// small on purpose and it is not a technical limit — nothing breaks at three.
/// It is the limit on what a person can still follow in a column they read
/// down, and on what an unattended chain can spend before anybody sees it.
const DEPTH: usize = 2;

/// What a delegated conversation inherits from the one it came out of.
///
/// Answers the parent's `source` and `about`, and refuses when the parent is
/// itself delegated — that being the third level. Both facts are read from the
/// parent rather than taken from whoever asked, so a caller cannot file work
/// under a record it has nothing to do with, and an agent — which reaches this
/// through a package's tool — cannot name one at all.
///
/// The live registry first and the pointers after it, because the ordinary case
/// is an agent delegating mid-turn and the other one is real: a conversation
/// continued from a pointer after a restart is delegated from a session this
/// run did raise, but one whose parent has since been stopped is not.
fn descent<R: Runtime>(
    app: &AppHandle<R>,
    sessions: &Sessions,
    project: &std::path::Path,
    parent: &str,
) -> Result<(Option<Source>, Option<About>), ProjectError> {
    if let Some(session) = sessions.all().into_iter().find(|session| {
        session
            .acp_session()
            .is_some_and(|id| id.0.to_string() == parent)
    }) {
        return inherited(
            session.parent.is_some(),
            session.source.clone(),
            session.about.clone(),
        );
    }

    let path = configuration_file(app, remembered::FILE)?;
    let store = Store::read(&path);
    let held = store
        .get(&project.to_string_lossy(), parent)
        .ok_or_else(|| {
            ProjectError::new(
                "conversation_unknown",
                "this project holds no conversation with that id, so there is nothing to \
                 delegate from",
            )
        })?;
    inherited(
        held.parent.is_some(),
        held.source.clone(),
        held.about.clone(),
    )
}

/// What a delegated conversation takes from the one above it, given what that
/// one is.
///
/// Apart from [`descent`] because finding the parent needs the application and
/// deciding what descends from it does not, and this half is the half with a
/// rule in it.
fn inherited(
    parent_was_itself_delegated: bool,
    source: Option<Source>,
    about: Option<About>,
) -> Result<(Option<Source>, Option<About>), ProjectError> {
    if parent_was_itself_delegated {
        return Err(ProjectError::new(
            "conversation_depth",
            format!(
                "that conversation was itself delegated, and a chain of them is {DEPTH} deep: \
                 delegate from the conversation it came out of, or start one of your own"
            ),
        ));
    }
    Ok((source, about))
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
    // The record the order named, and the conversation it was delegated from
    // when a package ordered on an agent's behalf rather than on a clock's.
    under: Under,
) -> Result<Arc<Session>, ProjectError> {
    let sessions = app.state::<Sessions>();
    // In the project's own tree. An order does not choose where it is
    // performed — `docs/background.md` §6.2 — and giving ordered work a
    // disposable tree is a decision about unattended work, taken separately
    // from making one available at all.
    open(
        app,
        &sessions,
        agent_id,
        Place::project(cwd.to_path_buf()),
        None,
        Some(source),
        under,
    )
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
    let project = session.project.to_string_lossy().into_owned();
    let mut store = Store::read(&path);
    store.remember(
        &project,
        Remembered {
            acp_session: acp_session.0.to_string(),
            agent_id: session.agent_id.clone(),
            agent_name: session.agent_name.clone(),
            cwd: project.clone(),
            worktree: session.worktree.clone(),
            title: session.title(),
            opened_at_ms: session.opened_at_ms,
            last_seen_ms: event::now_ms(),
            source: session.source.clone(),
            about: session.about.clone(),
            parent: session.parent.clone(),
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

    // The tree it was held in has to still be there. One that was thrown away
    // took the conversation's files with it, and resuming into the project
    // instead would be an agent answering about a tree it never saw.
    if let Some(tree) = held.worktree.as_ref()
        && !std::path::Path::new(&tree.path).is_dir()
    {
        return Err(ProjectError::new(
            "worktree_missing",
            format!(
                "that conversation was held in a working tree at {}, and it is no longer there",
                tree.path
            ),
        ));
    }

    let session = Session::new(
        sessions.mint_key(),
        held.agent_id.clone(),
        spec.display_name.replace('`', ""),
        Place {
            project: PathBuf::from(&project),
            worktree: held.worktree.clone(),
        },
        // Carried by the pointer this resume was read from, which is what
        // makes `docs/background.md` §6.3 true across a restart: the session
        // that was raised for this conversation held its source in memory, and
        // that memory ended with the process. The record it is under travels
        // the same way and for the same reason — a conversation that came back
        // out of a different group is one somebody has to look for twice.
        held.source.clone(),
        held.about.clone(),
        // And what it came out of, for the third time and the same reason: a
        // conversation resumed out from under its parent would leave the row
        // beneath it standing on nothing.
        held.parent.clone(),
    );
    // The name it already had. Nothing the agent replays carries one, and a
    // conversation coming back under a different title is a different
    // conversation as far as the person reading the list is concerned.
    if let Some(title) = held.title.as_deref() {
        session.set_title(title);
    }
    sessions.insert(Arc::clone(&session));

    let cwd = session.cwd.clone();
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
            // Nothing raised this conversation on its own — a person did, which
            // is the condition an outcome held for it has been waiting on since
            // whichever run ended under it.
            crate::work::delegated::deliver(&app, &project);
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
    let project = session.project.to_string_lossy().into_owned();
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
        decoded.push(HeldImage {
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

    let ids = session.keep_images(decoded).map_err(|held| {
        // Two different sentences, because they are two different problems. A
        // conversation that is already full is one a person can act on by
        // starting another; one picture too large for an empty conversation is
        // not, and telling them it is "already holding 0 MB" would be nonsense
        // dressed as an explanation.
        let message = if held == 0 {
            format!(
                "that is larger than the {} of images one conversation may hold",
                megabytes(live::IMAGE_LIMIT_BYTES)
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

/// How a turn ended, at the length whoever ordered it needs.
///
/// Deliberately not a [`Status`]. A status says where the *session* is and it
/// moves again the moment anything else happens in it; this is a fact about one
/// turn, read minutes later out of a queue, and the two would disagree the
/// first time somebody typed into a conversation an agent had delegated.
/// Written into the queue of answers waiting to be handed over
/// ([`crate::work::delegated`]), which is why it is serialised at all: an
/// outcome outlives the run that produced it, and a shape that could not be
/// written down would make that the one thing it could not do.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Ending {
    /// The agent stopped, with the reason it gave when it gave one.
    Stopped(Option<String>),
    /// The turn fell over, and this is what there was to say about it.
    Failed(String),
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
    // Whether this is the turn a delegated conversation was opened to run, and
    // taken here because everything below branches on it: what the agent is
    // told, what the row says while it waits, and where the answer goes.
    let owed = session
        .parent
        .clone()
        .filter(|_| session.take_what_is_owed());
    let text = match &owed {
        // Said by the host rather than by whoever delegated, because the host
        // is what takes the last words and hands them on.
        Some(_) => crate::work::delegated::briefed(&turn.text),
        None => turn.text,
    };
    // Recorded before it is sent, so the transcript holds it whether or not the
    // agent ever answers — and so a screen that comes back to this session is
    // handed the question along with the answer.
    session.record_prompt(text.clone(), turn.attachments.clone(), turn.kept);
    // The first thing said is what names the conversation, so the pointer
    // written when the session opened is holding `null` until now. Rewritten
    // here rather than only at open: a conversation the list could only call
    // "Untitled" is one nobody can pick out of it after a restart.
    remember(app, session);
    // Queued rather than working until the slot below is in hand. Set before
    // the task rather than inside it so there is no moment where a conversation
    // that is about to wait says it is running.
    session.set_status(
        match owed {
            Some(_) => Status::Queued,
            None => Status::Working,
        },
        None,
    );
    let request =
        schema::PromptRequest::new(acp_session, blocks(text, &turn.attachments, turn.sent));

    // The turn runs on its own task so this command can answer now. Nothing is
    // lost by that: every result of the turn is an event on the subscription.
    let watching = Arc::clone(session);
    let afterwards = app.clone();
    let project = session.project.to_string_lossy().into_owned();
    tauri::async_runtime::spawn(async move {
        // Held for the length of the turn, and only by the turn a delegated
        // conversation exists to run. **The turn rather than the agent**: what
        // two delegated runs at once would damage is a working tree, and a
        // process that is up and has been asked nothing touches nothing.
        let slot = match &owed {
            Some(parent) => Some(crate::work::delegated::slot(&afterwards, &project, parent).await),
            None => None,
        };
        if owed.is_some() {
            watching.set_status(Status::Working, None);
        }
        let ending = match connection.prompt(request).await {
            Ok(response) => {
                let reason = serde_json::to_value(response.stop_reason)
                    .ok()
                    .and_then(|value| value.as_str().map(ToOwned::to_owned));
                watching.set_status(Status::Ready, reason.clone());
                Ending::Stopped(reason)
            }
            Err(error) => {
                // The agent's stderr is the only account of a process that died
                // without saying anything in protocol.
                let mut detail = error.to_string();
                let tail = watching.recent_stderr().await;
                if !tail.is_empty() {
                    detail.push_str(&format!(" — {}", tail.join(" ")));
                }
                watching.set_status(Status::Failed, Some(detail.clone()));
                Ending::Failed(detail)
            }
        };
        if let Some(parent) = owed {
            crate::work::delegated::finished(&afterwards, &project, &parent, &watching, ending);
        }
        // After the answer is written down, so the conversation that takes the
        // slot next cannot start before the one it is waiting on has accounted
        // for itself.
        drop(slot);
        // A turn ending is the moment a conversation becomes free, and free is
        // the whole of what an outcome waiting for this one waits for.
        crate::work::delegated::deliver(&afterwards, &project);
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
        .image(&id)
        .ok_or_else(|| ProjectError::new("image_unknown", "that image is no longer held"))?;
    Ok(PastedView {
        mime_type: image.mime_type,
        data: BASE64.encode(&image.bytes),
    })
}

/// Writes one of a conversation's pictures to a file somebody chose.
///
/// The bytes go from the session straight to the path, and never through the
/// window: it already has them as base64 to draw with, but base64 is a third
/// larger and a file written from it would be this application decoding what it
/// had encoded for a different purpose. What crosses is a path.
///
/// The path is not confined to the project. This is somebody saving a picture
/// they were shown, and the desktop is where that goes; a save panel that
/// refused everywhere but the repository would be answering a question nobody
/// asked. It is the panel that chose it, so it is a place they can write.
///
/// Nothing is remembered about having done it. A picture saved is a file like
/// any other from then on, and a conversation that tracked where its pictures
/// went would be keeping a record of somebody's disk.
///
/// # Errors
///
/// [`ProjectError`] when the key names no session, the session no longer holds
/// that picture, or the file cannot be written.
#[tauri::command(async)]
pub fn session_image_save(
    sessions: State<'_, Sessions>,
    key: String,
    id: String,
    path: String,
) -> Result<(), ProjectError> {
    let image = lookup(&sessions, &key)?
        .image(&id)
        .ok_or_else(|| ProjectError::new("image_unknown", "that image is no longer held"))?;
    std::fs::write(&path, &image.bytes)
        .map_err(|error| ProjectError::new("image_unwritable", error.to_string()))
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
        project: session.project.to_string_lossy().into_owned(),
        cwd: session.cwd.to_string_lossy().into_owned(),
        status: session.status(),
        opened_at_ms: session.opened_at_ms,
        accepts_images: session.accepts_images(),
        source: session.source.clone(),
        about: session.about.clone(),
        worktree: session.worktree.clone(),
        acp_session: session.acp_session().map(|id| id.0.to_string()),
        parent: session.parent.clone(),
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
            project: "/tmp/project".to_owned(),
            cwd: "/tmp/project".to_owned(),
            status: Status::Working,
            opened_at_ms: 1234,
            accepts_images: true,
            source: None,
            about: None,
            worktree: None,
            acp_session: None,
            parent: None,
        };

        assert_eq!(
            serde_json::to_string(&row).expect("a row serialises"),
            r#"{"key":"s0","agentId":"opencode","agentName":"OpenCode","title":"Why does reconcile run twice?","project":"/tmp/project","cwd":"/tmp/project","status":"working","openedAtMs":1234,"acceptsImages":true}"#,
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
            project: "/tmp/project".to_owned(),
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
            about: None,
            worktree: None,
            acp_session: None,
            parent: None,
        };

        let json = serde_json::to_value(&row).expect("a row serialises");
        assert_eq!(json["source"]["work"], "w1-0");
        assert_eq!(json["source"]["extensionId"], "issues");
        assert_eq!(
            json["source"]["extensionName"], "Issues",
            "the name a heading is drawn from, so no catalogue is needed to draw one"
        );
        assert_eq!(json["source"]["handler"], "issues.poll");
        assert_eq!(
            json["source"]["about"], "issue-4c1a",
            "the key the order named, which is what a package matches its own work by"
        );
    }

    /// The other half of the same boundary, and the field a list groups by.
    ///
    /// Asserted separately from the source because the two are independent now:
    /// a conversation somebody opened from a task has no orderer and is still
    /// about that task, which is the case grouping by who asked cannot see. All
    /// three members are checked because a heading needs the title, opening the
    /// record needs the kind, and grouping needs the key — a row missing any
    /// one of them arrives as `undefined` and the group quietly disappears.
    #[test]
    fn a_row_says_which_record_it_is_under() {
        let row = SessionRow {
            key: "s2".to_owned(),
            agent_id: "claude".to_owned(),
            agent_name: "Claude Code".to_owned(),
            title: None,
            project: "/tmp/project".to_owned(),
            cwd: "/tmp/project".to_owned(),
            status: Status::Working,
            opened_at_ms: 1234,
            accepts_images: true,
            source: None,
            worktree: None,
            acp_session: None,
            parent: None,
            about: Some(About {
                key: "task-4c1a".to_owned(),
                kind: "tasks.task".to_owned(),
                title: "Support worktrees".to_owned(),
            }),
        };

        let json = serde_json::to_value(&row).expect("a row serialises");
        assert_eq!(json["about"]["key"], "task-4c1a");
        assert_eq!(json["about"]["kind"], "tasks.task");
        assert_eq!(json["about"]["title"], "Support worktrees");
        assert!(
            json.get("source").is_none(),
            "and a person opening one from a record is still a person"
        );
    }

    /// What a conversation came out of, and the id the rest of the list is read
    /// against.
    ///
    /// Both on one row because they are only useful together: a child names its
    /// parent by the agent's id for it, and no row could be found by that name
    /// until every row carried its own. The names are asserted for the reason
    /// the source's are — a member the window spells differently arrives as
    /// `undefined`, and a tree quietly flattens into the list it used to be.
    #[test]
    fn a_row_says_what_it_came_out_of_and_what_it_is_called_by() {
        let row = SessionRow {
            key: "s3".to_owned(),
            agent_id: "claude".to_owned(),
            agent_name: "Claude Code".to_owned(),
            title: None,
            project: "/tmp/project".to_owned(),
            cwd: "/tmp/project".to_owned(),
            status: Status::Working,
            opened_at_ms: 1234,
            accepts_images: true,
            source: None,
            about: None,
            worktree: None,
            acp_session: Some("thread-2".to_owned()),
            parent: Some("thread-1".to_owned()),
        };

        let json = serde_json::to_value(&row).expect("a row serialises");
        assert_eq!(json["acpSession"], "thread-2");
        assert_eq!(
            json["parent"], "thread-1",
            "read against another row's `acpSession`, whichever half of the list it came from"
        );
    }

    /// The other half of the same boundary: what the window sends arrives.
    ///
    /// The window puts both under one member because Rust takes them as one
    /// value, and a member spelled differently on either side is read as
    /// nothing at all — a delegation that silently became an ordinary
    /// conversation, with no error anywhere to say so.
    #[test]
    fn what_a_conversation_stands_under_crosses_from_the_window() {
        let under: Under = serde_json::from_value(serde_json::json!({
            "about": {"key": "task-4c1a", "kind": "tasks.task", "title": "Support worktrees"},
            "parent": "thread-1",
        }))
        .expect("the window's shape is read");

        assert_eq!(under.about.expect("a record").key, "task-4c1a");
        assert_eq!(under.parent.as_deref(), Some("thread-1"));

        let neither: Under = serde_json::from_value(serde_json::json!({
            "about": null,
            "parent": null,
        }))
        .expect("and so is a conversation under nothing");
        assert!(neither.about.is_none() && neither.parent.is_none());
    }

    /// A delegated conversation is filed where its parent is, and by nothing
    /// the caller said.
    ///
    /// This is what makes the heading unforgeable. Whoever asked for the
    /// session — a window, a handler, an agent through a package's tool — may
    /// name a parent and nothing else: what the work is about and who ordered
    /// it are read from that parent, so no caller can put a conversation under
    /// a record it has nothing to do with.
    #[test]
    fn a_delegated_conversation_is_filed_where_its_parent_is() {
        let source = Source {
            work: "w1-0".to_owned(),
            extension_id: "issues".to_owned(),
            extension_name: "Issues".to_owned(),
            handler: "issues.poll".to_owned(),
            about: Some("issue-4c1a".to_owned()),
        };
        let about = About {
            key: "task-4c1a".to_owned(),
            kind: "tasks.task".to_owned(),
            title: "Support worktrees".to_owned(),
        };

        let (descended_source, descended_about) =
            inherited(false, Some(source.clone()), Some(about.clone()))
                .expect("a conversation one deep is allowed");

        assert_eq!(descended_source.as_ref(), Some(&source));
        assert_eq!(descended_about.as_ref(), Some(&about));
    }

    /// And the chain stops at the second conversation.
    ///
    /// Refused in words rather than silently flattened, because an agent that
    /// asked for this is about to do something else instead and needs to know
    /// what: the refusal says to delegate from the conversation above.
    #[test]
    fn a_conversation_delegated_from_a_delegated_one_is_refused() {
        let refusal = inherited(true, None, None).expect_err("the third level is refused");
        assert_eq!(refusal.kind, "conversation_depth");
        assert!(
            refusal.message.contains(&DEPTH.to_string()),
            "the refusal says what the limit is: {}",
            refusal.message
        );
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
            project: "/tmp/project".to_owned(),
            cwd: "/tmp/project".to_owned(),
            status: Status::Ready,
            opened_at_ms: 0,
            accepts_images: false,
            source: None,
            about: None,
            worktree: None,
            acp_session: None,
            parent: None,
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
