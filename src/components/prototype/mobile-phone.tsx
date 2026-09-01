"use client";

import { useCallback, useRef, useState } from "react";
import type { PointerEvent, ReactNode } from "react";
import { Info, ListFilter, Plus, Search, SlidersHorizontal } from "lucide-react";
import type { FrameId } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";
import { BarButton, NavBar, Screen } from "@/components/prototype/mobile-chrome";
import {
  ITEMS,
  InspectorBody,
  ItemRows,
  PinnedRow,
  SectionRows,
  labelOfSection,
  WorkspaceBody,
} from "@/components/prototype/mobile-content";
import {
  hasInspector,
  levelsOf,
  type InspectorPresentation,
  type MobileLevel,
} from "@/components/prototype/mobile-geometry";

/**
 * The window, at 390 points.
 *
 * The columns a frame declares are drawn one at a time and reached in order, so
 * what was a row of panels becomes a stack of screens. What has to be judged by
 * eye is the moving between them, which is why the push is a real one — it can
 * be dragged back from the leading edge and abandoned halfway, the way it can
 * everywhere else on the platform. A prototype whose transitions only ever
 * complete would be answering an easier question than the one that was asked.
 *
 * The state below is one screen deep on purpose. Which section, which item and
 * how far in is the whole of it; nothing is fetched, nothing is remembered, and
 * a screen exists only while it is on the stack. The window has good reasons to
 * keep an area mounted for ever, and none of them apply to a drawing.
 */
export function MobilePhone({
  frame,
  inspector,
}: {
  frame: FrameId;
  inspector: InspectorPresentation;
}) {
  const levels = levelsOf(frame, inspector);

  const [reached, setReached] = useState(0);
  // Clamped rather than reset: a frame changed under a stack deeper than the
  // new frame goes must not leave the phone on a screen that frame does not
  // have, and returning to the root on every change would make the frames
  // above the device harder to compare with each other.
  const depth = Math.min(reached, levels.length - 1);

  const [section, setSection] = useState<string | null>(null);
  const [item, setItem] = useState<number | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);

  const push = useCallback(() => setReached(depth + 1), [depth]);
  const pop = useCallback(() => setReached(Math.max(0, depth - 1)), [depth]);

  const sectionLabel = labelOfSection(section);
  const itemLabel = ITEMS[item ?? 0] ?? ITEMS[0];

  /** What a screen is called, which is also what the way back to it is called. */
  const titleOf = (level: MobileLevel): string => {
    switch (level) {
      case "sections":
        return "Project";
      case "navigator":
        return sectionLabel;
      case "workspace":
        return levels.includes("navigator") ? itemLabel : sectionLabel;
      case "inspector":
        return "Properties";
    }
  };

  const inspectorControl = !hasInspector(frame) ? null : (
    <BarButton
      label="Properties"
      icon={Info}
      onPress={inspector === "sheet" ? () => setSheetOpen(true) : push}
    />
  );

  const screenOf = (level: MobileLevel, index: number) => {
    const bar = (
      <NavBar
        title={titleOf(level)}
        back={index > 0 ? titleOf(levels[index - 1]) : undefined}
        onBack={pop}
        trailing={
          level === "sections" ? (
            <BarButton label="Search" icon={Search} />
          ) : level === "workspace" ? (
            inspectorControl
          ) : null
        }
      />
    );

    switch (level) {
      case "sections":
        return (
          <Screen
            className="bg-sidebar"
            bar={bar}
            foot={
              <PinnedRow
                onOpen={(key) => {
                  setSection(key);
                  push();
                }}
              />
            }
          >
            <SectionRows
              activeKey={section}
              onOpen={(key) => {
                setSection(key);
                push();
              }}
            />
          </Screen>
        );
      case "navigator":
        return (
          <Screen
            className="bg-panel"
            bar={bar}
            // The division the desktop column keeps at its foot: what acts on
            // the list on the leading edge, what decides how it is shown on the
            // trailing one, and nothing here writing what the list contains.
            toolbar={
              <>
                <BarButton label="Add" icon={Plus} />
                <BarButton label="Arrange" icon={SlidersHorizontal} />
                <BarButton label="Filter" icon={ListFilter} className="ml-auto" />
              </>
            }
          >
            <ItemRows
              activeIndex={item}
              onOpen={(index) => {
                setItem(index);
                push();
              }}
            />
          </Screen>
        );
      case "workspace":
        return (
          <Screen className="bg-workspace" bar={bar}>
            <WorkspaceBody title={titleOf("workspace")} />
          </Screen>
        );
      case "inspector":
        return (
          <Screen className="bg-panel" bar={bar}>
            <InspectorBody />
          </Screen>
        );
    }
  };

  return (
    <div className="relative h-full overflow-clip bg-workspace text-fg">
      {/* The screen a sheet is raised over shrinks back and takes its corners
          with it, so the two read as a stack seen edge on. Without it a sheet
          is a panel that appeared, and the screen under it looks merely
          covered rather than set down. */}
      <div
        className={cn(
          "h-full origin-top overflow-clip transition-[transform,border-radius] duration-[400ms] ease-shell motion-reduce:transition-none",
          sheetOpen && "translate-y-1 scale-[0.94] rounded-lg",
        )}
      >
        <Stack depth={depth} onPop={pop}>
          {levels.map(screenOf)}
        </Stack>
      </div>

      {hasInspector(frame) && inspector === "sheet" ? (
        <InspectorSheet open={sheetOpen} onClose={() => setSheetOpen(false)} />
      ) : null}
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
function Stack({
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
const DISMISSED = 1;
/** How long the system takes to raise or drop a sheet. */
const SHEET_MS = 400;
/** How far a flick is taken to carry the sheet on past where it was let go. */
const CARRY_MS = 140;

/**
 * The inspector raised over the workspace instead of pushed after it.
 *
 * Everything about it is the platform's: it stops short of the top so the
 * screen it describes stays in sight, that screen shrinks back and takes its
 * corners with it so the two read as a stack rather than as one thing over
 * another, the grabber says the sheet can be moved before anybody tries, and a
 * drag down past the lower rest dismisses it.
 */
function InspectorSheet({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const element = useRef<HTMLDivElement | null>(null);
  const [detent, setDetent] = useState<number>(DETENTS.medium);
  /** Where the sheet is while a finger is on it, in fractions of its height. */
  const [held, setHeld] = useState<number | null>(null);
  const gesture = useRef({ from: 0, at: 0, when: 0, speed: 0, detent: 0 });

  const at = held ?? (open ? detent : DISMISSED);

  const close = () => {
    // The next raise starts where the system starts one: at the lower rest,
    // not wherever this reader happened to leave it.
    setDetent(DETENTS.medium);
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
          "absolute inset-x-0 top-14 bottom-0 flex flex-col overflow-clip rounded-t-lg bg-panel shadow-(--shadow-content) ease-shell motion-reduce:transition-none",
          moving ? null : "transition-transform",
        )}
        style={{
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
            title="Properties"
            trailing={<BarButton label="Done" onPress={close} />}
          />
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
          <InspectorBody />
        </div>
      </div>
    </>
  );
}
