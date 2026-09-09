"use client";

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

/**
 * Every command this window sends, and the count of the ones it is still owed
 * an answer to.
 *
 * The window asks Rust for everything it shows, and until now nothing knew how
 * many of those questions were outstanding: each client held its own `invoke`,
 * so *is anything happening* was a fact no component could reach. It is asked
 * from one place — the line under the header — and that line has to be true of
 * the whole window rather than of whichever column thought to report itself.
 *
 * **A section reports nothing.** An extension reads the corpus, opens a
 * document, asks the network — all of it through functions the shell exports,
 * all of which arrive here. A package that had to say *I am loading* would be
 * a package that can forget to, and the window would go quiet for exactly the
 * sections nobody wrote carefully.
 *
 * The count is per webview, which is what it should be: the settings window
 * asks its own questions and answers for itself.
 */

/**
 * How long a question must go unanswered before the window says so.
 *
 * Nearly every read is answered inside this, and a line that appeared for two
 * frames on each of them would be a flicker under the header — read as a defect
 * long before it is read as a report. What is left after the threshold is the
 * wait a person can feel, which is the only one worth drawing.
 */
const APPEARS_AFTER_MS = 250;

/**
 * How long it stays once it has appeared, however fast the answer came.
 *
 * Without this a read that lands at 260ms would flash the line for ten
 * milliseconds, which is worse than never having drawn it: something moved
 * under the header and nothing was legible.
 */
const STAYS_FOR_MS = 400;

let outstanding = 0;
const watchers = new Set<() => void>();

function announce() {
  for (const watcher of watchers) watcher();
}

function subscribe(watcher: () => void) {
  watchers.add(watcher);
  return () => {
    watchers.delete(watcher);
  };
}

const isWaiting = () => outstanding > 0;

/**
 * Send a command, and be counted while it is out.
 *
 * The one door to `invoke` in this window: a client that reached for Tauri
 * directly would be work nothing can see. The signature is `invoke`'s so the
 * change at each call site is the import and nothing else.
 *
 * @param quiet For the two kinds of work nobody is waiting on. One is work the
 *   window already reports in words — the exchange with memory's remote, which
 *   the header spells out; two reports of one wait is one of them contradicting
 *   the other the moment they disagree about when it ended. The other is what
 *   repeats on a timer, which no person asked for and which would otherwise
 *   pulse under the header for as long as the application is open.
 *
 *   It is not for work that is merely slow, and not for work a person did not
 *   start but will see the result of: a long wait nobody is told about is the
 *   failure this module exists to fix.
 */
export async function command<T>(
  name: string,
  args?: Record<string, unknown>,
  { quiet = false }: { quiet?: boolean } = {},
): Promise<T> {
  if (quiet) return invoke<T>(name, args);

  outstanding += 1;
  announce();
  try {
    return await invoke<T>(name, args);
  } finally {
    outstanding -= 1;
    announce();
  }
}

/**
 * Whether the window should say it is waiting.
 *
 * Not the count and not "is anything in flight": both of those are true far too
 * often to draw. This is the count put through the two thresholds above, so
 * what a component gets is a fact it can render without owning a timer of its
 * own — and every component that asks gets the same answer at the same moment.
 */
export function useWaiting(): boolean {
  const waiting = useSyncExternalStore(subscribe, isWaiting, () => false);
  const [shown, setShown] = useState(false);
  // When it appeared, so that the minimum below is measured from the moment
  // something was drawn rather than from the moment the last answer landed.
  const since = useRef(0);

  useEffect(() => {
    if (waiting) {
      // Already up: a second question starting while the first is out extends
      // the same wait rather than beginning a new one.
      if (shown) return;
      const timer = setTimeout(() => {
        since.current = performance.now();
        setShown(true);
      }, APPEARS_AFTER_MS);
      return () => clearTimeout(timer);
    }

    if (!shown) return;
    const left = STAYS_FOR_MS - (performance.now() - since.current);
    if (left <= 0) {
      setShown(false);
      return;
    }
    const timer = setTimeout(() => setShown(false), left);
    return () => clearTimeout(timer);
  }, [waiting, shown]);

  return shown;
}
