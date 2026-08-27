"use client";

/**
 * What a window is called, which is what its project is called.
 *
 * The title bar does not draw it — the window's is hidden, and the header says
 * the project's name in its own type — but the system does, everywhere it lists
 * windows: the Dock icon's menu, `Window` in the menu bar, Mission Control and
 * the app switcher's window list. With one window that never mattered; with
 * several, a list of identical `Sync`s is a list nobody can pick from.
 *
 * A window with no project open is called `Sync`, because that is what it is:
 * the application, waiting to be given one.
 *
 * The name is set through a command rather than through `setTitle`, because on
 * macOS renaming a window moves its traffic lights back to where the system
 * would have put them — see `window_named` in `src-tauri/src/windows.rs`, which
 * does both halves in one trip to the main thread.
 */

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { windowRole } from "@/lib/settings/window";

/** What a window without a project is called. */
const APPLICATION = "Sync";

/** Name the window after the project it has open. */
export function useWindowTitle(project: string | null): void {
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }

    // A name belongs to the window that has a project, which is not every
    // window. The check is here rather than left to which component mounted,
    // for the reason the material's is: the document hydrates as the main
    // window before the label corrects it, so the settings window commits the
    // shell once — and that one commit is long enough to name it `Sync` and,
    // because naming a window on macOS re-insets its traffic lights, to move
    // its buttons to where the project window's overlaid title bar wants them.
    // On the system title bar the settings window actually wears, that inset
    // leaves them below the bar, sitting over its content.
    if (windowRole() !== "main") return;

    // Not guarded against a later name arriving first: the command names the
    // window itself, and two of them cross the boundary in the order they were
    // called, so the last project chosen is the last name applied.
    void (async () => {
      try {
        await invoke("window_named", { title: project?.trim() || APPLICATION });
      } catch (error) {
        // The window works unnamed; it is only harder to find. Reported rather
        // than escalated, and never allowed to take a render down with it.
        console.error("The window could not be named.", error);
      }
    })();
  }, [project]);
}
