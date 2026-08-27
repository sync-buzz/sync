import { Blocks } from "lucide-react";

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
  label: "Extensions",
  description: "What this project can do, and what it could.",
  icon: Blocks,
  frame: "browse",
} as const;
