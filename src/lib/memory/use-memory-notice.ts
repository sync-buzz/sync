"use client";

import { useEffect, useRef } from "react";
import { Channel } from "@tauri-apps/api/core";

import { unwatchMemory, watchMemory, type MemoryMoved } from "@/lib/memory/client";

/**
 * Run something whenever this project's memory stops being what it was.
 *
 * Everything that draws the corpus is right only as of when it read, and until
 * this existed the one moment any of them re-read was somebody returning to the
 * window — so a record an agent wrote a minute ago was invisible to a person
 * sitting in front of it. This is the other moment, and it is an event rather
 * than a schedule: the notice starts at whichever door wrote and is carried to
 * the window on a channel the application has held open since it started.
 *
 * **There is no timer here, and adding one is the thing this replaces.** Asking
 * again every few seconds from the webview is the same clock one storey up: it
 * costs a read of the journal per project per tick whether or not anybody wrote
 * anything, and it is wrong for exactly as long as its interval.
 *
 * A reader is expected to *re-read*, not to trust what it was handed. The
 * notice carries no revision, because a number written into it was already old
 * when it was written — the memory can move twice while one line is in flight.
 *
 * Several readers of one project each call this, and each is its own
 * subscription: a screen unmounting does not silence the indicator beside it.
 */
export function useMemoryNotice(
  projectPath: string,
  /**
   * What to do about it. Held in a ref rather than watched, so a caller that
   * rebuilds this function every render does not resubscribe every render —
   * which would be a command pair per frame, and a window whose subscription
   * is briefly nobody's.
   */
  reread: () => void,
): void {
  const act = useRef(reread);
  // Declared before the subscription below, so that the newest function is in
  // place before a notice can reach it. On the first render the ref already
  // holds it and this changes nothing.
  useEffect(() => {
    act.current = reread;
  });

  useEffect(() => {
    if (!projectPath) return;
    let watching = true;
    let held: number | null = null;
    /**
     * A burst of writes is one thing to look at.
     *
     * An agent working through a folder writes many times in a few seconds, and
     * a re-read for each of them is that many journal reads down one connection
     * to show a list that settles once. This is not the timer above: it never
     * starts unless the engine has spoken, it fires once, and what it delays is
     * a redraw nobody has looked at yet.
     */
    let soon: ReturnType<typeof setTimeout> | null = null;

    const notice = new Channel<MemoryMoved>();
    notice.onmessage = () => {
      if (!watching || soon !== null) return;
      soon = setTimeout(() => {
        soon = null;
        if (watching) act.current();
      }, TOGETHER_WITHIN_MS);
    };

    void watchMemory(projectPath, notice).then(
      (watch) => {
        // The effect can be cleaned up before this resolves — switching
        // projects is exactly that — and a subscription registered afterwards
        // would outlive the hook that asked for it.
        if (watching) held = watch;
        else void unwatchMemory(projectPath, watch).catch(() => undefined);
      },
      () => {
        // Outside Tauri, or against a window that has no engine behind it.
        // Returning to the window still re-reads, which is what every reader
        // did before this existed.
      },
    );

    return () => {
      watching = false;
      if (soon !== null) clearTimeout(soon);
      if (held !== null) void unwatchMemory(projectPath, held).catch(() => undefined);
    };
  }, [projectPath]);
}

/**
 * How long writes landing together are treated as one.
 *
 * Long enough to collect the several records one transaction of an agent's
 * turn produces, short enough that a person watching the screen sees it change
 * rather than notices it changing late.
 */
const TOGETHER_WITHIN_MS = 300;
