"use client";

import { useMemo } from "react";
import qrcode from "qrcode-generator";

/**
 * The two values a device needs, as something its camera can read.
 *
 * A key is sixty-four characters of hex. Nobody types that correctly, and
 * asking somebody to is asking them to give up — so the code is the way to pair
 * and the text below it is the fallback, not the other way round.
 *
 * **The encoder is a library and the drawing is not.** `qrcode-generator` turns
 * a string into a grid of dark and light; wrapping that in React is thirty
 * lines and one dependency fewer than the wrappers that exist, and it is what
 * lets this draw one path instead of nine hundred rectangles.
 *
 * **Dark on white, whatever the window is wearing.** This is the one surface
 * here that is not themed, and deliberately: a scanner reads contrast, and a
 * code in the window's own greys is a code that works in one appearance and
 * fails in the other. The white quiet zone around it is part of the code rather
 * than padding — without it a reader cannot find the edges.
 */
export function PairingCode({ payload }: { payload: string }) {
  const { path, size } = useMemo(() => drawn(payload), [payload]);

  return (
    <div className="flex justify-center">
      <svg
        role="img"
        aria-label="Pairing code"
        viewBox={`0 0 ${size} ${size}`}
        className="size-56 rounded-(--radius-control) bg-white"
        shapeRendering="crispEdges"
      >
        {/* Named rather than a token: see above. */}
        <path d={path} fill="#000000" />
      </svg>
    </div>
  );
}

/**
 * The quiet zone, in modules, as the specification states it.
 *
 * Four on every side. It is not a margin somebody chose and it is not
 * negotiable: a reader finds the code by finding the emptiness around it.
 */
const QUIET = 4;

/** One path over the whole grid, rather than a rectangle per dark module. */
function drawn(payload: string): { path: string; size: number } {
  // Type 0 asks the encoder to pick the smallest version the payload fits in,
  // so a longer address does not silently overflow a fixed one. "M" is the
  // middle of the four error-correction levels: enough that a thumb over a
  // corner still reads, without the density that costs a phone its focus.
  const code = qrcode(0, "M");
  code.addData(payload);
  code.make();

  const count = code.getModuleCount();
  let path = "";
  for (let row = 0; row < count; row += 1) {
    for (let column = 0; column < count; column += 1) {
      if (code.isDark(row, column)) {
        path += `M${column + QUIET} ${row + QUIET}h1v1h-1z`;
      }
    }
  }
  return { path, size: count + QUIET * 2 };
}
