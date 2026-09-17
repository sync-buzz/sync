"use client";

import { highlight, type Tone } from "@/lib/console/resolve";

/**
 * A typed line, coloured.
 *
 * One component for both places a line appears — being typed, and printed in
 * the block it produced — because they are the same line and a person reads
 * them a centimetre apart. Two paintings of one string is the arrangement
 * where the colour of `@review` depends on whether you are still typing it.
 *
 * The palette is the sixteen the window declares for terminal output, and
 * nothing here invents a colour: what is typed and what comes back are read in
 * one column, so a word that changed hue between them would read as a fault.
 */

/**
 * What each part of a line is drawn in.
 *
 * The assignment follows what a shell has trained a person to expect: the mark
 * that chooses a route is quiet, what is going to be run stands out, and the
 * rest of the line is ordinary text.
 */
function paint(tone: Tone): string {
  switch (tone) {
    case "sigil":
      return "var(--ansi-8)";
    case "name":
      return "var(--ansi-6)";
    case "verb":
      return "var(--ansi-2)";
    case "shell":
      return "var(--ansi-3)";
    case "said":
      return "var(--text-secondary)";
  }
}

/** The tone of what a line would become if the suggestion were taken. */
export const GHOST = "var(--ansi-8)";

export function ConsoleLine({
  line,
  ghost,
}: {
  readonly line: string;
  /** What taking the first suggestion would add, drawn faintly after it. */
  readonly ghost?: string | null;
}) {
  return (
    <>
      {highlight(line).map((part, index) => (
        <span key={index} style={{ color: paint(part.tone) }}>
          {part.text}
        </span>
      ))}
      {ghost === null || ghost === undefined ? null : (
        <span style={{ color: GHOST }}>{ghost}</span>
      )}
    </>
  );
}
