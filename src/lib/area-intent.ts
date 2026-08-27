/**
 * What one part of the window asks an area to show.
 *
 * Areas own what they are showing — which type is selected, which record is
 * open — and they keep it for as long as the window is open, which is what
 * makes leaving one and coming back cost nothing. That leaves no way to ask an
 * area to show something from outside it, and search is exactly that ask: a
 * result belongs to whichever area owns its type, and the palette is in the
 * title bar, above all of them.
 *
 * An intent is that ask, and it is deliberately thin. It names what to show and
 * nothing about how: no scroll position, no panel, no mode. An area receiving
 * one is free to reach it however it reaches it from its own navigator, which
 * is the only way an intent can stay meaningful for an area this build has
 * never seen.
 *
 * **Identity is the signal.** An area applies an intent when the object it was
 * given is one it has not applied yet, so asking for the same record twice is
 * two objects and opens it twice — the second ask is somebody who wandered off
 * and wants it back, not a duplicate to be swallowed.
 */
export type AreaIntent =
  /**
   * Open a record. The kind travels with the key because an area lists records
   * by type: without it, an area would have to read the record to find out
   * which of its own lists the row it is about to open belongs in.
   */
  | { readonly show: "record"; readonly key: string; readonly kind: string }
  /** Show one entry of the catalogue, by extension id. */
  | { readonly show: "extension"; readonly id: string };
