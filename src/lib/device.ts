"use client";

/**
 * Which machine this window is running on.
 *
 * One document is shown by two applications — the Mac's window and the phone's
 * — and nothing in the document can tell them apart by looking: same commands,
 * same Tauri, same export. So the phone says which it is, in a global its own
 * application sets before the document is parsed, and the Mac says nothing at
 * all. Absence is therefore the computer's answer, and a browser during
 * development gets the same one, which is what makes the desktop unchanged by
 * the phone existing.
 *
 * Absence is a safe signal here for a reason that does not hold in general: the
 * script is part of creating the webview rather than something asked for later,
 * so a script that did not run is a window with nothing in it. A missing
 * *command*, by contrast, would be a race dressed as a fact.
 *
 * **This answers one question and must not grow a second.** It is not how a
 * call is routed — the phone's application implements the same command names in
 * Rust, and the window goes on calling them. It is not how the layout is
 * chosen either: the mobile geometry is a question of width, which is why it
 * can be looked at in a browser at all. What it decides is the one state that
 * only a phone can be in — having no computer to ask.
 */

import { useSyncExternalStore } from "react";

export type Device = "computer" | "phone";

declare global {
  interface Window {
    /** Set by the phone's application, and by nothing else. */
    __SYNC_DEVICE__?: "phone";
  }
}

/**
 * Read straight from the global, synchronously.
 *
 * Exported beside the hook for the same reason `windowRole` is: something that
 * has to decide before React has rendered cannot wait for a commit.
 */
export function device(): Device {
  return typeof window !== "undefined" && window.__SYNC_DEVICE__ === "phone"
    ? "phone"
    : "computer";
}

/**
 * The same answer, read the way the window's role and its loading state are.
 *
 * The exported HTML is one file and it is rendered ahead of any device, so the
 * server snapshot is the computer; the phone corrects it on its first commit,
 * while the window is still showing that it is starting. Reading it through
 * `useState` instead would be the same answer arriving as a hydration mismatch.
 */
export function useDevice(): Device {
  return useSyncExternalStore(subscribeNever, device, onServer);
}

function subscribeNever() {
  // A window does not move from one machine to another.
  return () => undefined;
}

function onServer(): Device {
  return "computer";
}
