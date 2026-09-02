//! Answering what the engine asks of Sync.
//!
//! Every other message between the two goes the other way: Sync asks, the
//! engine answers. This is the one call that goes from there to here, and it
//! carries a tool an agent asked for.
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
use sync_memory::{ATTEND, EXTENSION_FETCH, TOOL_CALL, carried};
use tauri::{AppHandle, Manager as _, Runtime};

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

        if method != TOOL_CALL && !carried(method) {
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
            let outcome = if method == TOOL_CALL {
                run(&app, &params)
            } else {
                about_a_package(&app, &method, &params)
            };
            let answered = match outcome {
                Ok(answer) => json!({"jsonrpc": "2.0", "id": id, "result": answer}),
                Err(why) => refusal(&id, &why),
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
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": -32000,
        "message": why,
        "data": {"kind": "extension_failed"},
    }})
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
