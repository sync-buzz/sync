"use client";

import { useRef, useState, type KeyboardEvent } from "react";
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import type { LucideIcon } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { showNativeContextMenu, type NativeMenuEntry } from "@/lib/native-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/**
 * A macOS source list: the column that answers "where am I".
 *
 * There are two of them in Sync — the sections of a project, and the sections
 * of the settings window — and they are one control rather than two that
 * resemble each other. Focus follows selection and the arrow keys move it, as a
 * native source list does; selection is a surface shift and a weight change,
 * with no coloured fill and no leading marker.
 *
 * It carries no header: a source list on its own surface is legible as
 * navigation without being labelled, and the row that label costs is worth more
 * than the word.
 *
 * **Rows can be rearranged where rearranging them means something**, which is
 * the sections of a project and not the sections of the settings window: the
 * first list is a place somebody works in every day and the second is two
 * fixed screens. That is `onReorder`, and a list without it drags nowhere.
 *
 * The gesture is the one macOS uses in Finder's sidebar and Mail's mailbox
 * list, and it is not the one the web usually reaches for. **The rows do not
 * part.** The row being carried stays where it is and goes quiet, and a hairs'
 * breadth of a line appears between the two rows it would land between — so
 * the list under the pointer never moves while somebody is aiming at it, which
 * is the same rule [`SourceTree`] already keeps for its own drags. The line is
 * drawn in the tier the badges use rather than in a colour: this window keeps
 * colour for status and for destruction, and where something will land is
 * neither.
 */

export interface SourceListItem {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  /**
   * What the row is for, in a sentence, shown under the pointer and nowhere
   * else. Not a count and not a state of what the row holds — those belong to
   * whatever the row is about.
   *
   * **It is never drawn beside the label.** A column this narrow has room for
   * one of the two, and the one a person navigates by is the name: a
   * description set on the row takes the width the label needed and abbreviates
   * it, so the list ends up hiding exactly what it exists to show. Hover is
   * where the longer answer goes, which is where [`SourceTree`] already puts
   * the same thing.
   */
  readonly note?: string;
  /**
   * What this row is about, in a figure — or that something happened to it.
   *
   * The one place in this column that says something about a row's *contents*
   * rather than about the row, which is why it sits at the trailing edge where
   * a source list on this system keeps one: Mail, Reminders, Xcode's navigator.
   * It carries no colour, because a count is information and this window keeps
   * colour for status and for destruction — position, weight and the shape of
   * the mark are what say it, so the row reads the same in greyscale.
   *
   * **The two kinds mean different things and are never each other.** A `count`
   * is how many there are, a standing figure that is as true when nobody is
   * looking; a `dot` is *something happened, go and look*, which is what a dot
   * means everywhere else on this system. A dot standing in for a count too
   * large to print would be one mark carrying both claims, so a large count is
   * abbreviated instead — see [`BADGE_CEILING`].
   */
  readonly badge?: { readonly kind: "count"; readonly value: number } | { readonly kind: "dot" };
  /**
   * What the secondary button offers over this row, or nothing where the row
   * answers to no commands.
   *
   * A thunk rather than a list, and built when the menu is asked for: by then
   * the row may stand for something that has changed since it was drawn, and a
   * menu made at render would act on what was on screen rather than on what is
   * there now. The same shape [`SourceTree`] gives its own rows, because a
   * secondary click on a row means one thing in this window whichever of the
   * two controls drew it — and a gesture that works in one column and dies in
   * the next teaches nobody where a command lives.
   */
  readonly menu?: () => readonly NativeMenuEntry[];
}

/**
 * The figure a row has room for, above which one is abbreviated.
 *
 * A fact about the space rather than about what is being counted, which is why
 * it is here and not beside whatever produced the number. A row is one line in
 * a column that folds, and four figures in it stop being read and start being
 * measured; `99+` is the same news in the width there is.
 */
export const BADGE_CEILING = 99;

/**
 * Where a row would land, relative to the row saying it.
 *
 * Two values and not an index, because the line is drawn by the row it is
 * beside rather than by the list: an index would have the list computing a gap
 * that only a row knows the geometry of.
 */
type Insertion = "above" | "below" | null;

/** What a row needs in order to be carried, or absent for a list that is not. */
interface RowMove {
  readonly ref: (element: HTMLElement | null) => void;
  readonly listeners: Record<string, unknown> | undefined;
  readonly roleDescription: string | undefined;
  readonly describedBy: string | undefined;
  readonly isDragging: boolean;
  readonly insert: Insertion;
}

/**
 * One row of the list.
 *
 * On a rail the row is the same row with its label taken away: the same height,
 * the same selected surface, the same icon in the same place from the leading
 * edge of the column. Folding the column is then a label disappearing rather
 * than one control being swapped for another, and nothing under the pointer
 * moves.
 *
 * The label the row no longer shows is the tooltip it grows, because a rail
 * that cannot be read is a rail nobody can navigate by. It is also still the
 * button's accessible name, which does not change with the width of a column.
 */
function SourceListRow({
  item,
  isActive,
  rail,
  onSelect,
  tabIndex = 0,
  rowRef,
  move,
}: {
  item: SourceListItem;
  isActive: boolean;
  rail?: boolean;
  onSelect: () => void;
  tabIndex?: number;
  rowRef?: (element: HTMLButtonElement | null) => void;
  /** How this row is carried, for a list that can be rearranged. */
  move?: RowMove;
}) {
  const Icon = item.icon;
  const badge = badgeText(item.badge);

  const row = (
    <button
      ref={(element: HTMLButtonElement | null) => {
        rowRef?.(element);
        move?.ref(element);
      }}
      {...move?.listeners}
      // Two of the library's attributes and not the set, for the reason
      // [`SourceTree`] states at length: `useDraggable` also offers
      // `tabIndex={0}`, and taking it would make every row a tab stop and turn
      // one source list into as many as it has sections.
      aria-roledescription={move?.roleDescription}
      aria-describedby={move?.describedBy}
      // The keyboard's way of doing what the pointer does, said rather than
      // left to be found. There is no mode to enter: the row that is selected
      // is the row that moves, and it stays selected after it has.
      aria-keyshortcuts={move === undefined ? undefined : "Alt+ArrowUp Alt+ArrowDown"}
      type="button"
      data-active={isActive}
      // Lifted rather than moved. The row stays where it was and goes quiet,
      // and the line below says where it is going.
      data-dragging={move === undefined ? undefined : move.isDragging}
      aria-current={isActive ? "true" : undefined}
      // A rail row has no text at all, a row with a badge has a number beside
      // its label that a screen reader would read as a second word, and a
      // description is not in the row at all. In each case the whole of what
      // the row says is said here instead — a hover is not a reading.
      aria-label={
        rail || badge !== null || item.note !== undefined
          ? spoken(item, badge)
          : undefined
      }
      tabIndex={tabIndex}
      onClick={onSelect}
      onContextMenu={(event) => {
        const entries = item.menu?.();
        if (!entries) {
          return;
        }
        // Selected only if a native menu is actually going to answer: in a
        // browser during development the system's own menu is left alone
        // rather than suppressed for nothing. The same order [`SourceTree`]
        // keeps — a menu opens over the row it is about, so the row has to be
        // the selected one before it does.
        if (showNativeContextMenu(event, entries)) {
          onSelect();
        }
      }}
      className={cn(
        "relative flex h-(--control-height-lg) w-full items-center gap-2.5 rounded-(--radius-control) text-left text-base text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg data-[active=true]:bg-selected data-[active=true]:font-medium data-[active=true]:text-fg data-[dragging=true]:opacity-50",
        rail ? "justify-center px-0" : "px-2",
      )}
    >
      {/* Where the carried row would land, drawn in the two pixels the rows
          are already spaced by — so nothing shifts to make room for it and the
          line sits exactly on the seam it names. */}
      {move?.insert === undefined || move.insert === null ? null : (
        <span
          aria-hidden
          className={cn(
            "pointer-events-none absolute inset-x-0 h-0.5 rounded-full bg-fg-tertiary",
            move.insert === "above" ? "-top-0.5" : "-bottom-0.5",
          )}
        />
      )}

      {/* On a rail the badge has nowhere of its own to go, so what can go on
          the icon does and what cannot does not. A dot fits, and it is the one
          that must get through: something happened is news whatever width the
          column is. A count does not fit, and it is the one that can be left
          behind — a standing figure is attached to the word it qualifies, and
          folding this column is the words leaving. The tooltip still says it. */}
      <span className="relative shrink-0">
        <Icon className="size-4 opacity-80" />
        {rail && item.badge?.kind === "dot" ? (
          <span
            aria-hidden
            className="absolute -top-0.5 -right-0.5 size-1.5 rounded-full bg-fg-tertiary"
          />
        ) : null}
      </span>
      {rail ? null : (
        <>
          <span className="truncate">{item.label}</span>
          {item.badge === undefined ? null : (
            <span
              aria-hidden
              className="ml-auto shrink-0 text-xs text-fg-tertiary tabular-nums"
            >
              {item.badge.kind === "count" ? (
                badgeText(item.badge)
              ) : (
                <span className="block size-1.5 rounded-full bg-fg-tertiary" />
              )}
            </span>
          )}
        </>
      )}
    </button>
  );

  // Whatever the row has to say that it is not showing. On a rail that is
  // everything, because the labels have left. In a column wide enough to read,
  // it is the description — which is never in the row — and a dot, which is the
  // one mark here that does not explain itself. A number beside its own label
  // explains itself and earns nothing.
  const unsaid = rail
    ? spoken(item, badge)
    : [item.note, item.badge?.kind === "dot" ? badge : null]
        .filter((part): part is string => part !== undefined && part !== null)
        .join(" — ");

  if (unsaid === "") return row;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{row}</TooltipTrigger>
      {/* Held to a readable measure: a description is a sentence, and a
          tooltip as wide as the window is one nobody finishes. */}
      <TooltipContent side="right" className="max-w-[40ch]">
        {unsaid}
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * The same row, wired to be carried.
 *
 * A component of its own rather than a branch inside the list's loop, because
 * `useDraggable` and `useDroppable` are hooks: a list that only called them for
 * some of its rows would call a different number of them whenever a section
 * arrived or left. The row is both — what is picked up and what is aimed at are
 * the same thing in a list you rearrange — and it adds no element, so the shape
 * a screen reader walks is the one the plain row describes.
 */
function MovableRow({
  item,
  isActive,
  rail,
  onSelect,
  tabIndex,
  rowRef,
  insert,
}: {
  item: SourceListItem;
  isActive: boolean;
  rail?: boolean;
  onSelect: () => void;
  tabIndex: number;
  rowRef: (element: HTMLButtonElement | null) => void;
  insert: Insertion;
}) {
  const draggable = useDraggable({ id: item.id });
  const droppable = useDroppable({ id: item.id });

  return (
    <SourceListRow
      item={item}
      isActive={isActive}
      rail={rail}
      onSelect={onSelect}
      tabIndex={tabIndex}
      rowRef={rowRef}
      move={{
        ref: (element) => {
          draggable.setNodeRef(element);
          droppable.setNodeRef(element);
        },
        listeners: draggable.listeners,
        roleDescription: draggable.attributes["aria-roledescription"],
        describedBy: draggable.attributes["aria-describedby"],
        isDragging: draggable.isDragging,
        insert,
      }}
    />
  );
}

/**
 * What a badge says, in the words it is read out as and printed with.
 *
 * A dot has no figure behind it — it is a section saying that something
 * happened and that it cannot put a number on it — so what is said of one is
 * the only thing true of every dot.
 */
function badgeText(badge: SourceListItem["badge"]): string | null {
  if (badge === undefined) return null;
  if (badge.kind === "dot") return "something new";
  return badge.value > BADGE_CEILING ? `${BADGE_CEILING}+` : String(badge.value);
}

/** The whole of what a row says, for the tooltip and for the accessible name. */
function spoken(item: SourceListItem, badge: string | null): string {
  const parts = [item.label, item.note, badge].filter(
    (part): part is string => part !== undefined && part !== null,
  );
  return parts.join(" — ");
}

export function SourceList({
  label,
  items,
  activeId,
  rail,
  onSelect,
  onReorder,
}: {
  /** What this list is, for assistive technology. */
  label: string;
  items: readonly SourceListItem[];
  activeId: string;
  /** The column has been folded to icons: rows lose their labels, not their
   *  place. */
  rail?: boolean;
  onSelect: (id: string) => void;
  /**
   * The rows were put in this order, and the list may be rearranged at all.
   *
   * Absent means the order is not the reader's to decide, which is the honest
   * state of a list of two fixed screens. What is handed over is every id in
   * its new order rather than the one that moved: whoever stores an
   * arrangement stores the whole of it, and a pair of indices would make them
   * re-derive what this list already worked out.
   */
  onReorder?: (ids: readonly string[]) => void;
}) {
  const rows = useRef(new Map<string, HTMLButtonElement>());
  // Which row is being carried and which one it is over. Held here rather than
  // asked of the library per row, because the answer is one row's business and
  // every row has to be told: only the row a drop would land beside draws a
  // line, and it needs the other one's position to know which side to draw on.
  const [carrying, setCarrying] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);

  // A drag has to start deliberately. Without a distance the first press on a
  // row would begin one, and a list whose rows lift when you click them is a
  // list you cannot click. The same four pixels [`MoveArea`] asks for.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  const from = items.findIndex((item) => item.id === carrying);
  const to = items.findIndex((item) => item.id === over);
  // Nothing is drawn while a row is over itself: that drop changes nothing, and
  // a line promising a move that will not happen is the one thing this mark
  // must never do.
  const settling = from >= 0 && to >= 0 && from !== to;

  function insertionAt(index: number): Insertion {
    if (!settling || index !== to) return null;
    return to > from ? "below" : "above";
  }

  function rearrange(fromIndex: number, toIndex: number) {
    onReorder?.(moved(items.map((item) => item.id), fromIndex, toIndex));
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const lastIndex = items.length - 1;
    const currentIndex = items.findIndex((item) => item.id === activeId);

    // The keyboard's way of doing what the pointer does. It is ⌥ and an arrow
    // rather than a mode entered with Space, because this list has already
    // spent Space and the plain arrows: selection follows focus here, so a row
    // is selected by arriving at it, and a gesture that took the arrows away to
    // mean something else would be a list you can rearrange but not walk.
    if (
      onReorder !== undefined &&
      event.altKey &&
      (event.key === "ArrowUp" || event.key === "ArrowDown")
    ) {
      const target = currentIndex + (event.key === "ArrowUp" ? -1 : 1);
      if (currentIndex < 0 || target < 0 || target > lastIndex) return;
      event.preventDefault();
      // The row keeps both its selection and the focus it already had: it is
      // the same element, moved, so nothing has to be given back to it.
      rearrange(currentIndex, target);
      return;
    }

    let nextIndex: number | null = null;
    if (event.key === "ArrowDown")
      nextIndex = Math.min(currentIndex + 1, lastIndex);
    else if (event.key === "ArrowUp") nextIndex = Math.max(currentIndex - 1, 0);
    else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = lastIndex;

    if (nextIndex === null || nextIndex === currentIndex || nextIndex < 0)
      return;

    event.preventDefault();
    const nextId = items[nextIndex].id;
    onSelect(nextId);
    rows.current.get(nextId)?.focus();
  }

  const list = (
    <ScrollArea className="min-h-0 flex-1">
      <nav
        aria-label={label}
        className={cn("pt-2 pb-3", rail ? "px-1.5" : "px-2")}
      >
        <div className="flex flex-col gap-0.5" onKeyDown={handleKeyDown}>
          {items.map((item, index) => {
            const isActive = item.id === activeId;
            // The list is one tab stop, as a native source list is: the
            // arrows move within it once it has focus.
            const tabIndex = isActive ? 0 : -1;
            const rowRef = (element: HTMLButtonElement | null) => {
              if (element) rows.current.set(item.id, element);
              else rows.current.delete(item.id);
            };

            return onReorder === undefined ? (
              <SourceListRow
                key={item.id}
                item={item}
                isActive={isActive}
                rail={rail}
                tabIndex={tabIndex}
                onSelect={() => onSelect(item.id)}
                rowRef={rowRef}
              />
            ) : (
              <MovableRow
                key={item.id}
                item={item}
                isActive={isActive}
                rail={rail}
                tabIndex={tabIndex}
                onSelect={() => onSelect(item.id)}
                rowRef={rowRef}
                insert={insertionAt(index)}
              />
            );
          })}
        </div>
      </nav>
    </ScrollArea>
  );

  if (onReorder === undefined) return list;

  const forget = () => {
    setCarrying(null);
    setOver(null);
  };

  return (
    // Its own region rather than the window's: this drag begins and ends inside
    // one column, and [`MoveArea`] is for the one that crosses them.
    <DndContext
      sensors={sensors}
      // The row nearest the one being carried, rather than the one it overlaps
      // most. Overlap is the wrong question for a list whose rows do not move:
      // it makes the target flicker between two neighbours around the halfway
      // point, and it finds nothing at all when a hand drifts out of a column
      // this narrow — which would be a drag that quietly does nothing.
      collisionDetection={closestCenter}
      onDragStart={({ active }: DragStartEvent) =>
        setCarrying(String(active.id))
      }
      onDragOver={({ over: target }: DragOverEvent) =>
        setOver(target === null ? null : String(target.id))
      }
      onDragEnd={({ active, over: target }: DragEndEvent) => {
        forget();
        if (target === null) return;
        const start = items.findIndex((item) => item.id === String(active.id));
        const end = items.findIndex((item) => item.id === String(target.id));
        if (start < 0 || end < 0 || start === end) return;
        rearrange(start, end);
      }}
      onDragCancel={forget}
    >
      {list}
    </DndContext>
  );
}

/**
 * The ids with one of them moved, which is what an arrangement is.
 *
 * Written here rather than taken from a sorting package: it is four lines, the
 * list is already in hand, and the meaning of `to` — the index the row ends up
 * at, once it has left the one it came from — is the same meaning the line
 * drawn during the drag promised.
 */
function moved(
  ids: readonly string[],
  from: number,
  to: number,
): readonly string[] {
  const next = [...ids];
  const [carried] = next.splice(from, 1);
  next.splice(to, 0, carried);
  return next;
}
