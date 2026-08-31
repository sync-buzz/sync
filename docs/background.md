# Background work

What an extension does when nobody is looking at it, and how one extension
reaches another.

*This describes what this version does. Where a decision here replaces an
earlier one, the earlier one is named — because a reader who arrives at a rule
that looks arbitrary is owed the thing it was chosen over.*

**This document is the application's side: why background work is shaped as it
is, and how the host does it.** The author's side — the manifest fields, the
module to write, the functions it may call and the limits it runs under — is
[`extensions.md`](extensions.md) §5a, and how two packages cooperate is §5b
there. Neither repeats the other: this one argues, that one instructs.

Read [`extensions.md`](extensions.md) first for what a package is and where it
may appear, and [`architecture.md`](architecture.md) for the process model.

---

## 1. What this is, and what it is not

Two capabilities were asked for: **periodic work**, and **a way for one
extension to hand work to another**. Both were described as systems. They are
not: they are two *occasions* for one mechanism, and building them as systems
would give the product two paths for doing the same thing.

The mechanism is a **handler** — a function an extension declares and the host
calls. The clock calls it. Another extension calls it. An agent calls it.
Installing the package calls it. What differs between those is the occasion, not
the machinery.

This is a shape that was designed once and then shelved. An earlier round of
research drew it as `invoke(id, fn, payload)`; `extensions.md`
§12 deferred it with a condition attached — *"a script becomes an agent tool
when an extension wants an action rather than a screen, and none does yet"*. One
does now, so the condition is met rather than broken.

**What this is not.** It is not a task queue, not a message bus, not a workflow
engine, and not a second place where product logic lives. Nothing here schedules
retries, fans out, or persists a graph of pending steps. A handler runs, it
returns or it fails, and what survives it is either a record or a running agent
session.

---

## 2. The vocabulary

Used exactly, here and in the code.

| Term | What it means | Not to be called |
| --- | --- | --- |
| **Handler** | A function a package declares in its manifest and ships in its service module. The host calls it; it runs, returns and is gone. | hook, callback, listener, worker |
| **Occasion** | Why a handler was called: the clock, another extension, an agent, a lifecycle moment. | event, trigger, message |
| **Service module** | The bundle a package's handlers live in — `service/index.js`, beside `ui/index.js` and equally optional. | background script, daemon, worker |
| **Work** | Something long-running the host performs on an extension's behalf and that outlives the caller. Today there is exactly one kind: an agent session. | task, job |
| **Source** | Who asked for a piece of work: a person, a handler, the clock, or an agent. Carried with the work for its whole life. | owner, requester |

A handler is not work. A handler is a call that lasts milliseconds and may
*order* work that lasts hours.

---

## 3. The service module

### 3.1 The shape

A package may ship a second bundle:

```
manifest.json
ui/index.js         the screen — optional, and already so
ui/index.css
service/index.js    the handlers — optional, and the new half
types/*.json
prompt/instructions.md
META/…
```

It exports one function, symmetrical with `activate`:

```ts
import { memory, type Handlers } from "@sync-buzz/extension-api/service"

export default function register(): Handlers {
  return {
    "issues.poll": async (payload) => {
      const listing = await memory.list({ limit: 3 })
      …
    },
  }
}
```

**The surface is imported, not handed in.** An earlier draft of this section
gave a handler a second argument — `(payload, sync)`. It is retired: two ways to
reach one thing is what this product refuses everywhere else, a UI module
imports `Panel` rather than being given one, and a second argument cannot be
typed without the author writing its type out by hand.

**It is TypeScript, and the same CLI builds it.** `sync-ext build` compiles
`src/service.ts` into the file the manifest names, exactly as it compiles
`src/index.tsx` into the one `ui` names. Until 2026-08-25 it did not — it built
the screen and returned early for everything else — so §3.2's promise that an
author "writes TypeScript in another file of the same package" was false, and
handlers were written as JavaScript by hand against a bare global.

**Everything on the surface answers a promise, and today nothing waits.** Every
host function is synchronous inside Sync, so a promise over one settles on the
first turn of the isolate's job queue. They are promises anyway, so that the day
one of them genuinely waits — the network is the case — no package is rewritten.
That cost one thing to be true: `sync_handlers::call` settles a returned promise
before taking the answer. Measured before it did, an `async` handler answered
`{}` — not an error, not a refusal, an empty answer.

Every key is declared in the manifest, and `sync-ext check` runs the module
against a stand-in host and compares what came back with what was declared —
the same check that already catches an area renamed in one place and not the
other. A handler declared and not returned fails the build; one returned and not
declared is refused at install.

**A package with no screen may have handlers, and a package with no handlers may
have a screen.** Project memory proves the first half is needed: it reaches a
project without a line of it executing. Neither bundle is a default, because a
default here means shipping an empty module whose only reader is the packer.

### 3.2 Why JavaScript in Rust, and not wasm

*The wasm runtime had been deferred rather than decided, and the research
behind that deferral recommended one. **Wasm is rejected, not postponed.***

Closing it costs nothing to remove: there is no `wasmtime`, no `extism` and no
`WebAssembly` anywhere in this repository — not in a manifest, not in a
dependency, not in a line of code. What is being ended is a decision that was
sitting in the deferred column, and a deferred decision is a place somebody
returns to. This is the return, and the answer is no.

That research recommended wasmtime with the Component Model, and rejected
running wasm inside the webview because that
"makes agent-callable scripts (which have no webview) impossible" — which is
exactly the three-in-the-morning case this document exists for. That rejection
stands. The recommendation does not, and the reason is that the world it was
written for did not arrive.

It assumed extensions written in any language and compiled to wasm. What
actually shipped is TypeScript: `@sync-buzz/extension-api`, `sync-ext build`,
React components, a published `.api.md` report. A wasm component would make an
author of a package that polls issues learn **two languages** — TypeScript for
the screen, Rust for the background — and buy a componentize toolchain to do it.
That price is not paid by the product; it is paid by the one person the system
exists to serve.

So the proposal is **QuickJS embedded in Rust** (`rquickjs`): the author writes
TypeScript in another file of the same package, the same CLI builds it, and the
host evaluates it with nothing available but what was explicitly handed in.

**Measured 2026-08-25**, because the objection to it was cost and cost is not an
argument, it is a number. All figures are release builds, `opt-level = "z"`,
LTO, stripped, on this machine:

| | Binary | Over baseline | Cold build |
| --- | --- | --- | --- |
| Empty Rust binary | 286 KB | — | 2 s |
| `rquickjs`, synchronous | 827 KB | **+528 KB** | 21 s |
| `rquickjs`, async + tokio + interrupt handler | 945 KB | **+643 KB** | ~30 s |
| `wasmtime`, minimal — no WASI, no components | 4.4 MB | **+4.03 MB** | 1 m 26 s |

Three behaviours were run rather than assumed:

- **A runaway handler fails its own call.** `while (true) {}` under an interrupt
  handler stopped after 199 ms; the call returned an error and the process was
  unharmed. The interrupt limit is mandatory for exactly this reason, and it is
  one closure.
- **A handler can await the host.** `await readRecord(key)` in JavaScript
  reached an `async` Rust function and came back with its value. That is the
  shape every real handler has, and it is the one thing that would have been
  fatal to discover late.
- **The memory cap is one line.** `set_memory_limit` before anything is
  evaluated.

Total integration code for all three: about thirty lines.

**What is given up, stated rather than hidden.** Multi-language extensions are
closed off — an author who wants Rust or Go has no way in. That was the headline
argument for wasm, and it is being traded for the language the extension
ecosystem actually uses. If a package one day genuinely needs another language,
the escape hatch is an MCP server the application spawns, which is a different
contract and not a redesign of this one. QuickJS is also not a security boundary in the way a wasm sandbox is —
it is an isolate. What makes it safe here is that nothing is ambient (§5): the
handler can reach only the functions handed to it, and those are gated by
declared permission.

**And the runtime is neither Node nor a browser, which an author has to be told
rather than left to discover.** Measured on the same build:

```
typeof WebAssembly  -> undefined      typeof JSON   -> object
typeof fetch        -> undefined      typeof BigInt -> function
typeof setTimeout   -> undefined      new Date(0).toISOString() -> 1970-01-01T00:00:00.000Z
typeof TextDecoder  -> undefined
typeof Intl         -> undefined
```

Two consequences follow. Rejecting wasm also rejects **wasm inside JavaScript**:
a package built around `tree-sitter`, `esbuild` or anything else that ships a
`.wasm` payload will not run as a handler. And every convenience an author
expects — timers, text decoding, a fetch — is a function the host decides to
hand over, one at a time, with a permission behind it where it needs one. That
is not a gap in the isolate; it *is* the isolate. But it means `sync-ext check`
has to fail a service module that reaches for a global which is not there, in
the author's own terminal, rather than at three in the morning on somebody
else's machine.

### 3.3 What a handler is handed

What a handler imports is a surface of its own, and it is deliberately not the
window's. `@sync-buzz/extension-api` publishes React components, panels and
hooks; none of that means anything without a document. The service surface is
functions over data, versioned on the same clock and by the same rules as the UI
one, reported by API Extractor and checked in CI the same way.

What a handler may call:

- **memory** — `memory.record`, `memory.list` and `memory.content`: read the
  project's corpus. The same operations `sync-mcp` already curates, not a second
  vocabulary. Reading only — a handler that has found something worth recording
  orders work, and whoever performs it writes through their own door.
- **work** — `work.order`: order a piece of work and read the state of one this
  extension ordered. §6.
- **vault** — `vault.read`, `vault.write` and `vault.forget`: the package's own
  secrets, in its own namespace and behind one capability. Below.
- **net** — `net.fetch`: one request against the hosts the manifest declared
  (`extensions.md` §4), made in Rust.
- **console** — routed to the host's log rather than to a terminal nobody is
  watching.

Nothing else. A handler that reaches for anything outside that list is refused
by name, and the refusal is a test rather than a promise: the surface is an
allow-list, so a capability that has not been built is not a gap somebody can
fall through.

No filesystem, no process launching, no access to another extension's records,
and no way to reach the window — a handler that could draw would be a second UI
path with none of the first one's rules.

**The last two are the same doors the window half has, not second ones.** A
tool an agent calls runs with no screen mounted, so a package that could reach
an API from its own panel and not from its handler would be a package whose
permissions depended on whether somebody was looking. One implementation
answers both halves: the same host list, the same secret namespace, the same
words when either is refused. A second implementation here is what would let
the two come to disagree, and the disagreement would be about *which hosts* and
*whose secrets*.

**A secret is never handed to an agent.** Not over MCP, not in the environment
an agent is started with, not in a prompt, and not in a configuration file Sync
writes for it. The package is the one participant a value is given to, and what
it offers an agent is a *function that uses* the secret — sign this, fetch that
— never the secret itself. Sync does not build the door that would hand one
over, and an author who builds it themselves has decided to, which is a
different thing from being handed it ready-made. The recommended path avoids
holding a value at all: `net.secrets` in the manifest names an entry and Rust
puts it in the header (`extensions.md` §4).

**A value a handler read does not reach the host's log.** `console` goes
somewhere that outlives the window, the afternoon and usually the debugging,
and an author who prints a token while working out why an API said no is
forgetting rather than abusing anything. The host knows every value it handed
over, so it takes them back out of what it prints and says where one was. That
is a mechanism rather than a rule, because a rule here would be advice nobody
is reading at the moment it matters.

---

## 4. Occasions

Three, and each is a different answer to *why is this handler running*. The
handler is told which one it was.

### 4.1 The clock

The only genuinely new machinery in this document. A package declares periodic
handlers in its manifest:

```json
"schedule": [
  {
    "handler": "issues.poll",
    "description": "Checks the tracker for new issues",
    "every": "1h"
  }
]
```

`description` is **required**, and it is the one thing on that row the package
gets to write. The extension's page has to tell somebody what they are agreeing
to let run while they are asleep, and it can work out *how often* from `every`
and *what for* from nothing at all: the handler's own name is the package's
internal name for one of its own functions and is deliberately not shown, for
the reason a type's identifier is not in the navigator's tooltip. So the row is
written by two authors and each says only what it knows — the package what it
does, the host how often, from `every`. A sentence about frequency that the
package had written could disagree with the frequency the clock actually uses,
and that is the two-lists-one-truth shape this repository has paid for twice.

It was made mandatory in the same change that shipped the capability, which is
the only moment it could be: until then nothing could install a schedule at all,
so no package declared one, and a field made required after the first one exists
breaks it.

The host keeps the clock, in Rust, in the process that already survives every
window being closed. Interval rather than cron: a product that lets a package
say *at 03:00 on weekdays* has bought a timezone question, a
missed-while-asleep question and a syntax to document, and no case here needs
any of them. Drift is not corrected and lateness is not made up for — a machine
that was asleep for six hours runs the handler once when it wakes, not six
times.

**As built** (`src-tauri/src/schedule.rs`): a thread of its own rather than a
task on the async runtime, because every part of a tick blocks — it reads files,
evaluates JavaScript and may reach the engine — and blocking work on that
runtime is a mistake this repository has already made once. It looks once a minute,
which is the shortest interval a manifest can express, so a clock looking less
often would be quietly refusing what it accepted.

**One handler at a time, for the whole machine.** That is the ceiling on
concurrent calls §5 asks for, and it is the shape rather than a number: one
loop, so a handler cannot be re-entered while it is running and two packages
cannot compete for the engine at three in the morning.

**A handler that has never run is overdue, not new.** The same case as a machine
that was asleep — the interval says how often, and nothing has happened for
longer than that. It is also what makes an extension somebody has just installed
prove itself within the minute rather than at this time tomorrow.

**The stamp records the attempt, not the success.** A handler that failed failed
(§9) and waits its interval like any other. Stamping only successes would make
the clock retry a broken handler every minute, which is a retry policy arrived
at by accident. Measured on the running application: a handler that throws is
reported once, with the package's id in front of it, and is silent until its
interval comes round.

#### Ticking for a project no window has open

This is the case that decides whether the clock is worth having: an application
sitting in the menu bar with no project open at all. The obstacle is not the
clock — it is that **what a project declares is written in that project's own
memory**, so finding out whether anything is scheduled appears to require
opening every repository to ask.

It does not, for two reasons.

**What the project declares is remembered when it is opened.** Sync already
reads `installed` at that moment, and writing *this path, these extension ids*
into the installation's own configuration costs nothing extra and is read by the
clock without any repository being touched. It is the same shape as a declared
badge, where the manifest carries the question and the host answers it with no
code running, and the same shape as `recent-projects.json`, which is already a
fact about the installation rather than about any project.

**Only that, and not the handler or the interval.** An earlier draft of this
section said to remember the derivative — path, handler, interval — and that was
wrong about where a manifest lives. A package is unpacked into this
installation's own store and `extensions/refs/<id>.json` says which artefact
serves an id right now (`sync-extensions/store.rs`), so the manifest is already
on this machine; the only thing genuinely inside the repository is the list of
declared ids. Remembering the interval would therefore be a copy of data the
clock can read for itself, and one that goes stale twice over: the artefact
pointer is machine-wide, so updating a package leaves every remembered
derivative naming yesterday's interval until each project is opened again, and a
package installed from a folder is one being written right now — its manifest
changes between one tick and the next. The clock resolves the manifest when it
ticks. One truth, read where it lives.

**And running a handler does not cost a copy of the engine.** There are two
process shapes in this product today: the window keeps one client per open
project (`memory.rs` — `HashMap<PathBuf, MemoryClient>`, each entry spawning a
sidecar), while the server agents connect to is **one process for every project
on the machine**, with `project` as the first argument of every call. The clock
takes the second shape. One engine, one loaded model, the project named per
call — so ten scheduled projects cost one process, not ten.

A project that has **never been opened in this installation** does not tick, and
that is honest rather than a limitation: nothing here knows what it declares,
and pretending to would mean opening repositories to find out.

**The list of scheduled projects is its own, not a view of the recents.**
`recent-projects.json` holds eight (`project.rs`), so a ninth project would stop
ticking silently — its owner would find out by noticing that something had
stopped happening, which is the worst way anybody finds out anything. The
scheduled list is added to when a project with a schedule is opened, and nothing
evicts from it but a person.

**Nothing is asked twice.** An earlier draft had the person mark each project
for background work. That is a second question about something already
answered: the extension declaring `schedule` was installed *into this project*
by somebody who was shown, on the card, that it runs on a clock and spends an
agent. Asking again would be the application forgetting rather than being
careful. What replaces the mark is a **switch, not a gate** — §5.

### 4.2 An agent

An agent calls a tool a package declared, by its full name — `<id>.<tool>`.

**One tool in the catalogue, not one per contribution.** `sync_call` takes the
name and the arguments; what the project's extensions offer is read on demand,
from `sync_project` for the names and from `sync_instructions` with the topic
`extension:<id>` for what each one does and takes. The reason is the agent's
context: every entry in a catalogue is paid for in tokens by every agent on
every turn, including the ones that will never call it, so a project with four
extensions offering three tools each would cost twelve descriptions and twelve
schemas to an agent that asked about none of them.

The cost of that, named: a client cannot check the arguments before sending
them, because the schema is not in the catalogue entry. `sync-mcp` holds it
instead and checks against it, and its refusal names what exists and which topic
describes it — which is more than a client's own check could have said.

**The body runs in Sync, not in `sync-mcp`.** A tool reaches the keychain, the
hosts its manifest declared, the artefact on this machine and `work.order`, and
none of those exist in the engine's process. So the engine decides whether the
call may be made — the project declares that extension, it offers that tool, the
arguments fit its schema — and the application runs it, over a channel the
application holds open from start-up. Sync spawns the engine and outlives it, so
the call travels back down a connection Sync made rather than to a door of ours.

The wait ends. A tool that has not answered within a minute is refused in words:
by then the agent's own client has almost certainly abandoned the call, and an
answer nobody is waiting for holds the machine's one handler at a time.

### 4.3 Lifecycle

Installed, removed, project opened. Small, and each is here because something
concrete needs it: a package that wants to seed a type's first records, one that
must clean up when it leaves, one that has to reconcile after a fetch. All three
are refusable — a handler that fails at install fails the install, and says why.

---

## 5. Permissions and limits

**Declared in the manifest, approved once at install, re-approved when a newer
version widens them.** The catalogue is already the place where a person decides what a project
would be agreeing to: the types it publishes, what it tells an agent, and now
what it does when nobody is watching.

- `background` — may declare handlers at all. Without it a service module is
  refused rather than ignored.
- `schedule` — may ask for the clock.
- `work.agent` — may order work that runs an agent. This is the expensive one
  and it is named separately for that reason: it spends somebody's tokens while
  they are asleep, and the card must say so before install, not after the first
  bill.
- `net` — may reach the hosts the manifest names, through Rust. The list is the
  permission; the capability without one is refused at parse.
- `net.write` — may use a verb that changes something where it reaches. Reading
  somebody's tracker and filing in it are two different agreements, and this is
  the second (`extensions.md` §4).
- `vault` — may read, replace and remove secrets in its own namespace. One
  agreement for all three, because the flow that needs any of them needs the
  rest: a package that signs somebody in ends up holding a token nobody could
  have typed, and refreshes it before it expires.
- `agent.tools` — may offer an agent tools to call.

**Limits are mandatory, not configurable by the package.** A wall-clock timeout
per call, a memory cap, a ceiling on concurrent handler calls, and one on how
often a handler may be re-entered. An extension that hangs fails its call, and
nothing else. All four were measured to be available in §3.2.

The numbers this build uses are stated once, in `extensions.md` §5a, so that an
author reads them where they are writing against them. The reason they are the
host's rather than the package's is here: an extension that could raise its own
ceiling has no ceiling.

### 5.1 Where a person sees it, and turns it off

**On the extension's own page**, in the area that already exists for exactly
this. Selecting `Extensions` turns all three columns over to them, and an
extension's page is already *what it does, what it adds to this window, the
types it would publish, what it tells an agent*. What it does on a clock belongs
in that list, and so does the switch that stops it.

That page can say it **without running anything**, which is what makes it work
for a project whose sections nobody has opened: the schedule is declared in the
manifest and the on/off state is the host's, so both are readable with no module
evaluated — the same property that makes a declared badge count for a section
nobody has visited.

This adds no section to the window and no control to a header. It is one more
thing an existing page says about an existing package, which is the only kind of
addition this document is entitled to make.

What running work looks like — where a person sees that an agent is going right
now — is Chat's (§7). There is no view of every project's activity at once;
Chat is per project, as the window is.

**As built**, in the section the page already had — *What it does with no
screen* — one row per scheduled handler, and the switch on the trailing edge of
the first of them. Two segments, `Off` and `On`, rather than a rocker: this
window has no switch of its own, and a lone one built here would be the only
control in the application drawn that way. The settings window reached the same
conclusion for the same reason and uses the same two segments.

Off is not drawn as a quieter version of the same sentence. The section is
titled with what the package *does*, and a package whose clock somebody stopped
is not doing this — so the row says `Off for this project` instead of
`with or without a window open`, and goes to the tertiary tier.

**What is stored is the exception, never the rule.** A project that has switched
nothing off has no entry at all, and turning a clock back on removes the
exception rather than writing a `true`. A file that listed every extension as
switched on would be the second consent this design refuses, written down — and
a project would stop ticking the day something failed to write a `true` nobody
had asked for.

**The page says what the package does, not what it has done.** The last-run
stamp exists and the window does not read it: a fact like *ran at 03:12* goes
stale while somebody is looking at it, and this page has nothing that refreshes.

**The switch is one per extension per project.** A package with two scheduled
handlers gets two rows and one control — it is one extension's clock in one
project, and a second copy of the control further down the list would read as a
second question.

---

## 6. Work

### 6.1 Most of it already exists

The one thing this document does *not* have to invent is the long-running half,
because `src-tauri/src/sessions/` already carries it and carries it in three
layers that are exactly the right three:

| Layer | What | Where it lives | Survives |
| --- | --- | --- | --- |
| **Live session** | the conversation happening now | a map in the application | nothing — keys are minted from zero each launch |
| **Pointer** | which agent, which directory, the agent's own session id | `conversations.json`, in this installation's configuration | quitting the application |
| **Record** | the conversation somebody decided to keep | the corpus, as `chat.conversation` | everything, and travels with the repository |

`session_resume` raises the agent and asks for the session back through
`session/load`, which **all four verified agents support**
(`acp-client/src/capabilities.rs`). The transcript comes back with it.

So "what happens when the application is closed mid-work" — asked as though it
needed a new mechanism — has one already, for the only kind of work there is.

### 6.2 Ordering work

A handler orders work; it does not perform it. The host performs it, because the
host is what outlives the handler:

```ts
const key = await sync.work.order({
  kind: "agent.session",
  agent: "claude-code",
  prompt: { text, attachments, images },
  onInterrupted: "continue" | "wait",
})
```

The payload is not invented: `prompt(text, attachments, images)` is what the
agent session layer already takes — text, absolute paths the agent reads itself,
and pasted pictures that have no path. **That is also the answer to how images
cross**, for the case where the source has one. A source with neither a file
nor a window cannot send one at all, which is the honest answer rather than a
placeholder for a better one.

`kind` is a registry, and today it has one entry. A second arrives with a second
executor, not before: a registry designed around one known case and four
imagined ones is four guesses shipped as a contract.

**As built**, `work.order` answers a key **before any of it has happened** — the
handler that called it is finished within milliseconds and the agent may run for
hours, so what is synchronous is minting the key and writing the order down, and
raising the agent goes onto the application's runtime. The key names the *order*,
not the conversation: there is no conversation yet when the handler is handed it,
and there may never be one if the agent will not start.

Two things the payload does not carry. There are no `images`: a handler has no
clipboard and no filesystem, so nothing could fill the field — the sentence above
about how images cross is about a source that is a person, and that source is the
window's prompt. And there is no `cwd`: the project is the one the handler was
called for, which is the host's to know, the same division that makes a scheduled
row's sentence two authors'.

`work:agent` is spelled **`work.agent`** in the manifest, with the dot every
unparameterised capability uses (`agents.acp`, `markdown.plugins`); the colon in
`net:<host>` separates a parameter, and this one has none. It is the first
capability enforced when the call is made rather than when the manifest is read,
because a manifest cannot show whether the JavaScript calls it. So the host
refuses the call — catchably, naming the capability — and `sync-ext check` scans
the built module for the same thing, which is the earliest an author can be told.
The scan has the limits every scan here has, and it was measured before it was
written: a package that does not import `work` does not carry it, because the
surface's unused members are dropped by the build.

### 6.3 The source travels with the work

Every piece of work carries **who ordered it** — a person, an extension and its
handler, the clock, or an agent — and **what it was about**, as a record key
where there is one. It is set when the work is ordered and never edited.

This is the field that makes everything else legible: Chat shows it, and the
person who wakes to find an agent has been working can see
what asked it to.

### 6.4 Interruption

**The source decides, when it orders.** `onInterrupted: "continue"` for the
nightly poll that should finish without anybody; `"wait"` for a conversation a
person started and would want to pick up themselves. The system guesses nothing,
because the two cases genuinely differ and no default is right for both.

What "continue" does is resume the session and take the next turn. What "wait"
does is leave it resumable and say so.

**The choice is recorded, and nothing acts on it.** It is required all the same,
and from the first day, because this is the only moment it exists to be captured
and a field made required after the first package ships breaks that package.
Nothing resumes a session except a person pressing for it: "take the next turn"
means choosing what to say to an agent, which is a decision worth making where
its result can be seen rather than one to make on somebody's behalf at launch.

---

## 7. Chat is the window onto every session

Chat was built as a way to talk to an agent. It is also the only screen that
shows what an agent is doing, so it is the screen that shows **every** session,
whoever ordered it — not only the ones a person started by typing.

Two sources, and they answer different questions, which is the same composition
the badge already makes and for the same reason:

- **Live sessions** — what is happening now. `useLiveSessions`.
- **Pointers** — what can be continued, including from a previous run of the
  application. `rememberedConversations`.

To which is added the source (§6.3), so a row can say that this conversation was
begun by the issues poll at 03:00 and not by anybody at the keyboard.

**Running work does not become a record.** `keeping.tsx` states the rule and the
reason: a corpus filling with unreviewed transcripts makes every honest record in
it count for less, so a conversation becomes a record when a person says it is
worth keeping. Work ordered by a handler is no different — nothing writes itself
into the project's memory because it ran.

What the area looks like is Chat's own business, and it is an extension, so it
is its author's decision and not this document's.

**As built**, that decision was **a group per record the work is under**, not a
caption on the row. It was argued the other way first — Login Items names the
responsible application on the row rather than grouping by it, Finder makes
"Group By" a view option, and Mail keeps the sender as a field — and the
counter-argument won on one point: **a caption has to be read row by row, a
heading answers once.** Somebody who set an extension working on five tickets is
watching five conversations that belong to one thing, which is Notification
Centre's condition for grouping rather than Login Items'.

The heading named the *extension* first, and the record is the better answer for
a reason the extension could not give: **a conversation somebody opened from a
record has no orderer at all.** Pressing `Send to agent` on a task is a person
doing it, so there is no source to group by, and every one of those landed in
the undifferentiated heap — which is most of what a section that hands work to
an agent produces. A record also says more than the package that ordered it to
somebody scanning the column, and it is a heading that can be opened.

So a session carries two fields rather than one, and they answer two questions:
`source` is who asked, `about` is which record the work is under. Only the first
has a person as an ordinary answer, which is exactly why collapsing them left
the second unanswerable for the conversations that most needed it.

Three things make the split safe, and two of them were paid for before:

- **Groups are ordered by their newest conversation, and rows within them by
  their own age.** Splitting the list must not cost the one order it always had,
  so "what happened last" is still the top of the top group.
- **What a conversation is about cannot change.** Chat had two groups once —
  `Running` and `Not running` — and they were reverted because a conversation
  changed group the moment it was continued, appearing in one before it left the
  other. `about` is set when the conversation is opened and never edited, so a
  row stays where it is. That is the difference between the split that failed
  and this one.
- **A heading is a name, not an address.** `about` carries the record's kind and
  its title beside the key: the title so a heading is drawn without reading the
  corpus once per row of a list that is polled every few seconds — the bargain
  `extension_name` and `agent_name` already make — and the kind because opening
  a record takes both, since which section shows one is decided by its type.

What is under no record stays under `Conversations` and leads. Nothing on the
row repeats the heading: the group says which record, and the row says which
agent and how it is getting on. Opening the record is the heading's secondary
click, because its plain click is spent — it collapses the group, which is what
a heading in a source list on this system does.

---

## 8. Which side each thing belongs to

Stated before anything is built, because a fix written on the wrong side has to
be written again for the next storage engine.

**Sync's**, because they are about what a person sees and decides, or about a
process this application owns:

- the service module, its runtime, its limits and its permissions
- the clock, the remembered declarations it reads, and the list of projects that
  tick
- ordering work, performing it, attributing it, resuming it
- what Chat shows, and where anything appears in the window

**memory-hub's**, because they are about how records are stored and read:

- a type that derives no freshness
- a type that is service by declaration
- a store, or a ref, that does not travel

---

## 9. Deliberately absent

Named so they are not re-proposed:

- **A retry policy.** A handler that failed failed. Whether to run it again is
  the next occasion's business, and a framework that retried on its own would be
  making decisions about somebody's tokens.
- **A workflow graph.** No fan-out, no dependencies between pieces of work, no
  step that waits for two others. If that turns out to be needed, it is an
  extension's own problem to solve with handlers, not the shell's to provide.
- **Cron syntax.** §4.1.
- **A second UI path.** Handlers cannot draw.
- **Multi-language service modules.** §3.2, with the escape hatch named.
- **Sandboxing beyond the isolate.** What protects the machine is that nothing
  is ambient, not that the interpreter is a jail. A package that is trusted
  enough to install is trusted to run its own handlers within its declared
  permissions.

---
