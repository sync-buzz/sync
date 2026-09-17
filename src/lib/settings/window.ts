"use client";

/**
 * Which window a document is, and how the settings window is opened.
 *
 * Sync ships one HTML document and opens it in two windows. Which one this is
 * cannot be a route: the frontend is a static export, so a second route is a
 * second file that has to resolve the same way under the dev server and inside
 * the bundle. The window's label answers it without that, and it is answered by
 * the same window that carries the setting — Tauri hands the label to the
 * document before any of it runs.
 *
 * Outside Tauri — `pnpm dev` in a browser — there is one window and it is the
 * main one.
 */

import { useEffect, useRef, useSyncExternalStore } from "react";
import { command } from "@/lib/command";

export type WindowRole = "main" | "settings";

/** The label the Rust side builds the settings window under. */
const SETTINGS_LABEL = "settings";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * The role of the window this document is running in, read straight from the
 * label. Exported because it is also the answer to what a window is allowed to
 * ask the platform for — a capability is granted per label, so anything that
 * addresses the window itself has to know which one it is addressing, and it
 * has to know synchronously rather than on a later commit.
 */
export function windowRole(): WindowRole {
  if (!inTauri()) return "main";

  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__?: {
        metadata?: { currentWindow?: { label?: string } };
      };
    }
  ).__TAURI_INTERNALS__;

  return internals?.metadata?.currentWindow?.label === SETTINGS_LABEL
    ? "settings"
    : "main";
}

/**
 * The role, read the way the loading state is read in `window-reveal.ts`.
 *
 * The exported HTML is one file, so the server snapshot has to be the main
 * window; the settings window corrects it on its first commit, while it is
 * still hidden. Reading it through `useState` instead would be the same answer
 * arriving as a hydration mismatch.
 */
export function useWindowRole(): WindowRole {
  return useSyncExternalStore(subscribeNever, windowRole, serverRole);
}

function subscribeNever() {
  // The label a window was built with never changes.
  return () => undefined;
}

function serverRole(): WindowRole {
  return "main";
}

/**
 * Open the settings window, or bring the open one forward.
 *
 * @param section Where somebody was sent, when they were sent somewhere. Left
 *   out is the ordinary case and means *wherever you were*: pressing `⌘,` asks
 *   for settings rather than for one of them, and landing somebody on a screen
 *   they did not ask for is worse than landing them where they last looked.
 *
 *   A control that sends somebody to a particular screen owes them that screen,
 *   which is the whole of why this argument exists: *choose an agent* that
 *   opened a window about fonts would be a control that did not do what it
 *   said.
 */
export async function openSettings(section?: string): Promise<void> {
  if (!inTauri()) return;

  try {
    // No project travels with the request. Everything in settings is this
    // Mac's: one server answers for every project, so connecting an agent no
    // longer names one.
    await command("settings_open", { section: section ?? null });
  } catch (error) {
    // Nothing to fall back to: there is no in-window settings surface to show
    // instead, so the failure is said out loud rather than swallowed.
    console.error("The settings window could not be opened.", error);
  }
}

/**
 * Which section this window was sent to, if it was sent to one, taking the ask.
 *
 * Asked rather than handed over, because the window that reads it may not have
 * existed when somebody pressed: it is built hidden and reveals itself once it
 * has painted, so anything shouted at it in between reaches nothing. Rust holds
 * the one value; the first frame reads it, and a window that was already open
 * is nudged to read it again.
 */
export async function askedSection(): Promise<string | null> {
  if (!inTauri()) return null;
  return command<string | null>("settings_section", {});
}

/**
 * Go where this window was sent, on the first frame and every time it is
 * re-opened.
 *
 * A callback rather than a value, because the same section asked for twice is
 * two errands and a value would be one unchanged string. Somebody who wandered
 * off and pressed the same control again is asking to be taken back.
 *
 * @param go What to do about it. Held in a ref, so a caller need not memoise
 *   one and the listener is registered once for the life of the window.
 */
export function useAskedSection(go: (section: string) => void): void {
  const latest = useRef(go);
  // In an effect rather than in the body: a ref written while rendering is a
  // render with an effect in it, and React says so out loud.
  useEffect(() => {
    latest.current = go;
  });

  useEffect(() => {
    if (!inTauri()) return;

    let listening = true;
    let stop: (() => void) | undefined;

    const collect = async () => {
      try {
        const asked = await askedSection();
        if (listening && asked !== null) latest.current(asked);
      } catch (error) {
        // A window that cannot ask stays on the screen it was showing, which is
        // the same answer as nobody having asked for one.
        console.error("Where this window was sent could not be read.", error);
      }
    };

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen("settings:section", () => void collect());
      // Unmounted while the listener was registering. Dropping it here is what
      // keeps a closed window from being told where to go.
      if (listening) stop = unlisten;
      else unlisten();
      // After the listener rather than before it, so an ask that lands in
      // between is not the one that is lost.
      await collect();
    })();

    return () => {
      listening = false;
      stop?.();
    };
  }, []);
}
