"use client";

import { useEffect, useState } from "react";

import type { Running } from "@/lib/console/works";

/**
 * What is going, on the shelf between the canvas and the line being typed.
 *
 * **It is the receipt.** A line addressed to an agent leaves the input and
 * draws no block until there is an answer, so this row is the only thing that
 * says it was taken — and it appears in the same frame the line left, before
 * any agent is up.
 *
 * **One line each, and never more.** What a row says is the name it is
 * addressed by, the tool the agent is in, and how long it has been going.
 * Reasoning is not here and a running answer is not here: a shelf that grew to
 * hold what an agent was saying would be the console becoming a second chat,
 * and that is the one thing the whole arrangement is built to refuse.
 *
 * Nothing is drawn when nothing is going. An empty shelf that kept its height
 * would move the input line down for a state that means *there is nothing to
 * report*.
 */

/** How often the ages are redrawn. */
const TICK_MS = 1_000;

/**
 * How long it has been going, in the words the block uses when it is over.
 *
 * Seconds, until there are enough of them to be minutes. Sub-second precision
 * on something a person is waiting through is a number that flickers and says
 * nothing they can act on.
 */
function age(startedAt: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - startedAt) / 1000));
  if (seconds < 60) return `${seconds} s`;
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

export function ConsoleShelf({
  running,
}: {
  readonly running: readonly Running[];
}) {
  // One clock for the whole shelf rather than one per row, and it only runs
  // while there is something to age: a timer ticking behind a closed console
  // is work nobody asked for, repeated for as long as the application is open.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (running.length === 0) return;
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, [running.length]);

  if (running.length === 0) return null;

  return (
    <div
      // `list` rather than `log`: the canvas is what is announced as it
      // happens, and a screen reader told about every tool an agent touched
      // would be a screen reader nobody could work through.
      role="list"
      aria-label="Work in progress"
      className="shrink-0 px-4 pb-1"
    >
      {running.map((work) => (
        <div
          key={work.id}
          role="listitem"
          className="flex items-start font-mono text-sm"
          style={{ lineHeight: "var(--console-cell-h)" }}
        >
          {/* The same two cells the canvas and the input line reserve, so the
              glyph stands in the gutter and the name in the column every other
              line starts in. */}
          <span
            aria-hidden
            className="w-[calc(var(--console-cell-w)*2)] shrink-0 text-fg-tertiary"
          >
            ›
          </span>

          <span className="shrink-0 text-fg">{`@${work.name}`}</span>

          {/* What it is doing, and it gives up the room first: a long tool
              title must not push the age off the end of the line, because the
              age is the part somebody is actually watching. */}
          <span className="min-w-0 flex-1 truncate pl-[calc(var(--console-cell-w)*2)] text-fg-secondary">
            {work.queued
              ? "waiting for an agent"
              : work.asking
                ? "waiting for an answer"
                : (work.doing ?? "")}
          </span>

          <span className="shrink-0 pl-[calc(var(--console-cell-w)*2)] text-fg-tertiary tabular-nums">
            {age(work.startedAt, now)}
          </span>
        </div>
      ))}
    </div>
  );
}
