//! Terminals: the process, and the tail of what it has said.
//!
//! A terminal is two things that fail differently — a process, which must
//! survive anything the window does to itself, and a screen, which is drawn,
//! scrolled, hidden and thrown away. This crate is the first of them. What the
//! screen does with the bytes is not decided here and cannot be: the same
//! terminal may be drawn twice, or not at all for a while, and neither changes
//! what the shell on the far end is doing.
//!
//! **A terminal is opened for an owner by an opener, and both are opaque
//! words.** This crate does not know what a project is or what a package is. It
//! knows only that a terminal belongs to one of the first and was raised by one
//! of the second, and it refuses to hand one to an opener that is not the one
//! that raised it. *Whether* that opener was allowed to raise anything is
//! decided where the application can see who is asking; a crate that could
//! answer that could also widen it.
//!
//! **Nothing here is ambient.** There is no global registry: a [`Terminals`] is
//! held by whoever raised it, and when that goes, so does every process it
//! opened. That is what makes closing a project close its terminals without
//! anybody remembering to.

// A test that raises a process has nothing to recover from when it will not
// start: the assertion is the point, and unwinding at the line that failed says
// more than a `Result` threaded to the top of the test would.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod error;
mod scrollback;
mod session;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

pub use error::{Error, Result};
pub use scrollback::Tail;
pub use session::{Exit, Opening, Session, Size};

/// How much of a terminal's output is kept, per terminal.
///
/// A quarter of a megabyte is a few thousand lines — far past what anybody
/// scrolls back to read, and small enough that a hundred terminals of it is
/// still nothing next to the window they are drawn in. The number is here
/// rather than at the call site because it is a property of what a terminal
/// *is* in this application, not a knob for the caller to get wrong.
const SCROLLBACK_BYTES: usize = 256 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a terminal is called, for as long as it is open.
///
/// A counter, and deliberately guessable, because holding one is not permission
/// to do anything with it: every call names the opener as well, and a name that
/// does not match the one that raised the terminal is answered as though there
/// were no such terminal. A secret for a name would be a permission nobody
/// granted, held by whoever happened to see it written down.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TerminalId(String);

impl TerminalId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TerminalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TerminalId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// One terminal, as the layer above lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRow {
    pub id: TerminalId,
    pub owner: String,
    /// How the process finished, or absent while it is running.
    pub exit: Option<Exit>,
}

struct Held {
    owner: String,
    /// Who raised it. Every later call has to be the same one.
    opener: String,
    session: Session,
}

/// Every terminal this application has open.
#[derive(Default)]
pub struct Terminals {
    open: Mutex<HashMap<TerminalId, Held>>,
    next: Mutex<u64>,
}

impl std::fmt::Debug for Terminals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The counter is left out on purpose: it says how many terminals this
        // application has ever opened, which reads like a count of what is open
        // and is not one.
        f.debug_struct("Terminals")
            .field("open", &lock(&self.open).len())
            .finish_non_exhaustive()
    }
}

impl Terminals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raise a terminal, and answer with the name it will be known by.
    ///
    /// # Errors
    ///
    /// When the folder is not one, or the system refuses to open a pty.
    pub fn open(&self, owner: &str, opener: &str, opening: &Opening) -> Result<TerminalId> {
        let session = Session::open(opening, SCROLLBACK_BYTES)?;
        let id = self.mint();
        lock(&self.open).insert(
            id.clone(),
            Held {
                owner: owner.to_owned(),
                opener: opener.to_owned(),
                session,
            },
        );
        Ok(id)
    }

    fn mint(&self) -> TerminalId {
        let mut next = lock(&self.next);
        *next += 1;
        TerminalId(format!("terminal-{next}"))
    }

    /// Do something with one terminal, or say there is no such thing.
    ///
    /// Every call goes through here, so a terminal that has been closed answers
    /// the same way to all of them rather than each caller inventing its own
    /// reading of a missing key.
    ///
    /// **An opener that did not raise this terminal is told the same thing as
    /// one asking after a terminal that never existed.** Two answers would be
    /// one answer plus a way to find out what else is open, and there is
    /// nothing an opener can do with that fact that it could not do by opening
    /// its own.
    ///
    /// # Errors
    ///
    /// When nothing is open under that name, or it was raised by somebody else.
    pub fn with<T>(
        &self,
        id: &TerminalId,
        opener: &str,
        act: impl FnOnce(&Session) -> T,
    ) -> Result<T> {
        let open = lock(&self.open);
        let held = open
            .get(id)
            .filter(|held| held.opener == opener)
            .ok_or(Error::Unknown)?;
        Ok(act(&held.session))
    }

    /// What one opener has open for one owner.
    #[must_use]
    pub fn list(&self, owner: &str, opener: &str) -> Vec<TerminalRow> {
        let mut rows: Vec<TerminalRow> = lock(&self.open)
            .iter()
            .filter(|(_, held)| held.owner == owner && held.opener == opener)
            .map(|(id, held)| TerminalRow {
                id: id.clone(),
                owner: held.owner.clone(),
                exit: held.session.exit(),
            })
            .collect();
        // A map has no order, and a list that reshuffles itself between two
        // calls is a column that reshuffles itself in front of somebody.
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// Close one. Closing something already closed is not an error: two people
    /// asking for the same end want the same end. Closing somebody else's is
    /// not an error either, and does nothing.
    pub fn close(&self, id: &TerminalId, opener: &str) {
        let mut open = lock(&self.open);
        if open.get(id).is_some_and(|held| held.opener == opener) {
            drop(open.remove(id));
        }
    }

    /// Close everything one owner opened.
    ///
    /// This is what a project window closing calls. It is on the owner rather
    /// than on a list of names because the caller that knows a project is going
    /// away does not necessarily know what was opened inside it.
    pub fn close_owned_by(&self, owner: &str, opener: &str) {
        lock(&self.open).retain(|_, held| !(held.owner == owner && held.opener == opener));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// The package that raised everything in these tests, unless one is testing
    /// what happens when it is somebody else.
    const OPENER: &str = "terminals";

    fn opening(program: &[&str]) -> Opening {
        Opening {
            cwd: std::env::temp_dir(),
            size: Size { rows: 24, cols: 80 },
            program: program.iter().map(|s| (*s).to_owned()).collect(),
            env: Vec::new(),
        }
    }

    /// Wait until the terminal has ended, or give up. A pty is a process and a
    /// scheduler, so the alternative is a sleep long enough to be flaky anyway.
    fn wait_for_exit(terminals: &Terminals, id: &TerminalId) -> Option<Exit> {
        for _ in 0..200 {
            if let Ok(Some(exit)) = terminals.with(id, OPENER, Session::exit) {
                return Some(exit);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }

    #[test]
    fn what_a_process_prints_comes_back() {
        let terminals = Terminals::new();
        let id = terminals
            .open("someone", OPENER, &opening(&["echo", "hello"]))
            .expect("opens");
        assert!(wait_for_exit(&terminals, &id).is_some(), "the process ends");

        let tail = terminals
            .with(&id, OPENER, |session| session.since(0))
            .expect("still listed");
        let said = String::from_utf8_lossy(&tail.bytes);
        assert!(said.contains("hello"), "said {said:?}");
    }

    #[test]
    fn what_is_typed_reaches_the_process() {
        let terminals = Terminals::new();
        let id = terminals
            .open("someone", OPENER, &opening(&["cat"]))
            .expect("opens");
        terminals
            .with(&id, OPENER, |session| session.write(b"knock\n"))
            .expect("open")
            .expect("written");

        // Twice: a pty echoes what is typed, and `cat` prints it again. The wait
        // counts to the same number the assertion does, because the two arrive
        // separately — waiting for the first and asserting on the second is a
        // test that passes on a slow machine and fails on a fast one.
        let mut said = String::new();
        for _ in 0..200 {
            let tail = terminals
                .with(&id, OPENER, |session| session.since(0))
                .expect("still listed");
            said = String::from_utf8_lossy(&tail.bytes).into_owned();
            if said.matches("knock").count() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(said.matches("knock").count() >= 2, "said {said:?}");
        terminals.close(&id, OPENER);
    }

    #[test]
    fn an_exit_is_reported_with_its_code() {
        let terminals = Terminals::new();
        let id = terminals
            .open("someone", OPENER, &opening(&["sh", "-c", "exit 3"]))
            .expect("opens");
        let exit = wait_for_exit(&terminals, &id).expect("ends");
        assert_eq!(exit.code, 3);
    }

    #[test]
    fn a_folder_that_is_not_one_is_refused_before_a_process_is_raised() {
        let terminals = Terminals::new();
        let mut asked = opening(&["echo", "hi"]);
        asked.cwd = std::env::temp_dir().join("no-such-folder-here");
        let refusal = terminals.open("someone", OPENER, &asked);
        assert!(matches!(refusal, Err(Error::NoSuchFolder(_))));
        assert!(terminals.list("someone", OPENER).is_empty());
    }

    #[test]
    fn a_terminal_is_listed_for_its_owner_and_for_nobody_else() {
        let terminals = Terminals::new();
        let id = terminals
            .open("one", OPENER, &opening(&["cat"]))
            .expect("opens");
        assert_eq!(terminals.list("one", OPENER).len(), 1);
        assert!(terminals.list("two", OPENER).is_empty());
        terminals.close(&id, OPENER);
        assert!(terminals.list("one", OPENER).is_empty());
    }

    #[test]
    fn closing_a_project_closes_what_it_opened_and_leaves_the_rest() {
        let terminals = Terminals::new();
        let mine = terminals
            .open("one", OPENER, &opening(&["cat"]))
            .expect("opens");
        let theirs = terminals
            .open("two", OPENER, &opening(&["cat"]))
            .expect("opens");

        terminals.close_owned_by("one", OPENER);

        assert!(matches!(
            terminals.with(&mine, OPENER, Session::exit),
            Err(Error::Unknown)
        ));
        assert!(terminals.with(&theirs, OPENER, Session::exit).is_ok());
        terminals.close(&theirs, OPENER);
    }

    #[test]
    fn writing_to_a_terminal_that_has_ended_says_so() {
        let terminals = Terminals::new();
        let id = terminals
            .open("someone", OPENER, &opening(&["sh", "-c", "exit 0"]))
            .expect("opens");
        wait_for_exit(&terminals, &id).expect("ends");
        let refusal = terminals
            .with(&id, OPENER, |session| session.write(b"anyone there\n"))
            .expect("open");
        assert!(matches!(refusal, Err(Error::Ended)));
    }

    #[test]
    fn acting_on_a_terminal_that_was_closed_says_so() {
        let terminals = Terminals::new();
        let id = terminals
            .open("someone", OPENER, &opening(&["cat"]))
            .expect("opens");
        terminals.close(&id, OPENER);
        assert!(matches!(
            terminals.with(&id, OPENER, Session::exit),
            Err(Error::Unknown)
        ));
    }

    /// The name of a terminal is guessable, so this is what stands between one
    /// package and a shell somebody else raised. Writing into it would be a
    /// command run under whatever that shell is.
    #[test]
    fn somebody_elses_terminal_is_answered_as_though_it_did_not_exist() {
        let terminals = Terminals::new();
        let id = terminals
            .open("one", OPENER, &opening(&["cat"]))
            .expect("opens");

        assert!(matches!(
            terminals.with(&id, "somebody-else", Session::exit),
            Err(Error::Unknown)
        ));
        let written = terminals.with(&id, "somebody-else", |session| session.write(b"whoami\n"));
        assert!(matches!(written, Err(Error::Unknown)));

        assert!(terminals.list("one", "somebody-else").is_empty());
        assert_eq!(
            terminals.list("one", OPENER).len(),
            1,
            "and the real one still has it"
        );
        terminals.close(&id, OPENER);
    }

    #[test]
    fn somebody_else_cannot_close_it_either() {
        let terminals = Terminals::new();
        let id = terminals
            .open("one", OPENER, &opening(&["cat"]))
            .expect("opens");

        terminals.close(&id, "somebody-else");
        assert!(
            terminals.with(&id, OPENER, Session::exit).is_ok(),
            "still open"
        );

        terminals.close_owned_by("one", "somebody-else");
        assert!(
            terminals.with(&id, OPENER, Session::exit).is_ok(),
            "and still open"
        );

        terminals.close(&id, OPENER);
        assert!(matches!(
            terminals.with(&id, OPENER, Session::exit),
            Err(Error::Unknown)
        ));
    }
}
