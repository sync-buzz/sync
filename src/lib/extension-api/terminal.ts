"use client";

import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  ExtensionTerminal,
  TerminalEvent,
  TerminalOpening,
  TerminalRow,
  TerminalSize,
} from "@/lib/extension-api/contract";

/**
 * A shell, in a folder, with a screen somewhere else.
 *
 * Here rather than beside the rest of the surface's functions for the reason
 * `net` is: **opening one is about the extension rather than about the
 * project.** Whether a package may is a sentence in its own manifest, so the
 * call has to arrive in Rust attributed to a package, and an id passed as an
 * argument by whoever called would be an extension naming its own permission.
 *
 * So this is not exported. The host builds one per package while it is
 * activating it, with the id closed over, and hands it over as `host.terminal`.
 *
 * The capability is read when a terminal is opened and not on every keystroke:
 * reading it means resolving the package on disk and parsing its manifest, and
 * a file read behind every character typed is a terminal that stutters. So the
 * id goes with **every** call, not only the first, and Rust refuses a terminal
 * to anybody but the package that raised it — as strong as reading the manifest
 * again, for the price of comparing two strings.
 *
 * A terminal's name is a counter and is not a secret. It does not have to be:
 * guessing one buys nothing without the name of whoever opened it, and that is
 * closed over here rather than passed by the caller.
 */
export function terminalFor(id: string): ExtensionTerminal {
  return {
    open: (opening: TerminalOpening) =>
      invoke<string>("terminal_open", {
        extension: id,
        project: opening.project,
        cwd: opening.cwd,
        size: opening.size,
      }),

    write: (terminal: string, data: string) =>
      invoke<void>("terminal_write", { extension: id, id: terminal, data }),

    resize: (terminal: string, size: TerminalSize) =>
      invoke<void>("terminal_resize", { extension: id, id: terminal, size }),

    // The channel is made here and never handed back, which is what keeps the
    // caller from having to know it is a channel at all: what a package holds
    // is a function that is called with what happened. Rust stops sending when
    // the terminal ends, when it is closed, and when another screen takes it
    // over — so there is nothing here that has to be torn down by hand.
    watch: (
      terminal: string,
      from: number,
      onEvent: (event: TerminalEvent) => void,
    ) => {
      const events = new Channel<TerminalEvent>();
      events.onmessage = onEvent;
      return invoke<void>("terminal_watch", {
        extension: id,
        id: terminal,
        from,
        events,
      });
    },

    list: (project: string) =>
      invoke<readonly TerminalRow[]>("terminal_list", { extension: id, project }),

    close: (terminal: string) =>
      invoke<void>("terminal_close", { extension: id, id: terminal }),

    closeProject: (project: string) =>
      invoke<void>("terminal_close_project", { extension: id, project }),
  };
}
