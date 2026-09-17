"use client";

import { useRef, useState } from "react";
import type { PointerEvent, ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * The one thing a phone still raises over the screen rather than putting beside
 * it.
 *
 * The columns of a frame are pages of a pager now, and the way out of a project
 * is a threshold at the edge of it — so nothing about a *section* is a sheet
 * any more. What is left is the phone's own settings: not part of any project,
 * reached from inside one, and read instead of what is underneath rather than
 * about it. That is what a sheet is for.
 *
 * Everything about how it behaves is the platform's: it stops short of the top
 * so the screen it was raised from stays in sight, the grabber says it can be
 * moved before anybody tries, a flick carries it on past where it was let go,
 * and a drag down past the lower rest dismisses it.
 */

const DETENTS = { large: 0, medium: 0.45 } as const;
/** Which of them a sheet arrives at, by the name its caller uses. */
export type Detent = keyof typeof DETENTS;
const DISMISSED = 1;
/** How long the system takes to raise or drop a sheet. */
const SHEET_MS = 400;
/** How far a flick is taken to carry the sheet on past where it was let go. */
const CARRY_MS = 140;

/**
 * A column raised over the screen instead of pushed after it.
 *
 * Everything about it is the platform's: it stops short of the top so the
 * screen it describes stays in sight, that screen shrinks back and takes its
 * corners with it so the two read as a stack rather than as one thing over
 * another, the grabber says the sheet can be moved before anybody tries, and a
 * drag down past the lower rest dismisses it.
 *
 * The inspector is what it holds, and that is a decision rather than a shape:
 * pushed after the workspace it would take the subject off the screen, and the
 * inspector is read *about* something. A sheet keeps a strip of that something
 * in sight and gives the rest to what is said about it.
 */
export function Sheet({
  open,
  title,
  rest = "medium",
  onClose,
  children,
}: {
  open: boolean;
  /** What the sheet is called, in its own bar. */
  title: string;
  /**
   * Where it arrives, and where it goes back to for the next reader.
   *
   * The lower rest is right for something read *about* what is underneath, and
   * wrong for something read instead of it: a sheet that is its own subject
   * arrives with as much of itself in sight as the platform allows. Both are
   * still reachable by dragging — this decides the first sight, not the range.
   */
  rest?: Detent;
  onClose: () => void;
  children: ReactNode;
}) {
  const element = useRef<HTMLDivElement | null>(null);
  const [detent, setDetent] = useState<number>(DETENTS[rest]);
  /** Where the sheet is while a finger is on it, in fractions of its height. */
  const [held, setHeld] = useState<number | null>(null);
  const gesture = useRef({ from: 0, at: 0, when: 0, speed: 0, detent: 0 });

  const at = held ?? (open ? detent : DISMISSED);

  const close = () => {
    // The next raise starts where this sheet starts, not wherever this reader
    // happened to leave it.
    setDetent(DETENTS[rest]);
    onClose();
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    gesture.current = {
      from: event.clientY,
      at: event.clientY,
      when: event.timeStamp,
      speed: 0,
      detent,
    };
    setHeld(detent);
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (held === null) return;
    const height = element.current?.clientHeight ?? 1;
    const elapsed = event.timeStamp - gesture.current.when;
    if (elapsed > 0) {
      gesture.current.speed = (event.clientY - gesture.current.at) / elapsed;
      gesture.current.at = event.clientY;
      gesture.current.when = event.timeStamp;
    }
    const moved =
      gesture.current.detent + (event.clientY - gesture.current.from) / height;
    // Above the top rest the sheet resists rather than stops: a hard stop reads
    // as something broken, and the platform gives way there.
    setHeld(moved < DETENTS.large ? moved * 0.3 : Math.min(DISMISSED, moved));
  };

  const onPointerEnd = () => {
    if (held === null) return;
    const height = element.current?.clientHeight ?? 1;
    // Where it would come to rest if it kept going, which is what the system
    // decides on rather than where the finger happened to stop.
    const projected = held + (gesture.current.speed / height) * CARRY_MS;
    const nearest = [DETENTS.large, DETENTS.medium, DISMISSED].reduce((a, b) =>
      Math.abs(b - projected) < Math.abs(a - projected) ? b : a,
    );
    setHeld(null);
    if (nearest === DISMISSED) close();
    else setDetent(nearest);
  };

  const moving = held !== null;

  return (
    <>
      <div
        onClick={close}
        className={cn(
          "absolute inset-0 bg-scrim ease-shell motion-reduce:transition-none",
          moving ? null : "transition-opacity",
          open ? "opacity-100" : "pointer-events-none opacity-0",
        )}
        style={{ transitionDuration: moving ? undefined : `${SHEET_MS}ms` }}
      />
      <div
        ref={element}
        inert={!open}
        className={cn(
          "absolute inset-x-0 bottom-0 flex flex-col overflow-clip rounded-t-[32px] ease-shell motion-reduce:transition-none",
          moving ? null : "transition-transform",
        )}
        style={{
          background: "var(--phone-shade)",
          backdropFilter: "blur(24px) saturate(140%)",
          boxShadow: "var(--phone-shade-lift)",
          // How far it stops short of the top, and it is a floor rather than a
          // measurement: a fixed inset that cleared the status bar on one
          // phone puts this sheet's own bar under the island on another, and
          // one that cleared the island would leave a hand's width of nothing
          // on a phone that has neither. The ten points past the hardware are
          // what leaves a strip of the screen underneath in sight, which is
          // what says the sheet is over something rather than replacing it.
          top: "max(3.5rem, calc(var(--safe-top) + 10px))",
          transitionDuration: moving ? undefined : `${SHEET_MS}ms`,
          transform: `translateY(${at * 100}%)`,
        }}
      >
        {/* The grabber and the bar under it are the handle. The list below is
            not: a sheet that moved when a list was scrolled would be a sheet
            nobody could read. */}
        <div
          className="shrink-0 touch-none select-none"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerEnd}
        >
          <div className="flex h-6 items-center justify-center">
            <div className="h-1 w-10 rounded-full bg-fg-tertiary" />
          </div>
          {/* The name on the left and the way out on the right, at the size
              everything else on a phone is read at. It was a centred title bar
              borrowed from the desk; what that bar is for is a stack of pushes,
              and this sheet is not in one. */}
          <div className="flex items-center gap-2 px-5 pt-1 pb-4">
            <h2 className="min-w-0 flex-1 truncate text-[22px] leading-[28px] font-semibold">
              {title}
            </h2>
            <button
              type="button"
              onClick={close}
              className="-mr-2 flex h-11 shrink-0 items-center rounded-(--radius-control) px-2 text-[17px] leading-[22px] text-focus active:bg-hover"
            >
              Done
            </button>
          </div>
        </div>

        {/* The column itself, and it keeps its own scrolling for the reason
            every column does: the sheet is a place to put it, not a thing that
            reads it. */}
        <div className="relative min-h-0 flex-1">{children}</div>
      </div>
    </>
  );
}
