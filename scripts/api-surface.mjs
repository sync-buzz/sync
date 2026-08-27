#!/usr/bin/env node
/**
 * The extension surface, extracted and compared.
 *
 * `SYNC_API_VERSION` is a promise a package is allowed to believe, so something
 * has to notice when the surface moves and the number does not. That is this:
 * declarations are emitted, API Extractor reduces them to a report, and the
 * report is compared with the one in `api/`. A difference fails the build and
 * names both halves of what to do about it.
 *
 *   node scripts/api-surface.mjs           check — fails on any difference
 *   node scripts/api-surface.mjs --update  accept the current surface
 *
 * One compiler diagnostic is filtered and it is worth stating why. TS2742 — "the
 * inferred type cannot be named without a reference to <pnpm store path>" — is
 * a property of how pnpm lays out `node_modules`, not of this code. It appears
 * only when declarations are emitted, so `pnpm typecheck` never reports it, and
 * the two occurrences are the Plate plugin array, which is nowhere near the
 * boundary. If such a type ever did reach the surface, API Extractor would
 * refuse it as a forgotten export rather than let it through.
 */

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const update = process.argv.includes("--update");

/** Runs a command from the repository root and hands back what it said. */
function run(command, args) {
  return spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    shell: false,
  });
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// 1. Declarations.
// ---------------------------------------------------------------------------

const emitted = run("pnpm", ["exec", "tsc", "-p", "tsconfig.api.json"]);
const diagnostics = `${emitted.stdout ?? ""}${emitted.stderr ?? ""}`
  .split("\n")
  .filter((line) => line.trim() !== "")
  .filter((line) => !line.includes("error TS2742"));

if (diagnostics.length > 0) {
  fail(
    `The declaration build failed:\n${diagnostics.join("\n")}\n\n` +
      "These are real type errors. `pnpm typecheck` should have caught them.",
  );
}

// ---------------------------------------------------------------------------
// 2. The report.
// ---------------------------------------------------------------------------

const extracted = run("pnpm", [
  "exec",
  "api-extractor",
  "run",
  ...(update ? ["--local"] : []),
]);
const said = `${extracted.stdout ?? ""}${extracted.stderr ?? ""}`;

if (extracted.status !== 0) {
  // API Extractor's own words, and it says them as a warning — which it then
  // treats as an error, because this run is not `--local`. That is the design:
  // in CI a surface that moved is a failure, not a note.
  const changed = said.includes("You have changed the API signature");
  if (!changed) fail(said.trim());

  const version = readVersion();
  fail(
    [
      "",
      "The extension API surface has changed.",
      "",
      `This build publishes SYNC_API_VERSION ${version}. Decide which of the`,
      "three it was, in `src/lib/extension-api/version.ts`:",
      "",
      "  removed / renamed / narrowed an export   -> major",
      "  added an export or an optional field     -> minor",
      "  neither                                  -> the change is not a surface change",
      "",
      "Then accept the new surface:",
      "",
      "  pnpm api:update",
      "",
      "and commit `api/extension-api.api.md` with the version beside it.",
      "",
      said.trim(),
    ].join("\n"),
  );
}

/** The number the report is supposed to move with. */
function readVersion() {
  const source = readFileSync(
    join(root, "src/lib/extension-api/version.ts"),
    "utf8",
  );
  return /SYNC_API_VERSION = "([^"]+)"/.exec(source)?.[1] ?? "unknown";
}

// ---------------------------------------------------------------------------
// 3. The half a matching report cannot answer.
//
// Everything above catches "the surface moved and nobody wrote it down". It
// cannot catch "somebody wrote it down and left the version alone" — that
// commit has a report which matches perfectly. So the two are compared against
// what is already in Git: if the report changed and the number beside it did
// not, the promise the number makes has quietly stopped being true.
//
// Skipped when there is no Git to ask, which is a tarball or a fresh worktree
// rather than a state worth failing over.
// ---------------------------------------------------------------------------

if (!update) {
  const against = process.env.API_SURFACE_BASE ?? "HEAD";
  const changed = (path) =>
    run("git", ["diff", "--quiet", against, "--", path]).status === 1;

  const reachable = run("git", ["rev-parse", "--verify", against]).status === 0;
  if (reachable && changed("api/extension-api.api.md")) {
    if (!changed("src/lib/extension-api/version.ts")) {
      fail(
        [
          "",
          `The API report differs from ${against}, and \`version.ts\` does not.`,
          "",
          "A report that moves while the number stands still is the failure this",
          "check exists for: a package states a range against SYNC_API_VERSION,",
          `and this build would keep publishing ${readVersion()} for a surface`,
          "that is no longer the one it described.",
          "",
          "Removed, renamed or narrowed an export -> major. Added one -> minor.",
        ].join("\n"),
      );
    }
  }
}

process.stdout.write(
  update
    ? `Surface accepted at SYNC_API_VERSION ${readVersion()}.\n`
    : `Surface unchanged at SYNC_API_VERSION ${readVersion()}.\n`,
);
