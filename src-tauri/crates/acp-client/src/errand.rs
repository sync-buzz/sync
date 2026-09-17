//! One turn, run for one tool's answer, with nobody watching it.
//!
//! Everywhere else in this crate a turn belongs to a conversation: somebody
//! asked for it, sees what the agent says, and answers the questions it asks
//! back. An errand is the other kind. It is started on a window's behalf to
//! fetch data, its whole product is the JSON one MCP tool returned, and the
//! agent's own words during it are discarded unread.
//!
//! # Why the answer is not the agent's words
//!
//! Because the agent is free to paraphrase, and a paraphrase of a tool's answer
//! is indistinguishable from the answer until somebody checks it against the
//! server. `raw_output` is the tool's own word and the only member read here;
//! [`crate::update::ToolCallReport`] is the one door onto it.
//!
//! # Why it is bounded and the conversation's turn is not
//!
//! `AgentConnection::prompt` deliberately carries no deadline: a turn is the
//! agent working, and a person watching it can cancel. Nobody is watching this
//! one, so the same reasoning gives the opposite answer — an unwatched turn
//! that never ends is a process this machine keeps forever, and [`Errand`]
//! carries its own patience for exactly that.
//!
//! # What it deliberately cannot do
//!
//! Read a file or write one. An errand exists to fetch, and there is nobody at
//! the keyboard to hand it anything.
//!
//! # The one permission it can give, and where that permission came from
//!
//! Not from the turn. [`Consent`] is carried in by whoever started the errand
//! and says one thing: a person agreed, before this turn existed, that the
//! server this errand was sent to may be called. A request about a tool of that
//! server is allowed; a request this side cannot read as one — the agent's own
//! shell, a tool of somebody else's server, a title that is a sentence — and an
//! errand carrying no consent at all are refused with the narrowest *no* the
//! agent offered, because a turn that approved its own operations would be this
//! side signing for somebody who never saw the question.
//!
//! The agreement is about the server rather than about the one tool that was
//! asked for, and the difference shows up here rather than anywhere else: a
//! turn sent to fetch one thing may be told by its own agent that it needs
//! permission for a neighbouring call, and that call is inside what somebody
//! agreed to. What it is never inside is another server.
//!
//! Allowed **once**, never always, although the agreement itself outlives the
//! turn. An *always* is remembered in the agent's own store, where this
//! installation can neither show it to the person who gave it nor take it back;
//! a settings screen offering to withdraw an agreement would then be describing
//! a decision it does not hold.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Error, RpcError};
use crate::handler::{self, ClientHandler};
use crate::launch::{self, SpawnOptions};
use crate::registry::AgentLaunchSpec;
use crate::schema;
use crate::tool_names::{McpToolName, McpToolNaming};
use crate::update::{SessionUpdateEvent, SessionUpdatePayload};

/// How long an errand may run before it is given up on and its process killed.
///
/// Generous, and sized for the same cold start [`crate::DEFAULT_REQUEST_TIMEOUT`]
/// is: the first launch of an adapter fetches a package before a frame is
/// written, and the tool at the far end may be talking to somebody's server
/// over a slow link. What it defends against is not slowness but a turn that
/// never ends at all — the one duration with no defence is forever.
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(180);

/// Whether somebody has already agreed to the call this errand makes.
///
/// Two states and no third, because there is no *ask them*: an errand runs with
/// no window of its own, so a question raised inside one would wait for a
/// person who is not there until the patience below ran out.
///
/// It is about the server this errand's tool belongs to. Who agreed, what they
/// were shown, how wide the agreement is and where it is stored are all the
/// caller's business — this crate is handed the answer, never the reasoning, so
/// a change in either of those is not a change in here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Consent {
    /// Nobody agreed to this call. A permission the agent asks for is refused.
    ///
    /// The default, and it is the default so that a caller which has never
    /// heard of consent refuses rather than approves.
    #[default]
    Withheld,
    /// A person agreed, before the turn started, that this errand's server may
    /// be called.
    Given,
}

/// One errand: which tool to call, with what, where, and on whose agreement.
///
/// Named in MCP's own vocabulary — a server, a tool, its arguments — rather
/// than in this protocol's. That is the point of the shape: what a caller states
/// is what the far end understands, so the turn underneath can be replaced by
/// some other way of reaching the same tool without a single caller changing.
#[derive(Debug, Clone)]
pub struct Errand {
    /// The directory the turn runs in, which reaches the agent as the session's
    /// `cwd`. An agent resolves its own configuration against it, so it is the
    /// project rather than anywhere this process happens to be.
    pub cwd: PathBuf,
    /// The tool to call, as *we* name it. Rendering it into the spelling this
    /// particular agent uses is [`McpToolNaming`]'s job and happens here.
    pub tool: McpToolName,
    /// The arguments, exactly as the caller stated them. Passed through without
    /// inspection: what a tool takes is the tool's business, and a client that
    /// validated them would be a second, older copy of somebody else's schema.
    pub arguments: Value,
    /// How long the turn may take. See [`DEFAULT_PATIENCE`].
    pub patience: Duration,
    /// Whether the person has agreed to this tool being called. See [`Consent`].
    pub consent: Consent,
}

impl Errand {
    /// An errand with the default patience, agreed to by nobody.
    ///
    /// A caller that has an agreement says so afterwards, by setting
    /// [`Errand::consent`]. That way round rather than as an argument, so that
    /// the errand which is written without a thought for permissions is the one
    /// that gives none away.
    #[must_use]
    pub fn new(cwd: PathBuf, tool: McpToolName, arguments: Value) -> Self {
        Self {
            cwd,
            tool,
            arguments,
            patience: DEFAULT_PATIENCE,
            consent: Consent::Withheld,
        }
    }

    /// What the agent is asked, in words.
    ///
    /// Written here rather than by the caller because the instruction and the
    /// verdict below are one decision: this asks for one call of one named
    /// tool, and [`Watch::verdict`] returns an answer only from a call bearing
    /// that name. Split across two files those two would drift, and the drift
    /// would read as the agent misbehaving.
    fn instruction(&self, naming: McpToolNaming) -> String {
        let wire = naming.render(&self.tool);
        let arguments =
            serde_json::to_string_pretty(&self.arguments).unwrap_or_else(|_| "{}".to_owned());
        format!(
            "Call the tool `{wire}` exactly once, with exactly these arguments:\n\n\
             {arguments}\n\n\
             Call no other tool, and change nothing anywhere. What the tool returns \
             is the whole of the answer; anything you write about it is discarded \
             unread, so do not summarise it and do not repeat it back."
        )
    }
}

/// Why an errand came back without the tool's answer.
///
/// Each of these is a different thing to say to somebody and a different thing
/// to do about it, which is why they are not one *it did not work*. The one
/// they must never collapse into is [`Refusal::NotCalled`]: reporting that for
/// an answer this side lost is a lie about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The turn ended and no tool was called. The agent answered in words, and
    /// words are not an answer here.
    NotCalled,
    /// The agent asked for permission to do something nobody had agreed to in
    /// advance, so it was refused and the tool never ran.
    ///
    /// Which covers both a turn nobody agreed to at all and one where the
    /// agreement was for this errand's tool and the agent asked about
    /// something else.
    PermissionNeeded,
    /// A frame announcing a tool call arrived that the compiled protocol types
    /// could not read. A call was made and its answer went past this client —
    /// which is a loss on this side, not a refusal on the agent's.
    AnswerLost,
    /// Tools were called and not one of them carried a result.
    NoAnswer,
    /// Tools were called, and not one of them was named as the tool that was
    /// asked for.
    ///
    /// Its neighbour [`Refusal::NotCalled`] is a turn in which no tool ran at
    /// all; this is a turn in which something ran and nothing said it was
    /// ours. The difference decides where somebody looks: at an agent that
    /// ignored the instruction, or at a title this build cannot read as a
    /// name.
    NotNamed,
    /// More than one call of the tool that was asked for came back with a
    /// result. Which of them is *the* answer is not something this can decide,
    /// and picking one would be a guess presented as data.
    SeveralAnswers,
    /// This build does not know how the agent spells MCP tool names, so it
    /// cannot state which tool to call.
    NamingUnknown,
    /// The turn outran its patience and the process was stopped.
    Overran(Duration),
}

impl Refusal {
    /// A stable name for this refusal, for whoever has to branch on it.
    ///
    /// Separate from the sentence because they are read by different readers:
    /// this one never changes wording, and the sentence is free to.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotCalled => "tool_not_called",
            Self::PermissionNeeded => "tool_permission_needed",
            Self::AnswerLost => "tool_answer_lost",
            Self::NoAnswer => "tool_answered_nothing",
            Self::NotNamed => "tool_never_named",
            Self::SeveralAnswers => "tool_answered_several",
            Self::NamingUnknown => "tool_naming_unknown",
            Self::Overran(_) => "tool_call_overran",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCalled => f.write_str("the agent finished without calling the tool"),
            Self::PermissionNeeded => f.write_str(
                "the agent asked permission for something nobody had agreed to, and there was nobody to ask",
            ),
            Self::AnswerLost => {
                f.write_str("the tool was called and its answer could not be read")
            }
            Self::NoAnswer => f.write_str("the tool was called and returned nothing"),
            Self::NotNamed => f.write_str(
                "the turn called tools, and none of them was named as the one that was asked for",
            ),
            Self::SeveralAnswers => {
                f.write_str("the tool answered more than once, so which answer is its answer cannot be told")
            }
            Self::NamingUnknown => {
                f.write_str("this build does not know how that agent names MCP tools")
            }
            Self::Overran(patience) => {
                write!(f, "the turn was still running after {patience:?}")
            }
        }
    }
}

/// Everything an errand can come back with instead of an answer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ErrandError {
    /// The turn ran and produced no answer this client may report.
    #[error("{0}")]
    Refused(Refusal),
    /// The connection itself failed — the process would not start, the agent
    /// refused a control request, it died mid-turn.
    #[error(transparent)]
    Connection(#[from] Error),
}

impl From<Refusal> for ErrandError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// One tool call, assembled from every frame that mentioned it.
///
/// The protocol spreads a call over an announcement and its amendments, so the
/// members fill in over time; `raw_output` arriving is what makes the call
/// answered.
#[derive(Debug)]
struct Call {
    id: String,
    title: Option<String>,
    raw_output: Option<Value>,
}

/// What the turn was seen to do.
#[derive(Debug, Default)]
struct Seen {
    calls: Vec<Call>,
    /// A frame that said `tool_call` and could not be read. Kept apart from
    /// having no calls at all, because the two are opposite reports.
    lost: bool,
    /// The agent asked to be allowed something and was told no.
    ///
    /// A permission that was *given* is not recorded, and that is the whole of
    /// the difference: this member exists to explain a turn with no answer in
    /// it, and a granted permission explains nothing — the tool ran. Recording
    /// both would report *nobody could allow it* about a call that was allowed
    /// and then went wrong somewhere else entirely.
    permission_refused: bool,
}

/// Follows one turn and says at the end of it what the tool answered.
///
/// Shared between the handler, which is on the connection's delivery path, and
/// whoever is awaiting the turn.
#[derive(Debug)]
struct Watch {
    naming: McpToolNaming,
    wanted: McpToolName,
    seen: Mutex<Seen>,
}

impl Watch {
    fn new(naming: McpToolNaming, wanted: McpToolName) -> Self {
        Self {
            naming,
            wanted,
            seen: Mutex::new(Seen::default()),
        }
    }

    /// Records what one `session/update` said. Cheap by contract: this runs on
    /// the ordered delivery path.
    fn observe(&self, event: &SessionUpdateEvent) {
        let Ok(mut seen) = self.seen.lock() else {
            return;
        };

        if let Some(report) = event.payload.tool_call() {
            let id = report.tool_call_id.0.to_string();
            let title = report.title.map(ToOwned::to_owned);
            // The one clone of `raw_output` this errand makes, and it is made
            // here because the report borrows the event and the event is gone
            // when the turn ends.
            let raw_output = report.raw_output.cloned();
            match seen.calls.iter_mut().find(|call| call.id == id) {
                // An amendment states only what changed, so nothing it left out
                // may overwrite what an earlier frame said.
                Some(call) => {
                    if title.is_some() {
                        call.title = title;
                    }
                    if raw_output.is_some() {
                        call.raw_output = raw_output;
                    }
                }
                None => seen.calls.push(Call {
                    id,
                    title,
                    raw_output,
                }),
            }
            return;
        }

        if let SessionUpdatePayload::Unrecognized(raw) = &event.payload {
            // Only a frame that named itself a tool call. Anything else this
            // client cannot read is some other part of the conversation, and
            // this turn is not reading the conversation.
            if matches!(
                raw.session_update.as_deref(),
                Some("tool_call" | "tool_call_update")
            ) {
                seen.lost = true;
            }
        }
    }

    fn permission_refused(&self) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.permission_refused = true;
        }
    }

    /// Whether the call a permission is being asked about is on the server this
    /// errand was sent to.
    ///
    /// What is left of the test once an agreement covers a server rather than
    /// one of its tools, and it is left rather than dropped: an agent asks
    /// permission for its own operations too, and *may this screen read your
    /// server* is not *may this turn run a shell command*. So the title still
    /// has to read as a tool, and the server it names still has to be the one
    /// somebody agreed to; which tool of it no longer decides anything, because
    /// there is no answer the agreement gives about one that it does not give
    /// about all of them.
    ///
    /// A request this side cannot read a name out of is not on our server. That
    /// is the same refusal a nameless answer gets, for the same reason: reading
    /// *it must be ours* off the only thing in the room is a guess, and here it
    /// would be a guess spending somebody's agreement.
    ///
    /// A permission is often asked about a call announced in an earlier frame,
    /// and a frame states only what it changes, so the title is taken from the
    /// request where it carries one and from what that call was announced under
    /// where it does not.
    fn permission_is_on_the_agreed_server(&self, about: &schema::ToolCallUpdate) -> bool {
        if let Some(title) = about.fields.title.as_deref() {
            return self.names_our_server(title);
        }
        let Ok(seen) = self.seen.lock() else {
            return false;
        };
        let id = about.tool_call_id.0.to_string();
        seen.calls
            .iter()
            .find(|call| call.id == id)
            .and_then(|call| call.title.as_deref())
            .is_some_and(|title| self.names_our_server(title))
    }

    /// What the turn answered, or why it did not.
    ///
    /// One test decides which call is the one that was asked for, and there is
    /// no second: the call's title, read back through [`McpToolNaming`] against
    /// the server the errand named. A title is the only place a tool's name
    /// reaches this client at all, so a call this side cannot name is a call
    /// this side cannot attribute — and an answer attributed by elimination is
    /// a guess wearing the shape of data. What comes back from here is the
    /// answer of the tool that was asked for, or a refusal; it is never
    /// whatever else the turn happened to run.
    ///
    /// The price is named and accepted. Agents differ over what they write in a
    /// title, and one that writes a sentence for a person to read gets
    /// [`Refusal::NotNamed`] here instead of an answer. That is the cheaper
    /// failure by a distance: a refusal is legible and somebody acts on it,
    /// while a wrong answer fills a panel that then looks like it is working.
    ///
    /// Several calls of the tool that was asked for are not ambiguous while
    /// only one of them answered — an agent that tried, failed and tried again
    /// has one answer between them. Two that answered are, and are refused.
    fn verdict(&self) -> Result<Value, Refusal> {
        let Ok(mut seen) = self.seen.lock() else {
            return Err(Refusal::NotCalled);
        };

        let mut ours = seen
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| self.is_wanted(call) && call.raw_output.is_some())
            .map(|(at, _)| at);
        match (ours.next(), ours.next()) {
            // The one that answered, out of however many calls of it were made.
            (Some(at), None) => return seen.calls[at].raw_output.take().ok_or(Refusal::NoAnswer),
            (Some(_), Some(_)) => return Err(Refusal::SeveralAnswers),
            _ => {}
        }

        let named = seen.calls.iter().any(|call| self.is_wanted(call));
        Err(unanswered(&seen, named))
    }

    /// Whether this call names the tool the errand asked for.
    ///
    /// An answer is attributed to one tool however wide anybody's agreement is:
    /// what may be *called* and what may be *reported back as the answer* are
    /// different questions, and the second one has only ever had one right
    /// answer.
    fn is_wanted(&self, call: &Call) -> bool {
        call.title
            .as_deref()
            .is_some_and(|title| self.names_it(title))
    }

    /// Whether a title, whatever frame it arrived on, names the tool that was
    /// asked for.
    fn names_it(&self, title: &str) -> bool {
        self.named(title).is_some_and(|name| name == self.wanted)
    }

    /// Whether a title names a tool of the server this errand was sent to,
    /// whichever tool that is.
    fn names_our_server(&self, title: &str) -> bool {
        self.named(title).is_some()
    }

    /// The tool a title names, read against the one server this errand knows
    /// about.
    ///
    /// One server rather than every server the person has, and that is what
    /// makes the reading safe: `sync_sync_status` splits into `sync` +
    /// `sync_status` only because a server called `sync` is known to exist, and
    /// guessing the split from the string alone would be a coin toss on every
    /// underscore.
    fn named(&self, title: &str) -> Option<McpToolName> {
        self.naming.parse(title, &[self.wanted.server.as_str()])
    }
}

/// Which refusal a turn with no answer in it deserves.
///
/// Ordered by which fact explains the most, because each one sends somebody
/// somewhere different. A permission that was asked for and refused is why the
/// tool never ran, and saying *it was not called* instead would send them
/// looking at the agent for a decision this side made. A frame this client
/// could not read is a loss here, not a refusal there.
///
/// `named` is whether the tool that was asked for was called at all — by the
/// only test there is for that, its title. It separates the last two: a tool
/// that ran and said nothing is a question for whoever wrote the tool, and a
/// turn full of calls that are nobody's is a question about titles.
fn unanswered(seen: &Seen, named: bool) -> Refusal {
    if seen.permission_refused {
        Refusal::PermissionNeeded
    } else if seen.lost {
        Refusal::AnswerLost
    } else if named {
        Refusal::NoAnswer
    } else if seen.calls.is_empty() {
        Refusal::NotCalled
    } else {
        Refusal::NotNamed
    }
}

/// The client half of an errand: it records, and it answers a permission from
/// what was agreed before the turn began.
struct ErrandHandler {
    watch: Arc<Watch>,
    /// What the caller was told, and the only thing that can turn a *no* here
    /// into a *yes*. Copied in at launch rather than read from anywhere while
    /// the turn runs: an agreement withdrawn mid-turn would otherwise land in
    /// the middle of a call that is already happening.
    consent: Consent,
}

#[async_trait]
impl ClientHandler for ErrandHandler {
    async fn session_update(&self, event: SessionUpdateEvent) {
        self.watch.observe(&event);
    }

    async fn request_permission(
        &self,
        request: schema::RequestPermissionRequest,
    ) -> Result<schema::RequestPermissionResponse, RpcError> {
        // Three things have to hold, and each of them is somebody's decision
        // rather than this handler's: a person agreed in advance, the request
        // is about the server they agreed to, and the agent is willing to be
        // allowed just this once. Any one of them missing is a refusal.
        if self.consent == Consent::Given
            && self
                .watch
                .permission_is_on_the_agreed_server(&request.tool_call)
        {
            if let Some(option_id) = handler::allow_once(&request.options) {
                return Ok(schema::RequestPermissionResponse::new(handler::allowed(
                    option_id,
                )));
            }
        }

        self.watch.permission_refused();
        Ok(schema::RequestPermissionResponse::new(
            handler::narrowest_no(&request.options),
        ))
    }

    async fn read_text_file(
        &self,
        _request: schema::ReadTextFileRequest,
    ) -> Result<schema::ReadTextFileResponse, RpcError> {
        Err(reaches_no_files())
    }

    async fn write_text_file(
        &self,
        _request: schema::WriteTextFileRequest,
    ) -> Result<schema::WriteTextFileResponse, RpcError> {
        Err(reaches_no_files())
    }
}

/// The one answer an errand gives to a request for a file.
///
/// A refusal rather than a silence: an unanswered request leaves the agent
/// waiting for as long as the errand's patience lasts, and it would spend that
/// time believing this side was about to hand it something.
fn reaches_no_files() -> RpcError {
    RpcError::invalid_params("this turn fetches one tool's answer and reaches no files")
}

/// Runs one errand against a fresh process, and answers with what the tool
/// returned.
///
/// The process is raised for this errand and killed when it ends, however it
/// ends. Nothing is kept: an agent held open between errands is a decision
/// about cost and staleness, and it is not this function's to take.
///
/// # Errors
///
/// [`ErrandError::Connection`] when the agent cannot be raised, refuses a
/// control request or dies; [`ErrandError::Refused`] when the turn ran and
/// produced no answer this client may report — see [`Refusal`] for which of
/// those happened.
pub async fn run(
    spec: &'static AgentLaunchSpec,
    options: &SpawnOptions,
    errand: &Errand,
) -> Result<Value, ErrandError> {
    let naming = spec.tool_naming.ok_or(Refusal::NamingUnknown)?;
    let watch = Arc::new(Watch::new(naming, errand.tool.clone()));

    let command = launch::command_for(spec, options);
    let mut process = launch::spawn(
        command,
        ErrandHandler {
            watch: Arc::clone(&watch),
            consent: errand.consent,
        },
    )?;

    let ran =
        tokio::time::timeout(errand.patience, turn(process.connection(), errand, naming)).await;
    // Before the verdict, and whatever the verdict turns out to be. The tool
    // has already answered or already has not; keeping the process alive past
    // that point only leaves one behind that nobody will ever look at.
    let _ = process.kill().await;

    match ran {
        Ok(Err(error)) => Err(error.into()),
        // A turn that ran to its end, and one that outran its patience, are
        // read the same way: the tool either answered or it did not, and an
        // answer that arrived before the deadline is not made worse by the
        // agent's failure to stop talking afterwards. What differs is the
        // refusal when there is no answer — that one names the deadline,
        // because *nothing came back* and *it never finished* send somebody
        // to different places.
        Ok(Ok(())) => watch.verdict().map_err(ErrandError::Refused),
        Err(_elapsed) => watch
            .verdict()
            .map_err(|_| ErrandError::Refused(Refusal::Overran(errand.patience))),
    }
}

/// `initialize`, `session/new`, one prompt. Split out so the whole of it can be
/// put under one deadline.
async fn turn(
    connection: &crate::connection::AgentConnection,
    errand: &Errand,
    naming: McpToolNaming,
) -> Result<(), Error> {
    connection
        .initialize(schema::InitializeRequest::new(
            crate::SUPPORTED_PROTOCOL_VERSION,
        ))
        .await?;

    // No `mcpServers`: the tool being called is one the agent already has, out
    // of its own configuration. Handing it a list here would be Sync serving
    // servers of its own, which is the thing this whole door exists not to do.
    let session = connection
        .new_session(schema::NewSessionRequest::new(errand.cwd.clone()))
        .await?;

    connection
        .prompt(schema::PromptRequest::new(
            session.session_id,
            vec![schema::ContentBlock::Text(schema::TextContent::new(
                errand.instruction(naming),
            ))],
        ))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn watching() -> Watch {
        Watch::new(
            McpToolNaming::Slash,
            McpToolName::new("sync", "sync_status"),
        )
    }

    fn event(update: &Value) -> SessionUpdateEvent {
        crate::update::decode_session_update(json!({ "sessionId": "s-1", "update": update }))
            .expect("the envelope carries a sessionId")
    }

    fn announced(title: &str) -> Value {
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "title": title,
            "kind": "fetch",
        })
    }

    fn finished(id: &str, raw_output: &Value) -> Value {
        json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": "completed",
            "rawOutput": raw_output,
        })
    }

    #[test]
    fn the_tools_own_answer_is_what_comes_back() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));
        assert_eq!(watch.verdict(), Ok(json!({ "up": true })));
    }

    /// A call whose title is a sentence for a person is a call this side cannot
    /// name, and a tool's answer is only ever returned under its own name.
    /// Handing this one back would be attribution by elimination — the door
    /// would be saying *this must be it* about the only thing in the room.
    #[test]
    fn a_call_this_side_cannot_name_is_refused_rather_than_attributed() {
        let watch = watching();
        watch.observe(&event(&announced("Checking the server")));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));
        assert_eq!(watch.verdict(), Err(Refusal::NotNamed));
    }

    /// And the refusal separates itself from the turn that ran nothing at all.
    /// One sends somebody to an agent that ignored its instruction, the other
    /// to a title this build cannot read as a name.
    #[test]
    fn calling_something_unnameable_is_not_the_same_as_calling_nothing() {
        let watch = watching();
        watch.observe(&event(&announced("Checking the server")));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));
        assert_eq!(watch.verdict().unwrap_err().code(), "tool_never_named");

        let silent = watching();
        silent.observe(&event(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "All good." },
        })));
        assert_eq!(silent.verdict().unwrap_err().code(), "tool_not_called");
    }

    /// A turn that also ran the agent's own tool answers with ours, whichever
    /// of them came back first. Order decides nothing here; the name does.
    #[test]
    fn only_the_tool_that_was_asked_for_answers_for_the_turn() {
        let watch = watching();
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-own",
            "title": "read_file",
            "kind": "read",
        })));
        watch.observe(&event(&finished(
            "call-own",
            &json!({ "text": "unrelated" }),
        )));
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));
        assert_eq!(watch.verdict(), Ok(json!({ "up": true })));
    }

    /// An agent that called the tool, got nothing, and called it again has one
    /// answer between the two — and it is the second. Reading the first call
    /// and stopping there reports a tool that answered as one that did not.
    #[test]
    fn a_tool_called_twice_answers_with_the_call_that_answered() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "failed",
        })));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-2",
            "title": "sync/sync_status",
            "kind": "fetch",
        })));
        watch.observe(&event(&finished("call-2", &json!({ "up": true }))));
        assert_eq!(watch.verdict(), Ok(json!({ "up": true })));
    }

    /// And the tool that was asked for, called and silent, is not answered for
    /// by whatever else the agent ran in the same turn.
    #[test]
    fn another_tools_answer_does_not_stand_in_for_the_one_asked_for() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-own",
            "title": "read_file",
            "kind": "read",
        })));
        watch.observe(&event(&finished(
            "call-own",
            &json!({ "text": "unrelated" }),
        )));
        assert_eq!(watch.verdict(), Err(Refusal::NoAnswer));
    }

    /// Two calls this side cannot name, both answering, and neither of them is
    /// reported: not one of them said it was the tool that was asked for.
    #[test]
    fn two_calls_that_cannot_be_named_answer_for_nobody() {
        let watch = watching();
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-a",
            "title": "Looking something up",
            "kind": "fetch",
        })));
        watch.observe(&event(&finished("call-a", &json!({ "first": true }))));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-b",
            "title": "Looking again",
            "kind": "fetch",
        })));
        watch.observe(&event(&finished("call-b", &json!({ "second": true }))));
        assert_eq!(watch.verdict(), Err(Refusal::NotNamed));
    }

    /// The tool that was asked for, answering twice. This is the one shape
    /// where there are two answers with an equal claim to being the answer, and
    /// there is nothing to choose between them but the order they arrived in —
    /// which is not a reason.
    #[test]
    fn a_tool_that_answered_twice_is_refused_rather_than_picked_between() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-2",
            "title": "sync/sync_status",
            "kind": "fetch",
        })));
        watch.observe(&event(&finished("call-2", &json!({ "up": false }))));
        assert_eq!(watch.verdict(), Err(Refusal::SeveralAnswers));
    }

    /// The failure this whole door is built against: the agent says the answer
    /// in words and calls nothing. Words are not an answer, and a panel filled
    /// from them looks like it is working.
    #[test]
    fn prose_alone_is_not_an_answer() {
        let watch = watching();
        watch.observe(&event(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Everything is up and healthy." },
        })));
        assert_eq!(watch.verdict(), Err(Refusal::NotCalled));
    }

    #[test]
    fn a_call_that_returned_nothing_is_not_a_call_that_never_happened() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "completed",
        })));
        assert_eq!(watch.verdict(), Err(Refusal::NoAnswer));
    }

    /// An answer this client could not read is a loss on this side. Reported as
    /// *not called*, it would be a lie about the agent — and somebody would go
    /// looking at the agent for it.
    #[test]
    fn an_unreadable_tool_call_frame_is_a_loss_and_says_so() {
        let watch = watching();
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "rawOutput": { "up": true },
        })));
        assert_eq!(watch.verdict(), Err(Refusal::AnswerLost));
    }

    #[test]
    fn a_permission_nobody_could_give_is_why_the_tool_never_ran() {
        let watch = watching();
        watch.permission_refused();
        watch.observe(&event(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "I need approval to do that." },
        })));
        assert_eq!(watch.verdict(), Err(Refusal::PermissionNeeded));
    }

    /// An amendment states only what changed. A later frame carrying no title
    /// must not erase the one that named the tool, or the answer stops being
    /// recognisable as the one that was asked for.
    #[test]
    fn a_later_frame_does_not_erase_what_an_earlier_one_said() {
        let watch = watching();
        watch.observe(&event(&announced("sync/sync_status")));
        watch.observe(&event(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "in_progress",
        })));
        watch.observe(&event(&finished("call-1", &json!({ "up": true }))));

        let seen = watch.seen.lock().expect("nothing poisoned the lock");
        assert_eq!(seen.calls.len(), 1, "one call, amended twice");
        assert_eq!(seen.calls[0].title.as_deref(), Some("sync/sync_status"));
    }

    /// The instruction carries the spelling *this* agent uses. Rendered with
    /// the wrong naming it names a tool the agent does not have, and the turn
    /// comes back as `NotCalled` with nothing anywhere saying why.
    #[test]
    fn the_instruction_names_the_tool_the_way_that_agent_spells_it() {
        let errand = Errand::new(
            PathBuf::from("/tmp"),
            McpToolName::new("sync", "sync_status"),
            json!({ "verbose": true }),
        );
        let said = errand.instruction(McpToolNaming::McpDoubleUnderscore);
        assert!(said.contains("`mcp__sync__sync_status`"), "{said}");
        assert!(said.contains("\"verbose\": true"), "{said}");
    }

    /// A permission request off the wire, about `about`, offering all three
    /// answers an agent can offer.
    ///
    /// Built by deserializing rather than by construction, for the reason the
    /// session updates above are: what this side has to read is JSON somebody
    /// else wrote, and a value assembled in Rust would prove only that this
    /// test can build one.
    fn permission(about: &Value) -> schema::RequestPermissionRequest {
        serde_json::from_value(json!({
            "sessionId": "s-1",
            "toolCall": about.clone(),
            "options": [
                { "optionId": "no", "name": "Don't allow", "kind": "reject_once" },
                { "optionId": "once", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "forever", "name": "Always allow", "kind": "allow_always" },
            ],
        }))
        .expect("a permission request as an agent sends it")
    }

    /// Which option was taken, or `None` for a refusal.
    fn taken(answer: &schema::RequestPermissionResponse) -> Option<String> {
        match &answer.outcome {
            schema::RequestPermissionOutcome::Selected(selected) => {
                Some(selected.option_id.0.to_string())
            }
            _ => None,
        }
    }

    fn handling(consent: Consent) -> ErrandHandler {
        ErrandHandler {
            watch: Arc::new(watching()),
            consent,
        }
    }

    /// The whole of what this task changed: a call to a server somebody agreed
    /// to no longer dies at the permission it was always going to be asked for.
    #[tokio::test]
    async fn a_tool_of_a_server_that_was_agreed_to_is_allowed() {
        let handler = handling(Consent::Given);
        let answer = handler
            .request_permission(permission(&json!({
                "toolCallId": "call-1",
                "title": "sync/sync_status",
                "kind": "fetch",
            })))
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("once"));
    }

    /// And allowed *once*, with the forever offered right beside it. Taking
    /// that one would leave a decision in the agent's own store that the
    /// settings screen offering to withdraw it cannot reach.
    #[tokio::test]
    async fn an_agreement_is_never_spent_on_a_forever() {
        let handler = handling(Consent::Given);
        let only_forever: schema::RequestPermissionRequest = serde_json::from_value(json!({
            "sessionId": "s-1",
            "toolCall": { "toolCallId": "call-1", "title": "sync/sync_status", "kind": "fetch" },
            "options": [
                { "optionId": "no", "name": "Don't allow", "kind": "reject_once" },
                { "optionId": "forever", "name": "Always allow", "kind": "allow_always" },
            ],
        }))
        .expect("an agent that only says yes forever");

        let answer = handler
            .request_permission(only_forever)
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("no"));
        assert_eq!(handler.watch.verdict(), Err(Refusal::PermissionNeeded));
    }

    /// The agreement is about a server, so a neighbouring tool of that server is
    /// inside it — which is the whole of what changed when the unit widened.
    #[tokio::test]
    async fn an_agreement_covers_every_tool_of_the_server_it_was_given_for() {
        let handler = handling(Consent::Given);
        let answer = handler
            .request_permission(permission(&json!({
                "toolCallId": "call-2",
                "title": "sync/sync_apply",
                "kind": "edit",
            })))
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("once"));
    }

    /// And it stops at that server. The agent's own operations are not on it,
    /// and neither is somebody else's server: an agreement about one thing a
    /// person configured says nothing about the next thing along.
    #[tokio::test]
    async fn an_agreement_stops_at_the_server_it_was_given_for() {
        for title in ["shell", "tracker/list_issues"] {
            let handler = handling(Consent::Given);
            let answer = handler
                .request_permission(permission(&json!({
                    "toolCallId": "call-2",
                    "title": title,
                    "kind": "execute",
                })))
                .await
                .expect("answering a permission is not a failure");
            assert_eq!(taken(&answer).as_deref(), Some("no"), "{title}");
            assert_eq!(handler.watch.verdict(), Err(Refusal::PermissionNeeded));
        }
    }

    /// A request carrying no title is about a call that was announced earlier,
    /// and the announcement is where its name is. Read only the request and an
    /// agreement stops working against every agent that amends rather than
    /// repeats.
    #[tokio::test]
    async fn a_permission_about_a_call_announced_earlier_is_still_named() {
        let handler = handling(Consent::Given);
        handler
            .watch
            .observe(&event(&announced("sync/sync_status")));

        let answer = handler
            .request_permission(permission(&json!({ "toolCallId": "call-1" })))
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("once"));
    }

    /// And a request about a call nobody named is not on the agreed server by
    /// default. Attribution by elimination is refused here for the same reason
    /// it is refused of an answer.
    #[tokio::test]
    async fn a_permission_this_side_cannot_name_is_not_on_the_agreed_server() {
        let handler = handling(Consent::Given);
        let answer = handler
            .request_permission(permission(&json!({ "toolCallId": "call-unknown" })))
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("no"));
    }

    /// An errand nobody agreed to answers exactly as it did before there was
    /// anything to agree with — including for the tool it was sent to fetch.
    #[tokio::test]
    async fn without_an_agreement_the_asked_tool_is_refused_too() {
        let handler = handling(Consent::Withheld);
        let answer = handler
            .request_permission(permission(&json!({
                "toolCallId": "call-1",
                "title": "sync/sync_status",
                "kind": "fetch",
            })))
            .await
            .expect("answering a permission is not a failure");
        assert_eq!(taken(&answer).as_deref(), Some("no"));
        assert_eq!(handler.watch.verdict(), Err(Refusal::PermissionNeeded));
    }

    /// A permission that was *given* explains nothing about a turn that then
    /// answered — and, given and answered, must not leave the turn reported as
    /// one nobody could allow.
    #[tokio::test]
    async fn a_permission_that_was_given_is_not_why_anything_failed() {
        let handler = handling(Consent::Given);
        handler
            .watch
            .observe(&event(&announced("sync/sync_status")));
        let _ = handler
            .request_permission(permission(&json!({ "toolCallId": "call-1" })))
            .await;
        handler
            .watch
            .observe(&event(&finished("call-1", &json!({ "up": true }))));
        assert_eq!(handler.watch.verdict(), Ok(json!({ "up": true })));
    }

    /// An errand is not born consenting: a caller that never heard of the
    /// question gives nothing away.
    #[test]
    fn an_errand_agrees_to_nothing_until_it_is_told_otherwise() {
        let errand = Errand::new(
            PathBuf::from("/tmp"),
            McpToolName::new("sync", "sync_status"),
            json!({}),
        );
        assert_eq!(errand.consent, Consent::Withheld);
    }

    /// Every refusal has to be tellable from every other one by a caller that
    /// only branches on the code.
    #[test]
    fn no_two_refusals_share_a_code() {
        let all = [
            Refusal::NotCalled,
            Refusal::PermissionNeeded,
            Refusal::AnswerLost,
            Refusal::NoAnswer,
            Refusal::NotNamed,
            Refusal::SeveralAnswers,
            Refusal::NamingUnknown,
            Refusal::Overran(DEFAULT_PATIENCE),
        ];
        let mut codes: Vec<&str> = all.iter().map(Refusal::code).collect();
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "two refusals answer to one code");
        assert!(all.iter().all(|refusal| !refusal.to_string().is_empty()));
    }
}
