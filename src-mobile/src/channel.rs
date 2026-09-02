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
//! # What a refusal from the door means here
//!
//! The door answers every rejected connection with one sentence and never says
//! which check produced it. That sentence is carried back to the person
//! unchanged. Inventing a friendlier one here would be inventing a claim about
//! why — the one thing the door deliberately does not say.

use std::io;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use iroh::endpoint::{Connection as Quic, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointId};
use serde_json::{Value, json};
use sync_memory::{
    CHANNEL_VERSION, Connection, Effect, MAX_FRAME_BYTES, METHODS, MemoryError, Operations,
    REMOTE_HELLO, Transport, effect,
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
/// Thirty seconds is longer than any operation the channel has and shorter than
/// somebody staring at a screen will tolerate.
const PATIENCE: Duration = Duration::from_secs(30);

/// The pairing this phone is holding: where to dial, and what to say.
#[derive(Clone)]
pub struct Pairing {
    pub endpoint: String,
    pub secret: String,
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
    talking: Mutex<Option<Sender<Asked>>>,
    /// Held across a dial, so that two screens asking at once make one.
    ///
    /// Without it the first call of a launch and the one beside it both find
    /// nothing to talk on and both dial, and the second connection replaces the
    /// first while the first is being asked on.
    dialling: Mutex<()>,
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
type Asked = (Value, Sender<sync_memory::Result<Value>>);

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
        self.dial(pairing)?;
        drop(one_at_a_time);
        Ok(())
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
    }

    /// Make sure there is something to say a call on.
    ///
    /// The one place a connection is opened on somebody's behalf rather than
    /// because they asked to pair. It is checked again after the lock: the wait
    /// may have been for the dial that is now the answer.
    fn reach(&self) -> sync_memory::Result<()> {
        let one_at_a_time = self.dialling.lock().map_err(|_| away(&poisoned().0))?;
        if self.open_now() {
            return Ok(());
        }
        let pairing = self
            .pairing()
            .ok_or_else(|| away("this phone is not paired with a computer"))?;
        let dialled = self.dial(&pairing).map_err(|trouble| away(&trouble.0));
        drop(one_at_a_time);
        dialled
    }

    /// Dial, and keep both what answered and what it was dialled with.
    ///
    /// The pairing is kept only once the computer has admitted this phone: a
    /// code that was refused is not a computer this phone has.
    fn dial(&self, pairing: &Pairing) -> Result<(), Trouble> {
        let asking = greeted(pairing)?;
        *self.talking.lock().map_err(|_| poisoned())? = Some(asking);
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
            Err(failure) if carried(&failure) => self.again(method, params),
            outcome => outcome,
        }
    }

    /// The connection went away under a call. Make another one, and decide
    /// whether the call itself may go out on it.
    fn again(&self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        self.drop_connection();
        self.reach()?;
        if effect(method) == Some(Effect::Reads) {
            return self.asked(method, params);
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
    pub fn open_now(&self) -> bool {
        self.talking.lock().is_ok_and(|held| held.is_some())
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
    }

    fn asked(&self, method: &str, params: &Value) -> sync_memory::Result<Value> {
        let (answering, answered) = channel();
        {
            let talking = self.talking.lock().map_err(|_| away(&poisoned().0))?;
            talking
                .as_ref()
                .ok_or_else(gone)?
                .send((json!({"method": method, "params": params}), answering))
                .map_err(|_| gone())?;
        }
        answered.recv().map_err(|_| gone())?
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
/// The three happen together because a connection that fails any of them is not
/// a connection anybody should be handed: an unadmitted device and a computer
/// speaking a channel this build cannot read are both *not paired with this*,
/// and finding either one out later would mean finding it out in the middle of
/// somebody's work.
fn greeted(pairing: &Pairing) -> Result<Sender<Asked>, Trouble> {
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
    std::thread::spawn(move || dialled(&dialling))
        .join()
        .map_err(|_| Trouble("the connection could not be started".to_owned()))?
}

/// The dial itself, on a thread that owns everything it makes.
fn dialled(pairing: &Pairing) -> Result<Sender<Asked>, Trouble> {
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

    // The greeting is written straight onto the transport rather than through
    // `Connection`, and for one reason: the door's refusal is a sentence
    // somebody reads, and a client that wrapped it — *`remote.hello` failed: …*
    // — would be showing them our formatting of a message whose whole point is
    // that it is the door's. Its id is 0, which the channel's own numbering
    // starts past.
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

    let mut connection = Connection::new(wire);
    agreed(&mut connection)?;
    Ok(serving(connection))
}

/// Read the computer's channel number and refuse to speak past it.
///
/// The same check the window makes against its own sidecar, for the reason the
/// number exists at all: a phone installed from a store is months behind by
/// construction, and *old client, new computer* is its ordinary condition
/// rather than its edge.
fn agreed<T: Transport>(connection: &mut Connection<T>) -> Result<(), Trouble> {
    let listed = connection
        .request(METHODS, &json!({"channel": CHANNEL_VERSION}))
        .map_err(Trouble::saying)?;
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

/// Hand the connection to a thread and hand back the way to ask it things.
///
/// The thread ends when the last sender is dropped, which is what makes
/// replacing a connection sufficient to close the one before it.
fn serving<T: Transport + Send + 'static>(mut connection: Connection<T>) -> Sender<Asked> {
    let (asking, asked): (Sender<Asked>, Receiver<Asked>) = channel();
    std::thread::spawn(move || {
        while let Ok((call, answering)) = asked.recv() {
            let method = call["method"].as_str().unwrap_or_default().to_owned();
            let answer = connection.request(&method, &call["params"]);
            // A phone that stopped waiting is not a reason to stop reading: the
            // answer has been taken off the wire either way, and the next call
            // needs the stream where this one left it.
            drop(answering.send(answer));
        }
    });
    asking
}
