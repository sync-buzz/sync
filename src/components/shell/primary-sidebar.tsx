"use client";

import { EXTENSIONS_AREA } from "@/components/shell/areas";
import { PanelFooter, PanelSurface } from "@/components/shell/panel";
import { SourceList } from "@/components/shell/source-list";
import type { MountedArea } from "@/lib/extension-host/areas";
import type { Badges } from "@/lib/extension-host/badges";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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
  badges,
  updates,
  activeAreaKey,
  rail,
  onSelectArea,
  onArrange,
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
}) {
  const isActive = activeAreaKey === EXTENSIONS_AREA.id;

  return (
    <PanelSurface className="bg-sidebar">
      <SourceList
        label="Sections"
        items={sections.map((area) => ({
          id: area.key,
          label: area.label,
          icon: area.icon,
          badge: badges.get(area.key),
        }))}
        activeId={activeAreaKey ?? ""}
        rail={rail}
        onSelect={onSelectArea}
        onReorder={onArrange}
      />

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
  return news === null
    ? EXTENSIONS_AREA.label
    : `${EXTENSIONS_AREA.label} — ${news}`;
}
