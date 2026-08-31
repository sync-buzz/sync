//! One live conversation with one agent, and the registry of all of them.
//!
//! # What a session owns
//!
//! One agent process, one ACP session inside it, and the record of everything
//! that has happened in it. Sync raises a process per session: the connection
//! would hold several, but sharing one is an optimisation and nothing here
//! depends on it.
//!
//! # Why the history is kept in Rust
//!
//! Because a session outlives the screen that opened it. An area that is
//! unmounted — the person switched sections — keeps its agents working, and
//! when it comes back it re-subscribes and is handed everything that happened
//! meanwhile. If the transcript lived in React state it would be destroyed by
//! the unmount, and the agent would go on working into nothing.
//!
//! That is also why a permission request waits here rather than being dropped:
//! the question arrives whether or not anybody is looking, and an unanswered
//! request stops the agent's turn until it is answered. It stays open until
//! somebody answers it or the session ends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use acp_client::{
    AgentConnection, AgentProcess, ClientHandler, McpToolName, RpcError, SessionUpdateEvent,
    SessionUpdatePayload, schema,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::oneshot;

use super::event::{PastedImage, SessionEvent, Status, now_ms};

/// How many events one session keeps for a screen that comes back to it.
///
/// A cap rather than everything, because a session may run for hours and the
/// window only ever re-reads it to redraw. What falls off the front is counted
/// and reported on subscription — a transcript that quietly begins in the
/// middle reads as the whole conversation.
const HISTORY_LIMIT: usize = 4000;

/// How long a name derived from what somebody said is allowed to be.
///
/// Long enough to tell two conversations apart at a glance, short enough to be
/// a title rather than a paragraph — it is offered as the title of the record
/// when a conversation is kept, and a title nobody would have typed is one
/// somebody has to shorten before they can use it.
const DERIVED_TITLE_LIMIT: usize = 60;

/// A live session.
/// Who asked for a session, when it was not a person at the keyboard.
///
/// **Set once, and never edited** (`docs/background.md` §6.3). It is the field
/// that makes everything downstream legible: Chat shows it beside a
/// conversation nobody in this window started, and an extension reads it to
/// find the sessions it ordered itself.
///
/// It lives here rather than with the work that carries it because it is a
/// property of the *session* — who asked for this one — and because both
/// readers must see one definition. `crate::work` writes it into its own
/// durable record from here, which keeps the dependency running one way.
///
/// `None` on a session is a person: the window's own conversations are started
/// by somebody typing, and an explicit variant saying so would be one nothing
/// branches on differently from absence.
///
/// Today the only orderer is an extension's handler. Which *occasion* called
/// that handler is not here — it is derivable from the manifest, and carrying
/// it would widen every signature between here and the call for a fact nothing
/// reads.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// The order this conversation came out of, as `work.order` answered it.
    ///
    /// "Who asked" is not enough on its own: one handler may ask three times,
    /// and three rows carrying the same extension and the same handler are
    /// three rows nothing can tell apart. This is the token the host handed the
    /// package at the moment it ordered, and it is what lets a package say
    /// *this* conversation is task 42 rather than *one of these three is*.
    ///
    /// It is also half of a journey back. `extension` says whom to ask, this
    /// says what to ask about — and the asking itself needs the mechanism that
    /// resolves a request by its kind rather than by an extension's name, which
    /// does not exist yet (`docs/background.md` §4.2).
    pub work: String,
    /// The extension whose handler ordered it, by its manifest id.
    ///
    /// What a package matches against to find its own work, and what a list
    /// groups by. Paired with [`Self::extension_name`] the way `agent_id` is
    /// paired with `agent_name` one file over, and for the same reason: an id
    /// is what things are equal by, a name is what a heading says.
    pub extension_id: String,
    /// What that extension is called, so a heading can be drawn without the
    /// catalogue.
    ///
    /// What it was called when the work was ordered. A package that renames
    /// itself later does not rewrite what it already asked for, exactly as a
    /// renamed agent does not rewrite the conversations it held.
    pub extension_name: String,
    /// The handler that ordered it, by the name an occasion calls.
    pub handler: String,
    /// The record key the order named, as the order named it.
    ///
    /// What a conversation is *about* is answered by [`About`] on the session
    /// itself, and this is not a second answer to it — it is what the order
    /// said, kept where the rest of the order is kept, and it is what a package
    /// matches on to find its own work by record. The two cannot disagree:
    /// both are written from one order, once.
    ///
    /// It stays a bare key. Dropping it would narrow something already
    /// returned, which is a major on this surface, and the number it would cost
    /// buys nothing a reader of [`About`] does not already have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
}

/// The record the work is being done under.
///
/// Held beside the source rather than inside it, because *who asked* and *what
/// it is about* are two questions and only the first of them has a person as an
/// ordinary answer. A conversation somebody opened from a task has no orderer
/// and is still about that task, and a source that carried both would make the
/// second unanswerable for exactly the conversations a list most wants to group.
///
/// Set when the session is opened and never edited, which is what lets a list
/// group by it: a row that changed group when its agent stopped was the mistake
/// the `Running`/`Not running` split made, and this cannot make it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// The record's key: what a list groups by, and what opening it resolves.
    pub key: String,
    /// Its kind, because opening a record takes both — an area lists records by
    /// type and cannot find out which of its own lists a key belongs in without
    /// reading the record first.
    pub kind: String,
    /// What it was called when the work began, so a heading is drawn without
    /// reading the corpus for every row of a list that is polled every few
    /// seconds. The same bargain `extension_name` makes one field up, and it
    /// goes stale the same way: a record renamed later is called what it was
    /// called here until something is ordered about it again.
    pub title: String,
}

/// Where a conversation happens.
///
/// One value rather than two arguments travelling together, because they are
/// only ever meaningful as a pair: a tree without the project it belongs to
/// cannot say whose conversation it is, and a project with a tree that is not
/// inside it is a state nothing should be able to construct.
#[derive(Clone, Debug)]
pub struct Place {
    /// The repository's own working tree.
    pub project: PathBuf,
    /// The tree made for this one conversation, when one was.
    pub worktree: Option<crate::worktree::Worktree>,
}

impl Place {
    /// The project itself.
    #[must_use]
    pub fn project(project: PathBuf) -> Self {
        Self {
            project,
            worktree: None,
        }
    }

    /// Where the agent is raised: the tree when there is one, the project
    /// otherwise.
    #[must_use]
    pub fn cwd(&self) -> PathBuf {
        self.worktree
            .as_ref()
            .map_or_else(|| self.project.clone(), |tree| PathBuf::from(&tree.path))
    }
}

pub struct Session {
    /// This application's own name for the session. The ACP session id is the
    /// agent's bookkeeping and never leaves Rust.
    pub key: String,
    pub agent_id: String,
    pub agent_name: String,
    /// The project this conversation belongs to: the repository's own working
    /// tree, whichever tree the agent was actually raised in.
    ///
    /// Held apart from [`Self::cwd`] because a conversation in a disposable
    /// tree is still that project's conversation. Everything keyed by project
    /// — the pointer written for it, the list it comes back in — reads this,
    /// and reading `cwd` instead would file the work under a directory that is
    /// about to be thrown away.
    pub project: PathBuf,
    /// Where the agent works. The project itself, or a tree made for this one
    /// conversation ([`crate::worktree`]).
    pub cwd: PathBuf,
    /// The tree made for this conversation, when one was.
    ///
    /// Beside the other immutable fields: where an agent works is decided when
    /// it is raised and cannot change under it. `None` is a conversation in the
    /// project's own tree, which is the ordinary answer.
    pub worktree: Option<crate::worktree::Worktree>,
    pub opened_at_ms: u64,
    /// Who asked for this session, when it was not a person.
    ///
    /// Beside the other immutable fields rather than in [`State`], and that is
    /// the invariant made structural: the source is set when the work is
    /// ordered and never edited (`docs/background.md` §6.3). A field that
    /// cannot change does not belong behind a lock that exists for fields that
    /// do.
    pub source: Option<Source>,
    /// The record this conversation is being held under, when it is being held
    /// under one. Beside [`Self::source`] and immutable for the same reason.
    ///
    /// `None` is a conversation about nothing in particular, which is what the
    /// window's own `New conversation` opens.
    pub about: Option<About>,
    state: Mutex<State>,
    /// Held apart from the rest of the state because ending a process is async
    /// and takes `&mut`, and nothing else about a session needs to wait.
    process: tokio::sync::Mutex<Option<AgentProcess>>,
}

#[derive(Default)]
struct State {
    status: Status,
    history: Vec<SessionEvent>,
    /// How many events fell off the front of `history`.
    dropped: u64,
    next_seq: u64,
    next_request_id: u64,
    sink: Option<Channel<SessionEvent>>,
    open_questions: HashMap<u64, oneshot::Sender<Option<schema::PermissionOptionId>>>,
    connection: Option<Arc<AgentConnection>>,
    acp_session: Option<schema::SessionId>,
    /// Whether the agent is replaying a loaded session rather than saying
    /// anything new. True for the span of one `session/load`, and stamped onto
    /// every update that arrives inside it — see [`SessionEvent::Update`].
    replaying: bool,
    /// The tool-name spelling this agent uses, from its registry row.
    tool_naming: Option<acp_client::McpToolNaming>,
    /// The last configuration the agent stated, so a re-subscribing screen can
    /// draw the model picker without waiting for the agent to restate it.
    configuration: Option<serde_json::Value>,
    /// The last mode state the agent stated, held for the same reason as
    /// [`Held::configuration`] and separately from it: a conversation an
    /// extension ordered at three in the morning is opened by a screen that
    /// was not there when the agent answered `session/new`, and a mode picker
    /// it could not draw would be a control that exists only for whoever
    /// happened to be watching.
    modes: Option<serde_json::Value>,
    /// Whether the agent said it reads images. Answered at `initialize` and
    /// fixed for the life of the session.
    accepts_images: bool,
    /// Every picture this conversation holds, by the id the window draws it by
    /// — the ones pasted into it and the ones the agent answered with alike.
    ///
    /// Held here, in memory, and written nowhere. They live exactly as long as
    /// the conversation does, which is what a person pasting a screenshot into
    /// a chat expects of it — and it is why they are here rather than in the
    /// window: the transcript survives leaving the section, and a picture kept
    /// in a screen's own state would leave half a message behind.
    ///
    /// One store for both directions rather than two, because the ceiling below
    /// is about what one conversation may hold and a second store would be a
    /// second ceiling — two halves of a limit neither of which is the limit.
    images: HashMap<String, HeldImage>,
    /// What those images come to, so one conversation cannot fill the machine.
    image_bytes: usize,
    /// The ordinal of the next image. Its own counter rather than the
    /// event sequence: `seq` is the position of an event in this session and
    /// the window builds block ids from it, so spending numbers on something
    /// that is not an event would put gaps in it for no reason.
    next_image: u64,
    /// Whether anything has been said in this conversation.
    ///
    /// What decides whether it is worth writing a pointer for. A session that
    /// was opened and never spoken in is not a conversation: it has no name to
    /// list it under, and an agent does not necessarily keep one — so a pointer
    /// to it is a row that can be neither read nor reopened.
    spoke: bool,
    /// What this conversation is called.
    ///
    /// `None` until somebody says something, because until then there is
    /// nothing to call it that is not already on the row — the agent's name.
    /// The first thing said fills it, and a person may replace it with anything,
    /// including nothing: this is not a key, nothing points at it, and no record
    /// is written from it, so a name here is free in the way a type's name is
    /// free and a type's identifier is not.
    title: Option<String>,
}

/// One picture a conversation holds, however it got there.
#[derive(Debug, Clone)]
pub struct HeldImage {
    /// What it is called where it is drawn. Every browser invents the same name
    /// for a clipboard image, so this is the agent's and the person's label
    /// rather than an identifier. Empty for a picture the agent sent: there is
    /// nothing it was called, and inventing a name would be a caption nobody
    /// wrote.
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// What one conversation may hold in pictures.
///
/// A ceiling rather than a policy: a screenshot is a megabyte or two, and a
/// long session that pasted forty of them would be holding a hundred megabytes
/// for as long as it stayed open. Refused rather than silently dropped — a
/// picture that vanished without a word is worse than one that was not taken.
///
/// It counts what the agent sent as well as what was pasted, and that is the
/// point of one ceiling: an agent asked for twenty pictures in one turn fills
/// the same conversation a person can.
pub const IMAGE_LIMIT_BYTES: usize = 64 * 1024 * 1024;

impl Session {
    pub fn new(
        key: String,
        agent_id: String,
        agent_name: String,
        place: Place,
        source: Option<Source>,
        about: Option<About>,
    ) -> Arc<Self> {
        let cwd = place.cwd();
        Arc::new(Self {
            key,
            agent_id,
            agent_name,
            project: place.project,
            cwd,
            worktree: place.worktree,
            opened_at_ms: now_ms(),
            source,
            about,
            state: Mutex::new(State::default()),
            process: tokio::sync::Mutex::new(None),
        })
    }

    pub fn status(&self) -> Status {
        self.locked().status
    }

    pub fn configuration(&self) -> Option<serde_json::Value> {
        self.locked().configuration.clone()
    }

    pub fn modes(&self) -> Option<serde_json::Value> {
        self.locked().modes.clone()
    }

    /// The connection, once the process is up.
    pub fn connection(&self) -> Option<Arc<AgentConnection>> {
        self.locked().connection.clone()
    }

    pub fn acp_session(&self) -> Option<schema::SessionId> {
        self.locked().acp_session.clone()
    }

    pub fn adopt(
        &self,
        process: AgentProcess,
        connection: Arc<AgentConnection>,
        tool_naming: Option<acp_client::McpToolNaming>,
    ) {
        {
            let mut state = self.locked();
            state.connection = Some(connection);
            state.tool_naming = tool_naming;
        }
        // The process moves in under its own lock; nothing above needs it.
        if let Ok(mut held) = self.process.try_lock() {
            *held = Some(process);
        } else {
            // Only reachable while another caller is ending the session, which
            // means this launch is already unwanted.
            drop(process);
        }
    }

    pub fn remember_session(&self, id: schema::SessionId) {
        self.locked().acp_session = Some(id);
    }

    /// Sends everything recorded so far to `sink`, then keeps sending.
    ///
    /// Replaces whatever was subscribed before: one screen at a time reads a
    /// session, and a second subscription is the same screen remounting.
    /// Returns how many events had already fallen off the front.
    pub fn subscribe(&self, sink: Channel<SessionEvent>) -> u64 {
        let (backlog, dropped) = {
            let mut state = self.locked();
            state.sink = Some(sink.clone());
            (state.history.clone(), state.dropped)
        };
        for event in backlog {
            // A send failure means the window went away between subscribing and
            // replaying. The session is unaffected — that is the point of it
            // living here — so there is nothing to do but stop replaying.
            if sink.send(event).is_err() {
                break;
            }
        }
        dropped
    }

    pub fn unsubscribe(&self) {
        self.locked().sink = None;
    }

    /// Everything this session has said, without watching it say any more.
    ///
    /// The same backlog [`Self::subscribe`] replays and the same count of what
    /// fell off the front of it, handed over once. It exists because keeping a
    /// conversation is a command on a *row*, and the window only holds the
    /// transcript of the one it has open: without this, keeping any other row
    /// would write the open conversation's words under that row's name.
    pub fn backlog(&self) -> (Vec<SessionEvent>, u64) {
        let state = self.locked();
        (state.history.clone(), state.dropped)
    }

    /// Records an event and forwards it to whoever is watching.
    fn emit(&self, build: impl FnOnce(u64, u64) -> SessionEvent) {
        let (event, sink) = {
            let mut state = self.locked();
            let seq = state.next_seq;
            state.next_seq += 1;
            let event = build(seq, now_ms());
            state.history.push(event.clone());
            if state.history.len() > HISTORY_LIMIT {
                let excess = state.history.len() - HISTORY_LIMIT;
                state.history.drain(..excess);
                state.dropped += excess as u64;
            }
            (event, state.sink.clone())
        };
        if let Some(sink) = sink {
            let _ = sink.send(event);
        }
    }

    pub fn set_status(&self, status: Status, detail: Option<String>) {
        {
            let mut state = self.locked();
            if state.status == status && detail.is_none() {
                return;
            }
            state.status = status;
        }
        self.emit(|seq, at_ms| SessionEvent::Status {
            seq,
            at_ms,
            status,
            detail,
        });
    }

    /// Records what a person said, in the order they said it.
    ///
    /// The first thing said also names the conversation, unless it already has
    /// a name. Derived here rather than in the window because the list of
    /// running sessions is read from this side, and a window that had to
    /// subscribe to every session to find out what each one is called would be
    /// holding every conversation open to draw a source list.
    pub fn record_prompt(&self, text: String, attachments: Vec<String>, images: Vec<PastedImage>) {
        {
            let mut state = self.locked();
            state.spoke = true;
            if state.title.is_none() {
                state.title = first_words(&text);
            }
        }
        self.emit(|seq, at_ms| SessionEvent::Prompt {
            seq,
            at_ms,
            text,
            attachments,
            images,
        });
    }

    /// Whether the agent said it reads images.
    pub fn accepts_images(&self) -> bool {
        self.locked().accepts_images
    }

    pub fn set_accepts_images(&self, accepts: bool) {
        self.locked().accepts_images = accepts;
    }

    /// Keeps the images of one message, and answers with the ids they are drawn
    /// by, in the order they were given.
    ///
    /// All of them or none. They belong to one message, and a message is either
    /// sent or it is not: keeping the first two of three would leave two
    /// pictures in this session that no transcript entry mentions, held until
    /// the conversation ends and drawn by nothing.
    ///
    /// # Errors
    ///
    /// The number of bytes already held, when these would take the session past
    /// what it is allowed to hold.
    pub fn keep_images(&self, images: Vec<HeldImage>) -> Result<Vec<String>, usize> {
        let mut state = self.locked();
        let asking: usize = images.iter().map(|image| image.bytes.len()).sum();
        if state.image_bytes + asking > IMAGE_LIMIT_BYTES {
            return Err(state.image_bytes);
        }

        let mut ids = Vec::with_capacity(images.len());
        for image in images {
            // Unique within the session, and saying nothing about the file it
            // came from — there is no file.
            let id = format!("i{}", state.next_image);
            state.next_image += 1;
            state.image_bytes += image.bytes.len();
            state.images.insert(id.clone(), image);
            ids.push(id);
        }
        Ok(ids)
    }

    /// One picture this conversation holds, for the window to draw.
    pub fn image(&self, id: &str) -> Option<HeldImage> {
        self.locked().images.get(id).cloned()
    }

    /// What this conversation is called, when it is called anything.
    pub fn title(&self) -> Option<String> {
        self.locked().title.clone()
    }

    /// Whether anything has been said here yet.
    pub fn spoke(&self) -> bool {
        self.locked().spoke
    }

    /// Renames it. An empty name is not a name: it clears the one there is, and
    /// the next thing said derives another — which is what "put it back" means
    /// for a name nobody has to have chosen.
    pub fn set_title(&self, title: &str) {
        let trimmed = title.trim();
        self.locked().title = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        };
    }

    pub fn state_configuration(&self, options: serde_json::Value) {
        self.locked().configuration = Some(options.clone());
        self.emit(|seq, at_ms| SessionEvent::Configuration {
            seq,
            at_ms,
            options,
        });
    }

    /// Records the modes the agent offers, and says so.
    ///
    /// The whole state each time — the current id travels with the list —
    /// because that is the shape the protocol answers in, at `session/new`, at
    /// `session/load` and at `session/set_mode` alike. Restating the list to
    /// say the current one changed costs nothing and keeps one carrier where
    /// there could have been two.
    pub fn state_modes(&self, modes: serde_json::Value) {
        self.locked().modes = Some(modes.clone());
        self.emit(|seq, at_ms| SessionEvent::Modes { seq, at_ms, modes });
    }

    /// Marks the span of a `session/load`, so that what arrives inside it is
    /// recorded as a replay rather than as something the agent just said.
    pub fn set_replaying(&self, replaying: bool) {
        self.locked().replaying = replaying;
    }

    fn record_update(&self, event: SessionUpdateEvent) {
        let (update, recognized, mut payload) = match event.payload {
            SessionUpdatePayload::Known(known) => {
                let payload = serde_json::to_value(&*known).unwrap_or(serde_json::Value::Null);
                let name = payload
                    .get("sessionUpdate")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned);
                (name, true, payload)
            }
            SessionUpdatePayload::Unrecognized(raw) => (raw.session_update, false, raw.raw),
        };
        // Read as it arrived, and only then relieved of its bytes: `recognized`
        // above is a statement about what the agent sent, and it has to stay
        // one.
        self.hold_pictures(&mut payload);
        let replayed = self.locked().replaying;
        self.emit(|seq, at_ms| SessionEvent::Update {
            seq,
            at_ms,
            update,
            recognized,
            payload,
            replayed,
        });
    }

    /// Takes every picture out of an update and puts it in the session.
    ///
    /// **The one place an update is not forwarded as the agent wrote it**, and
    /// the exception is paid for by the rule at the head of
    /// [`super::event`]: an image block carries base64, a session's history is
    /// replayed whole to every screen that comes back to the conversation, and
    /// a megabyte of picture in it would be paid for on every one of them. So
    /// the bytes move to the store this session already keeps pasted images in,
    /// and what is left in their place is the id they are held under — which is
    /// what the window fetches by, exactly as it does for a picture somebody
    /// pasted.
    ///
    /// Every block is walked rather than the one member `agent_message_chunk`
    /// puts its content in, because a tool call carries content too and an
    /// agent nobody has met yet may put a picture somewhere neither of them
    /// does. What must be true is not "the known places are handled" but "no
    /// image bytes are in the history", and only a walk says that.
    ///
    /// A picture that would not fit keeps its place in the conversation and
    /// loses its id: `imageId` is null, and the window says a picture was here
    /// rather than drawing a gap. Dropping the block instead would be a turn in
    /// which the agent answered with nothing.
    fn hold_pictures(&self, payload: &mut serde_json::Value) {
        match payload {
            serde_json::Value::Array(blocks) => {
                for block in blocks {
                    self.hold_pictures(block);
                }
            }
            serde_json::Value::Object(block) => {
                let picture = (block.get("type").and_then(serde_json::Value::as_str)
                    == Some("image"))
                .then(|| {
                    let data = block.get("data")?.as_str()?.to_owned();
                    let mime = block
                        .get("mimeType")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("image/png")
                        .to_owned();
                    Some((data, mime))
                })
                .flatten();
                if let Some((data, mime)) = picture {
                    let (id, size) = self.hold_one_picture(&data, &mime);
                    block.remove("data");
                    block.insert("imageId".to_owned(), id);
                    block.insert("bytes".to_owned(), size.into());
                    return;
                }
                for (_, value) in block.iter_mut() {
                    self.hold_pictures(value);
                }
            }
            _ => {}
        }
    }

    /// The id one picture is held under and how many bytes it came to, or a
    /// null id and a zero when it could not be kept.
    ///
    /// Undecodable and too large answer alike, because from the window they are
    /// one thing — a picture that is not there to draw — and two spellings of
    /// it would be two ways to say so in Chat for a difference nobody can act
    /// on.
    fn hold_one_picture(&self, data: &str, mime_type: &str) -> (serde_json::Value, usize) {
        let Ok(bytes) = BASE64.decode(data.as_bytes()) else {
            return (serde_json::Value::Null, 0);
        };
        let size = bytes.len();
        let kept = self.keep_images(vec![HeldImage {
            name: String::new(),
            mime_type: mime_type.to_owned(),
            bytes,
        }]);
        match kept.ok().and_then(|ids| ids.into_iter().next()) {
            Some(id) => (serde_json::Value::String(id), size),
            None => (serde_json::Value::Null, size),
        }
    }

    /// Opens a question and waits for the window to answer it.
    async fn ask(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Option<schema::PermissionOptionId> {
        let (tx, rx) = oneshot::channel();
        let request_id = {
            let mut state = self.locked();
            let id = state.next_request_id;
            state.next_request_id += 1;
            state.open_questions.insert(id, tx);
            id
        };

        let tool_name = self.canonical_tool_name(&request);
        let payload = serde_json::to_value(&request).unwrap_or(serde_json::Value::Null);
        self.emit(|seq, at_ms| SessionEvent::Permission {
            seq,
            at_ms,
            request_id,
            tool_name,
            request: payload,
        });
        self.set_status(Status::Asking, None);

        // The wait has no deadline on purpose. The question is a person's to
        // answer, and a person is allowed to be away from the machine; killing
        // an agent because nobody was looking would throw away the work it had
        // already done. What bounds it instead is visible and manual — the
        // session is listed as waiting, and it can be stopped.
        let chosen = rx.await.ok().flatten();
        self.locked().open_questions.remove(&request_id);
        let named = chosen.as_ref().map(|id| id.0.to_string());
        self.emit(|seq, at_ms| SessionEvent::PermissionSettled {
            seq,
            at_ms,
            request_id,
            chosen: named,
        });
        chosen
    }

    /// Answers an open question. `None` withdraws it without choosing.
    pub fn answer(&self, request_id: u64, chosen: Option<schema::PermissionOptionId>) -> bool {
        let sender = self.locked().open_questions.remove(&request_id);
        match sender {
            Some(sender) => sender.send(chosen).is_ok(),
            None => false,
        }
    }

    /// The tool a question is about, under this application's name for it.
    ///
    /// One MCP tool is spelled four ways across four agents, so a window that
    /// re-derived the name would have to know which agent it was looking at.
    /// A tool that is not an MCP tool is the agent's own and is passed through
    /// as it arrived.
    fn canonical_tool_name(&self, request: &schema::RequestPermissionRequest) -> Option<String> {
        // The title, because the programmatic `name` field is behind the schema
        // crate's `unstable_tool_call_name` feature and is not compiled here. In
        // the frames measured live it is the title that carries the MCP tool's
        // wire spelling anyway; a title that is a sentence simply fails to parse
        // as one and is passed through, which is the right answer for it.
        let raw = request.tool_call.fields.title.clone()?;
        let Some(naming) = self.locked().tool_naming else {
            return Some(raw);
        };
        let parsed: Option<McpToolName> = naming.parse(&raw, &["sync"]);
        Some(parsed.map_or(raw, |name| format!("{}/{}", name.server, name.tool)))
    }

    /// Ends the session: withdraws every open question, kills the process.
    pub async fn end(&self, detail: Option<String>) {
        let open: Vec<u64> = self.locked().open_questions.keys().copied().collect();
        for id in open {
            self.answer(id, None);
        }
        let taken = self.process.lock().await.take();
        if let Some(mut process) = taken {
            let _ = process.kill().await;
        }
        self.locked().connection = None;
        self.set_status(Status::Ended, detail);
    }

    /// The trailing stderr of the agent's process, for a failure that has no
    /// protocol-level explanation.
    pub async fn recent_stderr(&self) -> Vec<String> {
        match &*self.process.lock().await {
            Some(process) => process.recent_stderr(),
            None => Vec::new(),
        }
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, State> {
        // A poisoned lock means a panic while a session's state was held. The
        // state is a transcript and a status: recovering it is strictly better
        // than taking the window down with it.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The client half of the conversation, for one session.
///
/// Holds the session weakly: the session owns the process, the process owns the
/// connection and the connection owns this, so a strong handle here would be a
/// cycle that keeps an ended session's process alive forever.
pub struct SessionHandler {
    session: Weak<Session>,
    /// The one directory this session's agent may read and write through us.
    cwd: PathBuf,
}

impl SessionHandler {
    pub fn new(session: &Arc<Session>) -> Self {
        Self {
            session: Arc::downgrade(session),
            cwd: session.cwd.clone(),
        }
    }

    fn session(&self) -> Option<Arc<Session>> {
        self.session.upgrade()
    }

    /// Resolves a path the agent named, refusing anything outside the session's
    /// own directory.
    ///
    /// The agent is a separate program and its requests are input, not
    /// instructions: a session opened on one project must not be a way to read
    /// a file in another. Resolved against the real directory so that a
    /// symlink or a `..` cannot step out of it.
    fn within_cwd(&self, path: &str) -> Result<PathBuf, RpcError> {
        let requested = PathBuf::from(path);
        let absolute = if requested.is_absolute() {
            requested
        } else {
            self.cwd.join(requested)
        };
        let outside =
            || RpcError::invalid_params(format!("{path} is outside this session's directory"));

        let root = self.cwd.canonicalize().map_err(|_| outside())?;
        // The file may not exist yet — a write is allowed to create one — so the
        // parent is what has to resolve. A path whose parent does not resolve
        // either is refused rather than taken as it was written: it still holds
        // whatever `..` the agent put in it, and a directory prefix compared
        // against a path with those in it matches for exactly the paths that
        // walk back out of the directory.
        let anchor = match absolute.canonicalize() {
            Ok(resolved) => resolved,
            Err(_) => {
                let parent = absolute.parent().ok_or_else(outside)?;
                // `None` for a path ending in `..`, which names a directory
                // above rather than a file to write.
                let name = absolute.file_name().ok_or_else(outside)?;
                parent.canonicalize().map_err(|_| outside())?.join(name)
            }
        };
        if anchor.starts_with(&root) {
            Ok(anchor)
        } else {
            Err(outside())
        }
    }
}

#[async_trait::async_trait]
impl ClientHandler for SessionHandler {
    async fn session_update(&self, event: SessionUpdateEvent) {
        // Quick by contract: this call sits on the ordered delivery path, so
        // anything slow here delays every later frame on the connection.
        // Recording and forwarding is all it does.
        if let Some(session) = self.session() {
            session.record_update(event);
        }
    }

    async fn request_permission(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Result<schema::RequestPermissionResponse, RpcError> {
        let Some(session) = self.session() else {
            return Err(RpcError::internal("the session is gone"));
        };
        let outcome = match session.ask(request).await {
            Some(option_id) => schema::RequestPermissionOutcome::Selected(
                schema::SelectedPermissionOutcome::new(option_id),
            ),
            // Nothing was chosen because the session ended under the question.
            // Cancelled is the protocol's word for exactly that, and it is not
            // an error: the agent is being told the turn is over, not that we
            // failed to understand it.
            None => schema::RequestPermissionOutcome::Cancelled,
        };
        if session.status() == Status::Asking {
            session.set_status(Status::Working, None);
        }
        Ok(schema::RequestPermissionResponse::new(outcome))
    }

    async fn read_text_file(
        &self,
        request: schema::ReadTextFileRequest,
    ) -> Result<schema::ReadTextFileResponse, RpcError> {
        let path = self.within_cwd(&request.path.to_string_lossy())?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| RpcError::internal(format!("{}: {error}", path.display())))?;
        Ok(schema::ReadTextFileResponse::new(content))
    }

    async fn write_text_file(
        &self,
        request: schema::WriteTextFileRequest,
    ) -> Result<schema::WriteTextFileResponse, RpcError> {
        let path = self.within_cwd(&request.path.to_string_lossy())?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| RpcError::internal(format!("{}: {error}", parent.display())))?;
        }
        tokio::fs::write(&path, &request.content)
            .await
            .map_err(|error| RpcError::internal(format!("{}: {error}", path.display())))?;
        Ok(schema::WriteTextFileResponse::new())
    }
}

/// Every session this window is running, across every extension.
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Arc<Session>>>,
    next: AtomicU64,
}

impl Sessions {
    /// A key no other session has had in this run.
    pub fn mint_key(&self) -> String {
        format!("s{}", self.next.fetch_add(1, Ordering::SeqCst))
    }

    pub fn insert(&self, session: Arc<Session>) {
        self.locked().insert(session.key.clone(), session);
    }

    pub fn get(&self, key: &str) -> Option<Arc<Session>> {
        self.locked().get(key).cloned()
    }

    pub fn remove(&self, key: &str) -> Option<Arc<Session>> {
        self.locked().remove(key)
    }

    pub fn all(&self) -> Vec<Arc<Session>> {
        self.locked().values().cloned().collect()
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The opening words of what somebody said, as a name for the conversation.
///
/// The first line only: somebody who pasted a file after their question asked
/// the question on the first line, and a name made of the paste would be the
/// same name every time. Whole words, because a name cut mid-word reads as a
/// fault rather than as an abbreviation — and no trailing mark, because the row
/// that draws it truncates, and a name that already ends in an ellipsis would
/// tell a person the text was cut when it was not.
fn first_words(text: &str) -> Option<String> {
    let line = text.trim().lines().next()?.trim();
    if line.is_empty() {
        return None;
    }

    let mut name = String::new();
    for word in line.split_whitespace() {
        let next = if name.is_empty() {
            word.chars().count()
        } else {
            name.chars().count() + 1 + word.chars().count()
        };
        if !name.is_empty() && next > DERIVED_TITLE_LIMIT {
            break;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }

    // One word longer than the whole allowance is not made shorter by dropping
    // words, so it is cut. By characters, never by bytes: a name in Russian is
    // half the length in characters that it is in bytes, and slicing a string
    // there splits a letter in two.
    if name.chars().count() > DERIVED_TITLE_LIMIT {
        name = name.chars().take(DERIVED_TITLE_LIMIT).collect();
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Arc<Session> {
        Session::new(
            "s0".to_owned(),
            "opencode".to_owned(),
            "OpenCode".to_owned(),
            Place::project(std::env::temp_dir()),
            None,
            None,
        )
    }

    #[test]
    fn a_session_nobody_spoke_in_is_not_worth_a_pointer() {
        let session = session();
        assert!(
            !session.spoke(),
            "opening a session is not the same as holding a conversation"
        );
        session.set_title("Named before anything was said");
        assert!(
            !session.spoke(),
            "and naming one does not make it one either"
        );

        session.record_prompt("Why is it slow?".to_owned(), Vec::new(), Vec::new());
        assert!(session.spoke());
    }

    #[test]
    fn a_session_keeps_what_happened_while_nobody_watched() {
        let session = session();
        session.set_status(Status::Ready, None);
        session.state_configuration(serde_json::json!([{ "id": "model" }]));

        // Nothing is subscribed, and nothing is lost: the events are numbered
        // and kept, which is the whole reason a session does not live in React.
        let state = session.locked();
        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history[0].seq(), 0);
        assert_eq!(state.history[1].seq(), 1);
    }

    /// An update carrying an image, as an agent sends one.
    ///
    /// Built through the client crate's own decoder rather than hand-assembled,
    /// so what is tested is the shape that actually reaches this side.
    fn image_update(data: &str) -> SessionUpdateEvent {
        acp_client::decode_session_update(serde_json::json!({
            "sessionId": "s0",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "image", "data": data, "mimeType": "image/webp" },
            },
        }))
        .expect("the envelope carries a session id")
    }

    /// The picture goes to the session; the history keeps its place and not its
    /// bytes.
    ///
    /// This is the whole reason the transport touches the payload at all. The
    /// assertion that matters is the last one: it is made against the JSON the
    /// window is actually sent, because a base64 payload that survived in some
    /// nested member would still be paid for on every return to the
    /// conversation, and would still be invisible to an assertion about the
    /// members this test happens to name.
    #[test]
    fn an_image_in_an_update_moves_into_the_session_and_out_of_the_history() {
        let session = session();
        // "hello" — five bytes, and short enough that its base64 can be looked
        // for in the serialised history by eye as well as by the assertion.
        session.record_update(image_update("aGVsbG8="));

        let state = session.locked();
        let SessionEvent::Update { payload, .. } = &state.history[0] else {
            panic!("an update is recorded as an update");
        };
        let block = &payload["content"];
        assert_eq!(block["type"], serde_json::json!("image"));
        assert_eq!(
            block["imageId"],
            serde_json::json!("i0"),
            "what is left in the bytes' place is what the window fetches by"
        );
        assert_eq!(
            block["bytes"],
            serde_json::json!(5),
            "so a window can say how big it is without asking for it"
        );
        assert_eq!(
            block["mimeType"],
            serde_json::json!("image/webp"),
            "the agent's own media type, not a guess made here"
        );
        assert!(
            block.get("data").is_none(),
            "the bytes are the one thing that must not be in the history"
        );
        let history = serde_json::to_string(&state.history).expect("a history serialises");
        assert!(
            !history.contains("aGVsbG8="),
            "no base64 anywhere in what the window is sent: {history}"
        );
        drop(state);

        assert_eq!(
            session.image("i0").expect("the session holds it").bytes,
            b"hello",
            "and the bytes are in the session, under the id the block carries"
        );
    }

    /// A picture past the ceiling keeps its place in the conversation and loses
    /// its id.
    ///
    /// Dropping the block instead would be a turn in which the agent answered
    /// with nothing — the failure that is worse than the picture being missing,
    /// because there would be nothing to say a picture had been missed.
    #[test]
    fn an_image_that_would_not_fit_is_still_a_place_in_the_conversation() {
        let session = session();
        session
            .keep_images(vec![HeldImage {
                name: "Pasted image".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![0; IMAGE_LIMIT_BYTES],
            }])
            .expect("the conversation is empty, so this fits exactly");

        session.record_update(image_update("aGVsbG8="));

        let state = session.locked();
        let SessionEvent::Update { payload, .. } = &state.history[0] else {
            panic!("an update is recorded as an update");
        };
        let block = &payload["content"];
        assert_eq!(block["type"], serde_json::json!("image"));
        assert!(
            block["imageId"].is_null(),
            "there is no id because there is nothing held under one: {block}"
        );
        assert_eq!(
            block["bytes"],
            serde_json::json!(5),
            "how big it was is still worth saying"
        );
    }

    #[test]
    fn a_history_that_overflows_says_how_much_it_dropped() {
        let session = session();
        for _ in 0..(HISTORY_LIMIT + 10) {
            session.state_configuration(serde_json::Value::Null);
        }
        let state = session.locked();
        assert_eq!(state.history.len(), HISTORY_LIMIT);
        assert_eq!(state.dropped, 10, "what fell off the front is counted");
    }

    #[test]
    fn an_unanswered_question_is_still_open_and_an_answered_one_is_not() {
        let session = session();
        let (tx, _rx) = oneshot::channel();
        session.locked().open_questions.insert(7, tx);

        assert!(session.answer(7, None), "the question was open");
        assert!(
            !session.answer(7, None),
            "answering twice does not answer twice"
        );
    }

    #[test]
    fn the_first_thing_said_names_the_conversation() {
        let session = session();
        assert_eq!(session.title(), None, "an empty conversation has no name");

        session.record_prompt(
            "Why does reconcile run twice?".to_owned(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            session.title().as_deref(),
            Some("Why does reconcile run twice?"),
        );

        session.record_prompt("And what does it cost?".to_owned(), Vec::new(), Vec::new());
        assert_eq!(
            session.title().as_deref(),
            Some("Why does reconcile run twice?"),
            "the second thing said does not rename what the first named",
        );
    }

    #[test]
    fn a_name_can_be_replaced_and_put_back() {
        let session = session();
        session.record_prompt(
            "Why does reconcile run twice?".to_owned(),
            Vec::new(),
            Vec::new(),
        );

        session.set_title("  Reconcile  ");
        assert_eq!(
            session.title().as_deref(),
            Some("Reconcile"),
            "what is stored is what was typed, without the space around it",
        );

        session.set_title("   ");
        assert_eq!(
            session.title(),
            None,
            "a name of nothing but space is not a name",
        );

        session.record_prompt("Anything at all".to_owned(), Vec::new(), Vec::new());
        assert_eq!(
            session.title().as_deref(),
            Some("Anything at all"),
            "and the next thing said derives another",
        );
    }

    #[test]
    fn a_pasted_image_is_held_by_the_session_and_by_nothing_else() {
        let session = session();
        let ids = session
            .keep_images(vec![
                HeldImage {
                    name: "Pasted image".to_owned(),
                    mime_type: "image/png".to_owned(),
                    bytes: vec![1, 2, 3],
                },
                HeldImage {
                    name: "Pasted image".to_owned(),
                    mime_type: "image/png".to_owned(),
                    bytes: vec![4],
                },
            ])
            .expect("they fit");

        assert_eq!(ids.len(), 2, "two pastes are two images");
        assert_ne!(ids[0], ids[1]);
        assert_eq!(
            session.image(&ids[0]).expect("the first").bytes,
            vec![1, 2, 3],
            "the bytes come back as they went in, in the order they were given",
        );
        assert!(
            session.image("i404").is_none(),
            "an id nothing was kept under is not invented for",
        );
    }

    #[test]
    fn images_that_would_not_fit_are_refused_whole_rather_than_in_part() {
        let session = session();
        let refused = session.keep_images(vec![
            HeldImage {
                name: "Small".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![0; 16],
            },
            HeldImage {
                name: "Enormous".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![0; IMAGE_LIMIT_BYTES],
            },
        ]);
        assert_eq!(
            refused,
            Err(0),
            "it says how much is already held, which is nothing",
        );
        assert!(
            session.image("i0").is_none(),
            "and the one that would have fitted was not kept either — they are \
             one message, and half of it is not a message",
        );
    }

    #[test]
    fn a_derived_name_is_whole_words_from_the_first_line() {
        assert_eq!(first_words("   "), None);
        assert_eq!(first_words("\n\n"), None);
        assert_eq!(
            first_words("What is this?\nfn main() {}"),
            Some("What is this?".to_owned()),
            "a question with a paste under it is named by the question",
        );

        let long = first_words(
            "Explain in detail why the reconciliation pass runs a second time \
             after the working tree changes",
        )
        .expect("a name");
        assert!(
            long.chars().count() <= DERIVED_TITLE_LIMIT,
            "it fits: {long:?}",
        );
        assert!(
            !long.ends_with(' ') && long.split_whitespace().count() > 1,
            "and it is made of whole words: {long:?}",
        );

        let one_word = "ы".repeat(DERIVED_TITLE_LIMIT + 20);
        assert_eq!(
            first_words(&one_word).expect("a name").chars().count(),
            DERIVED_TITLE_LIMIT,
            "a single word longer than the allowance is cut by characters",
        );
    }

    #[test]
    fn a_path_outside_the_session_directory_is_refused() {
        let root = std::env::temp_dir().join("sync-acp-cwd-test");
        std::fs::create_dir_all(&root).expect("the test directory");
        let handler = SessionHandler {
            session: Weak::new(),
            cwd: root.clone(),
        };

        assert!(
            handler.within_cwd("notes.md").is_ok(),
            "a file in the session's own directory is allowed"
        );
        assert!(
            handler.within_cwd("../../etc/passwd").is_err(),
            "an agent must not read its way out of the project it was opened on"
        );
        assert!(
            handler.within_cwd("/etc/passwd").is_err(),
            "an absolute path outside the directory is the same refusal"
        );
        assert!(
            handler
                .within_cwd("nowhere/../../../../etc/passwd")
                .is_err(),
            "a directory that does not exist is not a way to keep the `..` \
             that walks out of the one that does"
        );
    }
}
