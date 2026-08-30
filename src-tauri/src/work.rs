//! Work a handler ordered, and the host that performs it.
//!
//! A handler runs for milliseconds and may order something that runs for hours
//! (`docs/background.md` §2). It cannot perform that itself — its isolate is
//! gone the moment it answers — so it orders, and this is what outlives it.
//!
//! # What is written down, and when
//!
//! An order is written **before** anything is raised, with no session attached,
//! and the session is attached afterwards. There is no transaction across the
//! two files, so the order is chosen to make every surviving state an ordinary
//! one:
//!
//! - order written, nothing raised — *ordered at 03:12, never started*, which
//!   is exactly what somebody needs to see the morning after.
//! - pointer written in `conversations.json`, order not yet updated — a
//!   conversation this machine can resume, which is what that file always
//!   means.
//!
//! The state the order forbids is the third one: an order naming an
//! `acpSession` that no pointer knows about. That is why the pointer is written
//! first — a session's only durable identity is the agent's own id for it
//! ([`crate::sessions::remembered`]), and referring to one before it exists
//! would be referring to nothing.
//!
//! # Why it is its own file
//!
//! It holds two facts nothing else can: **who ordered it** (§6.3, set once and
//! never edited) and **what to do if it is interrupted** (§6.4, the orderer's
//! choice rather than a system default). Neither can live on a conversation
//! pointer, because an order exists before there is a conversation and goes on
//! existing for work whose agent never rose — which is the case somebody most
//! needs an account of.
//!
//! # What is deliberately not here
//!
//! A status. What a session is doing is the session's own answer and it is
//! already asked and answered every few seconds ([`crate::sessions::SessionRow`]);
//! a copy of it here would be a second truth to keep in step, and it would go
//! stale in the file the moment the process ended. What *happened* — the account
//! of a run nobody watched — is the journal's, and the journal is step 7.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::project::{ProjectError, configuration_file, write_configuration};
use crate::sessions::event::now_ms;
use crate::sessions::live::{About, Source};

/// The file, in this installation's configuration directory.
const FILE: &str = "ordered-work.json";

/// How many orders are kept per project.
///
/// The same bound as the conversation pointers next door, for a sharper reason:
/// a handler on a one-minute clock can order work sixty times an hour, and
/// [`write_configuration`] writes whole files. What is kept is what somebody
/// could still be asking about; the oldest go first.
const PER_PROJECT_LIMIT: usize = 100;

/// The one kind of work there is.
///
/// A registry with one entry, and a second arrives with a second executor
/// rather than before it (`docs/background.md` §6.2). A registry designed
/// around one known case and four imagined ones is four guesses shipped as a
/// contract.
const KIND: &str = "agent.session";

/// Serialises this process's read-modify-writes of [`FILE`], and mints keys.
///
/// The lock is over the read-then-write rather than over the data, exactly as
/// [`crate::schedule::ScheduleFile`] is: the file stays the one truth and
/// nothing has to be kept in step with it. The counter is here because it is
/// the other thing that must not be handed out twice.
#[derive(Default)]
pub struct WorkFile {
    guard: Mutex<()>,
    minted: AtomicU64,
}

impl WorkFile {
    /// A key no other order has had, on this machine, ever.
    ///
    /// The clock is in it because the counter restarts with the process and the
    /// file does not; the counter is in it because two orders can fall inside
    /// one millisecond. Neither alone is enough and together they are, without
    /// a dependency for the sake of prettier characters.
    fn mint(&self) -> String {
        format!(
            "w{}-{}",
            now_ms(),
            self.minted.fetch_add(1, Ordering::SeqCst)
        )
    }
}

/// What to do with work a shutdown interrupted.
///
/// **The orderer's choice, made when the work is ordered** (§6.4). The two
/// cases genuinely differ — a nightly poll should finish without anybody, a
/// conversation a person started is theirs to pick up — and no default is right
/// for both, so there is no default and the field is required.
///
/// It is recorded here and honoured later. Recording it now is not optional:
/// this is the only moment the choice exists to be captured, and a field made
/// required after the first package ships breaks that package.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnInterrupted {
    /// Resume it and carry on, with or without anybody there.
    Continue,
    /// Leave it resumable and say so.
    Wait,
}

/// How many of the conversations one order's work produces are kept.
///
/// **The orderer's choice, like [`OnInterrupted`], and for the same reason:**
/// nothing else can know it. An extension that opens a conversation per issue
/// wants all of them; one that carries out a standing instruction on a clock
/// wants the last one, because the alternative is ninety-six rows a day for a
/// routine on fifteen minutes — which fills the hundred pointers a project
/// keeps within a day and starts pushing out the person's own conversations.
///
/// Unlike `OnInterrupted` this has a default, and the default is what every
/// package already built does. A required field added after the first package
/// ships breaks that package; an optional one whose absence means *what
/// happened before* breaks nothing.
///
/// What is deliberately not offered is a number — *keep the last five*. Five is
/// a guess, it would need a per-slot count kept somewhere, and the honest unit
/// for "what happened on the runs before this one" is the journal
/// (`docs/background.md` §8), not a list of live conversations.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Keep {
    /// Every run is its own conversation. What a package gets by saying nothing.
    #[default]
    Each,
    /// One conversation about this record at a time. The run that starts
    /// replaces the one before it.
    Latest,
}

/// One piece of work, as it was ordered and as it turned out.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Ordered {
    /// What [`order`] answered the handler with.
    pub key: String,
    /// Always [`KIND`] today. Written down rather than assumed, because the day
    /// there is a second one this file will already have said which is which.
    pub kind: String,
    /// Which agent was asked for, as [`crate::sessions::catalog`] names them.
    pub agent: String,
    /// What the conversation is called, as the package named it.
    pub title: String,
    /// The extension whose handler ordered it, and what it was called.
    ///
    /// Flat, rather than a [`Source`] nested here, because a `Source` names the
    /// order it belongs to and [`Self::key`] is already that name — nesting one
    /// would write the same string into this file twice. The wire shape is
    /// assembled by [`Self::source`], which is the one place that says what a
    /// source is made of.
    pub extension_id: String,
    pub extension_name: String,
    pub handler: String,
    /// What it was about, when the orderer named a record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<AboutOrder>,
    pub on_interrupted: OnInterrupted,
    /// Whether this order's conversation stands beside the ones before it or in
    /// place of them.
    #[serde(default)]
    pub keep: Keep,
    pub ordered_at_ms: u64,
    /// The agent's own id for the session, once there is one.
    ///
    /// `None` is not a failure and not a state to clean up: it is *ordered and
    /// not yet started*, and it stays that way for work whose agent never rose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_session: Option<String>,
}

impl Ordered {
    /// This order, in the shape a session carries and a row crosses with.
    #[must_use]
    pub fn source(&self) -> Source {
        Source {
            work: self.key.clone(),
            extension_id: self.extension_id.clone(),
            extension_name: self.extension_name.clone(),
            handler: self.handler.clone(),
            about: self.about.as_ref().map(|about| about.key().to_owned()),
        }
    }
}

/// What an order says it is about, in either spelling a package may use.
///
/// A record is what a list groups by, so a heading has to be drawable from what
/// the order carried: the key alone is an address, and an address is not
/// something a person can read or an area can open — opening one takes the kind
/// as well, because an area lists records by type and cannot find out which of
/// its own lists a key belongs in without reading the record first.
///
/// The bare key is the older spelling and is still read, both from packages
/// built against it and from orders already written to this machine's file. It
/// says which slot the work belongs to and nothing else, so a conversation
/// ordered with one carries no heading — that is the whole of what the newer
/// spelling buys, and it is why the older one is accepted rather than refused:
/// a package that has not been rebuilt goes on working.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AboutOrder {
    /// The record, in full: what a heading says and what opening it resolves.
    Record(About),
    /// Its key alone.
    Key(String),
}

impl AboutOrder {
    /// The key, which is what a slot is named by whichever spelling was used.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Record(record) => &record.key,
            Self::Key(key) => key,
        }
    }

    /// The record a heading can be drawn from, when the order carried one.
    #[must_use]
    pub fn record(&self) -> Option<About> {
        match self {
            Self::Record(record) => Some(record.clone()),
            Self::Key(_) => None,
        }
    }
}

/// Every order this machine holds, by the project it was ordered for.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Store {
    projects: BTreeMap<String, Vec<Ordered>>,
}

impl Store {
    /// Reads the file, answering with an empty store when there is none.
    ///
    /// An unreadable file is absent rather than fatal, for the reason the
    /// conversation pointers are: what it costs is an account of work already
    /// done, and refusing to run over it would cost the work itself.
    #[must_use]
    fn read<R: Runtime>(app: &AppHandle<R>) -> Self {
        configuration_file(app, FILE)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), ProjectError> {
        write_configuration(&configuration_file(app, FILE)?, self)
    }

    /// Writes down one order, dropping the oldest when the list stops being one.
    fn keep(&mut self, project: &str, entry: Ordered) {
        let held = self.projects.entry(project.to_owned()).or_default();
        held.push(entry);
        if held.len() > PER_PROJECT_LIMIT {
            held.sort_by_key(|entry| std::cmp::Reverse(entry.ordered_at_ms));
            held.truncate(PER_PROJECT_LIMIT);
        }
    }

    /// Attaches the session an order turned into.
    ///
    /// Answers whether there was an order to attach it to. There may not be:
    /// this machine's hundredth order in a busy project can push the first one
    /// out before its agent has finished rising, and losing the account of a
    /// piece of work is not a reason to abandon the work.
    fn began(&mut self, project: &str, key: &str, acp_session: &str) -> bool {
        let Some(held) = self.projects.get_mut(project) else {
            return false;
        };
        let Some(entry) = held.iter_mut().find(|entry| entry.key == key) else {
            return false;
        };
        entry.acp_session = Some(acp_session.to_owned());
        true
    }

    /// Takes this slot's earlier orders out, and answers with the conversations
    /// they became.
    ///
    /// **A slot is one package and one record, and deliberately not the
    /// handler.** A routine moved from every fifteen minutes to every hour is
    /// carried out by a different handler and is the same routine; keyed on the
    /// handler, yesterday's conversation would stand beside today's for ever
    /// with nothing able to tell they were the same thing.
    ///
    /// `spared` names the conversations that are not this package's to remove —
    /// see [`supersede`].
    fn supersede(
        &mut self,
        project: &str,
        extension_id: &str,
        about: &str,
        keeping: &str,
        spared: &std::collections::HashSet<String>,
    ) -> Vec<String> {
        let Some(held) = self.projects.get_mut(project) else {
            return Vec::new();
        };
        let mut going = Vec::new();
        held.retain(|entry| {
            let mine = entry.extension_id == extension_id
                && entry.about.as_ref().map(AboutOrder::key) == Some(about)
                && entry.key != keeping;
            if !mine {
                return true;
            }
            match entry.acp_session.as_deref() {
                // Spared, so its order stays with it: the two files agreeing is
                // what makes either of them readable.
                Some(acp) if spared.contains(acp) => true,
                Some(acp) => {
                    going.push(acp.to_owned());
                    false
                }
                // Ordered and never started. There is no conversation to
                // replace and nothing to read, so it goes with the rest of the
                // slot rather than accumulating one row per failed launch.
                None => false,
            }
        });
        going
    }

    /// The orders of one project, most recently ordered first.
    #[must_use]
    pub fn of_project(&self, project: &str) -> Vec<Ordered> {
        let mut held = self.projects.get(project).cloned().unwrap_or_default();
        held.sort_by_key(|entry| std::cmp::Reverse(entry.ordered_at_ms));
        held
    }

    fn forget(&mut self, project: &str) {
        self.projects.remove(project);
    }
}

/// Stop holding a project's orders. Called where a project is forgotten.
///
/// Quiet, like the schedule's: forgetting a project is a gesture that has
/// already taken it out of the menu and the registry, and a failure to tidy
/// this file is not a reason to tell somebody it did not happen.
pub(crate) fn forget<R: Runtime>(app: &AppHandle<R>, work: &WorkFile, project: &str) {
    let Ok(_held) = work.guard.lock() else { return };
    let mut store = Store::read(app);
    store.forget(project);
    let _ = store.write(app);
}

/// The package that is talking, at the length an order needs it.
///
/// Two borrowed strings rather than the whole manifest: what an order records
/// about a package is its id and its name, and handing this the manifest would
/// invite it to record something else later without anybody deciding to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Package<'a> {
    pub id: &'a str,
    pub name: &'a str,
}

/// What a handler asked for.
///
/// Parsed from the payload it passed, and every field of it is the orderer's.
/// What the *host* knows is not here: which project this is, and which package
/// is talking, both of which arrive from the call rather than from the package
/// — the same division that makes a scheduled row's sentence two authors'.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub kind: String,
    pub agent: String,
    /// What to call the conversation, in a list a person reads.
    ///
    /// **Required**, and required for the same reason a scheduled handler must
    /// say what it does: nothing else can know it. Without one the title is
    /// derived from the first words said — which a handler wrote, to an agent.
    /// A sentence written *for an agent* standing in for a sentence written
    /// *for a list* reads exactly like something a person typed, which is the
    /// one thing the list must not say.
    ///
    /// It is a name, not a lock: `session_rename` is still there, and somebody
    /// who does not like it can change it.
    pub title: String,
    pub prompt: Prompt,
    pub on_interrupted: OnInterrupted,
    #[serde(default)]
    pub about: Option<AboutOrder>,
    /// Whether to keep every conversation this handler orders, or only the most
    /// recent one about the same record. Absent is [`Keep::Each`].
    #[serde(default)]
    pub keep: Keep,
}

/// What the agent is to be asked, in the shape the session layer already takes.
///
/// `text` and `attachments`, and no images: a handler has no clipboard and no
/// filesystem, so there is no way for one to hold a picture. The window's own
/// prompt carries them and this one has nothing to carry.
///
/// `attachments` are absolute paths and cross to the agent as resource links,
/// which is the one attachment shape every agent must accept — Sync never opens
/// the file, it names one, and the agent is already running in the folder.
#[derive(Debug, Default, Deserialize)]
pub struct Prompt {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<String>,
}

/// Order a piece of work, and answer with its key before any of it has happened.
///
/// This is the whole of the division in §2: the handler that called this is
/// finished within milliseconds and the agent it asked for may run for hours.
/// What is synchronous here is what has to be — minting the key and writing the
/// order down — and raising the agent goes onto the application's runtime,
/// which outlives every isolate.
///
/// # Errors
///
/// When the order names a kind or an agent this build does not have, or when
/// this installation's configuration cannot be written. A refusal is a sentence
/// the handler can catch, and it names what *is* available rather than only
/// what is not.
pub(crate) fn order<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    extension: Package<'_>,
    handler: &str,
    mut order: Order,
) -> Result<String, String> {
    if order.kind != KIND {
        return Err(format!(
            "`{}` is not a kind of work this build performs — it performs `{KIND}`",
            order.kind
        ));
    }
    // Refused here rather than when the agent is raised, because here is where
    // somebody can still be told: a handler catches this, and a handler that
    // ran at three in the morning against a name nobody has would otherwise
    // fail into the error stream and nowhere else.
    if crate::sessions::catalog::spec(&order.agent).is_none() {
        let known: Vec<String> = crate::sessions::catalog::descriptors()
            .into_iter()
            .map(|agent| agent.id)
            .collect();
        return Err(format!(
            "`{}` is not an agent this build knows — it knows {}",
            order.agent,
            known.join(", ")
        ));
    }

    // `latest` is "the most recent one about the same thing", so there has to be
    // a thing. Refused here rather than ignored, because a package that asked
    // for one conversation and silently got ninety-six would find out from the
    // list a day later.
    if order.keep == Keep::Latest
        && !order
            .about
            .as_ref()
            .is_some_and(|about| !about.key().trim().is_empty())
    {
        return Err(
            "`keep: \"latest\"` needs `about`: it keeps the most recent conversation about one record, and without a record named there is nothing for it to be the latest of"
                .to_owned(),
        );
    }

    if order.title.trim().is_empty() {
        return Err(
            "an order needs a title: it is what the conversation is called in a list a person reads, and nothing but the package that ordered it can know what to call it"
                .to_owned(),
        );
    }

    let work = app.state::<WorkFile>();
    let key = work.mint();
    let entry = Ordered {
        key: key.clone(),
        kind: std::mem::take(&mut order.kind),
        agent: order.agent.clone(),
        title: std::mem::take(&mut order.title),
        extension_id: extension.id.to_owned(),
        extension_name: extension.name.to_owned(),
        handler: handler.to_owned(),
        about: order.about.take(),
        on_interrupted: order.on_interrupted,
        keep: order.keep,
        ordered_at_ms: now_ms(),
        acp_session: None,
    };
    // Taken before the entry is given away, and it is the only place a source
    // is composed: what one is made of is this module's question, and a second
    // composition somewhere else would answer it differently next year.
    let source = entry.source();
    let about = entry.about.clone();
    let title = entry.title.clone();
    {
        let _held = work
            .guard
            .lock()
            .map_err(|_| "the ordered-work file's lock is poisoned".to_owned())?;
        let mut store = Store::read(app);
        store.keep(project, entry);
        // Reported rather than swallowed, and it is the one failure here that
        // stops the work: an order nothing wrote down is work nobody could
        // account for afterwards, which is precisely what this file is for.
        store.write(app).map_err(|error| error.message)?;
    }

    let raising = app.clone();
    let project = project.to_owned();
    let started = key.clone();
    let keep = order.keep;
    tauri::async_runtime::spawn(async move {
        if let Err(error) = perform(
            &raising,
            &project,
            &started,
            Run {
                agent: order.agent,
                title,
                prompt: order.prompt,
                source,
                about,
                keep,
            },
        )
        .await
        {
            // The only account of this today, and it is the clock's answer one
            // step on: nobody is in front of it by definition, and the journal
            // that will hold it is step 7. The key is in front of the sentence
            // because it is what the file can be looked up by.
            eprintln!("work {started}: {}", error.message);
        }
    });
    Ok(key)
}

/// One run, as it was ordered: what [`perform`] needs and nothing the store
/// keeps.
///
/// A struct rather than five more parameters. The list had grown past what can
/// be read at a call site, and every one of these is a fact about the same
/// thing — this run — so naming it once is what the arguments were spelling out.
struct Run {
    agent: String,
    title: String,
    prompt: Prompt,
    source: Source,
    /// The record this run is under, which the source no longer carries: who
    /// asked and what it is about are two questions, and the session holds them
    /// as two fields.
    about: Option<AboutOrder>,
    keep: Keep,
}

/// Raise the agent and say the first thing to it.
///
/// The order is the one the module documentation argues for: the session's
/// pointer is written by the prompt, and only then does the order learn which
/// session it became.
async fn perform<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    key: &str,
    run: Run,
) -> Result<(), ProjectError> {
    let Run {
        agent,
        title,
        prompt,
        source,
        about,
        keep,
    } = run;
    let agent = agent.as_str();
    let cwd = std::path::PathBuf::from(project);
    // Which slot this run belongs to, taken before the source is given to the
    // session. Two borrowed strings rather than a clone of the whole source: it
    // is the pair the slot is named by and nothing else here needs the rest.
    let slot = (
        source.extension_id.clone(),
        about.as_ref().map(|about| about.key().to_owned()),
    );
    // The session carries both from the moment it exists, so a window that
    // opens while the agent is still rising already sees whose it is and which
    // record it belongs under.
    let session = crate::sessions::raise_for_work(
        app,
        agent,
        &cwd,
        source,
        about.and_then(|about| about.record()),
    )
    .await?;
    // Before the turn, because saying something is what writes the pointer, and
    // the pointer records the title. Set afterwards, the name would be right in
    // this run's list and wrong in every later one.
    session.set_title(&title);
    crate::sessions::send(
        app,
        &session,
        crate::sessions::Turn {
            text: prompt.text,
            attachments: prompt.attachments,
            ..crate::sessions::Turn::default()
        },
    )?;

    // After the prompt, because the prompt is what writes the pointer this
    // refers to. A session with nothing said in it is not a conversation and
    // has no pointer — `sessions::remember` says so — so attaching its id
    // before this point would name something that is not written down.
    let Some(acp_session) = session.acp_session() else {
        return Ok(());
    };
    let work = app.state::<WorkFile>();
    let Ok(_held) = work.guard.lock() else {
        return Ok(());
    };
    let mut store = Store::read(app);
    if store.began(project, key, acp_session.0.as_ref()) {
        let _ = store.write(app);
    }
    drop(_held);

    // Last, and after everything about this run is written down. The run that
    // replaces yesterday's has to exist before yesterday's is taken away —
    // interrupted here, the cost is one extra row, and interrupted the other way
    // round it would be an account of a routine that had been removed and not
    // yet rewritten.
    if let (Keep::Latest, (extension_id, Some(about))) = (keep, (&slot.0, slot.1.as_deref())) {
        supersede(app, project, key, extension_id, about).await;
    }
    Ok(())
}

/// Leaves this slot holding one conversation: the one that has just started.
///
/// **What a person did outranks what a package arranged.** A conversation kept
/// as a record is a decision somebody made about that conversation — it has a
/// record in the project's memory pointing at it — so it is spared, and its
/// order is spared with it. Everything else in the slot is ended and forgotten:
/// the pointer, so the row leaves the list, and the agent, if one is still up in
/// this run.
///
/// Quiet throughout. Every failure here costs a row that should have gone, and
/// none of them is a reason to interfere with work that has already started.
async fn supersede<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    key: &str,
    extension_id: &str,
    about: &str,
) {
    let path = crate::project::configuration_file(app, crate::sessions::remembered::FILE).ok();
    let spared: std::collections::HashSet<String> = path
        .as_ref()
        .map(|path| {
            crate::sessions::remembered::Store::read(path)
                .of_project(project)
                .into_iter()
                .filter(|held| held.record_key.is_some())
                .map(|held| held.acp_session)
                .collect()
        })
        .unwrap_or_default();

    let going = {
        let work = app.state::<WorkFile>();
        let Ok(_held) = work.guard.lock() else { return };
        let mut store = Store::read(app);
        let going = store.supersede(project, extension_id, about, key, &spared);
        if !going.is_empty() {
            let _ = store.write(app);
        }
        going
    };
    if going.is_empty() {
        return;
    }

    // The pointer before the agent: the row is what somebody sees, and a process
    // ended while its row still offered to continue it would be a conversation
    // that cannot be reopened and cannot be removed.
    if let Some(path) = path {
        let mut pointers = crate::sessions::remembered::Store::read(&path);
        for acp in &going {
            pointers.forget(project, acp);
        }
        let _ = pointers.write(&path);
    }

    let sessions = app.state::<crate::sessions::live::Sessions>();
    for session in sessions.all() {
        let Some(acp) = session.acp_session() else {
            continue;
        };
        if !going.iter().any(|one| one.as_str() == acp.0.as_ref()) {
            continue;
        }
        sessions.remove(&session.key);
        session.end(None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, at: u64) -> Ordered {
        Ordered {
            key: key.to_owned(),
            kind: KIND.to_owned(),
            agent: "claude".to_owned(),
            title: "Counting what this project holds".to_owned(),
            extension_id: "tick".to_owned(),
            extension_name: "Tick".to_owned(),
            handler: "poll".to_owned(),
            about: None,
            on_interrupted: OnInterrupted::Continue,
            keep: Keep::Each,
            ordered_at_ms: at,
            acp_session: None,
        }
    }

    /// One run of a routine: a slot named by the record, and the conversation
    /// it turned into.
    fn run(key: &str, about: &str, handler: &str, acp: Option<&str>, at: u64) -> Ordered {
        Ordered {
            extension_id: "routines".to_owned(),
            extension_name: "Routines".to_owned(),
            handler: handler.to_owned(),
            about: Some(AboutOrder::Key(about.to_owned())),
            keep: Keep::Latest,
            acp_session: acp.map(ToOwned::to_owned),
            ..entry(key, at)
        }
    }

    fn nothing() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    /// The state the write order exists to make impossible, asserted from the
    /// other side: an order is a complete, readable thing before any session
    /// exists, and gaining one changes nothing else about it.
    #[test]
    fn an_order_is_whole_before_there_is_a_session() {
        let mut store = Store::default();
        store.keep("/work/repo", entry("w1-0", 10));

        let held = store.of_project("/work/repo");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].acp_session, None, "ordered, and not yet started");
        assert_eq!(held[0].source().extension_id, "tick");

        assert!(store.began("/work/repo", "w1-0", "thread-9"));
        let held = store.of_project("/work/repo");
        assert_eq!(held[0].acp_session.as_deref(), Some("thread-9"));
        assert_eq!(
            held[0].source().handler,
            "poll",
            "the source is set once and nothing later edits it"
        );
    }

    #[test]
    fn a_session_for_an_order_that_is_gone_is_not_a_failure() {
        let mut store = Store::default();
        store.keep("/work/repo", entry("w1-0", 10));
        assert!(!store.began("/work/repo", "w1-1", "thread-9"));
        assert!(!store.began("/elsewhere", "w1-0", "thread-9"));
    }

    #[test]
    fn the_oldest_go_first_when_the_list_stops_being_a_list() {
        let mut store = Store::default();
        for at in 0..(PER_PROJECT_LIMIT + 20) {
            store.keep(
                "/work/repo",
                entry(&format!("w{at}"), u64::try_from(at).unwrap()),
            );
        }
        let held = store.of_project("/work/repo");
        assert_eq!(held.len(), PER_PROJECT_LIMIT);
        assert_eq!(
            held.first().map(|entry| entry.key.as_str()),
            Some(format!("w{}", PER_PROJECT_LIMIT + 19).as_str()),
            "most recently ordered first"
        );
        assert!(
            !held.iter().any(|entry| entry.key == "w0"),
            "and the oldest is the one that went"
        );
    }

    /// The two words cross the boundary as a package writes them, and a third
    /// is not a value this enum has. `serde` is what enforces it, which is why
    /// it is worth one assertion rather than none.
    #[test]
    fn interruption_is_one_of_two_words_and_no_others() {
        assert_eq!(
            serde_json::from_str::<OnInterrupted>(r#""continue""#).unwrap(),
            OnInterrupted::Continue
        );
        assert_eq!(
            serde_json::from_str::<OnInterrupted>(r#""wait""#).unwrap(),
            OnInterrupted::Wait
        );
        assert!(serde_json::from_str::<OnInterrupted>(r#""later""#).is_err());
        assert!(
            serde_json::from_str::<Order>(
                r#"{"kind":"agent.session","agent":"claude","title":"A poll","prompt":{"text":"go"}}"#
            )
            .is_err(),
            "and it is required: there is no default that is right for both cases"
        );
    }

    /// The whole of what a slot is for: a routine on a clock leaves one row,
    /// however many times it has run.
    #[test]
    fn a_slot_holds_the_run_that_has_just_started_and_nothing_before_it() {
        let mut store = Store::default();
        store.keep(
            "/work/repo",
            run("w1", "routine-a", "quarterly", Some("s1"), 10),
        );
        store.keep(
            "/work/repo",
            run("w2", "routine-a", "quarterly", Some("s2"), 20),
        );
        store.keep(
            "/work/repo",
            run("w3", "routine-a", "quarterly", Some("s3"), 30),
        );

        let going = store.supersede("/work/repo", "routines", "routine-a", "w3", &nothing());

        assert_eq!(going, vec!["s1".to_owned(), "s2".to_owned()]);
        let held = store.of_project("/work/repo");
        assert_eq!(held.len(), 1, "the run that has just started, and it alone");
        assert_eq!(held[0].key, "w3");
    }

    /// Two routines are two slots, and neither ends the other's conversation.
    #[test]
    fn a_slot_is_one_record_and_not_one_package() {
        let mut store = Store::default();
        store.keep(
            "/work/repo",
            run("w1", "routine-a", "quarterly", Some("s1"), 10),
        );
        store.keep(
            "/work/repo",
            run("w2", "routine-b", "quarterly", Some("s2"), 20),
        );
        store.keep(
            "/work/repo",
            run("w3", "routine-a", "quarterly", Some("s3"), 30),
        );

        let going = store.supersede("/work/repo", "routines", "routine-a", "w3", &nothing());

        assert_eq!(going, vec!["s1".to_owned()]);
        let keys: Vec<String> = store
            .of_project("/work/repo")
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert!(
            keys.contains(&"w2".to_owned()),
            "the other routine is untouched"
        );
    }

    /// The handler is not part of the slot, so moving a routine from every
    /// fifteen minutes to every hour does not leave yesterday's conversation
    /// standing beside today's.
    #[test]
    fn changing_the_interval_does_not_start_a_second_slot() {
        let mut store = Store::default();
        store.keep(
            "/work/repo",
            run("w1", "routine-a", "quarterly", Some("s1"), 10),
        );
        store.keep(
            "/work/repo",
            run("w2", "routine-a", "hourly", Some("s2"), 20),
        );

        let going = store.supersede("/work/repo", "routines", "routine-a", "w2", &nothing());

        assert_eq!(
            going,
            vec!["s1".to_owned()],
            "carried out by another handler, and the same routine"
        );
        assert_eq!(store.of_project("/work/repo").len(), 1);
    }

    /// A conversation somebody kept as a record is theirs. The package's
    /// arrangement of its own rows does not reach it, and its order stays with
    /// it so the two files still agree.
    #[test]
    fn a_conversation_kept_as_a_record_is_not_a_packages_to_remove() {
        let mut store = Store::default();
        store.keep(
            "/work/repo",
            run("w1", "routine-a", "quarterly", Some("s1"), 10),
        );
        store.keep(
            "/work/repo",
            run("w2", "routine-a", "quarterly", Some("s2"), 20),
        );
        store.keep(
            "/work/repo",
            run("w3", "routine-a", "quarterly", Some("s3"), 30),
        );
        let spared = std::collections::HashSet::from(["s1".to_owned()]);

        let going = store.supersede("/work/repo", "routines", "routine-a", "w3", &spared);

        assert_eq!(going, vec!["s2".to_owned()]);
        let keys: Vec<String> = store
            .of_project("/work/repo")
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert_eq!(keys, vec!["w3".to_owned(), "w1".to_owned()]);
    }

    /// An order whose agent never rose has no conversation to replace and
    /// nothing to read. It goes with the slot rather than accumulating one row
    /// per failed launch, which on a fifteen-minute clock is ninety-six a day.
    #[test]
    fn a_run_that_never_started_leaves_with_the_rest_of_the_slot() {
        let mut store = Store::default();
        store.keep("/work/repo", run("w1", "routine-a", "quarterly", None, 10));
        store.keep(
            "/work/repo",
            run("w2", "routine-a", "quarterly", Some("s2"), 20),
        );

        let going = store.supersede("/work/repo", "routines", "routine-a", "w2", &nothing());

        assert!(going.is_empty(), "there was no conversation to end");
        assert_eq!(store.of_project("/work/repo").len(), 1);
    }

    /// `latest` says *the most recent one about the same thing*, so there has to
    /// be a thing. Refused rather than ignored: a package that asked for one
    /// conversation and silently got every one of them would find out from the
    /// list a day later.
    #[test]
    fn keeping_the_latest_of_nothing_is_refused() {
        let order: Order = serde_json::from_str(
            r#"{"kind":"agent.session","agent":"claude","title":"A routine","prompt":{"text":"go"},"onInterrupted":"continue","keep":"latest"}"#,
        )
        .expect("it parses");
        assert_eq!(order.keep, Keep::Latest);
        assert_eq!(
            order.about, None,
            "and there is nothing for it to be the latest of"
        );
    }

    /// Saying nothing is what every package already built says, and it must go
    /// on meaning what it meant.
    #[test]
    fn a_package_that_says_nothing_keeps_every_conversation() {
        let order: Order = serde_json::from_str(
            r#"{"kind":"agent.session","agent":"claude","title":"A poll","prompt":{"text":"go"},"onInterrupted":"continue"}"#,
        )
        .expect("it parses");
        assert_eq!(order.keep, Keep::Each);
        assert!(serde_json::from_str::<Keep>(r#""some""#).is_err());
    }

    /// What is written is read back, in the spelling the file holds — the
    /// boundary lesson one file over, where a field that renames on one side
    /// and not the other arrives as nothing at all.
    #[test]
    fn what_is_written_is_read_back() {
        let mut store = Store::default();
        store.keep("/work/repo", entry("w1-0", 10));
        store.began("/work/repo", "w1-0", "thread-9");
        let text = serde_json::to_string(&store).expect("the store is written");

        assert!(text.contains(r#""acpSession":"thread-9""#), "{text}");
        assert!(text.contains(r#""onInterrupted":"continue""#), "{text}");
        assert_eq!(
            text.matches("w1-0").count(),
            1,
            "the order's key is written down once, and the source is assembled from it: {text}"
        );
        assert!(text.contains(r#""orderedAtMs":10"#), "{text}");

        let read: Store = serde_json::from_str(&text).expect("and read back");
        let held = read.of_project("/work/repo");
        assert_eq!(held[0].acp_session.as_deref(), Some("thread-9"));
        assert_eq!(held[0].on_interrupted, OnInterrupted::Continue);
    }
}
