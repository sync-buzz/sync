//! Calling an extension's handlers, and deciding what they may reach.
//!
//! `sync-handlers` knows how to run one; this is where it is decided *what*
//! runs, *for which project*, and *what it is allowed to see on the way*. The
//! division is the point: the crate cannot widen its own reach, because
//! everything a handler can do arrives as an implementation of its `Host` trait
//! and that implementation is here, beside the session registry, where the
//! project is known.
//!
//! # Questions, and one instruction
//!
//! Three of the four functions are questions: a handler can read a record, list
//! them and read a body — enough to decide something, which is what a handler
//! is for. The fourth orders work, and it is the first thing here that changes
//! anything. It arrives with the capability that gates it, by the rule every
//! one of them follows (`docs/background.md` §5), and it is the first capability
//! this build enforces *here* rather than when the manifest is read: whether a
//! handler calls it is inside the JavaScript, and no reader of a manifest can
//! see that. Writing to the corpus and the network are still absent, and adding
//! either before its capability would be granting it.
//!
//! # The occasion is a string
//!
//! Not an enum, because the set grows — a clock, another extension, an agent —
//! and each addition would otherwise be a change to this signature and to the
//! window's typed client for a fact neither of them interprets. What an
//! occasion *means* is the manifest's business: it names a handler, and the
//! name is what gets called.

use serde_json::Value;
use sync_extensions::Manifest;
use sync_handlers::{HandlerError, Host, Limits};
use tauri::{AppHandle, Manager, Runtime};

use crate::extensions::store;
use crate::memory::MemorySessions;

/// What a handler may not exceed on this build.
///
/// Numbers rather than a policy, and deliberately modest: a handler reads a
/// little and decides. They are the host's rather than the package's — an
/// extension that could raise its own ceiling has no ceiling — and the numbers
/// themselves are the kind that get revised, so they are one constant rather
/// than four scattered literals.
const LIMITS: Limits = Limits {
    memory_bytes: 16 * 1024 * 1024,
    wall_clock: std::time::Duration::from_secs(5),
};

/// Every function a handler may call, and the authority for the list.
///
/// `@sync-buzz/extension-api/service` states it a second time, in TypeScript,
/// because a package is built against a published contract rather than against
/// this source. That copy is behind this one by construction: an author who
/// calls something this does not answer hears a refusal naming what is here,
/// and hears it from `sync-ext check` in their own terminal. The same bargain
/// the manifest schema in that package already makes.
///
/// Checked against the match below by a test, because two lists and one truth
/// is what has cost this repository three defects.
const OFFERED: &[&str] = &[
    "memory.record",
    "memory.list",
    "memory.content",
    "work.order",
];

/// What a package must ask for before it may spend somebody's tokens.
///
/// Named here rather than beside [`sync_extensions::manifest::BACKGROUND_CAPABILITY`]
/// because there is no rule about it a manifest reader could apply. `background`
/// and `schedule` are visible in the file — a service module, a schedule — and
/// are refused at parse. This one is not visible anywhere but in the built
/// JavaScript, so the only honest place to enforce it is the moment the call is
/// made, which is here.
///
/// The consequence is that the refusal arrives when the handler runs, and for a
/// handler on a clock that is three in the morning with nobody watching. So
/// `sync-ext check` scans the built module for it as well, in the author's own
/// terminal, which is the earliest place the mistake can be caught at all.
const WORK_AGENT_CAPABILITY: &str = "work.agent";

/// The project's memory, as a handler is allowed to see it.
///
/// Holds the application rather than a session: a session is borrowed for the
/// length of one question and given back, so a handler that runs for a while
/// does not hold the engine open between two of its own reads. It also keeps
/// this owned, which is what lets it live inside the isolate.
struct ProjectMemory<R: Runtime> {
    app: AppHandle<R>,
    project: String,
    /// Which package is talking. Carried so that a line it writes says whose it
    /// is: a handler's output is only ever read beside other packages'.
    id: String,
    /// What that package is called. Carried because work it orders records it,
    /// so a heading naming the extension can be drawn without a catalogue —
    /// the bargain `agent_name` already makes one file over.
    name: String,
    /// Which of its handlers is talking. Carried for the same reason and one
    /// more: work ordered from here records who ordered it, and half of that
    /// answer is the handler's name (`docs/background.md` §6.3).
    handler: String,
    /// Whether the package asked for [`WORK_AGENT_CAPABILITY`].
    ///
    /// Decided once, where the manifest is in hand, rather than re-read on
    /// every call. A bool named for the question rather than the list of
    /// capabilities, because a list invites being consulted for something else.
    may_order_work: bool,
}

impl<R: Runtime> Host for ProjectMemory<R> {
    fn call(&mut self, function: &str, arguments: Value) -> Result<Value, String> {
        let sessions = self.app.state::<MemorySessions>();
        let said = |what: &str| format!("`{function}` needs {what}");

        match function {
            // A line a handler wrote goes to this process's own error stream,
            // which is where a developer looking at `tauri dev` finds it. The
            // host deciding where it goes — rather than the isolate having a
            // `console` of its own — is what keeps this the only place that
            // would have to change, with nothing in any package changing.
            _ if function.starts_with("console.") => {
                let said = arguments["said"].as_str().unwrap_or_default();
                let level = function.trim_start_matches("console.");
                eprintln!("[{}] {level}: {said}", self.id);
                Ok(Value::Null)
            }
            "memory.record" => {
                let key = arguments
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| said("a key"))?;
                sessions
                    .with_session_here(&self.app, &self.project, |client| client.get_record(key))
                    .map(|mut view| {
                        view.record = view.record.map(envelope_of);
                        serde_json::to_value(view).unwrap_or(Value::Null)
                    })
                    .map_err(|error| error.message)
            }
            "memory.list" => sessions
                .with_session_here(&self.app, &self.project, |client| {
                    client.list_records(&arguments)
                })
                .map(|listing| serde_json::to_value(listing).unwrap_or(Value::Null))
                .map_err(|error| error.message),
            "memory.content" => {
                let key = arguments
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| said("a key"))?;
                sessions
                    .with_session_here(&self.app, &self.project, |client| client.read_content(key))
                    .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
                    .map_err(|error| error.message)
            }
            // The one function here that changes something. It answers a key
            // and nothing else, because by the time the agent has been raised
            // this isolate is gone: a handler *orders* work and the host
            // performs it (`docs/background.md` §2), and the host is what
            // outlives a handler.
            "work.order" => {
                if !self.may_order_work {
                    return Err(format!(
                        "`{}` may not order work: its manifest does not ask for the \"{WORK_AGENT_CAPABILITY}\" capability. Ordering work spends somebody's tokens while they are asleep, and the card they install from has to say so first",
                        self.id
                    ));
                }
                let order: crate::work::Order =
                    serde_json::from_value(arguments).map_err(|error| {
                        format!("`{function}` was given an order it could not read: {error}")
                    })?;
                crate::work::order(
                    &self.app,
                    &self.project,
                    crate::work::Package {
                        id: &self.id,
                        name: &self.name,
                    },
                    &self.handler,
                    order,
                )
                .map(Value::String)
            }
            // Named rather than ignored, and named as a refusal rather than as a
            // missing function: a handler asking for something it has no
            // permission for should hear why, and its author should be able to
            // catch it and carry on.
            //
            // The sentence names what *is* offered, and it names it from the
            // list rather than from a second spelling of it: this refusal is
            // the whole of what bounds the drift between this match and the
            // surface `@sync-buzz/extension-api/service` publishes.
            other => Err(format!(
                "`{other}` is not something a handler may do — this build offers {}",
                OFFERED.join(", ")
            )),
        }
    }
}

/// The envelope inside a stored record.
///
/// The engine answers `records.get` with the record's *durable representation*
/// — `{"representation": "plaintext", "envelope": {…}}` — and the envelope is
/// where every member a handler reads lives, the type's own product fields
/// among them. The wrapper is the store's file format rather than the record,
/// and `@sync-buzz/extension-api/service` already says so: `RecordView.record`
/// is an `Envelope`, whose members are the engine's own.
///
/// Handing the wrapper across instead was a defect nothing reported. `routines`
/// read `record.fields`, got `undefined` for every routine because there is no
/// such member at either depth, and skipped all of them — a clock that ticked
/// for a day and ordered nothing, with no error anywhere to say so. That is the
/// boundary this repository has now paid for twice: a field that is not where a
/// reader looks arrives as nothing, and nothing is a value.
///
/// The tag is what it is recognised by, rather than the presence of an
/// `envelope` member. `envelope` is not one of the names
/// [`sync_memory::colliding_declarations`] reserves, so a type is free to
/// declare a product field called that — and unwrapping on the name alone would
/// hand such a record's *field* over as the whole record.
fn envelope_of(record: Value) -> Value {
    match record {
        Value::Object(mut members) if members.contains_key("representation") => {
            members.remove("envelope").unwrap_or(Value::Object(members))
        }
        other => other,
    }
}

/// Which handler an occasion names, if any.
///
/// A package that declares nothing for an occasion is the ordinary case, not a
/// failure: most extensions want none of them.
fn named_for<'a>(manifest: &'a Manifest, occasion: &str) -> Option<&'a str> {
    match occasion {
        "installed" => manifest.lifecycle.installed.as_deref(),
        _ => None,
    }
}

/// Runs the handler an occasion names, and answers what it returned.
///
/// `Ok(None)` says the package declares nothing for this occasion, which is not
/// a failure and is the usual answer. Any `Err` is the handler's own — it threw,
/// it ran too long, or the module and the manifest disagree — and it is carried
/// through in the words that will help whoever has to fix it.
///
/// # Blocking, and where it is done
///
/// Every part of this blocks: the store is read from the disk, a module is
/// evaluated, and a handler that reaches memory waits on the engine — for up to
/// [`LIMITS`]' wall clock. A plain `#[tauri::command]` runs on the **main
/// thread**, so until this was moved, installing an extension whose handler
/// took a second froze the window for a second.
///
/// It is `spawn_blocking` rather than `#[tauri::command(async)]`, which is a
/// rule this repository paid for with a silent defect:
/// `(async)` on a synchronous function means `tokio::spawn`, so the body runs
/// *inside* the async context, and anything that owns a runtime — every
/// `reqwest::blocking`, every nested `Runtime::new` — panics when it is
/// dropped there.
///
/// # Errors
///
/// When the extension is not installed on this machine, when its service module
/// cannot be read, or when the handler failed. A handler that fails at install
/// fails the install: this returns the reason and the caller decides, which for
/// an install is to stop and say so.
#[tauri::command]
pub async fn extension_handler_call<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    id: String,
    occasion: String,
    payload: Value,
) -> Result<Option<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installed = store(&app)?
            .resolve(&id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("`{id}` is not installed on this machine"))?;

        let Some(handler) = named_for(&installed.manifest, &occasion) else {
            return Ok(None);
        };
        run(&app, &installed, &project, handler, &payload).map(Some)
    })
    .await
    .map_err(|error| format!("running the handler did not finish: {error}"))?
}

/// Run one named handler of one package, for one project.
///
/// The occasion is the caller's business and has already been resolved to a
/// name by the time this is reached: the window resolves `installed` through
/// [`named_for`], and the clock resolves an entry of the manifest's schedule.
/// One evaluation path for both, so an occasion added later cannot quietly get
/// a different runtime, different limits or a different host.
///
/// It takes the package already resolved rather than its id, because both
/// callers have had to resolve it to know there was anything to call.
///
/// # Errors
///
/// When the service module cannot be read, or when the handler failed — it
/// threw, it ran too long, or the module and the manifest disagree.
pub(crate) fn run<R: Runtime>(
    app: &AppHandle<R>,
    installed: &sync_extensions::Installed,
    project: &str,
    handler: &str,
    payload: &Value,
) -> Result<Value, String> {
    let id = &installed.manifest.id;
    // A manifest that names a handler and no module is refused at parse, so
    // reaching here without one would be a package that changed under us.
    let path = installed.manifest.service.as_ref().ok_or_else(|| {
        format!("`{id}` names the handler `{handler}` and ships no service module")
    })?;
    let source = std::fs::read_to_string(installed.root.join(path))
        .map_err(|error| format!("`{id}` could not be read to run `{handler}`: {error}"))?;

    let host = ProjectMemory {
        app: app.clone(),
        project: project.to_owned(),
        id: id.clone(),
        name: installed.manifest.name.clone(),
        handler: handler.to_owned(),
        may_order_work: installed
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == WORK_AGENT_CAPABILITY),
    };
    match sync_handlers::call(&source, handler, payload, LIMITS, host) {
        Ok(answer) => Ok(answer),
        // The extension's id belongs in front of every one of these: by the
        // time somebody reads it, which package it was is the first thing they
        // need and the last thing the crate could have known.
        Err(error @ HandlerError::NotDeclared(_)) => {
            Err(format!("`{id}` and its service module disagree: {error}"))
        }
        Err(error) => Err(format!("`{id}`: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What a handler is handed for a record, asserted at the depth the defect
    /// was at. `routines` looked for its fields one level above where they are
    /// and found nothing — and nothing is not an error, so its clock ticked for
    /// a day, skipped every routine and said so to no one.
    #[test]
    fn a_record_reaches_a_handler_as_its_envelope() {
        let stored = json!({
            "representation": "plaintext",
            "envelope": {
                "key": "routine-354de7",
                "kind": "routines.routine",
                "title": "Checking the inbox",
                "archive": {"archived": false},
                "enabled": true,
                "every": "15m",
            },
        });

        let record = envelope_of(stored);
        assert_eq!(
            record["enabled"],
            json!(true),
            "a product field is a member"
        );
        assert_eq!(record["every"], json!("15m"));
        assert_eq!(record["title"], json!("Checking the inbox"));
        assert_eq!(record["archive"]["archived"], json!(false));
        assert!(
            record.get("representation").is_none(),
            "the store's own format does not cross: {record}"
        );
    }

    /// A record that is already an envelope crosses whole. That is what an
    /// older engine, or a second representation, would hand over, and reading
    /// one as nothing would be worse than the wrapper this unwraps.
    #[test]
    fn a_bare_envelope_is_left_alone() {
        let bare = json!({"key": "k", "kind": "routines.routine", "enabled": true});
        assert_eq!(envelope_of(bare.clone()), bare);
    }

    /// The tag is what the wrapper is recognised by. `envelope` is not a name
    /// the store reserves, so a type may declare a product field called that —
    /// and unwrapping on the name alone would hand that field over as the whole
    /// record.
    #[test]
    fn a_field_called_envelope_is_a_field() {
        let record = json!({
            "key": "k",
            "kind": "post.letter",
            "envelope": {"stamp": "first class"},
        });
        assert_eq!(envelope_of(record.clone()), record);
    }
}
