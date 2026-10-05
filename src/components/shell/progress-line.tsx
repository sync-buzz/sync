"use client";

import { useWaiting } from "@/lib/command";

/**
 * That the window is waiting, drawn on the hairline under its top band.
 *
 * One line for the whole window, because the wait is the window's: a column
 * that drew its own would be three reports of one engine, and the two that had
 * nothing to say would read as three columns disagreeing. It reports no
 * subject and no share of the work — an indeterminate wait is all the store
 * ever tells us, and a bar filling to a percentage nobody measured is the one
 * lie an interface is never forgiven for.
 *
 * It sits on the separator rather than above or below it, so nothing in the
 * window moves when it appears. A band that grew by two pixels would push every
 * column down and back, which is a larger event than the thing being reported.
 *
 * **A section contributes nothing to this.** What lights it is the count of
 * commands the window has not been answered on, kept in `lib/command.ts`; an
 * extension reads the corpus through the shell's own functions and is reported
 * by the act of asking.
 *
 * It is drawn in the accent rather than in a grey, and that is the third of
 * the three sentences one tone carries here: *this is happening*. A tertiary
 * grey two pixels high on a hairline is a line nobody notices, which for the
 * one mark that says the window is waiting is the whole of the failure.
 *
 * There is no fade. A line this thin fading in is a line that is illegible for
 * as long as it is arriving, and what it says — *something is happening* — is
 * worth nothing late. The steadiness comes from the thresholds instead: it is
 * not drawn for a wait too short to feel, and it stays long enough to be read.
 */
export function ProgressLine() {
  const waiting = useWaiting();
  if (!waiting) return null;

  return (
    <div
      role="progressbar"
      aria-label="Working"
      className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden motion-reduce:bg-accent-text/40"
    >
      {/*
        A segment that travels, on a track that is not drawn: the hairline it
        sits on is the track, and a second one over it would be two lines where
        the window has one. With reduced motion the segment is dropped and the
        whole width is tinted instead — the launch screen can drop it and keep
        the word `Starting`, and here there is no word to fall back to.
      */}
      <div className="h-full w-1/3 bg-accent-text animate-[indeterminate-progress_1.4s_var(--motion-ease)_infinite] motion-reduce:hidden" />
    </div>
  );
}
