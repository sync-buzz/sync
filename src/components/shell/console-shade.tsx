"use client";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

import { ConsoleCanvas } from "@/components/shell/console-canvas";
import { ConsoleInput } from "@/components/shell/console-input";
import { ConsoleShelf } from "@/components/shell/console-shelf";
import { addressParts } from "@/lib/console/corpus";
import { ConsoleTabs } from "@/components/shell/console-tabs";
import { useCellMetrics } from "@/lib/console/grid";
import {
  EDGE_HEIGHT,
  INPUT_HEIGHT,
  MAX_HEIGHT,
  MIN_CANVAS_LINES,
  STRIP_HEIGHT,
  useShadeHeight,
} from "@/lib/console/height";
import { useConsole } from "@/lib/console/state";
import type { Block } from "@/lib/console/types";

/**
 * The console, as a shade drawn down over the work.
 *
 * **Inside the slab, under the title bar, never over the whole window.** The
 * top of the window is the system's: the traffic lights are real ones and the
 * band around them is a drag region, so a shade that covered them would take
 * away the window's ability to be a window for as long as it was open.
 *
 * **It covers rather than pushes.** Nothing underneath moves. Making room would
 * recompute the whole composition of the window on every call — expensive, and
 * disorienting in the one moment it must not be: the console is asked for
 * precisely while somebody is looking at what is under it.
 *
 * **The only translucent surface inside the slab, and that is the argument for
 * it.** Everywhere else the glass in this window is the edge of it; here it
 * says the work is still there, one layer down, which is what makes this a
 * surface in the window rather than a second window. The material is mixed over
 * the workspace and not over the window's own material — see `--surface-console`
 * in `src/app/globals.css`.
 *
 * The movement is asymmetric on purpose: arriving is worth watching, leaving is
 * not worth waiting for. Both durations are tokens, so somebody who asked for
 * less motion gets none without this file knowing they did.
 */

/** A tab with no blocks, so the canvas is handed the same array every render. */
const NOTHING: readonly Block[] = [];

/**
 * The characters that call the console, for the layouts that do not put a
 * backquote on the key this shortcut lives on.
 *
 * `~` is here because on several layouts the key is unshifted for one of these
 * and shifted for the other, and somebody reaching for the console does not
 * check which.
 */
const CALLS = new Set(["`", "~", "ё", "Ё"]);

/** Whether the caret is somewhere that takes typing on its own account. */
function isField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT"
  );
}

export function ConsoleShade({
  root,
  onOpenRecord,
}: {
  readonly root: string;
  /**
   * Show a record, by the kind that decides which section draws it.
   *
   * The console does not know which area holds which kind and must not learn:
   * that lookup is `opening.ts`, the palette already asks it, and a second
   * answer to *who draws this* would go stale the first time a package changed
   * what it opens.
   */
  readonly onOpenRecord: (kind: string, key: string) => void;
}) {
  const [shown, setShown] = useState(false);
  /** Where the caret was when the shade took it. */
  const cameFrom = useRef<HTMLElement | null>(null);
  const dragging = useRef(false);
  /**
   * A close that arrived mid-drag.
   *
   * The edge holds a pointer capture, and an element removed while it holds one
   * throws `InvalidStateError` — the trap `docs/design-foundation.md` already
   * records against the shell's column edges. So a close asked for during a
   * gesture waits for the gesture to end rather than taking the edge out from
   * under the finger.
   */
  const closeAfterDrag = useRef(false);

  const hide = useCallback(() => {
    if (dragging.current) {
      closeAfterDrag.current = true;
      return;
    }
    setShown(false);
    // Back where it came from, so closing the console returns somebody to the
    // record, the list or the field they called it from.
    cameFrom.current?.focus();
    cameFrom.current = null;
  }, []);

  // Declared above the console because the console is handed it: an address on
  // its own line is a place to go, so the shade closes and whoever draws that
  // kind takes over. The kind leads the address and the key ends it, so the
  // folders in between are read by nobody — they are how a person found it
  // rather than how it is fetched.
  const {
    tabs,
    active,
    running,
    completions,
    open: openTab,
    select,
    selectAt,
    step,
    close,
    rename,
    setDraft,
    submit,
    rerun,
    recall,
  } = useConsole(root, (address) => {
    const at = addressParts(address);
    if (at === null) return;
    hide();
    onOpenRecord(at.kind, at.key);
  });
  const { fraction, set: setFraction, keep: keepFraction } = useShadeHeight();
  const measure = useCellMetrics();

  // Nothing is built until the console has been asked for once. What is
  // installed costing nothing until it is opened is the rule the areas of this
  // window already keep, and a shade is no different.
  const [everShown, setEverShown] = useState(false);
  // Bumped when the caret should return to the input line — opening, and every
  // change of tab. A counter rather than a fact about the console, because
  // *the same tab selected again* is still somebody asking for the line.
  const [focusKey, setFocusKey] = useState(0);

  const shade = useRef<HTMLDivElement>(null);

  const show = useCallback(() => {
    cameFrom.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setEverShown(true);
    setShown(true);
    // A console with no tab has nowhere to type, so the first one is made on
    // the way in rather than by an effect afterwards: an effect would draw one
    // frame of a shade with no line in it.
    if (tabs.length === 0) openTab();
    setFocusKey((key) => key + 1);
  }, [openTab, tabs.length]);

  /**
   * The key that calls it.
   *
   * Both the position and the character, because neither alone covers every
   * layout. `code` is the physical key under the finger, which is what a
   * shortcut in this position is remembered as — and it is the answer for the
   * layouts that keep a backquote there. But a layout is free to put that
   * character somewhere else entirely and to put a letter here instead: the
   * Russian layouts differ from each other on exactly this key, so somebody
   * who changes layout would find the console under their finger in one and
   * gone in the other. Accepting the character as well means the key that
   * *looks* like the console's key always is.
   *
   * `⌘`, `⌃` and `⌥` disqualify it; Shift does not, because on several layouts
   * the character here is only reachable with it.
   *
   * It answers only when the caret is not in a field — otherwise «ё» could not
   * be typed anywhere in the window — and it listens last, standing down where
   * something nearer the caret has already acted. The same courtesy `⌘K`
   * extends in `project-window.tsx`, and for the same reason: two answers to
   * one key is worse than either answer alone.
   */
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.code !== "Backquote" && !CALLS.has(event.key)) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.defaultPrevented) return;

      if (isField(event.target)) return;

      event.preventDefault();
      if (shown) hide();
      else show();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hide, show, shown]);

  // Closing the last tab leaves nothing to type into, which is the state a
  // console nobody has opened is already in. So the shade goes with it, and
  // calling it again starts a fresh tab — rather than standing open over a
  // surface with no line.
  useEffect(() => {
    if (shown && tabs.length === 0) hide();
  }, [hide, shown, tabs.length]);

  const chose = useCallback((act: () => void) => {
    act();
    setFocusKey((key) => key + 1);
  }, []);

  /**
   * Holding the element and measuring the grid, as one callback that keeps its
   * identity.
   *
   * Written inline it would be a new function every render, and React answers a
   * new ref callback by calling the old one with `null` and the new one with
   * the element — so the grid would be measured again on every keystroke, and
   * each measurement forces a layout. Stable, it runs when the shade is built
   * and not again.
   */
  const attach = useCallback(
    (element: HTMLDivElement | null) => {
      shade.current = element;
      measure(element);
    },
    [measure],
  );

  /**
   * The shortcuts the shade answers, and only while the caret is inside it.
   *
   * `⌘W` is not among them. It closes the window, and a tab strip that claimed
   * it would one day close the wrong thing — so a tab closes with `⌘⇧W`, which
   * nothing else here wants.
   *
   * Read from `code` rather than `key` throughout: `⌘⇧[` arrives as `{` on a
   * US layout and as something else again elsewhere, and the bracket keys are
   * where layouts differ first.
   */
  const onShadeKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      hide();
      return;
    }

    if (!event.metaKey) {
      // Typing anywhere in the console types into the line.
      //
      // The shade is one surface, not a transcript with a box under it, so
      // *the console* holding the caret is what matters and not which of its
      // parts holds it. Somebody who clicked a tab and started typing meant the
      // line; the alternative is a keystroke that vanishes and a person who
      // learns to check where the caret is before saying anything.
      //
      // Only printable characters, and never over a field of its own — the
      // rename field on the strip is the one that would otherwise lose every
      // letter to the line below it.
      if (
        event.key.length === 1 &&
        !event.ctrlKey &&
        !event.altKey &&
        !isField(event.target)
      ) {
        event.preventDefault();
        setDraft((active?.draft ?? "") + event.key);
        setFocusKey((key) => key + 1);
      }
      return;
    }

    if (event.code === "KeyT" && !event.shiftKey) {
      event.preventDefault();
      chose(openTab);
      return;
    }

    if (event.code === "KeyW" && event.shiftKey) {
      event.preventDefault();
      if (active !== null) chose(() => close(active.id));
      return;
    }

    if (event.shiftKey && event.code === "BracketLeft") {
      event.preventDefault();
      chose(() => step(-1));
      return;
    }

    if (event.shiftKey && event.code === "BracketRight") {
      event.preventDefault();
      chose(() => step(1));
      return;
    }

    const digit = /^Digit([1-9])$/u.exec(event.code);
    if (digit !== null) {
      event.preventDefault();
      chose(() => selectAt(Number(digit[1]) - 1));
    }
  };

  /**
   * A click on the canvas puts the caret back in the line.
   *
   * On release rather than on press, and only where nothing was selected: the
   * other reason to click a block is to select what it says, and a console that
   * stole the caret mid-drag would drop the selection as it was being made.
   * Controls answer for themselves — the ones that change a tab hand the caret
   * back through `chose`.
   */
  const onShadePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (!shown) return;

    const target = event.target;
    if (
      target instanceof HTMLElement &&
      target.closest("button, input, textarea, [role='separator']") !== null
    ) {
      return;
    }

    const selection = window.getSelection();
    if (selection !== null && !selection.isCollapsed) return;

    setFocusKey((key) => key + 1);
  };

  const onEdgeDown = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragging.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onEdgeMove = (event: PointerEvent<HTMLDivElement>) => {
    const element = shade.current;
    if (!dragging.current || element === null) return;

    const parent = element.parentElement;
    if (parent === null) return;

    const top = element.getBoundingClientRect().top;
    const available = parent.getBoundingClientRect().bottom - top;
    if (available <= 0) return;

    // The floor is in cells rather than in pixels, because what it protects is
    // measured in lines: the strip, the input line, and six lines of canvas.
    // Under that the canvas stops being where a result is read and becomes a
    // slot a result scrolls through.
    const cell =
      Number.parseFloat(
        getComputedStyle(element).getPropertyValue("--console-cell-h"),
      ) || 17.4;
    const floor =
      (STRIP_HEIGHT + INPUT_HEIGHT + cell * MIN_CANVAS_LINES) / available;

    const next = (event.clientY - top) / available;
    setFraction(Math.min(MAX_HEIGHT, Math.max(Math.min(floor, MAX_HEIGHT), next)));
  };

  const onEdgeUp = (event: PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    keepFraction();
    if (closeAfterDrag.current) {
      closeAfterDrag.current = false;
      hide();
    }
  };

  if (!everShown) return null;

  return (
    // The mask. The shade slides out of the top of this box and is clipped by
    // it, so what flies in and out never crosses the title bar — and the box
    // lets every pointer through, so a closed console is not a sheet of glass
    // over the work.
    <div
      aria-hidden={!shown}
      inert={!shown}
      className="pointer-events-none absolute inset-x-0 top-(--header-height) bottom-0 overflow-clip"
    >
      <div
        ref={attach}
        onKeyDown={onShadeKeyDown}
        onPointerUp={onShadePointerUp}
        // Square at the foot, not rounded. The shade spans the slab and stops
        // at a straight line, which reads as a layer over the work rather than
        // as a panel resting on top of it — and a rounded corner here would
        // put two radii a few pixels apart, its own and the slab's, in the one
        // place the eye is most likely to compare them.
        className="pointer-events-auto relative flex flex-col border-b border-separator-strong transition-transform ease-shell"
        style={{
          // Of the mask, which already stands under the title bar — so the
          // share is of the room the shade may actually take rather than of
          // the whole slab.
          height: `${fraction * 100}%`,
          backgroundColor: "var(--surface-console)",
          backdropFilter: "var(--console-backdrop)",
          WebkitBackdropFilter: "var(--console-backdrop)",
          transform: shown ? "translateY(0)" : "translateY(-100%)",
          // Withdrawn while the shade is up out of sight, because a shadow is
          // cast downwards: parked above the mask, the shade is invisible but
          // its shadow still falls into it, and it reads as a permanent smear
          // under the title bar of a window with no console open.
          boxShadow: shown ? "var(--shadow-content)" : "none",
          transitionDuration: shown
            ? "var(--motion-duration)"
            : "var(--motion-duration-fast)",
        }}
      >
        <ConsoleCanvas blocks={active?.blocks ?? NOTHING} onRerun={rerun} />

        <ConsoleShelf running={running} />

        <ConsoleInput
          value={active?.draft ?? ""}
          onChange={setDraft}
          onSubmit={submit}
          onHistory={recall}
          completions={completions}
          cwd={active?.cwd ?? root}
          focusOn={focusKey}
        />

        <ConsoleTabs
          tabs={tabs}
          activeId={active?.id ?? null}
          onSelect={(id) => chose(() => select(id))}
          onClose={(id) => chose(() => close(id))}
          onRename={rename}
          onOpen={() => chose(openTab)}
        />

        {/* The edge. It lies over the last six pixels of the shade — the room
            the tab strip leaves free — so a grab that misses the boundary by a
            pixel still finds it instead of switching a tab. */}
        <div
          onPointerDown={onEdgeDown}
          onPointerMove={onEdgeMove}
          onPointerUp={onEdgeUp}
          onPointerCancel={onEdgeUp}
          role="separator"
          aria-orientation="horizontal"
          aria-label="Console height"
          className="absolute inset-x-0 bottom-0 cursor-ns-resize touch-none"
          style={{ height: `${EDGE_HEIGHT}px` }}
        />
      </div>
    </div>
  );
}
