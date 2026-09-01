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

use sync_memory::{ATTACH, ATTEND, MAX_FRAME_BYTES, MemoryError, PROJECTS};

use crate::application::Application;
use crate::host::Host;

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
    path: PathBuf,
) -> io::Result<()> {
    let listener = bind(&path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let host = Arc::clone(&host);
        let application = Arc::clone(&application);
        // Per connection, because one window has several projects open and a
        // call in one of them must not be behind a call in another.
        tokio::spawn(async move {
            // This door is a file in this machine's own application directory,
            // reachable only by something running as this user. The window is
            // on the other end of it, and it names its project by path.
            if let Err(error) = attend(&host, &application, stream, &Naming { by_path: true }).await
            {
                // A connection ending is ordinary — the window closed a project
                // — so this is only worth a line when it ended for a reason.
                if error.kind() != io::ErrorKind::UnexpectedEof {
                    eprintln!("a host connection ended: {error}");
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
    stream: S,
    naming: &Naming,
) -> io::Result<()>
where
    // `Send` and `'static` are not the transport's business either — they are
    // what the writer task below needs of anything it is handed.
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (reading, mut writing) = tokio::io::split(stream);
    let mut lines = Frames::new(BufReader::new(reading), MAX_FRAME_BYTES);
    let mut attached: Option<PathBuf> = None;

    loop {
        let line = match lines.next().await? {
            Frame::Line(line) => line,
            // Answered rather than dropped, and the connection kept: a caller
            // that sent one message too big is a caller that can send a smaller
            // one, and a door that goes silent instead teaches nothing.
            Frame::TooLong => {
                write(&mut writing, &too_long()).await?;
                continue;
            }
            Frame::End => return Ok(()),
        };
        if line.trim().is_empty() {
            continue;
        }
        // Answered, not fatal, exactly as the stdio door answers it: a line
        // that is not JSON is a caller's mistake, and ending the connection
        // over it would report the same class of mistake two different ways.
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            write(&mut writing, &malformed()).await?;
            continue;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        // The one call that turns this connection around. Everything after
        // it is answers to what *this* process asked, so the loop below never
        // sees another line — [`answer_calls`] owns the connection from here.
        if method == ATTEND {
            let answer = json!({"jsonrpc": "2.0", "id": id, "result": {"attending": true}});
            write(&mut writing, &answer).await?;
            return answer_calls(application, lines, writing).await;
        }

        if method == ATTACH {
            let answer = if naming.by_path {
                match params.get("path").and_then(Value::as_str) {
                    Some(path) => {
                        attached = Some(PathBuf::from(path));
                        json!({"jsonrpc": "2.0", "id": id, "result": {"attached": path}})
                    }
                    None => refusal(&id, "`project.attach` needs the path of a project"),
                }
            } else {
                // Refused by name rather than ignored. A client that attached
                // and was answered with silence would go on to make every call
                // it meant for that project against nothing.
                refusal(
                    &id,
                    "this connection names a registered project in each call, as `project` — there is no `project.attach` on it and no naming a directory by its path",
                )
            };
            write(&mut writing, &answer).await?;
            continue;
        }

        // Asked before a project is demanded, because these are the two
        // questions a connection has to ask first: what this surface answers,
        // and what projects there are to choose between. Refusing them for
        // naming no project would leave a client that cannot see the file
        // system with no first move.
        let project = if Host::answers_without_project(method) {
            PathBuf::new()
        } else if naming.by_path {
            let Some(project) = attached.clone() else {
                // Said rather than answered from a guess. There is no default
                // project and inventing one would answer a call meant for one
                // repository out of another.
                let answer = refusal(
                    &id,
                    "this connection has not said which project it is about — call `project.attach` first",
                );
                write(&mut writing, &answer).await?;
                continue;
            };
            project
        } else {
            // The key is resolved through the registry and nowhere else, so a
            // name this machine has not registered is a refusal by name — never
            // an attempt to open whatever is at some path derived from it.
            let Some(key) = params.get("project").and_then(Value::as_str) else {
                let answer = refusal(
                    &id,
                    format!(
                        "every call on this connection names its project — `project`, a key as `{PROJECTS}` lists them"
                    )
                    .as_str(),
                );
                write(&mut writing, &answer).await?;
                continue;
            };
            let Some(project) = host.project_named(key) else {
                let answer = refusal(
                    &id,
                    format!("this machine holds no project called `{key}`").as_str(),
                );
                write(&mut writing, &answer).await?;
                continue;
            };
            project
        };

        let host = Arc::clone(host);
        let method = method.to_owned();
        // Off the runtime: a dispatch reaches Git, LanceDB and possibly a
        // model, and the agents' door shares these threads.
        let answered =
            tokio::task::spawn_blocking(move || host.dispatch(&project, &method, &params))
                .await
                .map_err(|error| {
                    io::Error::other(format!("a host call could not be run: {error}"))
                })?;

        let answer = match answered {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            // The engine's own `kind` travels in `data`, so the window can tell
            // a stale revision from a locked project without reading prose.
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32000,
                "message": error.to_string(),
                "data": {"kind": error.kind().map(sync_memory::MemoryErrorKind::as_wire)},
            }}),
        };
        write(&mut writing, &answer).await?;
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
async fn answer_calls<R, W>(
    application: &Arc<Application>,
    mut lines: Frames<R>,
    mut writing: W,
) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (queue, mut queued) = tokio::sync::mpsc::unbounded_channel::<String>();
    application.attend(queue);
    let writer = tokio::spawn(async move {
        while let Some(request) = queued.recv().await {
            if writing
                .write_all(format!("{request}\n").as_bytes())
                .await
                .is_err()
                || writing.flush().await.is_err()
            {
                break;
            }
        }
    });

    let reading = async {
        loop {
            let line = match lines.next().await? {
                Frame::Line(line) => line,
                Frame::TooLong => {
                    // Nothing to answer to: this direction carries answers, and
                    // an answer too long to read is a call that will time out
                    // on its own patience rather than one anybody can refuse.
                    eprintln!("an answer on the channel back was longer than the channel allows");
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
                eprintln!("an answer on the channel back could not be read as JSON");
                continue;
            };
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
enum Frame {
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
struct Frames<R> {
    reader: R,
    limit: usize,
    /// Set after a message was refused for its length: the rest of it is still
    /// on the stream, and reading it as the next message would answer a caller
    /// with a refusal for something it never sent.
    skipping: bool,
}

impl<R: AsyncBufRead + Unpin> Frames<R> {
    const fn new(reader: R, limit: usize) -> Self {
        Self {
            reader,
            limit,
            skipping: false,
        }
    }

    /// Read the next message, or say why there is not one.
    async fn next(&mut self) -> io::Result<Frame> {
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

async fn write<W: AsyncWrite + Unpin>(stream: &mut W, answer: &Value) -> io::Result<()> {
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

fn refusal(id: &Value, message: &str) -> Value {
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
            let _ = answer_calls(
                &attending,
                Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
                writing,
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
            let _ = attend(&host, &application, engine, &Naming { by_path: true }).await;
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
            let _ = answer_calls(
                &attending,
                Frames::new(BufReader::new(reading), MAX_FRAME_BYTES),
                writing,
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
            let _ = attend(&host, &application, engine, &Naming { by_path: true }).await;
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
            let _ = attend(&serving, &application, engine, &Naming { by_path: false }).await;
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
        assert_eq!(answer["result"]["projects"][0]["path"], "/w/a");
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
}
