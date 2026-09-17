"use client";

import { Channel } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef } from "react";

import { command } from "@/lib/command";
import type { BlockState, Presentation } from "@/lib/console/types";

/**
 * A line handed to a process, from the moment it starts to the code it ends
 * with.
 *
 * **The process is the host's and the drawing is this window's.** What arrives
 * here is bytes in the order they were written, by an offset this side names —
 * the ring they come out of is `sync-terminal`, the same one under the package
 * that draws terminals, so nothing about how a stream is kept is decided twice.
 *
 * **No emulator yet, and the block says so by what it can draw.** A screen that
 * moves the cursor about — a progress bar, anything full-screen — needs one,
 * and until there is one the sequences that would have moved it are dropped
 * rather than printed as rubble. What that leaves is what most commands
 * actually write: lines, in order.
 */

/** What a terminal says while it is running, as the host spells it. */
type TerminalEvent =
  | { readonly kind: "output"; readonly at: number; readonly gapped: boolean; readonly base64: string }
  | { readonly kind: "ended"; readonly code: number; readonly signal: string | null }
  | { readonly kind: "gone" };

/**
 * The size a process is told it has.
 *
 * Spelled `cols` because that is what `sync-terminal` calls it. The two sides
 * agree on a shape and an unknown member is dropped without a word — this was
 * `columns` first, and every line refused on deserialisation while the types on
 * this side were perfectly happy.
 */
const SIZE = { cols: 120, rows: 30 } as const;

/**
 * How much of one process's output a block keeps.
 *
 * The ring in the host holds 256 KB per process; this is what one block draws
 * out of it. A build that printed a hundred thousand lines would otherwise put
 * every one of them in the canvas, and the canvas holds every block.
 */
const KEEP = 64 * 1024;

function start(project: string, tab: string, line: string): Promise<string> {
  // The folder is not sent: the host holds where a tab is working, so a caller
  // naming one would be a caller choosing where a command runs.
  return command<string>("console_shell_start", {
    project,
    tab,
    line,
    size: SIZE,
  });
}

/**
 * What a run has printed so far, with the sequences an emulator would have
 * acted on taken out.
 *
 * Colour is dropped along with the rest, and that is a decision rather than an
 * omission: this window has the sixteen colours to draw it in, but a stretch of
 * text is coloured by sequences that arrive around it, and reading them is the
 * emulator's job. Half-reading them is how a console ends up printing the
 * escape and the colour both.
 */
function readable(text: string): string {
  return (
    text
      // CSI: what moves the cursor, clears the screen and sets colour.
      .replace(/\[[0-?]*[ -/]*[@-~]/gu, "")
      // OSC: what sets the window title, hyperlinks and the working directory.
      .replace(/\][^]*(?:|\\)/gu, "")
      // What is left of the two-character escapes.
      .replace(/[@-Z\\-_]/gu, "")
      // A carriage return without a newline redraws the line it is on, which
      // is what a progress bar does. Kept as a line break so the last state is
      // readable rather than overwritten into a smear.
      .replace(/\r\n?/gu, "\n")
  );
}

/**
 * Runs a line and answers with everything it printed.
 *
 * Its own path beside the watched one, because what a step of a chain needs is
 * the opposite of what a block needs: a block is drawn as the bytes arrive, and
 * a step has nothing to hand on until the process is over.
 */
export function collect(project: string, tab: string, line: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const decoder = new TextDecoder();
    let text = "";
    void start(project, tab, line)
      .then(async (terminal) => {
        const events = new Channel<TerminalEvent>();
        events.onmessage = (event) => {
          if (event.kind === "output") {
            const bytes = Uint8Array.from(atob(event.base64), (character) =>
              character.charCodeAt(0),
            );
            text += decoder.decode(bytes, { stream: true });
            return;
          }
          void command("console_shell_close", { id: terminal });
          // A command that failed still said something, and what it said is
          // usually why. The code is not carried on: the next step is being
          // given material to read, not a verdict to act on.
          resolve(readable(text));
        };
        await command("console_shell_watch", { id: terminal, from: 0, events });
      })
      .catch(reject);
  });
}

export function useShell(
  project: string,
  /** Called whenever a run has more to show, and once more when it ends. */
  onChange: (id: string, state: BlockState, result: Presentation) => void,
): {
  /** Run a line. Answers nothing: the block is fed through `onChange`. */
  readonly run: (id: string, tab: string, line: string) => void;
} {
  const tell = useRef(onChange);
  useEffect(() => {
    tell.current = onChange;
  }, [onChange]);

  /** What is open, so that unmounting does not leave processes unwatched. */
  const open = useRef(new Set<string>());
  /** Whether this screen is still here to be drawn into. */
  const watching = useRef(true);
  useEffect(() => {
    const held = open.current;
    watching.current = true;
    return () => {
      watching.current = false;
      for (const id of held) void command("console_shell_close", { id });
      held.clear();
    };
  }, []);

  const run = useCallback(
    (id: string, tab: string, line: string) => {
      let text = "";
      let gapped = false;
      // One decoder for the run, because a chunk can end in the middle of a
      // character: decoding each chunk on its own turns the two halves of one
      // letter into two wrong ones, and base64 was chosen upstream precisely so
      // the bytes would arrive whole.
      const decoder = new TextDecoder();

      const show = (state: BlockState, code: number | null) => {
        tell.current(id, state, {
          view: "stream",
          // A gap is said once, at the top, rather than marked where it
          // happened: the bytes that would have said where are the bytes that
          // were dropped.
          text: gapped ? `… earlier output was dropped\n${readable(text)}` : readable(text),
          code,
        });
      };

      void start(project, tab, line)
        .then(async (terminal) => {
          // The shade may have gone while this was starting, and its cleanup
          // has already emptied the set: closing here rather than adding to a
          // set nobody will read again is what keeps that process from being
          // left behind.
          if (!watching.current) {
            void command("console_shell_close", { id: terminal });
            return;
          }
          open.current.add(terminal);
          const events = new Channel<TerminalEvent>();
          events.onmessage = (event) => {
            if (event.kind === "output") {
              if (event.gapped) gapped = true;
              const bytes = Uint8Array.from(atob(event.base64), (character) =>
                character.charCodeAt(0),
              );
              text += decoder.decode(bytes, { stream: true });
              if (text.length > KEEP) {
                text = text.slice(text.length - KEEP);
                gapped = true;
              }
              show("running", null);
              return;
            }
            open.current.delete(terminal);
            // Closed as soon as it ends, because the host keeps a terminal's
            // ring — 256 KB of it — until somebody closes the terminal. A
            // console that only stopped watching would leave every line it ever
            // ran resident until the application quits.
            void command("console_shell_close", { id: terminal });
            if (event.kind === "ended") {
              // A code is the process's own verdict and is trusted as one:
              // zero is done, anything else failed, and the number is printed
              // beside it rather than translated into a sentence this window
              // would be inventing.
              show(event.code === 0 ? "done" : "failed", event.code);
              return;
            }
            show("failed", null);
          };
          await command("console_shell_watch", {
            id: terminal,
            from: 0,
            events,
          });
        })
        .catch((error: unknown) => {
          tell.current(id, "failed", {
            view: "text",
            text: `console: ${
              typeof error === "string"
                ? error
                : error instanceof Error
                  ? error.message
                  : "that could not be run"
            }`,
          });
        });
    },
    [project],
  );

  return { run };
}
