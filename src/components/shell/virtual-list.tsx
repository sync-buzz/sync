"use client";

import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";

import { ScrollArea } from "@/components/ui/scroll-area";

/**
 * A list long enough that drawing all of it is what costs.
 *
 * Every other list in this window is drawn whole, and that is right for them: a
 * project's sections, a folder's records, the agents a machine has installed
 * are all lists a person could count. This is for the one that has no bound —
 * a conversation that has been going for hours, a log, anything a machine
 * appends to faster than a person reads. Rows are mounted as they come near the
 * viewport and unmounted as they leave it, so what a screen costs is the size
 * of the window onto the list rather than the size of the list.
 *
 * # Why this is the window's rather than each caller's
 *
 * The same reason the component library is. Following a growing list, holding a
 * place while rows above are inserted, and telling a scroll somebody performed
 * from one the code performed are all readings of the same few numbers, and
 * every hand-written version of them gets the last one wrong. The library that
 * owns those numbers reaches for `react-dom` — one function of it — and a
 * package that bundled it would put a second copy of the renderer in the
 * document, which is the one thing an extension may not do. Here there is no
 * second copy: this *is* the window.
 *
 * So what a package is given is a list, not a scroller. It says what the rows
 * are and how to draw one; where they sit, when they are built and when they
 * are thrown away is this file's business, and a caller learns no pixel of it.
 *
 * # What it deliberately does not do
 *
 * It measures nothing itself and states no height. A row is whatever it draws —
 * a paragraph, a picture, a table — and is measured after it is drawn, which is
 * what lets a message grow a frame after the text that made it arrive. A caller
 * that had to declare a height would be guessing at Markdown.
 */
export interface VirtualListHandle {
  /**
   * Puts the end of the list on the screen.
   *
   * `smooth` for a movement somebody asked for and can see the length of;
   * `instant` for one the code performed, which is every other case — a stream
   * followed the way a terminal follows one, where a spring would leave the
   * last line chasing the edge after every chunk.
   */
  readonly toEnd: (animation?: "smooth" | "auto") => void;
}

export interface VirtualListProps<T> {
  /** The rows, oldest first. Prepending to it is what {@link VirtualListProps.onStart} is for. */
  readonly items: readonly T[];
  /**
   * What tells one row from another, across renders and across prepends.
   *
   * Not the index. An index is a fact about the array as it is now, and this
   * list's whole difficulty is that rows arrive at the *front* of it — under a
   * key that is an index, inserting ten rows renames every row below them, and
   * the reader is moved ten rows down the conversation they were reading.
   */
  readonly keyOf: (item: T) => string;
  /** Draws one row. */
  readonly children: (item: T) => ReactNode;
  /** Above the first row: what is earlier, or that there is nothing earlier. */
  readonly header?: ReactNode;
  /** Below the last row, and inside the scrolling, unlike a panel's footer. */
  readonly footer?: ReactNode;
  /**
   * Whether to follow the end of the list as it grows.
   *
   * Followed only for somebody who is *at* the end: a reader who has scrolled
   * up is reading something, and a list that pulled them back to the bottom on
   * every arrival would make reading a conversation impossible while it is
   * still being written. Coming back to the end takes the following back.
   */
  readonly follow?: boolean;
  /** Somebody has scrolled to the top. Where a page of what came before is asked for. */
  readonly onStart?: () => void;
  /** Whether the end is on the screen — what a "jump to the end" control is drawn from. */
  readonly onAtEndChange?: (atEnd: boolean) => void;
  readonly handle?: Ref<VirtualListHandle>;
  readonly className?: string;
  /** Read out to whatever names this list. */
  readonly label?: string;
}

/**
 * How close to the end counts as being at it.
 *
 * Generous rather than exact, and the generosity is the point: somebody who has
 * nudged the scroll by a few pixels while reading the last paragraph has not
 * left the end of the conversation, and a list that decided they had would stop
 * following the answer they are in the middle of reading.
 */
const AT_END_WITHIN_PX = 70;

/**
 * Where the first row sits in a numbering that survives rows arriving in front
 * of it.
 *
 * The library holds a place by an index, so inserting at the front has to move
 * every index down rather than move the reader. That needs somewhere to count
 * down *from*, and this is it — large enough that no list reaches zero, and a
 * number rather than a length so it never has to be recomputed.
 */
const FIRST = 1_000_000;

/**
 * How far past the viewport rows are built, in pixels.
 *
 * Two things at once, and the second is why the number is not smaller. It is
 * the margin that keeps a fast scroll from outrunning what has been drawn — a
 * list built exactly to the edge shows the gap for the frame it takes to fill.
 * And it is *when* the top of the list counts as reached: what asks for the
 * page before this one is the first row being built, so a reader who is a
 * screenful away has already asked, and the page is usually there by the time
 * they arrive. Reaching the top and only then asking is a wait with somebody
 * looking straight at it.
 */
const BEYOND_PX = 900;

/** What is drawn above the first row and below the last, as the library takes it. */
interface Slots {
  readonly header: ReactNode;
  readonly footer: ReactNode;
}

/**
 * The two slots, as components that never change identity.
 *
 * Built here rather than inside the component, and that is not tidiness: the
 * library takes a *component* for each slot, so one written in the render is a
 * new component type every render — and a new type is a remount, which throws
 * away whatever was in the slot and its scroll contribution with it. These read
 * what to draw out of the list's context instead, which changes freely.
 */
const SLOTS = {
  Header: ({ context }: { context?: Slots }) => <>{context?.header}</>,
  Footer: ({ context }: { context?: Slots }) => <>{context?.footer}</>,
};

export function VirtualList<T>({
  items,
  keyOf,
  children,
  header,
  footer,
  follow = false,
  onStart,
  onAtEndChange,
  handle,
  className,
  label,
}: VirtualListProps<T>) {
  const list = useRef<VirtuosoHandle>(null);
  // The node that actually scrolls, held as state rather than read from a ref:
  // there is a first render with no scroller attached at all, and the list is
  // mounted onto it only once there is one.
  const [viewport, setViewport] = useState<HTMLDivElement | null>(null);
  const atEnd = useRef(true);

  const keys = useMemo(() => items.map(keyOf), [items, keyOf]);
  const slots = useMemo<Slots>(() => ({ header, footer }), [header, footer]);

  // Where the numbering starts, moved down by however many rows arrived in
  // front of the row that used to be first. This is the whole of "the screen
  // does not jump": the reader is holding an index, and the rows they are
  // looking at keep the index they had.
  //
  // Worked out during the render that needs it rather than in an effect,
  // because an effect runs after the rows are on the screen — which is one
  // frame with the reader somewhere they did not ask to be, and one frame is
  // what a jump is. React is told during that same render, which it answers by
  // discarding this pass and running it again with the new numbering; nothing
  // is committed in between, so there is no frame at the old one.
  const leads = keys[0] ?? null;
  const [numbering, setNumbering] = useState({ first: FIRST, key: leads });
  if (numbering.key !== leads) {
    const moved = numbering.key === null ? 0 : keys.indexOf(numbering.key);
    setNumbering({
      // Not found at all is a different list rather than a longer one — every
      // row it used to hold is gone — so the numbering starts again.
      first: moved > 0 ? numbering.first - moved : moved === 0 ? numbering.first : FIRST,
      key: leads,
    });
  }

  useImperativeHandle(
    handle,
    () => ({
      toEnd: (animation = "auto") => {
        list.current?.scrollToIndex({ index: "LAST", align: "end", behavior: animation });
      },
    }),
    [],
  );

  // The half a list watching its own content cannot see: the window onto it
  // getting shorter.
  //
  // A panel is a column of fixed height, so every pixel something below the
  // list takes is a pixel the list gives up — and what is below it grows for
  // ordinary reasons a caller never reports. The rows have not changed, so
  // nothing the library observes has fired; the scroll is where it was, and the
  // end has moved below it. Only for a reader who is at the end: somebody who
  // has scrolled up is reading something, and a resize is not them asking to
  // leave it.
  useEffect(() => {
    if (viewport === null || !follow) return;
    const watch = new ResizeObserver(() => {
      if (atEnd.current) {
        list.current?.scrollToIndex({ index: "LAST", align: "end", behavior: "auto" });
      }
    });
    watch.observe(viewport);
    return () => watch.disconnect();
  }, [viewport, follow]);

  const sawEnd = useCallback(
    (reached: boolean) => {
      atEnd.current = reached;
      onAtEndChange?.(reached);
    },
    [onAtEndChange],
  );

  return (
    <ScrollArea viewportRef={setViewport} className={className}>
      {viewport === null ? null : (
        <Virtuoso
          ref={list}
          // The scroller is the window's, not the library's. A list that
          // brought its own would bring its own scrollbar with it, and the one
          // section of the window that scrolled differently from every other is
          // exactly what a shared component library exists to prevent.
          customScrollParent={viewport}
          data={items}
          firstItemIndex={numbering.first}
          computeItemKey={(_index, item) => keyOf(item)}
          itemContent={(_index, item) => children(item)}
          // The end, on arrival. A conversation is read from where it got to,
          // and one that opened at the top would be a list of things somebody
          // has already read.
          initialTopMostItemIndex={Math.max(0, items.length - 1)}
          followOutput={follow ? (reached) => (reached ? "auto" : false) : false}
          atBottomThreshold={AT_END_WITHIN_PX}
          increaseViewportBy={BEYOND_PX}
          atBottomStateChange={sawEnd}
          startReached={onStart}
          components={SLOTS}
          context={slots}
          aria-label={label}
        />
      )}
    </ScrollArea>
  );
}
