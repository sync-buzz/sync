#!/usr/bin/env node

// Which of the two applications a `tauri` command is about.
//
// The CLI decides that by finding `src-tauri` beside the directory it was
// started in, and this repository holds a second one in `src-mobile`. The
// phone's commands cannot be distinguished by a `cd` into it: the generated
// Xcode project calls the CLI back as `pnpm tauri ios xcode-script …` from
// inside `src-mobile/gen/apple`, and `pnpm` runs a script from the root of the
// package it belongs to — so by the time the CLI starts, the directory
// somebody chose is gone. `ios` is what is left to decide by, and it is enough:
// the desktop application has no iOS commands.
//
// `TAURI_APP_PATH` is the CLI's own way of being told, and it is read relative
// to the working directory, which is why this sets one.

import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);

const child = spawn(
  process.execPath,
  [join(root, "node_modules/@tauri-apps/cli/tauri.js"), ...args],
  {
    stdio: "inherit",
    cwd: root,
    env:
      args[0] === "ios"
        ? { ...process.env, TAURI_APP_PATH: "src-mobile" }
        : process.env,
  },
);

// A terminal's ^C reaches both of us on its own, but a `kill` aimed at this
// process reaches only this one — and what it would leave behind is a CLI still
// holding the build lock, which the next command then waits on for ever.
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    child.kill(signal);
  });
}

child.on("exit", (code, signal) => {
  // A signal that killed the CLI is reported as one rather than as a plain
  // failure: Xcode reads the difference, and so does anybody who pressed ^C.
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
