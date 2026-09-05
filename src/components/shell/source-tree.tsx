"use client";

import { useEffect, useMemo, type ReactNode } from "react";
import { ChevronRight, type LucideIcon } from "lucide-react";
import {
  hotkeysCoreFeature,
  syncDataLoaderFeature,
  type ItemInstance,
} from "@headless-tree/core";
import { useTree } from "@headless-tree/react";
import { useDraggable, useDroppable } from "@dnd-kit/core";

import {
  showNativeContextMenu,
  type NativeMenuEntry,
} from "@/lib/native-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

/**
 * A macOS source list with disclosure: the column that answers "where am I"
 * when where you are is nested.
 *
 * The sibling of [`SourceList`](./source-list.tsx) and deliberately its twin:
 * same row height, same selection, same one-tab-stop keyboard. A person moving
 * between the two should not be able to say which control they are in.
 *
 * **Selection follows focus**, which is what a native source list does and what
 * this window's foundation asks for. It is not what the WAI-ARIA tree pattern
 * does by default — there the arrows move focus and selecting is a second
 * keystroke — so the two arrow hotkeys are overridden below rather than left
 * alone. Left and Right collapse and expand, as they do in Finder.
 *
 * ## Why a library, and why you cannot see it from here
 *
 * The behaviour underneath is `@headless-tree/core`: expansion, focus, the
 * hotkeys, and the ARIA a flat-rendered tree needs — `role`, `aria-level`,
 * `aria-setsize`, `aria-posinset` — which is the part hand-written trees get
 * wrong. What it does *not* own is the markup: every element here is this
 * file's, so the row is a `<button>` with the window's own classes and the
 * window's one focus ring.
 *
 * That boundary is the point. Extensions are given this component and never the
 * library, so the library can be replaced without any of them noticing.
 */

/** One row of the tree. */
export interface SourceTreeItem {
  /** Unique within the tree, and what selection and expansion are stated in. */
  readonly id: string;
  readonly label: string;
  readonly icon?: LucideIcon;
  /** The ids of the rows below this one, in the order they are drawn. */
  readonly children?: readonly string[];
  /**
   * A number at the trailing edge, as the type rows carry. Absent draws
   * nothing, which is not the same as `0` — a folder holding no documents of
   * its own says so.
   */
  readonly count?: number;
  /**
   * Drawn quieter, for a row that exists without anything of ours in it — a
   * directory of the working tree no record is filed in. It is real, a person
   * sees it in Finder, and it is somewhere they can file into; it is just not
   * yet somewhere the project keeps anything.
   */
  readonly muted?: boolean;
  /**
   * Drawn in the warning tier, for a row that is waiting on the person reading
   * the column.
   *
   * One bit, and it says *this one wants you*. What it is waiting for belongs
   * to whatever the row is about and goes in [`tooltip`](#tooltip): a column
   * this narrow has room for a colour, not for a sentence.
   *
   * It is a state rather than a quantity, which is why it is not a value of
   * [`count`](#count). A number meaning "how many" at some values and "answer
   * me" at others is a column that has to be read twice.
   *
   * Beside [`muted`](#muted) it wins. A row cannot both be quieter than its
   * neighbours and be the one thing on screen worth answering, and of the two
   * only this one is about the person rather than about the row.
   */
  readonly emphasised?: boolean;
  /**
   * What the secondary button opens. Built when asked for, so the commands act
   * on the row as it stands then rather than as it stood when it was drawn.
   */
  readonly menu?: () => readonly NativeMenuEntry[];
  /**
   * What this row carries when it is dragged, or absent for a row that is not
   * dragged at all.
   *
   * Opaque: this component carries it and never reads it. What a payload means
   * is the caller's, because what a drop *does* is theirs too.
   */
  readonly drag?: unknown;
  /**
   * What this row means as a destination, or absent for a row nothing may be
   * dropped on. Handed to whoever is listening above; this component never
   * reads it.
   */
  readonly drop?: unknown;
  /**
   * What the row says under the pointer, for one that has more to say than
   * fits. A node rather than a string because what belongs there is the
   * caller's: a type shows what it is for and where its documents live, and
   * neither is this component's business to compose.
   */
  readonly tooltip?: ReactNode;
}

export function SourceTree({
  label,
  items,
  rootId,
  activeId,
  expanded,
  onSelect,
  onExpandedChange,
  indent = 14,
}: {
  /** What this tree is, for assistive technology. */
  label: string;
  /**
   * Every row, by id. A map rather than a nested shape because the rows arrive
   * flat — a list of folders is what the engine answers with — and building a
   * nesting here only to have the tree flatten it again would be two shapes to
   * keep in step.
   */
  items: ReadonlyMap<string, SourceTreeItem>;
  /**
   * The row every other row hangs from. It is never drawn: a source list shows
   * its contents, not a row standing for the list itself.
   */
  rootId: string;
  /** The selected row, or `null` while the selection is elsewhere. */
  activeId: string | null;
  /** Which rows are open. Held by the caller, because it outlives this tree:
   * collapsing a folder and coming back to the area should find it collapsed. */
  expanded: readonly string[];
  onSelect: (id: string) => void;
  onExpandedChange: (expanded: readonly string[]) => void;
  /**
   * Pixels of indent per level. Finder's is close to this; the default is set
   * against a 14px row icon so a child's icon starts under its parent's label.
   */
  indent?: number;
}) {
  // Rebuilt only when the rows change. The tree reads these on every render,
  // and a fresh closure each time would make it think the data moved.
  const loader = useMemo(
    () => ({
      getItem: (id: string): SourceTreeItem =>
        items.get(id) ?? { id, label: id },
      getChildren: (id: string): string[] => [
        ...(items.get(id)?.children ?? []),
      ],
    }),
    [items],
  );

  /**
   * Whether anything in this tree discloses at all.
   *
   * One answer for the whole tree, not one per group of siblings. Reserving the
   * triangle's width only where a sibling has children looks right until two
   * levels disagree: a row that drops the reservation moves left by more than a
   * level of indent moves it right, so a child ends up drawn *left of its own
   * parent's siblings* — measured at 6px, and a tree that indents backwards is
   * worse than one that indents for nothing.
   *
   * So it is all or nothing. A list of types with no folders under any of them
   * reserves nothing and is indistinguishable from the flat list it replaced —
   * which is the case that prompted this — and the moment one folder exists,
   * every row makes room and the indent is monotonic again.
   */
  const discloses = useMemo(
    () =>
      [...items.values()].some(
        // The root always has children — they are the list — and it is never
        // drawn, so counting it would reserve a triangle in every tree there
        // has ever been.
        (item) => item.id !== rootId && (item.children?.length ?? 0) > 0,
      ),
    [items, rootId],
  );

  // Held to one identity while the values hold. The library merges this over
  // its own state on every render, so a fresh array each time is a state that
  // never stops changing — which React 19 reports as "too many re-renders"
  // rather than as the slow loop it is.
  const state = useMemo(
    () => ({ expandedItems: [...expanded] }),
    [expanded],
  );

  const tree = useTree<SourceTreeItem>({
    rootItemId: rootId,
    // Expansion is the caller's state, handed in and handed back. Nothing about
    // which folder is open is remembered in here: this component is drawn and
    // thrown away as columns come and go, and state that went with it would be
    // the area forgetting where somebody was.
    //
    // Selection is not given to the library at all. `activeId` is the truth and
    // it lives a long way above this — a row is drawn selected because the area
    // says so, not because a control was clicked — so the selection feature is
    // left out entirely rather than kept in step with a second copy.
    state,
    setExpandedItems: (next) => {
      onExpandedChange(typeof next === "function" ? next([...expanded]) : next);
    },
    getItemName: (item) => item.getItemData().label,
    isItemFolder: (item) => (item.getItemData().children?.length ?? 0) > 0,
    dataLoader: loader,
    indent,
    // Selection follows focus. The library's own arrows move focus and leave
    // selection where it was — correct for a tree of checkboxes, wrong for the
    // control this is: in Mail and in Finder's sidebar the list *is* the
    // selection, and arrowing through it changes what the next column shows.
    hotkeys: {
      focusNextItem: {
        hotkey: "ArrowDown",
        canRepeat: true,
        preventDefault: true,
        handler: (_event, instance) => {
          instance.focusNextItem();
          instance.updateDomFocus();
          select(instance.getFocusedItem(), onSelect);
        },
      },
      focusPreviousItem: {
        hotkey: "ArrowUp",
        canRepeat: true,
        preventDefault: true,
        handler: (_event, instance) => {
          instance.focusPreviousItem();
          instance.updateDomFocus();
          select(instance.getFocusedItem(), onSelect);
        },
      },
    },
    features: [syncDataLoaderFeature, hotkeysCoreFeature],
  });

  // The rows are read through a data loader, and the library caches what that
  // loader answered — it walks the tree once on mount and again when told to.
  // Everything here arrives after mount: the types come from the project's
  // memory and the folders from the working tree, so without this the tree
  // shows what it knew before either answer landed, which is nothing at all.
  useEffect(() => {
    tree.rebuildTree();
  }, [tree, items]);

  return (
    // The label is handed to the library rather than written here: it puts it
    // on the container as `aria-label`, beside the `role` it also owns.
    <div {...tree.getContainerProps(label)} className="flex flex-col gap-0.5">
        {tree.getItems().map((item) => {
          const data = item.getItemData();
          const meta = item.getItemMeta();

          return (
            <TreeRow
              key={item.getId()}
              item={data}
              props={item.getProps()}
              level={meta.level}
              indent={indent}
              reserveDisclosure={discloses}
              isActive={data.id === activeId}
              isFolder={item.isFolder()}
              isExpanded={item.isExpanded()}
              onSelect={() => {
                onSelect(data.id);
                // Clicking the name of a folder opens it, the way clicking a
                // disclosed row in Finder does. The triangle is for closing it
                // again without moving the selection.
                if (item.isFolder() && !item.isExpanded()) item.expand();
              }}
              onToggle={() => {
                if (item.isExpanded()) item.collapse();
                else item.expand();
              }}
            />
          );
        })}
    </div>
  );
}

/**
 * One row, and the only place a hook may be called per row.
 *
 * A component rather than a branch of the loop above, because `useDraggable`
 * and `useDroppable` are hooks and a tree whose rows come and go would call a
 * different number of them on every render. It adds no element of its own: the
 * button *is* the row, so the tree's own children stay `treeitem`s and the
 * shape a screen reader walks is the one the library described.
 */
function TreeRow({
  item,
  props,
  level,
  indent,
  reserveDisclosure,
  isActive,
  isFolder,
  isExpanded,
  onSelect,
  onToggle,
}: {
  item: SourceTreeItem;
  /** What the tree says this row must carry: role, level, focus, `ref`. */
  props: Record<string, unknown>;
  level: number;
  indent: number;
  reserveDisclosure: boolean;
  isActive: boolean;
  isFolder: boolean;
  isExpanded: boolean;
  onSelect: () => void;
  onToggle: () => void;
}) {
  const Icon = item.icon;
  const draggable = useDraggable({
    id: `drag:${item.id}`,
    disabled: item.drag === undefined,
    data: { payload: item.drag },
  });
  const droppable = useDroppable({
    id: `drop:${item.id}`,
    disabled: item.drop === undefined,
    data: { target: item.drop },
  });

  const row = (
    <button
      {...props}
      // After the tree's own props, and every one of these is deliberate.
      // `ref` is two refs — the thing being dragged and the thing dropped on
      // are the same row — and the tree's own registration has to survive both.
      ref={(element: HTMLButtonElement | null) => {
        (props.ref as ((node: HTMLElement | null) => void) | undefined)?.(element);
        draggable.setNodeRef(element);
        droppable.setNodeRef(element);
      }}
      {...draggable.listeners}
      // Two of the library's attributes and not the set. `useDraggable` also
      // offers `role="button"` and `tabIndex={0}`, and spreading those over the
      // tree's own props takes the roving tabindex with them: every row becomes
      // a tab stop, so tabbing through the window walks the whole tree one
      // folder at a time. A source list is one stop and arrows inside it —
      // which is what this control promises, what `SourceList` does, and what
      // the tree pattern asks for. `aria-pressed` goes for the same reason: a
      // `treeitem` is not a toggle.
      aria-roledescription={draggable.attributes["aria-roledescription"]}
      aria-describedby={draggable.attributes["aria-describedby"]}
      type="button"
      role="treeitem"
      data-active={isActive}
      aria-selected={isActive}
      // Said while something is over it, and only for a row that would take it.
      data-drop={droppable.isOver && item.drop !== undefined}
      // Lifted rather than moved: the row stays where it is and goes quiet,
      // because a source list that reflowed under the pointer would be a list
      // whose rows move while you are aiming at one.
      data-dragging={draggable.isDragging}
      style={{ paddingLeft: `${8 + level * indent}px` }}
      onClick={onSelect}
      onContextMenu={(event) => {
        const entries = item.menu?.();
        if (!entries) return;
        // Selected only if a native menu is actually going to answer: in a
        // browser during development the system menu is left alone rather than
        // suppressed for nothing.
        if (showNativeContextMenu(event, entries)) onSelect();
      }}
      className="flex h-(--control-height-lg) w-full items-center gap-2.5 rounded-(--radius-control) pr-2 text-left text-base text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg data-[active=true]:bg-selected data-[active=true]:font-medium data-[active=true]:text-fg data-[drop=true]:bg-selected data-[drop=true]:ring-1 data-[drop=true]:ring-separator-strong data-[dragging=true]:opacity-50"
    >
      {reserveDisclosure ? (
        <span
          aria-hidden="true"
          // A hit target of its own, so closing a folder does not have to mean
          // selecting it. Pulled back against the row's own spacing: a triangle
          // belongs against the thing it opens, not a full gap away from it.
          onClick={(event) => {
            if (!isFolder) return;
            event.stopPropagation();
            onToggle();
          }}
          // The drag starts on the row, and the triangle is not part of it:
          // aiming at a disclosure and getting a drag is the most annoying
          // thing a tree can do.
          onPointerDown={(event) => event.stopPropagation()}
          className="-mr-1 flex size-3.5 shrink-0 items-center justify-center"
        >
          {isFolder ? (
            <ChevronRight
              className="size-3 text-fg-tertiary transition-transform duration-(--motion-duration-fast) ease-shell"
              style={{ transform: isExpanded ? "rotate(90deg)" : undefined }}
            />
          ) : null}
        </span>
      ) : null}

      {Icon ? (
        <Icon
          aria-hidden="true"
          className={
            item.emphasised
              ? "size-3.5 shrink-0 text-warning"
              : item.muted
                ? "size-3.5 shrink-0 text-fg-tertiary opacity-60"
                : "size-3.5 shrink-0 text-fg-tertiary"
          }
        />
      ) : null}

      <span className={item.muted && !item.emphasised ? "truncate text-fg-tertiary" : "truncate"}>
        {item.label}
      </span>

      {item.count === undefined ? null : (
        <span
          className={
            item.emphasised
              ? "ml-auto shrink-0 pl-2 font-mono text-xs font-medium text-warning tabular-nums"
              : "ml-auto shrink-0 pl-2 font-mono text-xs text-fg-tertiary tabular-nums"
          }
        >
          {item.count}
        </span>
      )}
    </button>
  );

  if (!item.tooltip) return row;

  // The trigger is the row itself rather than a wrapper: what a person points
  // at is the row, and a tooltip anchored to the gap beside it would describe
  // something they are not pointing at.
  return (
    <Tooltip>
      <TooltipTrigger asChild>{row}</TooltipTrigger>
      <TooltipContent
        side="right"
        className="max-w-[40ch] flex-col items-start gap-1"
      >
        {item.tooltip}
      </TooltipContent>
    </Tooltip>
  );
}

/** Report a focus move as a selection, for the two hotkeys that carry one. */
function select(
  item: ItemInstance<SourceTreeItem> | undefined,
  onSelect: (id: string) => void,
) {
  const id = item?.getItemData().id;
  if (id !== undefined) onSelect(id);
}
