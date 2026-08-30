//! What an extension runs when no screen is mounted.
//!
//! A handler is a function a package declares in its manifest and ships in its
//! service module. Something calls it — a clock, another extension, an agent,
//! an install — it runs, it answers or it fails, and it is gone. It is not a
//! process, not a worker and not a subscription: `docs/background.md` §2 is the
//! vocabulary, and the distinction that matters here is that **a handler is not
//! work**. It lives for milliseconds and may *order* something that lives for
//! hours.
//!
//! Tauri-free like `sync-memory` and `sync-extensions` beside it, and narrower
//! than either: this crate knows nothing about extensions, projects, packages
//! or memory. It takes a module's source, a handler's name and a payload, and
//! answers with JSON or with a failure. Everything a handler can reach arrives
//! through [`Host`], which the caller implements — so what a handler may do is
//! decided one layer up, where permissions are known, and cannot be widened
//! from in here.
//!
//! # Synchronous, deliberately
//!
//! The measurement behind the runtime choice covered an async host function and
//! it works, so this is a choice rather than a limit. It is the same one
//! `sync-extensions` made about `reqwest`: everything around this is
//! synchronous — memory is read through a blocking client, the command layer
//! above is plain functions — and one async island would spread through all of
//! it. When a handler needs the network, that is the moment to revisit, and the
//! path is known to exist.
//!
//! # A fresh isolate for every call
//!
//! Nothing is kept between calls: the module is evaluated, the handler runs,
//! the runtime is dropped. That is what makes a handler *a function that can be
//! called* rather than a daemon with a quiet lifetime — state that survived
//! between two calls would be a process nobody declared, and it would survive a
//! call that was interrupted halfway through with whatever it had written to
//! itself.
//!
//! The cost is one evaluation per call, and it is **540 µs** — isolate, module
//! and call together, measured over 200 runs by `what_a_fresh_isolate_costs`,
//! which is in the tests under `--ignored` so the number can be taken again
//! rather than believed. An hourly handler could pay it a thousand times an
//! hour and not be noticed; anything hot enough to mind is not a caching
//! problem here but a design problem one layer up.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Ctx, Function, Module, Object, Runtime, Value};
use serde_json::Value as Json;

/// The global a handler reaches the host through.
///
/// One entry point rather than a function per capability: the service surface
/// an author writes against is TypeScript that wraps this, so the names live in
/// the contract where they can be versioned and checked, and this crate stays
/// ignorant of every one of them.
const HOST_GLOBAL: &str = "__syncHost__";

/// The name a service module is evaluated under. It appears in stack traces.
const MODULE_NAME: &str = "service";

/// Every global a bare isolate provides, measured rather than remembered.
///
/// Kept because it is the contract an author is really writing against, and it
/// is much narrower than either Node or a browser: **there is no `console`, no
/// `fetch`, no `setTimeout`, no `TextDecoder`, no `Intl` and no
/// `WebAssembly`.** `sync-ext check` has to evaluate a service module in an
/// environment this poor, or it passes modules that cannot run.
///
/// The test beside it takes the list from a live isolate and compares, so a
/// newer `QuickJS` that adds or drops one is a failing test here rather than a
/// surprise in somebody's package.
// A table rather than a list of statements: one name per line would be sixty
// lines saying nothing that the shape does not already say.
#[rustfmt::skip]
pub const ISOLATE_GLOBALS: &[&str] = &[
    "AggregateError", "Array", "ArrayBuffer", "Atomics", "BigInt", "BigInt64Array",
    "BigUint64Array", "Boolean", "DataView", "Date", "Error", "EvalError",
    "FinalizationRegistry", "Float16Array", "Float32Array", "Float64Array", "Function",
    "Infinity", "Int16Array", "Int32Array", "Int8Array", "InternalError", "Iterator", "JSON",
    "Map", "Math", "NaN", "Number", "Object", "Promise", "Proxy", "RangeError",
    "ReferenceError", "Reflect", "RegExp", "Set", "SharedArrayBuffer", "String", "Symbol",
    "SyntaxError", "TypeError", "URIError", "Uint16Array", "Uint32Array", "Uint8Array",
    "Uint8ClampedArray", "WeakMap", "WeakRef", "WeakSet", "decodeURI", "decodeURIComponent",
    "encodeURI", "encodeURIComponent", "escape", "eval", "globalThis", "isFinite", "isNaN",
    "parseFloat", "parseInt", "performance", "queueMicrotask", "undefined", "unescape",
];

/// What the host is asked when a handler writes a line.
///
/// `console` is not in the isolate, and an author will reach for it in the
/// first hour. Giving it as a host call rather than as a global is what keeps
/// the rule intact: a line a handler wrote is something the host decides what
/// to do with, rather than something the interpreter swallowed.
const CONSOLE_LEVELS: [&str; 4] = ["log", "info", "warn", "error"];

/// What a handler may not exceed.
///
/// Mandatory rather than the package's to configure: an extension that hangs
/// must fail its own call and leave the application alone, which it cannot
/// promise about itself. The numbers are the caller's — this crate has no
/// opinion about how long a poll should take.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The isolate's ceiling. Allocating past it fails the call.
    pub memory_bytes: usize,
    /// How long a handler's **own code** may run before it is interrupted.
    ///
    /// This is a guarantee against a loop in JavaScript and against nothing
    /// else. The interpreter asks whether to stop between two bytecode
    /// instructions, so while the thread is inside a host function it is not
    /// asked at all — a handler waiting twenty seconds for somebody's API is
    /// bounded by that door's own timeout, and not by this.
    ///
    /// So the time spent inside [`Host::call`] does not spend this budget: it
    /// is measured and given back (see [`Clock`]). Otherwise a handler that
    /// waited for an answer would be stopped on the first instruction after
    /// receiving it — the answer arriving and the call failing anyway, which is
    /// the shape of failure this repository pays for most.
    pub wall_clock: Duration,
}

impl Default for Limits {
    /// Deliberately modest. A handler reads a little, decides, and orders
    /// something; one that needs more than this is doing work that belongs
    /// behind [`Host`], where it can be interrupted and reported on.
    fn default() -> Self {
        Self {
            memory_bytes: 16 * 1024 * 1024,
            wall_clock: Duration::from_secs(5),
        }
    }
}

/// How much of its budget one call has left, and why that can move.
///
/// A plain `Instant` was enough while every host function answered from memory.
/// The moment one of them dials out, it is not: the interpreter is asked
/// whether to stop only between two bytecode instructions, so a call that sat
/// twenty seconds inside `Host::call` comes back to an instant that passed
/// nineteen seconds ago and is stopped on the next instruction it runs. The
/// answer arrived and the handler failed anyway.
///
/// So waiting on the host is not spent from the budget. [`Self::waiting`] wraps
/// every host call, measures it and hands the same amount back, which leaves
/// [`Limits::wall_clock`] meaning what it says: how long a handler's own code
/// may run.
///
/// **This bounds JavaScript and not the wall.** A handler making a hundred
/// requests is bounded by a hundred network timeouts and by nothing here. What
/// that costs — one handler at a time on this machine — is the slot's question
/// rather than this type's.
///
/// Nanoseconds in an atomic rather than an `Instant` behind a lock, because the
/// interrupt handler reads this between bytecode instructions and a lock there
/// is a cost on every one of them. `Relaxed` because there is one writer, the
/// same thread that runs the isolate, and nothing is ordered against it.
#[derive(Clone)]
struct Clock {
    started: Instant,
    limit: Duration,
    waited: Arc<AtomicU64>,
}

impl Clock {
    fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
            waited: Arc::new(AtomicU64::new(0)),
        }
    }

    /// When this call runs out, as things stand.
    fn deadline(&self) -> Instant {
        self.started + self.limit + Duration::from_nanos(self.waited.load(Ordering::Relaxed))
    }

    /// Whether it already has.
    fn expired(&self) -> bool {
        Instant::now() >= self.deadline()
    }

    /// Does one thing on the host's side of the bridge, off the clock.
    ///
    /// Saturating, because the alternative to a budget that stops growing is
    /// one that wraps to nothing and interrupts a handler that had just
    /// started. Reaching it takes five hundred years of waiting.
    fn waiting<T>(&self, work: impl FnOnce() -> T) -> T {
        let began = Instant::now();
        let answer = work();
        let waited = u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.waited.fetch_add(waited, Ordering::Relaxed);
        answer
    }
}

/// What a handler can reach, decided by whoever calls it.
///
/// Nothing is ambient: a handler has no filesystem, no network, no clock beyond
/// `Date`, and no way to name another extension. It has this, and this answers
/// only what the caller decided it should.
pub trait Host {
    /// Answers one call. `Err` becomes an exception the handler may catch —
    /// a refusal is something a package can be written against, not a crash.
    ///
    /// # Errors
    ///
    /// Whatever the implementation refuses, in words a handler's author can act
    /// on. The string reaches JavaScript as the message of an `Error`.
    fn call(&mut self, function: &str, arguments: Json) -> Result<Json, String>;
}

/// A host that refuses everything, for checking what a module declares without
/// letting it do anything on the way.
pub struct NoHost;

impl Host for NoHost {
    fn call(&mut self, function: &str, _arguments: Json) -> Result<Json, String> {
        // A line is not a capability. A module that writes one while it is
        // being read is doing nothing, and refusing it would make `declared`
        // fail on a package whose only sin is a `console.log` at the top level.
        if function.starts_with("console.") {
            return Ok(Json::Null);
        }
        Err(format!(
            "`{function}` is not available while the module is only being read"
        ))
    }
}

/// Why a call did not produce an answer.
///
/// Each variant is a different conversation with a different person:
/// [`Self::Interrupted`] and [`Self::OutOfMemory`] are the package author's
/// problem, [`Self::NotDeclared`] is a mismatch between two files they own, and
/// [`Self::Threw`] is what their own code said.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HandlerError {
    /// The module did not evaluate: a syntax error, or something thrown while
    /// it was being read.
    #[error("the service module could not be evaluated: {0}")]
    Evaluation(String),
    /// No default export, or one that is not a function. The contract is
    /// `export default function register()`.
    #[error("the service module does not export a default function")]
    NoRegister,
    /// `register()` did not answer with a table of handlers.
    #[error("register() did not return an object of handlers")]
    NotATable,
    /// The module ran and does not have the handler that was asked for. The
    /// manifest and the module disagree, which is what `sync-ext check` exists
    /// to catch before anybody installs anything.
    #[error("the service module declares no handler called `{0}`")]
    NotDeclared(String),
    /// The name is there and is not callable.
    #[error("`{0}` is not a function")]
    NotAFunction(String),
    /// The handler threw.
    #[error("`{handler}` threw: {message}")]
    Threw { handler: String, message: String },
    /// The handler ran past its wall clock and was stopped. Nothing it had
    /// started is undone — this says the call failed, never that the world is
    /// unchanged.
    #[error("`{handler}` did not finish within {}ms and was stopped", .limit.as_millis())]
    Interrupted { handler: String, limit: Duration },
    /// The isolate hit its ceiling.
    #[error("`{handler}` ran out of memory")]
    OutOfMemory { handler: String },
    /// The handler is `async` and is waiting for something that cannot arrive.
    ///
    /// The isolate has no timers, no sockets and no host function that waits,
    /// so a promise the job queue cannot settle is one nothing will. Almost
    /// always a handler awaiting a promise it made itself and never resolved.
    #[error(
        "`{handler}` is waiting for something that cannot happen here: this isolate has no timers and no host call that waits, so a promise nothing settles never will"
    )]
    NeverSettled { handler: String },
    /// The payload or the answer would not cross as JSON. An answer that is
    /// `undefined` is not this: it is `null`, which is an answer.
    #[error("{0}")]
    NotJson(String),
}

/// What a module says it can do.
///
/// Evaluates the module with a host that refuses everything and returns the
/// names in the table `register()` answered with. This is the check that earns
/// its keep: a manifest and a module are related by nothing the type system can
/// see, so an area renamed in one and not the other type-checks perfectly and
/// installs as a section that does nothing. Running it is the only way to know.
///
/// # Errors
///
/// [`HandlerError::Evaluation`], [`HandlerError::NoRegister`] or
/// [`HandlerError::NotATable`] — the three ways a module can fail to be one.
pub fn declared(source: &str, limits: Limits) -> Result<Vec<String>, HandlerError> {
    in_isolate(limits, "register", NoHost, |ctx, _| {
        let table = register(&ctx, source)?;
        table
            .keys::<String>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| HandlerError::NotATable)
    })
}

/// Runs one handler and answers what it returned.
///
/// The module is evaluated, `register()` is called, the named handler is looked
/// up and called with `payload`. A fresh isolate every time — see the module
/// docs for why that is the design rather than an omission.
///
/// # Errors
///
/// Any [`HandlerError`]. A handler that threw is not a bug in this crate, and
/// the message it threw is carried through rather than summarised.
pub fn call(
    source: &str,
    handler: &str,
    payload: &Json,
    limits: Limits,
    host: impl Host + 'static,
) -> Result<Json, HandlerError> {
    in_isolate(limits, handler, host, |ctx, clock| {
        let table = register(&ctx, source)?;
        let function: Function = match table.get::<_, Value>(handler) {
            Ok(value) if value.is_function() => value
                .into_function()
                .ok_or_else(|| HandlerError::NotAFunction(handler.to_owned()))?,
            Ok(value) if value.is_undefined() => {
                return Err(HandlerError::NotDeclared(handler.to_owned()));
            }
            Ok(_) => return Err(HandlerError::NotAFunction(handler.to_owned())),
            Err(_) => return Err(HandlerError::NotDeclared(handler.to_owned())),
        };

        let argument = to_js(&ctx, payload)?;
        // A handler may be `async`, and one that is answers with a promise.
        //
        // Taking the returned value as it stands would serialise the promise
        // object rather than what it settles to — measured, and it comes back
        // as `{}`: not an error, not a refusal, an empty answer. A handler that
        // returned something nobody received is the silent failure this product
        // is least allowed to have, and step 4 makes it concrete, because the
        // thing `work.order` returns is the only handle on the work it started.
        //
        // [`MaybePromise`] passes a plain value through untouched and, for a
        // promise, runs the isolate's job queue until it settles. Nothing here
        // *waits*: every host function answers synchronously, so a promise over
        // one settles on the first turn of the queue. What this buys is the
        // shape — an author writes `await` from the first line, and the day a
        // host function genuinely waits, no package changes.
        let answered: Value = function
            .call((argument,))
            .catch(&ctx)
            .map_err(|error| threw(handler, &error.to_string(), clock, limits))?;
        let settled = match answered.as_promise() {
            // `finish` turns the isolate's job queue until the promise settles,
            // and answers `WouldBlock` when the queue runs dry with it still
            // pending. The interrupt handler is above all of this, so a job
            // that loops for ever is stopped by the same clock as a handler
            // that does.
            Some(promise) => {
                let finished = promise.finish::<Value>();
                // Matched on the variant rather than on the message. `WouldBlock`
                // renders as "blocking on a promise resulted in a dead lock",
                // which is the interpreter's word for its own condition; and a
                // library that reworded it would silently turn this refusal
                // into "the handler threw", which is the two-lists-one-truth
                // trap wearing a new coat.
                if matches!(finished, Err(rquickjs::Error::WouldBlock)) {
                    return Err(HandlerError::NeverSettled {
                        handler: handler.to_owned(),
                    });
                }
                finished
                    .catch(&ctx)
                    .map_err(|error| threw(handler, &error.to_string(), clock, limits))?
            }
            None => answered.clone(),
        };
        from_js(&ctx, settled)
    })
}

/// Sets up one isolate, runs the body in it, and takes it down.
///
/// The interrupt handler is installed **before** anything is evaluated, so a
/// module whose top level never returns is stopped by the same clock a handler
/// is — the failure mode is identical and so is the answer.
fn in_isolate<T>(
    limits: Limits,
    named: &str,
    host: impl Host + 'static,
    body: impl FnOnce(Ctx<'_>, &Clock) -> Result<T, HandlerError>,
) -> Result<T, HandlerError> {
    let runtime = Runtime::new().map_err(|error| HandlerError::Evaluation(error.to_string()))?;
    runtime.set_memory_limit(limits.memory_bytes);
    let clock = Clock::new(limits.wall_clock);
    let interrupt = clock.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || interrupt.expired())));

    let context =
        Context::full(&runtime).map_err(|error| HandlerError::Evaluation(error.to_string()))?;
    context.with(|ctx| {
        install_host(&ctx, host, clock.clone())?;
        install_console(&ctx)?;
        match body(ctx.clone(), &clock) {
            Ok(answer) => Ok(answer),
            // Anything that failed *after* the clock ran out failed because of
            // it, whatever the interpreter called the failure on the way up.
            Err(_) if clock.expired() => Err(HandlerError::Interrupted {
                handler: named.to_owned(),
                limit: limits.wall_clock,
            }),
            Err(error) => Err(error),
        }
    })
}

/// Puts [`HOST_GLOBAL`] in place. Takes and answers JSON strings, because a
/// string is the one shape that crosses without this crate learning the
/// vocabulary on either side of it.
fn install_host(
    ctx: &Ctx<'_>,
    host: impl Host + 'static,
    clock: Clock,
) -> Result<(), HandlerError> {
    // The host is owned by the bridge, which is owned by the isolate, which is
    // taken down before the call returns. A handler cannot outlive what it was
    // allowed to reach, because both die together.
    let held = RefCell::new(host);
    let bridge = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, function: String, arguments: String| -> rquickjs::Result<String> {
            let parsed: Json = serde_json::from_str(&arguments).unwrap_or(Json::Null);
            // Off the clock: what the host does here is not the handler's own
            // code, and a door that waits must not spend the budget that bounds
            // a loop. See [`Clock`].
            let answered = clock.waiting(|| held.borrow_mut().call(&function, parsed));
            match answered {
                Ok(answer) => Ok(answer.to_string()),
                Err(refusal) => {
                    Err(ctx.throw(rquickjs::String::from_str(ctx.clone(), &refusal)?.into()))
                }
            }
        },
    )
    .map_err(|error| HandlerError::Evaluation(error.to_string()))?;
    ctx.globals()
        .set(HOST_GLOBAL, bridge)
        .map_err(|error| HandlerError::Evaluation(error.to_string()))
}

/// Gives a handler somewhere to write a line, routed through the host.
///
/// Built in JavaScript over the one bridge rather than as four more Rust
/// functions: it is the same call, and `console.log(a, b)` has to become one
/// message before it can be one anything else.
fn install_console(ctx: &Ctx<'_>) -> Result<(), HandlerError> {
    let levels = CONSOLE_LEVELS
        .iter()
        .map(|level| {
            format!(
                "{level}: (...parts) => {HOST_GLOBAL}(\"console.{level}\", JSON.stringify({{ said: parts.map(String).join(\" \") }}))"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    ctx.eval::<(), _>(format!("globalThis.console = {{ {levels} }};"))
        .map_err(|error| HandlerError::Evaluation(error.to_string()))
}

/// Evaluates the module and calls its default export.
fn register<'js>(ctx: &Ctx<'js>, source: &str) -> Result<Object<'js>, HandlerError> {
    let (module, promise) = Module::declare(ctx.clone(), MODULE_NAME, source)
        .catch(ctx)
        .map_err(|error| HandlerError::Evaluation(error.to_string()))?
        .eval()
        .catch(ctx)
        .map_err(|error| HandlerError::Evaluation(error.to_string()))?;
    promise
        .finish::<()>()
        .catch(ctx)
        .map_err(|error| HandlerError::Evaluation(error.to_string()))?;

    let register: Function = module
        .get::<_, Value>("default")
        .ok()
        .and_then(rquickjs::Value::into_function)
        .ok_or(HandlerError::NoRegister)?;
    let table: Value = register
        .call(())
        .catch(ctx)
        .map_err(|error| HandlerError::Evaluation(error.to_string()))?;
    table.into_object().ok_or(HandlerError::NotATable)
}

/// JSON into the isolate, through the interpreter's own parser rather than a
/// value-by-value conversion — one shape, one set of rules, and no place for a
/// number to change meaning on the way in.
fn to_js<'js>(ctx: &Ctx<'js>, value: &Json) -> Result<Value<'js>, HandlerError> {
    ctx.json_parse(value.to_string())
        .map_err(|error| HandlerError::NotJson(error.to_string()))
}

/// And back out. `undefined` becomes `null`, which is what a handler that
/// returned nothing said.
fn from_js<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Json, HandlerError> {
    if value.is_undefined() {
        return Ok(Json::Null);
    }
    let text = ctx
        .json_stringify(value)
        .map_err(|error| HandlerError::NotJson(error.to_string()))?
        .ok_or_else(|| {
            HandlerError::NotJson("the handler answered with something that is not JSON".to_owned())
        })?
        .to_string()
        .map_err(|error| HandlerError::NotJson(error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| HandlerError::NotJson(error.to_string()))
}

/// Tells apart the three ways a call ends badly, which read identically to the
/// interpreter and differently to a person.
fn threw(handler: &str, message: &str, clock: &Clock, limits: Limits) -> HandlerError {
    if clock.expired() {
        return HandlerError::Interrupted {
            handler: handler.to_owned(),
            limit: limits.wall_clock,
        };
    }
    if message.contains("out of memory") {
        return HandlerError::OutOfMemory {
            handler: handler.to_owned(),
        };
    }
    HandlerError::Threw {
        handler: handler.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::{HandlerError, Host, ISOLATE_GLOBALS, Json, Limits, NoHost, call, declared};

    /// A host that answers one function and remembers being asked.
    struct Spy {
        asked: Vec<String>,
    }

    impl Host for Spy {
        fn call(&mut self, function: &str, arguments: Json) -> Result<Json, String> {
            self.asked.push(function.to_owned());
            match function {
                "memory.read" => Ok(json!({ "title": arguments["key"] })),
                other => Err(format!("`{other}` is not permitted")),
            }
        }
    }

    const MODULE: &str = r#"
        export default function register() {
          return {
            "hello.installed": (payload) => ({ greeted: payload.name, at: "install" }),
            "hello.reads": (payload) => JSON.parse(__syncHost__("memory.read", JSON.stringify(payload))),
            "hello.refused": () => {
              try { __syncHost__("filesystem.read", "{}"); return { caught: false }; }
              catch (error) { return { caught: true, said: String(error) }; }
            },
            "hello.throws": () => { throw new Error("the issue tracker said no"); },
            "hello.hangs": () => { while (true) {} },
            "hello.answers-nothing": () => {},
            "hello.eats": () => { const held = []; for (;;) held.push(new Array(100000).fill(0)); },
            "hello.waits": async (payload) => ({ greeted: payload.name, at: "eventually" }),
            "hello.awaits": async (payload) => {
              const read = await JSON.parse(__syncHost__("memory.read", JSON.stringify(payload)));
              return { title: read.title };
            },
            "hello.rejects": async () => { throw new Error("the tracker said no, eventually"); },
            "hello.never-settles": () => new Promise(() => {}),
            "hello.hangs-async": async () => { while (true) {} },
          };
        }
    "#;

    fn spy() -> Spy {
        Spy { asked: Vec::new() }
    }

    #[test]
    fn a_handler_runs_and_its_answer_comes_back() {
        let answer = call(
            MODULE,
            "hello.installed",
            &json!({ "name": "Sync" }),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer, json!({ "greeted": "Sync", "at": "install" }));
    }

    /// **The measurement this exists for.** Before the promise was unwrapped,
    /// an `async` handler answered `{}` — not an error and not a refusal, an
    /// empty answer, which is the silent failure this product is least allowed
    /// to have. `docs/background.md` §3.1's own example is written this way.
    #[test]
    fn an_async_handler_answers_what_it_settled_to() {
        let answer = call(
            MODULE,
            "hello.waits",
            &json!({ "name": "Sync" }),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer, json!({ "greeted": "Sync", "at": "eventually" }));
    }

    /// `await` over a host call, which is the shape every real handler has once
    /// the surface is written to return promises. Nothing here *waits*: the
    /// host answers synchronously and the promise settles on the first turn of
    /// the job queue. The point is that the author's code is already written
    /// for the day one of them does.
    #[test]
    fn an_async_handler_may_await_the_host() {
        let answer = call(
            MODULE,
            "hello.awaits",
            &json!({ "key": "d-3ad25f" }),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer, json!({ "title": "d-3ad25f" }));
    }

    /// A rejected promise is the author's own error, and it reaches them in
    /// their own words — the same conversation a synchronous `throw` has.
    #[test]
    fn a_rejected_promise_is_the_handler_throwing() {
        let error = call(
            MODULE,
            "hello.rejects",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .expect_err("it rejected");
        let said = error.to_string();
        assert!(matches!(error, HandlerError::Threw { .. }), "{said}");
        assert!(said.contains("the tracker said no"), "{said}");
    }

    /// A promise nothing can settle. The isolate has no timers and no host call
    /// that waits, so the job queue runs dry with the promise still pending —
    /// and the author hears about their own mistake rather than about
    /// `WouldBlock`, which is the interpreter's word for its own condition.
    #[test]
    fn a_promise_nothing_can_settle_is_refused_in_the_author_s_words() {
        let error = call(
            MODULE,
            "hello.never-settles",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .expect_err("nothing settles it");
        let said = error.to_string();
        assert!(matches!(error, HandlerError::NeverSettled { .. }), "{said}");
        assert!(
            said.contains("no timers"),
            "the refusal says why nothing is coming: {said}"
        );
    }

    /// The wall clock is above the job queue, not beside it: a runaway inside
    /// an `async` handler is stopped by exactly the same interrupt as a runaway
    /// in a plain one.
    #[test]
    fn a_runaway_async_handler_is_stopped_by_the_same_clock() {
        let error = call(
            MODULE,
            "hello.hangs-async",
            &json!({}),
            Limits {
                wall_clock: Duration::from_millis(150),
                ..Limits::default()
            },
            spy(),
        )
        .expect_err("it never returns");
        assert!(matches!(error, HandlerError::Interrupted { .. }), "{error}");
    }

    /// A handler that waited on the host answers, however long the wait was.
    ///
    /// The defect this is against has no error in it: the request succeeds, the
    /// value comes back into JavaScript, and the handler is stopped on the next
    /// instruction because the budget ran out while the thread sat in Rust. The
    /// author sees a handler that timed out and an API that was answered — and
    /// no amount of reading their own code explains it.
    #[test]
    fn a_handler_that_waited_on_the_host_is_not_charged_for_the_wait() {
        /// A door that takes its time, as a network one does.
        struct Slow;

        impl Host for Slow {
            fn call(&mut self, _function: &str, _arguments: Json) -> Result<Json, String> {
                std::thread::sleep(Duration::from_millis(200));
                Ok(json!({ "title": "an answer worth waiting for" }))
            }
        }

        let answer = call(
            MODULE,
            "hello.awaits",
            &json!({ "key": "a-record" }),
            Limits {
                wall_clock: Duration::from_millis(50),
                ..Limits::default()
            },
            Slow,
        )
        .expect("the wait is the host's, not the handler's");

        assert_eq!(answer["title"], json!("an answer worth waiting for"));
    }

    /// And the budget it gets back buys nothing for its own code.
    ///
    /// The wait is given back and no more, so a handler that waits and then
    /// loops is stopped by the same clock as one that only loops. Otherwise a
    /// package could buy itself unlimited execution by calling a slow door
    /// first, which is a ceiling an extension raises for itself.
    #[test]
    fn waiting_does_not_buy_a_handler_time_to_run_in() {
        const WAITS_THEN_LOOPS: &str = r#"
            export default function register() {
              return {
                "hello.after": () => {
                  __syncHost__("memory.read", "{}");
                  while (true) {}
                },
              };
            }
        "#;

        struct Slow;

        impl Host for Slow {
            fn call(&mut self, _function: &str, _arguments: Json) -> Result<Json, String> {
                std::thread::sleep(Duration::from_millis(200));
                Ok(Json::Null)
            }
        }

        let error = call(
            WAITS_THEN_LOOPS,
            "hello.after",
            &json!({}),
            Limits {
                wall_clock: Duration::from_millis(50),
                ..Limits::default()
            },
            Slow,
        )
        .expect_err("it never returns");
        assert!(matches!(error, HandlerError::Interrupted { .. }), "{error}");
    }

    #[test]
    fn what_a_module_declares_is_read_by_running_it() {
        let mut names = declared(MODULE, Limits::default()).unwrap();
        names.sort();
        assert_eq!(
            names,
            vec![
                "hello.answers-nothing",
                "hello.awaits",
                "hello.eats",
                "hello.hangs",
                "hello.hangs-async",
                "hello.installed",
                "hello.never-settles",
                "hello.reads",
                "hello.refused",
                "hello.rejects",
                "hello.throws",
                "hello.waits",
            ]
        );
    }

    #[test]
    fn a_handler_reaches_the_host_and_nothing_else() {
        let answer = call(
            MODULE,
            "hello.reads",
            &json!({ "key": "d-3ad25f" }),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer, json!({ "title": "d-3ad25f" }));
    }

    /// A refusal is something a package can be written against: it arrives as
    /// an exception the handler may catch, not as a failed call.
    #[test]
    fn a_refusal_is_catchable() {
        let answer = call(
            MODULE,
            "hello.refused",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer["caught"], json!(true));
        assert!(
            answer["said"].as_str().unwrap().contains("not permitted"),
            "the refusal's own words should reach the handler: {answer}"
        );
    }

    #[test]
    fn what_a_handler_threw_is_carried_through() {
        let error = call(MODULE, "hello.throws", &json!({}), Limits::default(), spy()).unwrap_err();
        match error {
            HandlerError::Threw { handler, message } => {
                assert_eq!(handler, "hello.throws");
                assert!(
                    message.contains("the issue tracker said no"),
                    "the author's own words, not a summary: {message}"
                );
            }
            other => panic!("expected the handler's own error, got {other:?}"),
        }
    }

    /// The one this whole design rests on: an extension that hangs fails its
    /// own call and leaves the process alone.
    #[test]
    fn a_runaway_handler_fails_its_own_call() {
        let limits = Limits {
            wall_clock: Duration::from_millis(150),
            ..Limits::default()
        };
        let began = std::time::Instant::now();
        let error = call(MODULE, "hello.hangs", &json!({}), limits, spy()).unwrap_err();
        let took = began.elapsed();

        assert!(
            matches!(error, HandlerError::Interrupted { .. }),
            "a loop that never yields is stopped by the clock and by nothing else, got {error:?}"
        );
        assert!(
            took < Duration::from_secs(2),
            "it was stopped at its limit rather than eventually: {took:?}"
        );
    }

    #[test]
    fn a_handler_that_eats_memory_fails_its_own_call() {
        let limits = Limits {
            memory_bytes: 2 * 1024 * 1024,
            wall_clock: Duration::from_secs(10),
        };
        let error = call(MODULE, "hello.eats", &json!({}), limits, spy()).unwrap_err();
        assert!(
            matches!(
                error,
                HandlerError::OutOfMemory { .. } | HandlerError::Threw { .. }
            ),
            "the ceiling is the isolate's, not the machine's, got {error:?}"
        );
    }

    /// A handler that returned nothing answered `null`. It is an answer.
    #[test]
    fn answering_nothing_is_null() {
        let answer = call(
            MODULE,
            "hello.answers-nothing",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .unwrap();
        assert_eq!(answer, Json::Null);
    }

    #[test]
    fn a_name_the_module_does_not_have_says_so() {
        let error = call(
            MODULE,
            "hello.imagined",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .unwrap_err();
        assert!(
            matches!(error, HandlerError::NotDeclared(name) if name == "hello.imagined"),
            "the manifest and the module disagree, and the message has to say which name"
        );
    }

    #[test]
    fn a_module_that_is_not_one_is_refused_before_anything_runs() {
        let error = call(
            "this is not JavaScript {",
            "x",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .unwrap_err();
        assert!(
            matches!(error, HandlerError::Evaluation(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn a_module_with_no_register_is_refused() {
        let error = call(
            "export const something = 1;",
            "x",
            &json!({}),
            Limits::default(),
            spy(),
        )
        .unwrap_err();
        assert!(matches!(error, HandlerError::NoRegister), "got {error:?}");
    }

    /// The top level is under the same clock as a handler: a module that never
    /// finishes being read fails the same way and at the same moment.
    #[test]
    fn a_module_that_hangs_while_being_read_is_stopped_too() {
        let limits = Limits {
            wall_clock: Duration::from_millis(150),
            ..Limits::default()
        };
        let error = declared(
            "while (true) {} export default function register() { return {}; }",
            limits,
        )
        .unwrap_err();
        assert!(
            matches!(error, HandlerError::Interrupted { .. }),
            "got {error:?}"
        );
    }

    /// What a fresh isolate per call actually costs, so the decision in the
    /// module docs is a number rather than a hope. Ignored because it is a
    /// measurement, not a claim about behaviour:
    ///
    /// ```text
    /// cargo test -p sync-handlers -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, run it deliberately"]
    fn what_a_fresh_isolate_costs() {
        const RUNS: u32 = 200;
        let began = std::time::Instant::now();
        for _ in 0..RUNS {
            call(
                MODULE,
                "hello.installed",
                &json!({ "name": "Sync" }),
                Limits::default(),
                spy(),
            )
            .unwrap();
        }
        let each = began.elapsed() / RUNS;
        println!("one isolate, module and call: {each:?}");
        assert!(
            each < Duration::from_millis(50),
            "an hourly handler can afford this many times over, but not if it is this slow: {each:?}"
        );
    }

    /// There is no `console` in the isolate, and an author reaches for one in
    /// the first hour. It is given as a host call so that where a line goes is
    /// the host's decision and not the isolate's.
    #[test]
    fn a_handler_can_write_a_line_and_the_host_decides_where_it_goes() {
        struct Listening {
            heard: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        }
        impl Host for Listening {
            fn call(&mut self, function: &str, arguments: Json) -> Result<Json, String> {
                self.heard.borrow_mut().push(format!(
                    "{function}: {}",
                    arguments["said"].as_str().unwrap_or_default()
                ));
                Ok(Json::Null)
            }
        }

        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        call(
            r#"
                export default function register() {
                  return {
                    "probe.talks": () => {
                      console.log("read", 3, "records");
                      console.warn("nothing to do");
                      return null;
                    },
                  };
                }
            "#,
            "probe.talks",
            &json!({}),
            Limits::default(),
            Listening {
                heard: heard.clone(),
            },
        )
        .unwrap();

        assert_eq!(
            heard.borrow().as_slice(),
            [
                "console.log: read 3 records".to_owned(),
                "console.warn: nothing to do".to_owned()
            ],
            "one message per call, already joined"
        );
    }

    /// The list an author is really writing against, taken from a live isolate
    /// rather than from memory. A newer `QuickJS` that adds or drops a global
    /// fails here, where somebody can go and update what the CLI checks
    /// against, rather than in a package that installs and cannot run.
    #[test]
    fn the_globals_a_handler_gets_are_the_ones_written_down() {
        let live = call(
            r#"
                export default function register() {
                  return {
                    "probe.globals": () => Object.getOwnPropertyNames(globalThis)
                      .filter((name) => !name.startsWith("__") && name !== "console")
                      .sort(),
                  };
                }
            "#,
            "probe.globals",
            &json!({}),
            Limits::default(),
            NoHost,
        )
        .unwrap();

        let live: Vec<String> = serde_json::from_value(live).unwrap();
        let mut written: Vec<String> = ISOLATE_GLOBALS.iter().map(|&n| n.to_owned()).collect();
        written.sort();
        assert_eq!(
            live, written,
            "what the isolate provides and what `sync-ext check` checks against have to be one list"
        );
        assert!(
            !written
                .iter()
                .any(|n| n == "console" || n == "fetch" || n == "setTimeout"),
            "none of these is in the isolate, and the whole point of the list is saying so"
        );
    }

    /// Reading what a module declares must not let it do anything on the way.
    #[test]
    fn reading_a_module_grants_it_nothing() {
        let mut refused = NoHost;
        assert!(refused.call("memory.read", json!({})).is_err());
    }
}
