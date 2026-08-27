import type { ReactNode } from "react";

/**
 * The shapes the window can take, and the only ones it can take.
 *
 * The workspace is present in every frame — that is the shell's rule, not this
 * module's — so frames differ by what stands around it. The primary sidebar is
 * in none of them: it belongs to the window rather than to whatever the window
 * is currently showing.
 *
 * An area declares which frame it uses and fills that frame's slots. It does
 * not compose columns of its own, and this is the whole reason the set is
 * closed: an application whose shape is decided by whatever is installed has as
 * many shapes as it has extensions, and no rule left that can be enforced
 * against the next one.
 *
 * Geometry is not here and never will be. Widths, collapse thresholds and the
 * order space is released in belong to `shell-layout.ts`; this module answers
 * only which columns exist. The two are separate so that a new frame cannot
 * change how an existing column behaves.
 */
export type FrameId = "browse" | "list" | "detail" | "single";

export interface Frame {
  /** The column listing what the area holds. */
  readonly navigator: boolean;
  /** The column describing what the workspace is showing. */
  readonly inspector: boolean;
}

export const FRAMES = {
  /** List, item, and what is true of the item. Records and Extensions. */
  browse: { navigator: true, inspector: true },
  /** A list and the item it opens, where the item has nothing else to say. */
  list: { navigator: true, inspector: false },
  /** One subject, with its properties beside it. */
  detail: { navigator: false, inspector: true },
  /** One subject. Where a home screen would sit, if there is ever one. */
  single: { navigator: false, inspector: false },
} as const satisfies Record<FrameId, Frame>;

/**
 * What an area puts in the columns its frame has.
 *
 * Nodes rather than components, because an area is rendered by the window in
 * the window's own panels: what it hands over is what is inside a column, never
 * the column. A slot a frame does not have is not rendered, and — once areas
 * arrive from extensions — supplying one is an install-time error rather than
 * something quietly dropped. A panel that is empty because a component was
 * discarded without a word is an hour of looking for the wrong bug.
 */
export interface AreaSlots {
  readonly navigator?: ReactNode;
  readonly workspace: ReactNode;
  readonly inspector?: ReactNode;
}
