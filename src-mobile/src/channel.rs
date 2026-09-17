//! The phone's end of the host channel.
//!
//! One connection to one computer, dialled by the public key on the pairing
//! code and kept for as long as the application runs. What travels on it is the
//! channel the window speaks — the same operations, the same framing, the same
//! version in the handshake — so this module owns the transport and nothing of
//! the vocabulary: [`sync_memory::Transport`] and `Connection` already spell
//! that, and a second spelling here would be a second place to get it wrong.
//!
//! # Why a thread rather than a task
//!
//! `Transport` is blocking by construction: the desktop's is a pipe to a child
//! process, read with `std::io`. `iroh` is async. Rather than write a second
//! async client for the sake of one connection, the connection lives on a
//! thread of its own that blocks on the runtime, and everything else reaches it
//! by asking. One thread parked on a socket costs a stack; a second client
//! would cost every rule the first one holds.
//!
//! # Why the connection is read even when nothing was asked
//!
//! It was not, and could not be. A call used to own the wire until its answer
//! came back, so between two calls nothing on this phone was reading — which is
//! exactly the state a conversation with an agent spends its time in. A word
//! the agent writes arrives with nothing outstanding, so it arrived nowhere.
//!
//! What replaced it is a reader that never stops and a writer beside it: a call
//! takes a number, leaves somewhere for its answer to be put, and waits. A line
//! with a number on it is somebody's answer; a line without one is the only
//! thing this channel carries that nobody asked for, and it goes to whoever is
//! watching that conversation. **No call arrives this way.** The door will not
//! carry one, and there is nothing here that would run it.
//!
//! # What a refusal from the door means here
//!
//! The door answers every rejected connection with one sentence and never says
//! which check produced it. That sentence is carried back to the person
//! unchanged. Inventing a friendlier one here would be inventing a claim about
//! why — the one thing the door deliberately does not say.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use iroh::endpoint::{Connection as Quic, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointId};
use serde_json::{Value, json};
use sync_memory::{
    CHANNEL_VERSION, Effect, MAX_FRAME_BYTES, METHODS, MemoryError, Operations, REMOTE_HELLO,
    Subscription, Transport, effect,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt as _, BufReader};
use tokio::runtime::Runtime;

/// What this machine is called on the wire, and what the door answers to.
///
/// Stated here rather than imported because the door is in the other
/// application: the two spell the same eight bytes, and the QUIC handshake is
/// what compares them — a client speaking anything else is refused before
/// either end has written a line.
const ALPN: &[u8] = b"sync/host/1";

/// How long a call may take before the phone stops waiting for it.
///
/// The connection can be alive and the answer still never come — a computer
/// that went to sleep between the question and the answer leaves exactly that.
/// Thirty seconds is longer than dialling a machine that is there and shorter
/// than somebody staring at a screen will tolerate.
const PATIENCE: Duration = Duration::from_secs(30);

/// How long one call may take before this phone stops waiting for it.
///
/// **Longer than the dial above, and deliberately longer than the computer's
/// own ceiling**, which is sixty seconds: raising an agent is an operation of
/// this channel now, and it starts a process and waits for it to speak. What a
/// person deserves when that takes too long is the computer's sentence about
/// which agent would not start — so this phone has to still be listening when
/// that sentence is written, and a deadline shorter than the far end's would
/// replace it with one of ours saying nothing.
const ANSWERING: Duration = Duration::from_secs(75);

/// How long to wait before dialling again, the first time.
///
/// Short, because the ordinary reason to be here is the one iOS causes on
/// purpose: the system suspends the application seconds after it goes into the
/// background and takes the connection with it, so *coming back* is the common
/// case rather than *the network is gone*. A person who has just looked at
/// their phone again should not be able to read the wait.
const FIRST_REST: Duration = Duration::from_millis(400);

/// The longest this phone will wait between dials.
///
/// Doubling from the first rest reaches it in seven tries, about a minute in.
/// Past that there is nothing left to learn by asking more often: a phone in a
/// pocket with no network stays that way for as long as it stays there, and
/// what a shorter ceiling buys is heat rather than a connection. The waits are
/// cut short from the outside anyway — the keeper is woken the moment a
/// connection drops or somebody presses *Try again* — so this is the interval
/// for a phone nobody is looking at.
const LONGEST_REST: Duration = Duration::from_secs(30);

/// The pairing this phone is holding: where to dial, and what to say.
#[derive(Clone)]
pub struct Pairing {
    pub endpoint: String,
    pub secret: String,
}

/// Where this phone stands with its computer.
///
/// **Three states and not one boolean, because a person was being shown the
/// same nothing for two different situations.** *Not connected* was the answer
/// while the first dial of a launch was still in flight and the answer after a
/// dial had failed, and the two ask opposite things of somebody reading it:
/// one is *wait*, and the other is *this is not going to happen by itself,
/// look at the computer*. And the second carried no reason at all, though the
/// reason had been in hand — the door's refusal, the address that names
/// nothing reachable, the two builds that cannot speak to each other — and was
/// being thrown away one line after it arrived.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    /// No computer to dial. The pairing screen is what answers this, and it is
    /// the only one of these that is not about a network.
    #[default]
    Unpaired,
    /// Dialling now. Nothing has failed; nothing has been established either.
    Reaching,
    /// There is a connection and calls go out on it.
    Talking,
    /// The last dial failed, in the words of whoever refused it.
    ///
    /// Not the end of trying: the keeper goes on dialling behind this, and the
    /// sentence is what is true *now* rather than a verdict. Which is why it
    /// carries words rather than a code — nothing branches on this, somebody
    /// reads it.
    Away(String),
}

impl Reach {
    /// The one word the window matches on.
    const fn word(&self) -> &'static str {
        match self {
            Self::Unpaired => "unpaired",
            Self::Reaching => "reaching",
            Self::Talking => "talking",
            Self::Away(_) => "away",
        }
    }

    /// What the window is handed: the word, and the sentence where there is
    /// one.
    ///
    /// The sentence belongs to [`Self::Away`] alone. A reason attached to
    /// *reaching* would be a reason for something that has not failed, and a
    /// window drawing whatever it was given would report a dial in flight as a
    /// dial that went wrong.
    #[must_use]
    pub fn told(&self) -> Value {
        json!({
            "reach": self.word(),
            "trouble": match self {
                Self::Away(said) => json!(said),
                _ => Value::Null,
            },
        })
    }
}

/// What went wrong, in the words the person is shown.
///
/// One type rather than a chain of `Box<dyn Error>`: every one of these is
/// read by somebody looking at a phone, so each one has to be a sentence
/// already.
#[derive(Debug)]
pub struct Trouble(pub String);

impl std::fmt::Display for Trouble {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

impl std::error::Error for Trouble {}

impl Trouble {
    fn saying(what: impl std::fmt::Display) -> Self {
        Self(what.to_string())
    }
}

/// What cuts a wait short.
///
/// The keeper below spends its life asleep, and the two things worth waking it
/// for both happen somewhere else: a connection ends on the thread that was
/// reading it, and a person presses *Try again* on a Tauri command's thread. A
/// plain `sleep` would make both of them wait out a rest that is no longer
/// about anything — up to half a minute of a phone showing *cannot reach your
/// computer* after the reason it could not had gone away.
#[derive(Default)]
struct Waker {
    rung: Mutex<bool>,
    bell: Condvar,
}

impl Waker {
    /// Wake the keeper, whether or not it is listening yet.
    ///
    /// The flag is what makes the second half true. A bell rung while the
    /// keeper is dialling rather than sleeping is a bell nobody hears, and the
    /// keeper would then sleep through the very event that rang it — so the
    /// ringing is remembered and the next wait returns at once.
    fn ring(&self) {
        if let Ok(mut rung) = self.rung.lock() {
            *rung = true;
        }
        self.bell.notify_all();
    }

    /// Sleep for `at_most`, or until somebody rings.
    fn waited(&self, at_most: Duration) {
        let Ok(mut rung) = self.rung.lock() else {
            // A poisoned lock is a keeper with no way to be woken, and a keeper
            // that then spun would be a phone dialling as fast as it can. The
            // wait still happens; only the shortcut is lost.
            std::thread::sleep(at_most);
            return;
        };
        if !*rung {
            // The guard is taken back whichever way the wait ended, and the
            // outcome is not read: rung or timed out, what follows is the same
            // turn of the loop.
            rung = match self.bell.wait_timeout(rung, at_most) {
                Ok((rung, _)) => rung,
                Err(_) => return,
            };
        }
        *rung = false;
    }
}

/// One QUIC stream, read and written as the channel's lines.
///
/// The `Runtime` is held rather than borrowed: this is what makes the blocking
/// trait true. Both halves of the stream belong to it, and dropping the
/// transport drops the connection with them.
struct Wire {
    runtime: Runtime,
    writing: SendStream,
    reading: BufReader<RecvStream>,
    /// Dropping this closes the streams, so it is held even though nothing
    /// reads it.
    _connection: Quic,
    /// The socket everything above is carried on, held for the same reason and
    /// a worse failure.
    ///
    /// A connection does not keep its endpoint alive. Dropped, it takes the
    /// socket with it and nothing drives the connection any more — and the
    /// handshake has already finished by then, so this side believes it is
    /// connected and the computer sees a connection that opens no stream and
    /// times out. It cost an afternoon: the phone said *connection lost* and
    /// then *the computer did not answer*, and the door's log said `no stream
    /// on the connection: timed out`, which is the same event told from the
    /// only end that could see it.
    _endpoint: Endpoint,
}

impl Transport for Wire {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        let line = format!("{message}\n");
        // The stream's own error rather than an I/O one: `Transport` speaks
        // `io::Error`, and QUIC's reasons for refusing a write are its own.
        self.runtime
            .block_on(async { self.writing.write_all(line.as_bytes()).await })
            .map_err(io::Error::other)
    }

    /// One line, and never more of one than a frame may be.
    ///
    /// Read by hand rather than with `lines()`, and the ceiling is the reason:
    /// that helper grows its buffer to whatever arrives, so a computer sending
    /// a line with no newline in it would be as much of this phone's memory as
    /// it cared to take. The channel has a frame size and the other end
    /// enforces it on what it reads; a limit only one end honours is a limit
    /// only one end has.
    ///
    /// An oversized line ends the connection rather than being skipped. What
    /// has been read is part of a message this build cannot parse, and reading
    /// on from the middle of one would put every answer after it against the
    /// wrong question.
    fn receive(&mut self) -> io::Result<Option<Value>> {
        let mut line = Vec::new();
        let read = self
            .runtime
            .block_on(async {
                let mut capped = (&mut self.reading).take(MAX_FRAME_BYTES as u64 + 1);
                tokio::time::timeout(PATIENCE, capped.read_until(b'\n', &mut line)).await
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "the computer did not answer")
            })??;
        if read == 0 {
            return Ok(None);
        }
        if line.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the computer sent a message larger than this channel carries",
            ));
        }
        serde_json::from_slice(&line)
            .map(Some)
            .map_err(|broken| io::Error::new(io::ErrorKind::InvalidData, broken))
    }
}

/// One conversation this phone is being shown as it happens.
///
/// The window made the sink and this holds it, along with the two things
/// re-asking for the watch needs after a connection dropped: which session it
/// is, and how far into it this phone has already got.
pub struct Watched {
    /// The session on the computer, as the computer calls it.
    key: String,
    /// Where the window put the events. Made in the webview, so it belongs to
    /// one load of it and dies with a reload.
    events: tauri::ipc::Channel<Value>,
    /// The highest sequence number this phone has handed the window.
    ///
    /// **The one member of an event this file reads.** Everything else on this
    /// channel is carried without being looked into, and this is the exception
    /// with a reason: after a connection drops the watch is asked for again,
    /// and the computer replays from wherever it is told to. Nobody but this
    /// phone knows where the window's transcript stopped — the computer knows
    /// what it said, not what arrived — so if this did not read it, a reconnect
    /// would write the whole conversation into a screen that already holds it.
    seen: Mutex<Option<u64>>,
}

/// Everything this phone is watching, by the number the computer gave each
/// watch — and, while a watch is being asked for, the words that arrived
/// before it existed.
///
/// By number rather than by session because the numbers are what events arrive
/// under, and because a phone that reconnected can be holding the old watch and
/// the new one for a moment. The session key lives inside each entry, for the
/// one caller that needs it: asking for the watch again.
#[derive(Default)]
pub struct Watching {
    register: Mutex<Register>,
}

/// The watches, and what came in ahead of one.
///
/// One lock over both, and that is the point rather than an economy: an event
/// that was kept and an event that arrives next must reach the window in the
/// order the computer wrote them, and two locks is exactly how the second gets
/// in front of the first.
#[derive(Default)]
struct Register {
    /// The watches this phone holds, by the computer's number for each.
    held: std::collections::HashMap<u64, Arc<Watched>>,
    /// Words of a conversation that arrived under a number nothing holds yet,
    /// in the order they arrived.
    ///
    /// **This is a conversation opened on the computer, and it is the ordinary
    /// case rather than a race.** Asking to watch one is answered with the
    /// number the watch will be known by — and the computer replays everything
    /// already said *before* it writes that answer, because the replay and the
    /// answer go out on one socket and the replay is what the call does. So a
    /// phone that only kept what arrives after the answer kept nothing at all:
    /// the conversation opened, empty, and stayed that way until somebody said
    /// something new into it.
    early: std::collections::HashMap<u64, Vec<Value>>,
    /// How many watches are being asked for right now.
    ///
    /// Nothing is kept when this is zero, which is what stops [`Self::early`]
    /// from becoming a place words go to be forgotten in: an event under a
    /// number nobody is waiting for is a watch let go of a moment ago, and it
    /// is dropped exactly as it always was.
    asking: usize,
}

/// How many words of one conversation are kept for a watch not yet in hand.
///
/// The replay itself cannot reach it: the computer keeps a bounded history per
/// conversation and replays that (`src-tauri/src/sessions/live.rs`), and this
/// stands well above it. What could reach it is a number nobody is going to
/// claim — a watch this phone let go of while another was being asked for —
/// and there the ceiling is the whole point.
const KEPT_AHEAD: usize = 16_384;

impl Watching {
    /// A watch is being asked for, so hold on to whatever arrives under a
    /// number nothing has claimed yet.
    ///
    /// The keeping ends when the value returned is dropped, which is what makes
    /// an ask that failed cost nothing: it ends on the way out of the call
    /// whether the computer answered, refused, or went away mid-question.
    fn expecting(&self) -> Expected<'_> {
        if let Ok(mut register) = self.register.lock() {
            register.asking += 1;
        }
        Expected { watching: self }
    }

    /// Nobody is asking for a watch any more.
    fn done_asking(&self) {
        if let Ok(mut register) = self.register.lock() {
            register.asking = register.asking.saturating_sub(1);
            if register.asking == 0 {
                // Whatever is still here belongs to a number no watch claimed,
                // and none will: every ask that was outstanding has been
                // answered. Kept any longer it would be delivered to whichever
                // watch the computer next minted that number for — somebody
                // else's conversation, in somebody else's transcript.
                register.early.clear();
            }
        }
    }

    /// Hold a watch under the number the computer minted for it, and give it
    /// what was said before it was in hand.
    ///
    /// The replay is written out while the register is still locked, so that a
    /// word arriving on the connection in the same instant queues behind it
    /// rather than in front of it. A send touches the webview and nothing here,
    /// so there is no way back into this lock.
    fn hold(&self, subscription: u64, watched: Arc<Watched>) {
        let Ok(mut register) = self.register.lock() else {
            return;
        };
        let early = register.early.remove(&subscription).unwrap_or_default();
        register.held.insert(subscription, Arc::clone(&watched));
        for event in early {
            if !given(&watched, &event) {
                register.held.remove(&subscription);
                return;
            }
        }
    }

    /// Give the window one thing a conversation said.
    ///
    /// A sink the window cannot be reached on any more is let go of here. That
    /// is what a reloaded webview leaves behind: the channel it made belongs to
    /// the load that made it, and nothing tells this side that load has gone.
    fn shown(&self, subscription: u64, event: &Value) {
        let Ok(mut register) = self.register.lock() else {
            return;
        };
        if let Some(watched) = register.held.get(&subscription).cloned() {
            if !given(&watched, event) {
                register.held.remove(&subscription);
            }
            return;
        }
        if register.asking == 0 {
            return;
        }
        let kept = register.early.entry(subscription).or_default();
        if kept.len() < KEPT_AHEAD {
            kept.push(event.clone());
        }
    }

    /// Stop holding one watch, and say which session it was of.
    fn forget(&self, subscription: u64) -> Option<Arc<Watched>> {
        let mut register = self.register.lock().ok()?;
        register.early.remove(&subscription);
        register.held.remove(&subscription)
    }

    /// The number this phone holds for a session, if it holds one.
    ///
    /// The window asks to stop watching by naming the conversation, because
    /// that is what a screen knows; the computer is told by naming the watch,
    /// because a conversation may have several. This is where the one becomes
    /// the other.
    fn number_for(&self, key: &str) -> Option<u64> {
        let register = self.register.lock().ok()?;
        register
            .held
            .iter()
            .find(|(_, watched)| watched.key == key)
            .map(|(subscription, _)| *subscription)
    }

    /// Take every watch off the register and hand them back.
    ///
    /// What a reconnection starts with. The numbers do not survive it — the
    /// computer mints a fresh one for each new watch — so the old ones are
    /// dropped here rather than left to be overwritten one by one, which would
    /// leave a phone holding a number that means nothing if any of the re-asks
    /// failed. What was kept ahead of a watch goes with them, for the same
    /// reason: it is filed under numbers this connection no longer means.
    fn taken(&self) -> Vec<Arc<Watched>> {
        self.register
            .lock()
            .map(|mut register| {
                register.early.clear();
                register.held.drain().map(|(_, watched)| watched).collect()
            })
            .unwrap_or_default()
    }
}

/// One word of a conversation put in front of the window, and whether the
/// window was still there to take it.
///
/// Where this phone's place in a conversation is written down, and it is
/// written before the send rather than after: a window that has gone still
/// moves the mark along, so the watch that replaces it does not begin by
/// replaying what this one was shown.
fn given(watched: &Watched, event: &Value) -> bool {
    if let Some(seq) = event.get("seq").and_then(Value::as_u64)
        && let Ok(mut seen) = watched.seen.lock()
    {
        *seen = Some(seen.map_or(seq, |was| was.max(seq)));
    }
    watched.events.send(event.clone()).is_ok()
}

/// A watch this phone has asked for and not yet been given a number for.
///
/// Nothing but a span of time: while one of these is alive, a word arriving
/// under a number nothing holds is kept instead of dropped.
struct Expected<'a> {
    watching: &'a Watching,
}

impl Drop for Expected<'_> {
    fn drop(&mut self) {
        self.watching.done_asking();
    }
}

/// The channel, as everything else in this application sees it.
///
/// Held by the application and asked from any thread. The lock is what makes
/// two screens asking at once two calls in a row rather than two half-written
/// lines: the pipelining that lets the computer answer out of order is the
/// door's business, and one phone gains nothing by it.
///
/// **Which computer this is and whether it is being spoken to are two facts,
/// and they are kept apart.** They were one, and the launch is where that
/// showed: dialling takes a relay and a handshake, so the window came up and
/// asked for the list of projects while the first dial was still going — and
/// was told this phone is not paired, which was untrue and looked like a
/// pairing that had been lost. A pairing is held from the moment it is known;
/// a conversation is opened when there is something to say.
#[derive(Default)]
pub struct Channel {
    /// The computer this phone belongs to, whether or not it can be reached.
    holding: Mutex<Option<Pairing>>,
    /// The conversation, while there is one.
    talking: Mutex<Option<Live>>,
    /// Held across a dial, so that two screens asking at once make one.
    ///
    /// Without it the first call of a launch and the one beside it both find
    /// nothing to talk on and both dial, and the second connection replaces the
    /// first while the first is being asked on.
    dialling: Mutex<()>,
    /// The conversations this phone is being shown as they happen.
    ///
    /// Outside the connection rather than inside it, and that is the point: a
    /// watch outlives the connection it was taken on. A phone that lost its
    /// network is a phone that still has a conversation open on the screen, and
    /// what it does when the network comes back is ask for the same watch from
    /// where it stopped.
    watching: Arc<Watching>,
    /// Where this phone stands, as one fact rather than as something read off
    /// three others.
    ///
    /// Derived from the members above it would be a lie in the one state that
    /// matters: *dialling* and *dialled and failed* are both "a pairing and no
    /// connection", and telling them apart is half of what this exists for.
    /// The other half is the sentence, which nothing above keeps.
    reach: Mutex<Reach>,
    /// How the window is told that changed, once there is a window.
    ///
    /// A callback rather than an `AppHandle`, and not for the sake of tests:
    /// this is built by `Default` before Tauri has an application to hand out
    /// handles of, and a member that could only be filled in later would be a
    /// member every reader has to check.
    told: Mutex<Option<Telling>>,
    /// What cuts short the keeper's wait.
    waker: Arc<Waker>,
}

/// How the window is told that the reach has moved.
type Telling = Box<dyn Fn(&Reach) + Send + Sync>;

/// A connection that is being spoken on, and whether it still is.
///
/// **The flag is the whole of why a dropped connection used to need the
/// application restarted.** The sender outlives the thread that serves it:
/// nothing on the way down sets it aside, so a phone whose computer had gone
/// went on holding one and went on answering *yes, connected* to the only
/// question anything asked. Every call then failed against a receiver that had
/// been dropped, and the re-dial written for exactly this never ran, because
/// the branch in front of it was reading a sender that was merely *present*
/// rather than *alive*.
struct Live {
    asking: Talking,
    /// Set false by the serving thread on its way out, before it rings the
    /// keeper. Read by everything that asks whether there is a connection.
    alive: Arc<AtomicBool>,
}

/// One question and where its answer goes.
///
/// The answer travels back on a channel of its own rather than on a shared
/// one, which is what lets any thread ask without hearing somebody else's
/// answer.
///
/// The failure kept its shape on the way rather than becoming a sentence, and
/// that is what the window needs: it branches on `conflict` and on `locked`,
/// and a computer's refusal flattened into a string here would arrive as a
/// screen that can only apologise.
///
/// Sent on a `tokio` channel and answered on a `std` one, which is exactly what
/// the two ends are: the writer is a task on the connection's runtime, and the
/// caller is a Tauri command on a thread that may block.
type Asked = (String, Value, Sender<sync_memory::Result<Value>>);

impl Channel {
    /// Dial the computer named by a pairing and prove this device may come in.
    ///
    /// Replaces whatever connection was open: pairing again is how a person
    /// says *this one instead*, and two connections would be two answers to
    /// *which computer is this*.
    ///
    /// # Errors
    ///
    /// The address that is not one, the network that will not carry us, or the
    /// door's own refusal — carried back in the door's words.
    pub fn open(&self, pairing: &Pairing) -> Result<(), Trouble> {
        let one_at_a_time = self.dialling.lock().map_err(|_| poisoned())?;
        // Whatever was being watched was being watched on another computer, or
        // on this one under numbers it no longer holds. Either way there is
        // nothing to ask again for: pairing lands a person on the list of
        // projects, which is a screen watching nothing.
        drop(self.watching.taken());
        let dialled = self.dial(pairing);
        drop(one_at_a_time);
        dialled
    }

    /// Say where this phone stands, and tell the window if that has moved.
    ///
    /// Only on a change, because this is called on every turn of the keeper's
    /// loop and most turns are a phone that is still exactly where it was. An
    /// event per turn would be a window re-rendering every half-minute to be
    /// told nothing.
    ///
    /// The window is told while the lock is held. Two transitions cannot then
    /// reach it in the other order from the one they happened in — which is
    /// the whole failure this is worth a lock for, since the last one to arrive
    /// is what a person is looking at. Nothing the window does with it comes
    /// back here.
    fn stands(&self, now: Reach) {
        let Ok(mut reach) = self.reach.lock() else {
            return;
        };
        if *reach == now {
            return;
        }
        *reach = now;
        if let Ok(told) = self.told.lock()
            && let Some(told) = told.as_ref()
        {
            told(&reach);
        }
    }

    /// Where this phone stands, for the window to draw.
    pub fn reach_now(&self) -> Reach {
        self.reach
            .lock()
            .map_or(Reach::Unpaired, |reach| reach.clone())
    }

    /// Say how the window is to be told when that changes.
    ///
    /// Installed once, at setup. Before it there is nowhere to say anything to
    /// and the state is still kept, which is what makes the window's first
    /// question answerable rather than a second source of truth.
    pub fn tell(&self, told: impl Fn(&Reach) + Send + Sync + 'static) {
        if let Ok(mut held) = self.told.lock() {
            *held = Some(Box::new(told));
        }
    }

    /// Dial now rather than at the end of the current wait.
    ///
    /// What *Try again* presses, and what the window says when the system hands
    /// it back to somebody: a phone coming out of a pocket should not spend the
    /// rest of a half-minute rest showing a screen about a network that came
    /// back while it was asleep.
    pub fn reach_soon(&self) {
        self.waker.ring();
    }

    /// Remember the computer this phone belongs to without dialling it.
    ///
    /// What the application does at launch, before the thread that dials gets
    /// anywhere: the window may ask for something in the same moment, and a
    /// phone that has a computer and has not reached it yet must not answer
    /// that it has none.
    pub fn hold(&self, pairing: &Pairing) {
        if let Ok(mut held) = self.holding.lock() {
            *held = Some(pairing.clone());
        }
        // Reaching rather than away: the keeper starts in the same breath as
        // this, so a dial is genuinely in flight. Saying nothing here is what
        // made a launch read as a failure — the window came up, found a pairing
        // and no connection, and had only one way to draw that.
        self.stands(Reach::Reaching);
    }

    /// Make sure there is something to say a call on.
    ///
    /// The one place a connection is opened on somebody's behalf rather than
    /// because they asked to pair. It is checked again after the lock: the wait
    /// may have been for the dial that is now the answer.
    fn reach(&self) -> sync_memory::Result<()> {
        let dialled = {
            let _one_at_a_time = self.dialling.lock().map_err(|_| away(&poisoned().0))?;
            if self.open_now() {
                return Ok(());
            }
            let Some(pairing) = self.pairing() else {
                self.stands(Reach::Unpaired);
                return Err(away("this phone is not paired with a computer"));
            };
            self.dial(&pairing).map_err(|trouble| away(&trouble.0))
        };
        dialled?;
        // Outside the lock, and it has to be: asking for a watch is an ordinary
        // call, and a call that failed reaches for this very function. Inside,
        // that would be a phone deadlocking itself on a bad train.
        self.watch_again();
        Ok(())
    }

    /// Dial, and keep both what answered and what it was dialled with.
    ///
    /// The pairing is kept only once the computer has admitted this phone: a
    /// code that was refused is not a computer this phone has.
    fn dial(&self, pairing: &Pairing) -> Result<(), Trouble> {
        self.stands(Reach::Reaching);
        match self.dialled(pairing) {
            Ok(()) => {
                self.stands(Reach::Talking);
                Ok(())
            }
            Err(trouble) => {
                // The refusal is kept rather than summarised, and the keeper
                // goes on dialling behind it. What a person is shown is the
                // last thing that actually happened — the door's sentence, the
                // address that names nothing reachable, the two builds that
                // cannot speak — rather than a phone reporting its own silence.
                self.stands(Reach::Away(trouble.0.clone()));
                Err(trouble)
            }
        }
    }

    /// The dial itself, with nothing said about how it went.
    fn dialled(&self, pairing: &Pairing) -> Result<(), Trouble> {
        let live = greeted(pairing, Arc::clone(&self.watching), Arc::clone(&self.waker))?;
        *self.talking.lock().map_err(|_| poisoned())? = Some(live);
        *self.holding.lock().map_err(|_| poisoned())? = Some(pairing.clone());
        Ok(())
    }

    /// Ask the computer one thing and wait for its answer.
    ///
    /// A connection that has died is dialled again once, with the pairing it
    /// was opened under. Nothing is queued while it is down: a phone that has
    /// been in a pocket for an hour wants what is true now rather than what it
    /// meant to ask then.
    ///
    /// **The call itself is only made again where making it twice is the same
    /// as making it once.** A connection that dies has not said whether the
    /// question got there, and *ask again* for a write is how one record
    /// becomes two on a bad train — so a read is replayed and a write comes
    /// back saying the connection was remade. The person's next press goes out
    /// over a connection that works, which is the difference between asking
    /// somebody to try again and doing something they did not ask for.
    ///
    /// An operation this build has never heard of is a write for the same
    /// reason a silence is: the safe answer to *what does this do* is the one
    /// that costs a second press.
    ///
    /// # Errors
    ///
    /// No pairing, a computer that will not answer, or the operation's own
    /// error, said in the computer's words.
    pub fn ask(&self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        if !self.open_now() {
            self.reach()?;
        }
        match self.asked(method, params) {
            Ok(answer) => Ok(answer),
            Err(Unanswered::Gone(_)) => self.again(method, params),
            Err(refused) => Err(refused.failure()),
        }
    }

    /// The connection went away under a call. Make another one, and decide
    /// whether the call itself may go out on it.
    fn again(&self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        self.drop_connection();
        self.reach()?;
        if effect(method) == Some(Effect::Reads) {
            return self.asked(method, params).map_err(Unanswered::failure);
        }
        Err(away(
            "the connection to the computer dropped while this was in flight, and has been made \
             again — ask for it once more",
        ))
    }

    /// The operations of one project, asked over this connection.
    ///
    /// The key travels in every call rather than being said once, because the
    /// network door keeps no memory of which project a connection is about —
    /// that is what makes one connection serve every project and what makes a
    /// phone unable to name a directory on somebody else's computer.
    pub fn about(&self, project: &str) -> Asking<'_> {
        Asking::about(self, project)
    }

    /// What this phone is paired with, if anything.
    pub fn pairing(&self) -> Option<Pairing> {
        self.holding.lock().ok()?.clone()
    }

    /// Whether there is a conversation to ask on.
    ///
    /// **Alive rather than present, and the difference is what used to cost
    /// somebody a restart.** The sender outlives the thread that serves it, so
    /// a phone whose computer had gone went on holding one — and this answered
    /// yes, which took every call past the re-dial written for exactly this
    /// case and into a channel nothing was reading. Nothing recovered from that
    /// but launching the application again.
    pub fn open_now(&self) -> bool {
        self.talking.lock().is_ok_and(|held| {
            held.as_ref()
                .is_some_and(|live| live.alive.load(Ordering::Acquire))
        })
    }

    /// Let go of a conversation that has ended, keeping the computer it was
    /// with. What [`Self::reach`] then opens is a new one to the same machine.
    fn drop_connection(&self) {
        if let Ok(mut talking) = self.talking.lock() {
            *talking = None;
        }
    }

    /// Forget the computer entirely: the conversation and which one it was.
    pub fn close(&self) {
        if let Ok(mut talking) = self.talking.lock() {
            *talking = None;
        }
        if let Ok(mut holding) = self.holding.lock() {
            *holding = None;
        }
        drop(self.watching.taken());
        self.stands(Reach::Unpaired);
        // So the keeper finds out now rather than at the end of a rest it began
        // while this phone still had a computer.
        self.waker.ring();
    }

    fn asked(&self, method: &str, params: &Value) -> Result<Value, Unanswered> {
        let (answering, answered) = channel();
        {
            let talking = self
                .talking
                .lock()
                .map_err(|_| Unanswered::Gone(away(&poisoned().0)))?;
            let live = talking.as_ref().ok_or_else(|| Unanswered::Gone(gone()))?;
            live.asking
                .send((method.to_owned(), params.clone(), answering))
                .map_err(|_| Unanswered::Gone(gone()))?;
        }
        // The deadline is the call's rather than one line's, which is what it
        // always meant and could not be while a call owned the wire. What it is
        // waiting for now is one entry in a map being filled in, and nothing
        // else can fill it.
        match answered.recv_timeout(ANSWERING) {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(failure)) if carried(&failure) => Err(Unanswered::Gone(failure)),
            Ok(Err(failure)) => Err(Unanswered::Refused(failure)),
            // Counted as the connection rather than as the computer, though
            // the socket may still believe in itself. This deadline is already
            // longer than the computer's own ceiling for a call — so nothing
            // answering inside it means the two ends no longer agree about
            // what is outstanding, and the cheapest way back to agreement is a
            // connection neither of them has any history on. What protects the
            // person from that being a decision taken on their behalf is the
            // rule in `again`: a read is replayed, a write is handed back.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(Unanswered::Gone(away("the computer did not answer")))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(Unanswered::Gone(gone())),
        }
    }

    /// Watch a conversation, and hand the window's sink whatever it says.
    ///
    /// Answers with how many events had already fallen off the front of the
    /// conversation's history, which is what the window is told on this machine
    /// too — a transcript that quietly begins in the middle reads as the whole
    /// of it.
    ///
    /// # Errors
    ///
    /// The computer's own refusal — most often a conversation that ended
    /// between being listed and being watched.
    pub fn watch(
        &self,
        key: &str,
        events: tauri::ipc::Channel<Value>,
    ) -> sync_memory::Result<Subscription> {
        // Whatever was watching this conversation before is not watching it
        // now. **The case this is for is the ordinary one on a phone**: the
        // system reloads the webview by itself, the window comes back and asks
        // to watch the conversation it had open, and the sink it asked with
        // last time belongs to a load of the page that no longer exists. A
        // watch left standing for that one would go on being written to from
        // the computer until something noticed the sink was dead — and this
        // phone would then be holding two numbers for one conversation, with
        // no way to say which of them the window means.
        //
        // Best effort, because failing here would refuse to show somebody a
        // conversation over a tidy-up: at worst the computer holds a watch
        // nobody drains until that connection ends, which is what it already
        // survives.
        drop(self.stop_watching(key));
        let watched = Arc::new(Watched {
            key: key.to_owned(),
            events,
            seen: Mutex::new(None),
        });
        self.watched(&watched, None)
    }

    /// Stop watching, by the conversation the window names.
    ///
    /// A window that was not watching is not a failure and not a call: nothing
    /// on the computer is holding anything for it.
    ///
    /// # Errors
    ///
    /// The computer's own refusal.
    pub fn stop_watching(&self, key: &str) -> sync_memory::Result<()> {
        let Some(subscription) = self.watching.number_for(key) else {
            return Ok(());
        };
        self.watching.forget(subscription);
        self.ask(
            sync_memory::SESSION_UNSUBSCRIBE,
            &json!({"key": key, "subscription": subscription}),
        )?;
        Ok(())
    }

    /// Ask for one watch and hold it under the number that comes back.
    ///
    /// The number is the computer's and never this phone's: it is minted on the
    /// connection the call went out on, because that is the only place that
    /// knows where the events have to be written. So the answer is where a
    /// watch becomes something this phone can find.
    fn watched(
        &self,
        watched: &Arc<Watched>,
        since: Option<u64>,
    ) -> sync_memory::Result<Subscription> {
        // Said before the call goes out, because what the call is *for* arrives
        // before it answers: the computer replays the conversation on
        // the way to saying under what number it will go on doing so. Without
        // this line the replay is a conversation's whole history arriving under
        // a number nothing holds yet, and the window is handed an empty screen
        // for a conversation full of words.
        let expected = self.watching.expecting();
        let answer = self.ask(
            sync_memory::SESSION_SUBSCRIBE,
            &json!({"key": watched.key, "since": since}),
        )?;
        let subscription = answer
            .get("subscription")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                MemoryError::Protocol(
                    "the computer agreed to show this conversation and did not say under what \
                     number"
                        .to_owned(),
                )
            })?;
        self.watching.hold(subscription, Arc::clone(watched));
        drop(expected);
        // Read as the shape rather than member by member. What the window does
        // with this decides what it draws over the first line of a transcript —
        // that the conversation is cut short, or that there is more to fetch —
        // and a phone spelling the members itself is a phone that can come to
        // disagree with the computer about which of those it is being told.
        serde_json::from_value(answer).map_err(|refusal| {
            MemoryError::Protocol(format!(
                "the computer agreed to show this conversation and said where it began in a way \
                 this phone cannot read: {refusal}"
            ))
        })
    }

    /// Ask again for everything this phone was watching, from where it stopped.
    ///
    /// What a reconnection is made of, and the whole reason a dropped
    /// connection costs a person nothing: the conversation went on running on
    /// the computer while the phone was away, so what is wanted back is the
    /// part that was said meanwhile and not the part already on the screen.
    ///
    /// A watch that cannot be taken again is let go of rather than retried. The
    /// ordinary reason is a conversation that ended while this phone was in
    /// somebody's pocket, and a screen showing what it last heard is a better
    /// answer than one that keeps asking.
    ///
    /// Asked with [`Self::asked`] rather than [`Self::ask`], which is not a
    /// detail: the second re-dials on a connection that failed, and this runs
    /// with the dial that just succeeded still in hand.
    fn watch_again(&self) {
        // Kept for the whole round, for the reason [`Self::watched`] keeps
        // anything at all: each of these answers with a number, and each
        // replays what was said while this phone was away before it does.
        let _expected = self.watching.expecting();
        for watched in self.watching.taken() {
            let since = watched.seen.lock().ok().and_then(|seen| *seen);
            let asked = self.asked(
                sync_memory::SESSION_SUBSCRIBE,
                &json!({"key": watched.key, "since": since}),
            );
            if let Ok(answer) = asked
                && let Some(subscription) = answer.get("subscription").and_then(Value::as_u64)
            {
                self.watching.hold(subscription, watched);
            }
        }
    }
}

/// Why a call did not come back, told apart by the only thing worth deciding
/// about it: whether asking again over a fresh connection would change the
/// answer.
///
/// It was decided by reading the failure's kind, and that reading was wrong in
/// the one case it mattered. A connection that had gone away was reported with
/// [`gone`] — a *domain* failure, because the sentence is what a person reads —
/// and the test in front of the re-dial looked for the two transport kinds
/// only. So the branch never ran. Saying it in the type instead means the
/// decision is made where the failure is produced, by the code that knows which
/// it is, rather than guessed at afterwards from a shape that was chosen for
/// somebody's screen.
enum Unanswered {
    /// The connection is not there any more. A fresh one might carry this.
    Gone(MemoryError),
    /// The computer was reached and said no, or said something unreadable.
    /// Reaching it again would ask the same machine the same question.
    Refused(MemoryError),
}

impl Unanswered {
    /// The failure as the window reads it, whichever of the two it was.
    fn failure(self) -> MemoryError {
        match self {
            Self::Gone(failure) | Self::Refused(failure) => failure,
        }
    }
}

/// Keep a connection to the computer up, for as long as this phone is running.
///
/// **The half of reconnecting that nothing asks for.** [`Channel::ask`] dials
/// again when somebody makes a call, which is enough for a screen a person is
/// tapping at and is nothing at all for the screen this phone spends its time
/// on: a conversation with an agent, where the words arrive unasked-for and the
/// only call was made a minute ago. A phone that dialled only on demand sat
/// there with an open transcript and no connection under it, and looked exactly
/// like an agent that had gone quiet.
///
/// It runs against the ordinary condition of an iOS application rather than
/// against a fault. The system suspends this process seconds after it goes into
/// the background and the connection goes with it — `docs/design-foundation.md`
/// §"Speaking from outside the window" says so where it explains why a phone
/// raises no banners — so every time somebody puts their phone down, this is
/// what puts it back.
///
/// The waits double from [`FIRST_REST`] to [`LONGEST_REST`] and are cut short
/// by [`Waker`]: a connection ending rings it, and so does somebody pressing
/// *Try again*. What is left is the cadence for a phone nobody is holding.
pub fn kept(channel: &Channel) {
    let waker = Arc::clone(&channel.waker);
    let mut rest = FIRST_REST;
    loop {
        // Neither of these is worth a dial, and both are worth the longest
        // wait: the keeper is woken by the bell for anything that changes
        // them, and a phone with a working connection should be asleep.
        let idle = if channel.pairing().is_none() {
            channel.stands(Reach::Unpaired);
            true
        } else {
            channel.open_now()
        };
        if idle {
            rest = FIRST_REST;
            waker.waited(LONGEST_REST);
            continue;
        }

        // The outcome is not read, and it is not an omission. `reach` has
        // already said where this leaves the phone — talking, or away in the
        // words of whatever refused — and the only thing left to decide is how
        // long to wait, which is the same answer either way. A computer that
        // admits this phone and drops it a moment later is the case that makes
        // it the same answer: rested only on failure, it would be dialled as
        // fast as the two of them could manage.
        drop(channel.reach());
        waker.waited(rest);
        rest = rest.saturating_mul(2).min(LONGEST_REST);
    }
}

/// Something that carries one call to the computer and brings the answer back.
///
/// A trait over a single method, and its whole purpose is that the thing on the
/// other side of it does not have to be a network. What this file decides about
/// a call — which operation it is, whose project it is about, what the
/// parameters are called — is decided the same way whether a QUIC stream or a
/// list in a test is underneath, and only the first of those can be dialled.
pub trait Asks {
    /// Ask, and answer with what came back.
    ///
    /// # Errors
    ///
    /// The connection, or the computer's own refusal.
    fn ask(&self, method: &str, params: &Value) -> sync_memory::Result<Value>;
}

impl Asks for Channel {
    fn ask(&self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        Self::ask(self, method, params)
    }
}

/// One project, as [`Operations`] asks about it.
pub struct Asking<'a> {
    asks: &'a dyn Asks,
    project: String,
}

impl<'a> Asking<'a> {
    /// Ask about `project` over whatever carries a call.
    pub fn about(asks: &'a dyn Asks, project: &str) -> Self {
        Self {
            asks,
            project: project.to_owned(),
        }
    }
}

impl Operations for Asking<'_> {
    /// Put the project's key in the call and hand it to the connection.
    ///
    /// The key goes in rather than beside, because that is where the door reads
    /// it. What the door does with it afterwards is the door's business — it
    /// spends the key and takes it back out before the operation sees it, so
    /// nothing downstream is handed an argument it never declared.
    ///
    /// A call whose parameters are not an object is refused rather than
    /// wrapped. Every operation of this channel takes named members, so this is
    /// a client that has gone wrong, and quietly building an object around it
    /// would send the computer a call nothing can read.
    fn request(&mut self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        let Some(members) = params.as_object() else {
            return Err(MemoryError::Protocol(format!(
                "`{method}` was asked with parameters that are not an object"
            )));
        };
        let mut named = members.clone();
        named.insert("project".to_owned(), json!(self.project));
        self.asks.ask(method, &Value::Object(named))
    }
}

/// Whether this failure is the connection rather than the computer's answer.
///
/// Only these two are worth dialling again for. A refusal and an unreadable
/// answer both mean the computer was reached, and reaching it a second time
/// would ask the same question of the same machine.
fn carried(failure: &MemoryError) -> bool {
    matches!(failure, MemoryError::Sidecar(_) | MemoryError::Io(_))
}

/// What the phone says when the computer is not there.
///
/// Its own kind rather than the one the desktop's client uses for the same
/// shape of failure: the window reads that one as *the memory engine is not
/// running*, which is a sentence about a process on this machine and there is
/// no such process here. The word is unknown to this build's vocabulary on
/// purpose — what reaches a person is the message, and inventing a kind for
/// something nothing branches on would be inventing a branch.
fn away(said: &str) -> MemoryError {
    MemoryError::domain("unreachable", said, Value::Null)
}

fn gone() -> MemoryError {
    away("the connection to the computer has closed")
}

fn poisoned() -> Trouble {
    Trouble("the connection is in an unknown state; pair again".to_owned())
}

/// Dial, greet, agree on the channel's version, and leave a thread holding it.
///
/// The four happen together because a connection that fails any of them is not
/// a connection anybody should be handed: an unadmitted device and a computer
/// speaking a channel this build cannot read are both *not paired with this*,
/// and finding either one out later would mean finding it out in the middle of
/// somebody's work.
fn greeted(pairing: &Pairing, watching: Arc<Watching>, waker: Arc<Waker>) -> Result<Live, Trouble> {
    // On a thread of its own, and it is not a preference. The runtime below is
    // built here and blocked on, and tokio refuses to build one on a thread
    // that is already driving one — *cannot start a runtime from within a
    // runtime*. Every command of this application is `async`, which is to say
    // every one of them runs on a worker of Tauri's runtime, so dialling
    // straight from a command panicked; and a panic inside a command is worse
    // than an error, because the window is left holding a promise that never
    // settles and a button that says *Pairing…* for ever.
    //
    // A plain thread has no runtime of its own to collide with. The caller
    // waits for it, which is what an `async` command is allowed to do and what
    // the main thread was never allowed to do.
    let dialling = pairing.clone();
    std::thread::spawn(move || dialled(&dialling, watching, waker))
        .join()
        .map_err(|_| Trouble("the connection could not be started".to_owned()))?
}

/// The way a call is put on the connection, once there is one.
type Talking = tokio::sync::mpsc::UnboundedSender<Asked>;

/// The dial itself, on a thread that owns everything it makes.
fn dialled(pairing: &Pairing, watching: Arc<Watching>, waker: Arc<Waker>) -> Result<Live, Trouble> {
    let runtime = Runtime::new().map_err(Trouble::saying)?;
    let named: EndpointId = pairing
        .endpoint
        .parse()
        .map_err(|_| Trouble("that is not a Sync computer's address".to_owned()))?;

    let wire = runtime.block_on(async {
        let endpoint = Endpoint::builder(presets::N0)
            .bind()
            .await
            .map_err(Trouble::saying)?;
        // Bounded, because a dial has no deadline of its own: an address that
        // names nothing reachable leaves discovery looking for it until the
        // application is killed, and what a person sees is a button that says
        // it is working. A wait that ends in a sentence is the whole
        // difference between a refusal and a hang.
        let connection = tokio::time::timeout(PATIENCE, endpoint.connect(named, ALPN))
            .await
            .map_err(|_| Trouble("the computer could not be reached at that address".to_owned()))?
            .map_err(Trouble::saying)?;
        let (writing, reading) = connection.open_bi().await.map_err(Trouble::saying)?;
        Ok::<_, Trouble>((writing, BufReader::new(reading), connection, endpoint))
    })?;

    let mut wire = Wire {
        runtime,
        writing: wire.0,
        reading: wire.1,
        _connection: wire.2,
        _endpoint: wire.3,
    };

    // **The two lines before anything else, said one at a time.** They are the
    // only exchange on this connection where a question owning the wire is
    // right: nothing else can arrive until this device has been admitted, and
    // a reader started before the greeting would be reading on behalf of a
    // connection that may be about to be refused.
    //
    // The greeting is written straight onto the transport rather than through
    // any client, and for one reason: the door's refusal is a sentence somebody
    // reads, and a client that wrapped it — *`remote.hello` failed: …* — would
    // be showing them our formatting of a message whose whole point is that it
    // is the door's. Its id is 0, which the numbering below starts past.
    wire.send(&json!({
        "jsonrpc": "2.0", "id": 0, "method": REMOTE_HELLO,
        "params": {"secret": pairing.secret},
    }))
    .map_err(Trouble::saying)?;
    let said = wire
        .receive()
        .map_err(Trouble::saying)?
        .ok_or_else(|| Trouble("the computer closed the connection".to_owned()))?;
    if let Some(refusal) = said
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return Err(Trouble(refusal.to_owned()));
    }
    if said["result"]["admitted"] != Value::Bool(true) {
        return Err(Trouble("the computer did not admit this phone".to_owned()));
    }

    agreed(&mut wire)?;
    Ok(serving(wire, watching, waker))
}

/// Read the computer's channel number and refuse to speak past it.
///
/// The same check the window makes against its own sidecar, for the reason the
/// number exists at all: a phone installed from a store is months behind by
/// construction, and *old client, new computer* is its ordinary condition
/// rather than its edge.
fn agreed(wire: &mut Wire) -> Result<(), Trouble> {
    wire.send(&json!({
        "jsonrpc": "2.0", "id": GREETING, "method": METHODS,
        "params": {"channel": CHANNEL_VERSION},
    }))
    .map_err(Trouble::saying)?;
    let said = wire
        .receive()
        .map_err(Trouble::saying)?
        .ok_or_else(|| Trouble("the computer closed the connection".to_owned()))?;
    let listed = said.get("result").cloned().unwrap_or(Value::Null);
    let theirs = listed.get("channel").and_then(Value::as_u64);

    // Said once per connection, beside the greeting the application writes at
    // launch, and for the same reason: the first question about a client that
    // will not talk to a computer is what the two of them agreed on. A count of
    // operations is the shortest answer that distinguishes *reached the door*
    // from *reached something that answered*.
    //
    // Written whole rather than with `eprintln!` — iOS files each piece of
    // stderr as its own line, so a formatted message arrives in fragments.
    let count = listed
        .get("methods")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let noted = format!("Connected: channel {theirs:?}, {count} operations\n");
    drop(std::io::Write::write_all(
        &mut std::io::stderr(),
        noted.as_bytes(),
    ));

    match theirs {
        Some(theirs) if theirs == u64::from(CHANNEL_VERSION) => Ok(()),
        Some(theirs) if theirs > u64::from(CHANNEL_VERSION) => Err(Trouble(
            "this phone is older than the computer it dialled; update Sync on the phone".to_owned(),
        )),
        _ => Err(Trouble(
            "the computer is older than this phone; update Sync on the computer".to_owned(),
        )),
    }
}

/// The number the handshake is asked under, and the last one anybody asks
/// under before the numbering below takes over.
const GREETING: u64 = 1;

/// Hand the connection to a thread and hand back the way to put a call on it.
///
/// **Two tasks and not one loop**, which is the whole shape of this file since
/// a conversation could arrive unasked-for. One writes what this phone asks;
/// one reads what the computer says and decides, per line, whether it is
/// somebody's answer or somebody's conversation. A single loop selecting over
/// both would have to abandon a half-read line whenever the other side fired,
/// and a line abandoned halfway is every answer after it against the wrong
/// question.
///
/// The thread ends when the connection does. It owns the runtime, so ending
/// takes both tasks with it — and with them the receiver the writer was
/// waiting on, which is what makes the next call say the connection has closed
/// rather than wait for an answer nothing will write.
fn serving(wire: Wire, watching: Arc<Watching>, waker: Arc<Waker>) -> Live {
    let Wire {
        runtime,
        mut writing,
        reading,
        _connection,
        _endpoint,
    } = wire;
    let (asking, mut asked) = tokio::sync::mpsc::unbounded_channel::<Asked>();
    let alive = Arc::new(AtomicBool::new(true));
    let ending = Arc::clone(&alive);
    std::thread::spawn(move || {
        // Held for as long as the conversation is, and dropped with it. A
        // connection does not keep its endpoint alive: dropped, it takes the
        // socket with it and nothing drives the connection any more.
        let _connection = _connection;
        let _endpoint = _endpoint;
        let waiting: Arc<
            Mutex<std::collections::HashMap<u64, Sender<sync_memory::Result<Value>>>>,
        > = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let writers = Arc::clone(&waiting);
        runtime.block_on(async move {
            let writer = tokio::spawn(async move {
                let mut next = GREETING + 1;
                while let Some((method, params, answering)) = asked.recv().await {
                    let id = next;
                    next += 1;
                    // Left where the answer will be looked for before the line
                    // goes out, because the answer can be read by the other
                    // task the instant it is written.
                    if let Ok(mut waiting) = writers.lock() {
                        waiting.insert(id, answering);
                    }
                    let line = json!({
                        "jsonrpc": "2.0", "id": id, "method": method, "params": params,
                    });
                    if writing
                        .write_all(format!("{line}\n").as_bytes())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
            reading_answers(reading, &waiting, &watching).await;
            writer.abort();
            // Everything still waiting is failed rather than left to time out.
            // The connection has ended and this side knows it; a caller left to
            // discover it by waiting would wait the whole of its patience for a
            // fact that is already true.
            if let Ok(mut waiting) = waiting.lock() {
                for (_, answering) in waiting.drain() {
                    drop(answering.send(Err(gone())));
                }
            }
        });
        // Here rather than anywhere above, because here is where it becomes
        // true: both tasks are over and the receiver the writer drained has
        // gone with them, so from this line on a call put on the sender is a
        // call nothing will ever read. Everything that decides whether to dial
        // reads this, so saying it late is a phone that believes in a
        // connection for a moment longer than there was one.
        ending.store(false, Ordering::Release);
        // And the keeper hears about it now, rather than at the end of a wait
        // it started while this connection was still up. Without the bell a
        // conversation left open on the screen would sit under a dead
        // connection for as long as the current rest had left to run.
        waker.ring();
    });
    Live { asking, alive }
}

/// Read the connection until it ends, and put each line where it belongs.
///
/// Three kinds of line and no fourth. An answer goes to whoever asked. A
/// conversation's event goes to whoever is watching it. Anything else is
/// dropped without ending the connection — a computer newer than this phone
/// says things this build has no name for, and a phone that hung up over one
/// would be a phone that stops working the week the computer updates.
async fn reading_answers(
    mut reading: BufReader<RecvStream>,
    waiting: &Mutex<std::collections::HashMap<u64, Sender<sync_memory::Result<Value>>>>,
    watching: &Watching,
) {
    loop {
        let Ok(Some(said)) = a_line(&mut reading).await else {
            return;
        };
        if let Some(id) = said.get("id").and_then(Value::as_u64) {
            let answering = waiting.lock().ok().and_then(|mut held| held.remove(&id));
            if let Some(answering) = answering {
                // A caller that stopped waiting is not a reason to stop
                // reading: the answer is off the wire either way, and the next
                // line needs the stream where this one left it.
                drop(answering.send(answered(&said)));
            }
            continue;
        }
        if said.get("method").and_then(Value::as_str) == Some(sync_memory::SESSION_EVENT) {
            let params = said.get("params").unwrap_or(&Value::Null);
            if let Some(subscription) = params.get("subscription").and_then(Value::as_u64) {
                watching.shown(subscription, params.get("event").unwrap_or(&Value::Null));
            }
        }
    }
}

/// One line off the connection, or nothing where it has ended.
///
/// Read by hand rather than with `lines()`, and the ceiling is the reason: that
/// helper grows its buffer to whatever arrives, so a computer sending a line
/// with no newline in it would be as much of this phone's memory as it cared to
/// take. The channel has a frame size and the other end enforces it on what it
/// reads; a limit only one end honours is a limit only one end has.
///
/// An oversized line ends the connection rather than being skipped. What has
/// been read is part of a message this build cannot parse, and reading on from
/// the middle of one would put every answer after it against the wrong
/// question.
///
/// **No deadline here, and that is the change worth naming.** A read used to be
/// given the patience a *call* deserves, because a call was the only reason to
/// be reading; now the connection is read whether or not anything was asked,
/// and a conversation that has been quiet for a minute is an agent thinking
/// rather than a computer that has gone. What ends a wait is the caller's own
/// deadline, and what ends the connection is the door's idle timer.
async fn a_line(reading: &mut BufReader<RecvStream>) -> io::Result<Option<Value>> {
    let mut line = Vec::new();
    let read = {
        let mut capped = reading.take(MAX_FRAME_BYTES as u64 + 1);
        capped.read_until(b'\n', &mut line).await?
    };
    if read == 0 {
        return Ok(None);
    }
    if line.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the computer sent a message larger than this channel carries",
        ));
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|broken| io::Error::new(io::ErrorKind::InvalidData, broken))
}

/// One answer, as what the caller asked for or as the computer's own refusal.
///
/// The kind travels rather than the sentence: the window branches on
/// `conflict`, on `locked` and on every word a conversation refuses with, and a
/// refusal flattened here would arrive as a screen that can only apologise.
fn answered(said: &Value) -> sync_memory::Result<Value> {
    if let Some(error) = said.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the computer refused and did not say why");
        let data = error.get("data").cloned().unwrap_or(Value::Null);
        let kind = data
            .get("kind")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        return Err(match kind {
            Some(kind) => MemoryError::domain(&kind, message, data),
            None => MemoryError::Protocol(message.to_owned()),
        });
    }
    said.get("result")
        .cloned()
        .ok_or_else(|| MemoryError::Protocol("the computer answered with neither".to_owned()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::{Channel, Live, Pairing, Reach, Unanswered, Waker, Watched, Watching, answered};

    /// A window's sink, and everything that has been put into it.
    fn sink() -> (tauri::ipc::Channel<Value>, Arc<Mutex<Vec<Value>>>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let writing = Arc::clone(&heard);
        let channel = tauri::ipc::Channel::new(move |body| {
            let said: Value = match body {
                tauri::ipc::InvokeResponseBody::Json(text) => {
                    serde_json::from_str(&text).expect("the window is sent JSON")
                }
                tauri::ipc::InvokeResponseBody::Raw(_) => Value::Null,
            };
            writing.lock().expect("nothing panicked").push(said);
            Ok(())
        });
        (channel, heard)
    }

    fn watching(key: &str) -> (Watching, Arc<Watched>, Arc<Mutex<Vec<Value>>>) {
        let (events, heard) = sink();
        let watched = Arc::new(Watched {
            key: key.to_owned(),
            events,
            seen: Mutex::new(None),
        });
        (Watching::default(), watched, heard)
    }

    /// A word of a conversation reaches the window that asked to watch it.
    ///
    /// The whole of what the reverse direction is for, and the thing that could
    /// not happen at all while a call owned the wire: nothing was asked, and the
    /// event still arrives.
    #[test]
    fn an_event_reaches_the_window_watching_that_conversation() {
        let (register, watched, heard) = watching("s0");
        register.hold(7, watched);

        register.shown(7, &json!({"kind": "update", "seq": 0, "text": "hel"}));
        register.shown(7, &json!({"kind": "update", "seq": 1, "text": "lo"}));

        let heard = heard.lock().expect("nothing panicked");
        assert_eq!(heard.len(), 2, "the agent's words arrived as it wrote them");
        assert_eq!(heard[0]["text"], "hel");
        assert_eq!(heard[1]["seq"], 1);
    }

    /// A conversation that was already going arrives before the watch does,
    /// and still reaches the window.
    ///
    /// **The whole of what a conversation opened on the computer looks like
    /// from here.** Asking to watch one is answered with the number the watch
    /// will be known by, and the computer replays everything already said
    /// before it writes that answer — one socket, replay first, because the
    /// replay is what the call does. A phone that kept only what arrived after
    /// the answer opened every such conversation empty.
    #[test]
    fn what_was_said_before_the_watch_had_a_number_still_arrives() {
        let (register, watched, heard) = watching("s0");
        let expected = register.expecting();

        register.shown(7, &json!({"kind": "update", "seq": 0, "text": "hel"}));
        register.shown(7, &json!({"kind": "update", "seq": 1, "text": "lo"}));
        register.hold(7, Arc::clone(&watched));
        drop(expected);

        let read = heard.lock().expect("nothing panicked");
        assert_eq!(
            read.len(),
            2,
            "the conversation was replayed into the window"
        );
        assert_eq!(read[0]["text"], "hel", "in the order the computer said it");
        assert_eq!(read[1]["seq"], 1);
        assert_eq!(
            *watched.seen.lock().expect("nothing panicked"),
            Some(1),
            "and this phone knows how far into it the window has got"
        );
    }

    /// What was kept ahead of a watch is shown before what came after it.
    ///
    /// The order is the only thing a transcript is: the window folds events
    /// into it as they arrive and never sorts them, so a word of the replay
    /// delivered after a word said since would be a conversation with its own
    /// history quoted underneath its answer.
    #[test]
    fn the_replay_is_shown_before_what_was_said_since() {
        let (register, watched, heard) = watching("s0");
        let expected = register.expecting();

        register.shown(7, &json!({"seq": 0}));
        register.hold(7, watched);
        register.shown(7, &json!({"seq": 1}));
        drop(expected);

        let read = heard.lock().expect("nothing panicked");
        assert_eq!(read[0]["seq"], 0);
        assert_eq!(read[1]["seq"], 1);
    }

    /// Nothing is kept when nothing is being asked for.
    ///
    /// What stops the keeping from becoming a place words go to be forgotten
    /// in. An event under a number nobody is waiting for is a watch let go of a
    /// moment ago, and holding it would mean handing somebody else's
    /// conversation to whichever watch the computer next minted that number
    /// for.
    #[test]
    fn nothing_is_kept_for_a_watch_nobody_is_asking_for() {
        let (register, watched, heard) = watching("s0");

        register.shown(7, &json!({"seq": 0}));
        register.hold(7, watched);

        assert!(heard.lock().expect("nothing panicked").is_empty());
    }

    /// The keeping ends with the asking.
    ///
    /// A watch that was refused — a conversation that ended between being
    /// listed and being asked for — leaves words behind under a number it never
    /// took. They go when the last ask is over, rather than waiting for a
    /// number that is not coming.
    #[test]
    fn what_no_watch_claimed_is_let_go_of_when_the_asking_ends() {
        let (register, watched, heard) = watching("s0");
        let expected = register.expecting();
        register.shown(7, &json!({"seq": 0}));
        drop(expected);

        register.hold(7, watched);

        assert!(heard.lock().expect("nothing panicked").is_empty());
    }

    /// An event under a number this phone does not hold reaches nobody, and is
    /// not a failure.
    ///
    /// It is what a watch let go of a moment ago looks like from the far end,
    /// and the far end has no way of knowing sooner.
    #[test]
    fn an_event_for_a_watch_that_has_gone_is_dropped_in_silence() {
        let (register, watched, heard) = watching("s0");
        register.hold(7, watched);
        register.forget(7);

        register.shown(7, &json!({"seq": 0}));

        assert!(heard.lock().expect("nothing panicked").is_empty());
    }

    /// The phone remembers how far into a conversation it has got.
    ///
    /// This is what a reconnection is asked from. Without it the computer would
    /// replay the whole conversation into a transcript that already holds it,
    /// and the window's own fold appends rather than replaces — so what a person
    /// would see is every word they had already read, a second time, below
    /// itself.
    #[test]
    fn what_the_window_has_been_shown_is_where_the_next_watch_begins() {
        let (register, watched, _heard) = watching("s0");
        register.hold(7, Arc::clone(&watched));

        for seq in [0, 1, 2] {
            register.shown(7, &json!({"seq": seq}));
        }

        assert_eq!(*watched.seen.lock().expect("nothing panicked"), Some(2));
    }

    /// An event out of order does not move the mark backwards.
    ///
    /// The door answers calls concurrently and a re-subscription overlaps the
    /// watch it replaces, so two events can cross. Taking the later of the two
    /// is the only reading that never asks for something twice.
    #[test]
    fn a_later_event_never_moves_the_mark_back() {
        let (register, watched, _heard) = watching("s0");
        register.hold(7, Arc::clone(&watched));

        register.shown(7, &json!({"seq": 5}));
        register.shown(7, &json!({"seq": 2}));

        assert_eq!(*watched.seen.lock().expect("nothing panicked"), Some(5));
    }

    /// Watching a conversation a second time replaces the first watch.
    ///
    /// The case is the ordinary one on a phone rather than an edge: iOS reloads
    /// the webview by itself, and the window comes back asking to watch the
    /// conversation it had open. The sink it asked with last time belongs to a
    /// page that no longer exists, and a phone holding two numbers for one
    /// conversation cannot say which of them the window means when it asks to
    /// stop.
    #[test]
    fn a_second_watch_of_one_conversation_replaces_the_first() {
        let (register, first, was) = watching("s0");
        register.hold(1, first);
        let (events, is) = sink();
        register.hold(
            2,
            Arc::new(Watched {
                key: "s0".to_owned(),
                events,
                seen: Mutex::new(None),
            }),
        );
        // What `Channel::watch` does before it takes the new one.
        if let Some(stale) = register.number_for("s0").filter(|held| *held == 1) {
            register.forget(stale);
        }

        register.shown(2, &json!({"seq": 0}));

        assert!(
            was.lock().expect("nothing panicked").is_empty(),
            "the page that was reloaded away is still being written to"
        );
        assert_eq!(is.lock().expect("nothing panicked").len(), 1);
    }

    /// A watch is found by the conversation the window names.
    ///
    /// The window says *stop watching this conversation*, because a conversation
    /// is what a screen knows; the computer is told a number, because a
    /// conversation may have several watches on it.
    #[test]
    fn a_watch_is_found_by_the_conversation_the_window_names() {
        let (register, watched, _heard) = watching("s3");
        register.hold(11, watched);

        assert_eq!(register.number_for("s3"), Some(11));
        assert_eq!(register.number_for("s0"), None);
        register.forget(11);
        assert_eq!(register.number_for("s3"), None);
    }

    /// A reconnection takes every watch off the register at once.
    ///
    /// Together rather than one at a time: the numbers do not survive a new
    /// connection, so leaving one behind would leave this phone holding a number
    /// the computer has never heard of and events arriving under nothing.
    #[test]
    fn a_reconnection_starts_by_letting_go_of_every_number() {
        let (register, first, _one) = watching("s0");
        let (second, _two) = {
            let (events, heard) = sink();
            (
                Arc::new(Watched {
                    key: "s1".to_owned(),
                    events,
                    seen: Mutex::new(None),
                }),
                heard,
            )
        };
        register.hold(1, first);
        register.hold(2, second);

        let taken = register.taken();

        assert_eq!(taken.len(), 2);
        assert_eq!(register.number_for("s0"), None);
        assert_eq!(register.number_for("s1"), None);
    }

    /// A refusal keeps the word the computer gave it.
    ///
    /// The window branches on that word and does something different for each:
    /// a conversation the agent no longer holds is offered a kept transcript, a
    /// working tree that was deleted is not offered anything. Flattened into a
    /// sentence they all become a screen that can only apologise.
    #[test]
    fn a_refusal_arrives_with_the_kind_the_computer_gave_it() {
        let refused = answered(&json!({
            "jsonrpc": "2.0", "id": 4,
            "error": {"message": "the agent no longer holds that session",
                      "data": {"kind": "agent_session_load"}},
        }))
        .expect_err("the computer refused");

        let refused = sync_memory::CommandError::from(refused);
        assert_eq!(refused.kind, "agent_session_load");
        assert!(refused.message.contains("no longer holds"));
    }

    /// A connection whose thread has ended is not one to ask on.
    ///
    /// **The defect this file carried, in one assertion.** The sender outlives
    /// the thread that serves it — nothing on the way down set it aside — so
    /// *is there a connection* answered yes for ever, every call went past the
    /// re-dial written for exactly this case and into a channel nothing was
    /// reading, and nothing recovered but launching the application again. It
    /// is the ordinary condition of an iOS application rather than an edge: the
    /// system suspends the process seconds after it goes into the background
    /// and takes the connection with it.
    #[test]
    fn a_connection_whose_thread_has_ended_is_not_open() {
        let channel = Channel::default();
        let (asking, draining) = tokio::sync::mpsc::unbounded_channel();
        let alive = Arc::new(AtomicBool::new(true));
        *channel.talking.lock().expect("nothing panicked") = Some(Live {
            asking,
            alive: Arc::clone(&alive),
        });
        assert!(channel.open_now(), "the connection is being served");

        // What the serving thread does on its way out, in that order.
        drop(draining);
        alive.store(false, Ordering::Release);

        assert!(
            !channel.open_now(),
            "and this phone knows it has to dial again"
        );
    }

    /// A call that found nothing to carry it blames the connection.
    ///
    /// Which is the only thing [`Channel::ask`] reads to decide whether
    /// dialling again is worth anything. Told apart by the failure's *kind* it
    /// could not be: what a lost connection answers with is a sentence
    /// somebody reads, which makes it a domain failure, and the test in front
    /// of the re-dial looked for the two transport kinds only.
    #[test]
    fn a_call_with_nothing_to_carry_it_asks_to_be_dialled_again() {
        let channel = Channel::default();

        let refused = channel
            .asked(sync_memory::SESSION_SUBSCRIBE, &json!({}))
            .expect_err("there is no connection to ask on");

        assert!(
            matches!(refused, Unanswered::Gone(_)),
            "a connection that is not there is not the computer refusing"
        );
    }

    /// The computer's own refusal is not dialled again.
    ///
    /// The other half of the same decision, and the half that costs something
    /// when it is wrong: a second connection would ask the same machine the
    /// same question, and for a write it would be the record written twice.
    #[test]
    fn a_refusal_from_the_computer_is_not_worth_a_second_connection() {
        let refused = answered(&json!({
            "jsonrpc": "2.0", "id": 4,
            "error": {"message": "no such record", "data": {"kind": "invalid_argument"}},
        }))
        .expect_err("the computer refused");

        assert!(!super::carried(&refused));
    }

    /// A bell rung before anybody was listening is still heard.
    ///
    /// The case is not a race to be tidied away, it is the common one: a
    /// connection ends while the keeper is in the middle of dialling, not while
    /// it is asleep. Forgotten, the keeper would then sleep through the very
    /// event that rang it — up to half a minute of a phone showing that it
    /// cannot reach a computer that is sitting there waiting.
    #[test]
    fn a_bell_rung_before_the_wait_is_still_heard() {
        let waker = Waker::default();
        waker.ring();

        let began = Instant::now();
        waker.waited(Duration::from_secs(30));

        assert!(
            began.elapsed() < Duration::from_secs(1),
            "the keeper woke on the bell rather than waiting out the rest"
        );
    }

    /// And a bell is heard once.
    ///
    /// Otherwise every wait after the first returns at once, which is not a
    /// keeper that reacts quickly — it is a phone dialling as fast as it can
    /// for as long as it has no network.
    #[test]
    fn a_bell_is_not_heard_twice() {
        let waker = Waker::default();
        waker.ring();
        waker.waited(Duration::from_secs(30));

        let began = Instant::now();
        waker.waited(Duration::from_millis(120));

        assert!(began.elapsed() >= Duration::from_millis(100));
    }

    /// What the window is handed for each state it draws.
    ///
    /// The word is what it matches on and the sentence is what it shows, and
    /// only one state has a sentence: the others are this phone saying where it
    /// is, and a reason attached to *reaching* would be a reason for something
    /// that has not failed.
    #[test]
    fn the_window_is_told_the_state_and_the_reason_for_it() {
        assert_eq!(
            Reach::Reaching.told(),
            json!({"reach": "reaching", "trouble": null})
        );
        assert_eq!(
            Reach::Talking.told(),
            json!({"reach": "talking", "trouble": null})
        );
        assert_eq!(
            Reach::Away("the computer did not admit this phone".to_owned()).told(),
            json!({"reach": "away", "trouble": "the computer did not admit this phone"}),
        );
    }

    /// Where the phone stands is said once per change and not once per look.
    ///
    /// The keeper asks this on every turn of its loop, and most turns are a
    /// phone exactly where it was. An event per turn would be a window
    /// re-rendering every half-minute to be told nothing.
    #[test]
    fn the_window_hears_about_a_change_and_not_about_a_turn() {
        let channel = Channel::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let writing = Arc::clone(&heard);
        channel.tell(move |reach: &Reach| {
            writing
                .lock()
                .expect("nothing panicked")
                .push(reach.clone());
        });

        channel.stands(Reach::Reaching);
        channel.stands(Reach::Reaching);
        channel.stands(Reach::Away("the computer could not be reached".to_owned()));
        channel.stands(Reach::Away("the computer could not be reached".to_owned()));

        let heard = heard.lock().expect("nothing panicked");
        assert_eq!(heard.len(), 2);
        assert_eq!(heard[0], Reach::Reaching);
        assert_eq!(channel.reach_now(), heard[1]);
    }

    /// Forgetting the computer leaves the phone with nothing to reach.
    ///
    /// Said rather than left to be worked out: the keeper would otherwise go on
    /// reporting the last thing that happened to a connection this phone is no
    /// longer entitled to make.
    #[test]
    fn forgetting_the_computer_says_so() {
        let channel = Channel::default();
        channel.hold(&Pairing {
            endpoint: "somewhere".to_owned(),
            secret: "something".to_owned(),
        });
        assert_eq!(channel.reach_now(), Reach::Reaching);

        channel.close();

        assert_eq!(channel.reach_now(), Reach::Unpaired);
        assert!(channel.pairing().is_none());
    }

    /// An answer is what the caller asked for, and nothing is read out of it.
    #[test]
    fn an_answer_is_handed_over_whole() {
        let answer = answered(&json!({"jsonrpc": "2.0", "id": 4, "result": {"dropped": 0}}))
            .expect("the computer answered");
        assert_eq!(answer, json!({"dropped": 0}));
    }
}
