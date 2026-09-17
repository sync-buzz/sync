/**
 * What the harness around the prototype measures, and nothing the window uses.
 *
 * The geometry itself moved to `src/lib/mobile-geometry.ts` when the window
 * gained it: the arrangement stopped being an argument and became the thing.
 * What stayed is the size of the box this page draws the phone in, which is a
 * property of looking at a prototype rather than of being one.
 */

/**
 * A screen the prototype is drawn at: an iPhone in portrait, in points, and
 * what its hardware takes out of that at either edge.
 *
 * The insets are the measured ones — 62 above, 34 below — rather than round
 * numbers, because what they are for is showing whether an arrangement still
 * holds when a notch and a home indicator take a hundred points between them.
 * Round numbers would make that test easier than the device makes it.
 */
export const DEVICE = {
  width: 390,
  height: 844,
  insets: { "--safe-top": "62px", "--safe-bottom": "34px" },
} as const;
