"use client";

import { useRef, useState } from "react";
import type { ComponentType, PointerEvent, ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * The parts a phone screen is built from.
 *
 * The desktop bands are 34 px high, hold a title and at most one control, and
 * are addressed by role. These are 44 points high because that is the size of a
 * finger, and they carry a control on each side because a phone has nowhere
 * else to put one. They are a second set rather than the first set restyled:
 * the two densities have nothing to say to each other, and a band that tried to
 * be both would be a band that is neither.
 *
 * This began in the prototype beside `src/app/prototype/mobile`, with a comment
 * saying it should never become the shell's chrome. It became it, and the
 * reason the comment is gone rather than argued with is that the argument it
 * was making was won: the arrangement was looked at, agreed, and is now what a
 * phone shows.
 *
 * Colour comes from the same tokens the columns use, so both appearances hold
 * without a single value being invented here. Type does not: the interface
 * scale in `src/app/globals.css` runs 11 px to 15 px, which is desktop density
 * read at arm's length. A phone is read at a foot and tapped rather than
 * clicked, so the sizes below are stated here until the mobile scale exists in
 * the token layer.
 */

/** Characters past which a screen's name gives way to the word `Back`. */
const BACK_LABEL_LIMIT = 14;

/**
 * The band above a screen, which on a phone is where the way back lives.
 *
 * Its height is the touch target plus whatever the hardware claims at the top
 * of the screen — a notch, an island, a status bar. `env()` is nothing in a
 * window on a desktop, so the same expression gives a plain 44 there and the
 * true inset on a device.
 */
export function NavBar({
  title,
  back,
  onBack,
  trailing,
  inset = true,
}: {
  /**
   * What the screen is called. Absent where the screen shows the thing it
   * would be named after, which is what the platform does rather than repeat
   * a word that is already in front of the reader.
   */
  title?: string;
  /** What tapping the leading control returns to. Absent on a root screen. */
  back?: string;
  onBack?: () => void;
  trailing?: ReactNode;
  /**
   * Whether to keep the space the hardware claims at the top of the screen.
   *
   * True for a bar at the top of the screen, which is where the notch is.
   * False for one that is not — a sheet stops short of the top, and a bar
   * inside it that reserved the inset anyway would sit under a band of nothing
   * as tall as an island it is nowhere near.
   */
  inset?: boolean;
}) {
  return (
    <div
      className="shrink-0 border-b border-separator"
      style={
        inset ? { paddingTop: "max(0px, env(safe-area-inset-top))" } : undefined
      }
    >
      <div className="flex h-11 items-center gap-1 px-1">
        <div className="flex min-w-0 flex-1 justify-start">
          {back ? (
            <button
              type="button"
              onClick={onBack}
              className="-ml-1 flex h-11 min-w-11 items-center gap-0.5 rounded-lg pr-2 pl-1 text-[17px] leading-[22px] text-focus active:bg-hover"
            >
              <ChevronLeft className="size-5 shrink-0" strokeWidth={2.25} />
              {/* The name of what you are going back to, and the word `Back`
                  when that name is too long to fit beside a title. That
                  substitution is the platform's, and leaving it out is how a
                  bar ends up with a truncated word on each side and no room
                  for either. */}
              <span className="truncate">
                {back.length > BACK_LABEL_LIMIT ? "Back" : back}
              </span>
            </button>
          ) : null}
        </div>

        {/* Centred and allowed to be cut rather than to push its neighbours:
            the two controls are the only way off this screen, and a long title
            must not be able to take one of them away. */}
        {title ? (
          <h1 className="min-w-0 shrink truncate px-1 text-[17px] leading-[22px] font-semibold">
            {title}
          </h1>
        ) : null}

        <div className="flex min-w-0 flex-1 items-center justify-end gap-1">
          {trailing}
        </div>
      </div>
    </div>
  );
}

/** A control in a navigation bar or a toolbar, sized to a finger. */
export function BarButton({
  label,
  icon: Icon,
  onPress,
  className,
}: {
  /** Read out, and shown when there is no icon to stand for it. */
  label: string;
  icon?: ComponentType<{ className?: string }>;
  onPress?: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      aria-label={Icon ? label : undefined}
      className={cn(
        "flex h-11 min-w-11 items-center justify-center gap-1 rounded-lg px-2 text-[17px] leading-[22px] text-focus active:bg-hover",
        className,
      )}
    >
      {Icon ? <Icon className="size-5" /> : label}
    </button>
  );
}

/**
 * The band at the foot of a root screen, holding what belongs to the window.
 *
 * There are two claims on the foot of a phone screen and only one foot. A
 * column's own strip of controls is already there — filtering a list, adding to
 * it — put there by `useBandSlot` because the platform has one place for
 * controls that act on a list. What is left over is everything that belongs to
 * the *window*: searching the project, the state of its memory, the way to what
 * this phone is. On a Mac all of that lives in the title bar, which costs
 * nothing; a phone has no title bar, and the top corners it was pushed into are
 * the two places on the screen a thumb reaches worst.
 *
 * **So the two never meet: this is drawn only where there is no column.** That
 * is the root of the phone — the list of a computer's projects — and the root
 * of a project, the list of its sections. Both are the window's own screens,
 * neither has an area in it, and their feet are free. One screen deeper the
 * foot is the column's, and the window goes back to speaking from the top bar.
 * Two bands stacked would be two rows of controls with nothing to say which is
 * whose, and the one underneath would be the package's.
 *
 * **It is not navigation.** A tab bar is for two to five places that are always
 * there; sections are brought by packages, the set is open, and it is chosen by
 * the person rather than by us. Nothing here selects a section — what it holds
 * is a search, a state, and one place that is about this phone.
 */
export function WindowBar({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-11 items-center justify-between gap-1 px-1">
      {children}
    </div>
  );
}

/**
 * One screen: a bar, a scroller, and optionally the strip of controls that
 * acts on what the scroller holds.
 *
 * The strip is at the foot rather than at the head, which is where the desktop
 * column keeps the same controls — both for the same reason, that they belong
 * to the list rather than to the window. On a phone the foot is also the only
 * part of a tall screen a thumb reaches.
 */
export function Screen({
  className,
  bar,
  toolbar,
  foot,
  children,
}: {
  className?: string;
  bar: ReactNode;
  /** Controls that act on what the scroller holds, as one band. */
  toolbar?: ReactNode;
  /** Anything else that stays at the foot, drawn as it is given. */
  foot?: ReactNode;
  children: ReactNode;
}) {
  const standing =
    toolbar !== undefined ? (
      <div className="flex h-11 items-center gap-1 px-1">{toolbar}</div>
    ) : (
      foot
    );

  return (
    <div className={cn("flex h-full min-h-0 flex-col", className)}>
      {bar}
      {/* `clip`, not `hidden`: a hidden box is still a scrollport that has lost
          its bar, and the window's own rule about that is stated in full in
          `docs/design-foundation.md` §"Panel roles and priority". It holds
          here for the same reason. */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-clip overscroll-contain">
        {children}
        {standing ? null : <SafeAreaFoot />}
      </div>
      {standing ? (
        // Outside the scroller, or the space the hardware claims would be
        // reachable only by scrolling to it, and the row above it would sit
        // against the foot of the screen looking cut off.
        //
        // The hardware's space is this band's own padding rather than a spacer
        // under it, and that is the difference between a bar and a stump: the
        // bar's surface runs to the bottom edge of the screen and the home
        // indicator sits *on* it, which is what every bar on the system does.
        // A spacer leaves the bar floating a finger's width up, over a strip
        // of something else, and the screen reads as a page that was cut off.
        // One element also means one reserve — two of them, however they came
        // to be, are the same mistake twice as tall.
        <div
          className="shrink-0 border-t border-separator"
          style={{ paddingBottom: "env(safe-area-inset-bottom, 0px)" }}
        >
          {standing}
        </div>
      ) : null}
    </div>
  );
}

/**
 * The space the hardware takes at the foot of the screen, which on a device
 * with a home indicator is where the gesture lives. Nothing may sit in it, and
 * a list that ends exactly at it reads as a list that has been cut off.
 */
export function SafeAreaFoot() {
  return (
    <div
      className="shrink-0"
      // The fallback is stated, and it is not decoration: `env()` with no
      // second argument is a value a browser may not have, and a length it
      // cannot resolve takes the whole declaration with it — leaving a strip
      // of no height at all, which is the one outcome this exists to prevent.
      style={{ height: "max(20px, env(safe-area-inset-bottom, 0px))" }}
    />
  );
}

/**
 * A row in a list, at the height of a finger.
 *
 * The chevron is the phone's whole answer to "does this go somewhere": a
 * desktop row says so by opening a column beside it, and a row that opens a
 * screen has to say so before it is tapped.
 */
export function Row({
  icon: Icon,
  label,
  detail,
  badge,
  leadsOn,
  selected,
  disabled,
  onPress,
}: {
  icon?: ComponentType<{ className?: string }>;
  label: string;
  detail?: string;
  /** A count, or a dot when there is news and no number for it. */
  badge?: number | "dot";
  leadsOn?: boolean;
  selected?: boolean;
  /**
   * The row is here to be read and not to be tapped.
   *
   * Drawn rather than dropped, because the two say different things: a row
   * that is not here says the project does not have this, and a row that does
   * not respond says the project has it and this machine is not where it runs.
   * The system's own way of saying so is weight — the label goes to the
   * secondary tier and nothing about the row moves — so a person's thumb finds
   * the same list on both machines.
   */
  disabled?: boolean;
  onPress?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      disabled={disabled}
      className={cn(
        "flex min-h-11 w-full items-center gap-3 px-4 py-2 text-left",
        // Selection is a surface and a weight, and nothing else — the rule the
        // window keeps, and the one that survives greyscale.
        selected ? "bg-selected font-semibold" : "active:bg-hover",
        disabled && "text-fg-tertiary active:bg-transparent",
      )}
    >
      {Icon ? (
        <Icon
          className={cn(
            "size-5 shrink-0",
            disabled ? "text-fg-tertiary" : "text-fg-secondary",
          )}
        />
      ) : null}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[17px] leading-[22px]">
          {label}
        </span>
        {detail ? (
          <span className="block truncate text-[15px] leading-[20px] text-fg-secondary">
            {detail}
          </span>
        ) : null}
      </span>
      {badge !== undefined ? <Badge value={badge} /> : null}
      {leadsOn ? (
        <ChevronRight className="size-4 shrink-0 text-fg-tertiary" />
      ) : null}
    </button>
  );
}

/**
 * What a row says about its own contents. A figure is how many there are and a
 * dot is that something happened — two claims, never each other, and the
 * window's rule for them is the same at this width. Neither is coloured: a
 * count is information, and colour here is kept for status and destruction.
 */
function Badge({ value }: { value: number | "dot" }) {
  if (value === "dot") {
    return (
      <span
        aria-label="Something new"
        className="size-2 shrink-0 rounded-full bg-fg-tertiary"
      />
    );
  }
  return (
    <span className="shrink-0 text-[15px] leading-[20px] text-fg-tertiary tabular-nums">
      {value > 99 ? "99+" : value}
    </span>
  );
}

/** The hairline between rows, inset to where the text starts. */
export function RowSeparator({ inset = 16 }: { inset?: number }) {
  return (
    <div
      className="h-px bg-separator"
      style={{ marginLeft: `${inset}px` }}
      aria-hidden
    />
  );
}

/**
 * A block of placeholder text.
 *
 * The prototype measures an arrangement, so its content is bars rather than
 * invented sentences: a surface that is not settled is left empty and labelled
 * instead of filled with something plausible, and a screen of convincing prose
 * would be read as a proposal about what the screen is for.
 */
export function TextPlaceholder({ widths }: { widths: readonly number[] }) {
  return (
    <div className="space-y-2" aria-hidden>
      {widths.map((width, index) => (
        <div
          key={index}
          className="h-3 rounded-sm bg-selected"
          style={{ width: `${width}%` }}
        />
      ))}
    </div>
  );
}

/**
 * The numbers a push is made of, taken from the platform rather than chosen.
 *
 * A phone application that invents its own navigation is a phone application
 * that reads as foreign, however good the invention is, so these follow the
 * system: a third of a second, the screen underneath at 30 per cent, an edge
 * gesture that completes on distance *or* on speed. The last of those is the
 * one that is always missed — a fast flick from the edge finishes the way back
 * on iOS even when the finger never reached the middle of the screen, and a
 * gesture that only measures distance feels stuck to anybody used to one that
 * does not.
 */
const EDGE_WIDTH = 24;
const PARALLAX = 30;
const SCRIM = 0.5;
const POP_AT = 0.3;
/** Pixels per millisecond past which letting go is a flick, not a stop. */
const FLICK = 0.45;
/** A third of a second, which is what the system takes to push a screen. */
const PUSH_MS = 350;

/**
 * The screens, and the moving between them.
 *
 * A screen that has been pushed past does not leave: it sits a third of the way
 * off the leading edge under a scrim and a shadow, which is what makes a push
 * read as going *into* something rather than as a slide show. Everything is
 * placed by transform, so two layers move and nothing is laid out again.
 *
 * The drag is the reason this is a prototype rather than a screenshot. It
 * follows the finger, it can be abandoned, it completes on a flick, and the
 * screen underneath moves with it — four things a person can judge in a second
 * and nobody can judge from a description of them.
 */
export function Stack({
  depth,
  onPop,
  children,
}: {
  depth: number;
  onPop: () => void;
  children: readonly ReactNode[];
}) {
  const element = useRef<HTMLDivElement | null>(null);
  /** How far back the standing screen has been dragged, 0 to 1, or not at all. */
  const [progress, setProgress] = useState<number | null>(null);
  const gesture = useRef({ from: 0, at: 0, when: 0, speed: 0 });

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (depth === 0) return;
    const bounds = element.current?.getBoundingClientRect();
    // Only from the leading edge. The rest of the screen belongs to whatever is
    // drawn on it, and a gesture claiming all of it would take the horizontal
    // half of every list and every editor with it.
    if (!bounds || event.clientX - bounds.left > EDGE_WIDTH) return;
    gesture.current = {
      from: event.clientX,
      at: event.clientX,
      when: event.timeStamp,
      speed: 0,
    };
    setProgress(0);
    // The press has been claimed. Without this it also starts a selection, and
    // the drag ends with half the screen highlighted.
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (progress === null) return;
    const width = element.current?.clientWidth ?? 1;
    const elapsed = event.timeStamp - gesture.current.when;
    if (elapsed > 0) {
      gesture.current.speed = (event.clientX - gesture.current.at) / elapsed;
      gesture.current.at = event.clientX;
      gesture.current.when = event.timeStamp;
    }
    setProgress(
      Math.min(1, Math.max(0, (event.clientX - gesture.current.from) / width)),
    );
  };

  const onPointerEnd = () => {
    if (progress === null) return;
    // The screen covers the rest of the distance on its own, at the same speed
    // it would have taken had the way back been tapped.
    if (progress > POP_AT || gesture.current.speed > FLICK) onPop();
    setProgress(null);
  };

  return (
    <div
      ref={element}
      className="relative h-full touch-pan-y"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerEnd}
      onPointerCancel={onPointerEnd}
    >
      {children.map((screen, index) => {
        const standing = index === depth;
        const behind = index < depth;
        const dragging = progress !== null;
        // While a drag is in progress the two screens it involves are placed by
        // hand: the top one follows the finger, the one under it comes back in
        // step. Every other screen keeps the place its index gives it.
        const led = dragging && (standing || index === depth - 1);
        const carried = progress ?? 0;

        return (
          <div
            key={index}
            inert={!standing}
            className={cn(
              "absolute inset-0 overflow-clip",
              // The shadow the system draws down the leading edge of a screen
              // that has been pushed, which is what separates it from the one
              // it is covering while both are moving.
              index > 0 && "shadow-(--shadow-content)",
              !dragging &&
                "transition-transform ease-shell motion-reduce:transition-none",
            )}
            style={{
              transitionDuration: dragging ? undefined : `${PUSH_MS}ms`,
              transform: led
                ? standing
                  ? `translateX(${carried * 100}%)`
                  : `translateX(${-PARALLAX + carried * PARALLAX}%)`
                : standing
                  ? "translateX(0)"
                  : behind
                    ? `translateX(-${PARALLAX}%)`
                    : "translateX(100%)",
            }}
          >
            {screen}
            {/* The screen left behind is dimmed rather than blurred or shrunk:
                it is still the thing you are inside, and it has to read as
                somewhere you can come back to. */}
            <div
              aria-hidden
              className={cn(
                "pointer-events-none absolute inset-0 bg-scrim",
                !dragging &&
                  "transition-opacity ease-shell motion-reduce:transition-none",
              )}
              style={{
                transitionDuration: dragging ? undefined : `${PUSH_MS}ms`,
                opacity: !behind ? 0 : led ? (1 - carried) * SCRIM : SCRIM,
              }}
            />
          </div>
        );
      })}
    </div>
  );
}

/**
 * Where a sheet can rest, as a fraction of its own height rather than of the
 * screen's — the screen is whatever the window gives it, and a fraction of the
 * sheet is the one measure that survives that.
 *
 * Two resting places and not one, because that is what the platform offers and
 * because the inspector is read both ways: a glance at two properties while the
 * subject stays in sight, and a long read where it does not matter. Dragging
 * past the lower one dismisses, which is the third position and the reason the
 * list ends where it does.
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
          "absolute inset-x-0 bottom-0 flex flex-col overflow-clip rounded-t-lg bg-panel shadow-(--shadow-content) ease-shell motion-reduce:transition-none",
          moving ? null : "transition-transform",
        )}
        style={{
          // How far it stops short of the top, and it is a floor rather than a
          // measurement: a fixed inset that cleared the status bar on one
          // phone puts this sheet's own bar under the island on another, and
          // one that cleared the island would leave a hand's width of nothing
          // on a phone that has neither. The ten points past the hardware are
          // what leaves a strip of the screen underneath in sight, which is
          // what says the sheet is over something rather than replacing it.
          top: "max(3.5rem, calc(env(safe-area-inset-top) + 10px))",
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
          <div className="flex h-5 items-center justify-center">
            <div className="h-1 w-9 rounded-full bg-separator-strong" />
          </div>
          <NavBar
            title={title}
            inset={false}
            trailing={<BarButton label="Done" onPress={close} />}
          />
        </div>

        {/* The column itself, and it keeps its own scrolling for the reason
            every column does: the sheet is a place to put it, not a thing that
            reads it. */}
        <div className="relative min-h-0 flex-1">{children}</div>
      </div>
    </>
  );
}
