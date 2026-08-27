"use client";

/**
 * What the window is made of, as far as a person gets to decide it.
 *
 * Two answers, and they are deliberately the only two. **Appearance** is
 * whether the shell follows the system or is held light or dark — the system is
 * the default and stays the default, because a desktop application that ignores
 * the appearance the Mac is in is the one that looks wrong. **Base colour** is
 * the hue every grey in the token layer is mixed from, which is what the five
 * palettes shadcn/ui publishes actually differ by; nothing else about the
 * design changes with it.
 *
 * Both live in this window's own storage rather than in a file, for one
 * reason: they have to be applied before the first frame the window paints, and
 * a value that arrives over IPC arrives after it. Losing them costs the two
 * choices and nothing else — no project data is here.
 */

import { useEffect, useLayoutEffect, useSyncExternalStore } from "react";

export type Appearance = "system" | "light" | "dark";

export const APPEARANCES = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
] as const satisfies readonly { id: Appearance; label: string }[];

export type Tint = "zinc" | "neutral" | "gray" | "slate" | "stone";

export const TINTS = [
  { id: "zinc", label: "Zinc" },
  { id: "neutral", label: "Neutral" },
  { id: "gray", label: "Gray" },
  { id: "slate", label: "Slate" },
  { id: "stone", label: "Stone" },
] as const satisfies readonly { id: Tint; label: string }[];

export interface AppearanceSettings {
  readonly appearance: Appearance;
  readonly tint: Tint;
}

/** The shell as it was designed: the system's appearance, in zinc. */
export const DEFAULT_APPEARANCE: AppearanceSettings = {
  appearance: "system",
  tint: "zinc",
};

const STORAGE_KEY = "sync.appearance";

/** How the other window hears that the choice changed. */
const APPEARANCE_EVENT = "sync://appearance";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function stored(): AppearanceSettings {
  if (typeof window === "undefined") return DEFAULT_APPEARANCE;

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_APPEARANCE;

    const parsed = JSON.parse(raw) as Partial<AppearanceSettings>;
    return {
      appearance:
        APPEARANCES.find((entry) => entry.id === parsed.appearance)?.id ??
        DEFAULT_APPEARANCE.appearance,
      tint:
        TINTS.find((entry) => entry.id === parsed.tint)?.id ??
        DEFAULT_APPEARANCE.tint,
    };
  } catch {
    // A value this window cannot read is a value it has: the default. There is
    // nothing to report and nothing a person could do about it.
    return DEFAULT_APPEARANCE;
  }
}

/**
 * Put the choice on the document.
 *
 * `data-appearance` is absent for the system's own, because the token layer
 * expresses "follow the system" as the absence of a decision rather than as a
 * third value it would have to keep in step with two media queries.
 */
function apply(settings: AppearanceSettings) {
  const root = document.documentElement;

  if (settings.appearance === "system") root.removeAttribute("data-appearance");
  else root.setAttribute("data-appearance", settings.appearance);

  root.setAttribute("data-tint", settings.tint);
}

/**
 * The choice, as one value for the whole document.
 *
 * It is a store outside React rather than state inside it, for the same reason
 * the loading state in `window-reveal.ts` is: it exists before React does — it
 * is read out of storage and put on the document — and both windows read the
 * same one. Two components asking would otherwise be two answers.
 */
let current: AppearanceSettings | null = null;
const listeners = new Set<() => void>();

function snapshot(): AppearanceSettings {
  current ??= stored();
  return current;
}

function serverSnapshot(): AppearanceSettings {
  // One exported document, so the prerendered HTML has to be the default. The
  // window is hidden until it has painted, so the correction is never seen.
  return DEFAULT_APPEARANCE;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function commit(next: AppearanceSettings) {
  current = next;
  apply(next);
  for (const listener of listeners) listener();
}

/**
 * The current choice, applied to this window and shared with the other one.
 *
 * Applied in a layout effect so it lands before the window is revealed — the
 * reveal runs in an ordinary effect, which is later, so nothing is ever seen
 * changing colour.
 */
export function useAppearance(): {
  settings: AppearanceSettings;
  set: (next: Partial<AppearanceSettings>) => void;
} {
  const settings = useSyncExternalStore(subscribe, snapshot, serverSnapshot);

  useLayoutEffect(() => {
    apply(settings);
  }, [settings]);

  // The two windows are two webviews and do not share a storage event, so the
  // choice travels the way everything else between them would: as an event.
  useEffect(() => {
    if (!inTauri()) return;

    let cancelled = false;
    let stop: (() => void) | undefined;

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen<AppearanceSettings>(
        APPEARANCE_EVENT,
        (event) => commit(event.payload),
      );

      if (cancelled) unlisten();
      else stop = unlisten;
    })().catch((error: unknown) => {
      console.warn("Appearance changes made elsewhere will not arrive.", error);
    });

    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  return {
    settings,
    set: (next) => {
      const merged = { ...snapshot(), ...next };
      commit(merged);

      try {
        window.localStorage.setItem(STORAGE_KEY, JSON.stringify(merged));
      } catch {
        // The choice still applies to this window; it just will not survive it.
      }

      if (!inTauri()) return;
      void import("@tauri-apps/api/event")
        .then(({ emit }) => emit(APPEARANCE_EVENT, merged))
        .catch(() => undefined);
    },
  };
}
