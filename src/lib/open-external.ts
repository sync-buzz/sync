"use client";

/**
 * A link to the web, handed to the system.
 *
 * The one thing a desktop window must not do with `https://` is follow it
 * itself: a webview that navigated would replace the application with a page,
 * with no way back and the open record gone. So it is not navigation at all —
 * the address goes to whatever the person has set as their browser, and this
 * window keeps showing the record they were reading.
 *
 * Only the four schemes the capability grants ever get here, and the capability
 * grants `open-url` alone. `open-path`, which would launch a file on the disk,
 * and `reveal-item-in-dir`, which would open Finder, are both refused at the
 * boundary rather than by this function remembering not to call them — a
 * record's body is somebody else's text, and what it can ask of this machine
 * has to be decided where it cannot be argued with.
 */

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function openExternal(url: string): Promise<void> {
  // A browser during development opens its own tab. Not a fallback for the
  // packaged application — there is no packaged case where this is reached —
  // but the difference between a link that works while developing and one that
  // silently does nothing.
  if (!inTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }

  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
