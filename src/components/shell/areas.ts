import { Blocks, History } from "lucide-react";

/**
 * The one row of the sidebar that is the window's rather than a project's.
 *
 * Everything above it is a section an extension brought, and there is no list
 * of those here — there cannot be. `SHELL_AREAS` used to be that list, and
 * `ShellAreaId` used to be a union of its ids, which meant the compiler knew
 * the name of every extension the build shipped and no extension could exist
 * that it did not. Both are gone: an area is now whatever a loaded manifest
 * declared, addressed by a string the shell has never seen, and the type system
 * has nothing left to say about which ones there are.
 *
 * What survives is this row, and it survives because it is not an extension. It
 * is where a person decides which sections the project has, so it is pinned to
 * the foot of the column: the sections grow above it as extensions install
 * them, and it stays where it is.
 *
 * There is still no default area, and that is still the point. Which section a
 * project opens on is decided by the project — the first one it declared, and
 * this row when it declared nothing. A constant naming one would be the build
 * deciding what a repository contains.
 */
export const EXTENSIONS_AREA = {
  id: "extensions",
  // Named for the screen it opens on, which is the name that was already
  // inside it: the row a person pressed to get to the catalogue said
  // Marketplace from the beginning, so the section and its way in were two
  // names for one place. The id is untouched — it is what the layout is
  // remembered against, and moving it would move somebody's column.
  label: "Marketplace",
  description: "What this project can do, and what it could.",
  icon: Blocks,
  frame: "browse",
} as const;

/**
 * The other row that is the window's rather than a project's, and it is at the
 * top for the reason `Marketplace` is at the foot: it is not a section.
 *
 * A section shows what a project holds of one kind. This shows what has
 * happened across every kind at once, which is a view of the corpus rather than
 * a part of it — the same claim the search palette makes, drawn as a place
 * instead of as a gesture. It names no type and no extension: what it lists is
 * a key, a kind and a title, and where each one opens is asked of `opening.ts`,
 * exactly as the palette asks.
 *
 * It cannot be dragged. Somebody arranging their sections is deciding where
 * they work; this row is not one of the places they work, it is where they find
 * out what to look at first.
 */
export const ACTIVITY_AREA = {
  id: "activity",
  label: "Activity",
  description: "What has changed since you last looked.",
  icon: History,
  // A list and what it is a list of. The navigator holds the kinds that have
  // changed and the workspace holds the changes, which is how this system draws
  // a history everywhere else it draws one.
  //
  // There is no inspector: what would stand in one — which record, of what
  // kind, whose hand, when — *is* what a row already says, and a third column
  // repeating it would be the window printing the same four facts twice.
  frame: "list",
} as const;
