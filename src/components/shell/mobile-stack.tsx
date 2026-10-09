"use client";

import {
  useCallback,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { ChevronLeft } from "lucide-react";

import { edgeMask, useScrollFade } from "@/lib/mobile-scroll";
import { haptic } from "@/lib/haptic";
import { cn } from "@/lib/utils";

/**
 * The columns of a frame, as screens one in front of the next.
 *
 * On a desk the columns stand side by side and a person reads left to right.
 * At 390 points they cannot stand side by side, and the platform that owns
 * this hardware has already decided what happens then: a split view collapses
 * its columns into a navigation stack. Narrower and deeper are the same thing
 * in a hand, and choosing a row to look at it *is* going deeper — which is why
 * this is a stack of pushes rather than pages side by side in time.
 *
 * **What a push buys that a pager could not.** A back button that is on the
 * screen, named after what it returns to, at every depth; a gesture that is the
 * system's own and starts where the system starts it; and a stack per section,
 * so leaving a record to glance at another section and coming back arrives at
 * the record rather than at the list it was chosen from.
 *
 * **The columns themselves are untouched, and that is the point.** An area
 * draws into the same three slots by the same names; what differs is where the
 * slots are on the screen and how a person moves between them. An extension
 * cannot tell a phone from a Mac.
 */

/** How wide the strip along the left edge that owns the back gesture is. */
const EDGE = 20;

/**
 * How far across the screen a drag has to carry before letting go pops.
 *
 * A third rather than a half, and the difference is what the gesture is for: it
 * is an *accelerator* for a control that is already on the screen, so it may be
 * generous. A half means a hand that has committed has to keep going for a
 * distance it can already see the end of.
 */
const COMMIT = 0.33;

/**
 * How fast a flick has to be to pop regardless of how far it travelled, in
 * points per millisecond.
 *
 * Without this a quick flick from the edge — which is what the gesture usually
 * is once somebody knows it is there — is measured as a short drag and put
 * back, and the screen refuses a movement that was unambiguous to make.
 */
const FLICK = 0.4;

/** How far the screen underneath has already travelled when it is covered. */
const BEHIND = 0.3;

export interface Screen {
  readonly key: string;
  /**
   * What fills the screen.
   *
   * A column, which in the window is an extension's and arrives by portal — so
   * the screen gives it a positioned box of a known size and asks nothing else
   * of it. Scrolling is the column's own, everywhere in this shell.
   */
  readonly body: ReactNode;
  /**
   * What the screen before this one is called, for the back button.
   *
   * The screen's own name is not here and is not wanted: what is in front of a
   * person is what they are looking at, and a bar that said so would be the
   * window naming the thing the thing is already showing. What is *not* on the
   * screen is where leaving goes, which is the whole of what a back button is
   * for.
   */
  readonly behind: string;
  /** The column's own controls — its filters, its `new` command. */
  readonly band?: ReactNode;
  /** One control at the end of this screen's bar, where it has one. */
  readonly trailing?: ReactNode;
  /**
   * What the bar says in the middle, where anything does.
   *
   * Only the first screen of a section has one: it is the project and what its
   * memory is doing, which is true of the whole window rather than of a column.
   * Deeper screens leave it empty, because the column below has its own head
   * and two titles in a row is the window saying one thing twice.
   */
  readonly title?: ReactNode;
}

export function Stack({
  screens,
  depth,
  canPop,
  onPop,
}: {
  screens: readonly Screen[];
  /** Which screen is in front, counted from the first. */
  depth: number;
  /**
   * Whether there is anywhere to go back to from the first screen.
   *
   * A Mac closes a project by closing its window and has no list to fall back
   * to, so at the root of the first section there is nothing behind — and a
   * button that names something and does nothing when it is pressed is worse
   * than no button. Deeper in the stack there always is.
   */
  canPop: boolean;
  /**
   * Go back one. At the first screen this leaves the project, and the caller
   * decides what that means.
   */
  onPop: () => void;
}) {
  const box = useRef<HTMLDivElement | null>(null);
  /**
   * How far the top screen has been dragged out, 0 to 1, while a finger is
   * down.
   *
   * `null` rather than 0 when nothing is being dragged, because the two are
   * different states and only one of them animates: while a finger is down the
   * finger is the animation, and the moment it lifts the rest of the movement
   * is the screen's own.
   */
  const [dragged, setDragged] = useState<number | null>(null);
  const from = useRef<{ x: number; at: number; width: number } | null>(null);

  const pop = useCallback(() => {
    haptic();
    onPop();
  }, [onPop]);

  // The depth the caller holds is the only record of where the stack is; what
  // is held here is the drag, and it is dropped the moment the stack moves —
  // otherwise the screen the pop arrived at would sit a third of the way out
  // with nothing left to pull it back.
  //
  // Read during the render that has the new depth rather than in an effect
  // after it, the way this shell reads every fact that moves the screen: an
  // effect would draw one frame of the old position under a hand that has
  // already let go.
  const [was, setWas] = useState(depth);
  if (was !== depth) {
    setWas(depth);
    setDragged(null);
  }

  const edge = {
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => {
      const width = box.current?.clientWidth ?? 0;
      if (width === 0) return;
      from.current = { x: event.clientX, at: event.timeStamp, width };
      setDragged(0);
      // The gesture belongs to this strip until it is let go, even once the
      // finger has left it — which is immediately, since the strip is twenty
      // points wide and the gesture crosses the screen.
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => {
      const start = from.current;
      if (start === null) return;
      setDragged(
        Math.min(1, Math.max(0, (event.clientX - start.x) / start.width)),
      );
    },
    onPointerUp: (event: ReactPointerEvent<HTMLElement>) => {
      const start = from.current;
      from.current = null;
      if (start === null) return;
      const travelled = event.clientX - start.x;
      const speed = travelled / Math.max(1, event.timeStamp - start.at);
      if (travelled / start.width >= COMMIT || speed >= FLICK) pop();
      else setDragged(null);
    },
    onPointerCancel: () => {
      from.current = null;
      setDragged(null);
    },
  };

  const reachable = depth > 0 || canPop;

  return (
    <div ref={box} className="relative min-h-0 flex-1 overflow-clip">
      {/* Every screen of the frame, including the ones nobody has pushed yet.
          They cost nothing — each is a positioned box a column is portalled
          into, and the columns are mounted by the window regardless — and what
          they buy is both halves of the movement for free: a push is the
          screen at one width to the right coming to nothing, and a pop is the
          same travel back. Mounting only as far as the top screen would mean
          the popped one vanishing on the frame it was let go of. */}
      {screens.map((screen, index) => (
        <ScreenFrame
          key={screen.key}
          screen={screen}
          // Where this screen stands relative to the one being read: nothing
          // while it is in front, one once it is a full screen to the right,
          // and negative once something is covering it.
          shift={index - depth + (dragged ?? 0)}
          settling={dragged === null}
          onBack={index > 0 || canPop ? pop : undefined}
        />
      ))}

      {/* The system's own place for this gesture, and the reason it is a strip
          of its own rather than the whole screen: the column under it belongs
          to a package and scrolls itself, so a gesture read across the whole
          width is a gesture that argues with whatever the package put there.
          Twenty points at the edge argues with nothing, and it is where a hand
          that has used an iPhone already reaches.

          `touch-action: none` because this strip answers the gesture itself.
          Without it the browser gives the movement to whichever box under the
          finger scrolls, and the pop never begins. */}
      {reachable ? (
        <div
          {...edge}
          className="absolute inset-y-0 left-0 z-20 touch-none"
          style={{ width: EDGE }}
        />
      ) : null}
    </div>
  );
}

/**
 * One screen of the stack: its bar, its column, and where it stands.
 *
 * A component rather than a branch of the loop above because each screen reads
 * its own scroller, and what is read from it is drawn on the screen itself.
 */
function ScreenFrame({
  screen,
  shift,
  settling,
  onBack,
}: {
  screen: Screen;
  shift: number;
  settling: boolean;
  /** Absent where this screen has nothing behind it, which draws no button. */
  onBack?: () => void;
}) {
  const body = useRef<HTMLDivElement | null>(null);
  const edges = useScrollFade(body, "y");
  const mask = edgeMask("y", edges);
  const covered = Math.max(0, -shift);

  return (
    <div
      // Nothing but the top screen may be reached, by a finger or by anything
      // reading the window aloud: the others are on the screen to be seen
      // moving, and a control under — or beyond — another screen is a control
      // pressed by accident. The top one stays live through a drag, which
      // costs nothing: the pointer is captured by the strip beside it.
      inert={shift < -0.001 || shift > 0.999}
      className={cn(
        "absolute inset-0 flex flex-col bg-workspace",
        // Only while nothing is being dragged. A finger that is down is the
        // animation, and a transition on top of it is the screen arriving
        // where the finger was a moment ago.
        settling &&
          "transition-[transform,opacity] ease-shell duration-(--motion-duration)",
      )}
      style={{
        // Ahead of the screen it covers by a full width, behind it by a third:
        // what a push looks like everywhere on this platform, and what makes
        // the two read as one in front of the other rather than as two sliding
        // past each other.
        transform: `translateX(${shift > 0 ? shift * 100 : shift * BEHIND * 100}%)`,
        // The covered screen loses light rather than being hidden, so that a
        // gesture half way through shows what it is going back to.
        opacity: 1 - covered * 0.6,
      }}
    >
      <Bar
        behind={screen.behind}
        title={screen.title}
        trailing={screen.trailing}
        onBack={onBack}
      />

      {screen.band}

      <div
        ref={body}
        className="relative min-h-0 flex-1"
        style={{
          // The column holds back a little as its screen leaves, so the two
          // are seen to be separate things: a screen sliding out whole is a
          // slide, and a screen whose content lags behind its own edge has a
          // depth to it.
          transform: `translateX(${shift * 10}%)`,
          // Where the column meets the furniture above and below it. Nothing
          // is drawn there — no line, no shadow — so what says the list
          // continues is the list itself going out of light, and only at an
          // end it has something past.
          maskImage: mask,
          WebkitMaskImage: mask,
        }}
      >
        {screen.body}
      </div>
    </div>
  );
}

/**
 * The bar at the head of a screen.
 *
 * It costs forty-four points and buys the one thing the gesture alone could
 * not: a way back that can be *seen*. A phone whose only way out of a project
 * was a swipe is a phone where somebody who has not been told the swipe exists
 * is stuck — and where somebody who has is still guessing how far to swipe.
 *
 * It is not a title bar. The middle is empty on every screen but the first of
 * a section, where what stands there is the project and what its memory is
 * doing: both true of the window rather than of the column, and both otherwise
 * homeless now that the window has no chrome of its own.
 */
function Bar({
  behind,
  title,
  trailing,
  onBack,
}: {
  behind: string;
  title?: ReactNode;
  trailing?: ReactNode;
  /** Absent where there is nothing behind, which draws no button at all. */
  onBack?: () => void;
}) {
  return (
    <div
      className="relative flex shrink-0 items-center gap-1 px-1"
      style={{ paddingTop: "var(--safe-top)", height: "calc(44px + var(--safe-top))" }}
    >
      {/* Named rather than bare, which is the platform's own rule and the
          reason the gesture became legible at all: a chevron says *back* and
          a chevron with a word says *back to what*. The word is what the
          threshold strip used to carry on its side, where it could only be
          read once the gesture had already started. */}
      {onBack === undefined ? null : (
        <button
          type="button"
          onClick={onBack}
          className="flex h-11 max-w-[45%] shrink-0 items-center gap-0.5 rounded-(--radius-control) pr-2 pl-1 active:opacity-60"
        >
          <ChevronLeft className="size-6 shrink-0 text-fg-secondary" />
          <span className="truncate text-[17px] leading-[22px] text-fg-secondary">
            {behind}
          </span>
        </button>
      )}

      {/* Centred on the bar rather than on what is left of it, so that a long
          back label moves the title instead of shifting it off centre — which
          is what a flex row does and what reads as a bar that cannot make up
          its mind from one screen to the next. */}
      {title === undefined ? null : (
        <div className="pointer-events-none absolute inset-x-0 flex justify-center">
          <div className="pointer-events-auto flex max-w-[55%] min-w-0 flex-col items-center">
            {title}
          </div>
        </div>
      )}

      <div className="min-w-0 flex-1" />
      {trailing}
    </div>
  );
}

/**
 * The foot of one column, drawn under the bar rather than under the list.
 *
 * Above the column here, where on a desk it is below it, and the reason is
 * arithmetic rather than taste. The foot of a phone already carries the
 * sections, and the sections already carry the strip the hardware claims for
 * the home indicator. A third band under the column put three horizontal zones
 * between the last row of content and the bottom edge — a fifth of the screen
 * spent on furniture.
 *
 * It draws nothing when the column put nothing in it — a rule of the box rather
 * than a second piece of state, because what is in it arrives by portal and
 * this file never sees it.
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
