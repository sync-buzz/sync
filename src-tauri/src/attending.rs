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
use sync_memory::{ATTEND, TOOL_CALL};
use tauri::{AppHandle, Runtime};

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

        if method != TOOL_CALL {
            say(
                &writing,
                &refusal(&id, &format!("Sync does not answer `{method}`")),
            )?;
            continue;
        }

        // One thread per call, and the connection stays readable while it runs.
        // A tool that waits twenty seconds on somebody's API would otherwise be
        // twenty seconds in which no other call could even be read — including
        // one for a different project.
        let app = app.clone();
        let writing = Arc::clone(&writing);
        std::thread::spawn(move || {
            let answered = match run(&app, &params) {
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
