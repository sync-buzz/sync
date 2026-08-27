"use client";

/**
 * Opening another window of Sync.
 *
 * A window holds one project, so a second project means a second window rather
 * than something inside the first: the areas, the selection and the columns all
 * belong to the window, and two projects in one of them would be two of
 * everything fighting over one set.
 *
 * The window is built in Rust — see `src-tauri/src/windows.rs` — because it is
 * built from the configured one, and the configuration is not something the
 * webview can read. This is the ask, from the two places a person makes it: the
 * File menu here, and the Dock icon's menu, which never reaches the frontend at
 * all.
 */

import { invoke } from "@tauri-apps/api/core";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Open a window on the welcome screen, where a project is chosen. */
export async function openNewWindow(): Promise<void> {
  if (!inTauri()) return;

  try {
    await invoke("window_new", {});
  } catch (error) {
    // Nothing to fall back to — a window is the one thing this cannot do some
    // smaller version of — so the failure is said out loud rather than
    // swallowed into a command that appears to do nothing.
    console.error("A new window could not be opened.", error);
  }
}
