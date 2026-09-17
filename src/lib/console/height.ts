"use client";

import { useCallback, useState } from "react";

/**
 * How far down the shade comes, and the one thing about the console that
 * outlives the window.
 *
 * It belongs to the hand rather than to the project: somebody settles on a
 * console they can read without losing sight of what is under it, and they
 * settle on it once. A window that forgot it between launches would be a window
 * that asked the same question every morning.
 *
 * Kept as a fraction of the slab rather than as a number of pixels, so that the
 * shade is the same *share* of a window resized between sessions instead of
 * covering a laptop screen because it was dragged on a large display.
 *
 * In this window's own storage rather than in a file, for the reason
 * `src/lib/shell-layout.ts` gives about the columns: the shade has to be the
 * right height in the frame it first appears in, and a value fetched over IPC
 * arrives after that frame. Losing it costs one number.
 */

const STORAGE_KEY = "sync.console.height";

/** Where the shade sits before anybody has dragged it. */
export const DEFAULT_HEIGHT = 0.42;

/**
 * As far as it goes. Not the whole slab: the shade covers the work rather than
 * replacing it, and a strip of what is underneath is what says so.
 */
export const MAX_HEIGHT = 0.88;

/**
 * The floor, in cells: the tab strip, the input line and six lines of canvas.
 * Fewer than six and the canvas stops being a place a result is read and
 * becomes a slot a result scrolls through.
 */
export const MIN_CANVAS_LINES = 6;

/**
 * The two fixed bands at the foot of the shade, in pixels, and the room the
 * input line needs at one line.
 *
 * Here rather than beside what draws each of them, because the floor above is
 * computed from all three and the strip has to leave the edge its six pixels.
 * Two files agreeing on a number by copying it is two files that stop agreeing.
 */
export const STRIP_HEIGHT = 30;
export const EDGE_HEIGHT = 6;
export const INPUT_HEIGHT = 28;

function read(): number {
  if (typeof window === "undefined") return DEFAULT_HEIGHT;

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_HEIGHT;
    const parsed = Number.parseFloat(raw);
    // A stored value has no right to put the shade somewhere it could not be
    // dragged to. Anything outside the range is read as nothing stored.
    if (!Number.isFinite(parsed) || parsed <= 0 || parsed > MAX_HEIGHT) {
      return DEFAULT_HEIGHT;
    }
    return parsed;
  } catch {
    return DEFAULT_HEIGHT;
  }
}

export function useShadeHeight(): {
  /** The share of the space under the title bar that the shade takes. */
  readonly fraction: number;
  /** Move it. Called for every frame of a drag, so it touches nothing but state. */
  readonly set: (fraction: number) => void;
  /**
   * Remember where it was left.
   *
   * Separate from moving it, and that separation is the whole reason there are
   * two members: writing to storage is synchronous, and doing it on every
   * pointer move would put a write between the finger and the frame it is
   * dragging. What is worth remembering is where the gesture *ended*.
   */
  readonly keep: () => void;
} {
  const [fraction, setFraction] = useState(read);

  const keep = useCallback(() => {
    try {
      window.localStorage.setItem(STORAGE_KEY, String(fraction));
    } catch {
      // Storage this window may not write to costs it one number the next time
      // it opens and nothing while it is open. There is nothing to report and
      // nothing anybody could do about it.
    }
  }, [fraction]);

  return { fraction, set: setFraction, keep };
}
