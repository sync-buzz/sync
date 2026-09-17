"use client";

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { ChevronLeft } from "lucide-react";
import { edgeMask, useScrollAt, useScrollFade } from "@/lib/mobile-scroll";
import { cn } from "@/lib/utils";

/**
 * The columns of a frame, one screen wide each, side by side.
 *
 * On a desk they stand beside each other and a person reads left to right. At
 * 390 points they cannot, so the same order is kept and the screen is moved
 * along it: what lists, then what is shown, then what is true of it. Reading
 * left to right is still reading first to last — that is why the order of the
 * columns was worth keeping, and why this is a pager rather than a stack of
 * pushes. A push says *deeper*; these are not deeper than each other.
 *
 * **It is a scroll container, not a drag.** Everything that makes a swipe feel
 * like the platform's — momentum, the rubber band at the ends, the half-way
 * rule for which page a flick lands on, and above all a horizontal list inside
 * a column keeping a gesture that was meant for it — is the scroller's, for
 * free and correct the first time. A hand-written drag gets the first two
 * right, the third roughly, and the fourth never.
 *
 * ## The threshold
 *
 * Before the first column there is a narrow page that is not a column: pull the
 * first column to the right and it appears, naming what is behind it. Let go
 * past half way and the scroller snaps to it, and whatever it named is what
 * happens.
 *
 * **What is behind is usually the section to the left, not the way out.** The
 * sections are a row a person moves along, so continuing to move along it past
 * the first column of one is how the row is read — the phone arrives at the
 * section before this one. Only where there is no section before this one does
 * the threshold mean leaving the project, and by then it has stopped being a
 * gesture with two meanings: it is the same movement all the way along the row,
 * and the end of the row is the way out.
 *
 * That is a correction. Leaving on every section made the row unusable: a list
 * over-swiped by a few points took the project away, and the one thing a phone
 * cannot do quickly is get back into one.
 *
 * It is narrow on purpose. A full-width page would make the step the same size
 * of gesture as moving between columns. At this width the snap's own half-way
 * rule is the decision: sixty points of travel is a hand that meant it.
 */

/** How wide the page before the first column is, in points. */
const THRESHOLD = 120;

/** How long the scroller has to be still before resting on it counts as leaving. */
const SETTLED_MS = 140;

/** How long a step takes to land, during which nothing else is a step. */
const STEPPING_MS = 450;

/**
 * How long a flick keeps gliding after the finger is gone.
 *
 * Measured rather than chosen: a scroll set inside this window is undone by the
 * end of the glide, and one set after it holds. It is the platform's number,
 * not ours, and it is here because nothing in the DOM will say when a snap has
 * finished settling on every engine this runs on.
 */
const GLIDE_MS = 320;

/**
 * Pushing the pager sideways with a mouse.
 *
 * Deliberately not offered to a finger: a touch device scrolls this container
 * itself, and a hand-written drag on top of that would fight the platform for
 * the same gesture and lose in the interesting cases — momentum and the snap
 * that follows it.
 */
function usePointerDrag(scroller: RefObject<HTMLElement | null>) {
  const from = useRef<{ x: number; left: number } | null>(null);
  // Held as state rather than as a ref because it has to reach the class name,
  // and it has to reach the class name for two reasons.
  //
  // Snapping is the one that is not obvious. A mandatory snap container pulls
  // itself back to the nearest snap point after *every* scroll it is given by
  // hand, so a drag applied a step at a time never leaves the column it
  // started on — the box was being moved twenty-five points and put back
  // twenty-five times. While a mouse is down the snapping is therefore off,
  // and it comes back for the landing, which is asked for as a whole movement.
  //
  // The other is selection: dragging across text selects it, and a swipe that
  // leaves half the column highlighted reads as a broken gesture rather than
  // as a page turn. Neither applies to a finger, which is why this whole hook
  // is a mouse's.
  const [dragging, setDragging] = useState(false);

  const handlers = {
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => {
      if (event.pointerType !== "mouse" || scroller.current === null) return;
      from.current = { x: event.clientX, left: scroller.current.scrollLeft };
      setDragging(true);
      // The gesture belongs to this box until it is let go, even once the
      // cursor has left it. Without this a drag that runs past the edge of the
      // phone is abandoned where it stood — which is exactly where a swipe
      // ends, since a swipe is a movement *off* the screen.
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => {
      const start = from.current;
      const box = scroller.current;
      if (start === null || box === null) return;
      box.scrollTo({ left: start.left - (event.clientX - start.x) });
    },
    onPointerUp: (event: ReactPointerEvent<HTMLElement>) => {
      const start = from.current;
      const box = scroller.current;
      from.current = null;
      setDragging(false);
      if (start === null || box === null) return;
      // Snapping does not follow a scroll position set by hand, so the landing
      // is asked for: the nearest column to where the mouse let go.
      const width = box.clientWidth;
      const page = Math.round(Math.max(0, box.scrollLeft - THRESHOLD) / Math.max(1, width));
      const home = box.scrollLeft < THRESHOLD / 2;
      box.scrollTo({
        left: home ? 0 : THRESHOLD + page * width,
        behavior: "smooth",
      });
      event.preventDefault();
    },
    onPointerCancel: () => {
      from.current = null;
      setDragging(false);
    },
  };

  return { handlers, dragging };
}

export interface Page {
  readonly key: string;
  /**
   * What fills the page.
   *
   * A column, which in the window is an extension's and arrives by portal —
   * so the page gives it a positioned box of a known size and asks nothing
   * else of it. Scrolling is the column's own, everywhere in this shell.
   */
  readonly body: ReactNode;
  /**
   * The column's own controls — its filters, its `new` command.
   *
   * Above the column here, where on a desk they are below it, and the reason is
   * arithmetic rather than taste. The foot of a phone already carries the
   * sections, and the sections already carry the strip the hardware claims for
   * the home indicator. A third band under the column put three horizontal
   * zones between the last row of content and the bottom edge — a fifth of the
   * screen spent on furniture, which is what this looked like on a device.
   *
   * Above, it costs nothing: the top of the screen is otherwise the notch and
   * whatever the column starts with, and a strip of controls there reads as the
   * head of the column it belongs to.
   */
  readonly band?: ReactNode;
}

export function Pager({
  pages,
  behind,
  returning,
  goto,
  onPosition,
  onBehind,
  hasBehind,
}: {
  pages: readonly Page[];
  /**
   * What the threshold names: the section to the left, or the list of projects
   * where this is the first section.
   */
  behind: string;
  /**
   * Changing this puts the pager back on its first column.
   *
   * Which is what choosing a different section means: the columns are that
   * section's, and arriving in the middle of a section you have not opened yet
   * would be the screen keeping a position that belongs to somewhere else.
   */
  returning: string;
  /**
   * A column to move to, asked for by something that is not this pager.
   *
   * Choosing a row in the list is the case that matters: on a desk the
   * workspace is already on the screen and changes under the choice, and here
   * it is a page away, so something has to move. It carries a number that
   * changes with every request because the same column asked for twice is two
   * requests — a bare page number would move once and then look settled.
   */
  goto: { page: number; id: number } | null;
  /**
   * Where the pager is, in pages, as it moves.
   *
   * Reported rather than drawn, because the marks that say it live in the band
   * of sections below — the one place on this screen that is already furniture.
   */
  onPosition?: (at: number) => void;
  /** Reached the threshold: go to whatever `behind` names. */
  onBehind: () => void;
  /**
   * Whether there is anything behind at all.
   *
   * A Mac closes a project by closing its window and has no list to fall back
   * to, so on the first section there the threshold is not drawn — a strip that
   * names something and does nothing when it is reached is worse than no strip.
   */
  hasBehind: boolean;
}) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const at = useScrollAt(scroller, "x");
  const [width, setWidth] = useState(0);
  /**
   * Where the pager was the last time anything read it.
   *
   * What the threshold reacts to is *arriving* at it — a scroller that was on a
   * column and now is not — rather than being at zero, and the difference is
   * everything. A scroll container starts at zero, so a pager that acted on the
   * position alone would step off the moment it was created; and a flag saying
   * *has been on a column* has to be put back afterwards, which is one more
   * thing to get wrong in the frames where a section is changing underneath.
   *
   * A crossing needs neither. The pager is put on a column, this catches up to
   * that, and the next time a finger brings it back to zero the crossing is
   * there to be read — every time, with nothing to reset.
   */
  const was = useRef(0);
  /** Whether a step is still landing, and crossings are therefore not gestures. */
  const stepping = useRef(false);

  // Before paint, and measured rather than assumed: this is drawn in a box on
  // a desk as often as on a phone, and the page width is that box's.
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null) return;
    const measure = () => setWidth(box.clientWidth);
    measure();
    const watching = new ResizeObserver(measure);
    watching.observe(box);
    return () => watching.disconnect();
  }, []);

  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null || width === 0) return;

    const place = () => {
      // Snapping off for the length of the move and back on after it: a
      // mandatory snap container refuses a scroll set in code while it still
      // holds a point of its own, which is the same rule that silently ate the
      // mouse drag.
      const held = box.style.scrollSnapType;
      box.style.scrollSnapType = "none";
      box.scrollTo({ left: THRESHOLD, behavior: was.current > 4 ? "smooth" : "auto" });
      // Read straight back rather than waited for: a jump lands immediately, so
      // the crossing below is measured from where the pager actually is rather
      // than from where a scroll event will say it is a frame later.
      was.current = box.scrollLeft;
      box.style.scrollSnapType = held;
    };

    // Already on a column — the section was chosen from the band — so there is
    // nothing in flight to wait for.
    if (box.scrollLeft > 4) {
      place();
      return;
    }

    // Otherwise this pager is sitting on the threshold because a finger put it
    // there, and the scroller is still gliding to the point that finger chose.
    // A move made during that glide is overwritten by the end of it: the pager
    // went to the column and was carried straight back, which read as a second
    // crossing and stepped the sections twice. So it waits the glide out.
    const settling = setTimeout(place, GLIDE_MS);
    return () => clearTimeout(settling);
  }, [returning, width]);

  // Held in a ref rather than depended on, and this is not a micro-optimisation
  // — it is the difference between stepping once and stepping until the row
  // runs out. The caller writes this handler inline, so it is a new function on
  // every render; an effect that depended on it re-armed its own timer each
  // time the section changed under it, and one swipe walked all the way to the
  // list of projects.
  const behindRef = useRef(onBehind);
  // Written in an effect rather than during the render, which React's own rule
  // asks for: a ref set while rendering is a value that did not come from this
  // render's props, and the linter is right that reading one there is a bug in
  // waiting. Here the handler only has to be current by the time a finger has
  // finished a gesture, and an effect is several frames ahead of that.
  useEffect(() => {
    behindRef.current = onBehind;
  }, [onBehind]);

  // Resting on the threshold is the decision, and *resting* is the word doing
  // the work: the scroller passes through zero on its way to a snap point, and
  // acting on the way past would move somebody who was still moving. So it is
  // asked again once nothing has moved for a moment.
  useEffect(() => {
    const crossed = was.current > 4 && at <= 4;
    was.current = at;
    // One step per gesture. A section changing under the pager moves the
    // scroller twice — off the threshold, and again as the snap finishes — and
    // without this the second of those reads as somebody asking for another
    // step. The window is the time that whole settling takes, not a guess at
    // how fast a hand is.
    if (!crossed || !hasBehind || stepping.current) return;
    const waiting = setTimeout(() => {
      stepping.current = true;
      behindRef.current();
      setTimeout(() => {
        stepping.current = false;
      }, STEPPING_MS);
    }, SETTLED_MS);
    return () => clearTimeout(waiting);
  }, [at, hasBehind]);

  // Dragging with a pointer, which a phone never does and this prototype is
  // looked at with more often than with a finger. A touch screen scrolls a
  // container by pushing it; a mouse has no such gesture, so on a desk the
  // swipe being judged here would be unreachable — and a swipe nobody can try
  // is a swipe nobody can have an opinion about.
  const drag = usePointerDrag(scroller);

  const asked = goto?.id ?? null;
  useEffect(() => {
    const box = scroller.current;
    if (box === null || asked === null || goto === null || width === 0) return;
    box.scrollTo({ left: THRESHOLD + goto.page * width, behavior: "smooth" });
    // `goto` itself is deliberately not a dependency: it is a fresh object on
    // every render of the screen above, and depending on it would scroll the
    // pager back every time anything up there changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asked, width]);

  const position = width === 0 ? 0 : (at - THRESHOLD) / width;

  // In an effect rather than during the render that computed it: what reads
  // this is a component above, and telling a parent something while a child is
  // rendering is the one update React refuses to do.
  useEffect(() => {
    onPosition?.(position);
  }, [onPosition, position]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        ref={scroller}
        className={cn(
          "flex h-full overflow-x-auto overscroll-x-contain",
          drag.dragging ? "select-none" : "snap-x snap-mandatory",
        )}
        {...drag.handlers}
        // No `touch-action` here, and that is a correction rather than an
        // omission. It said `pan-y`, which reads as *this box only moves
        // vertically* — so the browser handed every horizontal gesture
        // straight past the pager and the swipe this whole file exists for did
        // nothing. The default is what was wanted: the inner column takes a
        // vertical drag because it is the scroller under the finger, and this
        // one takes a horizontal drag because nothing inside it wants one.
        style={{ scrollbarWidth: "none" }}
      >
        {hasBehind ? (
        <div
          className="flex shrink-0 snap-start flex-col items-center justify-center gap-2"
          style={{ width: THRESHOLD }}
        >
          <ChevronLeft className="size-5 text-fg-tertiary" />
          {/* Set on its side, which is what a strip this narrow can hold and
              also what it is: a way out standing on edge beside the work,
              rather than a control competing with it. */}
          <span
            className="text-[11px] tracking-[0.18em] text-fg-tertiary uppercase"
            style={{ writingMode: "vertical-rl", rotate: "180deg" }}
          >
            {behind}
          </span>
        </div>
        ) : null}

        {pages.map((page, index) => (
          <PageFrame
            key={page.key}
            page={page}
            // Where this page is relative to the screen: nothing while it is
            // the one being read, one when it is a full screen away.
            from={Math.max(-1.5, Math.min(1.5, position - index))}
          />
        ))}
      </div>


    </div>
  );
}

/**
 * One page of the pager: the column, its controls, and the two edges.
 *
 * A component rather than a branch of the loop above because each page reads
 * its own scroller, and what is read from it is drawn on the page itself.
 */
function PageFrame({ page, from }: { page: Page; from: number }) {
  const body = useRef<HTMLDivElement | null>(null);
  const edges = useScrollFade(body, "y");
  const away = Math.abs(from);
  const mask = edgeMask("y", edges);

  return (
    <div
      className="flex h-full w-full shrink-0 snap-start flex-col overflow-clip"
      style={{
        // The hardware's own space at the top, kept by the pager
        // rather than by the column: a column is a package's, and a
        // package cannot be asked to know about a notch. On a desk
        // this is nothing at all.
        paddingTop: "var(--safe-top)",
        // The page being left behind falls back and dims rather than
        // sliding out flat. Both are read from the scroll position, so
        // they follow the finger exactly and stop when it stops.
        transform: `scale(${1 - away * 0.04})`,
        opacity: 1 - away * 0.5,
      }}
    >
      {page.band}
      <div
        ref={body}
        className="relative min-h-0 flex-1"
        style={{
          // The column holds back a little as its page leaves, so the two are
          // seen to be separate things: a screen sliding out whole is a slide,
          // and a screen whose content lags behind its own edge has a depth to
          // it. A tenth is the most that reads as depth rather than as a
          // column that has come loose from the page it is on.
          transform: `translateX(${from * 10}%)`,
          // Where the column meets the furniture above and below it. Nothing
          // is drawn there — no line, no shadow — so what says the list
          // continues is the list itself going out of light, and only at an
          // end it has something past.
          maskImage: mask,
          WebkitMaskImage: mask,
        }}
      >
        {page.body}
      </div>
    </div>
  );
}

/**
 * The foot of one column.
 *
 * It draws nothing when the column put nothing in it — a rule of the box rather
 * than a second piece of state, because what is in it arrives by portal and
 * this file never sees it.
 *
 * No line under it. A hairline is how the desk separates a band from what it
 * belongs to, and it was the one border left on this screen: everything else
 * here is told apart by depth, and a single rule across the top of a column
 * read as a fragment of another design. What separates them now is the column
 * losing light as it passes beneath — which says the same thing and says it
 * about the content rather than about the furniture.
 */
export function ColumnBand({
  attach,
}: {
  attach: (element: HTMLElement | null) => void;
}) {
  return (
    <div
      ref={attach}
      className="flex min-h-11 shrink-0 items-center gap-1 px-1 not-has-[*]:hidden"
    />
  );
}

/**
 * Which column of how many, drawn as marks rather than named.
 *
 * The name is on the screen already — it is what the column is showing — so a
 * label here would be the window saying the same thing twice. What is not on
 * the screen is how many more there are, and that is the one thing a mark can
 * say at this size.
 *
 * Drawn *into the band of sections* rather than in a strip of its own. Sixteen
 * points of screen for three dots is sixteen points too many, and a row of
 * furniture between the content and the next row of furniture is what made the
 * foot of this phone look like a margin.
 */
export function PageMarks({ count, at }: { count: number; at: number }) {
  if (count < 2) return null;

  return (
    <div className="flex h-4 shrink-0 items-center justify-center gap-1.5">
      {Array.from({ length: count }, (_, index) => {
        const near = Math.max(0, 1 - Math.abs(index - at));
        return (
          <span
            key={index}
            className="h-1 rounded-full bg-fg-tertiary"
            style={{
              // The mark for the page you are on is a line rather than a dot,
              // and it grows out of the dot as the page arrives: at rest there
              // is one shape on the row that is unlike the others, which is
              // read faster than one that is merely brighter.
              width: 4 + near * 14,
              opacity: 0.3 + near * 0.7,
            }}
          />
        );
      })}
    </div>
  );
}
