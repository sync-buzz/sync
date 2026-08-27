"use client";

/**
 * The menu the system draws, for the gesture the system owns.
 *
 * A context menu is not a control the shell designs: it is what the pointer's
 * secondary button does everywhere else on the Mac, and a web menu in its place
 * is the one part of the window that would announce it is a webview — wrong
 * font, wrong metrics, wrong dismissal, no keyboard behaviour the rest of the
 * system has. Tauri carries the native menu itself, so nothing is added to the
 * dependency list and nothing is granted beyond `core:default`, which already
 * covers `core:menu`.
 *
 * Outside Tauri — `pnpm dev` in a browser — there is no native menu to show, so
 * [`nativeMenusAvailable`] answers no and the caller leaves the event alone.
 * Suppressing the browser's own menu to then show nothing would be worse than
 * either menu.
 *
 * Everything reachable from here has to be reachable another way. A menu that
 * opens under the pointer is invisible to the keyboard, so it carries actions
 * rather than owning them — see the actions control in the navigator's bottom
 * bar, which offers the same two.
 *
 * Nothing here talks to the menu API directly: it goes through the queue in
 * [`@/lib/menu-queue`], which the menu bar shares, because a second menu built
 * while this one is on screen deadlocks the window rather than replacing it.
 */

import {
  menuIsShowing,
  nativeMenusAvailable,
  queueMenuOnScreen,
} from "@/lib/menu-queue";

/** One command. Without `onSelect` it is a line that says something and does
 *  nothing, which is how a menu explains why an action is unavailable. */
export interface NativeMenuItem {
  label: string;
  enabled?: boolean;
  onSelect?: () => void;
}

/**
 * One of the system's own editing commands, by name.
 *
 * Cut, Copy and Paste are not commands Sync implements. They are the
 * webview's, routed by the system once a menu claims them — which is why the
 * menu bar carries them too — and reimplementing them here would be ours
 * wearing the system's labels, with the system's clipboard behaviour missing.
 */
export interface NativeEditingCommand {
  predefined: "Cut" | "Copy" | "Paste" | "SelectAll" | "Undo" | "Redo";
}

/** A command, or the rule that sets a destructive one apart from the rest. */
export type NativeMenuEntry =
  | NativeMenuItem
  | NativeEditingCommand
  | "separator";

/**
 * Show a menu at the pointer and wait for the person to be done with it.
 *
 * `popup` resolves when the system lets the menu go, so the handle is a live
 * local for as long as the menu is drawn — menus live in the Rust process, and
 * one collected mid-gesture takes the menu with it. It is freed on the way out
 * rather than left for the next gesture to free: by then the system has no
 * claim on it, and the item chosen has already been dispatched by its id.
 *
 * The chosen item's `onSelect` runs on its own, whenever the person chooses it.
 */
async function showNativeMenu(
  entries: readonly NativeMenuEntry[],
): Promise<void> {
  const { Menu } = await import("@tauri-apps/api/menu");

  const menu = await Menu.new({
    items: entries.map((entry) => {
      if (entry === "separator") return { item: "Separator" as const };
      if ("predefined" in entry) return { item: entry.predefined };
      return {
        text: entry.label,
        enabled: entry.enabled ?? entry.onSelect !== undefined,
        action: entry.onSelect,
      };
    }),
  });

  try {
    await menu.popup();
  } finally {
    await menu.close().catch(() => undefined);
  }
}

/**
 * Show a menu in answer to a `contextmenu` event, if there is a native one to
 * show. Answers whether the event was taken, so a caller that has nothing else
 * to offer can leave the system's own menu alone.
 */
export function showNativeContextMenu(
  event: { preventDefault: () => void },
  entries: readonly NativeMenuEntry[],
): boolean {
  if (!nativeMenusAvailable()) return false;
  // Before the first await: an event is only cancellable while it is being
  // dispatched, and the menu is opened a round trip later.
  event.preventDefault();
  // A menu of ours is already up, so this gesture is the one dismissing it —
  // the system will do that on its own. Answering it with a second menu would
  // build one from a thread that cannot have the lock the first one holds, and
  // the window would never come back. The event is still taken: the gesture is
  // answered by the menu on screen.
  if (menuIsShowing()) return true;
  queueMenuOnScreen(() =>
    showNativeMenu(entries).catch((error: unknown) => {
      // Nothing to fall back to — the system menu was already refused — so this
      // is said out loud rather than left as a gesture that did nothing.
      console.warn("A native menu could not be shown.", error);
    }),
  );
  return true;
}
