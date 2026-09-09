"use client";

import { useCallback, useEffect, useRef, type RefCallback } from "react";

/**
 * The end of a list coming into view, which is how a list asks for the rest of
 * itself.
 *
 * A list longer than one read of the store is read further by somebody reading
 * it: they arrive at the bottom, and the next page is asked for because they
 * got there. There is no control to find and no page to choose — the gesture
 * *is* the request, which is what "it is a list, not a page of a website"
 * means in the one place a person can feel the difference.
 *
 * # Why this is the window's rather than each package's
 *
 * The same reason the virtual list is. What is difficult here is not the
 * observer but everything around it: an element that comes and goes as the
 * list is drawn, a callback that changes identity on every render and would
 * otherwise tear the observer down and build it again each time, and a list
 * whose page is shorter than the panel — where the end is on screen from the
 * first frame and the naive version asks once, forever. Each of those is a
 * loop or a leak in somebody's section, found late, and none of them is about
 * records or messages or anything a package is for.
 *
 * So a package is given a marker to put at the end of its rows, and learns no
 * pixel of where the scroller is or how far down it has gone. Whether there is
 * anything left to ask for is the package's to say — it is the one that holds
 * the answer from the store — and it says it by passing `null`.
 *
 * # What it deliberately does not do
 *
 * It reaches ahead by nothing. There is no margin below the marker and no
 * fraction of a screen to fetch early: reading ahead is a guess about where
 * somebody is going, paid for out of the same engine the window in front of
 * them is being served by. The end of the list is the end of the list.
 *
 * @param onReach What to do when the marker is on screen, and `null` whenever
 *   there is nothing to ask for *right now* — the list is whole, or a page of
 *   it is already on its way. Both halves of that matter. An observer watches
 *   for a crossing rather than for a state, so a marker that was on screen and
 *   stays on screen is reported once; it is `null` and back that hands the
 *   marker over again and asks afresh, which is what carries a list whose page
 *   is shorter than the panel down to its end instead of stopping at two
 *   pages.
 * @returns A ref for an element placed after the last row.
 */
export function useListEnd(
  onReach: (() => void) | null,
): RefCallback<HTMLElement | null> {
  // The callback as it stands right now, kept where the observer can read it
  // without being rebuilt for it. A list passes a fresh closure on every
  // render, and an observer that depended on its identity would disconnect and
  // reconnect between frames — long enough to miss the intersection that was
  // the whole point.
  const reach = useRef(onReach);
  useEffect(() => {
    reach.current = onReach;
  }, [onReach]);

  // Whether there is anything to ask for at all, which is the one thing about
  // the callback that has to reach the observer rather than sit behind it: a
  // finished list keeps its marker on screen, and an observer left watching it
  // would be a callback fired at every scroll for nothing.
  const wanted = onReach !== null;
  const watched = useRef<HTMLElement | null>(null);
  const observer = useRef<IntersectionObserver | null>(null);

  const watch = useCallback(() => {
    observer.current?.disconnect();
    observer.current = null;
    const marker = watched.current;
    if (!wanted || marker === null) return;

    const seen = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) reach.current?.();
    });
    seen.observe(marker);
    observer.current = seen;
  }, [wanted]);

  useEffect(() => {
    watch();
    return () => {
      observer.current?.disconnect();
      observer.current = null;
    };
  }, [watch]);

  return useCallback(
    (marker: HTMLElement | null) => {
      watched.current = marker;
      watch();
    },
    [watch],
  );
}
