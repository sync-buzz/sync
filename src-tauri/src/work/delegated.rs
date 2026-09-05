//! What comes back from work one conversation delegated to another.
//!
//! A conversation may order work of its own ([`super::order`] with a parent),
//! and when that work is done the conversation that asked for it is usually in
//! the middle of something else — or not running at all. Neither is a reason to
//! lose the answer, and neither is a reason to interrupt: what is here is the
//! queue in between, and the two rules that empty it.
//!
//! # Nothing is raised, and nothing is polled
//!
//! An outcome is delivered when the conversation it belongs to is **up and not
//! in a turn**. If it is up and busy, the outcome waits for the turn to end. If
//! it is not up, the outcome waits in the file — a conversation is resumed by a
//! person and by nothing else (`docs/background.md` §6.4), and raising an agent
//! two days later to hand it a paragraph would be spending somebody's money
//! without asking.
//!
//! The other half of that bargain is that nobody asks either. There is no tool
//! an agent calls to find out whether its delegated work is finished: work that
//! runs for a day would be asked about thousands of times for one answer, and
//! the answer arrives as an ordinary turn anyway.
//!
//! # Every delegated run starts when it is ordered
//!
//! Work delegated from a conversation is performed **in that conversation's own
//! working tree** — an order does not choose where it is carried out
//! (`docs/background.md` §6.2). So two of them at once are two agents editing
//! one set of files, and both go anyway. The tree is the person's, and so is
//! the decision: somebody who delegates a second piece of work while the first
//! is going has said what they want happening in it.
//!
//! Holding the second back until the first was finished is the alternative, and
//! it costs the thing delegating is for. An agent that has been asked to do
//! something is expected to be doing it, and a conversation that has been open
//! for an hour with nothing asked of it is a wait nobody ordered — invisible in
//! the transcript, because the turn that is waiting has not been sent. What is
//! bought for that hour is narrower than it looks: two agents that ran an hour
//! apart still meet in what the first one left behind.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::project::{ProjectError, configuration_file, write_configuration};
use crate::sessions::event::{Status, now_ms};
use crate::sessions::live::{Session, Sessions};
use crate::sessions::{Ending, Turn};

/// The file, in this installation's configuration directory.
const FILE: &str = "delegated-outcomes.json";

/// How many undelivered outcomes one project keeps.
///
/// The same bound as the orders and the pointers beside it, and here it guards
/// one case: a conversation that is never resumed. Its outcomes have nowhere to
/// go and nothing else would ever remove them, so without a ceiling one
/// abandoned conversation grows this file for the life of the installation.
const PER_PROJECT_LIMIT: usize = 100;

/// One finished piece of delegated work, waiting for the conversation that
/// asked for it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Outcome {
    /// The conversation that delegated it, by the agent's own id for it — the
    /// one identity that outlives the run, which is what this file needs of it.
    pub parent: String,
    /// The conversation the work was done in, so the answer can say which one
    /// it came out of and a person can go and read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<String>,
    /// What that conversation was called, as the order named it.
    pub title: String,
    /// The last thing the agent said, which is the answer
    /// ([`crate::sessions::live::Session::said`]).
    pub said: String,
    pub ending: Ending,
    pub finished_at_ms: u64,
}

impl Outcome {
    /// What this one outcome says, when there is a conversation to say it to.
    ///
    /// An ending nobody would act on is not reported: `end_turn` is an agent
    /// finishing, and a sentence saying so under every answer would be noise on
    /// every delegation ever made. What is reported is an empty answer — the
    /// turn stopped on a tool call, or was cut short — because that is a case
    /// the conversation reading it has to handle rather than quote.
    fn account(&self) -> String {
        let said = self.said.trim();
        match (&self.ending, said.is_empty()) {
            (Ending::Failed(detail), true) => format!("It did not finish: {detail}"),
            (Ending::Failed(detail), false) => format!("{said}\n\nIt did not finish: {detail}"),
            (Ending::Stopped(reason), true) => format!(
                "It ended without saying anything ({}).",
                reason.as_deref().unwrap_or("no reason given")
            ),
            (Ending::Stopped(_), false) => said.to_owned(),
        }
    }

    /// How this one is headed when it is not the only one being handed over.
    fn heading(&self) -> String {
        match self.child.as_deref() {
            Some(child) => format!("## {} (conversation {child})", self.title),
            None => format!("## {}", self.title),
        }
    }
}

/// The one message a set of outcomes is handed over as.
///
/// **One message, however many outcomes.** Three turns for three finished
/// children would be three interruptions of whatever the conversation went on
/// to do, and the second and third would arrive into a turn the first had
/// started — so they would queue behind it and arrive anyway, one at a time,
/// slower and out of order.
fn message(outcomes: &[Outcome]) -> String {
    if let [only] = outcomes {
        let named = match only.child.as_deref() {
            Some(child) => format!("“{}” (conversation {child})", only.title),
            None => format!("“{}”", only.title),
        };
        return format!(
            "The work you delegated has finished: {named}.\n\n{}",
            only.account()
        );
    }
    let accounts: Vec<String> = outcomes
        .iter()
        .map(|outcome| format!("{}\n\n{}", outcome.heading(), outcome.account()))
        .collect();
    format!(
        "{} pieces of work you delegated have finished.\n\n{}",
        outcomes.len(),
        accounts.join("\n\n")
    )
}

/// What the host tells an agent about the ending it is writing.
///
/// Appended by the host and not by whoever delegated, because the host is what
/// takes that ending and hands it on: a rule stated only in a package's own
/// words would be true until a second package ordered delegated work, and then
/// quietly false — with nothing failing and one conversation answering another
/// with a sentence about being about to answer.
///
/// It is added to the prompt rather than said separately, so it is one message
/// in the transcript of the work: a person reading it sees exactly what the
/// agent was asked, in one place.
pub(crate) fn briefed(text: &str, parent: &str) -> String {
    format!(
        "{text}\n\n---\n\nThis work was delegated from another conversation, `{parent}`. \
         Whatever you say last in this turn is what goes back to it, on its own: that \
         conversation cannot read anything else said here. End with the answer itself, not with \
         a note about having finished.\n\nIf this needs work delegated of its own, it is \
         delegated from `{parent}` and not from here: a chain is two conversations deep, so what \
         you delegate stands beside this one rather than under it."
    )
}

/// Every undelivered outcome this machine holds, by the project it belongs to.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct Store {
    projects: BTreeMap<String, Vec<Outcome>>,
}

impl Store {
    /// Reads the file, answering with an empty store when there is none.
    ///
    /// An unreadable file is absent rather than fatal, as it is for the orders
    /// and the pointers beside it: what it costs is the answers waiting in it,
    /// and refusing to run over it would cost every conversation on the machine.
    #[must_use]
    pub fn read(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// # Errors
    ///
    /// [`ProjectError`] when the configuration directory cannot be written.
    pub fn write(&self, path: &std::path::Path) -> Result<(), ProjectError> {
        write_configuration(path, self)
    }

    /// Writes one outcome down, dropping the oldest when the list stops being
    /// one.
    pub fn queue(&mut self, project: &str, outcome: Outcome) {
        let held = self.projects.entry(project.to_owned()).or_default();
        held.push(outcome);
        if held.len() > PER_PROJECT_LIMIT {
            held.sort_by_key(|outcome| std::cmp::Reverse(outcome.finished_at_ms));
            held.truncate(PER_PROJECT_LIMIT);
        }
    }

    /// The conversations this project is holding answers for, oldest answer
    /// first — which is the order they are handed over in.
    #[must_use]
    fn awaited(&self, project: &str) -> Vec<String> {
        let mut named: Vec<String> = Vec::new();
        for outcome in self.projects.get(project).into_iter().flatten() {
            if !named.iter().any(|held| held == &outcome.parent) {
                named.push(outcome.parent.clone());
            }
        }
        named
    }

    /// Takes one conversation's answers out, in the order they finished.
    #[must_use]
    pub fn take(&mut self, project: &str, parent: &str) -> Vec<Outcome> {
        let Some(held) = self.projects.get_mut(project) else {
            return Vec::new();
        };
        let mut taken: Vec<Outcome> = Vec::new();
        held.retain(|outcome| {
            if outcome.parent == parent {
                taken.push(outcome.clone());
                return false;
            }
            true
        });
        taken.sort_by_key(|outcome| outcome.finished_at_ms);
        if held.is_empty() {
            self.projects.remove(project);
        }
        taken
    }

    /// Whether this project is holding anything at all.
    #[must_use]
    fn empty(&self, project: &str) -> bool {
        self.projects.get(project).is_none_or(Vec::is_empty)
    }

    /// Stop holding a project's answers. Called where a project is forgotten.
    fn forget(&mut self, project: &str) {
        self.projects.remove(project);
    }
}

/// The live conversation with that id, when it is up and not in a turn.
///
/// The whole of *free*, and the whole of what is asked before an answer is
/// handed over. A conversation that is not in this registry is one nothing here
/// will raise; one that is in it and working is one nothing here will interrupt.
#[must_use]
fn free(sessions: &Sessions, acp_session: &str) -> Option<Arc<Session>> {
    sessions.all().into_iter().find(|session| {
        session.status() == Status::Ready
            && session
                .acp_session()
                .is_some_and(|id| id.0.to_string() == acp_session)
    })
}

/// What can go now, taken out of the store, with the message each conversation
/// is to be handed.
///
/// The half of delivery with the rule in it, and the half a test can reach:
/// which conversations are free is answered from the registry, which is a
/// value, and what is left in the store afterwards is the answer to *what
/// happens to work whose parent is not there*.
fn ready(store: &mut Store, sessions: &Sessions, project: &str) -> Vec<(Arc<Session>, String)> {
    let mut going: Vec<(Arc<Session>, String)> = Vec::new();
    for parent in store.awaited(project) {
        let Some(session) = free(sessions, &parent) else {
            continue;
        };
        let taken = store.take(project, &parent);
        if taken.is_empty() {
            continue;
        }
        going.push((session, message(&taken)));
    }
    going
}

/// Hands over everything waiting for a conversation that is free to take it.
///
/// Called at the three moments a conversation can have become free: a turn of
/// its own ended, it was resumed by somebody, or an answer arrived for it just
/// now. Quiet throughout — every failure here leaves the answer in the file,
/// which is where it already was.
pub(crate) fn deliver<R: Runtime>(app: &AppHandle<R>, project: &str) {
    let Ok(path) = configuration_file(app, FILE) else {
        return;
    };
    let held = app.state::<Delegations>();
    let going = {
        let Ok(_guard) = held.file.lock() else {
            return;
        };
        let mut store = Store::read(&path);
        if store.empty(project) {
            return;
        }
        let sessions = app.state::<Sessions>();
        let going = ready(&mut store, &sessions, project);
        if going.is_empty() {
            return;
        }
        // Written before anything is said, and that is the order the pair of
        // them is chosen by: interrupted here, an answer is handed over twice,
        // which somebody reads and dismisses. The other way round it is handed
        // over never, and nobody knows there was one.
        let _ = store.write(&path);
        going
    };
    for (session, message) in going {
        let _ = crate::sessions::send(
            app,
            &session,
            Turn {
                text: message,
                ..Turn::default()
            },
        );
    }
}

/// Writes down what came of one delegated run, and hands it over if it can.
pub(crate) fn finished<R: Runtime>(
    app: &AppHandle<R>,
    project: &str,
    parent: &str,
    session: &Arc<Session>,
    ending: Ending,
) {
    let outcome = Outcome {
        parent: parent.to_owned(),
        child: session.acp_session().map(|id| id.0.to_string()),
        title: session
            .title()
            .unwrap_or_else(|| session.agent_name.clone()),
        said: session.said(),
        ending,
        finished_at_ms: now_ms(),
    };
    let Ok(path) = configuration_file(app, FILE) else {
        return;
    };
    {
        let held = app.state::<Delegations>();
        let Ok(_guard) = held.file.lock() else {
            return;
        };
        let mut store = Store::read(&path);
        store.queue(project, outcome);
        if store.write(&path).is_err() {
            return;
        }
    }
    deliver(app, project);
}

/// Stop holding a project's undelivered answers. Called where a project is
/// forgotten, beside the orders they came out of.
pub(crate) fn forget<R: Runtime>(app: &AppHandle<R>, project: &str) {
    let Ok(path) = configuration_file(app, FILE) else {
        return;
    };
    let held = app.state::<Delegations>();
    let Ok(_guard) = held.file.lock() else { return };
    let mut store = Store::read(&path);
    store.forget(project);
    let _ = store.write(&path);
}

/// The lock over this module's file.
///
/// Its own rather than [`super::WorkFile`]'s because this is a second file with
/// a second read-modify-write, and one lock over two files would make each of
/// them wait for the other for no reason.
#[derive(Default)]
pub struct Delegations {
    file: Mutex<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::live::Place;

    fn outcome(parent: &str, child: &str, said: &str, at: u64) -> Outcome {
        Outcome {
            parent: parent.to_owned(),
            child: Some(child.to_owned()),
            title: "Rename the columns".to_owned(),
            said: said.to_owned(),
            ending: Ending::Stopped(Some("end_turn".to_owned())),
            finished_at_ms: at,
        }
    }

    /// A session as the registry holds one, with the agent's id on it. No agent
    /// is raised: a session is a value, which is what makes the rule below
    /// testable at all.
    fn session(sessions: &Sessions, acp: &str, status: Status) -> Arc<Session> {
        let session = Session::new(
            sessions.mint_key(),
            "claude".to_owned(),
            "Claude Code".to_owned(),
            Place::project(std::env::temp_dir()),
            None,
            None,
            None,
        );
        session.remember_session(acp_client::schema::SessionId::new(acp));
        session.set_status(status, None);
        sessions.insert(Arc::clone(&session));
        session
    }

    /// The second criterion: what arrives while a conversation is busy arrives
    /// as one message when it stops being busy.
    #[test]
    fn two_answers_that_waited_together_are_handed_over_together() {
        let sessions = Sessions::default();
        session(&sessions, "parent-1", Status::Working);

        let mut store = Store::default();
        store.queue("/work/repo", outcome("parent-1", "child-a", "Renamed.", 10));
        store.queue(
            "/work/repo",
            outcome("parent-1", "child-b", "Ran the tests.", 20),
        );

        assert!(
            ready(&mut store, &sessions, "/work/repo").is_empty(),
            "a conversation in a turn is not interrupted"
        );

        session(&sessions, "parent-1", Status::Ready);
        let going = ready(&mut store, &sessions, "/work/repo");
        assert_eq!(going.len(), 1, "one message, however many answers");
        let (_, message) = &going[0];
        assert!(message.starts_with("2 pieces of work you delegated have finished."));
        assert!(message.contains("Renamed."), "{message}");
        assert!(message.contains("Ran the tests."), "{message}");
        assert!(
            message.find("Renamed.") < message.find("Ran the tests."),
            "and in the order they finished: {message}"
        );
        assert!(
            store.empty("/work/repo"),
            "and what has been handed over is not held twice"
        );
    }

    /// The third criterion, and the rule the module exists to keep: nothing is
    /// raised to be told something.
    #[test]
    fn an_answer_for_a_conversation_that_is_not_up_waits_in_the_file() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join(FILE);
        let mut store = Store::default();
        store.queue("/work/repo", outcome("parent-1", "child-a", "Renamed.", 10));
        store.write(&path).expect("the store is written");

        // Nothing in the registry: the conversation was never resumed, and no
        // amount of asking makes one.
        let sessions = Sessions::default();
        let mut read = Store::read(&path);
        assert!(ready(&mut read, &sessions, "/work/repo").is_empty());
        assert!(
            !read.empty("/work/repo"),
            "and the answer is still there to be handed over later"
        );

        // Resumed by a person, which is the only thing that raises one.
        session(&sessions, "parent-1", Status::Ready);
        let going = ready(&mut read, &sessions, "/work/repo");
        assert_eq!(going.len(), 1);
        assert!(going[0].1.contains("Renamed."), "{}", going[0].1);
    }

    /// What is written is read back — the failure this guards against being a
    /// spelling one, where a member serialised under a name the reader does not
    /// use quietly becomes nothing.
    #[test]
    fn an_answer_survives_the_application_ending() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join(FILE);
        let mut store = Store::default();
        store.queue("/work/repo", outcome("parent-1", "child-a", "Renamed.", 10));
        store.write(&path).expect("the store is written");

        let mut read = Store::read(&path);
        let taken = read.take("/work/repo", "parent-1");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].said, "Renamed.");
        assert_eq!(taken[0].child.as_deref(), Some("child-a"));
        assert_eq!(
            taken[0].ending,
            Ending::Stopped(Some("end_turn".to_owned()))
        );
        assert!(
            read.take("/work/repo", "parent-1").is_empty(),
            "and taking it is what removes it"
        );
    }

    /// One conversation's answers are not another's.
    #[test]
    fn a_conversation_is_handed_only_what_it_asked_for() {
        let mut store = Store::default();
        store.queue("/work/repo", outcome("parent-1", "child-a", "Renamed.", 10));
        store.queue(
            "/work/repo",
            outcome("parent-2", "child-b", "Ran the tests.", 20),
        );

        let taken = store.take("/work/repo", "parent-1");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].child.as_deref(), Some("child-a"));
        assert_eq!(store.awaited("/work/repo"), vec!["parent-2".to_owned()]);
        assert!(store.take("/other/clone", "parent-2").is_empty());
    }

    /// What a conversation is handed when the work said nothing, which is a
    /// turn that stopped on a tool call or was cut short.
    #[test]
    fn an_answer_with_nothing_in_it_says_so_rather_than_arriving_blank() {
        let mut silent = outcome("parent-1", "child-a", "   ", 10);
        silent.ending = Ending::Stopped(Some("cancelled".to_owned()));
        let said = message(&[silent]);
        assert!(
            said.contains("without saying anything (cancelled)"),
            "{said}"
        );

        let mut broken = outcome("parent-1", "child-a", "Half of it is done.", 10);
        broken.ending = Ending::Failed("the agent's process ended".to_owned());
        let said = message(&[broken]);
        assert!(said.contains("Half of it is done."), "{said}");
        assert!(
            said.contains("It did not finish: the agent's process ended"),
            "{said}"
        );
    }

    /// The agent writing the answer is told that it is writing one.
    #[test]
    fn delegated_work_is_told_what_its_last_words_are_for() {
        let briefed = briefed("Rename the columns", "sess-14");
        assert!(briefed.starts_with("Rename the columns"));
        assert!(briefed.contains("what goes back to it"), "{briefed}");
        // The identifier, and it is the half an agent cannot work out for
        // itself: said nowhere, a conversation that needs a second pair of
        // hands delegates under nothing and its work stands loose.
        assert!(briefed.contains("`sess-14`"), "{briefed}");
    }
}
