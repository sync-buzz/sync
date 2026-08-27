# Architecture

This describes the repository as delivered: the application shell, the flow that
opens a folder as a project, the memory integration behind it, and the seam
every extension arrives through.

It is the map rather than the whole country. Four subjects have documents of
their own and are only pointed at from here:
[`design-foundation.md`](design-foundation.md) for what the window looks like
and why, [`extensions.md`](extensions.md) for what a package is and how one is
loaded, [`background.md`](background.md) for the half of an extension that runs
with no window open, and [`voice.md`](voice.md) for what the application says
out loud.

## Tauri is the desktop adapter

`src-tauri` opens a native macOS window, serves the frontend from the app bundle, and exposes a thin command layer over the domain crates:

- Four plugins are enabled, and each is here because something shipped needs it: `dialog`, for the native open panel a person chooses a project folder with; `autostart`, so the server can be offered at login; `opener`, so a link reaches the browser; and `updater`, which the window has no route into at all — see below. The generated logging plugin was removed, and no filesystem, shell, HTTP or global-shortcut capability is requested. Both the memory engine and `git` are launched directly with `std::process::Command`, so no shell plugin is needed for either.
- `src-tauri/capabilities/default.json` grants `core:default`, the window commands the header's drag region and the launch sequence need, and `dialog:allow-open` — not the dialog plugin's save, message or ask commands. It applies to the main window alone.
- The settings window has a capability of its own, `src-tauri/capabilities/settings.json`: `core:default` plus `show` and `set-focus`. It opens no folder, borrows no memory session and drags no title bar, so none of the main window's other permissions reach it; closing it is the menu bar's own Close Window, which is not an IPC call.
- The menu bar is built in `src/lib/app-menu.ts` from Tauri's menu API and installed once, from the main window, with `setAsAppMenu`. Nothing is granted for it: `core:default` already covers `core:menu`.
- The `invoke` commands are declared in one list in `src-tauri/src/lib.rs`, and there are a hundred-odd of them across nine modules: `memory.rs` and `project.rs`, then `sessions.rs` for agent conversations, `extensions.rs` and `handlers.rs` for packages, `schedule.rs` for their clocks, `connect.rs` and `server.rs` for the MCP interface, `settings.rs`, `voice.rs` and `windows.rs`. Each parses its input, calls a domain function, and maps the result — no branching, no policy. The list being one list is the point: what the webview may ask for is readable in a single place.
- A Content-Security-Policy is set for the packaged application. It allows only same-origin assets and the Tauri IPC endpoint.

### The style directives are not redundant

The policy spells out `style-src-elem` and `style-src-attr` in addition to `style-src`, and that is load-bearing rather than belt-and-braces.

Tauri rewrites the configured policy at build time: it hashes the inline _scripts_ found in the exported assets and injects a nonce into `script-src` and `style-src`. Per CSP, once a nonce is present in a directive, `'unsafe-inline'` in that same directive is ignored. For `style-src` that has two consequences in this application, both of which produce a window that looks plausible at a glance and is actually broken:

- Inline `style` **attributes** stop applying. The resizable panel group sizes its columns through inline styles, so every panel falls back to its content width and the shell renders as four ragged columns.
- Inline `<style>` **elements** stop applying, and Tauri hashes scripts but not styles. Radix's scroll area ships one such element to hide the native scrollbar inside its viewport, so scroll areas would show two scrollbars.

Declaring `style-src-elem` and `style-src-attr` without a nonce restores both, because CSP3 gives the more specific directive precedence. Script policy is unaffected and stays as strict as before.

Both failures appear only in a packaged build: in development the frontend is loaded from `devUrl` over http, where Tauri does not apply the policy at all. Any change to the CSP therefore has to be verified against `pnpm tauri build`, never against `pnpm tauri dev`.

### Two windows, one document

`settings_open` in `src-tauri/src/settings.rs` builds a second webview on the same exported document, under the label `settings`. Which window a document is showing is decided by that label, read in `src/lib/settings/window.ts` — not by a route. The frontend is a static export, so a second route is a second HTML file that has to resolve identically under the dev server and inside the bundle; a label answers the same question without that, and it is answered before anything renders.

Like the main window it is built hidden and reveals itself once it has painted, through the same `useWindowReveal`. It wears no window material: the material is the main window's edge, and a second window carrying it would turn a deliberate detail into a theme.

Nothing crosses between the two windows. Settings hold what is true of the installation — which agents reach Sync — and extensions are a project's, so they are chosen in the project's own window.

The window keeps its native decorations. `titleBarStyle: "Overlay"` with `hiddenTitle` lets the application header occupy the title bar while the real traffic lights, the drag region, double-click-to-zoom, window resizing and accessibility all remain the system's. `trafficLightPosition` aligns the buttons with the header, and the header reserves the leading space through the `--titlebar-inset` token.

Native window materials (`windowEffects`) **are** used, and they cost this application the Mac App Store. The `underWindowBackground` material is what the frame is made of; the Tauri schema requires a transparent window for it, transparency on macOS requires `macOSPrivateApi`, and an application built on a private API cannot be published to the App Store. Distribution is therefore direct, through the updater below.

The material is asked for in `tauri.conf.json` and confirmed in `src/lib/window-material.ts`, which is also what withdraws it when the system asks for reduced transparency. Nothing in CSS assumes it: `data-vibrancy` is set only once Tauri reports the effect applied, so a browser, a system that refused it and a platform that has none all keep the opaque surfaces the shell is designed to fall back to. The reasoning is [`design-foundation.md`](design-foundation.md)'s.

## Next.js is a static UI build system

Next.js never runs as a server. `output: "export"` produces a folder of static assets in `out/`, which Tauri consumes through `frontendDist`. There is no Node.js runtime in the packaged application, therefore no SSR, no Route Handlers, no Server Actions, no middleware, no rewrites and no image optimizer. See the README for the full list.

In development, `beforeDevCommand` starts the Next.js dev server and Tauri loads `devUrl`. In a release build, `beforeBuildCommand` produces the export and Tauri embeds it. The frontend never assumes a server is reachable.

## No business logic in Tauri

The command layer is a thin typed adapter: parse input, call a domain function, map the result. No branching, no I/O policy and no state machines belong in it.

Domain modules are ordinary Rust that compiles and runs without Tauri, so they stay testable from plain `cargo test` and remain usable from a CLI or a service later. Tauri is one of several possible callers, never the place the logic lives.

`src-tauri/src/project.rs` is the smaller case: probing a folder and making it a repository are plain functions over a `Path` that shell out to `git`, and the commands only convert a `String` into one. The two settings commands are the one place outside `memory.rs` that borrows a memory session, which is why `MemorySessions::with_session` is `pub(crate)` rather than private — opening a folder has to read and write one record before any screen exists to do it from. All of it is covered by `tests/project_commands.rs`, which drives the commands through Tauri's IPC against real folders on disk and, where an engine is installed, a real one.

`src-tauri/crates/sync-memory` is the first such crate. It owns the sidecar process, the JSON-RPC framing, the surface handshake and error mapping — and the vocabulary both sides share: the DTOs and the entity-to-envelope mapping. What it no longer owns is the code that *uses* that mapping. The conflict replay, the type corpus and everything else that knows a decision from a document moved into `sync-mcp`, on the engine's side of the process boundary, when the engine moved inside it. Its tests run against a scripted transport for the framing and against a real `sync-mcp` process for the contract — the latter is what caught a search failure on a freshly built index that no unit test saw.

## Memory is a separate process, and the process is ours

`LanceDB` and llama.cpp are the heaviest and least predictable dependencies in the stack; in a separate process a crash in ggml costs a reconnect, while linked into Tauri it would take the window with it. So the boundary stays — but it has moved. The engine used to be the process on the far side of it and is now a library linked into `sync-mcp`, which is the process Sync bundles and supervises. A panic in ggml still costs a reconnect; what it no longer costs is a second protocol between two of our own components.

`sync-mcp` serves two callers over stdio and they do not share a surface. An agent gets MCP: an allow-list of ten reading tools, described in the engine's own words because the descriptions are the engine's to write. The window gets a channel of product operations that is not MCP — no descriptions, no schemas written for a model, and no route to it from the agent's connection. Two dispatchers rather than one list with a filter over it, so there is nothing for a client to guess the name of.

One session is kept per open project for the life of the application. Opening one is lazy in the way that matters: the sidecar starts and greets without reading the corpus, so a project it cannot read yet — a repository nobody has kept memory in — refuses by name instead of by ending the process.

## The project says where its memory lives, and Sync answers for it

The engine will not guess where a project's records go: its host says so when it opens the project, and nothing is written down that could later disagree. Sync answers `git_metadata` — Git objects — because a project here is a repository and memory kept in Git travels with it, versions itself, pushes to the same remote and puts nothing in the working tree. Opening that storage is what creates it, so the first read of a repository that has never held memory is what gives it some. It happens on the way in rather than behind a button: the alternative is a window that opens on a project it cannot read to ask a question whose answer it already knows.

Where a type's _documents_ live is a separate decision, and one there is something to decide about. Attaching a folder writes the directory into the type's own definition — `"storage": "docs"` is the path, not a label standing for one — so reading a type answers where its documents are outright. There is no mask: every file below the folder is a document of the type, diagrams and PDFs included, which is why one folder belongs to one type.

## A record is patched, never rebuilt

`memory_document_update` carries a key and a patch: a title, a body, tags, links, scope or observed paths, the archive flag, product fields — any of them, and only the ones that changed. `MemoryClient::update_document` reads the stored record, applies what the patch names, and hands the rest back to the store untouched.

That shape is deliberate and it is the same one `update_type` has. A window that rebuilt an envelope from what it knows would drop whatever it does not model, including whatever a newer engine added, and it would do so silently while somebody fixed a typo. It is also what lets two surfaces write the same record without racing: the panel sends `{tags}` while the editor is still holding an unsent body, and neither carries a stale copy of the other.

Freshness is not in the patch at all. The engine derives it by reconciling code history against the record's scope, so it is an answer rather than a field.

`memory_document_create` reads the type's own definition to decide which fields a new record must carry — its stated `default`, else the first value of an enumeration — and generates a key of the corpus's usual shape, checking it is free before writing. `memory_document_delete` takes a list and applies it as one transaction, whatever the kinds in it are: every envelope lives in the storage the project keeps records in — a type naming a storage puts its _documents_ elsewhere, never its records — so deleting a decision together with the document it is about is one write and atomic in the way the person who asked for it assumed. `memory_document_dependents` maps the engine's backlinks into two lists, explicit links and body mentions, which is the distinction a confirmation needs to say something true.

One kind of record is refused before the engine has to see it: a `__type__` definition, which is edited through the type sheet and would leave a corpus nothing can parse. Two more are refused for creation and deletion only — a definition, which is created and removed as a type, and the record that names the project, of which there is exactly one and without which the project could not be opened. The project's record is otherwise an ordinary document: its title is the project's name, its body the description, its `language` field the language it writes in.

**Links are validated against the type, not against the window.** The engine rejects a relation the record's type does not declare, and it checks the target's kind unless the declaration says `any`. That is why `MemoryType` carries the declared `relationships` as well as the `fields`: the panel offers exactly those relations, and a type declaring none cannot hold a link at all.

## Updating is Rust's, and the window is not told about it

`src/updates.rs` looks once at launch, downloads what it finds and installs it, and says nothing. None of the updater plugin's commands appears in any capability, so the flow has no entry point from the webview: a record's body cannot ask this application to fetch a bundle and run it. That is the whole reason the plugin is loaded rather than a command exposed.

What is downloaded is verified before it is installed, against a minisign public key compiled into the binary. That is what makes fetching from a static file over plain HTTPS safe to do unattended — the file says where to download from, and cannot say what is trusted. Rotating that key strands every copy in the field, so it is not a thing to do casually.

The new version starts on the next launch rather than by interrupting one. Sync is a menu bar application that people leave running, so "the next launch" could be a long way off; the menu bar item grows a restart entry when there is something to restart into, and that entry is the only thing the whole flow ever puts in front of anybody. It is built into the menu rather than enabled in it — an item permanently greyed out is a promise made at every launch and kept almost never.

## Nothing in this repository is a section of the window

This is the rule the rest of the design rests on, and the one a compiler cannot hold, so CI holds it instead: there is no `src/extensions/` directory, and a job fails the build if one appears or if anything resolves a path into it. Records, Chat and Project memory are packages like any other — built elsewhere, delivered as archives, installed through the path a stranger's extension is installed through. A window that could name one section in a type or a constant would be a window that treats that section differently from the next one, and the difference would not show until the day somebody else's package needed the same thing.

What the shell keeps for itself is what a project cannot choose to be without: the window, the columns, the project flow, the search palette, the settings, and the seam. Everything else is a decision somebody makes per project. [`extensions.md`](extensions.md) is the whole of how that works — the manifest, the capabilities, the loader, the registry, updates.

**Sync publishes one interface to agents**, over MCP, and the settings window connects one by writing a single server entry into that agent's own configuration. The write is a Rust command with a fixed policy — this server, in that file, for this project — rather than a filesystem capability handed to the webview, and it splices rather than reformats: somebody's other servers, their comments and their formatting are not ours to tidy.

`Extensions` is the second area of the sidebar, pinned to the foot of the column. Selecting it turns all three columns over to packages: the navigator lists what this project runs, the workspace is the marketplace or one extension's page — what it does, the types it would publish, what it tells an agent — and the inspector describes the package itself. **Every package there is gets a card, and the card says where it stands**: installed, unpacked and never asked for, declared and absent, refused by this build with the reason, running from a folder, an update waiting. A package already on somebody's disk is taking up room whether or not this build can run it, and a catalogue that hid it would hide the thing they came to remove.

What does *not* get a card is a feature that does not exist. There is no entry for a section nobody can install, because a card for something a person cannot choose is a way of saying no to a question they asked in good faith, and this is exactly where they are asking it. What is coming belongs in release notes.

## Two absences worth stating

Not a list of what is unbuilt — that belongs in release notes, and a document that kept one would be a roadmap pretending to be a description. These two are properties of the application rather than gaps in it, and a reader will otherwise go looking for them:

- **No account, and nothing to sign in to.** Sync has no server of its own. There is no identity, no session and no licence check, and a project's memory never leaves the machine except through the Git remote its owner chose.
- **No telemetry, no analytics, no crash reporting.** Not disabled by default — absent. The application makes exactly one kind of outward request on its own: the update check, and the extension registry when somebody opens the catalogue. Both go to GitHub, both are in Rust with the reachable hosts compiled in, and the webview is granted no `connect-src` at all.

## Search asks the corpus; the area that owns the type opens the answer

The palette in the title bar (`⌘K`) is one question over the whole corpus, answered by `memory_search`. It is not a section of the window and deliberately not: a project cannot choose not to have search, and everything a project can choose not to have is an extension.

**What opens a result is decided by its kind, and by nothing else.** An extension declares the types it publishes, so `kind → extension` is a lookup rather than a guess, and `src/components/shell/opening.ts` is the whole of it. Nothing there reads a file name, a media type or a path. Sync did not create the type and does not decide how its documents are read: an extension publishing a type of videos owns opening videos, and until one is installed the honest answer is that this project cannot show them. A kind no extension published is one the project made itself — including a folder somebody attached — and those are Records', which is the section a project's own types are read in.

There is no registry to keep in step. The set of openers is the catalogue intersected with what the project declares: an extension is in it because it publishes the type and out of it because `installed` no longer names it, so installing registers nothing and removing forgets nothing. That the catalogue is consulted rather than only the installed set is what lets a result name the extension a person is missing instead of failing quietly.

Exactly one state remains: a record whose kind belongs to an extension the project has not installed. It is reachable in ordinary use, because removing an extension deliberately leaves its types and records where they are — the palette says which extension publishes the type and leads to its card, and installing stays a decision made in the catalogue rather than in passing from a search result.

**The palette opens nothing itself.** An area owns what it is showing and keeps it for as long as the window is open, which leaves no way in from outside; `AreaIntent` in `src/lib/area-intent.ts` is that way in, and it carries what to show and nothing about how. `ProjectWindow` selects the area and hands it the intent in the same commit, so an area mounting with one already in its props is the ordinary case. An area applies an ask it has not applied yet and stops applying it the moment somebody selects something of their own — derived at render rather than copied into state, so what is open has one answer rather than two that can drift.

The type filter is the navigator's own control, mounted over the same stored preference: a type somebody took out of this window is not one they want back in an answer, so unticking it removes the type from the list, from the counts and from search at once. It is `src/components/shell/type-filter.tsx`, published through the extension boundary rather than owned by Records, because two controls over one stored fact would be two ways of writing it.

**Part of a word finds the word.** BM25 matches whole terms, so `arch` did not find `Architecture` — which reads as a broken search rather than as a property of term matching. The engine now runs a substring pass between the index and the vector channel, and only when the index came back with almost nothing: every term of the query has to appear inside a title or a body, so a second word narrows rather than widens. It appends rather than re-ranks, so an exact term match keeps its place above a fragment of one. It is a scan, which is why it is second and conditional.

**A hit says how it was found, and the palette believes it accordingly.** The engine runs BM25 first and falls back to a vector channel when the words matched thin, which means it answers _something_ for any input at all — in a corpus of a dozen records one of them is always the nearest, and how near depends on how broadly that record is written rather than on the question. Measured against this project: nonsense (`qqqqqqqqq`) peaks at 0.46, off-topic English (`photosynthesis`) at 0.517, and a correct cross-language question (`шифрование записей`) at 0.487 — so no threshold separates the relevant from the irrelevant, and one tuned until it looked like it did would be cutting real answers. `memory-hub` therefore labels each hit `matched: words | meaning | both`, and the palette shows the second kind under its own heading — "No words matched. Nearest by meaning:" when there is nothing else. What used to look like a search that found something now reads as what it is. The cosine floor stays as well, raised to 0.45, because below it every candidate in every class was noise.

The set travels to the engine as `kinds` in one request. Asking one kind at a time and fusing the answers is not open to a client — rank is only comparable inside a single answer — so the capability was added to `memory-hub` rather than approximated here: filtering a page the engine had already truncated would report a total that is not one.

## Folders are the engine's, and the window never touches a working tree

A folder is a name a record is filed under, and what it *is* underneath differs by where the type keeps its documents: a directory for a type over an attached folder, and the record carrying `is_folder` for a type whose documents are its own records. **The window does not branch on that.** One command — `memory_folder_create`, `memory_folder_rename`, `memory_document_move` — and the engine decides from the kind. A window that asked where a type stores documents before offering "New Folder" would be a second place that decision is made, and a second place it can be made differently.

That is also why Sync writes no file itself. Moving a document moves its file *and* its record, in that order, and both are the engine's: `documents.move` is one operation because the two halves may not disagree, and a window doing half of it would leave a record pointing at a path with nothing at it.

`useFolders` reads the hierarchy beside the corpus rather than inside it, keyed to the same revision so the tree and the list describe one moment. It is read live and never cached across projects: Git keeps no empty directories, so an empty `docs/api/` is a fact about one working tree and simply absent from a fresh clone. One call per type, because folders are a namespace the whole project shares — a decision filed in `docs/guides` next to the documents sits there quite happily — so a project-wide answer could not say whose a folder is.

`SourceTree` in `src/components/shell/source-tree.tsx` draws it. The behaviour underneath is `@headless-tree/core` — expansion, focus, the ARIA a flat-rendered tree needs — and the markup is the shell's, so extensions get the window's rows and never the library. Selection follows focus, as a macOS source list does and as `SourceList` beside it does; the tree is one tab stop and the arrows move within it.

## Memory moves in two directions, and says what it cost

`refs/memory/main` is the one ref a project's memory lives on, and it travels on a remote of its own: an ordinary `git push` never sends it, and `git clone` never brings it. There is no second ref for "mine" against "everyone's" — the local branch is that, and `SyncState.unpublished` is how the window says so.

A fetch merges. Every member of a record is asked the same three-way question against the common ancestor: where this side still matches the ancestor only the other side moved, so theirs is taken; where both moved and disagree, **this side is kept and the member is named** in `overlaps`. Whoever is fetching is at the keyboard and can put it right; the other party is not here to be asked. Sets — tags, links, source paths — merge per member and cannot collide.

Nothing is silently absorbed and nothing is destroyed. What lost is still a commit, and `memory_rewind` puts memory back where the fetch found it — backwards along its own history only, and refused outright if anything has been written since, because that record is not part of what the fetch did.

## Where a fact lives is a decision

Three stores, and which one a fact belongs to follows from whose fact it is:

| Fact                                                        | Where                                                               | Why                                                                                                |
| ----------------------------------------------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Name, description, knowledge language                       | The project's memory, as the `project` record                       | It is the project's, and it has to travel with the repository                                      |
| Recently opened projects                                    | `recent-projects.json` in the app config directory                  | It is this installation's, and no repository should carry it                                       |
| Appearance and base colour                                  | The webview's own storage, read by `src/lib/settings/appearance.ts` | It has to be on the document before the first frame, and a value fetched over IPC arrives after it |
| Panel widths, which area is selected, which project is open | React state                                                         | It is this run of the window's                                                                     |

The first two are commands in `src-tauri/src/project.rs`; nothing in the frontend reads a path or a file itself.

## Layout state is ephemeral and separate

`src/lib/shell-layout.ts` owns panel roles, geometry and collapse rules. It holds no product data, writes nothing to disk and makes no network or IPC call. The layout is rebuilt from its defaults on every launch.

The shell keeps two pieces of product-shaped state. The open project lives in `AppShell`, because it decides which of the two windows the slab holds; selection — which area, which type filter — lives in `ProjectWindow`, which mounts with a project and unmounts with it, so nothing has to be reset when a different folder is opened. Both are local React state, neither is routed or persisted, and neither is backed by a domain model.

When product state does arrive it belongs somewhere else again. Keeping the three apart — layout, selection, domain — is what allows any of them to change independently.

## Frontend structure

```
src/
  app/            layout, page, and the design token layer in globals.css
  components/
    app-root.tsx  which of the two windows this document is showing
    shell/        the application shell, one file per panel role
    editor/       the record editor: block components, the slash list, the toolbar
    settings/     the settings window, one file per section
    ui/           vendored shadcn/ui components (Radix base, unified radix-ui)
  lib/
    extension-api/    the contract a package compiles against, and its version
    extension-host/   loading a package, its areas, badges, clocks, the marketplace
    agent-sessions/   the client over one ACP conversation, and its transcript
    editor/           the plugin list, the Markdown round trip and its fidelity check
    memory/           typed client over the memory commands
    project/          typed client over the project commands, and what a project is
    settings/         appearance, typography, voice, agents, the window's role
    shell-layout.ts   panel roles, geometry and collapse rules
src-tauri/
  src/memory.rs             the memory command layer
  src/project.rs            probing a folder, making it a repository, its settings and recents
  src/sessions/             agent conversations: adapters, the catalogue, live and remembered
  src/extensions.rs         the `syncext://` scheme and the package commands
  src/handlers.rs           calling into a package's service module
  src/schedule.rs           the clock that ticks with no window open
  src/connect.rs            writing Sync into an agent's own MCP configuration
  src/server.rs             the MCP interface other agents reach
  src/voice.rs              who may ask this machine to say something
  src/updates.rs            the update check, with no route in from the webview
  crates/sync-memory/       the engine client, independent of Tauri
  crates/sync-mcp/          the sidecar: the engine linked in, and two surfaces over it
  crates/sync-extensions/   manifests, archives, the store, the registry, a package's net
  crates/sync-handlers/     service modules in a QuickJS isolate
  crates/acp-client/        the agent protocol, checked against frames from real CLIs
  crates/agent-bridge/      Codex, which does not speak that protocol
  crates/sync-voice/        the system's speech synthesiser
  binaries/                 the bundled sidecar (a build artifact, not source)
scripts/prepare-sidecar.sh  builds or stages that binary
scripts/release.sh          the version, the tag, and the update manifest
scripts/api-surface.mjs     what an extension may see, and the number it is promised under
```

Every crate under `src-tauri/crates/` compiles without Tauri. That is the test
for whether something belongs in one: a crate that cannot see the application
cannot widen its own reach, and the module in `src-tauri/src/` beside it is
where the application decides who may ask.

Shell components address panels only by role. Nothing in `components/shell` knows a pixel width, and nothing in `lib/shell-layout.ts` renders anything.
