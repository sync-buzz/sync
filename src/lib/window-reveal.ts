"use client";

import { useEffect, useSyncExternalStore } from "react";

/**
 * When the window appears, and whether it appears loading.
 *
 * `src-tauri/tauri.conf.json` creates the window hidden. A window that is
 * visible before its first frame shows the desktop through itself — the
 * material has nothing over it yet — which reads as a broken launch rather
 * than as a starting application, so nothing is revealed until there is
 * something to reveal.
 *
 * Which makes revealing it this hook's responsibility alone: if it fails to run
 * for any reason, the application has no window at all. That rules out waiting
 * for anything the hidden window itself has to produce.
 *
 * Whether the shell is still loading is not this hook's state to hold — it is
 * the document's, so it is read from the document rather than mirrored into
 * React. Subscribing to it means the answer is already right at the first
 * commit instead of being corrected by an effect a frame later:
 *
 * - **Fast launch** — the document is complete before the first paint, so the
 *   window opens directly onto the finished interface. No splash flashes for a
 *   frame, which would look worse than no splash at all.
 * - **Slow launch** — the document is still loading, so the window opens on the
 *   loading state and crosses over to the interface when `load` fires.
 *
 * The server snapshot is deliberately "still loading": the statically exported
 * HTML is what the window paints first, and the loading state has to be in it
 * for a slow launch to have anything to show.
 *
 * Nothing is delayed to make the loading state easier to see. It appears when
 * there is genuinely something to wait for and not otherwise, which is the only
 * version of it that tells the truth.
 */
export function useWindowReveal() {
  const isLoading = !useSyncExternalStore(
    subscribeToLoad,
    () => document.readyState === "complete",
    () => false,
  );

  useEffect(() => {
    // Revealed straight from the effect, once React has committed the DOM.
    //
    // Not from `requestAnimationFrame`: a hidden window paints no frames, so
    // its callbacks never run and the window would stay hidden forever. That is
    // a deadlock, not a delay — waiting for a frame is exactly what cannot
    // happen here.
    void revealWindow();
  }, []);

  return isLoading;
}

function subscribeToLoad(onChange: () => void) {
  window.addEventListener("load", onChange);
  return () => window.removeEventListener("load", onChange);
}

async function revealWindow() {
  if (!("__TAURI_INTERNALS__" in window)) return;

  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const appWindow = getCurrentWindow();
    await appWindow.show();
    await appWindow.setFocus();
  } catch (error) {
    // A window that stays hidden is not a cosmetic problem: there would be no
    // application on screen at all. Say so rather than failing silently.
    console.error("The window could not be revealed.", error);
  }
}
