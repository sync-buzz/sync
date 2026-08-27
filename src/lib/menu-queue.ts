"use client";

/**
 * One menu at a time, because the system's menus are not re-entrant.
 *
 * Menus live in the Rust process, and every call that builds, installs or
 * frees one goes through Tauri's menu plugin. Two of those calls are ordinary
 * synchronous commands — `menu|new` and `resources|close` — which Tauri runs on
 * the main thread, and both take the window's resource-table lock. Showing a
 * context menu takes that same lock and then blocks the main thread inside
 * AppKit's own event loop for as long as the menu is on screen.
 *
 * So a menu built while another is showing is a deadlock, not a race: the main
 * thread waits for a lock the menu holds, the menu waits for a main thread that
 * will never come back to dismiss it, and the window is gone for good. It was
 * reachable by pointing at a row and pressing the secondary button — opening a
 * menu selects the row, selecting the row changes what File can do, and the
 * menu bar rebuilt itself into the lock.
 *
 * Hence one queue for every menu this window owns. Work waits its turn; work
 * that leaves a menu on screen keeps the turn until a person dismisses it. The
 * menu bar therefore catches up after a context menu closes rather than during
 * — which is what a menu bar does anyway, since nobody can reach it while a
 * context menu is up.
 */

/** Work already promised to the menu API, in the order it was asked for. */
let queued: Promise<unknown> = Promise.resolve();

/** Whether a menu of ours is on screen, or on its way there. */
let showing = false;

/**
 * Whether a native menu can be built at all.
 *
 * Outside Tauri — `pnpm dev` in a browser — there is none, and a caller that
 * has nothing else to offer leaves the browser's own menu alone rather than
 * suppressing it to then show nothing.
 */
export function nativeMenusAvailable(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Whether the system is showing a menu of ours, or is about to. */
export function menuIsShowing(): boolean {
  return showing;
}

/**
 * Run `work` once every menu asked for before it is done with.
 *
 * A failure does not break the chain: a menu that could not be built is one
 * menu missing, not a window without menus for the rest of the session.
 */
export function queueMenuWork<T>(work: () => Promise<T>): Promise<T> {
  const run = queued.then(work, work);
  queued = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

/**
 * Run `work` that leaves a menu on screen, holding the queue until it is gone.
 *
 * The flag is raised now rather than when the turn comes: the gesture that
 * would ask for a second menu is the one dismissing the first, and it has to be
 * turned away while this one is still queued as much as while it is drawn.
 */
export function queueMenuOnScreen(work: () => Promise<void>): void {
  showing = true;
  void queueMenuWork(work).finally(() => {
    showing = false;
  });
}
