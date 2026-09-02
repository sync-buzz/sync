//! The conversations this machine can ask an agent to hand back.
//!
//! A session lives in the application and dies with it: [`super::live::Sessions`]
//! is a map in memory, so quitting Sync used to end every conversation in it,
//! whether or not anybody had finished with one. What survives now is not the
//! conversation — it is a *pointer* to it: which agent, in which directory, and
//! the agent's own id for the session. The words stay where they always were,
//! with the agent, and come back through `session/load`.
//!
//! # Why this is not in the project's memory
//!
//! Because it is not true of the project. An agent's session id means something
//! on the machine whose agent holds it and nothing anywhere else, so a pointer
//! written into a repository would travel to a colleague as an instruction to
//! resume something they do not have. It lives in this installation's own
//! configuration directory, beside the recent-projects list, for the same
//! reason that one does.
//!
//! That placement is also what answers "is this conversation mine?". There is
//! no machine identifier anywhere and none is needed: a pointer that is here is
//! this machine's, and one that is absent is somebody else's — or one this
//! machine has forgotten, which needs the same answer.
//!
//! # What it deliberately does not hold
//!
//! The transcript. Keeping one here would make this a second archive of
//! conversations sitting next to the project's memory, with its own growth and
//! its own pruning, and would blur the one distinction the product is built on:
//! a conversation is with an agent, a *kept* conversation is a record. What a
//! person wants to survive the agent forgetting is what they kept.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::project::{ProjectError, write_configuration};

/// The file, in this installation's configuration directory.
pub const FILE: &str = "conversations.json";

/// How many pointers are kept per project.
///
/// A bound rather than a policy: the list is what a person picks a conversation
/// out of, and one that has grown to hundreds is not a list any more. The
/// oldest go first, and losing one costs nothing that was not already the
/// agent's to lose.
const PER_PROJECT_LIMIT: usize = 100;

/// One conversation this machine opened, and enough to ask for it back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Remembered {
    /// The agent's own id for the session, and what `session/load` takes.
    ///
    /// This is the identity of the entry as well. It is stable across runs,
    /// which the live key is not — `Sessions::mint_key` counts from zero every
    /// time the application starts, so `s0` names a different conversation
    /// after every restart and could never have been written down.
    pub acp_session: String,
    /// Which agent holds it. A session belongs to the agent that made it, so
    /// resuming with another is not a thing that can work.
    pub agent_id: String,
    /// What that agent is called, so a row can be drawn without the catalogue.
    pub agent_name: String,
    /// The project the conversation was held in. Checked before resuming: the
    /// same repository cloned elsewhere is a different working tree, and an
    /// agent asked to resume into it is being asked about files that are not
    /// the ones it read.
    ///
    /// Where the agent actually worked is [`Self::worktree`] when that was not
    /// the project's own tree.
    pub cwd: String,
    /// The disposable tree it was held in, when it was held in one.
    ///
    /// Written down because resuming has to land in the same files, and a tree
    /// is the one thing about a conversation that a person can delete from
    /// underneath it. A pointer naming a tree that is gone is refused rather
    /// than quietly resumed in the project — the agent would answer about files
    /// it never read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<crate::worktree::Worktree>,
    /// What the conversation is called, or `None` before anything was said.
    pub title: Option<String>,
    pub opened_at_ms: u64,
    /// When this machine last saw it, which is what the oldest-first pruning
    /// reads and what orders the list.
    pub last_seen_ms: u64,
    /// Who asked for this conversation, when it was not a person.
    ///
    /// Held here rather than joined from the orders file at read time, and for
    /// the reason `agent_name` above is: **so a row can be drawn without a
    /// second lookup**. It buys more than convenience. The orders file is
    /// bounded and prunes on its own clock, so a busy project would start
    /// showing yesterday's ordered conversations as though a person had begun
    /// them — a lie by omission, in the list somebody scans in the morning.
    ///
    /// The two copies cannot disagree: both are written from the session, once,
    /// on the same path, and a source is never edited (`docs/background.md`
    /// §6.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<super::live::Source>,
    /// The record the conversation was held under, when it was held under one.
    ///
    /// Written from the session for the reason [`Self::source`] is: a dormant
    /// row and a live one are the same conversation at two moments, and one
    /// that lost its heading when its agent stopped would move up the list
    /// under somebody reading it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<super::live::About>,
    /// The conversation this one was delegated from, by that conversation's own
    /// agent id.
    ///
    /// Here rather than only on the live session because a tree of
    /// conversations that flattened on restart would be a tree nobody could
    /// rely on: the child outlives the run its parent was raised in, and both
    /// halves of the list are drawn from the same two facts.
    ///
    /// It may name a conversation this file no longer holds — pointers prune
    /// oldest-first, and a parent can be forgotten while its child is kept.
    /// That is an ordinary state and not one to repair: a child whose parent is
    /// gone is drawn where a conversation with no parent is drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The record it was kept as, when somebody kept it.
    ///
    /// The link is held here rather than in the record because it is the half
    /// that is machine-local: the record travels with the repository and must
    /// not carry an id that is meaningless wherever it lands. A kept record
    /// asks this file whether *this* machine can resume it, and the answer for
    /// a colleague's clone is a plain no.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_key: Option<String>,
}

/// Every pointer this machine holds, by the project it belongs to.
///
/// Keyed by project path, because the question is always asked from inside one
/// project and a flat list would be filtered on every read.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Store {
    projects: HashMap<String, Vec<Remembered>>,
}

impl Store {
    /// Reads the file, answering with an empty store when there is none.
    ///
    /// A file that cannot be parsed is treated as absent rather than as a
    /// failure. It holds pointers to conversations, all of which the agent
    /// still has: the worst an unreadable one costs is that they are not
    /// offered, and refusing to open the application over it would be a far
    /// larger price than the thing is worth.
    #[must_use]
    pub fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// # Errors
    ///
    /// [`ProjectError`] when the configuration directory cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), ProjectError> {
        write_configuration(path, self)
    }

    /// The conversations of one project, most recently seen first.
    #[must_use]
    pub fn of_project(&self, project: &str) -> Vec<Remembered> {
        let mut held = self.projects.get(project).cloned().unwrap_or_default();
        held.sort_by_key(|entry| std::cmp::Reverse(entry.last_seen_ms));
        held
    }

    /// One conversation, by the agent's id for it.
    #[must_use]
    pub fn get(&self, project: &str, acp_session: &str) -> Option<&Remembered> {
        self.projects
            .get(project)?
            .iter()
            .find(|held| held.acp_session == acp_session)
    }

    /// The pointer for a kept record, when this machine has one.
    ///
    /// The whole of the "is it mine?" test. `None` is the ordinary answer for a
    /// record somebody else wrote, and the caller continues from the
    /// transcript instead.
    #[must_use]
    pub fn for_record(&self, project: &str, record_key: &str) -> Option<&Remembered> {
        self.projects
            .get(project)?
            .iter()
            .find(|held| held.record_key.as_deref() == Some(record_key))
    }

    /// Writes one pointer, replacing whatever was held for the same session.
    ///
    /// Everything else about the entry is taken from the new one except the
    /// record it was kept as: keeping happens once and re-opening the
    /// conversation afterwards must not quietly unlink it.
    pub fn remember(&mut self, project: &str, mut entry: Remembered) {
        let held = self.projects.entry(project.to_owned()).or_default();
        if let Some(at) = held
            .iter()
            .position(|other| other.acp_session == entry.acp_session)
        {
            if entry.record_key.is_none() {
                entry.record_key = held[at].record_key.clone();
            }
            held[at] = entry;
        } else {
            held.push(entry);
        }
        if held.len() > PER_PROJECT_LIMIT {
            held.sort_by_key(|entry| std::cmp::Reverse(entry.last_seen_ms));
            held.truncate(PER_PROJECT_LIMIT);
        }
    }

    /// Says which record a conversation was kept as.
    ///
    /// Answers whether there was a pointer to say it of. There may not be: a
    /// conversation can be kept in the same run it was opened in, before
    /// anything wrote one, and that is not a failure worth stopping a keep for.
    pub fn kept_as(&mut self, project: &str, acp_session: &str, record_key: &str) -> bool {
        let Some(held) = self.projects.get_mut(project) else {
            return false;
        };
        let Some(entry) = held
            .iter_mut()
            .find(|entry| entry.acp_session == acp_session)
        else {
            return false;
        };
        entry.record_key = Some(record_key.to_owned());
        true
    }

    /// Drops one pointer. What the agent holds is not touched — this says only
    /// that this machine has stopped offering it.
    pub fn forget(&mut self, project: &str, acp_session: &str) {
        if let Some(held) = self.projects.get_mut(project) {
            held.retain(|entry| entry.acp_session != acp_session);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, seen: u64) -> Remembered {
        Remembered {
            acp_session: id.to_owned(),
            agent_id: "claude".to_owned(),
            agent_name: "Claude Code".to_owned(),
            cwd: "/work/repo".to_owned(),
            worktree: None,
            title: Some("Why is it slow?".to_owned()),
            opened_at_ms: 1_000,
            last_seen_ms: seen,
            source: None,
            about: None,
            parent: None,
            record_key: None,
        }
    }

    #[test]
    fn a_pointer_that_is_not_here_is_somebody_elses() {
        let mut store = Store::default();
        store.remember("/work/repo", entry("thread-1", 10));
        assert!(store.kept_as("/work/repo", "thread-1", "conversation-aa11"));

        assert!(
            store
                .for_record("/work/repo", "conversation-aa11")
                .is_some(),
            "the machine that held the conversation can be asked for it back"
        );
        assert!(
            store
                .for_record("/work/repo", "conversation-written-elsewhere")
                .is_none(),
            "a record a colleague wrote has no pointer here, which is the whole of the test"
        );
        assert!(
            store
                .for_record("/other/clone", "conversation-aa11")
                .is_none(),
            "and the same record in another working tree is not this one"
        );
    }

    #[test]
    fn keeping_survives_the_conversation_being_seen_again() {
        let mut store = Store::default();
        store.remember("/work/repo", entry("thread-1", 10));
        store.kept_as("/work/repo", "thread-1", "conversation-aa11");

        // Re-opened later: the pointer is rewritten with a newer sighting, and
        // the record it was kept as must not be dropped on the way.
        store.remember("/work/repo", entry("thread-1", 99));
        let held = store.get("/work/repo", "thread-1").expect("still held");
        assert_eq!(held.last_seen_ms, 99);
        assert_eq!(held.record_key.as_deref(), Some("conversation-aa11"));
        assert_eq!(
            store.of_project("/work/repo").len(),
            1,
            "the same session is one conversation, not two"
        );
    }

    #[test]
    fn the_oldest_go_first_when_the_list_stops_being_a_list() {
        let mut store = Store::default();
        for at in 0..(PER_PROJECT_LIMIT + 20) {
            store.remember(
                "/work/repo",
                entry(&format!("thread-{at}"), u64::try_from(at).unwrap()),
            );
        }
        let held = store.of_project("/work/repo");
        assert_eq!(held.len(), PER_PROJECT_LIMIT);
        assert_eq!(
            held.first().map(|entry| entry.acp_session.as_str()),
            Some(format!("thread-{}", PER_PROJECT_LIMIT + 19).as_str()),
            "most recently seen first"
        );
        assert!(
            store.get("/work/repo", "thread-0").is_none(),
            "and the oldest is the one that went"
        );
    }

    #[test]
    fn an_unreadable_file_is_an_empty_store_rather_than_a_failure() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("conversations.json");
        std::fs::write(&path, "{ this is not json").expect("the fixture is written");
        assert!(Store::read(&path).of_project("/work/repo").is_empty());
        assert!(
            Store::read(&directory.path().join("absent.json"))
                .of_project("/work/repo")
                .is_empty()
        );
    }

    #[test]
    fn what_is_written_is_read_back() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("conversations.json");
        let mut store = Store::default();
        store.remember("/work/repo", entry("thread-1", 10));
        store.kept_as("/work/repo", "thread-1", "conversation-aa11");
        store.write(&path).expect("the store is written");

        let read = Store::read(&path);
        let held = read.get("/work/repo", "thread-1").expect("held");
        assert_eq!(held.agent_id, "claude");
        assert_eq!(held.cwd, "/work/repo");
        assert_eq!(held.record_key.as_deref(), Some("conversation-aa11"));
    }

    /// What a conversation came out of survives the application ending.
    ///
    /// This is the whole reason the fact is written down here rather than held
    /// on the session: a child outlives the run that raised its parent, and a
    /// tree that flattened at every restart would be a tree nobody could trust.
    /// Written and read through the file rather than asserted on the struct,
    /// because the failure this guards against is a spelling one — a member
    /// serialised under a name the reader does not use is a member that quietly
    /// becomes nothing.
    #[test]
    fn what_a_conversation_came_out_of_is_read_back_too() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("conversations.json");
        let mut store = Store::default();
        store.remember("/work/repo", entry("thread-1", 10));
        let mut child = entry("thread-2", 11);
        child.parent = Some("thread-1".to_owned());
        store.remember("/work/repo", child);
        store.write(&path).expect("the store is written");

        let read = Store::read(&path);
        assert_eq!(
            read.get("/work/repo", "thread-2")
                .expect("the child is held")
                .parent
                .as_deref(),
            Some("thread-1"),
            "and it names the parent by the agent's id, which is the one identity that outlives \
             the run"
        );
        assert!(
            read.get("/work/repo", "thread-1")
                .expect("the parent is held")
                .parent
                .is_none(),
            "a conversation nobody delegated says nothing, rather than saying null"
        );
    }
}
