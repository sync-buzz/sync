/**
 * What the harness around the prototype measures, and nothing the window uses.
 *
 * The geometry itself moved to `src/lib/mobile-geometry.ts` when the window
 * gained it: the arrangement stopped being an argument and became the thing.
 * What stayed is the size of the box this page draws the phone in, which is a
 * property of looking at a prototype rather than of being one.
 */

/** A screen the prototype is drawn at: an iPhone in portrait, in points. */
export const DEVICE = { width: 390, height: 844 } as const;
