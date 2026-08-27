# Sync

**A desktop application that gives a project a memory — and tells you when that
memory has gone out of date.**

Every project accumulates things that are true but written nowhere the code can
hold them: why this was chosen over that, what must never be done here, what
somebody found out the hard way, what nobody has settled yet. They live in chat
logs, in issue comments, in one person's head. Sync keeps them in the project's
own Git repository, beside the code they are about, and reconciles them against
the code's history — so a claim the code has moved out from under is **marked as
stale rather than quietly believed**.

Agents read the same memory over MCP. That is the point of keeping it this way:
what you write down once is what your agents know.

> **Pre-1.0 and under active development.** It is used daily on the machine it
> is built on, and it is not finished. What is here works; what is absent is
> absent rather than stubbed. See [Where it stands](#where-it-stands).

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

## What it gives you

**Knowledge that lives in the repository.** A project's memory is written to Git
objects under `refs/memory/*`. It travels with the repository, versions itself,
pushes to the same remote and puts nothing in the working tree. Clone the
project somewhere else and it is the same project, with the same memory. There
is no account and no server of ours between you and it.

**Records that know when they have rotted.** A record can name the paths its
claim is about. The engine reconciles those paths against the repository's
history and answers `fresh`, `unverified`, `stale` or `invalid` — and `stale`
means the code moved under the claim. It is derived, never typed in. This is the
one thing a wiki cannot do.

**Types you declare, not types we chose.** A decision, a constraint, an
observation, a question, a task, a routine — each is a type with its own fields,
its own relationships and its own rules, published into the project. The window
knows nothing about what a field means: an enumeration draws a picker, a flag
draws a checkbox, and the control comes from the declaration.

**Documents, not form fields.** A record opens as the text it is, with no edit
mode — the caret goes where you clicked. Markdown is the format, so Markdown
decides the feature set, and a body that would not survive the round trip is
shown read-only with the reason rather than silently mangled.

**Search that admits what it did not find.** Words are matched first, meaning
second, and each hit says which it was. When nothing matched by words the
palette says so — *"No words matched. Nearest by meaning:"* — instead of
presenting the nearest record as an answer. There is no relevance threshold,
because measurement showed no threshold separates the relevant from the
irrelevant.

**Agents, from both directions.** Connect Claude Code, Codex, Gemini CLI, Grok
or OpenCode to Sync over MCP with one control in settings, and they can read the
project's memory. Or drive one *from* Sync over ACP, in the window, with its
plan, its tool calls and its permission prompts drawn as part of the interface.

**Memory merges like code.** `refs/memory/main` fetches and pushes on a remote of
its own. A fetch is a three-way merge, member by member: where only the other
side moved, theirs is taken; where both moved and disagree, **yours is kept and
the member is named**. Nothing is silently absorbed, nothing is destroyed, and
`memory_rewind` puts memory back where a fetch found it.

**It talks to nothing else.** No account, no telemetry, no analytics, no crash
reporting — absent, not merely off by default. The two outward requests the
application makes on its own are the update check and the extension registry,
both to GitHub, both in Rust with the reachable hosts compiled in. The webview
is granted no `connect-src` at all.

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
corpus. The only thing kept on this machine is the list of recently opened
projects; losing it costs a shorter menu and nothing else.

## Everything a project can do is a package

Sync is a shell. **There is no `src/extensions/` directory in this repository**,
and CI fails the build if one appears — Records, Chat and Project memory are
packages like any other, built elsewhere, installed through the same path a
stranger's extension is installed through. That rule is the whole design: a
window that could name one section in a type or a constant would treat that
section differently from yours.

A package declares what it needs, and a build publishes what it can do. The ten
capabilities today:

`records` · `agents.acp` · `markdown.plugins` · `native-menu` · `folders` ·
`sheets` · `net` · `background` · `schedule` · `work.agent`

A missing one is a refusal with a sentence a person can act on, never a silent
degradation. `net` is the one that is a capability *and* a list: a package that
wants the network names the exact hosts, without wildcards, and every redirect is
checked against the same list.

**This is where contributions are most welcome.** You do not have to work on the
core to add something real — an extension is a package with its own types, its
own screen, its own background work and its own clock, and it installs into any
project without this repository changing:

- The contract is published as [`@sync-buzz/extension-api`](https://www.npmjs.com/package/@sync-buzz/extension-api).
  It carries the types, the version your manifest states a range against, and a
  CLI that packs and checks an archive.
- The three packages Sync ships with are built in
  [sync-buzz/sync-extensions](https://github.com/sync-buzz/sync-extensions) and
  are worth reading as worked examples.
- [docs/extensions.md](docs/extensions.md) is the whole of how a package is
  built, loaded, permitted, published and updated.
- [docs/background.md](docs/background.md) is the half of an extension that runs
  with no window open.

Ideas that need nothing from us: an issue tracker over your forge, a reader for
a document format, a review surface, a dashboard over a type you invented.

## Where it stands

**Working:** the shell and both windows; opening a folder as a project; types,
records, folders and the document editor; the context panel; search; the memory
engine, its sync, merge and rewind; the MCP interface and connecting an agent to
it; ACP conversations with five agent CLIs; the extension format, loader,
registry, marketplace and updates; background service modules and their clocks;
the system speech synthesiser; self-updating releases.

**Nothing in the interface is a stub.** There are no disabled controls, no
cards that say "soon" and no sections that open onto an explanation of what will
one day be there. What you can see, you can use; what a project cannot do yet is
a package nobody has written.

**Known rough edges** are the ones a first release has: the extension catalogue
is small, packages are not yet signed — the format and the verification are
there, the key is not — and the memory engine is a young dependency on its own
release cadence.

## Building it

Prerequisites: macOS 13+, Xcode Command Line Tools, Node.js 20.9+ (developed on
23.11), pnpm 11 (`corepack enable` picks up the `packageManager` field), and
Rust stable.

```sh
pnpm install       # from the lockfile
pnpm tauri dev     # build and run the application
pnpm tauri build   # produce the installer

pnpm lint          # eslint
pnpm typecheck     # tsc --noEmit
pnpm api:check     # the extension surface, and the number it is promised under
pnpm build         # static export to ./out
```

For the Rust side, from `src-tauri`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

**Build the memory sidecar before bundling.** `sync-mcp` ships inside the bundle
as a Tauri `externalBin`; it *links* the memory engine as a library rather than
spawning it, and serves two callers over stdio — an agent gets MCP, the window
gets a channel of product operations that is not MCP and appears in no
`tools/list`.

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
  crates and the memory process fit together
- [docs/extensions.md](docs/extensions.md) — what a package is, how the window
  decides it may run one, where packages come from, and how one is updated
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
