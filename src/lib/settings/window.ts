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

import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

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

/** Open the settings window, or bring the open one forward. */
export async function openSettings(): Promise<void> {
  if (!inTauri()) return;

  try {
    // No project travels with the request. Everything in settings is this
    // Mac's: one server answers for every project, so connecting an agent no
    // longer names one.
    await invoke("settings_open", {});
  } catch (error) {
    // Nothing to fall back to: there is no in-window settings surface to show
    // instead, so the failure is said out loud rather than swallowed.
    console.error("The settings window could not be opened.", error);
  }
}
