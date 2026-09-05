//! Which device is watching which conversation.
//!
//! The narrowest thing in this process, and deliberately: it is the whole of
//! what the reverse direction on a device's connection amounts to. A device
//! asks to watch a session, this mints a number for the watch and remembers
//! which connection's queue it belongs to, and every event the application
//! afterwards says under that number is written onto that connection. Nothing
//! else travels that way. No call does.
//!
//! # Why the engine holds this rather than the application
//!
//! The application cannot see a device leave. It writes events into a socket to
//! an engine that is still there; the connection that ended is one hop further
//! on, and a session with a watcher nobody drains goes on serialising its every
//! word for as long as the conversation runs. This side is where the ending is
//! visible, so this side is what says so — once per connection, naming what it
//! held, rather than once per event.
//!
//! # Why a number rather than the session's own key
//!
//! Two devices may watch one conversation, and a person's phone reconnecting is
//! a third watch of it before the first two have been noticed gone. A key would
//! make those one entry with three owners; a number makes them three entries
//! with one owner each, which is what letting one of them go requires.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

/// Every watch this process is holding, across every device.
#[derive(Default)]
pub(crate) struct Subscriptions {
    /// Where each watch's events are written.
    ///
    /// The queue is the connection's own — the one every answer on it also goes
    /// through — so an event and an answer written at the same moment are two
    /// lines rather than two halves of one.
    held: Mutex<HashMap<u64, UnboundedSender<String>>>,
    /// Never reused, and not merely unique. A number handed out again after the
    /// watch under it ended would take the events of a conversation somebody
    /// else is now watching.
    next: AtomicU64,
}

impl Subscriptions {
    /// Take a number for a watch on this connection.
    pub(crate) fn mint(&self, queue: UnboundedSender<String>) -> u64 {
        // From one rather than zero: the greeting on the network door is
        // written under id 0, and a number that reads as both is a number
        // somebody debugging a connection has to disambiguate by eye.
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut held) = self.held.lock() {
            held.insert(id, queue);
        }
        id
    }

    /// Put one event on the connection watching under `subscription`.
    ///
    /// A number nobody holds is dropped in silence. It is what an event says
    /// about a watch that has just been let go of, and the application is told
    /// about that separately and in one message — reporting each straggler
    /// would be reporting the same fact once per word the agent wrote.
    pub(crate) fn deliver(&self, subscription: u64, event: &Value) {
        let queue = self
            .held
            .lock()
            .ok()
            .and_then(|held| held.get(&subscription).cloned());
        let Some(queue) = queue else {
            return;
        };
        let line = json!({
            "jsonrpc": "2.0",
            "method": sync_memory::SESSION_EVENT,
            "params": {"subscription": subscription, "event": event},
        });
        // A queue whose writer has gone is a connection that ended between this
        // event being taken off the socket and being put on the wire. Nothing to
        // do about it here: the connection's own loop is already on its way to
        // saying so.
        let _ = queue.send(line.to_string());
    }

    /// Let go of the watches a connection held, and say which they were.
    ///
    /// Called once, when the connection ends, whichever way it ended. What
    /// comes back is what the application has to be told, and it is empty for
    /// the ordinary connection that watched nothing.
    pub(crate) fn ended(&self, mine: &[u64]) -> Vec<u64> {
        let Ok(mut held) = self.held.lock() else {
            return Vec::new();
        };
        mine.iter()
            .copied()
            .filter(|subscription| held.remove(subscription).is_some())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn queue() -> (
        UnboundedSender<String>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
    ) {
        tokio::sync::mpsc::unbounded_channel()
    }

    /// An event reaches the connection that asked for the watch, under the
    /// number that connection was given.
    #[test]
    fn an_event_goes_to_the_connection_watching_under_that_number() {
        let subscriptions = Subscriptions::default();
        let (writing, mut written) = queue();
        let watch = subscriptions.mint(writing);

        subscriptions.deliver(watch, &json!({"kind": "status", "seq": 3}));

        let line: Value = serde_json::from_str(&written.try_recv().expect("a line went out"))
            .expect("it is JSON");
        assert_eq!(line["method"], sync_memory::SESSION_EVENT);
        assert_eq!(line["params"]["subscription"], watch);
        assert_eq!(line["params"]["event"]["seq"], 3);
        // A notification and nothing else: an id would make it a request, and a
        // request in this direction is the one thing this door does not carry.
        assert!(line.get("id").is_none(), "{line}");
    }

    /// Two devices watching one conversation are two watches, and an event for
    /// one does not reach the other.
    #[test]
    fn two_watches_are_two_numbers_and_do_not_share_a_connection() {
        let subscriptions = Subscriptions::default();
        let (first, mut heard_first) = queue();
        let (second, mut heard_second) = queue();
        let one = subscriptions.mint(first);
        let two = subscriptions.mint(second);
        assert_ne!(one, two);

        subscriptions.deliver(one, &json!({"seq": 0}));

        assert!(heard_first.try_recv().is_ok());
        assert!(
            heard_second.try_recv().is_err(),
            "the other device heard somebody else's conversation"
        );
    }

    /// A connection that ended says what it held, once, and holds nothing
    /// after.
    #[test]
    fn a_connection_that_ended_gives_up_what_it_held() {
        let subscriptions = Subscriptions::default();
        let (writing, mut written) = queue();
        let watch = subscriptions.mint(writing);

        assert_eq!(subscriptions.ended(&[watch]), vec![watch]);
        // Said once. A second ending is not a second thing to tell the
        // application about.
        assert!(subscriptions.ended(&[watch]).is_empty());

        subscriptions.deliver(watch, &json!({"seq": 1}));
        assert!(
            written.try_recv().is_err(),
            "an event was written to a watch that had been let go of"
        );
    }

    /// A number is never handed out twice, including after its watch ended.
    ///
    /// Reuse would hand the events of one conversation to whoever is watching
    /// another — a defect that appears only on a device that reconnects often,
    /// which is the ordinary condition of a phone.
    #[test]
    fn a_number_is_never_given_out_again() {
        let subscriptions = Subscriptions::default();
        let mut seen = Vec::new();
        for _ in 0..8 {
            let (writing, _held) = queue();
            let watch = subscriptions.mint(writing);
            assert!(!seen.contains(&watch), "{watch} was minted twice");
            seen.push(watch);
            subscriptions.ended(&[watch]);
        }
        assert!(!seen.contains(&0), "0 is the greeting's id: {seen:?}");
    }
}
