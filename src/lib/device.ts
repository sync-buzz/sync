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
 * **What it decides is what the machine is, never what a caller wants.** It is
 * not how a call is routed — the phone's application implements the same
 * command names in Rust, and the window goes on calling them. What it does
 * decide is the shape of the window around those calls: the state only a phone
 * can be in, having no computer to ask, and the arrangement of a frame's
 * columns, which on a phone is depth rather than width.
 *
 * That second one is a device question and not a width question, deliberately.
 * A narrow window on a Mac is a window somebody chose to make narrow, with a
 * pointer, a keyboard and a title bar; a phone is a different set of gestures
 * and a different reach, and an arrangement derived from width alone would give
 * a pushed navigation stack to a Mac window dragged small. What *is* a width
 * question is everything inside a column — an area makes itself fit the space
 * it is given, on either machine, and reads none of this.
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
 * Which desktop draws this window, or `null` where none does.
 *
 * A second question from the one above and asked for a different reason. The
 * device decides the shape of the window; this decides the *words* — what the
 * system a person is looking at calls its own parts. Three desktops are shipped
 * and each names the same thing differently, so a window that said one of the
 * three everywhere would be wrong on two of them.
 *
 * Read from the user agent because the three webviews are three engines and say
 * so: WebView2 names Windows, WebKitGTK names Linux, and WKWebView names the
 * Mac. That is a plugin and a permission not asked for, to answer a question the
 * document is already holding the answer to.
 *
 * `null` for the phone, and `null` again for a user agent naming none of the
 * three. Not a default of one of them: what reads this leaves a command out
 * where the answer is `null`, and a wrong guess would instead put a command on
 * screen that does nothing when it is chosen.
 */
export type Desktop = "mac" | "windows" | "linux";

export function desktop(): Desktop | null {
  if (device() === "phone") return null;
  if (typeof navigator === "undefined") return null;
  const agent = navigator.userAgent;
  // Windows first: its user agent names no other system, while a Mac's names
  // "Mac OS X" and would be read as neither by a test looking for "Linux".
  if (/Windows/.test(agent)) return "windows";
  if (/Mac/.test(agent)) return "mac";
  if (/Linux|X11|BSD/.test(agent)) return "linux";
  return null;
}

/**
 * What this system calls showing a file where it lives, or `null` where nothing
 * here can.
 *
 * One answer in one place, for the same reason [`absenceLabel`] is: a menu, a
 * button and a message that name the file manager differently name three
 * things. The words are the system's own — *Finder* is a name and *File
 * Explorer* is another, and *File Manager* is what is said where a person may
 * be running any of a dozen.
 *
 * `null` on the phone, which has no file manager to show anything in and no
 * command behind this either — a menu item there would fail when it was chosen.
 * What offers this leaves the command out rather than showing it refused.
 */
export function revealLabel(): string | null {
  switch (desktop()) {
    case "mac":
      return "Show in Finder";
    case "windows":
      return "Show in File Explorer";
    // No one name to use. Every desktop here has its own file manager and a
    // person may be running any of them, so the words say what the thing is
    // rather than guess whose it is — which stays true of the fallback the
    // system takes when it cannot select the file and opens its folder instead.
    case "linux":
      return "Show in File Manager";
    default:
      return null;
  }
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
