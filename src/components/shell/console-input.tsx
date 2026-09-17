"use client";

import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type UIEvent,
} from "react";

import { ConsoleLine } from "@/components/shell/console-line";
import { abbreviate } from "@/lib/console/path";
import { ghost, type Suggestion } from "@/lib/console/resolve";
import { cn } from "@/lib/utils";

/**
 * The line somebody types into.
 *
 * **The seam matters more than what is behind it.** Nothing else in the console
 * knows how this is drawn: the shade, the strip and the canvas see the four
 * members below and nothing else, so replacing a textarea with an editor later
 * is one file rather than a search through the console for the word *textarea*.
 *
 * What crosses it: the value and the three things that change it — typing,
 * submitting, walking the history — plus what the line could become, where it
 * runs, and when to take the caret. What does not cross it is how any of that
 * is drawn.
 *
 * **It is a textarea, deliberately.** Composition, dead keys and a switch of
 * layout mid-word are the system's to handle, and they are handled correctly in
 * a text field and badly in a `contenteditable`. Undo, selection and the caret
 * come with it. The cost is that the typed line cannot be coloured without
 * laying a highlighted layer under a transparent field, and the console is not
 * paying that cost to find out whether it is worth it — what the *result* is
 * coloured in is where colour earns its keep, which is the arrangement bash has
 * had all along.
 *
 * It stands in the same grid as the canvas: its line box is one cell tall and
 * its text starts at the same column a block's result does, so the caret lands
 * on the grid rather than half a cell off it.
 */

/** As many lines as it grows to before it starts scrolling instead. */
const MAX_LINES = 5;

/** How many suggestions are shown at once. */
const VISIBLE = 5;

/**
 * Everything that decides where a character lands.
 *
 * One string, used by the field and by the layer painted under it, because the
 * whole technique rests on the two agreeing exactly: a different padding, a
 * different wrap rule or a different face by a tenth of a pixel and the colour
 * slides off the letters it belongs to. This is the known weakness of painting
 * under a transparent field, and naming the geometry once is what keeps it from
 * drifting on the next edit.
 */
const GEOMETRY =
  "min-w-0 flex-1 resize-none bg-transparent font-mono text-sm break-words whitespace-pre-wrap";

export function ConsoleInput({
  value,
  onChange,
  onSubmit,
  onHistory,
  completions,
  cwd,
  focusOn,
}: {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly onSubmit: () => void;
  /** Walk the history, which is what Up and Down mean at the ends of the line. */
  readonly onHistory: (direction: "older" | "newer") => void;
  /**
   * What the line could become, best first.
   *
   * Worked out by whoever knows what a line means — this draws the list, moves
   * through it and takes what is chosen.
   */
  readonly completions: readonly Suggestion[];
  /** Where a command in this tab would run, drawn at the end of the line. */
  readonly cwd: string;
  /**
   * Take the caret when this changes. The shade owns *when* that is — opening,
   * and switching tabs — because it is the one that knows; this only has to
   * notice that it was asked again.
   */
  readonly focusOn: string | number;
}) {
  const field = useRef<HTMLTextAreaElement>(null);
  const painted = useRef<HTMLDivElement>(null);

  /**
   * Which suggestion the arrows are on, and which list that position is in.
   *
   * The two are kept together and adjusted during the render that brings a new
   * list, rather than corrected afterwards in an effect: a cursor left on
   * position three while the list shrank to two points at something that is no
   * longer offered, and fixing it a frame later means drawing that frame.
   */
  const offered = completions.map((one) => one.token).join("\u0000");
  const [picked, setPicked] = useState({
    offered,
    at: 0,
    from: 0,
    open: false,
  });
  // Adjusted during the render that brings a new list rather than corrected
  // afterwards in an effect: a cursor left on position three while the list
  // shrank to two points at something no longer offered, and fixing it a frame
  // later means drawing that frame. Typing does not close the list — somebody
  // narrowing it is still using it — but it does start again from the top.
  if (picked.offered !== offered) {
    setPicked({ offered, at: 0, from: 0, open: picked.open });
  }

  const at = Math.min(picked.at, Math.max(0, completions.length - 1));
  const from = Math.min(picked.from, Math.max(0, completions.length - VISIBLE));
  const open = picked.open && completions.length > 0;
  const tail = useMemo(
    () => ghost(completions[at]?.token ?? null, value),
    [completions, at, value],
  );

  /**
   * Keeps the colour under the text when the field scrolls.
   *
   * A line that has grown past five is scrolled by the field alone, and a layer
   * that stayed put would colour the wrong lines — which looks like a rendering
   * fault rather than like a scroll.
   */
  const follow = (event: UIEvent<HTMLTextAreaElement>) => {
    const layer = painted.current;
    if (layer !== null) layer.scrollTop = event.currentTarget.scrollTop;
  };

  // Before the browser paints, because the alternative is one frame of a
  // one-line field under a line that is already three long — visible as a jump
  // every time the history is walked onto a multi-line entry.
  useLayoutEffect(() => {
    const element = field.current;
    if (element === null) return;

    // Measured from nothing rather than from the height it currently has: a
    // field never shrinks on its own, so a line deleted from a grown field
    // would leave the gap it used to need.
    element.style.height = "auto";
    const line = Number.parseFloat(
      getComputedStyle(element).lineHeight || "0",
    );
    const ceiling = line > 0 ? line * MAX_LINES : Infinity;
    element.style.height = `${Math.min(element.scrollHeight, ceiling)}px`;
  }, [value]);

  useEffect(() => {
    field.current?.focus();
  }, [focusOn]);

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Mid-composition every key belongs to the input method: Return commits a
    // candidate rather than submitting a line, and a console that took it
    // would be unusable in any language that composes.
    if (event.nativeEvent.isComposing) return;

    const element = event.currentTarget;

    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      onSubmit();
      return;
    }

    // The history is reached from the ends of the line, so a line long enough
    // to wrap can still be walked through with the arrows. Only when the caret
    // has nowhere left to go does Up stop meaning *up*.
    const collapsed = element.selectionStart === element.selectionEnd;

    // Tab shows the list, and Tab again takes what the arrows are on — the way
    // zsh answers the same key. Refused when there is nothing to offer rather
    // than moving the focus out of the console: the next thing to take focus is
    // the tab strip, and a console that jumped there mid-word is a console
    // somebody stops trusting.
    if (event.key === "Tab") {
      event.preventDefault();
      if (completions.length === 0) return;
      if (!open) {
        setPicked({ offered, at: 0, from: 0, open: true });
        return;
      }
      const chosen = completions[at];
      if (chosen !== undefined) {
        // A folder ends in a slash and is a step rather than an answer: the
        // line stays where it is, with the list open on what is inside. Taking
        // anything else finishes the word, so it gets the space.
        const stepping = chosen.token.endsWith("/");
        onChange(stepping ? chosen.token : `${chosen.token} `);
        setPicked({ offered, at: 0, from: 0, open: stepping });
      }
      return;
    }

    // Escape closes the list and stops there. The shade is listening for the
    // same key, and a person dismissing a list of names does not mean to put
    // the console away with it.
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      setPicked({ offered, at: 0, from: 0, open: false });
      return;
    }

    // The arrows belong to the list while it is open, and to the history while
    // it is not. Which one a key means is answered by what is on the screen.
    if (open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setPicked((was) => {
        const next =
          (Math.min(was.at, completions.length - 1) +
            step +
            completions.length) %
          completions.length;
        // The window follows the choice rather than the choice being trapped
        // in the window: walking past either edge scrolls by one, and wrapping
        // round takes the window with it.
        const start = Math.max(0, Math.min(was.from, completions.length - VISIBLE));
        return {
          offered,
          at: next,
          from:
            next < start
              ? next
              : next >= start + VISIBLE
                ? next - VISIBLE + 1
                : start,
          open: true,
        };
      });
      return;
    }

    if (event.key === "ArrowUp" && collapsed && element.selectionStart === 0) {
      event.preventDefault();
      onHistory("older");
      return;
    }

    if (
      event.key === "ArrowDown" &&
      collapsed &&
      element.selectionStart === element.value.length
    ) {
      event.preventDefault();
      onHistory("newer");
    }
  };

  return (
    // No rule above and none below. The line being typed is the last line of
    // the same stream the blocks are in, and a hairline across the shade would
    // make the console two surfaces that happen to be stacked — a transcript,
    // and a box to type into. It is one surface: what was said is above where
    // you are saying the next thing, in the same column, in the same grid.
    <div className="shrink-0">
      <div className="flex items-start px-4 pb-1">
      {/* The gutter, the same two cells the canvas reserves beside a block, so
          that what is being typed stands in the column that what was typed
          landed in. */}
      <span
        aria-hidden
        className="w-[calc(var(--console-cell-w)*2)] shrink-0 font-mono text-sm text-fg-tertiary"
        style={{ lineHeight: "var(--console-cell-h)" }}
      >
        ›
      </span>

      {/* The field and the colour under it, in one box so that they share its
          width. The painted layer cannot be the field — a textarea holds text
          and not spans — so the field is made transparent and laid over the
          layer, with the caret left visible. */}
      <div className="relative min-w-0 flex-1">
        <div
          aria-hidden
          ref={painted}
          className={cn(
            GEOMETRY,
            "pointer-events-none absolute inset-0 overflow-hidden",
          )}
          style={{ lineHeight: "var(--console-cell-h)" }}
        >
          <ConsoleLine line={value} ghost={tail} />
        </div>

        <textarea
          ref={field}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={onKeyDown}
          onScroll={follow}
          rows={1}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          autoComplete="off"
          aria-label="Console input"
          /* No focus ring. In a text field the caret is the indicator, and it
             is the one the platform draws here — a ring around a line that
             holds the caret from the moment the shade opens would be permanent
             chrome that marks nothing. */
          className={cn(
            GEOMETRY,
            "relative w-full overflow-y-auto text-transparent caret-fg outline-none focus-visible:outline-none",
          )}
          style={{ lineHeight: "var(--console-cell-h)" }}
        />
      </div>

        {/* Where this tab would run, at the end of the line rather than over
            the canvas: it is a fact about the line being typed, and the top of
            the shade belongs to what came back. */}
        <span
          className="shrink-0 pl-[calc(var(--console-cell-w)*2)] font-mono text-sm"
          style={{
            lineHeight: "var(--console-cell-h)",
            color: "var(--text-tertiary)",
          }}
        >
          {abbreviate(cwd)}
        </span>
      </div>

      {/* Under the line, in the same grid, and nothing like a card: this is how
          zsh and fish answer the same question, and a popover with a radius and
          a shadow over a console canvas reads as somebody else's window.

          Five at a time. The list is there to be read at a glance while typing,
          and a glance is about five lines — past that it stops being a hint and
          starts being a screen of its own. */}
      {open ? (
        <div
          role="listbox"
          aria-label="Suggestions"
          className="px-4 pb-1 font-mono text-sm"
        >
          {completions.slice(from, from + VISIBLE).map((candidate, index) => (
            <div
              key={candidate.token}
              role="option"
              aria-selected={from + index === at}
              className="flex gap-[var(--console-cell-w)] pl-[calc(var(--console-cell-w)*2)]"
              style={{ lineHeight: "var(--console-cell-h)" }}
            >
              <span
                className="w-[calc(var(--console-cell-w)*16)] shrink-0 truncate"
                style={{
                  color:
                    from + index === at
                      ? "var(--text-primary)"
                      : "var(--ansi-8)",
                }}
              >
                {candidate.token}
              </span>
              <span
                className="min-w-0 flex-1 truncate"
                style={{
                  color:
                    from + index === at
                      ? "var(--text-secondary)"
                      : "var(--text-tertiary)",
                }}
              >
                {candidate.hint}
              </span>
            </div>
          ))}

          {/* What is out of sight, at whichever end it is out of sight at: a
              list that says nothing about it reads as the whole of what there
              is, and somebody stops pressing the arrow. */}
          {completions.length > VISIBLE ? (
            <div
              className="flex justify-between pl-[calc(var(--console-cell-w)*2)] text-fg-tertiary"
              style={{ lineHeight: "var(--console-cell-h)" }}
            >
              <span>{from > 0 ? `${from} above` : ""}</span>
              <span>
                {completions.length - from - VISIBLE > 0
                  ? `${completions.length - from - VISIBLE} below`
                  : ""}
              </span>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
