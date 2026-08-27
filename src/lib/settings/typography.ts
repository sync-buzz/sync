"use client";

/**
 * How a record's text is set, as far as a person gets to decide it.
 *
 * Four decisions, and they are deliberately the ones a reader actually has: how
 * big the text is, what it is set in, how wide the column runs, and how far
 * apart the lines sit. Everything else about the typography is the design's and
 * is not up for negotiation — a person choosing a heading's weight is a person
 * being handed a job nobody asked for.
 *
 * **Concrete numbers, in the units they are measured in.** Not "small, medium,
 * large": somebody who knows they read at seventeen pixels should be able to
 * say seventeen. The size is the *base* of the type scale rather than a patch
 * over it — every other size in a record is a multiple of it, expressed in `em`
 * — so headings, quotes, code and tables all move together and the proportions
 * the design chose survive being resized.
 *
 * The measure is in pixels for the same reason, and it is a real trade rather
 * than an oversight: a column fixed in pixels holds fewer characters as the
 * text grows, which is what somebody who just made the text bigger is asking
 * for. A column in `ch` would have kept the line length and grown the window.
 *
 * The line and the gap between blocks are one decision, not two — that is why
 * there is no separate control for the gap. It is half a line, whatever a line
 * is, which is what keeps a paragraph break reading as a paragraph break rather
 * than as a section.
 *
 * Stored beside the appearance and for the same reason: both have to be applied
 * before the first frame the window paints, and a value arriving over IPC
 * arrives after it. Losing them costs four choices and nothing else.
 */

import { useEffect, useLayoutEffect, useSyncExternalStore } from "react";

/**
 * The families on offer, and why it is a list rather than every font installed.
 *
 * A free choice over the system's fonts is a choice that includes fonts with no
 * Cyrillic, no italic and no monospaced companion, and a record set in one of
 * those is a record somebody has to go and repair. These four are stacks the
 * platform guarantees, and each is a different answer to "what is this document
 * like" rather than a different taste in letterforms.
 */
export type ProseFamily = "system" | "sans" | "serif" | "mono";

export const PROSE_FAMILIES = [
  { id: "system", label: "System", stack: "var(--font-sans)" },
  { id: "sans", label: "Grotesque", stack: "Helvetica Neue, Arial, sans-serif" },
  { id: "serif", label: "Serif", stack: "New York, Georgia, Times New Roman, serif" },
  { id: "mono", label: "Monospaced", stack: "var(--font-mono)" },
] as const satisfies readonly {
  id: ProseFamily;
  label: string;
  stack: string;
}[];

export interface TypographySettings {
  /** The base of the type scale, in pixels. */
  readonly size: number;
  readonly family: ProseFamily;
  /** How wide the column of text runs, in pixels. */
  readonly measure: number;
  /** The line, as a multiple of the size. The gap between blocks is half of it. */
  readonly leading: number;
}

/**
 * The text as it was designed: sixteen pixels of the system's own face, in a
 * column of six hundred and eighty, set at one and a half.
 *
 * The measure is what `76ch` came to at this size and face, so a project opened
 * for the first time after this shipped looks like it did before it.
 */
export const DEFAULT_TYPOGRAPHY: TypographySettings = {
  size: 16,
  family: "system",
  measure: 680,
  leading: 1.5,
};

/** What a control will accept, so a stored value cannot make a record unreadable. */
export const LIMITS = {
  size: { min: 11, max: 32 },
  measure: { min: 360, max: 1200 },
  leading: { min: 1.2, max: 2 },
} as const;

const STORAGE_KEY = "sync.typography";

/** How the other window hears that the choice changed. */
const TYPOGRAPHY_EVENT = "sync://typography";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function clamp(value: unknown, { min, max }: { min: number; max: number }, fallback: number) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}

function stored(): TypographySettings {
  if (typeof window === "undefined") return DEFAULT_TYPOGRAPHY;

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_TYPOGRAPHY;

    const parsed = JSON.parse(raw) as Partial<TypographySettings>;
    return {
      size: clamp(parsed.size, LIMITS.size, DEFAULT_TYPOGRAPHY.size),
      family:
        PROSE_FAMILIES.find((entry) => entry.id === parsed.family)?.id ??
        DEFAULT_TYPOGRAPHY.family,
      measure: clamp(parsed.measure, LIMITS.measure, DEFAULT_TYPOGRAPHY.measure),
      leading: clamp(parsed.leading, LIMITS.leading, DEFAULT_TYPOGRAPHY.leading),
    };
  } catch {
    // A value this window cannot read is a value it has: the default.
    return DEFAULT_TYPOGRAPHY;
  }
}

/**
 * Put the choice on the document, as the four variables every prose surface
 * reads.
 *
 * Variables rather than classes because there are two surfaces — a record being
 * edited and a record being read — and they must not be able to disagree. One
 * of them holding its own copy of the size is how a record changes shape the
 * moment it becomes editable, which is the thing `nodes.tsx` exists to prevent.
 */
function apply(settings: TypographySettings) {
  const root = document.documentElement;
  const family =
    PROSE_FAMILIES.find((entry) => entry.id === settings.family) ??
    PROSE_FAMILIES[0];

  root.style.setProperty("--prose-size", `${settings.size}px`);
  root.style.setProperty("--prose-family", family.stack);
  root.style.setProperty("--prose-measure", `${settings.measure}px`);
  root.style.setProperty("--prose-leading", `${settings.leading}`);
}

let current: TypographySettings | null = null;
const listeners = new Set<() => void>();

function snapshot(): TypographySettings {
  current ??= stored();
  return current;
}

function serverSnapshot(): TypographySettings {
  // One exported document, so the prerendered HTML has to be the default. The
  // window is hidden until it has painted, so the correction is never seen.
  return DEFAULT_TYPOGRAPHY;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function commit(next: TypographySettings) {
  current = next;
  apply(next);
  for (const listener of listeners) listener();
}

export function useTypography(): {
  settings: TypographySettings;
  set: (next: Partial<TypographySettings>) => void;
  reset: () => void;
} {
  const settings = useSyncExternalStore(subscribe, snapshot, serverSnapshot);

  useLayoutEffect(() => {
    apply(settings);
  }, [settings]);

  // The two windows are two webviews and do not share a storage event, so the
  // choice travels the way the appearance does: as an event.
  useEffect(() => {
    if (!inTauri()) return;

    let cancelled = false;
    let stop: (() => void) | undefined;

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen<TypographySettings>(
        TYPOGRAPHY_EVENT,
        (event) => commit(event.payload),
      );

      if (cancelled) unlisten();
      else stop = unlisten;
    })().catch((error: unknown) => {
      console.warn("Text changes made elsewhere will not arrive.", error);
    });

    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const write = (merged: TypographySettings) => {
    commit(merged);

    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(merged));
    } catch {
      // The choice still applies to this window; it just will not survive it.
    }

    if (!inTauri()) return;
    void import("@tauri-apps/api/event")
      .then(({ emit }) => emit(TYPOGRAPHY_EVENT, merged))
      .catch(() => undefined);
  };

  return {
    settings,
    set: (next) => write({ ...snapshot(), ...next }),
    // Worth a control of its own: four numbers are four ways to end up with a
    // page you cannot read and no memory of what it was.
    reset: () => write(DEFAULT_TYPOGRAPHY),
  };
}
