<div align="center">

# Sync

### A desktop environment where agents do the work

[![Latest release](https://img.shields.io/github/v/tag/sync-buzz/sync?label=release)](https://github.com/sync-buzz/sync/releases/latest)
[![macOS 13+](https://img.shields.io/badge/macOS-13%2B%20Apple%20silicon-black)](#install)
[![Licence: FSL-1.1-MIT](https://img.shields.io/badge/licence-FSL--1.1--MIT-blue)](./LICENSE)
[![Extension API](https://img.shields.io/npm/v/%40sync-buzz%2Fextension-api?label=extension%20API)](https://www.npmjs.com/package/@sync-buzz/extension-api)

**[⬇&nbsp; Download for macOS](https://github.com/sync-buzz/sync/releases/latest/download/Sync_macOS_aarch64.dmg)**
· [sync.buzz](https://sync.buzz)
· [Write an extension](docs/extensions.md)
· [How it is put together](docs/architecture.md)

</div>

<img src="assets/brand/window-anatomy.png" alt="The Sync window drawn as four labelled columns — the primary sidebar holding the areas a package added, the context navigator holding what belongs to the selected area, the workspace holding the record as the text it is, and the context inspector holding what is true of it. Beneath them: what a project knows is kept in the repository's own Git objects, agents reach it over MCP or are driven from the window over ACP, and every piece of subject matter arrives as a package.">

**Open a folder on your Mac and it becomes a project.** The agent you already
use works inside it, and everything the project learns is written to that
folder's own Git repository — where it branches, merges, travels to whoever
clones it, and belongs to you.

**What kind of work is up to you.** Sync is a shell. It holds what every kind of
work needs — projects, the agents that act on them, the permissions those agents
run under, a clock that keeps going when the window is closed, and a store of
what the project knows that says when it has gone out of date. What a project is
*about* comes entirely from the packages installed into it.

**No package is privileged, and the application ships none.** Nothing is
bundled and nothing is unpacked at launch: every package in the catalogue
arrived the way yours will — built outside this repository, declared in a
manifest, fetched from a registry and installed into a project that asked for
it. There is no `src/extensions/` directory here and CI fails the build if one
appears, so the shell cannot name a section in a type or a constant even by
accident. That rule is the whole design — it is what makes the environment yours
to shape rather than ours to extend on your behalf.

**What the shell guarantees, so a package does not have to:**

- **A project is a repository**, and everything it knows is written to that
  repository's own Git objects. It branches, merges, travels to whoever clones
  it, and belongs to you. No account, no server of ours.
- **What it knows says when it has rotted.** A record names the files its claim
  is about; the engine reconciles them against the repository's history and
  marks the claim `stale` when they move. Not a review date somebody set and
  forgot — derived, every time it is read.
- **Agents, in both directions.** Seven clients are connected with one control
  and work against the project through Sync's own server — Claude Code, Codex
  CLI, Grok CLI, Claude Desktop, Cursor, Visual Studio Code and Zed. Or drive an
  agent *from* the window over ACP, with its plan, its tool calls and its
  permission prompts drawn as part of the interface: five CLIs are measured and
  raised by name.
- **Work outlives the window.** A package can declare handlers on a clock and
  order work that runs with nothing open, under permissions it declared and a
  person approved, attributed to whoever asked for it. A conversation can hand
  work to another conversation and be handed back the answer.
- **What a package needs of the machine, it asks for by name.** Fourteen
  capabilities, each a promise about behaviour rather than a switch: the network
  with its hosts written out and no wildcard, a corner of the system keychain, a
  shell in a folder, a handler on a clock. A missing one is a refusal with a
  sentence a person can act on, never a silent degradation.
- **The window is not only at the desk.** The iPhone application shows the same
  window arranged for a phone, and every call it makes is answered by the very
  function the Mac's own window calls — so a conversation started at the desk is
  continued on the phone, because there is only ever one conversation.

Structured knowledge that stays honest, agents that act on it, and an
environment you assemble rather than accept.

> **Pre-1.0 and under active development.** Used daily on the machine it is
> built on, and not finished. What is here works; what is absent is absent
> rather than stubbed. See [Where it stands](#where-it-stands).

## Install

| | |
| --- | --- |
| macOS 13+ (Apple silicon) | [**Sync_macOS_aarch64.dmg**](https://github.com/sync-buzz/sync/releases/latest/download/Sync_macOS_aarch64.dmg) |

**macOS only, deliberately.** The window is drawn against macOS materials — the
`underWindowBackground` material behind the frame, the title bar the header
occupies, the traffic lights placed by hand, the Dock menu, the system speech
synthesiser. Some of that degrades honestly on other platforms and some of it
does not, and nobody has yet launched a Linux or Windows build to find out
which. Shipping an installer nobody has opened would be a worse answer than
shipping none. The pipeline still builds one from a single matrix row —
see [docs/releasing.md](docs/releasing.md).

Sync updates itself: it looks once at each launch, verifies what it finds
against a key compiled into the binary, installs it and says nothing. The next
launch is the new version, and the menu bar item offers a restart to anybody who
would rather not wait. Nothing about that flow is reachable from the window's
content.

**The iPhone application is built from source.** It lives in `src-mobile/` —
its own Cargo workspace, showing the same static export the Mac shows — and this
release page carries the macOS installer only. `pnpm tauri ios dev "iPhone 17
Pro"` puts it on a simulator. The two carry one version deliberately: somebody
holding both should be able to say which is older without learning two numbering
schemes.

**Upgrading from the earlier Sync?** Read [If you came here looking for the old
Sync](#if-you-came-here-looking-for-the-old-sync) first: the two share no
settings and no data, and one thing does break on the agent's side.

## What that looks like in practice

**Four freshness states, and the engine derives every one.** `fresh`,
`unverified`, `stale`, `invalid` — never typed in, never a field somebody
maintains. `stale` means the paths a record named have moved since it was
written. The navigator counts them, so a project can be read by how much of what
it knows is still load-bearing.

**A record opens as the text it is.** No edit mode, no form: the caret goes
where you clicked and typing changes the record, title included. Markdown is the
format, so Markdown decides the feature set — and a body that would not survive
the round trip is shown read-only with the reason, rather than silently mangled.

**The schema is the project's, published at runtime.** The window knows nothing
about what a field means: an enumeration draws a picker, a flag draws a
checkbox, a relation offers exactly the targets the type declares. Add a type
and the interface follows without a line changing here.

**Removing has two meanings and both are offered.** Archive is reversible and
keeps every link. Delete first says what holds on to the record, in two numbers
— what links to it, and what mentions it in prose — because a record that named
this one is somebody's reasoning, and taking it silently would take the
reasoning with the conclusion.

**Connect an agent with one control.** One server on this machine answers for
every project it holds, and connecting writes exactly one entry into that
agent's own configuration file — an address, a header, and nothing else touched.
Disconnecting takes exactly that entry back out. The server outlives the last
window, so an agent asked something at midnight is not asking a closed
application; whether it also starts at login is a switch in Settings.

**A conversation can be held somewhere disposable.** An agent raised in a
working tree edits a copy of the project nobody else is looking at, and undoing
all of it is one gesture rather than a review of what changed under your own
open files. The trees are listed in Settings and thrown away from there, because
a tree outlives the conversation that made it and a copy nobody can name is a
disk that fills up.

**A conversation can delegate, and gets an answer rather than a transcript.**
The delegated run happens in the parent's own working tree, so the second agent
sees the first one's files. What travels back is the last thing the child said,
and nothing else. A chain is two conversations deep; a third is refused in
words. Nothing polls: an outcome is delivered when the conversation that asked
for it is open and between turns, and waits when it is not — raising an agent
two days later to hand it a paragraph would be spending somebody's money without
asking.

**A shell, when a package opens one.** The process is Sync's and the screen is
the package's: what it opens runs whatever is typed into it, in the folder it
was opened in, as the person who opened it, and the card says exactly that
because nothing narrower would be honest. A terminal belongs to the project
rather than to the section that opened it — leaving the section, hiding it or
reloading it changes nothing, and closing the project is what ends them.

**A secret is the system's.** What a package needs to reach a service is kept
in its own corner of the macOS keychain, and it is never handed to an agent. A
secret is not a fact about the project, so it is not in the project's memory:
that memory travels on a Git remote, and ciphertext that has left this machine
cannot be called back. Settings lists whose entry each one is and what it is
called — nothing there can answer with a value — and forgetting one is a row.

**A phone, if you pair one.** Settings shows a code, the phone's camera reads
it, and after that the two ends find each other by public key and talk over one
encrypted connection — hole punched where it can be and relayed where it cannot,
so *remote* means remote rather than *upstairs*. A device may watch a
conversation and is answered with a number; every word that conversation says
afterwards arrives under it as it is written, and watching from the phone does
not take the live view away from the window at the desk. Revoking a device is a
row in the same list.

**It talks to nothing else.** No account, no telemetry, no analytics, no crash
reporting — absent, not off by default. On its own the application makes two
kinds of outward request: the update check, and the extension registry when you
open the catalogue. Both go to GitHub, both are made in Rust with the reachable
hosts compiled into the binary, and the webview is granted no network origin at
all — its `connect-src` names the local IPC and nothing else. Everything beyond
those two is something a person switched on: a package reaches only the hosts
its manifest wrote out, and an installation that has never been asked for remote
access never binds a UDP socket or talks to a relay.

## How a project works

A project is a Git repository. Sync keeps what a project knows in the
repository's own refs, so a folder outside version control has nowhere to put
any of it.

Opening a folder asks as little as it can:

1. Not a repository? It says why that is a problem and offers `git init`.
   Declining ends the flow.
2. Already carries a project record? That is the whole flow — it opens with the
   name, description and language it was given the first time. Nothing is asked.
3. Otherwise it asks for those three, then shows the catalogue of extensions.

**A project's settings live in the project**, as the `project` record in the
repository's own memory, written in the same transaction that publishes the type
corpus. What is kept on this machine is what is true of the machine rather than
of any project: the recently opened list, the server's port and token, the
devices that may reach in, where working trees are made, the secrets a package
was granted, and the voice.

## Everything a project can do is a package

Sync is a shell. **There is no `src/extensions/` directory in this repository**,
and CI fails the build if one appears — every section of the project's window is
a package, built elsewhere, installed through the same path a stranger's
extension is installed through. One row of the sidebar is the window's own, and
it is the catalogue: where a person decides which sections the project has. That rule is the whole design: a window that could name
one section in a type or a constant would treat that section differently from
yours.

A package declares what it needs, and a build publishes what it can do. The
fourteen capabilities today:

`records` · `agents.acp` · `markdown.plugins` · `native-menu` · `folders` ·
`sheets` · `net` · `net.write` · `vault` · `background` · `schedule` ·
`work.agent` · `agent.tools` · `terminal`

A missing one is a refusal with a sentence a person can act on, never a silent
degradation. `net` is the one that is a capability *and* a list: a package that
wants the network names the exact hosts, without wildcards, and every redirect
is checked against the same list. Four of the fourteen are things that have to
happen where the screen is — raising a session, a keychain, a shell, a system
menu — so a phone honours ten, and a package that needs one of the other four is
drawn in its list with the reason it cannot open there. It stays installed:
what a manifest is checked against is a project, and a project is one repository
open on more than one machine.

The surface a package compiles against carries its own version, moving on its
own clock rather than the application's, and a manifest states a range over it.
It is **3.10.0** today, and `pnpm api:check` fails a build where the surface
moved and the number did not.

**What is published so far.** All of it is installed from the catalogue in the
window, and none of it is inside the application:

| | |
| --- | --- |
| **Records** | The project's types, and every record written as one. |
| **Project memory** | The answers a project has already worked out, so nobody works them out twice. |
| **Chat** | The coding agents installed on this machine, driven in this project's folder without leaving the window. |
| **Terminals** | Shells in the project's folder, arranged in tabs and tiles. |
| **Tasks** | The work a project has committed to, written so that finishing it can be checked. |
| **Routines** | Standing instructions an agent carries out on a clock, whether or not anybody is there. |
| **Issues** | The issues of a public repository on GitHub, read beside the project they are about. |
| **Posts** | Drafts of what a project will say outside itself, and the text of everything already said. |

**This is where contributions are most welcome.** You do not have to work on the
core to add something real — an extension is a package with its own types, its
own screen, its own background work and its own clock, and it installs into any
project without this repository changing:

- The contract is published as [`@sync-buzz/extension-api`](https://www.npmjs.com/package/@sync-buzz/extension-api).
  It carries the types, the version your manifest states a range against, and a
  CLI that packs and checks an archive.
- The packages above are built in
  [sync-buzz/sync-extensions](https://github.com/sync-buzz/sync-extensions) and
  are worth reading as worked examples.
- [docs/writing-an-extension.md](docs/writing-an-extension.md) builds one from
  nothing, with every file it contains.
- [docs/extension-architecture.md](docs/extension-architecture.md) is the seam
  drawn rather than argued: four boundaries, the manifest field by field, each
  lifecycle as a sequence, and every refusal with the place it is heard.
- [docs/extensions.md](docs/extensions.md) is the whole of how a package is
  built, loaded, permitted, published and updated, and
  [docs/background.md](docs/background.md) is the half of one that runs with no
  window open.

Ideas that need nothing from us: a reader for a document format, a review
surface, a dashboard over a type you invented, a section over whatever your team
already keeps in a folder.

## Where it stands

**Working:** the shell and both windows; opening a folder as a project; types,
records, folders and the document editor; the context panel; search; the memory
engine, its sync, merge and rewind; the server, and connecting seven clients to
it; ACP conversations with five agent CLIs, in the project's folder or in a
working tree that is discarded afterwards; conversations that delegate to
conversations; terminals; the vault; tools an agent calls that a package
answers; the extension format, loader, registry, marketplace and updates;
background service modules, their clocks and their work orders; the system
speech synthesiser; pairing a phone and holding a conversation from it;
self-updating releases.

**Nothing in the interface is a stub.** There are no disabled controls, no
cards that say "soon" and no sections that open onto an explanation of what will
one day be there. What you can see, you can use; what a project cannot do yet is
a package nobody has written.

**Known rough edges** are the ones a first release has: the catalogue is eight
packages deep, packages are not yet signed — the format and the verification are
there, the key is not — the memory engine is a young dependency on its own
release cadence, and the phone is built from source rather than downloaded.

## If you came here looking for the old Sync

A different application used to live at this address. It is preserved,
unmaintained, at **[sync-buzz/sync-legacy](https://github.com/sync-buzz/sync-legacy)**,
and its releases are still downloadable there.

**This is not a new version of it.** It is a different application with a
different data format, and there is no migration — nothing here reads what that
one wrote. If you are using it, it goes on working exactly as it does today:
copies in the field will **not** be updated to this, deliberately and by three
independent mechanisms. The two were signed with different keys, so a build from
this repository is not merely unwanted by an old copy but unverifiable by it.
The reasoning is in [docs/releasing.md](docs/releasing.md).

**They do not share settings or data.** The two bundles carry different
identifiers — `buzz.sync` here, `chat.sync.desktop` there — so macOS treats them
as separate applications and neither reads the other's configuration directory.

**Installing this one replaces that one.** Both bundle as `Sync.app` and want
the same path in `/Applications`, so there is no side-by-side unless you rename
one in Finder first. What gets replaced is the application; the old one's data
sits under its own identifier and is untouched, though nothing here can read it.

**One thing does break, and it is worth knowing before you install.** If you
connected an agent to the old Sync, that agent's configuration names a program
inside the bundle — `Sync.app/Contents/MacOS/git-sync` — and replacing the
bundle takes it away. From the agent's side the server simply stops starting.
This build serves the same purpose from one server on this machine, and it will
**not** quietly rewrite the entry: an entry under Sync's name that points
somewhere else is reported to you rather than overwritten, because somebody
else's configuration file is not ours to tidy. Open **Settings → Agents** after
installing and connect again; it is one control per agent.

## Building it

Prerequisites: macOS 13+, Xcode Command Line Tools, Node.js 20.9+ (built on 22),
pnpm 11 (`corepack enable` picks up the `packageManager` field), and Rust 1.91
or newer — the floor the workspace states, because Cargo resolves one graph
against the lowest one in it and a lower floor picks dependency versions the
engine cannot build with.

```sh
pnpm install       # from the lockfile
pnpm tauri dev     # build and run the application
pnpm tauri build   # produce the installer

pnpm lint          # eslint
pnpm typecheck     # tsc --noEmit
pnpm api:check     # the extension surface, and the number it is promised under
pnpm prose:check   # the prose rules a machine can hold — see AGENTS.md
pnpm build         # static export to ./out
```

`pnpm tauri:build` is the same bundle with the system's `PATH` in front: the
step that clears the quarantine attribute calls `xattr`, and a different one
earlier on your path fails the build after everything has compiled.

For the Rust side, from `src-tauri`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The phone is a second application rather than a second target of the first, so
it has its own workspace and its own commands, which go through
`scripts/tauri.mjs`:

```sh
pnpm tauri ios dev "iPhone 17 Pro"   # onto a simulator
pnpm tauri ios build                 # onto a device
```

**Build the memory sidecar before bundling.** `sync-mcp` ships inside the bundle
as a Tauri `externalBin`; it *links* the memory engine as a library rather than
spawning it, and serves three doors from one dispatcher — an agent gets MCP, the
window and the clock get a channel of product operations that is not MCP and
appears in no `tools/list`, and a paired device gets that same channel over an
encrypted connection.

```sh
./scripts/prepare-sidecar.sh                        # build it from this workspace
./scripts/prepare-sidecar.sh --from /path/to/binary # stage one already built
```

The engine is pinned to a tag in `sync-mcp`'s manifest and resolved from GitHub
like any other dependency, so a clean checkout builds exactly what a release is
cut against and needs no directory beside it. In development, point Sync at your
own build with `SYNC_MCP_BINARY=/path/to/sync-mcp` — there is no search of
`PATH`, because a stray `sync-mcp` would carry a different engine inside it and
nothing would say so.

### Two constraints worth knowing before you write code

**Next.js is a build system here, never a server.** `output: "export"` makes the
whole application a folder of static assets that Tauri embeds. Nothing needing a
Node.js runtime after packaging can be used: no SSR, no Route Handlers, no
Server Actions, no middleware, no rewrites or headers, no image optimizer. System
access arrives through Tauri commands.

**The Mac App Store is ruled out.** The native `underWindowBackground` material
requires a transparent window, which on macOS requires `app.macOSPrivateApi`, and
an application built on a private API cannot be published there. Distribution is
direct. The reasoning is in
[docs/design-foundation.md](docs/design-foundation.md).

Two dependencies are deliberately **not** on their newest major: TypeScript,
because `typescript-eslint` (via `eslint-config-next`) does not yet accept the
next one, and ESLint, because `eslint-plugin-import`, `eslint-plugin-jsx-a11y`
and `eslint-plugin-react` declare support only through 9. Both are pinned in
`pnpm-lock.yaml`; revisit when the upstream configs catch up.

## Documentation

- [docs/design-foundation.md](docs/design-foundation.md) — the visual system,
  the panel roles, and the rules for changing them. **Read this first** before
  touching the window.
- [docs/architecture.md](docs/architecture.md) — how Tauri, Next.js, the Rust
  crates and the memory process fit together, and the three doors into the
  channel
- [docs/extensions.md](docs/extensions.md) — what a package is, how the window
  decides it may run one, where packages come from, and how one is updated
- [docs/extension-architecture.md](docs/extension-architecture.md) — the same
  subject drawn: the boundaries in order, the manifest field by field, and every
  refusal with the place it is heard
- [docs/writing-an-extension.md](docs/writing-an-extension.md) — one package
  built from nothing, with every file it contains
- [docs/background.md](docs/background.md) — work that runs with no window open
- [docs/voice.md](docs/voice.md) — what the application says out loud, and who
  may ask it to
- [docs/releasing.md](docs/releasing.md) — cutting a release, and the separate
  act that ships it to installed copies

`AGENTS.md` is the short version for a coding agent working in this repository.

## Licence

[Functional Source License 1.1, MIT Future License](./LICENSE) — source-available.
Use it, modify it, redistribute it for any purpose except building a product or
service that competes with Sync; internal use, education and research are named
as permitted. Each version becomes plain MIT two years after it is released.

## Security

Please report a suspected vulnerability privately to **security@sync.buzz**
rather than in a public issue. [SECURITY.md](./SECURITY.md) also describes what
this application reaches and what it deliberately does not.
