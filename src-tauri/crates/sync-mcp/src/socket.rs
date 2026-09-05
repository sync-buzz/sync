//! The host channel's door, for the window and for the clock.
//!
//! The second door on the resident process, and the one Sync itself comes
//! through. What it serves is [`crate::host::Host`] — the same dispatcher the
//! stdio door serves, unchanged, because `host.rs` never knew its transport.
//!
//! # Why a socket rather than the port the agents use
//!
//! The MCP door listens on a fixed TCP port, and a port already taken is
//! reported rather than stepped around — `lib.rs` treats a server that did not
//! start as survivable precisely because the window did not depend on it. Put
//! the window's memory on that port and whatever else is listening on 41847
//! takes the whole product down with it.
//!
//! A socket in the application's own directory cannot collide with anything,
//! and its permissions are the whole of its access control. The token exists
//! because an agent is configured with a URL; the window is configured with
//! nothing and needs none.
//!
//! # One connection, one project
//!
//! A connection names its project once, with `project.attach`, and every
//! message after that is **exactly** what the stdio door has always carried —
//! same methods, same params, same errors. That is deliberate: putting the
//! project on every call would have meant taking it off the params again before
//! the operations saw it, and threading it through the one piece of per-project
//! state the client keeps — the revision it expects its next write to be
//! against. A connection is a file descriptor, not an engine, so a window with
//! four projects open holds four of them against one process.
//!
//! # What runs where
//!
//! Every dispatch reaches Git, `LanceDB` and possibly a model, so it goes to a
//! blocking thread. The alternative is a call in one project stalling the
//! runtime that the agents' door is sharing.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use sync_memory::{
    ATTACH, ATTEND, MAX_FRAME_BYTES, MemoryError, PROJECTS, REMOTE_DEVICES, carried,
};

use crate::application::Application;
use crate::host::Host;
use crate::remote::Devices;
use crate::watching::Subscriptions;

/// What a connection may name a project by.
///
/// One question, asked of the door rather than of the caller, because it is a
/// property of how the caller got here. The socket in this machine's own
/// application directory is reached by something already on this machine, and
/// its permissions are the whole of that claim; anything that arrived from
/// somewhere else is a different kind of caller however well it identified
/// itself.
///
/// A path is the difference. On this machine it is a convenience — the window
/// already knows where its project is, and the opening flow reads a repository
/// to find the name it will be registered under, so a door that took only names
/// could not open a project at all. From elsewhere the same field is the right
/// to name **any** directory on somebody else's computer, and no amount of
/// pairing makes that a thing to hand out.
pub(crate) struct Naming {
    /// Whether this connection may say `project.attach` with a path.
    ///
    /// False means the connection names a registered project by its key, in
    /// every call, and has no attach at all: with no project to remember, one
    /// connection serves every project at once and there is no per-connection
    /// state for anybody to get wrong.
    pub(crate) by_path: bool,
}

impl Naming {
    /// Whether this door may say where a project is.
    ///
    /// The same question the field asks, put about an answer rather than about
    /// a call, and it is one question rather than two: a door that will not be
    /// handed a path has no business handing one out.
    fn says_where(&self, method: &str) -> bool {
        self.by_path || method != PROJECTS
    }
}

/// Serve the host channel on `path` until the process ends.
///
/// # Errors
///
/// When the socket cannot be bound — including when another process is already
/// listening on it, which is a machine running two copies of Sync rather than a
/// state to recover from.
pub async fn serve(
    host: Arc<Host>,
    application: Arc<Application>,
    devices: Arc<Devices>,
    subscriptions: Arc<Subscriptions>,
    path: PathBuf,
) -> io::Result<()> {
    let listener = bind(&path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let host = Arc::clone(&host);
        let application = Arc::clone(&application);
        let devices = Arc::clone(&devices);
        let subscriptions = Arc::clone(&subscriptions);
        // Per connection, because one window has several projects open and a
        // call in one of them must not be behind a call in another.
        tokio::spawn(async move {
            // This door is a file in this machine's own application directory,
            // reachable only by something running as this user. The window is
            // on the other end of it, and it names its project by path.
            if let Err(error) = attend(
                &host,
                &application,
                &devices,
                &subscriptions,
                stream,
                &Naming { by_path: true },
            )
            .await
            {
                // A connection ending is ordinary — the window closed a project
                // — so this is only worth a line when it ended for a reason.
                if error.kind() != io::ErrorKind::UnexpectedEof {
                    tracing::info!(%error, "a host connection ended");
                }
            }
        });
    }
}

/// Take the socket, refusing to steal one another process is using.
///
/// A socket file outlives the process that made it, so binding means deciding
/// what an existing one is. Connecting to it is the only way to tell: an answer
/// means somebody is alive and this process must not take their door away, and
/// a refusal means the file is what a crash left behind.
fn bind(path: &Path) -> io::Result<UnixListener> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "another process is already serving the host channel on {}",
                    path.display()
                ),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let listener = UnixListener::bind(path)?;
    // The permissions are the access control, so they are set rather than left
    // to whatever umask this process was started with.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    // Written beside the door, because whoever wants this door next has to be
    // able to take it. A socket says somebody is there and not who, and Sync's
    // rule is that this process ends when Sync does — so the next Sync needs a
    // way to end one that outlived its own.
    let _ = std::fs::write(pid_file(path), std::process::id().to_string());
    Ok(listener)
}

/// Where the process serving `socket` writes its own process id.
#[must_use]
pub fn pid_file(socket: &Path) -> PathBuf {
    socket.with_extension("pid")
}

/// Read one connection to its end, answering every line.
///
/// What arrives is a stream of bytes in both directions, and which kind of
/// stream it is belongs to the door that accepted it rather than to the
/// channel. `host.rs` says the dispatcher does not know its transport; a
/// signature naming one is the place that claim quietly stops being true, and
/// a second door would have had to copy this loop to get past it.
pub(crate) async fn attend<S>(
    host: &Arc<Host>,
    application: &Arc<Application>,
    devices: &Arc<Devices>,
    subscriptions: &Arc<Subscriptions>,
    stream: S,
    naming: &Naming,
) -> io::Result<()>
where
    // `Send` and `'static` are not the transport's business either — they are
    // what the writer task below needs of anything it is handed.
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (reading, writing) = tokio::io::split(stream);
    attend_read(
        host,
        application,
        devices,
        subscriptions,
        Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
        writing,
        naming,
    )
    .await
}

/// The same, for a door that has already read something off the stream.
///
/// The network door has to know who is calling before it lets them at any of
/// this, and what it reads to find out is one line of the same framing. Handing
/// the halves over rather than the stream is what lets it read that line with
/// [`Frames`] and pass the very same reader on — a door that made its own
/// buffer would either lose whatever the first read pulled in behind the
/// greeting, or need its own copy of the framing to avoid it.
pub(crate) async fn attend_read<R, W>(
    host: &Arc<Host>,
    application: &Arc<Application>,
    devices: &Arc<Devices>,
    subscriptions: &Arc<Subscriptions>,
    mut lines: Frames<R>,
    writing: W,
    naming: &Naming,
) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Send + Unpin + 'static,
{
    // Everything written on this connection goes through one queue and one
    // task, in both of the directions this connection can end up serving. That
    // is what lets a call be answered by whichever task finished it rather than
    // by the loop that read it — two tasks writing into a stream would
    // interleave two answers into one unreadable line.
    let (queue, queued) = tokio::sync::mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(draining(writing, queued));

    let mut attached: Option<PathBuf> = None;
    // The watches taken on this connection, so that the end of it can be said
    // once and in one message rather than discovered per event by an
    // application that cannot see this far.
    let mut mine: Vec<u64> = Vec::new();
    // Calls in flight on this connection, and the reading loop waits here when
    // they are all taken. Back-pressure rather than a queue that grows: a
    // caller sending faster than this machine answers is a caller that should
    // be made to wait, not one whose work should be accumulated in memory.
    let running = Arc::new(tokio::sync::Semaphore::new(AT_ONCE));
    // What a carried call is handed to the application with. Built once,
    // because none of the four changes for the life of a connection.
    let carrying = Carrying {
        host,
        application,
        subscriptions,
        naming,
    };

    let outcome = loop {
        let line = match lines.next().await {
            Ok(Frame::Line(line)) => line,
            // Answered rather than dropped, and the connection kept: a caller
            // that sent one message too big is a caller that can send a smaller
            // one, and a door that goes silent instead teaches nothing.
            Ok(Frame::TooLong) => {
                if say(&queue, &too_long()) {
                    continue;
                }
                break Ok(());
            }
            Ok(Frame::End) => break Ok(()),
            Err(error) => break Err(error),
        };
        if line.trim().is_empty() {
            continue;
        }
        // Answered, not fatal, exactly as the stdio door answers it: a line
        // that is not JSON is a caller's mistake, and ending the connection
        // over it would report the same class of mistake two different ways.
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            if say(&queue, &malformed()) {
                continue;
            }
            break Ok(());
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let mut params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        // The one call that turns this connection around. Everything after
        // it is answers to what *this* process asked, so the loop below never
        // sees another line — [`answer_calls`] owns the connection from here.
        if method == ATTEND {
            if !naming.by_path {
                if say(&queue, &no_turning_round(&id)) {
                    continue;
                }
                break Ok(());
            }
            say(
                &queue,
                &json!({"jsonrpc": "2.0", "id": id, "result": {"attending": true}}),
            );
            return answer_calls(application, subscriptions, lines, queue, writer).await;
        }

        if let Some(answer) =
            about_this_connection(devices, naming, &mut attached, &id, method, &params)
        {
            if say(&queue, &answer) {
                continue;
            }
            break Ok(());
        }

        // Carried to the application and not read on the way. Every one of
        // these is a fact about the machine rather than about a project — an
        // artefact on its disk, the registry it fetches, a secret in its
        // keychain — and the checks that go with them are all on that side. A
        // package's own request is the clearest case: what it may reach is a
        // sentence in the manifest of the artefact installed there, so the
        // check and the request are both there and this door's whole part is to
        // carry the call.
        //
        // Not refused on this machine's own socket either, and there is nothing
        // to gain by refusing it: the answer would be the same wherever the
        // caller is, because none of these depends on where the caller is.
        if carried(method) {
            if !onward(&carrying, &running, &queue, &mut mine, (id, method, params)).await {
                break Ok(());
            }
            continue;
        }

        let project = match about_a_project(host, naming, attached.as_ref(), method, &mut params) {
            Ok(project) => project,
            Err(why) => {
                if say(&queue, &refusal(&id, &why)) {
                    continue;
                }
                break Ok(());
            }
        };
        // Taken before the call is spawned and let go when it is answered, so
        // that this `await` is where a connection with everything in flight
        // stops reading.
        let Ok(permit) = Arc::clone(&running).acquire_owned().await else {
            break Ok(());
        };
        let says_where = naming.says_where(method);
        let host = Arc::clone(host);
        let method = method.to_owned();
        let queue = queue.clone();
        // **Off the reading loop, which is the whole of this.** The call still
        // goes to a blocking thread — a dispatch reaches Git, `LanceDB` and
        // possibly a model — but the loop no longer waits for it, so the next
        // line is read while this one is being worked on. Over a socket the
        // difference is invisible; over a network with fifty milliseconds on it
        // the twenty small calls that draw a screen were a second of waiting.
        //
        // What this does *not* loosen is the order the engine writes in: a
        // project's memory is behind one mutex, so two calls about one project
        // are as serialised as they ever were. The concurrency is the
        // transport's, and it is the transport that was serialising things that
        // had no reason to be.
        //
        // What it does change, and this is worth saying out loud: two calls a
        // caller sent without waiting for the first answer may now run in
        // either order. That is this engine's ordinary condition already — the
        // agents' door has served callers concurrently all along — and a write
        // states the revision it expects, so a caller that needs one call after
        // another waits for the first, which is what it must do for the
        // revision anyway.
        tokio::spawn(async move {
            let answered =
                tokio::task::spawn_blocking(move || host.dispatch(&project, &method, &params))
                    .await;
            let answer = match answered {
                Ok(outcome) => answered_with(&id, outcome.map(|it| named_only(says_where, it))),
                // The blocking thread was lost. Answered rather than dropped:
                // a caller waiting on an id that will never come back waits for
                // as long as its own patience, and this is not its mistake.
                Err(error) => refusal(&id, &format!("a host call could not be run: {error}")),
            };
            say(&queue, &answer);
            drop(permit);
        });
    };

    // Whoever was watching a conversation over this connection is not watching
    // it any more. Said before the wait below, because the wait is for answers
    // already queued and an event queued after this point would be written into
    // a stream nobody is reading.
    nobody_is_watching(application, subscriptions, &mine);

    // Let go of this loop's own sender, then wait. The calls still running hold
    // senders of their own, so the channel closes when the last of them has
    // queued its answer and the writer stops when it has written it — a
    // connection that ended is not a reason to drop an answer somebody is
    // already waiting for.
    drop(queue);
    let _ = writer.await;
    outcome
}

/// What one connection carries a call to the application with.
///
/// Four handles that never change for the life of a connection, held together
/// so that carrying a call takes one argument rather than four. What varies per
/// call is the call.
struct Carrying<'a> {
    host: &'a Arc<Host>,
    application: &'a Arc<Application>,
    subscriptions: &'a Arc<Subscriptions>,
    naming: &'a Naming,
}

/// Hand one call to the application, and say whether this connection is still
/// worth reading.
///
/// Everything carried is a fact about the machine rather than about the
/// contents of a project — an artefact on its disk, the registry it fetches, a
/// secret in its keychain, a process it raised — and the checks that go with
/// them are all on that side. A package's own request is the clearest case:
/// what it may reach is a sentence in the manifest of the artefact installed
/// there, so the check and the request are both there and this door's whole
/// part is to carry the call.
///
/// Not refused on this machine's own socket either, and there is nothing to
/// gain by refusing it: the answer would be the same wherever the caller is,
/// because none of these depends on where the caller is.
///
/// `false` means the connection has ended — the queue this would have answered
/// on has no writer left.
async fn onward(
    carrying: &Carrying<'_>,
    running: &Arc<tokio::sync::Semaphore>,
    queue: &tokio::sync::mpsc::UnboundedSender<String>,
    mine: &mut Vec<u64>,
    (id, method, mut params): (Value, &str, Value),
) -> bool {
    // The key becomes the path the agent will be raised in, here and nowhere
    // else. Replaced rather than removed — which is where this parts company
    // with an operation: the application works *in* the project and has to be
    // told which one, while the engine's surface is handed a project already
    // and would be confused by a second name for it.
    let by_key = !carrying.naming.by_path;
    let mut asked: Option<String> = None;
    if by_key && sync_memory::names_a_project(method) {
        match a_key_into_a_path(carrying.host, &mut params) {
            Ok(key) => asked = Some(key),
            Err(why) => return say(queue, &refusal(&id, &why)),
        }
    }
    // A watch is bound to this connection *before* the application hears about
    // it, and that order is the whole of it: the agent may say something
    // between being asked to watch and the answer reaching this line, and an
    // event arriving under a number nothing holds yet is a word of somebody's
    // conversation dropped in silence.
    let watching = (method == sync_memory::SESSION_SUBSCRIBE).then(|| {
        let watch = carrying.subscriptions.mint(queue.clone());
        if let Some(members) = params.as_object_mut() {
            members.insert("subscription".to_owned(), json!(watch));
        }
        mine.push(watch);
        watch
    });
    let Ok(permit) = Arc::clone(running).acquire_owned().await else {
        return false;
    };
    let application = Arc::clone(carrying.application);
    let subscriptions = Arc::clone(carrying.subscriptions);
    let host = Arc::clone(carrying.host);
    let queue = queue.clone();
    let method = method.to_owned();
    tokio::spawn(async move {
        let answered = application.call(&method, params).await;
        // A watch the application refused is let go of now rather than when the
        // connection ends: the number was minted on the way in and nothing on
        // the other side is holding it.
        if let (Some(watch), true) = (watching, answered.is_err()) {
            subscriptions.ended(&[watch]);
        }
        let answered = if by_key && sync_memory::about_a_session(&method) {
            answered.map(|answer| named_by_key(&host, answer, asked.as_deref()))
        } else {
            answered
        };
        say(&queue, &answered_with(&id, answered));
        drop(permit);
    });
    true
}

/// Tell the application about the watches this connection took with it.
///
/// One message naming all of them rather than one per watch, and it is the only
/// way the application can find out: it writes events into a socket to an
/// engine that is still there, and the connection that ended is a hop further
/// on. Without it a phone put in a pocket leaves a conversation serialising
/// every word its agent writes into a queue nobody drains.
fn nobody_is_watching(
    application: &Arc<Application>,
    subscriptions: &Arc<Subscriptions>,
    mine: &[u64],
) {
    let gone = subscriptions.ended(mine);
    if gone.is_empty() {
        return;
    }
    let application = Arc::clone(application);
    tokio::spawn(async move {
        let _ = application
            .call(sync_memory::SESSION_DROPPED, json!({"subscriptions": gone}))
            .await;
    });
}

/// The key a device named its project by, turned into where that project is.
///
/// The same resolution [`about_a_project`] makes for an operation, made again
/// here because a carried call does not go through it — and made *without*
/// spending the key: the engine's surface is handed a project and would choke
/// on a second name for one, while the application is handed nothing and has to
/// be told.
///
/// A key the registry does not hold is a refusal by name. Nothing is derived
/// from it and no directory is looked for, which is the difference between a
/// key and a path and the reason a connection from elsewhere gets only the
/// first.
fn a_key_into_a_path(host: &Arc<Host>, params: &mut Value) -> Result<String, String> {
    let key = params
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "this call names the project it is about — `project`, a key as `{PROJECTS}` lists them"
            )
        })?
        .to_owned();
    let found = host
        .project_named(&key)
        .ok_or_else(|| format!("this machine holds no project called `{key}`"))?;
    if let Some(members) = params.as_object_mut() {
        members.insert(
            "project".to_owned(),
            json!(found.to_string_lossy().into_owned()),
        );
    }
    Ok(key)
}

/// An answer with every project in it named the way the device that asked names
/// them.
///
/// The other half of [`a_key_into_a_path`], and it exists because the answer to
/// a call about conversations is full of directories: a row says which project
/// it belongs to and where its agent is working, and a pointer says only the
/// second. Both are paths on somebody else's computer. A device chose that
/// project by key and picks its own conversations out by comparing with it, so
/// the key is at once the only thing it can use and the only thing it may be
/// told.
///
/// **`worktree` is removed rather than translated.** A device cannot name a
/// tree, throw it away or open one, so what the member carries is the layout of
/// somebody's disk in exchange for nothing.
///
/// **`project` and `cwd` become keys, by three rules in this order**, and the
/// order is what makes them cover the two shapes an answer comes in:
///
/// 1. the registry knows the project at that path — a live row's `project`;
/// 2. the object itself said which project it belongs to — a live row's `cwd`
///    when the agent is working in a tree;
/// 3. the call named one — a dormant pointer, which carries `cwd` and nothing
///    to say whose it is, in a list that was asked for by key.
///
/// Rules two and three both answer a conversation being held in a working tree
/// by naming the project instead. That is not a rounding: the conversation
/// belongs to that project, and the tree is the part this device is not shown.
fn named_by_key(host: &Arc<Host>, answer: Value, asked: Option<&str>) -> Value {
    match answer {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| named_by_key(host, item, asked))
                .collect(),
        ),
        Value::Object(members) => {
            let mut members: serde_json::Map<String, Value> = members
                .into_iter()
                .filter(|(name, _)| name != "worktree")
                .map(|(name, value)| (name, named_by_key(host, value, asked)))
                .collect();
            let named = |members: &serde_json::Map<String, Value>, member: &str| {
                members
                    .get(member)
                    .and_then(Value::as_str)
                    .and_then(|path| host.key_at(std::path::Path::new(path)))
            };
            let belongs = named(&members, "project");
            if let Some(key) = belongs.clone() {
                members.insert("project".to_owned(), json!(key));
            }
            if members.contains_key("cwd")
                && let Some(key) = named(&members, "cwd")
                    .or(belongs)
                    .or_else(|| asked.map(ToOwned::to_owned))
            {
                members.insert("cwd".to_owned(), json!(key));
            }
            Value::Object(members)
        }
        other => other,
    }
}

/// One outcome, as the answer to the call that asked for it.
///
/// The engine's own `kind` travels in `data`, so the window can tell a stale
/// revision from a locked project without reading prose.
fn answered_with(id: &Value, outcome: sync_memory::Result<Value>) -> Value {
    match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32000,
            "message": error.to_string(),
            "data": {"kind": error.kind().map(sync_memory::MemoryErrorKind::as_wire)},
        }}),
    }
}

/// How many calls one connection may have in flight at once.
///
/// Named rather than left open, because every one of them holds a blocking
/// thread while it runs and a caller deciding how many of those this process
/// spends is a caller deciding how much of the machine it gets.
///
/// Sixteen: the case this exists for is a screen drawing itself, which the
/// measurement behind it put at about twenty small calls, so this turns a
/// second of waiting into two rounds rather than into one. A larger number
/// would buy the last round at the cost of a worse answer to *how much can one
/// caller take*.
const AT_ONCE: usize = 16;

/// Put one message on the connection, and say whether there is still one.
///
/// False means the writer has gone, which is the connection having ended. It is
/// not an error to report: the caller left, and there is nobody to tell.
fn say(queue: &tokio::sync::mpsc::UnboundedSender<String>, answer: &Value) -> bool {
    queue.send(answer.to_string()).is_ok()
}

/// Write whatever is queued, in the order it was queued, until nobody is left
/// to queue anything.
async fn draining<W>(mut writing: W, mut queued: tokio::sync::mpsc::UnboundedReceiver<String>)
where
    W: AsyncWrite + Send + Unpin + 'static,
{
    while let Some(message) = queued.recv().await {
        if writing
            .write_all(format!("{message}\n").as_bytes())
            .await
            .is_err()
            || writing.flush().await.is_err()
        {
            break;
        }
    }
}

/// Which project a call is about, or what to tell the caller instead.
///
/// The two doors answer this differently and that difference is the whole of
/// what a connection from elsewhere may not do — so it is asked of the door,
/// once, rather than decided again inside each call.
fn about_a_project(
    host: &Arc<Host>,
    naming: &Naming,
    attached: Option<&PathBuf>,
    method: &str,
    params: &mut Value,
) -> Result<PathBuf, String> {
    // Asked before a project is demanded, because these are the two questions a
    // connection has to ask first: what this surface answers, and what projects
    // there are to choose between. Refusing them for naming no project would
    // leave a client that cannot see the file system with no first move.
    if Host::answers_without_project(method) {
        return Ok(PathBuf::new());
    }
    if naming.by_path {
        // Said rather than answered from a guess. There is no default project
        // and inventing one would answer a call meant for one repository out of
        // another.
        return attached.cloned().ok_or_else(|| {
            "this connection has not said which project it is about — call `project.attach` first"
                .to_owned()
        });
    }
    // The key is resolved through the registry and nowhere else, so a name this
    // machine has not registered is a refusal by name — never an attempt to
    // open whatever is at some path derived from it.
    let key = params
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "every call on this connection names its project — `project`, a key as `{PROJECTS}` lists them"
            )
        })?
        .to_owned();
    let found = host
        .project_named(&key)
        .ok_or_else(|| format!("this machine holds no project called `{key}`"))?;
    // Spent where it was read. Several operations hand their parameters to the
    // engine whole, so a key left in them would arrive at a tool that never
    // asked for one — and what a strict schema does with an argument it does
    // not know is not this door's to guess.
    if let Some(members) = params.as_object_mut() {
        members.remove("project");
    }
    Ok(found)
}

/// What a door that may not be turned around says when it is asked to be.
///
/// This machine's own application, and nothing else. What is on the other side
/// of an attended connection is whoever answers every tool an agent calls — so
/// a caller that could turn its connection around would be answering, in the
/// application's name, calls made by agents it has never met.
fn no_turning_round(id: &Value) -> Value {
    refusal(
        id,
        "this connection asks and is answered — the channel back belongs to the application this engine is a part of",
    )
}

/// The project list with nothing in it but what a caller off this machine may
/// have.
///
/// A key and a name are what such a caller chooses between and all it can act
/// on: it names the project by key in every call, and there is no operation it
/// could hand a path to. What is left is the layout of somebody's disk —
/// their home directory's name, where they keep their work, how many projects
/// they have under one folder — sent to a phone that has no use for a word of
/// it. Nothing was leaking; it was being given away for nothing, which is the
/// version of this that never shows up as a defect.
fn named_only(says_where: bool, mut answer: Value) -> Value {
    if says_where {
        return answer;
    }
    if let Some(listed) = answer.get_mut("projects").and_then(Value::as_array_mut) {
        for project in listed {
            if let Some(members) = project.as_object_mut() {
                members.remove("path");
            }
        }
    }
    answer
}

/// The calls a door answers about the connection itself.
///
/// Neither is an [`Operation`](crate::host::Operation), and for one reason: an
/// operation is handed a [`Domain`](crate::domain::Domain) — one project's
/// memory — and neither of these is about a project. They are about *this
/// connection*, which is why the answer to both depends on which door heard
/// them, and why what a door does not allow is a refusal rather than a silence.
///
/// `None` means this was not one of them, and the caller goes on to the surface.
fn about_this_connection(
    devices: &Arc<Devices>,
    naming: &Naming,
    attached: &mut Option<PathBuf>,
    id: &Value,
    method: &str,
    params: &Value,
) -> Option<Value> {
    match method {
        ATTACH if naming.by_path => Some(match params.get("path").and_then(Value::as_str) {
            Some(path) => {
                *attached = Some(PathBuf::from(path));
                json!({"jsonrpc": "2.0", "id": id, "result": {"attached": path}})
            }
            None => refusal(id, "`project.attach` needs the path of a project"),
        }),
        // Refused by name rather than ignored. A client that attached and was
        // answered with silence would go on to make every call it meant for
        // that project against nothing.
        ATTACH => Some(refusal(
            id,
            "this connection names a registered project in each call, as `project` — there is no `project.attach` on it and no naming a directory by its path",
        )),
        REMOTE_DEVICES if naming.by_path => {
            devices.stated(params);
            Some(json!({"jsonrpc": "2.0", "id": id, "result": devices.described()}))
        }
        // The whole reason this one is heard by the door: a device that could
        // name the devices this machine admits is a device nobody can revoke.
        REMOTE_DEVICES => Some(refusal(
            id,
            "the devices this machine admits are stated by the application on it, and not over this connection",
        )),
        _ => None,
    }
}

/// Hold the channel back until the application lets go of it.
///
/// Two halves and they are genuinely independent: a task drains the queue of
/// outgoing requests, and this one reads answers as they come. Putting both in
/// one loop would mean a request could only go out between two answers, which
/// for a channel whose calls take seconds each is a queue behind a wait.
///
/// The connection ending is how it is meant to end — Sync closing, or Sync
/// reconnecting after this process restarted. Everything still waiting is
/// failed rather than left, so an agent hears why now rather than in a minute.
async fn answer_calls<R>(
    application: &Arc<Application>,
    subscriptions: &Arc<Subscriptions>,
    mut lines: Frames<R>,
    queue: tokio::sync::mpsc::UnboundedSender<String>,
    writer: tokio::task::JoinHandle<()>,
) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
{
    // The queue and the task draining it are the ones this connection has had
    // since it was accepted. Handed over rather than made again: one stream has
    // one writer, and a second would interleave its lines with the first's.
    application.attend(queue);

    let reading = async {
        loop {
            let line = match lines.next().await? {
                Frame::Line(line) => line,
                Frame::TooLong => {
                    // Nothing to answer to: this direction carries answers, and
                    // an answer too long to read is a call that will time out
                    // on its own patience rather than one anybody can refuse.
                    tracing::warn!(
                        "an answer on the channel back was longer than the channel allows"
                    );
                    continue;
                }
                Frame::End => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(answer) = serde_json::from_str::<Value>(&line) else {
                // Not fatal, for the reason a malformed request is not: one
                // unreadable line is a mistake at the other end, and dropping
                // the channel over it would take every tool call with it.
                tracing::warn!("an answer on the channel back could not be read as JSON");
                continue;
            };
            // The one thing on this connection that is not an answer, and the
            // only message this process ever sends that nobody asked for. It
            // carries no id because it expects none: an event is a word of a
            // conversation on its way to whoever is watching, and a device that
            // has stopped watching is a fact this side already holds.
            if answer.get("method").and_then(Value::as_str) == Some(sync_memory::SESSION_EVENT) {
                let said = answer.get("params").unwrap_or(&Value::Null);
                if let Some(subscription) = said.get("subscription").and_then(Value::as_u64) {
                    subscriptions.deliver(subscription, said.get("event").unwrap_or(&Value::Null));
                }
                continue;
            }
            let Some(id) = answer.get("id").and_then(Value::as_u64) else {
                continue;
            };
            application.answered(id, outcome(&answer));
        }
        Ok(())
    }
    .await;

    application.withdrew();
    writer.abort();
    reading
}

/// One message off the channel, or the reason there is not one.
pub(crate) enum Frame {
    Line(String),
    /// Longer than the channel allows. Said as soon as the ceiling is passed
    /// rather than when the message finally ends, because a caller that never
    /// sends a newline is exactly the caller this exists for — waiting for the
    /// end of its message to refuse it is waiting for something that is not
    /// coming.
    TooLong,
    End,
}

/// Messages off a stream, none of them longer than `limit`.
///
/// Written by hand rather than with `lines()`, and that is the whole of the
/// change: `lines()` grows its buffer until a newline arrives, so a client that
/// never sends one is a client deciding how much of this process's memory it
/// would like. Here the buffer stops at `limit`, everything past it is consumed
/// and dropped, and the cost of a caller sending a gigabyte is the time to read
/// a gigabyte and nothing else.
pub(crate) struct Frames<R> {
    reader: R,
    limit: usize,
    /// How long a message may take to arrive before the caller is treated as
    /// gone. `None` on this machine's own socket, where the other end is the
    /// window and a connection costs a file descriptor; set on the network
    /// door, where it is what closes a phone whose screen went off.
    patience: Option<std::time::Duration>,
    /// Set after a message was refused for its length: the rest of it is still
    /// on the stream, and reading it as the next message would answer a caller
    /// with a refusal for something it never sent.
    skipping: bool,
}

impl<R: AsyncBufRead + Unpin> Frames<R> {
    pub(crate) const fn new(reader: R, limit: usize) -> Self {
        Self {
            reader,
            limit,
            patience: None,
            skipping: false,
        }
    }

    /// Give up on a caller that has said nothing for this long.
    #[must_use]
    pub(crate) const fn patience(mut self, waiting: std::time::Duration) -> Self {
        self.patience = Some(waiting);
        self
    }

    /// Read the next message, or say why there is not one.
    ///
    /// A silence longer than this reader's patience ends the connection, which
    /// is the only sense in which it can end: there is nobody to answer, and a
    /// task waiting on somebody who left is the cost being avoided.
    pub(crate) async fn next(&mut self) -> io::Result<Frame> {
        let Some(patience) = self.patience else {
            return self.arriving().await;
        };
        match tokio::time::timeout(patience, self.arriving()).await {
            Ok(frame) => frame,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "nothing arrived on this connection for {} seconds",
                    patience.as_secs()
                ),
            )),
        }
    }

    async fn arriving(&mut self) -> io::Result<Frame> {
        if self.skipping && !self.skip_to_end_of_message().await? {
            return Ok(Frame::End);
        }

        let mut line: Vec<u8> = Vec::new();
        loop {
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(if line.is_empty() {
                    Frame::End
                } else {
                    // A last message with no newline after it. It is under the
                    // ceiling and complete, so it is answered; that the stream
                    // ended is the next read's news.
                    Frame::Line(String::from_utf8_lossy(&line).into_owned())
                });
            }

            if let Some(at) = available.iter().position(|byte| *byte == b'\n') {
                let whole = line.len() + at <= self.limit;
                if whole {
                    line.extend_from_slice(&available[..at]);
                }
                self.reader.consume(at + 1);
                return Ok(if whole {
                    Frame::Line(String::from_utf8_lossy(&line).into_owned())
                } else {
                    Frame::TooLong
                });
            }

            let read = available.len();
            if line.len() + read > self.limit {
                // Past the ceiling with no end in sight. What was held is
                // dropped here rather than at the newline — holding it would be
                // paying the memory this exists to refuse — and the rest of the
                // message is skipped on the next read.
                self.reader.consume(read);
                self.skipping = true;
                return Ok(Frame::TooLong);
            }
            line.extend_from_slice(available);
            self.reader.consume(read);
        }
    }

    /// Throw away what is left of a refused message. False at the end of the
    /// stream, which is a caller that sent one enormous thing and left.
    async fn skip_to_end_of_message(&mut self) -> io::Result<bool> {
        loop {
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(false);
            }
            if let Some(at) = available.iter().position(|byte| *byte == b'\n') {
                self.reader.consume(at + 1);
                self.skipping = false;
                return Ok(true);
            }
            let read = available.len();
            self.reader.consume(read);
        }
    }
}

/// What one answer on the channel back says: a result, or a refusal with the
/// application's own `kind` on it.
///
/// A response carrying neither is a refusal too. The alternative is answering
/// `null` as though the tool returned nothing, and a tool that returned nothing
/// is a thing that happens — so the two must not read alike.
fn outcome(answer: &Value) -> sync_memory::Result<Value> {
    if let Some(result) = answer.get("result") {
        return Ok(result.clone());
    }
    let error = answer.get("error");
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Sync answered with neither a result nor a reason");
    let kind = error
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("extension_failed");
    Err(MemoryError::domain(kind, message, Value::Null))
}

pub(crate) async fn write<W: AsyncWrite + Unpin>(stream: &mut W, answer: &Value) -> io::Result<()> {
    stream.write_all(format!("{answer}\n").as_bytes()).await?;
    stream.flush().await
}

/// There is no id to answer with, so it is `null`, which is what JSON-RPC says
/// to do.
fn malformed() -> Value {
    json!({"jsonrpc": "2.0", "id": Value::Null, "error": {
        "code": -32700,
        "message": "the request could not be read as JSON",
    }})
}

/// There is no id to answer with, for the same reason a malformed line has
/// none: nothing was read far enough to carry one.
fn too_long() -> Value {
    json!({"jsonrpc": "2.0", "id": Value::Null, "error": {
        "code": -32600,
        "message": format!(
            "the message was longer than the {MAX_FRAME_BYTES} bytes this channel reads"
        ),
    }})
}

pub(crate) fn refusal(id: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": -32000,
        "message": message,
        "data": {"kind": Value::Null},
    }})
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use tokio::net::UnixStream;

    use crate::projects::Projects;

    use super::*;
    use sync_memory::EXTENSION_FETCH;

    /// The whole of the inversion, on a socket pair: a request goes out on the
    /// connection that attended, and the answer written back reaches whoever
    /// asked.
    ///
    /// On a pair rather than through a bound socket because what is being
    /// tested is the direction of the messages, not the door — [`bind`] has its
    /// own tests and this would be waiting for a listener to prove something
    /// about a line.
    #[tokio::test]
    async fn a_connection_that_attended_carries_a_call_and_brings_the_answer() {
        let (engine, application) = UnixStream::pair().expect("a pair of connections");
        let held = Arc::new(Application::new());
        let (reading, writing) = engine.into_split();
        let attending = Arc::clone(&held);
        tokio::spawn(async move {
            let (queue, queued) = tokio::sync::mpsc::unbounded_channel::<String>();
            let writer = tokio::spawn(draining(writing, queued));
            let _ = answer_calls(
                &attending,
                &Arc::new(Subscriptions::default()),
                Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
                queue,
                writer,
            )
            .await;
        });

        let (theirs, mut ours) = application.into_split();
        let mut arriving = BufReader::new(theirs).lines();

        let asking = Arc::clone(&held);
        let call =
            tokio::spawn(async move { asking.call("extension.tool", json!({"tool": "s"})).await });

        let request: Value = serde_json::from_str(
            &arriving
                .next_line()
                .await
                .expect("the line is readable")
                .expect("a request arrived"),
        )
        .expect("it is JSON");
        assert_eq!(request["method"], "extension.tool");
        assert_eq!(request["params"]["tool"], "s");

        let answer = json!({"jsonrpc": "2.0", "id": request["id"], "result": {"found": 1}});
        ours.write_all(format!("{answer}\n").as_bytes())
            .await
            .expect("the application answers");

        let answered = call
            .await
            .expect("the call finished")
            .expect("it was answered");
        assert_eq!(answered, json!({"found": 1}));
    }

    /// The same dispatcher, over a stream that is no kind of socket.
    ///
    /// `tokio::io::duplex` is a pipe in memory: it shares nothing with a Unix
    /// socket except the two traits, which is what makes it evidence rather
    /// than a second copy of the socket test. A channel that answers over it is
    /// a channel with no opinion about how the bytes arrived.
    #[tokio::test]
    async fn the_channel_answers_over_a_stream_that_is_no_kind_of_socket() {
        let (window, engine) = tokio::io::duplex(4096);
        let host = Arc::new(Host::over(Arc::new(Projects::over(Vec::new(), None)), None));
        let application = Arc::new(Application::new());
        tokio::spawn(async move {
            let _ = attend(
                &host,
                &application,
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                engine,
                &Naming { by_path: true },
            )
            .await;
        });

        let (reading, mut writing) = tokio::io::split(window);
        let mut answers = BufReader::new(reading).lines();

        let attach = json!({
            "jsonrpc": "2.0", "id": 1, "method": ATTACH,
            "params": {"path": "/tmp/a-project"},
        });
        writing
            .write_all(format!("{attach}\n").as_bytes())
            .await
            .expect("the window asked");
        let answer: Value = serde_json::from_str(
            &answers
                .next_line()
                .await
                .expect("the line is readable")
                .expect("an answer arrived"),
        )
        .expect("it is JSON");
        assert_eq!(answer["result"]["attached"], "/tmp/a-project");

        // Past the attach and into the surface itself. `methods.list` is
        // answered before any project is opened, so an answer to it is the
        // dispatcher having been reached rather than this loop having replied
        // for it — and it needs no repository on disk to prove that.
        let methods = json!({
            "jsonrpc": "2.0", "id": 2, "method": sync_memory::METHODS, "params": {},
        });
        writing
            .write_all(format!("{methods}\n").as_bytes())
            .await
            .expect("the window asked again");
        let answer: Value = serde_json::from_str(
            &answers
                .next_line()
                .await
                .expect("the line is readable")
                .expect("an answer arrived"),
        )
        .expect("it is JSON");
        let named = answer["result"]["methods"]
            .as_array()
            .expect("the surface lists what it answers");
        assert!(
            named.iter().any(|method| method == sync_memory::METHODS),
            "the list leaves itself out: {answer}"
        );
        assert_eq!(answer["result"]["channel"], sync_memory::CHANNEL_VERSION);
    }

    /// A refusal from the application arrives as a refusal, carrying its own
    /// `kind` rather than being flattened into "something went wrong".
    #[tokio::test]
    async fn a_refusal_from_the_application_stays_a_refusal() {
        let (engine, application) = UnixStream::pair().expect("a pair of connections");
        let held = Arc::new(Application::new());
        let (reading, writing) = engine.into_split();
        let attending = Arc::clone(&held);
        tokio::spawn(async move {
            let (queue, queued) = tokio::sync::mpsc::unbounded_channel::<String>();
            let writer = tokio::spawn(draining(writing, queued));
            let _ = answer_calls(
                &attending,
                &Arc::new(Subscriptions::default()),
                Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
                queue,
                writer,
            )
            .await;
        });

        let (theirs, mut ours) = application.into_split();
        let mut arriving = BufReader::new(theirs).lines();
        let asking = Arc::clone(&held);
        let call = tokio::spawn(async move { asking.call("extension.tool", json!({})).await });

        let request: Value = serde_json::from_str(
            &arriving
                .next_line()
                .await
                .expect("the line is readable")
                .expect("a request arrived"),
        )
        .expect("it is JSON");
        let answer = json!({"jsonrpc": "2.0", "id": request["id"], "error": {
            "code": -32000,
            "message": "`acme.tracker` is not installed on this machine",
            "data": {"kind": "extension_failed"},
        }});
        ours.write_all(format!("{answer}\n").as_bytes())
            .await
            .expect("the application refuses");

        let refused = call
            .await
            .expect("the call finished")
            .expect_err("it was refused");
        assert_eq!(
            refused.kind().map(sync_memory::MemoryErrorKind::as_wire),
            Some("extension_failed")
        );
        assert!(
            refused
                .to_string()
                .contains("not installed on this machine"),
            "the application's own words reach the agent: {refused}"
        );
    }

    /// An answer with neither result nor error is a refusal, and specifically
    /// not `null`.
    ///
    /// A tool that answered with nothing is an ordinary tool, so the two must
    /// not read alike — otherwise a broken answer looks like a working one that
    /// had nothing to say.
    #[test]
    fn an_answer_that_says_nothing_at_all_is_not_an_answer() {
        let refused =
            outcome(&json!({"jsonrpc": "2.0", "id": 1})).expect_err("that is not an answer");
        assert!(
            refused
                .to_string()
                .contains("neither a result nor a reason")
        );

        assert_eq!(
            outcome(&json!({"jsonrpc": "2.0", "id": 1, "result": Value::Null}))
                .expect("a tool may answer with nothing"),
            Value::Null
        );
    }

    /// The ceiling, read from both sides of it: what fits is answered, what
    /// does not is refused the moment it passes, and the message after a
    /// refused one is read as itself rather than as the tail of the last.
    #[tokio::test]
    async fn a_message_past_the_ceiling_is_refused_and_the_next_one_still_reads() {
        let (mut writing, reading) = tokio::io::duplex(64);
        // Small enough to be quick, and it is the same number the door passes
        // in: a limit that only exists at one size is a limit tested once.
        let mut frames = Frames::new(BufReader::new(reading), 8);

        tokio::spawn(async move {
            writing
                .write_all(b"short\nfar too long to be allowed through\nafter\n")
                .await
                .expect("the caller wrote");
        });

        assert!(
            matches!(frames.next().await.expect("a frame"), Frame::Line(line) if line == "short")
        );
        assert!(matches!(
            frames.next().await.expect("a frame"),
            Frame::TooLong
        ));
        assert!(
            matches!(frames.next().await.expect("a frame"), Frame::Line(line) if line == "after"),
            "the tail of the refused message was read as the next one"
        );
    }

    /// The case the ceiling exists for: bytes with no newline anywhere in them.
    ///
    /// The refusal has to come while the caller is still sending, because
    /// waiting for the end of the message is waiting for the thing that is not
    /// going to happen. Nothing here holds more than the buffer.
    #[tokio::test]
    async fn bytes_with_no_end_are_refused_while_they_are_still_arriving() {
        let (mut writing, reading) = tokio::io::duplex(64);
        let mut frames = Frames::new(BufReader::new(reading), 8);
        let sending = tokio::spawn(async move {
            // Far past the ceiling and never terminated. The write ends when
            // the reader stops taking it, which is the point.
            for _ in 0..64 {
                if writing.write_all(&[b'a'; 64]).await.is_err() {
                    return;
                }
            }
        });

        assert!(matches!(
            frames.next().await.expect("a frame"),
            Frame::TooLong
        ));
        sending.abort();
    }

    /// And the same thing on the door itself, at the size the door uses: a
    /// caller gets a sentence back rather than a silence, and the connection is
    /// still there afterwards.
    #[tokio::test]
    async fn the_door_answers_an_oversized_message_with_a_refusal() {
        let (window, engine) = tokio::io::duplex(64 * 1024);
        let host = Arc::new(Host::over(Arc::new(Projects::over(Vec::new(), None)), None));
        let application = Arc::new(Application::new());
        tokio::spawn(async move {
            let _ = attend(
                &host,
                &application,
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                engine,
                &Naming { by_path: true },
            )
            .await;
        });

        let (reading, mut writing) = tokio::io::split(window);
        let mut answers = BufReader::new(reading).lines();
        let sending = tokio::spawn(async move {
            let block = vec![b'a'; 64 * 1024];
            for _ in 0..=(MAX_FRAME_BYTES / block.len()) {
                if writing.write_all(&block).await.is_err() {
                    return;
                }
            }
        });

        let answer: Value = serde_json::from_str(
            &answers
                .next_line()
                .await
                .expect("the line is readable")
                .expect("a refusal arrived"),
        )
        .expect("it is JSON");
        assert_eq!(answer["error"]["code"], -32600);
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a message")
                .contains("longer than"),
            "the refusal says what was wrong: {answer}"
        );
        sending.abort();
    }

    /// A connection that may not name a path, which is what anything reaching
    /// this process from off the machine will be.
    ///
    /// The registry is given one project so that the refusals below are refusals
    /// about *naming* rather than about an empty machine.
    /// A door that holds one project, for the two translations below.
    fn holding_a_project() -> Arc<Host> {
        Arc::new(Host::over(
            Arc::new(Projects::over(
                vec![crate::projects::Registered {
                    path: PathBuf::from("/w/a"),
                    name: "A".to_owned(),
                    identifier: "A".to_owned(),
                }],
                None,
            )),
            None,
        ))
    }

    /// A device names its project by key and the application is handed a path.
    ///
    /// Replaced rather than spent, which is where this parts company with an
    /// operation: the engine's surface is given a project and would choke on a
    /// second name for one, and the application is given nothing and has to be
    /// told where to raise the agent.
    #[test]
    fn a_key_becomes_the_path_the_agent_will_be_raised_in() {
        let mut params = json!({"project": "A", "agentId": "claude"});

        let key = a_key_into_a_path(&holding_a_project(), &mut params).expect("a known project");

        assert_eq!(key, "A");
        assert_eq!(params["project"], "/w/a");
        assert_eq!(
            params["agentId"], "claude",
            "the rest of the call is untouched"
        );
    }

    /// A key the registry does not hold is refused by name, and nothing is
    /// derived from it.
    #[test]
    fn a_key_this_machine_does_not_hold_is_refused_rather_than_guessed() {
        let mut params = json!({"project": "B"});

        let refused =
            a_key_into_a_path(&holding_a_project(), &mut params).expect_err("no such project");

        assert!(refused.contains('B'), "{refused}");
        assert_eq!(params["project"], "B", "nothing was put in its place");
    }

    /// A call of the family that names none says so, rather than being answered
    /// about whatever the connection last looked at.
    #[test]
    fn a_call_that_names_no_project_is_told_to_name_one() {
        let mut params = json!({"agentId": "claude"});

        let refused =
            a_key_into_a_path(&holding_a_project(), &mut params).expect_err("it names nothing");

        assert!(refused.contains(PROJECTS), "{refused}");
    }

    /// A live row goes back to a device naming its project the way the device
    /// named it, and saying nothing about anybody's disk.
    #[test]
    fn a_live_row_is_answered_in_the_keys_the_device_asked_in() {
        let answered = named_by_key(
            &holding_a_project(),
            json!([{
                "key": "s0",
                "project": "/w/a",
                "cwd": "/w/a",
                "agentName": "Claude",
            }]),
            None,
        );

        assert_eq!(answered[0]["project"], "A");
        assert_eq!(answered[0]["cwd"], "A");
        assert_eq!(answered[0]["agentName"], "Claude");
    }

    /// A conversation held in a working tree is answered as the project's, and
    /// the tree does not travel at all.
    ///
    /// Both halves matter. The tree is a directory a device cannot name, open
    /// or throw away, so sending it is the layout of somebody's disk in
    /// exchange for nothing — and a `cwd` left as that directory would be the
    /// same disclosure through the other member.
    #[test]
    fn a_conversation_in_a_working_tree_is_answered_as_the_project_s() {
        let answered = named_by_key(
            &holding_a_project(),
            json!([{
                "project": "/w/a",
                "cwd": "/w/trees/a-3f9",
                "worktree": {"path": "/w/trees/a-3f9", "branch": "sync/a-3f9"},
            }]),
            None,
        );

        assert_eq!(answered[0]["project"], "A");
        assert_eq!(answered[0]["cwd"], "A");
        assert!(
            answered[0].get("worktree").is_none(),
            "the tree reached the device: {answered}"
        );
    }

    /// A dormant pointer carries a directory and nothing to say whose it is, so
    /// it is answered with the key the call named.
    ///
    /// The third of the three rules, and the only one the answer itself cannot
    /// supply: the list was asked for by key, and that key is what these rows
    /// are.
    #[test]
    fn a_pointer_with_no_project_on_it_is_named_by_the_call_that_asked() {
        let answered = named_by_key(
            &holding_a_project(),
            json!([{"acpSession": "abc", "cwd": "/w/trees/a-3f9", "title": "A talk"}]),
            Some("A"),
        );

        assert_eq!(answered[0]["cwd"], "A");
        assert_eq!(answered[0]["title"], "A talk");
    }

    /// Nothing else in an answer is touched.
    ///
    /// Said out loud because the walk is recursive and blunt by design: it
    /// visits every object in the answer, and a rule that reached further than
    /// these three members would be quietly editing an agent's own words.
    #[test]
    fn the_rest_of_an_answer_crosses_unchanged() {
        let said = json!({
            "events": [{"kind": "update", "payload": {"text": "the path /w/a is in the prose"}}],
            "dropped": 2,
        });

        assert_eq!(named_by_key(&holding_a_project(), said.clone(), None), said);
    }

    fn from_elsewhere() -> (tokio::io::DuplexStream, Arc<Host>) {
        let registered = vec![crate::projects::Registered {
            path: PathBuf::from("/w/a"),
            name: "A".to_owned(),
            identifier: "A".to_owned(),
        }];
        let host = Arc::new(Host::over(Arc::new(Projects::over(registered, None)), None));
        let (window, engine) = tokio::io::duplex(4096);
        let serving = Arc::clone(&host);
        let application = Arc::new(Application::new());
        tokio::spawn(async move {
            let _ = attend(
                &serving,
                &application,
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                engine,
                &Naming { by_path: false },
            )
            .await;
        });
        (window, host)
    }

    /// Ask one thing and read the one answer to it.
    async fn ask(stream: &mut tokio::io::DuplexStream, request: &Value) -> Value {
        stream
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("the caller wrote");
        let mut byte = [0_u8; 1];
        let mut line = Vec::new();
        loop {
            let read = tokio::io::AsyncReadExt::read(stream, &mut byte)
                .await
                .expect("the answer is readable");
            if read == 0 || byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        serde_json::from_slice(&line).expect("the answer is JSON")
    }

    /// The first move a client with no file system in front of it can make.
    /// Refusing it for naming no project would leave such a client unable to
    /// find out what there is to name.
    #[tokio::test]
    async fn a_connection_that_has_named_no_project_can_still_be_told_what_there_is() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({"jsonrpc": "2.0", "id": 1, "method": PROJECTS, "params": {}}),
        )
        .await;
        assert_eq!(answer["result"]["projects"][0]["project"], "A");
    }

    /// A key and a name are what such a caller chooses between, and all it
    /// could act on: it names the project by key in every call, and there is no
    /// operation on this door it could hand a path to.
    ///
    /// So the path is not sent. It was, and nothing leaked — it was given away
    /// for nothing, which is the version of this that never turns up as a
    /// defect: the layout of somebody's disk on a phone that has no use for a
    /// word of it.
    #[tokio::test]
    async fn a_connection_from_elsewhere_is_not_told_where_a_project_is() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({"jsonrpc": "2.0", "id": 1, "method": PROJECTS, "params": {}}),
        )
        .await;
        let listed = &answer["result"]["projects"][0];
        assert_eq!(listed["project"], "A");
        assert_eq!(listed["name"], "A");
        assert_eq!(
            listed.get("path"),
            None,
            "the door sent where the project is: {listed}"
        );
    }

    /// The whole of what this connection is not allowed to do. A path from
    /// somewhere else is the right to name any directory on this machine, and
    /// the refusal says what to send instead rather than going quiet.
    #[tokio::test]
    async fn a_connection_from_elsewhere_cannot_name_a_directory() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": ATTACH,
                "params": {"path": "/etc"},
            }),
        )
        .await;
        let message = answer["error"]["message"]
            .as_str()
            .expect("a refusal in words");
        assert!(
            message.contains("names a registered project in each call"),
            "the refusal says how to name one instead: {message}"
        );
        assert!(
            !message.contains("/etc"),
            "the refusal repeats the path back: {message}"
        );
    }

    /// A key this machine has not registered is refused by name. Nothing is
    /// opened, nothing is looked for on disk, and the caller is told which name
    /// failed rather than that something went wrong.
    #[tokio::test]
    async fn a_key_the_registry_does_not_hold_is_refused_by_name() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "types.list",
                "params": {"project": "SOMEWHERE-ELSE"},
            }),
        )
        .await;
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a refusal in words")
                .contains("no project called `SOMEWHERE-ELSE`"),
            "{answer}"
        );
    }

    /// And a call that names no project at all is told how to, rather than
    /// being answered out of whichever project was asked about last.
    #[tokio::test]
    async fn a_call_from_elsewhere_that_names_no_project_is_told_to() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "types.list", "params": {}}),
        )
        .await;
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a refusal in words")
                .contains("every call on this connection names its project"),
            "{answer}"
        );
    }

    /// A socket file left by a process that is gone is what a crash leaves, and
    /// refusing to start over it would mean a crash costs somebody a restart of
    /// their machine's memory rather than of the application.
    #[test]
    fn a_socket_left_by_a_dead_process_is_taken_over() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");
        std::fs::write(&path, b"not a socket").expect("a file in the way");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        assert!(bind(&path).is_ok(), "a stale socket was not cleared");
    }

    /// Two copies of Sync on one machine is a different matter: taking the door
    /// would leave the first one's window talking to nothing, with nothing
    /// anywhere saying why.
    #[test]
    fn a_socket_another_process_is_serving_is_not_stolen() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the first process took it");

        let error = bind(&path).expect_err("the second process took a door in use");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        drop(held);
    }

    /// Whoever holds the door says who they are, because the next Sync has to
    /// be able to take it from a process that outlived the application that
    /// started it.
    #[test]
    fn the_door_names_the_process_holding_it() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the door opened");

        let written = std::fs::read_to_string(pid_file(&path)).expect("a pid beside the socket");
        assert_eq!(
            written.trim().parse::<u32>().expect("a number"),
            std::process::id()
        );
        drop(held);
    }

    #[test]
    fn the_door_is_readable_by_nobody_else() {
        let directory = tempfile::tempdir().expect("a directory to bind in");
        let path = directory.path().join("host.sock");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let _guard = runtime.enter();
        let held = bind(&path).expect("the door opened");

        let mode = std::fs::metadata(&path)
            .expect("the socket is there")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the permissions are this door's whole access control"
        );
        drop(held);
    }

    /// The connection a device gets asks and is answered, and that is all.
    ///
    /// Turning it around would put whoever is on the other end in the place the
    /// application occupies — answering, in its name, every tool call every
    /// agent on this machine makes. A paired device is trusted to read and
    /// write somebody's memory; it is not the application.
    #[tokio::test]
    async fn a_connection_from_elsewhere_cannot_turn_itself_around() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({"jsonrpc": "2.0", "id": 1, "method": ATTEND, "params": {}}),
        )
        .await;
        assert!(
            answer["result"].is_null(),
            "the channel was turned around: {answer}"
        );
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a refusal in words")
                .contains("belongs to the application"),
            "{answer}"
        );
        // Still answering afterwards, which is the other half of it being a
        // refusal rather than the connection being taken over.
        let after = ask(
            &mut window,
            &json!({"jsonrpc": "2.0", "id": 2, "method": PROJECTS, "params": {}}),
        )
        .await;
        assert_eq!(after["result"]["projects"][0]["project"], "A");
    }

    /// A device cannot say who is admitted. If it could, the first thing a
    /// stolen device would do is add itself under a second secret, and revoking
    /// the one in the list would revoke nothing.
    #[tokio::test]
    async fn a_connection_from_elsewhere_cannot_state_the_devices() {
        let (mut window, _host) = from_elsewhere();
        let answer = ask(
            &mut window,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": REMOTE_DEVICES,
                "params": {"devices": [{"fingerprint": "f0", "secret": "one it minted"}]},
            }),
        )
        .await;
        assert!(
            answer["error"]["message"]
                .as_str()
                .expect("a refusal in words")
                .contains("stated by the application"),
            "{answer}"
        );
    }

    /// And on the socket in this machine's own directory it is answered, with
    /// the count the application compares against what it believes it sent.
    #[tokio::test]
    async fn the_application_states_the_devices_and_is_told_what_was_taken() {
        let host = Arc::new(Host::over(Arc::new(Projects::over(Vec::new(), None)), None));
        let devices = Arc::new(Devices::default());
        let (mut window, engine) = tokio::io::duplex(4096);
        let serving = Arc::clone(&host);
        let held = Arc::clone(&devices);
        let application = Arc::new(Application::new());
        tokio::spawn(async move {
            let _ = attend(
                &serving,
                &application,
                &held,
                &Arc::new(Subscriptions::default()),
                engine,
                &Naming { by_path: true },
            )
            .await;
        });

        let answer = ask(
            &mut window,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": REMOTE_DEVICES,
                "params": {"devices": [
                    {"fingerprint": "f0", "secret": "a phone"},
                    {"fingerprint": "f1", "secret": "a tablet"},
                ]},
            }),
        )
        .await;
        assert_eq!(
            answer["result"]["devices"].as_array().map(Vec::len),
            Some(2)
        );
        // No door open in a test, so there is no name to answer with — and the
        // application shows the absence rather than an address that would not
        // work.
        assert!(answer["result"]["endpoint"].is_null());

        // Stated again with one of them gone, which is the whole of revoking.
        let answer = ask(
            &mut window,
            &json!({
                "jsonrpc": "2.0", "id": 2, "method": REMOTE_DEVICES,
                "params": {"devices": [{"fingerprint": "f0", "secret": "a phone"}]},
            }),
        )
        .await;
        assert_eq!(
            answer["result"]["devices"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(answer["result"]["devices"][0]["fingerprint"], "f0");
    }

    /// One operation that takes its time, and one that does not.
    ///
    /// Registered on the surface rather than borrowed from the real ones,
    /// because what is being measured is the transport: a real operation would
    /// bring a repository, a model and a reason for the timing to be anything
    /// other than what this test set.
    struct Slow;

    impl crate::host::Operation for Slow {
        fn name(&self) -> &'static str {
            "test.slow"
        }

        // No corpus, so nothing here opens a repository that is not there.
        fn needs_memory(&self) -> bool {
            false
        }

        fn run(
            &self,
            _domain: &mut crate::domain::Domain,
            _params: &Value,
        ) -> sync_memory::Result<Value> {
            std::thread::sleep(std::time::Duration::from_millis(300));
            Ok(json!({"slow": true}))
        }
    }

    struct Fast;

    impl crate::host::Operation for Fast {
        fn name(&self) -> &'static str {
            "test.fast"
        }

        fn needs_memory(&self) -> bool {
            false
        }

        fn run(
            &self,
            _domain: &mut crate::domain::Domain,
            _params: &Value,
        ) -> sync_memory::Result<Value> {
            Ok(json!({"fast": true}))
        }
    }

    /// How many calls were running at the same moment, at the most.
    struct Counting(Arc<Watching>);

    #[derive(Default)]
    struct Watching {
        now: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
    }

    impl crate::host::Operation for Counting {
        fn name(&self) -> &'static str {
            "test.counted"
        }

        fn needs_memory(&self) -> bool {
            false
        }

        fn run(
            &self,
            _domain: &mut crate::domain::Domain,
            _params: &Value,
        ) -> sync_memory::Result<Value> {
            use std::sync::atomic::Ordering;
            let now = self.0.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.0.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(60));
            self.0.now.fetch_sub(1, Ordering::SeqCst);
            Ok(json!({}))
        }
    }

    /// A machine holding `count` projects, named `P0`, `P1` and so on.
    fn holding_projects(count: usize) -> Vec<crate::projects::Registered> {
        (0..count)
            .map(|at| crate::projects::Registered {
                path: PathBuf::from(format!("/w/{at}")),
                name: format!("P{at}"),
                identifier: format!("P{at}"),
            })
            .collect()
    }

    /// Serve a surface on a connection that names its projects by key.
    fn serving(host: Host) -> tokio::io::DuplexStream {
        let host = Arc::new(host);
        let (caller, engine) = tokio::io::duplex(64 * 1024);
        let application = Arc::new(Application::new());
        tokio::spawn(async move {
            let _ = attend(
                &host,
                &application,
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                engine,
                &Naming { by_path: false },
            )
            .await;
        });
        caller
    }

    async fn send(stream: &mut tokio::io::DuplexStream, request: &Value) {
        stream
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("the caller wrote");
    }

    /// Read one whole line, byte at a time, so nothing of the next answer is
    /// swallowed into a buffer this test would then have to remember about.
    async fn next_answer(stream: &mut tokio::io::DuplexStream) -> Value {
        let mut byte = [0_u8; 1];
        let mut line = Vec::new();
        loop {
            let read = tokio::io::AsyncReadExt::read(stream, &mut byte)
                .await
                .expect("the answer is readable");
            if read == 0 || byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        serde_json::from_slice(&line).expect("the answer is JSON")
    }

    /// Two calls sent one after the other, and the second does not wait for the
    /// first.
    ///
    /// Two projects rather than one, and that is the point rather than a
    /// convenience: a project's memory is behind one mutex, so two calls about
    /// the *same* project are still done one at a time — which is the ordering
    /// this change was not allowed to loosen. What it loosened is the
    /// transport, and two projects is where that becomes visible.
    #[tokio::test]
    async fn a_slow_call_does_not_hold_up_the_one_sent_after_it() {
        let mut host = Host::over(Arc::new(Projects::over(holding_projects(2), None)), None);
        host.register(Box::new(Slow));
        host.register(Box::new(Fast));
        let mut caller = serving(host);

        send(
            &mut caller,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "test.slow", "params": {"project": "P0"}}),
        )
        .await;
        send(
            &mut caller,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "test.fast", "params": {"project": "P1"}}),
        )
        .await;

        let first = next_answer(&mut caller).await;
        let second = next_answer(&mut caller).await;
        assert_eq!(first["id"], 2, "the quick call answered first: {first}");
        assert_eq!(first["result"]["fast"], true);
        assert_eq!(second["id"], 1, "and the slow one after it: {second}");
        assert_eq!(second["result"]["slow"], true);
    }

    /// The number of calls one connection may have running is a number, and it
    /// is this one.
    ///
    /// Without it a caller decides how many of this process's blocking threads
    /// it spends, which is a caller deciding how much of the machine it gets.
    #[tokio::test]
    async fn a_connection_runs_no_more_than_the_stated_number_of_calls_at_once() {
        let watching = Arc::new(Watching::default());
        let asked = AT_ONCE * 2;
        let mut host = Host::over(
            Arc::new(Projects::over(holding_projects(asked), None)),
            None,
        );
        host.register(Box::new(Counting(Arc::clone(&watching))));
        let mut caller = serving(host);

        // One project each, so that nothing but the ceiling is holding them
        // back.
        for at in 0..asked {
            send(
                &mut caller,
                &json!({
                    "jsonrpc": "2.0", "id": at, "method": "test.counted",
                    "params": {"project": format!("P{at}")},
                }),
            )
            .await;
        }
        let mut answered = 0;
        while answered < asked {
            let answer = next_answer(&mut caller).await;
            assert!(answer["error"].is_null(), "{answer}");
            answered += 1;
        }

        let most = watching.most.load(std::sync::atomic::Ordering::SeqCst);
        assert!(most > 1, "nothing ran at the same time as anything: {most}");
        assert!(
            most <= AT_ONCE,
            "{most} ran at once, and the ceiling is {AT_ONCE}"
        );
    }

    /// A request a package makes is carried to the application whole, and the
    /// engine adds nothing to it and reads nothing out of it.
    ///
    /// That is the claim worth a test rather than the plumbing: what a package
    /// may reach is a sentence in a manifest on the machine the request goes out
    /// from, so a door that helpfully attached a host, a list or an id of its
    /// own would be a door deciding a permission it cannot see.
    #[tokio::test]
    async fn a_package_s_request_is_carried_to_the_application_and_not_read_on_the_way() {
        let held = Arc::new(Application::new());

        // The application's own connection, turned around.
        let (engine, application) = UnixStream::pair().expect("a pair of connections");
        let (reading, writing) = engine.into_split();
        let attending = Arc::clone(&held);
        tokio::spawn(async move {
            let (queue, queued) = tokio::sync::mpsc::unbounded_channel::<String>();
            let writer = tokio::spawn(draining(writing, queued));
            let _ = answer_calls(
                &attending,
                &Arc::new(Subscriptions::default()),
                Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
                queue,
                writer,
            )
            .await;
        });
        let (theirs, mut ours) = application.into_split();
        let mut arriving = BufReader::new(theirs).lines();

        // And a caller off this machine, on a connection of its own.
        let host = Arc::new(Host::over(Arc::new(Projects::over(Vec::new(), None)), None));
        let (mut caller, door) = tokio::io::duplex(64 * 1024);
        let serving = Arc::clone(&host);
        let carrying = Arc::clone(&held);
        tokio::spawn(async move {
            let _ = attend(
                &serving,
                &carrying,
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                door,
                &Naming { by_path: false },
            )
            .await;
        });

        let asked = json!({"url": "https://example.test/a", "method": "GET"});
        send(
            &mut caller,
            &json!({
                "jsonrpc": "2.0", "id": 7, "method": EXTENSION_FETCH,
                "params": {"id": "a-package", "request": asked},
            }),
        )
        .await;

        let carried: Value = serde_json::from_str(
            &arriving
                .next_line()
                .await
                .expect("the line is readable")
                .expect("a request arrived"),
        )
        .expect("it is JSON");
        assert_eq!(carried["method"], EXTENSION_FETCH);
        assert_eq!(carried["params"]["id"], "a-package");
        assert_eq!(carried["params"]["request"], asked);
        // Nothing else. A member the engine added would be a permission the
        // engine decided.
        assert_eq!(
            carried["params"]
                .as_object()
                .expect("an object")
                .keys()
                .collect::<Vec<_>>(),
            vec!["id", "request"]
        );

        let answer = json!({"jsonrpc": "2.0", "id": carried["id"], "result": {"status": 200}});
        ours.write_all(format!("{answer}\n").as_bytes())
            .await
            .expect("the application answers");

        let answered = next_answer(&mut caller).await;
        assert_eq!(answered["id"], 7, "the answer found the call: {answered}");
        assert_eq!(answered["result"]["status"], 200);
    }

    /// With nobody attending there is no machine to make the request from, and
    /// the caller is told so rather than left waiting on its own patience.
    #[tokio::test]
    async fn a_request_with_no_application_behind_it_is_refused_at_once() {
        let host = Arc::new(Host::over(Arc::new(Projects::over(Vec::new(), None)), None));
        let (mut caller, door) = tokio::io::duplex(64 * 1024);
        let serving = Arc::clone(&host);
        tokio::spawn(async move {
            let _ = attend(
                &serving,
                &Arc::new(Application::new()),
                &Arc::new(Devices::default()),
                &Arc::new(Subscriptions::default()),
                door,
                &Naming { by_path: false },
            )
            .await;
        });

        send(
            &mut caller,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": EXTENSION_FETCH,
                "params": {"id": "a-package", "request": {"url": "https://example.test/a", "method": "GET"}},
            }),
        )
        .await;
        let answered = next_answer(&mut caller).await;
        assert!(
            answered["error"]["message"]
                .as_str()
                .expect("a refusal in words")
                .contains("not on the other end"),
            "{answered}"
        );
    }
}
