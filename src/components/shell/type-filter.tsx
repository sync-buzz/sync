"use client";

import { ListFilter } from "lucide-react";

import { kindIcon } from "@/components/shell/entity-marks";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { MemoryType } from "@/lib/memory/types";
import type { ProjectViewState } from "@/lib/project/use-project-view";

/**
 * Which of the project's types this window lists, and searches.
 *
 * It sits in the bottom bar of the column it governs, beside the control that
 * adds a type: that bar is where macOS keeps the actions belonging to a source
 * list, and it is one of the two bands in that column that do not scroll. A
 * control that acts on one column from another is one you have to remember
 * rather than find; a control inside the scroller is one you have to scroll
 * back to.
 *
 * One control for one fact. Unticking a type takes it out of the navigator, out
 * of "All claims", out of the counts and out of search at the same time: a
 * window that hid a type from the list while still finding it would be
 * answering with something it refuses to show. Nothing is removed from the
 * project by it — the records stay, agents go on writing them, and the
 * preference never leaves this machine.
 *
 * It is the shell's rather than any area's for the same reason the preference
 * is: two controls over one stored fact would be two ways of asking the same
 * question, and the second one to be written would win silently. The palette
 * and the navigator mount the same component over the same state.
 */
export function TypeFilter({
  types,
  counts,
  view,
  verb = "listed",
  align = "start",
}: {
  types: readonly MemoryType[];
  /**
   * How many records each listed kind holds. Omitted where the surface has no
   * count to stand behind — a palette knows what a search returned, not what
   * the corpus holds.
   */
  counts?: Readonly<Record<string, number>>;
  view: ProjectViewState;
  /** What this filter does to a type, in a word: `listed`, or `searched`. */
  verb?: string;
  align?: "start" | "end";
}) {
  if (types.length === 0) return null;

  const shown = types.length - view.hidden.length;
  const everything = view.hidden.length === 0;

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              // Emphasised while it is doing something, the way the panel
              // toggles in the title bar are: a filter that is on and looks off
              // is a list that seems to be missing rows.
              data-active={!everything}
              // The state is in the name, not only in the surface: an
              // icon-only control has to say what it is doing to somebody who
              // cannot see that it is emphasised.
              aria-label={
                everything
                  ? `Types ${verb} — all types`
                  : `Types ${verb} — ${shown} of ${types.length}`
              }
              className="text-fg-tertiary hover:text-fg data-[active=true]:bg-selected data-[active=true]:text-fg"
            >
              <ListFilter />
            </Button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>
          {everything
            ? `All types are ${verb}`
            : `${shown} of ${types.length} types are ${verb}`}
        </TooltipContent>
      </Tooltip>

      <DropdownMenuContent align={align} className="w-64">
        <DropdownMenuLabel>{`Types ${verb}`}</DropdownMenuLabel>
        {types.map((type) => {
          const Icon = kindIcon(type.icon);
          const isShown = !view.isHidden(type.kind);

          return (
            <DropdownMenuCheckboxItem
              key={type.kind}
              checked={isShown}
              // The menu stays open: hiding three types is one decision, not
              // three trips to the same button.
              onSelect={(event) => event.preventDefault()}
              onCheckedChange={() => view.toggle(type.kind)}
              className="gap-2"
            >
              <Icon aria-hidden="true" className="text-fg-tertiary" />
              <span className="truncate">{type.title}</span>
              {/* A hidden type has no count, because it is not being counted.
                  The number appears again the moment it is listed. */}
              {counts === undefined ? null : (
                <span className="ml-auto pr-4 font-mono text-xs text-fg-tertiary tabular-nums">
                  {isShown ? (counts[type.kind] ?? 0) : ""}
                </span>
              )}
            </DropdownMenuCheckboxItem>
          );
        })}

        <DropdownMenuSeparator />
        <DropdownMenuItem disabled={everything} onSelect={view.showAll}>
          Show all types
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
