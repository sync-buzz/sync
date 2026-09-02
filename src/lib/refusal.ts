"use client";

/**
 * What was refused, in the words of whoever refused it.
 *
 * A rejection reaches the window in one of three shapes, and the third is the
 * whole reason this exists. A command that answers `Result<_, String>` rejects
 * with a sentence; something that threw rejects with an `Error`; and a command
 * that answers a typed failure — every one that can say `conflict` or `locked`
 * — rejects with an **object**, because that is what the window branches on.
 *
 * `String(…)` of that third shape is the literal words `[object Object]`, and a
 * screen that says them is a screen that has nothing to say. It was written
 * fifteen times in this window before it was written once here, and it was
 * invisible for as long as it was: on this machine the commands that fail most
 * often answer with a string, so the shape that breaks is the shape a person
 * meets on a bad day rather than on their first one.
 *
 * **What it must never do is invent a friendlier sentence.** The words belong
 * to whoever refused — an engine, a computer across a network, somebody's
 * server — and a phrase written here would be this window claiming to know
 * something about a failure it did not make.
 */
export function said(refused: unknown): string {
  if (typeof refused === "string" && refused.trim() !== "") return refused;
  if (refused instanceof Error) return refused.message;

  if (refused !== null && typeof refused === "object") {
    const named = refused as { message?: unknown; error?: unknown };
    if (typeof named.message === "string") return named.message;
    if (typeof named.error === "string") return named.error;
    // Its own words, whatever they were called. Unreadable prose beats an
    // unreadable placeholder: somebody can at least report what it said.
    try {
      return JSON.stringify(refused);
    } catch {
      return "the reason could not be read";
    }
  }

  return String(refused);
}
