//! Calling an extension's handlers, and deciding what they may reach.
//!
//! `sync-handlers` knows how to run one; this is where it is decided *what*
//! runs, *for which project*, and *what it is allowed to see on the way*. The
//! division is the point: the crate cannot widen its own reach, because
//! everything a handler can do arrives as an implementation of its `Host` trait
//! and that implementation is here, beside the session registry, where the
//! project is known.
//!
//! # Questions, instructions, and the two doors that leave the machine
//!
//! Three functions are questions: a handler can read a record, list them and
//! read a body — enough to decide something, which is what a handler is for.
//! The rest change something or leave: it orders work, it reaches the keychain,
//! and it dials out. Each arrives with the capability that gates it, by the
//! rule every one of them follows (`docs/background.md` §5), and each is
//! enforced *here* rather than when the manifest is read: whether a handler
//! calls it is inside the JavaScript, and no reader of a manifest can see that.
//! Writing to the corpus is still absent, and adding it before its capability
//! would be granting it.
//!
//! # The keychain and the network are one implementation, not two
//!
//! Both doors exist already for the half of an extension that has a screen, and
//! this is the same door reached from the other side: [`crate::vault`] and
//! [`crate::extensions`] hold the one implementation and both halves call it.
//! A handler that runs for an agent has no window, so a second implementation
//! here is what would let the two halves come to disagree about what a package
//! may reach — and the disagreement would be *which hosts* and *whose secrets*.
//!
//! What a package does with a secret once it holds one is not something this
//! file can enforce, and the rule is stated where an author reads:
//! `docs/background.md` §3.3. What it *can* do is keep a value the handler read
//! out of the host's log, which is [`ProjectMemory::redacted`].
//!
//! # The occasion is a string
//!
//! Not an enum, because the set grows — a clock, another extension, an agent —
//! and each addition would otherwise be a change to this signature and to the
//! window's typed client for a fact neither of them interprets. What an
//! occasion *means* is the manifest's business: it names a handler, and the
//! name is what gets called.

use serde_json::Value;
use sync_extensions::{Manifest, NET_CAPABILITY, NetRequest};
use sync_handlers::{HandlerError, Host, Limits};
use tauri::{AppHandle, Manager, Runtime};

use crate::extensions::{asked_for, fetch_now, store};
use crate::memory::MemorySessions;
use crate::vault::{VAULT_CAPABILITY, address, for_package_now};

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
    "vault.read",
    "vault.write",
    "vault.forget",
    "net.fetch",
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
    /// What the package asked for, and where it is allowed to reach.
    ///
    /// **Read once, when the call began, and never from the caller.** Every
    /// permission this host answers to is in here — the capabilities, and the
    /// hosts the network door is checked against — so an argument arriving from
    /// the isolate cannot widen any of them.
    ///
    /// The whole manifest rather than the two or three answers taken off it,
    /// because it was in hand where the isolate was built: taking a snapshot of
    /// each answer instead would mean a second reading of the same file per
    /// call, which can come back different halfway through a handler.
    manifest: Manifest,
    /// Secret values this call has read, so that a line it writes cannot carry
    /// one.
    ///
    /// `console` goes to the host's log and a log outlives everything around
    /// it. An author who logs a token while working something out does not
    /// abuse anything — they forget — and no review catches what is only in one
    /// developer's terminal. The host knows every value it handed over, so it
    /// takes them back out of what it prints.
    ///
    /// This is not secrecy from the package: it holds the value and may send it
    /// anywhere its manifest allows. It is one accident, closed.
    redacted: Vec<String>,
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
                let level = function.trim_start_matches("console.");
                let said = without_secrets(
                    &self.redacted,
                    arguments["said"].as_str().unwrap_or_default(),
                );
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
                if !self.manifest.asks_for(WORK_AGENT_CAPABILITY) {
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
            // The keychain. One capability for all three, because the flow that
            // needs any of them needs the others: a package that signs somebody
            // in ends up holding a token nobody could have typed, and refreshes
            // it before it expires. The owner half of the address is the id
            // this host was built with, so a name is only ever a name.
            "vault.read" => {
                asked_for(&self.manifest, &self.id, VAULT_CAPABILITY)?;
                let slot = addressed(&self.id, &arguments)?;
                let secret = for_package_now(&slot, |vault, slot| vault.read(slot))?;
                self.redacted.push(secret.clone());
                Ok(Value::String(secret))
            }
            "vault.write" => {
                asked_for(&self.manifest, &self.id, VAULT_CAPABILITY)?;
                let slot = addressed(&self.id, &arguments)?;
                let secret = arguments
                    .get("secret")
                    .and_then(Value::as_str)
                    .ok_or_else(|| said("a secret to store"))?
                    .to_owned();
                // Written down before the call rather than after it, because a
                // write that failed still had the value pass through here — and
                // the line an author logs while working out why it failed is
                // exactly the line this exists for.
                self.redacted.push(secret.clone());
                for_package_now(&slot, |vault, slot| vault.write(slot, &secret))?;
                Ok(Value::Null)
            }
            "vault.forget" => {
                asked_for(&self.manifest, &self.id, VAULT_CAPABILITY)?;
                let slot = addressed(&self.id, &arguments)?;
                for_package_now(&slot, |vault, slot| vault.forget(slot))?;
                Ok(Value::Null)
            }
            // The network. The request is the package's; where it may go, and
            // which secrets ride along in headers it never sees, come off the
            // manifest — [`fetch_now`] is the same call the window makes.
            //
            // This one waits, and it is the first thing here that does: the
            // thread of the isolate sits in Rust until the answer arrives or
            // the door's own timeout stops it. That wait is not charged to the
            // handler's wall clock, which is `Clock` in `sync-handlers`.
            "net.fetch" => {
                asked_for(&self.manifest, &self.id, NET_CAPABILITY)?;
                let request: NetRequest = serde_json::from_value(arguments).map_err(|error| {
                    format!("`{function}` was given a request it could not read: {error}")
                })?;
                fetch_now(&self.id, &self.manifest, &request)
                    .map(|response| serde_json::to_value(response).unwrap_or(Value::Null))
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

/// Which entry a handler's keychain call is about.
///
/// **The owner is the package this host was built for, and there is no
/// argument that can be the other half.** A call supplies a name; the id comes
/// from the artefact the isolate was built from. So a name that reads like a
/// way out of the namespace — a path, another package's id, a leading separator
/// — addresses an entry of the caller's own with an odd name, and there is
/// nothing to spell that would reach anybody else's.
///
/// # Errors
///
/// When the call named nothing, or named something the store cannot address.
fn addressed(id: &str, arguments: &Value) -> Result<sync_vault::Slot, String> {
    let name = arguments
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "a keychain call needs the name of a secret".to_owned())?;
    address(id, name)
}

/// One line, with every secret this call has read taken out of it.
///
/// A plain substring replacement, which is the right shape here: what is being
/// caught is a value printed, formatted into a sentence or joined into a URL,
/// and all three are substrings. A value the handler took apart before printing
/// is not caught, and nothing pretends otherwise — the packages this is for log
/// the token, not its halves.
///
/// An empty entry is skipped. Replacing it would put the marker between every
/// pair of characters in the line, which turns a log into nothing at the one
/// moment somebody is reading it.
fn without_secrets(redacted: &[String], said: &str) -> String {
    redacted
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(said.to_owned(), |line, secret| {
            line.replace(secret.as_str(), REDACTED)
        })
}

/// What stands in a log where a secret was.
///
/// Says that something was taken out rather than leaving a gap: a line that
/// silently lost a value reads as a bug in the handler, and the author goes
/// looking for the wrong thing.
const REDACTED: &str = "[a secret]";

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

        // Resolved by the manifest rather than here, and that is what keeps the
        // occasions one thing: an install, a clock and a tool an agent named
        // are three callers asking *which function is this*, and asking it in
        // three places is how one of them quietly gets a different answer. What
        // this layer decides is who may ask, which is the division the crate
        // boundary already draws.
        //
        // A package that declares nothing for an occasion is the ordinary case
        // rather than a failure: most extensions want none of them.
        let Some(handler) = installed.manifest.handler_for(&occasion) else {
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
/// name by the time this is reached: an install and a tool through
/// [`sync_extensions::Manifest::handler_for`], and the clock through an entry
/// of the manifest's schedule. One evaluation path for all of them, so an
/// occasion added later cannot quietly get a different runtime, different
/// limits or a different host.
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
        manifest: installed.manifest.clone(),
        redacted: Vec::new(),
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

    /// A keychain call reaches the calling package's namespace and no other.
    ///
    /// The owner is not something a call can carry: it is the id the isolate
    /// was built for. So every way of spelling somebody else's is a name, and
    /// what it addresses is an oddly-named entry of the caller's own.
    #[test]
    fn a_keychain_call_cannot_name_another_package() {
        for arguments in [
            json!({ "name": "token" }),
            json!({ "name": "../another-package/token" }),
            json!({ "name": "another-package/token" }),
            json!({ "name": "/token" }),
            // The one that would matter if the owner were ever taken from the
            // call: a member that looks exactly like the missing half.
            json!({ "name": "token", "owner": "another-package" }),
            json!({ "name": "token", "id": "another-package" }),
        ] {
            let slot = addressed("a-package", &arguments).expect("a package addresses a secret");
            assert_eq!(
                slot.owner(),
                "a-package",
                "{arguments} addressed somebody else's namespace"
            );
        }
    }

    /// A call that names nothing is refused rather than addressed.
    #[test]
    fn a_keychain_call_with_no_name_is_refused_in_words() {
        let refused = addressed("a-package", &json!({})).expect_err("there is nothing to address");
        assert!(
            refused.contains("name of a secret"),
            "the refusal says what was missing: {refused}"
        );
    }

    /// A secret a handler read does not reach the host's log.
    ///
    /// The author is not abusing anything: they print a token while working out
    /// why somebody's API says no, and they forget. The log outlives the
    /// afternoon, the window, and usually the debugging — so what the host
    /// handed over, the host takes back out.
    ///
    /// Every shape a value is printed in, because a check that only caught a
    /// bare value would pass while the interesting cases went through: a token
    /// in a sentence, in a header, and in a query string.
    #[test]
    fn a_line_a_handler_wrote_does_not_carry_a_secret_it_read() {
        let read = vec!["ghp_averyrealtoken".to_owned()];

        for said in [
            "ghp_averyrealtoken",
            "the token is ghp_averyrealtoken, which should work",
            "Authorization: Bearer ghp_averyrealtoken",
            "https://api.example.com/things?access_token=ghp_averyrealtoken&page=2",
        ] {
            let logged = without_secrets(&read, said);
            assert!(
                !logged.contains("ghp_averyrealtoken"),
                "the value went to the log: {logged}"
            );
            assert!(
                logged.contains(REDACTED),
                "and the line does not say something was taken out: {logged}"
            );
        }
    }

    /// A line with no secret in it is the line the author wrote.
    ///
    /// Including when the call read a secret that happens to be empty — an
    /// entry somebody stored as nothing. Replacing that would put the marker
    /// between every pair of characters, and a log nobody can read is the
    /// failure this was written to avoid rather than one to introduce.
    #[test]
    fn an_ordinary_line_is_left_exactly_as_it_was() {
        let read = vec![String::new(), "ghp_averyrealtoken".to_owned()];

        assert_eq!(
            without_secrets(&read, "asked the tracker for page 2"),
            "asked the tracker for page 2"
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
