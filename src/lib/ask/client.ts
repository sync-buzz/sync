"use client";

/**
 * What the panel asks the application for, and the one thing it is told.
 *
 * Four calls, and none of them is about *what a line means*: the panel reads a
 * line with the console's engine, which is already the only reading of one in
 * this window. What is here is the surface — which project it speaks for, how
 * tall it is, and when it was opened — because all three are facts the panel
 * cannot know from inside its own webview.
 */

import { useEffect, useRef } from "react";

import { command } from "@/lib/command";
import type { RecentProject } from "@/lib/project/types";

/** Said at the panel when the key opened it. */
const OPENED = "ask:opened";

/**
 * Put the panel away.
 *
 * Escape, a line that has been taken, and a click that landed somewhere else
 * all mean this. The panel decides *when*, because what closes a surface is a
 * question about the surface.
 */
export function dismissPanel(): Promise<void> {
  return command<void>("ask_dismiss", {});
}

/**
 * The project the panel speaks for, as it was when the key was pressed.
 *
 * `null` means nobody had a project open, which is the ordinary state of an
 * application sitting in the menu bar. The panel asks a person which project
 * they mean rather than guessing one, so this answer is the whole of the
 * difference between the two things it can open as.
 */
export function panelProject(): Promise<RecentProject | null> {
  return command<RecentProject | null>("ask_project", {});
}

/** Speak for this project from now on, because somebody said so out loud. */
export function panelUse(project: string): Promise<void> {
  return command<void>("ask_use", { project });
}

/**
 * Be exactly this tall.
 *
 * Quiet, because nobody is waiting on it: the panel measures itself on every
 * keystroke that changes what it is showing, and a window that reported each of
 * those as work in flight would pulse for as long as somebody was typing.
 */
export function panelHeight(height: number): Promise<void> {
  return command<void>("ask_height", { height }, { quiet: true });
}

/**
 * Do this each time the key opens the panel.
 *
 * The panel is built once and shown many times, so *opened* is an event rather
 * than a mount: everything a fresh surface does — clearing the line, taking the
 * caret, reading which project is in front — happens here and not in an effect
 * that runs once.
 *
 * @param opened What to do about it. Held in a ref, so a caller need not
 *   memoise one and the listener is registered once for the life of the panel.
 */
export function useOpened(opened: () => void): void {
  const latest = useRef(opened);
  // In an effect rather than in the body: a ref written while rendering is a
  // render with an effect in it, and React says so out loud.
  useEffect(() => {
    latest.current = opened;
  });

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    let listening = true;
    let stop: (() => void) | undefined;

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen(OPENED, () => latest.current());
      // Unmounted while the listener was registering. Dropping it here is what
      // keeps a surface that is gone from being told it was opened.
      if (listening) stop = unlisten;
      else unlisten();
    })();

    return () => {
      listening = false;
      stop?.();
    };
  }, []);
}
