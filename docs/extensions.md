# Extensions

Sync is a shell. What a project can do is what that project has installed, and
everything installable arrives as a package this repository did not build. This
describes what a package is, how the window decides it is allowed to run one,
where packages come from, and what happens when a newer one exists.

Read [`architecture.md`](architecture.md) first for the window, the process
model and the memory integration. This document is about the seam between them
and code the application did not compile.

This describes what this version does. Where a rule here replaces an earlier
one, the earlier one is named — a reader who arrives at something that looks
arbitrary is owed the thing it was chosen over. §12 is what the version
deliberately does not do.

## 1. No extension ships inside the application

There is no `src/extensions/` directory. Records, Chat and Project memory are
packages like any other: built in the `sync-extensions` repository, delivered as
archives, installed through the same path a stranger's extension is installed
through. Project memory went first because it has no code at all — it exercises
everything about a package reaching a project except loading a module — and the
other two followed without a line of their own changing, because their imports
already went through the boundary.

`SHELL_AREAS`, `AREA_IMPLEMENTATIONS` and the `ShellAreaId` union are gone. A
section is now whatever a loaded manifest declared and a module returned, keyed
by `<extension>/<area>` — a string the shell has never seen and cannot have an
opinion about. `PROJECT_TYPES` went with them, into `opens.projectTypes`, and
`FETCHES_DEPENDENCIES` into `dependencies.npm`.

The `EXTENSIONS` catalogue constant is gone too, and with it the last place
the shell wrote an extension's name, summary and description down. The
catalogue now joins two questions — what is unpacked on this machine, and what
does the project declare — and every word on a card comes out of a package.

**A rule a compiler cannot hold is held by CI.** Nothing stops somebody adding
`src/extensions/` back and importing from it, and the failure would look like a
feature until the day an extension had to be loaded from a package. So a job
refuses a tree that has the directory at all, or any code that resolves a path
into it. The lint rule that used to restrict what such code could import is
retired: it was the best a repository could do about code it contains, and it
contains none.

The reason is not symmetry. An extension compiled into the application is one
refactor away from being part of it — a helper lifted into `lib/` because two
areas wanted it, an `if (id === "chat")` in a loader, a type union in the shell
that happens to list every extension the build has. Each of those is reasonable
on its own and together they are the thing this design exists to prevent: an
extension system that only works for extensions we wrote.

So the rule is stated as a property of the code rather than as a habit:

> **The core cannot name an extension.** Not in a constant, not in a type, not
> in a conditional.

And it is held by the compiler rather than by review. `ShellAreaId` stops being
a union of literals and becomes `string`; `AREA_IMPLEMENTATIONS`, `SHELL_AREAS`
and the `EXTENSIONS` catalogue constant are deleted rather than emptied. An area
is whatever a loaded manifest declared, an opener is whatever a loaded manifest
claimed a kind for, and there is no place left where an identifier could be
written down. What survives that deletion is a lookup over data; what does not
survive it was the leak.

Two places in the shell name an extension today and are the measure of the work:
`opening.ts` holds `PROJECT_TYPES = "records"` — which extension opens the
project's own types — and `composition.ts` holds `FETCHES_DEPENDENCIES =
{"chat"}` — which extension needs something from npm before it works. Both
become manifest fields. Neither is a special case once it is written down by the
extension that has the property.

## 2. The boundary

An extension sees the application through `src/lib/extension-api/index.ts` and
through nothing else. This used to be a lint rule over `src/extensions/**`,
which is the best a repository can do about code it contains; now it is the
resolver. An extension is built in another repository against the rolled-up
declarations of that one module, so there is no Sync source to reach into and an
import that goes past the contract does not resolve — the rule became a fact
about where the code lives.

The API re-exports the shell's own objects: panel primitives, the source list
and tree, typed marks, the vendored component library, the record editor and
metadata panel, the Markdown renderer, the corpus hook, the folder hooks, the
removal sheets, the native context menu, the menu-bar hook, the agent-session
layer. Two things are deliberately absent and stay absent: **geometry**, which
lives in `shell-layout.ts` and is the window's, and **the shell's own screens**.

What crosses at build time is types; what crosses at runtime is objects. An
extension is compiled against `@sync-buzz/extension-api` with `react` and the API
itself replaced by shims, and it returns one entry per area its manifest
declared:

```js
export default function activate({ id }) {
  return { memory: { Provider, Navigator, Workspace, Inspector } }
}
```

The shims read an object the host publishes on the global —
`__syncExtensionHost__` — **before** it fetches the module, and that ordering is
the whole mechanism. An author writes `import { useState } from "react"`, which
is resolved while the module is being evaluated; there is no way to hand
anything to a module during its own evaluation, so the objects have to be
somewhere it can look. What is passed to `activate` is only the extension's own
id, which is the one thing a module cannot know about itself — an earlier draft
passed React and the surface there as well, from before the shims existed, and
two ways to reach one object is how one of them goes stale.

**`lucide-react` is bundled into the extension**, which reverses the earlier
"marked external so there is one copy". One copy matters where identity does:
React, because of the dispatcher, and the component library, because of portals
and focus traps. An icon is a pure SVG component with neither, so the six an
extension uses cost a couple of kilobytes — while serving the library from the
host would mean the application bundling fifteen hundred icon modules so that an
extension can pick six. What the host does resolve is the icon *names* a
manifest gives — `"icon": "book-marked"` — against its own curated table, with a
neutral mark for a name it does not have.

This is why the component library is **not** extracted into a package of its
own. A package would be a second copy of every portal, focus trap and scroll
lock in the window, and "the same styles" would become "styles that were the
same when both were last published". The published package carries the contract;
the application carries the implementation.

## 3. The API has a version, and it is not the application's

`0.8.0` is the version of a window. What an extension depends on is the shape of
`extension-api`, which changes on a different clock: a release can redraw every
panel without moving it, and a patch release can remove an export.

So the surface carries `SYNC_API_VERSION`, semver, incremented by its own rules:

| Change | Bump |
| --- | --- |
| An export removed, renamed or narrowed; a parameter added to a callback; a returned field dropped | **major** |
| An export added; an optional field added; a widened accepted type | **minor** |
| Nothing in the surface changed | **patch**, or nothing at all |

A manifest states the range it was written against — `"syncApi": "^3.2"` — and
the host checks it **before executing a line of the package**. An extension
outside the range is not loaded, contributes no area, and says so in the
catalogue in the only two forms that are actionable: *needs a newer Sync* or
*was written for an older Sync and has an update*.

**The version is checked by machine, because a version nobody verifies is a
comment.** The surface is extracted with
[API Extractor](https://api-extractor.com) into `api/extension-api.api.md`,
which is committed. `pnpm api:check` runs in CI and fails on two different
things, because there are two ways for the promise to stop being true:

- **the surface moved and the report did not** — someone changed an export and
  did not run `pnpm api:update`;
- **the report moved and the number did not** — the report was regenerated and
  `SYNC_API_VERSION` was left where it was. This is the quiet one: that commit
  has a report matching its own build perfectly, and every package that stated
  a range goes on believing a number that no longer describes anything.

The second is caught by comparing both files against the base branch rather than
against the build, which is why the check needs the repository's history and not
just its working tree. The report is also what the published types package is
built from, so what an author compiles against and what CI compares are one
artefact rather than two that agree by convention.

Setting this up found six types that crossed the boundary without being
exported from it — `MarkdownBlock`'s underlying union, `buttonVariants`,
`MenuRecordType`, `TransactionResult`, `RenameCandidate` and `ProjectSettings`
with the two types it is made of. Each was a value an extension could be handed
and could not name. None of them was visible to `tsc`, to the lint rule, or to
anybody reading the file: they were reachable, they were just anonymous.

## 4. Capabilities answer a different question

Semver answers *is this surface compatible*. It cannot answer *can this build do
the thing* — a platform without a bundled ACP sidecar exposes the same
`useAgentSession` type and cannot raise an agent behind it.

So a build publishes named capabilities and a manifest may require them:
`records`, `agents.acp`, `markdown.plugins`, `native-menu`, `folders`, `sheets`,
`net`, `background`, `schedule`, `work.agent`. A missing capability is a refusal
with a sentence a person can act on, and it is also what lets an extension
degrade deliberately — asking for a capability is a choice, and reading whether
one is present is allowed.

### `net` is a capability and a list, and neither works alone

The other capabilities are one word each, because what they promise is the same
for every package that asks. Reading outside this window is not: *may this
package dial out* and *where to* are two questions, and a person shown only the
first has agreed to something with no edges. So a package that asks for `net`
also writes `net: { hosts: [...] }`, and each of the two without the other is
refused when the manifest is parsed — by the packer, by this crate, and by the
schema, which are the three places a manifest is read.

Each entry is exactly one host: no scheme, no port, no path and **no wildcard**.
`*.example.com` is a family nobody enumerated, and it is the shape every
allow-list is eventually widened by, because the one host that needed it is
indistinguishable in the manifest from the thousand that did not.

The list is read where the request is made — off the artefact on this machine,
in `extension_fetch` — and never taken from the caller. What the window hands a
package is `host.net`, built for that package and closed over its id, so a call
carries which package is making it rather than stating it. Every redirect is
checked again against the same list, so a hop off it is refused as firmly as the
first request, and the surface has no method, no body and no header: a header is
where a token goes, and a token is a further agreement with a person rather than
a field that was already there.

## 5. What a package is

A single zip, extension `.syncext`, registered as a document type so
double-clicking opens the install flow.

```
manifest.json           id, version, syncApi range, capabilities, areas, types, prompt
types/*.json            __type__ definitions, one per file
ui/index.js             the built ESM bundle; host runtime external — and optional
ui/index.css            the rules its own markup uses; no values — and optional
service/index.js        the handlers, for what happens with no screen — planned, and optional
prompt/instructions.md  served to agents as topic extension:<id>
META/hashes.json        path -> sha256 for every file above
META/signature          minisign over the canonical hashes.json
```

**An extension is not necessarily a screen.** `ui` is optional and absent from
the first extension to leave the application: Project memory publishes five
kinds of claim and a prompt, and its whole contribution reaches a project
without a line of it being executed. A default here would have made it ship a
two-line module whose only reader is the packer, so what is declared is what the
package contains — and the manifest refuses the other half of that, a section
declared with no code to draw it.

### Serving `ui/index.js` — measured 2026-08-24

The archive is unpacked outside the bundle, so its files are served to the
window over a URI scheme of the application's own, `syncext://<id>/<path>`, and
the policy names that scheme in `script-src`. What a packaged build does with
that was the one thing in this document nothing could settle by argument, so it
was built and looked at. Four findings, and the second is the one that matters:

1. **The policy is not the obstacle.** With `syncext:` added to `script-src`,
   nothing in CSP refuses the module.
2. **CORS is.** A module fetched from another origin is a cross-origin request,
   and the first attempt failed with *"Cross-origin script load denied by
   Cross-Origin Resource Sharing policy"* — a refusal that names neither the
   policy nor the scheme and reads like a network error. The response has to
   carry `Access-Control-Allow-Origin`. It is the window's own origin rather
   than `*`: the window is served from `tauri://localhost`, an artefact is meant
   for it and for nothing else, and any other origin is answered 403.
3. **There is one React.** The probe module calls `useState` across the module
   boundary and the counter counts, which is what says the host's objects were
   injected rather than a second copy bundled.
4. **The bundler must be told to keep its hands off.** Turbopack resolves a
   dynamic `import()` at build time and there is nothing on disk to resolve —
   the artefact arrives at runtime. `/* turbopackIgnore: true */` (and
   `webpackIgnore` beside it) leaves `import(e)` in the chunk, which is what the
   loader needs.

The fallback that was held in reserve — reading the module in Rust and importing
it as a `blob:` URL, which would widen `script-src` further — is not needed.

### A package carries its own rules and none of its own values

The reason this rule exists is the quietest failure in this document.

**Tailwind generates the classes it finds in the source files it is told to
read.** The window's build reads the window's own `src`; a package is not in it.
So every utility an extension used that the shell did not happen to use as well
produced no rule at all — no error, no warning, nothing in any file to open. The
section mounted, held its state, answered the keyboard, and drew without one of
its own margins. Chat looked redesigned for a fortnight: its close button
positioned `-top-1.5 -right-1.5` sat at the bottom of its chip, because
`position: absolute` existed and the two offsets did not. Twenty-six utilities
were absent from two extensions and every file anybody thought to open was
correct.

So a package names a stylesheet, `sync-ext build` compiles it from the package's
own source, and the host adds it to the document before it loads the module —
before, so the first frame a section draws is already styled. It is served over
`syncext://` like the module and `style-src` names the scheme, which is the same
widening already made for scripts and no larger.

**The division is values against rules, and it is the one this document already
makes for React and for icons.** One copy of anything with identity or a design
in it — React because of the dispatcher, the component library because of
portals and focus traps, the tokens because they are the design. Its own copy of
anything that is a pure rule: an icon, a utility class. `.gap-1\.5` means the
same thing wherever it is compiled, so two packages carrying it cost forty
identical bytes each and cannot come to disagree.

What makes that work is `@theme inline`. The theme the contract publishes is
generated from `globals.css` by `pnpm api:publish`, and every name in it
resolves to a `var()` the window defines on `:root` — so `bg-panel` compiles to
`background-color: var(--surface-panel)` and the package carries no colour.
Retint the window and every extension in it retints, with nothing rebuilt and
nothing republished.

**One thing is closed and it is narrow.** A package may not declare a token —
`--surface-*`, `--spacing`, `--radius-*` and the rest — because a package that
sets one is not styling its section, it is repainting every column, sheet and
menu in the application. `sync-ext check` refuses it. Everything else is the
author's: any utility, any class of their own, any variable under their own
name, and plain CSS for the panel we have no component for. A vocabulary of
permitted utilities was considered and rejected — a list of what we thought of
in advance is a wall in front of exactly the author who needed something we did
not.

**And the second finding was measured in one build out of two.** The probe ran
against a packaged build, where the window's origin is `tauri://localhost`; the
two origins named above are that build's and no other. `tauri dev` does not
serve the window from the bundle at all — it points the webview at `devUrl`, so
the origin is `http://localhost:1420` — and every extension loaded in
development was refused with the very sentence this section warns names neither
the policy nor the scheme. The one loop an extension author works in was the one
loop the mechanism had never been run in, and it stayed that way from the
measurement on 2026-08-24 until the first person opened a window and looked, the
same afternoon.

What that cost is worth stating: §6 says installing from a folder *is what makes
the system usable by anyone outside this repository*, and until this it made it
usable by nobody. The origin a dev server serves the window from is now read out
of the configuration rather than written down a second time, and it is honoured
only in a debug build — `devUrl` survives into a release configuration, and
trusting it there would make a development port a way into a shipped app's
artefact directory. Four tests hold the rule, and one of them reads `devUrl` out
of `tauri.conf.json` so that moving the port moves the test with it.

JSON rather than TOML because four things read the manifest — the packer, CI,
the Rust loader and the window — and one JSON Schema validates it for all of
them. A TOML manifest means a second parser inside the webview and a schema that
is prose.

minisign rather than a hand-rolled ed25519 envelope because the bundle already
verifies minisign: the updater's signing key is the same mechanism, and
`minisign-verify` is already in the dependency tree. One signing story for both
artefacts a person downloads.

The signature covers **id and version**, not only file contents, or a signed
package can be republished under another identifier. The packer normalises mtime
and entry order, so an artefact is reproducible and "build it and compare"
remains available. **Verification is soft in v0**: hashes always gate, the
signature is shown and does not.

What the manifest declares, and what each field replaces:

```json
{
  "manifestVersion": 1,
  "id": "project-memory",
  "version": "1.0.0",
  "engines": { "syncApi": "^1.0" },
  "capabilities": ["records"],
  "name": "Project memory",
  "icon": "book-marked",
  "summary": "…", "description": "…",
  "types": ["types/decision.json"],
  "opens": { "kinds": [], "projectTypes": false },
  "prompt": "prompt/instructions.md"
}
```

That is the whole of an extension that draws nothing. One with a screen adds the
module and the sections it fills, and one that needs something fetched says so:

```json
{
  "requires": { "extensions": ["records@^1"] },
  "areas": [{ "id": "memory", "label": "Memory", "frame": "browse", "icon": "book-marked" }],
  "ui": "ui/index.js",
  "dependencies": { "npm": ["@zed-industries/claude-code-acp"] }
}
```

`icon` is a name rather than a component, so the host resolves it against
`lucide-react` at load. `opens.projectTypes` is the field that retires
`PROJECT_TYPES`; `dependencies.npm` is the field that retires
`FETCHES_DEPENDENCIES`.

## 5a. What a package does with no screen

*Why background work is shaped this way — the runtime chosen and what it was
measured against, the clock's process shape, and where the state lives — is
[`background.md`](background.md). This section is what to write.*

An extension may ship a second module. `ui/index.js` is the screen; `service/index.js`
is the half that runs when no screen is mounted, and a package may have either,
both, or neither.

```
src/index.tsx   ->  ui/index.js        the section, React
src/service.ts  ->  service/index.js   the handlers, no React
```

Both are built by `sync-ext build`, from the same package, in the same language.
That is the point of choosing JavaScript for it: an author who can write a
section can write a handler without learning a second toolchain.

### A handler is not work

It runs for milliseconds and answers. What it may do is **order** work that runs
for hours — the host performs that, because the host is what outlives a handler.
A handler that tried to do the work itself would be killed by its own time limit.

### The module registers what it declares

```ts
import { memory, work, type Handlers } from "@sync-buzz/extension-api/service"

export default function register(): Handlers {
  return {
    "issues.poll": async (payload) => {
      const listing = await memory.list({ kind: "issues.ticket", limit: 20 })
      return { seen: listing.total }
    },
  }
}
```

The default export is called once, and what it answers is a table of handlers by
name. Every name in it must be one an occasion in the manifest calls, and every
name an occasion calls must be in it — `sync-ext check` fails either way round,
because a handler nothing calls will never run and an occasion with nothing
behind it fails at the moment it matters.

### Two occasions, so far

**Install.** `lifecycle.installed` names a handler called when the package is
installed into a project. It is synchronous, a person is in front of it, and a
handler that fails **fails the install** and says why. Use it to seed a type's
first records or to reconcile after a fetch.

**A clock.** `schedule` names a handler and how often:

```json
"schedule": [
  { "handler": "issues.poll", "every": "30m",
    "description": "Checks the tracker for new tickets" }
]
```

`every` is an interval — `15m`, `1h`, `24h` — with a floor of one minute, not a
cron expression: a syntax that can express *the first Monday of a quarter* is a
syntax somebody will express it in, on a machine that was asleep.

**The row a person reads is written by two authors.** The package says *what it
does*, in `description`, because nothing else can know it — and it is required.
The host says *how often*, from `every`, because a package's own sentence about
frequency could disagree with the frequency the clock uses.

The promise is an interval and nothing more. Lateness is not made up for, drift
is not corrected, and a machine asleep for six hours runs a handler once when it
wakes rather than six times.

**A clock runs with no window open**, which is why it is a capability of its own,
and why a person can stop it for one project from the extension's own page
without removing the package.

### Three capabilities, because they are three different agreements

| Capability | What a person is agreeing to |
| --- | --- |
| `background` | this package runs code with no screen mounted |
| `schedule` | it runs while nobody is there |
| `work.agent` | it may raise an agent, which **spends money while they sleep** |

A manifest that ships a service module and does not ask for `background` is
refused when it is read, and so is one that schedules a handler without
`schedule`. `work.agent` cannot be checked that way — whether a handler calls
`work.order` is inside the built JavaScript — so the host refuses the *call*, in
words the handler can catch, and `sync-ext check` scans the built module for it
so an author hears about it in their own terminal instead.

### What a handler may reach

Only what it was handed. Nothing is ambient.

- `memory.record(key)`, `memory.list(query)`, `memory.content(key)` — the
  project's memory, reading only.
- `work.order(order)` — raise an agent on something, and answer with a key
  before any of it has happened.
- `console.*` — a line, which the host places.

Asking for anything else is a refusal naming what *is* offered, which a handler
can catch and carry on from. Writing to the corpus and the network are not
offered by this build; each will arrive with the capability that gates it.

### Ordering work

```ts
const key = await work.order({
  kind: "agent.session",
  agent: "claude",
  title: "Fixing #47: the login redirect loop",
  prompt: { text: "Read issue 47 and propose a fix.", attachments: ["/abs/path.md"] },
  onInterrupted: "continue",
  about: "ticket-47",
})
```

It answers as soon as the order is written down — before the agent has been
raised, and long before it has finished. **The key names the order, not the
conversation**: there is no conversation yet, and there may never be one if the
agent will not start. Keep it: it comes back as `source.work` on every
`SessionRow` the order produced, which is how your own screen says *task 47 is
running* rather than *three things are running*.

`title` is required, and nothing else can supply it. Without one the conversation
is named after the first words said in it — which your handler wrote, to an
agent. A sentence written for an agent standing in for a sentence written for a
list reads exactly like something a person typed.

`onInterrupted` is required too, and there is no default because neither answer
is right for both cases: `"continue"` for a nightly poll that should finish
without anybody, `"wait"` for something a person would want to pick up
themselves.

`attachments` are absolute paths and cross to the agent as resource links. Sync
never opens the file; it names one, and the agent — already running in the
project's folder — opens it itself.

### The isolate is not Node and not a browser

Handlers run in QuickJS, embedded in Sync. Measured, not assumed:

- **Absent**: `fetch`, `setTimeout`, `TextDecoder`, `Intl`, `WebAssembly`.
- **Present**: `JSON`, `Math`, `Date`, `Promise`, `BigInt`, the typed arrays,
  `queueMicrotask`.

`console` is there, and what it writes is the host's to place. Everything else
arrives through the surface above, one function at a time. `sync-ext check`
scans a built module for calls to what the isolate lacks and names each one —
it is a scan, so it sees `fetch(...)` and does not see
`globalThis["fet" + "ch"]`.

**Everything answers a promise, and today nothing waits.** Every function is
settled by the time the job queue turns once, because every one of them is
synchronous inside Sync. They are typed as promises anyway, so the day one of
them genuinely waits, no package has to be rewritten. Write `await`.

### Limits are the host's, not the package's

A wall clock of five seconds per call, sixteen megabytes of memory, and one
handler at a time for the whole machine. They are not configurable by the
package — an extension that could raise its own ceiling has no ceiling — and a
handler that exceeds one fails its own call and nothing else.

---

## 5b. How two extensions work together

**No extension can call another.** There is no message bus, no directory of ids
to dial and no way to ask *the extension called `chat`* for anything. That is a
design decision rather than an omission, and the reason is worth stating: a
package that addresses another by id breaks the day a better implementation of
the same thing arrives. Everything below addresses a **kind of thing** instead,
which survives that day.

Cooperation is real and there are three channels for it. None of them requires
either package to know the other exists.

### 1. A published type is a public interface

This is the main one, and most cooperation needs nothing else.

An extension publishes types into the project's memory — a vocabulary, with
fields, required-ness and relationships. Every kind is namespaced with the
package's id, so Issues publishes `issues.ticket` and nothing else can be
mistaken for it.

**Reading is open.** Any extension reads any record, of any kind, in the project
it is running in — `useCorpus` from a screen, `memory.list` from a handler.
A package that wants to act on tickets asks for records of kind `issues.ticket`.
It does not ask for Issues, does not check whether Issues is installed, and does
nothing differently when a better tracker replaces it under the same kind.

**The type definition is the contract.** It says what a record of that kind has,
and the engine validates every write against it — so a consumer can rely on the
shape without a second agreement about it.

**Records travel with the repository.** This channel survives quitting the
application, cloning the project and a colleague opening it. Nothing else here
does.

**What the manifest fences is narrower than you might expect.** Two fields
accept only your *own* kinds:

- `opens.kinds` — which kinds the shell may hand you to display. Claiming
  another package's kind would be claiming you can show something you did not
  define.
- `badge.kinds` — what your row counts. A badge counting somebody else's records
  is a number nobody can act on.

Reading is not fenced at all, and writing is fenced by the type rather than by
the manifest: the engine checks the shape, not the author. **Writing into
another package's vocabulary is therefore possible and is real coupling** — the
shape can change under you when they publish an update. Prefer publishing a type
of your own and **linking** to theirs: a relationship names a record without
claiming to own its shape, and `dependentsOf` answers who points at what.

### 2. Routing by kind — the shell decides who shows a thing

An extension declares the kinds it opens; the shell resolves `kind → section`.
That resolution is a lookup over data and never a guess, and it has three honest
outcomes: open it here, name the extension that *would* open it and offer to
install it, or say plainly that nothing in this window can show it.

So "show this to whoever shows these" is expressible without naming anybody. The
search palette already works this way — a result is a record of some kind, and
the kind decides where it opens.

**Where this stops today.** The shell produces those requests; a section
*receives* one as an intent. An extension cannot yet emit one, so a package
cannot currently say *take the person to whoever shows this*. When that arrives
it will be one function over the resolution already described, not a new
mechanism.

Note the difference between the two halves: you may **render** another package's
records inside your own screen however you like — nothing prevents it — but you
may not **register as the destination** for their kind. Displaying is yours;
being the answer to "where does this open" is theirs.

### 3. The host performs work, so nobody has to ask anybody

The case that looks most like it needs a direct call is *my extension wants an
agent to work on something*. It does not need one.

A handler calls `work.order` and the **host** raises the agent. The extension
that displays conversations is not involved, is not asked, and need not be
installed. Meanwhile every session carries a `source` naming the package, the
handler and the order it came from — so whichever extension does show
conversations can group them, label them and count them without knowing who
ordered them until it reads the field.

Two packages cooperate on one piece of work having never named each other. That
is the shape to reach for.

### Designing a pair that work together

1. **Decide what the thing is and publish a type for it.** That is the
   interface, and it is the only part that has to be agreed.
2. **The producer writes records; the consumer reads by kind.** Neither names the
   other anywhere.
3. **Link rather than write into somebody else's vocabulary.** A relationship is
   a reference; a write is a dependency on a shape you do not control.
4. **If you need work done, order it.** Do not look for who could do it.
5. **Degrade rather than require.** A consumer whose producer is not installed
   finds no records of that kind, which is an ordinary empty list. Refusing to
   run would turn a soft absence into a hard failure.

`requires.extensions` exists in the manifest for the case where a package
genuinely depends on a particular other one. It is a **statement, not a gate** —
this build does not enforce it — and it is not the default for the reason at the
top of this section.

---

## 6. Three sources, three degrees of trust

| Source | Signature | Shown as | For |
| --- | --- | --- | --- |
| Registry | verified, soft in v0 | nothing special | how extensions normally arrive |
| A `.syncext` file | verified, soft in v0 | the file it came from | sideloading, an internal extension |
| **A folder** | none, and none possible | *Development* on the card | writing one |

Installing from a folder is what makes the system usable by anyone outside this
repository. The folder is read where it lies, the UI bundle is loaded from disk,
and a command reloads it without restarting the window — the loop an author
needs is edit, reload, look. It is stated on the card, because the one thing
worse than no sandbox is a sandbox somebody forgot they had opened.

It is **not** stated beside the section in the sidebar, which is where it was
first put and where it was rejected on sight on 2026-08-24. A sidebar row is a
name and a mark at 34 px; a word as long as *Development* hanging off every row
an author is working on crowds out the one thing the row is for. The card is
where somebody decides whether to trust a package, and the card is where the
word belongs.

## 7. The registry is one repository and one file

Every extension we publish lives in one public repository. Its CI builds each
one into a `.syncext`, attaches it to a GitHub Release, and generates the
index from the manifests — nobody edits the index by hand:

```
registry.json              the index the application reads
extensions/<id>/           source, manifest, types, prompt
dist/<id>.json             every published version: syncApi range, url, sha256, changelog
```

The window reads `registry.json` and searches it locally. That is the whole
search: one file, a few kilobytes, no GitHub API and therefore no rate limit and
no token. It is fetched with an ETag and cached, so opening the catalogue
usually costs a 304.

**Nothing is signed yet, and the window says so rather than implying otherwise.**
The format carries a signature over the canonical hashes file, the verification
is written, and the state a package is in — signed, unsigned, bad signature — is
read and drawn on its card. What is missing is the key: `SIGNING_KEY` in
`src-tauri/src/extensions.rs` is `None`, so a signature is reported and not
enforced. Every other check is: an archive whose contents disagree with its
hashes file, or that carries a file the hashes do not cover, is refused outright
and always has been. Provisioning the key turns a reported state into a
refusal, and is the last thing this section is waiting on.

**The network lives in Rust.** `registry_fetch` and `extension_download` are
Tauri commands over `reqwest` with the reachable hosts fixed in the binary. The
webview gains no `connect-src`, the CSP is not widened for it, and what can be
reached is a property of the build rather than of a page. This is where the
no-network rule was first reversed, and reversing it in Rust is what keeps the
reversal small.

`extension_fetch` is the second reversal and the same shape one step over: the
hosts are the package's rather than the build's, out of its own manifest, and
the webview still gains nothing. See §4 — a package reaching outside this window
is a permission with an extent, not a switch.

### Nothing is built in, and a fresh install still works

The recommended extensions are **seeded**: their `.syncext` archives are bundle
resources, unpacked into the artefact cache on first launch. They are not built
into the application — the code is not in this tree, they are compiled by the
registry's CI, they install through the ordinary path, and they update from the
registry independently of the application. A first launch with no network can
compose a project; a first launch with one gets whatever is newer.

## 8. Installing, and what it writes

A project declares what it depends on in its own record —
`installed: [{id, version, integrity, source}]` — so the declaration travels with
the repository and the same folder opened elsewhere resolves the same versions.
`integrity` is the artefact's sha256, which makes the declaration a lockfile:
re-tagging a release under the same version is detected rather than trusted.
Artefacts are the machine's, content-addressed under the app data directory and
shared by every project.

Installing is types first, declaration second, and the order is load-bearing: a
failure between them leaves types nobody declared, which the next install
reuses, rather than a declaration whose schema is missing. Removing writes only
the declaration — **types and records stay exactly where they are**, and the
confirmation says how many records will be left with nothing to show them.

Opening a project republishes the types of everything it declares, so a
colleague who clones it gets the schema by opening it.

### What is on the disk, and what it cost to check

Measured 2026-08-24 against a packaged build, with a package built by
`sync-ext pack` — which produced the same file byte for byte on two runs, so
"build it yourself and compare" is available from the first archive rather than
from some later version of the packer. It began as a script in this repository
and moved into the CLI once there was one; the two were compared on the same
input before the script was deleted, and the archives were identical.

- A well-formed archive unpacks to `artefacts/<sha256>/`, loads over
  `syncext://`, and its module's `useState` counts — one React, injected rather
  than bundled.
- The same archive with one line appended to `ui/index.js` and `META/hashes.json`
  untouched is refused **before anything is unpacked**: the artefact directory
  is left with exactly one entry, named after the honest archive.
- A folder installs, is marked *development*, and carries no integrity — there
  is no fixed content to hash.
- Removing takes the pointer away and leaves the artefact, which is what makes
  re-installing free and an update reversible.

**The vocabulary join is made.** A package's `types/*.json` and its prompt are
read out of the artefact by Rust and carried to the window with the rest of what
it says about itself, and the window forwards them to the engine untouched. They
are read in Rust rather than fetched from the window because a file inside an
artefact is reachable over `syncext://` and nothing else — fetching one would
widen the webview's `connect-src` — and because it has to work for a package
with no code, which is exactly the case that proved it: Project memory reaches a
project's memory without being executed. There is no translation step on the
way, so an author's file and the engine's transaction are one shape.

**The area join is made too.** A package's areas reach the sidebar: the window
activates what the project declared, in the order it declared it, and draws a
row per area the modules returned. An activation is cached against
`id@version#url`, because calling a module's `activate` twice returns different
component objects — React sees a different type and rebuilds the whole area,
losing exactly the state the mounting rules exist to keep.

Nothing is selected while that is happening, and the window says so by drawing
no current row. Choosing the catalogue and then jumping to a section as it
arrives would be two windows in the first second.

## 9. Updating

The registry index is the source of truth for what exists. It is read when the
catalogue is opened, with its `ETag`, so a second look usually costs a 304 and
an unreachable network leaves whatever was cached. Nothing polls it in the
background: an update is found because somebody opened the catalogue, which is
also the only place they could act on one.

**Read and remembered are two different things, and only one of them dials
out.** The mark on the pinned row is for the person who has *not* opened the
catalogue, and the catalogue is where the index is fetched — so the window reads
what the last fetch left on the disk when a project opens, and fetches nothing.
A machine that has never opened the catalogue says nothing about updates, which
is the honest state of it rather than a gap: it has never been told what exists.
The alternative was a request at every launch on behalf of somebody who did not
ask, for the sake of one dot.

**What a version says about itself is per version.** The index carries the
newest one of each extension, which is what answers *is there something newer*;
the changelog and every older version are in the extension's own ledger —
`registry/<id>.json`, one file per extension, fetched when its page is opened
and cached with an `ETag` like the index. The `syncApi` range lives on each
release rather than on the extension, because an extension whose newest release
needs a Sync this build is below is one whose older releases may still install.

**A folder is never offered one.** It is read where it lies and its files are
whoever is writing them, so replacing it with a published artefact would stop
serving somebody's working copy from the one screen that exists to make writing
an extension possible. Everything else may be moved, including a package that
arrived as a file or came seeded with the build.

**The mark says news, not a number.** A dot on the pinned Extensions row, by
rule 11 in `design-foundation.md`, and only for what could actually be moved
to: a newer version this build is too old to run would leave a dot standing
until somebody updated the application, and a mark that is permanently on is not
news. That case is a sentence on the extension's card instead, said once.

An update is offered only when the new version's `syncApi` range accepts this
build. When it does not, the card says which Sync it needs instead of offering a
button that would fail — and the person is told once, on the card, rather than
by a notification about an application they may not want to update.

**Nothing updates itself.** Installing publishes type definitions into the
project's memory, which is a write to the repository; doing that while somebody
is not looking is not an update, it is a commit they did not make. What the
window does instead is say so: a mark on the pinned Extensions row in the
sidebar, a line on the card with the new version and its changelog entry.

Applying one is two-phase and never mutates an installed artefact:

1. download → verify hashes, then the signature, then the `syncApi` range
2. unpack into a new content-addressed directory
3. flip the id's pointer to it
4. publish the new type definitions in one memory transaction
5. write the new `{version, integrity}` into the project record
6. any failure rolls the pointer back; the previous artefact was never touched

A downgrade is the same operation with a smaller number, and it is offered for
the case that matters: a project pinned to an older version because the newer
one needs a Sync this machine does not have.

## 9a. Where an extension may appear

*Decided 2026-08-24. The set is closed, and the closing is the point: a window
whose shape is decided by whatever is installed has as many shapes as it has
extensions, and no rule left to enforce against the next one.*

| Point | What it is | State |
| --- | --- | --- |
| **Area** | A section in the sidebar, drawn in one of four frames | built |
| **Badge** | A count or a dot on that section's row | built, both halves |
| **Types** | A vocabulary published into the project's memory | built |
| **Prompt** | What a connected agent is told, as `extension:<id>` | built |
| **Opener** | Which kinds it can show, and whether it shows the project's own types | in the manifest |
| **Menu commands** | What File offers while its area is selected | built |
| **Markdown plugin** | Replacing how one block of stored prose is drawn | built |
| **Native menu** | Secondary click, through the host's own menu | built |
| **Handler** | A function the host calls with no screen mounted — at install, and on a clock | built, §5a |
| **Settings page** | A section of the settings window, for what belongs to the machine | planned |
| **Palette commands** | Entries in ⌘K that open an area with an intent | planned |
| **System notification** | A macOS banner, gated — see below | planned |

**Not open, and each for its own reason.** The record inspector: it is drawn by
whichever extension shows records, so contributing to it would be a protocol
between extensions rather than a host API — a much larger thing, and not one to
introduce sideways. §5b is how packages cooperate without one. It does not reopen with handlers either: a handler may
*order work*, which the host performs, and a handler cannot draw. Reaching into a column
somebody else is rendering is still a different question, and still shut. Geometry: `shell-layout.ts` is the window's. The shell's own
screens — the project switcher, the sidebar itself. The Dock and menu-bar items:
they belong to the application, not to a project. Arbitrary docking: there is
one visual language, and it stays one.

### The badge belongs to the area, not to the extension

An extension with no area has nowhere to put a mark, and this is a rule rather
than an accident: a notification with no place to land would have to become a
system alert, which is a much louder thing than anybody asked for. **No section,
no notification** — and the catalogue says so on the card, because otherwise it
is exactly the kind of absence a person discovers by waiting for something that
never arrives.

There are two sources, and both are needed because of how areas live:

- **Declared.** *Built 2026-08-24.* The manifest carries a query over the corpus
  and the host counts. **No code runs**, which is what makes this work for a
  section nobody has opened yet: an area is mounted on first visit, so a
  runtime-only badge is silent in exactly the case a person most needs it — the
  first launch after opening a project.
- **Live.** *Built 2026-08-24.* A mounted area calls `useBadge`, and what it
  reports wins over the declared count for as long as it is mounted. This is the
  one an agent's reply needs: "the agent answered while you were elsewhere" is
  not in the corpus and cannot be counted from it.

  **Reporting nothing is not a report**, and that one word is what lets a
  section have both halves rather than choose between them. Chat declares how
  many conversations there are, so its row carries a figure before a line of
  Chat has run and goes on carrying it while nobody is talking to an agent; what
  Chat reports takes over only when there is something it alone could know.
  Composing the two is the area's own business — it is mounted, it holds both
  answers, and it decides which one its row should say. The rule the first draft
  had, where a mounted area's report always won, would have blanked the standing
  figure the moment somebody opened the section.

### What a declared badge may ask, and why it is that narrow

`"badge": { "kinds": ["chat.conversation"] }` is the whole of Chat's, and
`freshness` is the only other field. That is not a first cut of a query
language: it is the shape of the one answer the engine gives in a single call. A listing's
counts are over what its filters selected rather than over its page and they
arrive broken down by kind, so **one `limit: 1` listing per distinct freshness
filter answers every section at once** — two sections watching the same states
cost one call between them. A filter over a product field, which §9a's first
draft offered, is not in that answer and would be a scan of the corpus per
section; it is left out rather than approximated.

**A badge that names no kinds counts what its section opens**, and that is a
lookup the window already had. `opening.ts` answers *which section opens a record
of this kind* for the palette; asking it again here is what keeps a badge from
counting records its own section would refuse to show. It is also the only thing
that could work for the section that opens the project's **own** types: those
kinds are invented in somebody's window long after the manifest was written, so
no manifest could list them. Naming `kinds` is for the extension with two
sections over one vocabulary — `opens` belongs to the package and cannot tell
them apart.

Three things are left out and each is a claim rather than an omission. **Archived
records are not counted**, because archiving takes a record out of the lists and
a number in front of something the section will not show is a number nobody can
act on. **The kinds this window is not listing are not subtracted**: that
preference belongs to the frame and every area holds its own copy of it, so a
third copy in the window would drift from the other two the moment somebody ticks
a box — a badge therefore counts what the project holds rather than what this
window is showing of it, which is the narrower of the two claims and the one that
cannot go stale. And **it is read when the project opens and when the window is
returned to**, where `useSyncState` reads, because freshness is derived by
reconciling code history against each record's scope: what moves a count is
somebody's commit, and coming back to the window is when that has happened.

**The freshness states are the engine's, so the host passes them through.**
`Freshness` is deliberately open — a newer engine may derive a state this build
has no mark for — so a host refusing an unfamiliar one would be this build
having an opinion about the engine's vocabulary. A question the engine will not
answer costs those sections their number and leaves every other section's alone.
`sync-ext check` is stricter than the host on purpose: it holds an author to the
four states the contract they built against publishes, and catches in their own
terminal the badge whose kind the package never writes — a count of zero for
ever, which draws nothing and looks exactly like a section with nothing to say.

An area that is frozen is still mounted, so it keeps reporting. That is a
deliberate narrowing of the rule that a frozen area stops reading the store: it
stops
reading, it does not stop existing, and the one channel it keeps is this one.

**A figure and a dot are two claims, and never each other.** An extension
reports either a count or merely that something is worth a look, and the two are
drawn differently because they mean different things: a figure is how many there
are, standing and as true when nobody is looking, while a dot is *something
happened, go and look* — which is what a dot means everywhere else on this
system.

The first draft had the dot doing both jobs, standing in for a count too large
to print, and it was rejected on sight the day it was first looked at. The
objection is the right one and it is now a rule: **a mark that is permanently on
is not news.** A large figure is abbreviated to `99+`, and the dot is left
meaning the one thing it means.

Where a figure stops being read, what a dot looks like, and which of the two
survives the column being folded are the window's business — an extension that
could choose would be an extension that could shout. See rule 11 in
`design-foundation.md`.

### A system notification is a banner, and banners are gated

Three conditions, all of them: the extension declares the `notifications`
capability, the person has allowed it for that extension, and the window is not
frontmost. A banner is the same event as the badge, said louder — never a
different event, and never one the badge did not already carry. An extension
that notified without a badge would be telling somebody about a place they
cannot go and look.

## 10. The catalogue

Extensions is pinned to the foot of the sidebar and turns all three columns over
to itself. It is not a list of the things this build happens to contain:

- **Navigator** — `Marketplace` as the first row, then the group `Installed`,
  and nothing else. The search field is on the marketplace page rather than in
  the band above every column, because that is what it searches: a field in the
  title bar would claim to search the project. Development is a mark on the card
  rather than a group of its own — it is a state a package is in, not a place it
  comes from, and a group would sort by the wrong thing.

  **A row and a card are two different claims**, and getting that wrong is what
  the first arrangement did: it listed every unpacked package as a row, so the
  column said *this is a part of this window* about something the project had
  never asked for. The list is now what the project runs — declared by it,
  answered by a package, runnable in this build — and everything short of that
  is a card.

  **The two doors a package can be brought in through by hand are icons at the
  foot of this column** — a `.syncext` file and a folder — in the band macOS
  keeps for what acts on a list, on the leading edge. The other two ways a
  package arrives are not doors anybody opens: the registry, and the archives
  this build ships with. They were under the selected package's facts in the inspector until
  2026-08-24, which made the only door in the application open from a room
  nobody could reach: a machine with nothing unpacked and a project declaring
  nothing selects no card, so the inspector drew nothing and there was no button
  anywhere. That is the state everybody starts in, and two other places in the
  window — the navigator's own empty text and the project-setup step — sent
  people to it by name. A control that only appears once its own job has been
  done is not discovered by reading; it is discovered by somebody already having
  what it offers.
- **Workspace** — the marketplace, or one extension. The marketplace is every
  entry there is, as cards, and it is what the area opens on: what somebody
  arrives to decide is *what could this project do*, and the answer to that is a
  set to compare rather than a column of names to click one at a time. An
  extension's page is what it does, what it adds to the window, the types it
  would publish, what it tells an agent, its changelog. All of it read from the
  manifest, the type files and the prompt inside the package; the prompt is
  shown whole rather than summarised, because a summary of it would be a second
  thing to keep true. **A card is not a decision** — choosing one opens the
  page, because what a project would be agreeing to does not fit on a card and
  must not be agreed to without it.
- **Inspector** — the package: id, version, source, author, required Sync,
  required capabilities, sha256, signature state, and the two different things
  removing it can mean. Empty while the marketplace is open: this column
  describes one thing and the marketplace is about a set.

**Everything is listed, and the card says where it stands.** This reverses
"only what can be installed is listed", which was right about a product that has
not shipped and wrong about a package already unpacked: it is on somebody's
disk, it is taking up room, and removing it is something they may want to do.
The rule survived only while a second panel listed the disk in full, and that
panel is gone — the marketplace is the one place a package is seen, so hiding
one there hides it everywhere.

States the card must be able to say, because each is reachable in ordinary use:
installed; unpacked and never asked for; declared by this project and absent;
unpacked and refused by this build, with the reason; failed to load, with the
reason; running from a folder; unsigned; update available; **update available
but needs a newer Sync**.

## 11. Writing one

`@sync-buzz/extension-api` is published from a repository of its own and carries
types, the manifest schema, `SYNC_API_VERSION`, the capability names, and a CLI:

```
sync-ext init      scaffold from the template
sync-ext dev       build in watch mode; Sync loads the folder and reloads on change
sync-ext check     manifest schema, kind prefixes, API range, forbidden imports
sync-ext pack      the reproducible .syncext, signed when given a key
```

*The package exists.* `@sync-buzz/extension-api` is a repository of its own carrying
the rolled-up declarations, the manifest's JSON Schema, the list of which of the
surface's names exist at runtime, and three of those four commands. `init` is
not among them and is waiting on the template, because a scaffold has to reflect
what actually works rather than what was intended — and what actually works is
only knowable once something has been built with it.

What `check` learned is worth stating because it was not obvious: **the check that earns its keep is the one that runs the module.**
`activate` is ordinary code and the manifest is JSON, so nothing in the type
system relates them — an area renamed in one and not the other type-checks
perfectly and installs as an empty column. Running it against a stand-in host
and comparing what came back with what was declared catches that at the end of a
build, in the terminal of the person who caused it, rather than in front of
somebody opening a project.

The rules a check enforces, and the reasons, are short enough to state here.
Every kind an extension publishes is prefixed with the extension id and a `.` —
two extensions may both want to call something a decision. An area declares one
of four frames (`browse`, `list`, `detail`, `single`) and fills that frame's
slots; it never composes columns, and returning a slot the frame does not have
fails to install rather than being dropped. An area is mounted on first visit,
frozen when another is selected, and never unmounted — so no extension
implements state restoration, and a frozen area stops reading the store.

## 12. Deliberately absent

The wasm runtime — **rejected outright** on 2026-08-25, rather than deferred as
it had been. An extension does now want an action rather than a screen, which was
the condition this line used to carry, and the answer to it is a JavaScript
service module rather than a wasm component: the extensions that exist are
TypeScript, and a wasm runtime would make their authors learn a second language
to poll an issue. The measurements that decided it: QuickJS through `rquickjs` adds
528 KB to a release binary synchronously and 643 KB with async, against
4.03 MB for a minimal wasmtime — measured as the delta over an empty binary,
`opt-level = "z"`, LTO, stripped. Signature verification as a
gate. A sandboxed tier for unsigned UI; the signature format is what lets that
arrive without a redesign. Paid extensions, ratings, and anything else that
makes the registry a storefront rather than an index.
