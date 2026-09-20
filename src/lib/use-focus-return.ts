/**
 * One focus listener for the whole window, shared by every hook that
 * re-reads on return.
 *
 * Without this, each hook adds its own `window.addEventListener("focus",
 * …)`, and four of them fire four independent invoke chains on the same
 * event — a scan, a journal, a sync-state and a badge recount, all started
 * at once because somebody alt-tabbed back. The shared listener debounces
 * them into one round.
 *
 * The Tauri window listener is here because the DOM's `focus` does not fire
 * on `window` when the caret is already in a field: the event goes to the
 * element and does not bubble. The OS's own "this application is in front"
 * is what the four hooks meant to hear, and only one of them was listening
 * for it. Now they all do, through one listener.
 */

import { useEffect, useRef } from "react";

const DEBOUNCE_MS = 100;

const callbacks = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | null = null;
let installed = false;

function fire(): void {
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    for (const cb of callbacks) cb();
  }, DEBOUNCE_MS);
}

function install(): void {
  if (installed) return;
  installed = true;

  window.addEventListener("focus", fire);

  void (async () => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        if (focused) fire();
      });
    } catch {
      // Outside Tauri there is no window to follow.
    }
  })();
}

export function useFocusReturn(callback: () => void): void {
  const ref = useRef(callback);

  useEffect(() => {
    ref.current = callback;
  });

  useEffect(() => {
    install();
    const cb = () => ref.current();
    callbacks.add(cb);
    return () => {
      callbacks.delete(cb);
    };
  }, []);
}
