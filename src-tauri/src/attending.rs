//! Answering what the engine asks of Sync.
//!
//! Every other message between the two goes the other way: Sync asks, the
//! engine answers. These are the calls that go from there to here — a tool an
//! agent asked for, a fact about this machine's packages, and everything about
//! talking to an agent, which is here because the process is here.
//!
//! One thing travels *out* on this connection without having been asked for: a
//! word of a conversation somebody off this machine is watching, written as a
//! notification under the number the engine's door minted for that watch. It is
//! the only such message, and it carries no call — see
//! [`sync_memory::SESSION_EVENT`].
//!
//! **A tool's body runs here because everything it reaches is here.** The
//! keychain, the host list off the manifest a person installed, the artefact on
//! this machine, `work.order` — none of it exists in `sync-mcp`, and a second
//! copy there would be a second place an extension's permissions are decided.
//! So the engine decides *whether* a call may be made — the project declares
//! that extension, it offers that tool, the arguments match its schema — and
//! this runs it.
//!
//! # Why Sync connects rather than being connected to
//!
//! Sync spawns the engine and outlives it, so there is no door of ours for it
//! to knock on. Instead this takes one connection on the socket that already
//! exists, says `host.attend`, and that connection is turned around.
//!
//! It is held **from start-up**, not from the moment somebody opens a project:
//! an agent calls a tool with no window anywhere, and a channel that appeared
//! with the first window would be a product whose tools worked only while
//! somebody was looking at it.
//!
//! # It reconnects, deliberately
//!
//! Changing the port restarts the engine, and the socket goes with it. A
//! channel that gave up on the first failure would leave every extension's
//! tools dead until somebody restarted Sync, with nothing anywhere saying why —
//! so this waits and connects again, for as long as the application is running.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use sync_memory::{ATTEND, EXTENSION_FETCH, SESSION_DROPPED, TOOL_CALL, carried};
use tauri::{AppHandle, Manager as _, Runtime};

use crate::project::ProjectError;

/// The largest file this channel will carry out of an artefact.
///
/// Half of a frame rather than a fraction close to it, and the arithmetic is
/// the reason: base64 is four bytes for every three, and the answer is a JSON
/// object with a media type beside it — so four mebibytes of file is about five
/// and a third on the wire, comfortably inside a frame with room for whatever
/// else the shape grows.
///
/// **A file over this is refused by name and by size**, which is the whole
/// point of the number being here rather than left to the frame reader. A
/// package with a large asset is an ordinary thing to build, and the author of
/// one deserves a sentence naming the file rather than a connection that dies
/// on a line nobody can read. Splitting a file across frames is the answer when
/// somebody needs it; until then this says plainly that it does not happen.
const BIGGEST_FILE: usize = sync_memory::MAX_FRAME_BYTES / 2;

/// How long to wait before reaching for a door that was not there.
///
/// A second, flat, rather than a backoff that grows. The thing being waited for
/// is a child process this application starts itself — it is either coming back
/// within a few seconds or it is not coming back at all — and a delay that grew
/// would mean a restart somebody watched took a minute to be noticed.
const BETWEEN_ATTEMPTS: Duration = Duration::from_secs(1);

/// Hold the channel back for as long as this application runs.
///
/// Its own thread rather than a task, because everything it does is blocking:
/// the socket is the synchronous one `sync-memory` uses, and a tool's body is
/// a handler that may sit inside a network call.
pub fn attend<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            match crate::server::host_socket(&app) {
                // A socket that is not there yet is ordinary while the engine
                // is starting, and ordinary again for a second after somebody
                // changes the port — so it is not said out loud. A line per
                // second in a log nobody is reading is how a real one is
                // missed.
                Ok(path) => {
                    if let Ok(stream) = UnixStream::connect(&path)
                        && let Err(error) = answer(&app, stream)
                    {
                        eprintln!("the channel from the memory engine ended: {error}");
                    }
                }
                Err(error) => eprintln!("the memory engine's socket: {}", error.message),
            }
            std::thread::sleep(BETWEEN_ATTEMPTS);
        }
    });
}

/// Say what this connection is for, then answer what comes down it.
fn answer<R: Runtime>(app: &AppHandle<R>, stream: UnixStream) -> std::io::Result<()> {
    let writing = Arc::new(Mutex::new(stream.try_clone()?));
    say(
        &writing,
        &json!({"jsonrpc": "2.0", "id": 0, "method": ATTEND, "params": {}}),
    )?;

    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            eprintln!("the memory engine sent a line that is not JSON");
            continue;
        };
        // The engine's answer to `host.attend` itself, which carries no method.
        // Nothing to do with it: the channel is open by the time it arrives.
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        if method != SESSION_DROPPED && method != TOOL_CALL && !carried(method) {
            say(
                &writing,
                &refusal(&id, &format!("Sync does not answer `{method}`")),
            )?;
            continue;
        }
        let method = method.to_owned();

        // One thread per call, and the connection stays readable while it runs.
        // A tool that waits twenty seconds on somebody's API would otherwise be
        // twenty seconds in which no other call could even be read — including
        // one for a different project.
        let app = app.clone();
        let writing = Arc::clone(&writing);
        std::thread::spawn(move || {
            // Answered apart from the rest, because a conversation refuses in a
            // vocabulary the window branches on: `agent_session_load` is the cue
            // to continue from a kept transcript rather than from the agent,
            // `worktree_missing` is a tree somebody deleted, `session_unknown`
            // is a row that has already ended. Flattened into one kind they all
            // become a screen that can only apologise — which is what every
            // other call on this channel is allowed to be, and this one is not.
            let answered = if sync_memory::about_a_session(&method) {
                match about_a_conversation(&app, &writing, &method, &params) {
                    Ok(answer) => json!({"jsonrpc": "2.0", "id": id, "result": answer}),
                    Err(refused) => refused_as(&id, &refused.kind, &refused.message),
                }
            } else {
                let outcome = if method == TOOL_CALL {
                    run(&app, &params)
                } else if method == SESSION_DROPPED {
                    nobody_is_watching(&app, &params)
                } else {
                    about_a_package(&app, &method, &params)
                };
                match outcome {
                    Ok(answer) => json!({"jsonrpc": "2.0", "id": id, "result": answer}),
                    Err(why) => refusal(&id, &why),
                }
            };
            if let Err(error) = say(&writing, &answered) {
                eprintln!("an answer to the memory engine could not be sent: {error}");
            }
        });
    }
    Ok(())
}

/// Run one tool of one extension, for one project.
///
/// What the engine already established is not checked again — that the project
/// declares this extension and that the arguments fit the schema its author
/// published. What is checked here is what only this side knows: whether the
/// package is on this machine at all, and whether it really has the handler its
/// declaration named.
///
/// # Errors
///
/// In words, for an agent to read: the package is not installed here, the
/// declaration names a handler the module does not have, or the handler failed.
fn run<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    let Asked {
        project,
        id,
        tool,
        arguments,
    } = asked(params)?;
    let (project, id, tool) = (project.as_str(), id.as_str(), tool.as_str());

    let installed = crate::extensions::store(app)?
        .resolve(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "`{id}` is declared by this project and is not installed on this machine, so `{tool}` cannot be run here"
            )
        })?;

    // Resolved through the manifest, which is where every occasion is resolved:
    // an install, a clock and a tool an agent named all ask *which function is
    // this*, and asking it in three places is how one of them quietly gets a
    // different answer.
    let handler = installed.manifest.handler_for(tool).ok_or_else(|| {
        format!("`{id}` offers no tool called `{tool}` in the package installed on this machine")
    })?;

    crate::handlers::run(app, &installed, project, handler, &arguments)
}

/// Answer one of the calls the door carries here about this machine's packages.
///
/// Every one of them is what the window's own command layer does, called
/// through the same function rather than a second copy of it — so a phone and
/// this window get the same answer, and a check added to one is not a check the
/// other quietly lacks.
///
/// The name is refused rather than defaulted. A call that reached here is one
/// [`sync_memory::carried`] named, and a name that list gained without this
/// match gaining it is a defect between two halves of one product — which is
/// worth a sentence saying so rather than a silent nothing.
fn about_a_package<R: Runtime>(
    app: &AppHandle<R>,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    use sync_memory::{
        EXTENSION_FILE, EXTENSION_FORGET, EXTENSION_INSTALL, EXTENSION_LIST, EXTENSION_OCCASION,
        EXTENSION_REPOINT, REGISTRY_CACHED, REGISTRY_INDEX, REGISTRY_LEDGER, SCHEDULE_OFF,
        SCHEDULE_REMEMBER, SCHEDULE_SWITCH,
    };

    match method {
        EXTENSION_FETCH => reach(app, params),
        EXTENSION_LIST => encoded(crate::extensions::listed(app)?),
        EXTENSION_FILE => carrying_file(app, params),
        EXTENSION_INSTALL => encoded(crate::extensions::install_now(
            app,
            &read(params, "artefact")?,
        )?),
        EXTENSION_FORGET => {
            crate::extensions::forget_now(app, named(params, "id")?)?;
            Ok(Value::Null)
        }
        EXTENSION_REPOINT => encoded(crate::extensions::repoint_now(
            app,
            &read(params, "pointer")?,
        )?),
        EXTENSION_OCCASION => encoded(crate::handlers::occasion_now(
            app,
            named(params, "project")?,
            named(params, "id")?,
            named(params, "occasion")?,
            params.get("payload").unwrap_or(&Value::Null),
        )?),
        REGISTRY_INDEX => encoded(crate::extensions::index_now(app)?),
        REGISTRY_CACHED => encoded(crate::extensions::cached_now(app)?),
        REGISTRY_LEDGER => encoded(crate::extensions::ledger_now(app, named(params, "id")?)?),
        SCHEDULE_REMEMBER => {
            let guard = app.state::<crate::schedule::ScheduleFile>();
            crate::schedule::remember_now(
                app,
                &guard,
                named(params, "project")?,
                read(params, "extensions")?,
            )
            .map_err(|refusal| refusal.message)?;
            Ok(Value::Null)
        }
        SCHEDULE_OFF => {
            let guard = app.state::<crate::schedule::ScheduleFile>();
            encoded(
                crate::schedule::switched_off_now(app, &guard, named(params, "project")?)
                    .map_err(|refusal| refusal.message)?,
            )
        }
        SCHEDULE_SWITCH => {
            let guard = app.state::<crate::schedule::ScheduleFile>();
            crate::schedule::switch_now(
                app,
                &guard,
                named(params, "project")?,
                named(params, "id")?,
                params.get("on").and_then(Value::as_bool).unwrap_or(false),
            )
            .map_err(|refusal| refusal.message)?;
            Ok(Value::Null)
        }
        _ => Err(format!("Sync does not answer `{method}`")),
    }
}

/// One file of an artefact, as text a JSON line can carry.
///
/// Base64 because the protocol is JSON and a package ships pictures and fonts
/// as readily as it ships code. The media type travels with it: what a file is
/// is decided where the file is, and a phone guessing from an extension would
/// be a second answer to a question this machine already answers for its own
/// webview.
fn carrying_file<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    use base64::Engine as _;

    let id = named(params, "id")?;
    let path = named(params, "path")?;
    let (bytes, media) = crate::extensions::file_of(app, id, path)?;
    if bytes.len() > BIGGEST_FILE {
        return Err(format!(
            "`{path}` is {} bytes, and a file crossing this channel may be at most {BIGGEST_FILE}",
            bytes.len()
        ));
    }
    Ok(json!({
        "mediaType": media,
        "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
}

/// A member the call must carry, as a string.
fn named<'a>(params: &'a Value, member: &str) -> Result<&'a str, String> {
    params
        .get(member)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the call carries no `{member}`"))
}

/// A member the call must carry, read into what it stands for.
fn read<T: serde::de::DeserializeOwned>(params: &Value, member: &str) -> Result<T, String> {
    let value = params
        .get(member)
        .cloned()
        .ok_or_else(|| format!("the call carries no `{member}`"))?;
    serde_json::from_value(value).map_err(|error| format!("`{member}` is not one: {error}"))
}

/// What was answered, in the shape a JSON line carries.
fn encoded<T: serde::Serialize>(answer: T) -> Result<Value, String> {
    serde_json::to_value(answer).map_err(|error| format!("the answer could not be sent: {error}"))
}

/// Make one package's request, from this machine.
///
/// The two lines that matter are the two [`crate::extensions::extension_fetch`]
/// runs, and they are the same two on purpose: what a package may reach is read
/// off the artefact installed here, on the first request and on every redirect,
/// and the secret its manifest declared is put in the header here. A caller
/// somewhere else named neither and cannot.
///
/// What arrives is what the surface's own door takes — an id and a request —
/// so a member added to `NetRequest` is a member both routes gain at once
/// rather than one that quietly never crosses.
///
/// # Errors
///
/// In words: the request was not one, the package is not installed here, it did
/// not ask for the capability, or the request itself failed.
fn reach<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or("the memory engine asked for a request without naming an extension")?;
    let request: sync_extensions::NetRequest = params
        .get("request")
        .cloned()
        .ok_or_else(|| "the memory engine asked for a request without one".to_owned())
        .and_then(|request| {
            serde_json::from_value(request).map_err(|error| {
                format!("the memory engine asked for a request it did not describe: {error}")
            })
        })?;

    let installed = crate::extensions::permitted(app, id, sync_extensions::NET_CAPABILITY)?;
    let answered = crate::extensions::fetch_now(id, &installed.manifest, &request)?;
    serde_json::to_value(answered).map_err(|error| format!("the answer could not be sent: {error}"))
}

/// What one request off the channel is about.
#[derive(Debug)]
struct Asked {
    project: String,
    id: String,
    tool: String,
    arguments: Value,
}

/// Read one, or say which part of it was missing.
///
/// Split out from [`run`] because it is the half that can be checked without an
/// application around it, and because the sentence matters: every one of these
/// is a defect *between* the two halves of this product rather than anything a
/// package or a person did, and a message blaming the extension would send
/// somebody to read the wrong code.
///
/// Arguments are optional and default to nothing, which is what a tool that
/// takes nothing is called with.
///
/// # Errors
///
/// When the request named no project, no extension or no tool.
fn asked(params: &Value) -> Result<Asked, String> {
    let named = |what: &str| format!("the memory engine asked for a tool without naming {what}");
    let read = |member: &str| params.get(member).and_then(Value::as_str);
    Ok(Asked {
        project: read("project")
            .ok_or_else(|| named("a project"))?
            .to_owned(),
        id: read("extension")
            .ok_or_else(|| named("an extension"))?
            .to_owned(),
        tool: read("tool").ok_or_else(|| named("a tool"))?.to_owned(),
        arguments: params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({})),
    })
}

/// Write one message, whole.
///
/// Behind a lock because several tools may be answering at once and a line
/// interleaved with another is two unreadable answers rather than one late one.
fn say(stream: &Arc<Mutex<UnixStream>>, message: &Value) -> std::io::Result<()> {
    let mut stream = stream
        .lock()
        .map_err(|_| std::io::Error::other("the channel's writer is unusable"))?;
    stream.write_all(format!("{message}\n").as_bytes())?;
    stream.flush()
}

/// A refusal in the shape the engine reads.
///
/// One `kind` for everything that can go wrong on this side, because the
/// distinction that matters to whoever reads it is already in the sentence: a
/// tool that failed is a tool that failed, and what an agent does next is read
/// the words rather than branch on a code.
fn refusal(id: &Value, why: &str) -> Value {
    refused_as(id, "extension_failed", why)
}

/// The same, keeping the word whoever refused chose.
///
/// The one caller is a conversation, and it is the one caller that has such a
/// word: every kind [`crate::sessions`] refuses with is read by a screen that
/// does something different for each. Everything else on this channel is a tool
/// that failed, which is one thing however it failed.
fn refused_as(id: &Value, kind: &str, why: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": -32000,
        "message": why,
        "data": {"kind": kind},
    }})
}

/// Answer one of the calls about talking to an agent.
///
/// **Every one of them calls the very function the window's own command calls.**
/// Not a copy of it, not a narrower version: a phone and this window raise the
/// same agent, in the same directory, with the same permission questions
/// waiting in the same place — so a conversation started on one is continued on
/// the other because there is only ever one conversation. A second
/// implementation here would be a second answer to *what is running*, and the
/// two would drift on the first check somebody added to one of them.
///
/// The project arrives as a path. The engine's door resolved the key a device
/// named before this saw the call, exactly as it resolves the key on any other
/// call from elsewhere, so nothing here knows or asks which door its caller
/// came through.
///
/// # Errors
///
/// Whatever the command refused, kind and all.
fn about_a_conversation<R: Runtime>(
    app: &AppHandle<R>,
    writing: &Arc<Mutex<UnixStream>>,
    method: &str,
    params: &Value,
) -> Result<Value, ProjectError> {
    use sync_memory::{
        AGENT_ADAPTERS, AGENT_ADAPTERS_FORGET, AGENT_ADAPTERS_PREPARE, SESSION_BACKLOG,
        SESSION_CANCEL, SESSION_CATALOG, SESSION_CLOSE, SESSION_FOR_RECORD, SESSION_FORGET,
        SESSION_FORGET_REMEMBERED, SESSION_KEPT_AS, SESSION_LIVE, SESSION_OPEN,
        SESSION_PERMISSION_RESPOND, SESSION_PROMPT, SESSION_REMEMBERED, SESSION_RENAME,
        SESSION_RESUME, SESSION_SET_MODE, SESSION_SET_OPTION, SESSION_SUBSCRIBE,
        SESSION_UNSUBSCRIBE,
    };
    use tauri::async_runtime::block_on;

    let sessions = app.state::<crate::sessions::live::Sessions>();
    let text = |member: &str| -> Result<String, ProjectError> {
        params
            .get(member)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| asked_without(member))
    };
    let number = |member: &str| -> Result<u64, ProjectError> {
        params
            .get(member)
            .and_then(Value::as_u64)
            .ok_or_else(|| asked_without(member))
    };
    let maybe = |member: &str| {
        params
            .get(member)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    };
    match method {
        SESSION_CATALOG => told(crate::sessions::session_catalog()),
        SESSION_LIVE => told(crate::sessions::session_live(sessions)),
        SESSION_OPEN => told(block_on(crate::sessions::session_open(
            app.clone(),
            sessions,
            text("agentId")?,
            text("project")?,
            maybe("model"),
            part(params, "worktree")?,
            part(params, "under")?,
        ))?),
        SESSION_PROMPT => told(block_on(crate::sessions::session_prompt(
            app.clone(),
            sessions,
            text("key")?,
            text("text")?,
            part(params, "attachments")?,
            part(params, "images")?,
        ))?),
        SESSION_RESUME => told(block_on(crate::sessions::session_resume(
            app.clone(),
            sessions,
            text("project")?,
            text("acpSession")?,
        ))?),
        SESSION_REMEMBERED => told(crate::sessions::session_remembered(
            app.clone(),
            sessions,
            text("project")?,
        )?),
        SESSION_FORGET_REMEMBERED => told(crate::sessions::session_forget_remembered(
            app.clone(),
            text("project")?,
            text("acpSession")?,
        )?),
        SESSION_RENAME => told(crate::sessions::session_rename(
            app.clone(),
            sessions,
            text("key")?,
            text("title")?,
        )?),
        SESSION_CANCEL => told(crate::sessions::session_cancel(sessions, text("key")?)?),
        SESSION_CLOSE => told(block_on(crate::sessions::session_close(
            sessions,
            text("key")?,
        ))?),
        SESSION_FORGET => told(block_on(crate::sessions::session_forget(
            sessions,
            text("key")?,
        ))?),
        SESSION_KEPT_AS => told(crate::sessions::session_kept_as(
            app.clone(),
            sessions,
            text("key")?,
            text("recordKey")?,
        )?),
        SESSION_FOR_RECORD => told(crate::sessions::session_for_record(
            app.clone(),
            text("project")?,
            text("recordKey")?,
        )?),
        SESSION_SET_MODE => Ok(block_on(crate::sessions::session_set_mode(
            sessions,
            text("key")?,
            text("modeId")?,
        ))?),
        SESSION_SET_OPTION => Ok(block_on(crate::sessions::session_set_option(
            sessions,
            text("key")?,
            text("configId")?,
            text("valueId")?,
        ))?),
        SESSION_PERMISSION_RESPOND => told(crate::sessions::session_permission_respond(
            sessions,
            text("key")?,
            number("requestId")?,
            maybe("optionId"),
        )?),
        SESSION_BACKLOG => told(crate::sessions::session_backlog(sessions, text("key")?)?),
        SESSION_SUBSCRIBE => {
            let watcher: Arc<dyn crate::sessions::live::Watcher> = Arc::new(Elsewhere {
                subscription: number("subscription")?,
                writing: Arc::clone(writing),
            });
            let dropped = crate::sessions::session_watched(
                &sessions,
                &text("key")?,
                number("subscription")?,
                params.get("since").and_then(Value::as_u64),
                &watcher,
            )?;
            // The number goes back with the answer, and it is the door's rather
            // than this side's: the device that asked has to know what its
            // events will arrive under, and the alternative was a door
            // reshaping an answer it did not write.
            Ok(json!({"subscription": number("subscription")?, "dropped": dropped}))
        }
        SESSION_UNSUBSCRIBE => {
            sessions.stop_watching(number("subscription")?);
            Ok(Value::Null)
        }
        AGENT_ADAPTERS => told(crate::sessions::agent_adapters(app.clone())?),
        AGENT_ADAPTERS_PREPARE => told(block_on(crate::sessions::agent_adapters_prepare(
            app.clone(),
        ))?),
        AGENT_ADAPTERS_FORGET => told(block_on(crate::sessions::agent_adapters_forget(
            app.clone(),
        ))?),
        // Named in `sync_memory::SESSIONS` and not here: a defect between two
        // halves of one product, said in a sentence rather than left as a call
        // that arrives with nothing to run it.
        _ => Err(ProjectError::new(
            "session_unsupported",
            format!("Sync does not answer `{method}`"),
        )),
    }
}

/// One device has stopped watching, whether or not it said so.
///
/// The engine is the only side that can see it: this one writes events into a
/// socket to a process that is still there, and the connection that ended is a
/// hop further on. Without this a phone put in a pocket leaves a conversation
/// serialising every word the agent writes into a queue nobody drains.
///
/// # Errors
///
/// None. A number nothing holds is what a device that said so itself before its
/// connection ended looks like, and it is not worth a word.
fn nobody_is_watching<R: Runtime>(app: &AppHandle<R>, params: &Value) -> Result<Value, String> {
    let sessions = app.state::<crate::sessions::live::Sessions>();
    for subscription in params
        .get("subscriptions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_u64)
    {
        sessions.stop_watching(subscription);
    }
    Ok(Value::Null)
}

/// A device being shown a conversation, over the channel it asked on.
///
/// It writes onto the same socket every answer goes out on, behind the same
/// lock, so an event and an answer written in the same instant are two lines
/// rather than two halves of one. Where it goes after that is the engine's
/// business: the number is what the engine looks up to find the connection.
struct Elsewhere {
    subscription: u64,
    writing: Arc<Mutex<UnixStream>>,
}

impl crate::sessions::live::Watcher for Elsewhere {
    fn saw(&self, event: &crate::sessions::event::SessionEvent) -> bool {
        let Ok(event) = serde_json::to_value(event) else {
            // An event that will not serialise is a defect in its own shape and
            // not a device that has gone. Skipping one word is a smaller wrong
            // than ending somebody's transcript over it.
            eprintln!("an event of a watched conversation could not be written");
            return true;
        };
        say(
            &self.writing,
            &json!({
                "jsonrpc": "2.0",
                "method": sync_memory::SESSION_EVENT,
                "params": {"subscription": self.subscription, "event": event},
            }),
        )
        .is_ok()
    }
}

/// A member of a call about a conversation, read into what it stands for.
///
/// Absent is the default and never a refusal, because for every member that
/// reaches this the default is the answer: no working tree, nothing to attach,
/// no pictures, nothing said about what the conversation stands under. A phone
/// that omits one is a phone whose caller had nothing to put there, which is the
/// ordinary case rather than a malformed call.
fn part<T: serde::de::DeserializeOwned + Default>(
    params: &Value,
    member: &str,
) -> Result<T, ProjectError> {
    match params.get(member) {
        None | Some(Value::Null) => Ok(T::default()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            ProjectError::new(
                "session_malformed",
                format!("`{member}` is not one: {error}"),
            )
        }),
    }
}

/// What a call of this family was missing, said as the engine's mistake.
fn asked_without(member: &str) -> ProjectError {
    ProjectError::new(
        "session_malformed",
        format!("the memory engine asked about a conversation without `{member}`"),
    )
}

/// An answer in the shape a JSON line carries.
fn told<T: serde::Serialize>(answer: T) -> Result<Value, ProjectError> {
    serde_json::to_value(answer)
        .map_err(|error| ProjectError::new("session_malformed", error.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A tool that takes nothing is called with nothing, and that is an
    /// ordinary call rather than a request missing a member.
    #[test]
    fn a_request_without_arguments_is_a_call_with_none() {
        let asked = asked(&json!({
            "project": "/somewhere",
            "extension": "acme.tracker",
            "tool": "search",
        }))
        .expect("a tool may take nothing");

        assert_eq!(asked.arguments, json!({}));
        assert_eq!(asked.tool, "search");
    }

    /// A request missing a part of its address names the part, and names it as
    /// the engine's mistake rather than the package's: whoever reads this line
    /// has to know which half of the product to open.
    #[test]
    fn a_request_missing_part_of_its_address_says_which_part() {
        for (missing, without) in [
            (
                "a project",
                json!({"extension": "acme.tracker", "tool": "search"}),
            ),
            (
                "an extension",
                json!({"project": "/somewhere", "tool": "search"}),
            ),
            (
                "a tool",
                json!({"project": "/somewhere", "extension": "acme.tracker"}),
            ),
        ] {
            let refused = asked(&without).expect_err("it is not a call anybody can make");
            assert!(
                refused.contains(missing),
                "the refusal should name {missing}: {refused}"
            );
            assert!(
                refused.contains("the memory engine asked"),
                "and say whose mistake it is: {refused}"
            );
        }
    }

    /// A refusal goes back in the shape the engine reads answers in, under the
    /// id it asked with — an answer under another id reaches nobody.
    #[test]
    fn a_refusal_answers_the_call_it_is_about() {
        let refusal = refusal(&json!(7), "`acme.tracker` is not installed on this machine");

        assert_eq!(refusal["id"], json!(7));
        assert_eq!(refusal["error"]["data"]["kind"], "extension_failed");
        assert!(
            refusal["error"]["message"]
                .as_str()
                .expect("a message")
                .contains("not installed"),
            "the words reach the agent unchanged: {refusal}"
        );
    }
}
