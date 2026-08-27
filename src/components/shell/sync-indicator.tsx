"use client";

import { Button } from "@/components/ui/button";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import { cn } from "@/lib/utils";

/**
 * Whether this project's memory is in step with its remote, in the header.
 *
 * Synchronisation is true of the whole project at once, which is what puts it
 * in the band that already holds the project switcher rather than in any one
 * column. It is a word and not an icon with a badge, because colour here is
 * reserved for status and destructive actions, and a count in a dot is a thing
 * you have to learn to read.
 *
 * **Silence is a state.** A project whose memory matches its remote shows
 * nothing at all — the same silence a record nobody has typed into gets from
 * the band above the workspace. What appears, appears because something is
 * true that a person would want to know: their writing is only here, or
 * somebody else's is not here yet.
 *
 * **The state is the door.** One element says what is true and opens the sheet
 * where it can be acted on, because they are the same subject: a person who
 * reads "3 unpublished" is already asking the question the sheet answers. When
 * there is nothing to say there is nothing to press either — a project in step
 * with its remote has no control here at all, which is the same silence the
 * band above the workspace keeps for a record nobody has typed into.
 */
export function SyncIndicator({
  sync,
  onOpen,
}: {
  sync: SyncStatus;
  onOpen: () => void;
}) {
  const said = sentence(sync);
  if (!said) return null;

  // The one coloured state in this band, and it is the only one that reports a
  // decision taken on somebody's behalf: a merge that kept this side's text
  // where a colleague had edited the same lines. Everything else here is a
  // fact about where memory is, which colour would only decorate.
  const overwrote = sync.overlaps.length > 0 && sync.busy === null;

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={onOpen}
      className={cn(
        "min-w-0 text-xs font-normal",
        overwrote ? "text-warning" : "text-fg-tertiary",
      )}
    >
      <span className="truncate">{said}</span>
    </Button>
  );
}

/**
 * What is worth saying, in the order of what a person would want to know.
 *
 * Unpublished work comes first: it is theirs, and it is the half that is
 * nowhere else. `null` is the ordinary case and means the header carries
 * nothing here at all.
 */
function sentence(sync: SyncStatus): string | null {
  // An exchange in progress outranks what it is about to change: the count is
  // already out of date, and this is the one place an ellipsis is allowed.
  if (sync.busy === "fetching") return "Fetching…";
  if (sync.busy === "publishing") return "Publishing…";

  // A merge that had to choose outranks the counts: it is news, and it is the
  // only thing here somebody may want to act on before it scrolls out of mind.
  if (sync.overlaps.length > 0) {
    return sync.overlaps.length === 1
      ? "1 merged over"
      : `${sync.overlaps.length} merged over`;
  }

  const state = sync.state;
  if (!state) return null;

  // A memory nobody has given a remote is not a fault, and this must not read
  // as one. It is said at all because the alternative is a person discovering
  // on a second machine that a year of their project's memory never left the
  // first one.
  if (!state.remoteConfigured) return "Not published";

  const mine =
    state.unpublished > 0 ? `${state.unpublished} unpublished` : null;

  // "Nobody could say" is not "nothing is waiting", so it is said rather than
  // absorbed — but never in place of the count, which is still true.
  if (state.remote === "unreachable") {
    return mine ? `${mine}, remote unreachable` : "Remote unreachable";
  }
  if (state.remote === "waiting") {
    return mine ? `${mine}, updates waiting` : "Updates waiting";
  }
  return mine;
}
