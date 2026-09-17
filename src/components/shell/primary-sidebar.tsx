"use client";

import { ChevronUp } from "lucide-react";

import { ACTIVITY_AREA, EXTENSIONS_AREA } from "@/components/shell/areas";
import { PanelFooter, PanelSurface } from "@/components/shell/panel";
import { SourceList } from "@/components/shell/source-list";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { MountedArea } from "@/lib/extension-host/areas";
import type { Badges } from "@/lib/extension-host/badges";
import type { NativeMenuEntry } from "@/lib/native-menu";
import { cn } from "@/lib/utils";

/**
 * The durable sections of the product. This column stays narrow and stable:
 * it answers "where am I", never "what is in here" — that is the navigator's
 * job in the column beside it.
 *
 * It carries no panel header. A source list on the window material is legible
 * as navigation without being labelled "Sections", and the row it saves is
 * worth more than the word.
 *
 * Extensions is the one row pinned to the foot of the column. It is an area
 * like the others — selecting it deselects whatever was selected — but it is
 * not a section of the project: it is where a person decides which sections the
 * project has. The sections grow above it; it stays where it is.
 *
 * The sections can be dragged into the order somebody wants them, and the
 * pinned row cannot: it is pinned, and being able to carry it away from the
 * foot of the column would be the interface disagreeing with its own rule. The
 * order is remembered per project on this Mac — see `use-section-order.ts` for
 * why it is not written into the repository.
 *
 * The column folds in two steps, and the first of them is this: the labels go
 * and the icons stay. Every row keeps its height, its place and its icon, so
 * the fold reads as the words leaving rather than as a different column
 * arriving — and the sections are still there to be switched between, which is
 * the whole reason to stop here instead of closing the column outright.
 */
export function PrimarySidebar({
  sections,
  hiddenAreas,
  badges,
  updates,
  unseen,
  activeAreaKey,
  rail,
  onSelectArea,
  onArrange,
  onHide,
  onShow,
}: {
  /**
   * The sections this project has, which is what its extensions brought. An
   * empty column is the ordinary state of a project somebody has just made:
   * nothing labels it, because a line reading "no sections" would name an
   * absence instead of showing one.
   *
   * Every one of them arrived from a package this build has never heard of, so
   * a row is a label and a mark the manifest named — there is nothing else the
   * column knows about a section, and nothing else it needs.
   */
  sections: readonly MountedArea[];
  /**
   * Sections this person put away, kept here rather than dropped so the
   * disclosure row at the foot of the list can name them and offer them back.
   *
   * Empty is the ordinary state — nothing is hidden — and the row is absent
   * rather than labelled, so the column reads the same as it did before
   * hiding was a thing it could do.
   */
  hiddenAreas: readonly MountedArea[];
  /**
   * How much of what a section holds is worth a look, by area key.
   *
   * Counted by the window rather than reported by the section, because a
   * section is mounted the first time it is opened and a number that arrived
   * with it would be missing from every section nobody has been to yet. A
   * section with nothing to say is absent from the map rather than in it with a
   * zero: a badge saying none is a mark that means nothing, and this column has
   * no room for one.
   */
  badges: Badges;
  /**
   * How many of this project's extensions have a newer version to move to.
   *
   * Drawn as a dot rather than as the figure, and that follows rule 11 rather
   * than saving room: a count is how many there are, standing and as true when
   * nobody is looking, while this is *something happened, go and look* — which
   * is what the row is for. The number is only here so that the tooltip can say
   * it in words, since the dot cannot explain itself.
   */
  updates: number;
  /**
   * How many records have changed since this person last looked at the
   * activity, or `null` while that is still being read.
   *
   * A figure rather than a dot, and the difference is the claim: unread things
   * are countable and stay countable while nobody is looking, which is what a
   * figure means here and everywhere else on this system. `null` draws nothing
   * — a window that printed `0` before it had asked would tell somebody with
   * twenty things waiting that nothing had happened.
   */
  unseen: number | null;
  /**
   * The section showing, or `null` while the window is still finding out what
   * there is. Nothing is current in that moment, which is the truth of it: the
   * packages are being read and no section has been chosen over another.
   */
  activeAreaKey: string | null;
  /** The column is folded to icons. */
  rail?: boolean;
  onSelectArea: (key: string) => void;
  /**
   * The sections were put in this order, by key.
   *
   * A person deciding where they work, which is why this column can be
   * rearranged and the settings window's cannot: the sections above are a place
   * somebody is in every day, and where they sit by default is the order the
   * project happens to declare its extensions in. The pinned row below is not
   * in it and never moves — it is not a section of the project.
   */
  onArrange: (keys: readonly string[]) => void;
  /** Put a section away from the sidebar, by area key. */
  onHide: (key: string) => void;
  /** Bring a section back to the sidebar, by area key. */
  onShow: (key: string) => void;
}) {
  const isActive = activeAreaKey === EXTENSIONS_AREA.id;

  return (
    <PanelSurface className="bg-sidebar">
      {/* One list, and the window's own rows simply do not move in it. Two
          lists was the first attempt and it was wrong twice over: a source list
          is built to fill its column, so two of them divided the height between
          them, and even boxed to its own height the upper one kept its own
          padding — which read as a gap between two things rather than as a
          column of rows.

          What is left is the arrangement macOS actually uses. Activity is a row
          like any other, at the top because that is where what has happened
          belongs, and it stays there because it is the window's rather than the
          project's — the same claim the pinned row at the foot makes from the
          other end. The sections between them are what the project brought, and
          those are a person's to arrange.

          More of the window's own rows will arrive. They join this list with
          `fixed`, above the sections, and nothing else here changes: the rule is
          about who a row belongs to, not about how many there are. */}
      <SourceList
        label="Sections"
        items={[
          {
            id: ACTIVITY_AREA.id,
            label: ACTIVITY_AREA.label,
            icon: ACTIVITY_AREA.icon,
            note: ACTIVITY_AREA.description,
            fixed: true,
            // Nothing unseen draws nothing at all, which is the silence the
            // title bar keeps for a project in step with its remote: a mark
            // that is always there is not news, and a zero is not a count
            // anybody can act on.
            badge:
              unseen === null || unseen === 0
                ? undefined
                : { kind: "count" as const, value: unseen },
          },
          ...sections.map((area) => ({
            id: area.key,
            label: area.label,
            icon: area.icon,
            badge: badges.get(area.key),
            menu: hideMenuFor(area, onHide),
          })),
        ]}
        activeId={activeAreaKey ?? ""}
        rail={rail}
        onSelect={onSelectArea}
        onReorder={onArrange}
      />

      {/* The quiet line that says something is hidden and offers it back. It
          exists only while there is something to offer — the column is exactly
          as it was before hiding was a thing it could do when there is not —
          and it sits between the list and the pinned row rather than inside
          either: the list is the places a person works and the pinned row is
          the window's own, and this is neither.

          A popover rather than an expansion, so the list above does not shift:
          the rows keep their places, the popover floats over them, and a
          person who put three sections away and wants one back does not find
          the other two have moved under their pointer. */}
      {hiddenAreas.length > 0 ? (
        <HiddenSectionsDisclosure areas={hiddenAreas} rail={rail} onShow={onShow} />
      ) : null}

      {/* The band is the one the navigator's bottom bar sits in, so the two
          line up across the slab. What is in it is therefore shorter than a
          row in the list above, and — for the same reason — it is not marked
          by a filled surface: a fill at this height would run into the
          hairline above it. Weight and colour carry the selection instead,
          which is the half of the rule that survives greyscale anyway. */}
      <PanelFooter>
        <ExtensionsRow
          isActive={isActive}
          updates={updates}
          rail={rail}
          onSelect={() => onSelectArea(EXTENSIONS_AREA.id)}
        />
      </PanelFooter>
    </PanelSurface>
  );
}

/** The row at the foot of the column, folding the way the ones above it do. */
function ExtensionsRow({
  isActive,
  updates,
  rail,
  onSelect,
}: {
  isActive: boolean;
  updates: number;
  rail?: boolean;
  onSelect: () => void;
}) {
  const Icon = EXTENSIONS_AREA.icon;
  const news = updates > 0 ? spokenUpdates(updates) : null;

  const row = (
    <button
      type="button"
      data-active={isActive}
      aria-current={isActive ? "true" : undefined}
      // The dot is drawn rather than written, so what it says is said here.
      aria-label={rail || news !== null ? spoken(news) : undefined}
      onClick={onSelect}
      className={cn(
        "group flex h-(--control-height) min-w-0 flex-1 items-center gap-2.5 rounded-(--radius-control) text-left text-base text-fg-tertiary transition-colors duration-(--motion-duration-fast) ease-shell hover:text-fg data-[active=true]:font-medium data-[active=true]:text-fg",
        rail ? "justify-center px-0" : "px-2",
      )}
    >
      {/* Folded, the dot goes on the icon, which is where the rows above put
          theirs: news is news at any width, and this column narrowing is the
          words leaving rather than a different column arriving. */}
      <span className="relative shrink-0">
        <Icon className="size-4 opacity-70 transition-opacity duration-(--motion-duration-fast) group-hover:opacity-100 group-data-[active=true]:opacity-100" />
        {rail && news !== null ? (
          <span
            aria-hidden
            className="absolute -top-0.5 -right-0.5 size-1.5 rounded-full bg-fg-tertiary"
          />
        ) : null}
      </span>
      {rail ? null : (
        <>
          <span className="truncate">{EXTENSIONS_AREA.label}</span>
          {news === null ? null : (
            <span
              aria-hidden
              className="ml-auto block size-1.5 shrink-0 rounded-full bg-fg-tertiary"
            />
          )}
        </>
      )}
    </button>
  );

  // A dot is the one mark here that cannot explain itself, so it earns a
  // tooltip even in a column wide enough to have needed none — the same rule
  // the sections above this row read.
  if (!rail && news === null) return row;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{row}</TooltipTrigger>
      <TooltipContent side="right">{spoken(news)}</TooltipContent>
    </Tooltip>
  );
}

/**
 * What the dot means, in words, and it never names a figure.
 *
 * The count decides which sentence rather than appearing in it. "3 updates" on
 * this row would be a standing figure, which is the claim a dot is not making:
 * what it says is that there is something to go and look at, and the number of
 * things is on the page it leads to.
 */
function spokenUpdates(updates: number): string {
  return updates === 1 ? "an update is available" : "updates are available";
}
function spoken(news: string | null): string {
  return news === null ? EXTENSIONS_AREA.label : `${EXTENSIONS_AREA.label} — ${news}`;
}

/**
 * What the secondary button offers over a section row.
 *
 * One command — *Hide* — and it names the section under the pointer, which is
 * the one place in the window where the label is the thing being acted on
 * rather than a word the header already said. The same shape [`SourceTree`]
 * gives its own rows, because a secondary click means one thing in this window
 * whichever control drew it.
 */
function hideMenuFor(
  area: MountedArea,
  onHide: (key: string) => void,
): () => readonly NativeMenuEntry[] {
  return () => [{ label: `Hide ${area.label}`, onSelect: () => onHide(area.key) }];
}

/**
 * The quiet line between the list and the pinned row, and the popover that
 * floats over the list when it is pressed.
 *
 * It is the one piece of furniture this column carries besides the pinned row,
 * and it is held to the same visual tier: tertiary text, no icon, no surface
 * fill — weight and colour alone, the half of the selection rule that survives
 * greyscale. A person who has never hidden anything never sees it, and a
 * person who has is told it is there without being stopped by it.
 *
 * The popover opens upward, because the row is at the foot of the list and the
 * space above is where the hidden sections can be listed without running past
 * the bottom edge. It closes after each restore: restoring is a one-at-a-time
 * gesture, and a menu that stayed open after the last section it listed was
 * taken would be a menu pointing at a trigger that had just vanished.
 */
function HiddenSectionsDisclosure({
  areas,
  rail,
  onShow,
}: {
  areas: readonly MountedArea[];
  rail?: boolean;
  onShow: (key: string) => void;
}) {
  const trigger = (
    <button
      type="button"
      aria-label={rail ? `${areas.length} hidden` : undefined}
      className={cn(
        "flex h-(--control-height) min-w-0 items-center gap-1.5 rounded-(--radius-control) text-left text-xs text-fg-tertiary transition-colors duration-(--motion-duration-fast) ease-shell hover:text-fg-secondary",
        rail ? "justify-center px-0 w-full" : "px-4",
      )}
    >
      {rail ? (
        <ChevronUp className="size-3" />
      ) : (
        <>
          <span>{areas.length} hidden</span>
          <ChevronUp className="size-3 shrink-0" />
        </>
      )}
    </button>
  );

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent side="right">
          {areas.length === 1 ? "1 hidden section" : `${areas.length} hidden sections`}
        </TooltipContent>
      </Tooltip>
      <DropdownMenuContent side="top" align="start" className="w-52">
        <DropdownMenuLabel>Hidden</DropdownMenuLabel>
        {areas.map((area) => {
          const Icon = area.icon;
          return (
            <DropdownMenuItem key={area.key} onSelect={() => onShow(area.key)} className="gap-2">
              <Icon aria-hidden className="size-4 text-fg-tertiary" />
              <span className="truncate">{area.label}</span>
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
