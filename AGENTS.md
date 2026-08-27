# Working in this repository

Sync is a macOS desktop application: a Tauri shell around a statically exported
Next.js frontend, with the domain in Rust crates under `src-tauri/crates/`. It
gives a project a memory kept in that project's own Git repository, publishes it
to agents over MCP, and hosts agent conversations over ACP. Everything a project
can *do* is an extension package installed into it.

## Read before you write

- **[docs/design-foundation.md](docs/design-foundation.md) outranks the rest.**
  It is what the window is and why. Nothing is added to the window that it has
  not agreed to; if your change and this document disagree, the document wins
  until a person says otherwise.
- [docs/architecture.md](docs/architecture.md) — the process model, the command
  layer, where a fact lives, and the repository map.
- [docs/extensions.md](docs/extensions.md) — the package format, capabilities,
  the loader, the registry, updates.
- [docs/background.md](docs/background.md) — service modules, clocks, work
  orders: the half of an extension with no screen.
- [docs/voice.md](docs/voice.md), [docs/releasing.md](docs/releasing.md).

## Three rules the compiler cannot hold

CI holds them instead, which means breaking one is a red build rather than a
defect discovered months later.

1. **No extension lives in this repository.** There is no `src/extensions/`, and
   nothing may resolve a path into one. Extensions are built elsewhere and
   installed as packages.
2. **`sync-memory` is the only door to the engine.** Nothing else opens
   `refs/memory/*`, touches the search index, or loads a model. The isolation is
   what makes a crash in ggml a reconnect rather than a lost window.
3. **The extension surface is versioned, and the number must move with it.**
   `pnpm api:check` fails both when the surface moved and the report was not
   updated, and when the report was updated and `SYNC_API_VERSION` was left
   behind. The second is the quiet one: a package states a range against that
   number and would go on believing it.

## Verify with these, and read the exit code

```sh
pnpm lint && pnpm typecheck && pnpm api:check && pnpm build
cd src-tauri
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not pipe a check into `tail` or `head` and trust the result — the exit code
becomes the pager's, and a failing `cargo fmt --check` reads as a pass.

`cargo check` builds the workspace's default members, which **exclude**
`sync-mcp`: it links the memory engine, and building it is minutes rather than
seconds. Reach it deliberately with `-p sync-mcp`.

## What lives where

| | |
| --- | --- |
| `src/components/shell/` | the shell, one file per panel role |
| `src/lib/extension-api/` | the contract packages compile against, and its version |
| `src/lib/extension-host/` | loading a package, its areas, badges, clocks, marketplace |
| `src/lib/memory/`, `src/lib/project/` | typed clients over the Tauri commands |
| `src-tauri/src/*.rs` | the command layer: parse, call a domain function, map the result |
| `src-tauri/crates/` | the domain, every crate compiling without Tauri |

The division between a crate and the module beside it is deliberate: a crate
that cannot see the application cannot widen its own reach, so *who may ask* is
decided in `src-tauri/src/`, and *how it is done* in the crate.

Every `invoke` command is declared in one list in `src-tauri/src/lib.rs`. Adding
one means adding it there, and what the webview may ask for stays readable in a
single place.

## Traps that have already cost time

- **The CSP applies only to a packaged build.** `tauri dev` loads over http,
  where Tauri does not apply it at all. Any CSP change is verified against
  `pnpm tauri build`, never against `pnpm tauri dev`.
- **A new field on a record does not cross the boundary by itself.** The window
  and the engine agree on a shape; an unknown member is dropped without an
  error. Write it, read it back, and cover it with a test that does both.
- **Green unit tests say nothing about the outside world.** The tests that
  reach GitHub are `#[ignore]`d and have to be asked for:
  `cargo test -p sync-extensions --test live_registry -- --ignored`.
- **Changing what a project record carries means rebuilding the sidecar**, or
  the window talks to an engine that has never heard of the field.
- **Extensions are linked, not compiled in.** Reseed the bundled archives with
  `pnpm extensions:seed` when the packages they came from move.

## Contributing an extension rather than core

Most new capability belongs in a package, not here. The contract is published as
`@sync-buzz/extension-api` and carries a CLI that packs and checks an archive;
the packages Sync ships with are built in
[sync-buzz/sync-extensions](https://github.com/sync-buzz/sync-extensions) and
read well as worked examples. Start from
[docs/extensions.md](docs/extensions.md).

## Prose in this repository

Comments and documents here explain *why*, name what was rejected, and say what
goes wrong when a rule is broken. They do not restate what the code already
says. Match that when you add to them — and do not cite anything a reader cannot
open: no internal record keys, no private branches, no paths from somebody's
machine.

<!-- BEGIN:nextjs-agent-rules -->

# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->
