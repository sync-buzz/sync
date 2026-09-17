"use client";

import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * Where a scroller is, read often enough to draw with.
 *
 * The moving in this prototype is the platform's own: a scroll container with
 * snap points, rather than a transform that follows a pointer. That is the
 * whole reason it exists as a prototype — inertia, the rubber band at the end
 * of the run, the half-way rule that decides which snap point a flick lands
 * on, and a nested scroller taking a gesture that was never meant for its
 * parent are four behaviours nobody can write convincingly and everybody
 * recognises instantly when they are wrong. Borrowing them costs one listener.
 *
 * What cannot be borrowed is *drawing from* the position — the page beside the
 * one you are on has to dim and fall back as it leaves — so the position is
 * read here and handed to React. On a frame basis: the listener only marks the
 * scroller as dirty, and a single animation frame reads it, so a flick that
 * fires forty events in a second still costs one measurement per frame.
 */
export function useScrollAt(
  ref: RefObject<HTMLElement | null>,
  axis: "x" | "y",
): number {
  const [at, setAt] = useState(0);
  // The last value handed to React, kept out of state so that the frame below
  // can compare against it without being re-created every time it changes.
  const held = useRef(0);

  useEffect(() => {
    const box = ref.current;
    if (box === null) return;

    let frame = 0;
    const read = () => {
      frame = 0;
      const now = axis === "x" ? box.scrollLeft : box.scrollTop;
      // Sub-pixel changes are below what any of this draws and above what it
      // costs to render, which is the whole of the comparison.
      if (Math.abs(now - held.current) < 0.5) return;
      held.current = now;
      setAt(now);
    };

    const scrolled = () => {
      if (frame === 0) frame = requestAnimationFrame(read);
    };

    read();
    box.addEventListener("scroll", scrolled, { passive: true });
    return () => {
      box.removeEventListener("scroll", scrolled);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  }, [ref, axis]);

  return at;
}

/**
 * How far a scroller is from each of its ends, as something to draw with.
 *
 * The phone has no hairlines, so the edge where content meets furniture cannot
 * be drawn as a line — and left undrawn it is a cut: a row sliced in half by
 * the band under it reads as a rendering fault rather than as a list with more
 * in it. What says *there is more this way* here is the content going out: it
 * loses light towards the edge it is leaving by.
 *
 * Which means the fade has to know whether there is anything past the edge, or
 * the first line of an unscrolled list would sit half lit for ever. Hence a
 * pair of numbers rather than a constant: how much of the fade each end has
 * earned, nothing at an end that is already the end.
 *
 * **The scroller is not always this box.** In the window a column belongs to a
 * package and scrolls itself, so what is handed here is the box the column was
 * put in and the scroller is found inside it. `scroll` does not bubble, so it
 * is heard in the capture phase, which reaches a descendant's event without the
 * descendant knowing anybody is listening.
 */
export interface ScrollEdges {
  /** Top, or left: the end the content came from. 0 to 1. */
  readonly start: number;
  /** Bottom, or right: the end it is going to. 0 to 1. */
  readonly end: number;
}

/** Over how many points the fade reaches its full strength. */
const EARNED = 24;

export function useScrollFade(
  ref: RefObject<HTMLElement | null>,
  axis: "x" | "y",
): ScrollEdges {
  const [edges, setEdges] = useState<ScrollEdges>(NEITHER);

  useEffect(() => {
    const box = ref.current;
    if (box === null) return;

    let frame = 0;
    // The scroller last heard from, which is the one the fade is about. Held
    // rather than searched for on every frame: the search walks the column, and
    // a flick would run it sixty times a second for an answer that changes when
    // a person moves their finger somewhere else.
    let scroller: HTMLElement | null = scrollerIn(box);

    const read = () => {
      frame = 0;
      const element = scroller;
      if (element === null) return;
      const along =
        axis === "x" ? element.scrollLeft : element.scrollTop;
      const room =
        axis === "x"
          ? element.scrollWidth - element.clientWidth
          : element.scrollHeight - element.clientHeight;
      const next = {
        start: Math.min(1, Math.max(0, along) / EARNED),
        end: Math.min(1, Math.max(0, room - along) / EARNED),
      };
      setEdges((held) =>
        // Below what any of this draws, and above what it costs to render —
        // the same comparison `useScrollAt` makes, for the same reason.
        Math.abs(held.start - next.start) < 0.02 &&
        Math.abs(held.end - next.end) < 0.02
          ? held
          : next,
      );
    };

    const scrolled = (event: Event) => {
      const target = event.target;
      if (target instanceof HTMLElement) scroller = target;
      if (frame === 0) frame = requestAnimationFrame(read);
    };

    read();
    box.addEventListener("scroll", scrolled, { capture: true, passive: true });
    // A column that arrives with its rows already in it never scrolls, and
    // without this it would be drawn with no fade at the foot while running
    // straight under the band. What changes size is the scroller *or* the box,
    // so both are watched and the answer is read again either way.
    const watching = new ResizeObserver(() => {
      scroller ??= scrollerIn(box);
      if (frame === 0) frame = requestAnimationFrame(read);
    });
    watching.observe(box);
    return () => {
      box.removeEventListener("scroll", scrolled, { capture: true });
      watching.disconnect();
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  }, [ref, axis]);

  return edges;
}

/** Nothing at either end, which is what a box with nothing to scroll gets. */
const NEITHER: ScrollEdges = { start: 0, end: 0 };

/**
 * The box that actually scrolls, starting from the one that was handed over.
 *
 * Breadth-first and shallow: what is being looked for is the column's own
 * scroller, which is at or near the top of whatever a package rendered, and a
 * full walk of a long list would visit every row to find something two levels
 * down.
 */
function scrollerIn(box: HTMLElement): HTMLElement | null {
  const queue: HTMLElement[] = [box];
  for (let visited = 0; visited < 64 && queue.length > 0; visited += 1) {
    const element = queue.shift();
    if (element === undefined) break;
    const how = getComputedStyle(element);
    if (
      (how.overflowY === "auto" || how.overflowY === "scroll") &&
      element.scrollHeight > element.clientHeight
    )
      return element;
    if (
      (how.overflowX === "auto" || how.overflowX === "scroll") &&
      element.scrollWidth > element.clientWidth
    )
      return element;
    for (const child of element.children)
      if (child instanceof HTMLElement) queue.push(child);
  }
  return null;
}

/**
 * The fade itself, as a mask.
 *
 * A mask rather than a gradient of the field laid over the top, and the
 * difference matters where the content is not on the field: the band of
 * sections has a surface of its own, and a white-to-transparent veil over it
 * would fade the band rather than what is scrolling inside it. A mask takes
 * light from whatever is under it and leaves everything else alone.
 *
 * How long the fade is belongs to the appearance rather than to this function.
 * On black a line at half strength is still comfortably readable; on white the
 * same half is under the contrast a phone is held to, so the dark appearance
 * can afford a long fade and the light one cannot. Both are `--phone-fade`.
 */
export function edgeMask(axis: "x" | "y", edges: ScrollEdges): string {
  const towards = axis === "x" ? "to right" : "to bottom";
  // Nothing where the appearance has not said, which is every screen that is
  // not a phone: an unset custom property makes the whole declaration invalid,
  // and a mask that fails to parse is a mask that is silently not there.
  const start = `calc(var(--phone-fade, 0px) * ${edges.start.toFixed(2)})`;
  const end = `calc(var(--phone-fade, 0px) * ${edges.end.toFixed(2)})`;
  return `linear-gradient(${towards}, transparent 0, #000 ${start}, #000 calc(100% - ${end}), transparent 100%)`;
}
