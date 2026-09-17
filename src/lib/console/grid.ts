"use client";

import { useCallback, useRef } from "react";

/**
 * The size of one cell, measured from the face that actually resolved.
 *
 * `--console-cell-w` and `--console-cell-h` are declared in the token layer as
 * a starting guess, and this is what replaces the guess with the truth. The
 * monospace stack ends in whatever the machine has — SF Mono on most Macs,
 * Menlo where it does not, something else again where a person installed one —
 * and their advance widths differ by a fraction of a pixel. A fraction is
 * enough: a table eighty columns wide is eighty of them, and a grid that was
 * assumed rather than measured is visibly not a grid by the right-hand edge.
 *
 * Measured against the element that carries the console's own font, so what is
 * measured is what will be drawn rather than what this file believes about it.
 */

/**
 * How many characters the probe is. Wide enough that dividing by it puts any
 * subpixel error well below the width of one cell, short enough not to be a
 * layout of its own.
 */
const SAMPLE = 100;

/**
 * The leading of the grid, as a multiple of the type size.
 *
 * Tighter than prose is set and looser than a terminal usually is. A console
 * is read in lines that are scanned rather than in sentences that are followed,
 * so the line has to be findable without the column turning into a block of
 * texture — and a control standing in a cell needs somewhere to stand.
 */
const LEADING = 1.45;

export function useCellMetrics(): (element: HTMLElement | null) => void {
  const measured = useRef<HTMLElement | null>(null);

  return useCallback((element: HTMLElement | null) => {
    measured.current = element;
    if (element === null) return;

    const measure = () => {
      // The element may have gone between being asked and being measured —
      // opening the shade and closing it before the fonts settle is enough.
      const host = measured.current;
      if (host === null || !host.isConnected) return;

      const probe = document.createElement("span");
      probe.textContent = "0".repeat(SAMPLE);
      // Out of the flow and out of the accessibility tree both: it is a ruler,
      // and a screen reader reading a hundred zeroes would be this measurement
      // becoming audible.
      probe.setAttribute("aria-hidden", "true");
      // Carrying the console's own face rather than inheriting the host's. The
      // shade is set in the interface face like everything else in the window,
      // so a probe that inherited would measure a proportional font and hand
      // back a cell width no character in the grid actually has.
      probe.className = "font-mono";
      probe.style.cssText = `position:absolute;top:0;left:0;visibility:hidden;white-space:pre;pointer-events:none;font-size:var(--type-sm);line-height:${LEADING};letter-spacing:normal`;
      host.append(probe);

      const rect = probe.getBoundingClientRect();
      probe.remove();

      if (rect.width === 0 || rect.height === 0) return;

      // Kept fractional. Rounding to whole pixels here is the same error as
      // guessing, only later: it would accumulate across the width of a line.
      host.style.setProperty("--console-cell-w", `${rect.width / SAMPLE}px`);
      host.style.setProperty("--console-cell-h", `${rect.height}px`);
    };

    measure();

    // Again once the system has settled its faces. A first measurement taken
    // against a fallback face is a grid built on a font nobody will see, and
    // on a cold start that is the ordinary case rather than the rare one.
    void document.fonts?.ready.then(measure);
  }, []);
}
