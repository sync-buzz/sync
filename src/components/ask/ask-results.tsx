"use client";

import { useEffect, useState } from "react";

import { KindMark } from "@/components/shell/entity-marks";
import { memoryTypes } from "@/lib/memory/client";
import type { MemoryType, SearchHit } from "@/lib/memory/types";
import { typeName } from "@/lib/memory/use-corpus";
import { cn } from "@/lib/utils";

/**
 * What the project answered, under the line that asked it.
 *
 * **One flat list in the order the store ranked it**, and that is the whole
 * difference between this and the palette a project window opens. The palette
 * groups by type and labels the groups, because it is read as a report of what
 * the corpus holds for a question. This is read in the second before somebody
 * goes back to the application they called it from, so the first answer has to
 * be the first row — a heading above every second row is a line to get past on
 * the way to it.
 *
 * **It opens nothing.** A record is read in a window and this surface has
 * nowhere to draw one, so a row says which record it is and the line that owns
 * the keyboard decides what that means. The cursor is the line's too: the
 * keyboard and the pointer address one position, so a click and Return do the
 * same thing to the same row.
 *
 * **A record with no type cannot be reached from here**, and it says so in
 * place rather than being left out of the answer. What a window follows is an
 * address, an address names a kind, and a record that has none has no address —
 * dropping it would be the panel answering a question with less than the store
 * found.
 */
export function AskResults({
  project,
  hits,
  cursor,
  onCursor,
  onOpen,
}: {
  /** The project the question was asked of, for naming the types in the rows. */
  readonly project: string;
  readonly hits: readonly SearchHit[];
  readonly cursor: number;
  readonly onCursor: (at: number) => void;
  readonly onOpen: (hit: SearchHit) => void;
}) {
  // What the types are called and what mark each carries. Read here rather
  // than handed down, because it is the one thing this list needs that the
  // line above it has no use for — and it is read once per project rather
  // than per question: a name does not change between keystrokes.
  const [types, setTypes] = useState<readonly MemoryType[]>([]);
  useEffect(() => {
    let reading = true;
    void memoryTypes(project).then(
      (answer) => {
        if (reading) setTypes(answer);
      },
      () => {
        // A project whose types could not be read is a list of rows named by
        // their kind rather than by its title, which is what `typeName` already
        // answers with. There is nothing to say about it in a surface this
        // small.
      },
    );
    return () => {
      reading = false;
    };
  }, [project]);

  return (
    <ul
      // Its own scroller, which is what every panel in this application owns
      // for itself, and no hairline of its own: the edge above belongs to the
      // board this stands in.
      className="h-full overflow-y-auto px-3 py-2"
      role="listbox"
      aria-label="What the project holds"
    >
      {hits.map((hit, index) => {
        const kind = hit.kind;
        const snippet = hit.excerpt?.replace(/\s+/gu, " ").trim() ?? "";

        return (
          <li key={hit.id}>
            <button
              type="button"
              role="option"
              aria-selected={index === cursor}
              aria-disabled={kind === null}
              onMouseMove={() => onCursor(index)}
              onClick={() => onOpen(hit)}
              className={cn(
                "flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-left",
                index === cursor && "bg-selected",
              )}
            >
              <KindMark icon={types.find((type) => type.kind === kind)?.icon} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm text-fg">
                  {hit.title ?? hit.id}
                </span>
                {snippet === "" ? null : (
                  <span className="block truncate text-xs text-fg-tertiary">
                    {snippet}
                  </span>
                )}
              </span>
              <span className="shrink-0 text-xs text-fg-tertiary">
                {kind === null ? "No address" : typeName(types, kind)}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
