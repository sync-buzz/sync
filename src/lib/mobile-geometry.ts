import { FRAMES, type FrameId } from "@/lib/shell-frames";

/**
 * The second geometry for the same four frames, and the reason there has to be
 * a second one.
 *
 * `src/lib/shell-frames.ts` answers which columns a frame has and refuses to
 * say how wide they are; `src/lib/shell-layout.ts` is the answer for a desktop
 * window. This is the answer for a screen 390 points wide, and it cannot be the
 * first one narrowed: the workspace alone declares a floor of 500 px, so the
 * columns of a frame cannot stand beside each other at any width a phone has.
 * What they can do is stand one after another. The arrangement stops being a
 * division of space and becomes an order in time.
 *
 * Everything here reads the frames and none of it redefines them. That is the
 * whole claim being tested: if a phone needed a fifth frame, or a column no
 * frame declares, that would be the finding — and it is not.
 */

/**
 * The screens a phone can be on, in the order it reaches them.
 *
 * `sections` is not part of any frame, and that is not an oversight in the
 * frames: the column listing the sections belongs to the window rather than to
 * what the window is showing, so on a desktop it stands outside the frame
 * entirely. A phone has no outside. Something has to be the first screen, and
 * the only honest candidate is the one that says where you are.
 */
export const MOBILE_LEVELS = [
  "sections",
  "navigator",
  "workspace",
  "inspector",
] as const;

export type MobileLevel = (typeof MOBILE_LEVELS)[number];

/**
 * How the inspector arrives, which is the one question the frames do not
 * answer and a phone cannot avoid.
 *
 * On a desktop the inspector stands beside the workspace, so what it describes
 * stays in sight while it is read. Neither option below keeps that: a push
 * replaces the workspace, and a sheet covers most of it. They fail differently
 * — a push gives the inspector the whole screen and takes the subject away, a
 * sheet keeps a strip of the subject and gives the inspector two thirds — and
 * which failure is acceptable is a judgement about the product, not about the
 * layout. So the prototype builds both and a person decides.
 */
export type InspectorPresentation = "push" | "sheet";

/**
 * The screens this frame has on a phone, deepest last.
 *
 * The workspace is in every one of them because it is in every frame, which is
 * the shell's rule rather than this module's. The two optional columns become
 * the two optional screens, in the order they sit in on a desktop: what lists,
 * then what is shown, then what is true of it. Reading left to right is reading
 * first to last, and that is not a coincidence to be relied on — it is why the
 * order of the columns was worth keeping.
 */
export function levelsOf(
  frame: FrameId,
  inspector: InspectorPresentation,
): readonly MobileLevel[] {
  const { navigator, inspector: hasInspector } = FRAMES[frame];
  return [
    "sections" as const,
    ...(navigator ? (["navigator"] as const) : []),
    "workspace" as const,
    // A sheet is raised over the workspace rather than pushed after it, so in
    // that presentation the inspector is not a screen the stack knows about.
    ...(hasInspector && inspector === "push" ? (["inspector"] as const) : []),
  ];
}

/** Whether this frame has an inspector at all, however it is presented. */
export function hasInspector(frame: FrameId): boolean {
  return FRAMES[frame].inspector;
}

/**
 * The measurements a touch screen imposes, kept here because they are
 * behaviour rather than paint — the same reason panel widths are not CSS
 * tokens.
 *
 * They are not the desktop's numbers scaled. A pointer lands where it is
 * aimed, so a desktop row can be 24 px and a band 34; a finger is about 44
 * points across whatever the screen density is, and that number is the floor
 * under every row and control here.
 */
export const TOUCH_TARGET = 44;
