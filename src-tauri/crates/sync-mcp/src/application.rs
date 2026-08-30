//! The channel back to Sync itself.
//!
//! Every other message on the host socket goes one way: the window asks, this
//! process answers. This is the one that goes the other, and it exists because
//! of where a tool's body has to run.
//!
//! **A tool an extension offers is executed in the application, not here.** Its
//! body reaches the keychain, the network under the manifest's host list, the
//! artefact on this machine and `work.order` — every one of which lives in Sync
//! and none of which lives in this process. A second runtime here would be a
//! second place an extension's logic lives, and the two would come to disagree
//! about what a package may reach. So this process decides *whether* a call is
//! allowed to be made, and Sync makes it.
//!
//! # Why the application connects rather than being connected to
//!
//! Sync spawns this process and outlives it, so a call in this direction cannot
//! be made to a door of our own — there is nowhere to knock. Instead the
//! application takes one connection on the socket it already has and says
//! [`ATTEND`], and that connection is inverted: requests go out on it, answers
//! come back on it, matched by `id` exactly as JSON-RPC matches anything else.
//!
//! It is a connection of its own rather than one of the attached ones, and the
//! difference is when it exists. A connection with a project on it lives for as
//! long as somebody has that project open; an agent calls a tool with no window
//! anywhere, at three in the morning, so the channel is taken at start-up and
//! held.
//!
//! # What a caller here gets
//!
//! One `await` and one of three outcomes: the answer, whatever Sync refused, or
//! a refusal in words saying the application is not reachable. Never a wait
//! with no end — see [`PATIENCE`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use sync_memory::{MemoryError, Result};
use tokio::sync::{mpsc, oneshot};

/// How long one call waits for the application before giving up.
///
/// **Sixty seconds, taken from what an agent's own client holds a call open
/// for** rather than from anything about this process. Our honest worst path is
/// the network door's twenty seconds plus a keychain that may be asking
/// somebody for permission, and past a minute the agent that asked has almost
/// certainly abandoned the call — so waiting longer produces an answer nobody
/// is listening for while holding this process's one handler slot.
///
/// A ceiling rather than a policy: what a slow tool should *do* is
/// `task-c60f38`'s question, and the number moves when that is measured. What
/// matters here is that the wait ends, and ends in a sentence.
const PATIENCE: Duration = Duration::from_secs(60);

/// Sync, as this process can reach it.
///
/// Shared by both doors: the socket puts a connection into it and the agents'
/// server takes calls out, so one `Arc` is held by each. Empty is the ordinary
/// state for a `sync-mcp` somebody started themselves — there is no application
/// on the other end, and every call says so.
#[derive(Default)]
pub struct Application {
    /// Where a request is written, when there is anywhere to write it.
    ///
    /// A channel rather than the socket's write half, so that two calls racing
    /// each other cannot interleave halves of two lines: the writer is one task
    /// and this is its queue.
    attending: Mutex<Option<mpsc::UnboundedSender<String>>>,
    /// Calls that have gone out and not come back, by the id they went out
    /// under.
    waiting: Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>,
    next: AtomicU64,
}

impl Application {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a connection as the channel back, replacing any earlier one.
    ///
    /// Replacing rather than refusing: an application that reconnected after
    /// this process was restarted, or after its own connection broke, is the
    /// ordinary case and the newer connection is the live one. Whoever held it
    /// before is dropped, which ends its writer task.
    pub fn attend(&self, writer: mpsc::UnboundedSender<String>) {
        if let Ok(mut attending) = self.attending.lock() {
            *attending = Some(writer);
        }
    }

    /// Give up the channel and fail everything still waiting on it.
    ///
    /// **Failing them is the point.** A call whose answer can no longer arrive
    /// would otherwise sit until its patience runs out, and the agent would
    /// wait a minute to be told something this process knew the moment the
    /// connection closed.
    pub fn withdrew(&self) {
        if let Ok(mut attending) = self.attending.lock() {
            *attending = None;
        }
        let Ok(mut waiting) = self.waiting.lock() else {
            return;
        };
        for (_, answer) in waiting.drain() {
            let _ = answer.send(Err(unreachable(
                "Sync closed the channel while this call was in flight",
            )));
        }
    }

    /// Hand one answer back to whoever is waiting for it.
    ///
    /// An id nobody is waiting for is dropped in silence, which is what a
    /// duplicate answer or one that arrived after its patience ran out is.
    /// Neither is worth ending a connection over.
    pub fn answered(&self, id: u64, answer: Result<Value>) {
        let waiting = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut held| held.remove(&id));
        if let Some(waiting) = waiting {
            let _ = waiting.send(answer);
        }
    }

    /// Ask Sync to do one thing, and wait for what it says.
    ///
    /// # Errors
    ///
    /// `unreachable` when no application is on the channel, when it went away
    /// mid-call, or when [`PATIENCE`] ran out; otherwise whatever Sync refused,
    /// in its own words.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = oneshot::channel();

        {
            let Ok(attending) = self.attending.lock() else {
                return Err(unreachable("the channel to Sync is unusable"));
            };
            let Some(writer) = attending.as_ref() else {
                return Err(unreachable(
                    "Sync is not on the other end of this engine's channel, so nothing can run a tool right now",
                ));
            };
            // Registered before the line goes out, because the answer can be
            // read by another task the instant it is written.
            if let Ok(mut waiting) = self.waiting.lock() {
                waiting.insert(id, answer);
            }
            let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            if writer.send(request.to_string()).is_err() {
                self.forget(id);
                return Err(unreachable(
                    "Sync's channel closed before the call went out",
                ));
            }
        }

        match tokio::time::timeout(PATIENCE, answered).await {
            Ok(Ok(answer)) => answer,
            // The sender was dropped without answering: the connection ended
            // and `withdrew` did not reach this one.
            Ok(Err(_)) => Err(unreachable("Sync did not answer this call")),
            Err(_) => {
                self.forget(id);
                Err(unreachable(&format!(
                    "Sync did not answer within {} seconds",
                    PATIENCE.as_secs()
                )))
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&id);
        }
    }
}

/// Why nothing could be run, said as the engine says everything else.
///
/// One `kind` for every way the channel can be unavailable, because they are
/// one thing to whoever reads it: the tool did not run and it was not the
/// package's fault. What differs between them is the sentence.
fn unreachable(why: &str) -> MemoryError {
    MemoryError::domain("unreachable", why.to_owned(), Value::Null)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Arc;

    use super::*;

    /// A `sync-mcp` somebody started themselves has no application behind it,
    /// and every tool call has to say so **now**. The alternative is a minute
    /// of silence and then the same answer, which the agent has long stopped
    /// waiting for.
    #[tokio::test]
    async fn a_call_with_nobody_attending_is_refused_at_once() {
        let application = Application::new();

        let refused = application
            .call("extension.tool", json!({}))
            .await
            .expect_err("there is nobody to run it");

        assert!(
            refused.to_string().contains("not on the other end"),
            "the refusal says what is missing: {refused}"
        );
    }

    /// The ordinary path: a request goes out carrying an id, and the answer
    /// that comes back under that id is what the caller gets.
    #[tokio::test]
    async fn an_answer_reaches_whoever_asked_for_it() {
        let application = Arc::new(Application::new());
        let (queue, mut queued) = mpsc::unbounded_channel::<String>();
        application.attend(queue);

        let asking = Arc::clone(&application);
        let call =
            tokio::spawn(async move { asking.call("extension.tool", json!({"a": 1})).await });

        let request: Value =
            serde_json::from_str(&queued.recv().await.expect("a request went out"))
                .expect("it is JSON");
        assert_eq!(request["method"], "extension.tool");
        assert_eq!(request["params"]["a"], 1);
        let id = request["id"].as_u64().expect("an id to answer under");

        application.answered(id, Ok(json!({"answered": true})));

        let answer = call.await.expect("the call finished").expect("it answered");
        assert_eq!(answer, json!({"answered": true}));
    }

    /// An answer nobody is waiting for is dropped rather than reported. It is
    /// what a duplicate is, and what one that arrived after its caller gave up
    /// is — neither is worth ending the channel over.
    #[tokio::test]
    async fn an_answer_to_nothing_is_dropped_quietly() {
        let application = Application::new();
        application.answered(41, Ok(json!({"answered": true})));
    }

    /// The channel closing fails what is in flight immediately.
    ///
    /// Otherwise a call whose answer can never arrive sits until its patience
    /// runs out, and the agent waits a minute to be told something this process
    /// knew the moment the connection ended.
    #[tokio::test]
    async fn calls_in_flight_are_failed_when_the_channel_goes() {
        let application = Arc::new(Application::new());
        let (queue, mut queued) = mpsc::unbounded_channel::<String>();
        application.attend(queue);

        let asking = Arc::clone(&application);
        let call = tokio::spawn(async move { asking.call("extension.tool", json!({})).await });
        queued.recv().await.expect("the request went out");

        application.withdrew();

        let refused = call
            .await
            .expect("the call finished")
            .expect_err("nothing can answer it now");
        assert!(
            refused.to_string().contains("closed the channel"),
            "the refusal says what happened: {refused}"
        );

        // And the channel is gone rather than merely quiet: the next call is
        // refused for having nobody to reach, not for waiting too long.
        let after = application
            .call("extension.tool", json!({}))
            .await
            .expect_err("there is nobody to run it now");
        assert!(
            after.to_string().contains("not on the other end"),
            "{after}"
        );
    }

    /// A second connection replaces the first, which is what a reconnection is.
    #[tokio::test]
    async fn the_newer_connection_is_the_live_one() {
        let application = Arc::new(Application::new());
        let (first, mut was) = mpsc::unbounded_channel::<String>();
        application.attend(first);
        let (second, mut is) = mpsc::unbounded_channel::<String>();
        application.attend(second);

        let asking = Arc::clone(&application);
        tokio::spawn(async move { asking.call("extension.tool", json!({})).await });

        assert!(
            is.recv().await.is_some(),
            "the request went out on the new one"
        );
        assert!(
            was.try_recv().is_err(),
            "and not on the connection that was replaced"
        );
    }
}
