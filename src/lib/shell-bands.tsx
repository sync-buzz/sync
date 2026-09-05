"use client";

import { createContext, useContext } from "react";

/**
 * Where a column's bands are drawn, which is not always inside the column.
 *
 * A column on a Mac carries its own head and foot: a title above the list, and
 * a strip of controls under it that act on what the list holds. At the width of
 * a phone the column *is* the screen, and a strip drawn inside it would be a
 * second bar under the screen's own — the platform has one place for controls
 * that act on a list, and it is the bar at the foot of the screen.
 *
 * So the strip stays where a package puts it and appears where the phone keeps
 * it. That is the same arrangement the columns themselves use, one level down,
 * and for the same reason: a package cannot be asked to know which machine it
 * is drawn on, and a shell that moved the controls by rewriting them would be
 * a shell that has to be taught about every package that ever ships.
 *
 * Both contexts are absent on a Mac, and absence is what "draw it in place"
 * means — the desktop is not a case handled here, it is the case with no
 * context at all.
 */

/** The columns of a frame, by the names the frame gives them. */
export type AreaColumn = "Navigator" | "Workspace" | "Inspector";

/** Which column the tree below is inside of. */
const WhichColumn = createContext<AreaColumn | null>(null);

/** The node each column's foot is drawn into, where one is offered. */
const BandSlots = createContext<Partial<Record<AreaColumn, HTMLElement | null>>>(
  {},
);

export const ColumnProvider = WhichColumn.Provider;
export const BandSlotsProvider = BandSlots.Provider;

/**
 * The node this column's foot belongs in, or `null` to draw it in place.
 *
 * Read through the context that the *area's* tree carries rather than through
 * the DOM, because a column is portalled: it is rendered where its area is and
 * appears where the window put it. React context follows the tree a component
 * was rendered in, so it crosses that portal — which is exactly what is needed
 * here, and exactly what a DOM lookup could not do.
 */
export function useBandSlot(): HTMLElement | null {
  const column = useContext(WhichColumn);
  const slots = useContext(BandSlots);
  return column === null ? null : (slots[column] ?? null);
}
