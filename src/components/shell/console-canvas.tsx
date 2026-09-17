"use client";

import { Fragment, useEffect, useMemo, useRef, type UIEvent } from "react";
import { RotateCcw } from "lucide-react";

import { ConsoleLine } from "@/components/shell/console-line";
import { painted } from "@/lib/console/ansi";
import { lines, type Line, type Piece } from "@/lib/console/markdown";
import { blockDuration, type Block, type Presentation } from "@/lib/console/types";
import { cn } from "@/lib/utils";

/**
 * The canvas: what has been typed into this tab and what came back.
 *
 * A vertical stream of blocks rather than a scrollback of lines, and the
 * difference shows the moment something is longer than the screen — a block is
 * a unit a person can scroll past, keep, or run again, and a line is not.
 *
 * **It rests on the input line and grows upwards.** Two blocks pinned to the
 * top of a tall shade, with the line being typed a screen below them, are two
 * separate things; standing on that line they are one stream, which is how a
 * console in a game reads and why that is the arrangement worth copying. It is
 * the whole of why the canvas holds its own scroller rather than the window's
 * virtual list: that list always scrolls inside a parent it did not make, and
 * bottom alignment in the library behind it only applies to a scroller of its
 * own. A property that silently does nothing is worse than one that is absent.
 *
 * Nothing here is virtualised, and nothing here needs to be: a block appears
 * because somebody pressed Return, so a tab holds as many as one person typed.
 *
 * Blocks are separated by a blank line and never by a rule. A rule turns a
 * stream into a table and invites the eye to read across it; this is read
 * downwards, in the order it happened.
 */

/**
 * How near the bottom still counts as being at it.
 *
 * Generous rather than exact: somebody who nudged the wheel while reading the
 * last block has not left the end, and a canvas that decided they had would
 * stop following the answer they are in the middle of.
 */
const AT_END_WITHIN_PX = 40;

export function ConsoleCanvas({
  blocks,
  onRerun,
}: {
  readonly blocks: readonly Block[];
  readonly onRerun: (typed: string) => void;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  /** Whether the last thing the reader did left them at the end. */
  const atEnd = useRef(true);

  // Following the end, and only for somebody who is at it. A reader who has
  // scrolled up is reading something, and a canvas that pulled them back down
  // on every block would make that impossible.
  useEffect(() => {
    const element = viewport.current;
    if (element === null || !atEnd.current) return;
    element.scrollTop = element.scrollHeight;
  }, [blocks]);

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const { scrollTop, scrollHeight, clientHeight } = event.currentTarget;
    atEnd.current = scrollHeight - scrollTop - clientHeight <= AT_END_WITHIN_PX;
  };

  return (
    // A live region that announces a block, not a byte. Reading out a build's
    // output as it arrives would make the window unusable with a screen reader
    // on, which is the failure this role is most often the cause of.
    <div
      ref={viewport}
      onScroll={onScroll}
      role="log"
      aria-label="Console output"
      className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
    >
      {/* `min-h-full` with the content pushed to the end, rather than the
          scroller itself centring or ending its content: a flex scroller whose
          items are justified to the end clips them at the top once they
          overflow, and what is clipped is exactly the oldest block somebody
          scrolled up to find. */}
      <div className="flex min-h-full flex-col justify-end px-4 pb-1 font-mono text-sm">
        {/* Nothing stands here before the first block. What a console can do
            is said under the line being typed, where the hand is, rather than
            over the canvas, which belongs to what came back. */}
        {blocks.map((block) => (
          <ConsoleBlock key={block.id} block={block} onRerun={onRerun} />
        ))}
      </div>
    </div>
  );
}

/**
 * The gutter, two cells wide, carrying where a block stands.
 *
 * Success draws nothing. It is the ordinary outcome, and a tick beside every
 * line that worked turns the canvas into a checklist of things nobody needed to
 * be told — leaving the eye nothing to catch on when something did fail.
 */
function gutter(block: Block): { glyph: string; tone: string } | null {
  switch (block.state) {
    case "waiting":
      return { glyph: "·", tone: "text-fg-tertiary" };
    case "running":
      return { glyph: "›", tone: "text-fg-secondary" };
    case "failed":
      return { glyph: "✗", tone: "text-danger" };
    case "done":
      return null;
  }
}

function ConsoleBlock({
  block,
  onRerun,
}: {
  readonly block: Block;
  readonly onRerun: (typed: string) => void;
}) {
  const mark = gutter(block);
  const took = blockDuration(block);

  return (
    <div
      className="group/block"
      style={{ lineHeight: "var(--console-cell-h)" }}
    >
      <div className="flex items-start">
        <span
          aria-hidden
          className={cn(
            "w-[calc(var(--console-cell-w)*2)] shrink-0",
            mark?.tone,
          )}
        >
          {mark?.glyph ?? " "}
        </span>

        <span className="min-w-0 flex-1 break-words whitespace-pre-wrap">
          <ConsoleLine line={block.typed} />
        </span>

        {/* Under the pointer, and reserving its room whether or not it shows:
            appearing is a change of opacity rather than of layout, so the line
            the eye is on does not move when the pointer crosses it. */}
        <span className="ml-[calc(var(--console-cell-w)*2)] flex shrink-0 items-center gap-[calc(var(--console-cell-w)*1.5)] opacity-0 transition-opacity duration-(--motion-duration-fast) ease-shell group-hover/block:opacity-100 focus-within:opacity-100">
          {took === null ? null : (
            <span className="text-xs text-fg-tertiary tabular-nums">{took}</span>
          )}
          <button
            type="button"
            onClick={() => onRerun(block.typed)}
            title="Run again"
            className="flex h-[var(--console-cell-h)] items-center rounded-(--radius-control) px-1 text-fg-tertiary hover:text-fg"
          >
            <RotateCcw aria-hidden className="size-3" />
            <span className="sr-only">Run again</span>
          </button>
        </span>
      </div>

      {block.result === null ? null : (
        <div className="pl-[calc(var(--console-cell-w)*2)]">
          <ConsoleResult result={block.result} failed={block.state === "failed"} />
        </div>
      )}

      {/* The blank line between blocks, one cell tall — the same unit
          everything else on the canvas is measured in. */}
      <div aria-hidden className="h-[var(--console-cell-h)]" />
    </div>
  );
}

/**
 * A result, drawn by the name of its view.
 *
 * One view so far, and the switch is written as a switch all the same: the
 * exhaustiveness check is what makes adding a view a compile error at this one
 * place instead of a silently blank block.
 */
function ConsoleResult({
  result,
  failed,
}: {
  readonly result: Presentation;
  readonly failed: boolean;
}) {
  switch (result.view) {
    case "text":
      return (
        <p
          className={cn(
            "break-words whitespace-pre-wrap",
            failed ? "text-danger" : "text-fg-secondary",
          )}
        >
          <Printed text={result.text} />
        </p>
      );
    case "markdown":
      return <Answer text={result.text} />;
    case "stream":
      return (
        <pre
          className={cn(
            "overflow-x-auto break-words whitespace-pre-wrap",
            failed ? "text-danger" : "text-fg-secondary",
          )}
        >
          <Printed text={result.text} />
          {/* The code, and only when it says something. Zero is the ordinary
              end of a command and printing it would be the console reporting
              that nothing happened. */}
          {result.code !== null && result.code !== 0 ? (
            <span className="text-danger">{`\nexit ${result.code}`}</span>
          ) : null}
        </pre>
      );
  }
}

/**
 * What a process printed, in its own colours.
 *
 * Spans rather than markup: what comes back from a process is whatever it
 * chose to send, so it is set as text and never as HTML. The window has no
 * `dangerouslySetInnerHTML` in it and this is exactly the place one would have
 * crept in.
 *
 * A run with nothing on it is drawn as plain text, which keeps the common case
 * — a command that prints no colour at all — one node per block instead of one
 * per word.
 */
function Printed({ text }: { readonly text: string }) {
  const runs = useMemo(() => painted(text), [text]);

  return (
    <>
      {runs.map((run, index) =>
        run.fg === null &&
        run.bg === null &&
        !run.bold &&
        !run.dim &&
        !run.italic &&
        !run.underline &&
        !run.struck ? (
          <Fragment key={index}>{run.text}</Fragment>
        ) : (
          <span
            key={index}
            style={{
              color: run.fg ?? undefined,
              backgroundColor: run.bg ?? undefined,
              fontWeight: run.bold ? "bold" : undefined,
              opacity: run.dim ? 0.6 : undefined,
              fontStyle: run.italic ? "italic" : undefined,
              textDecoration: [
                run.underline ? "underline" : null,
                run.struck ? "line-through" : null,
              ]
                .filter(Boolean)
                .join(" ") || undefined,
            }}
          >
            {run.text}
          </span>
        ),
      )}
    </>
  );
}

/**
 * An agent's answer, set so it can be read.
 *
 * Everything is in the same monospaced grid the rest of the canvas is in —
 * a heading is not a larger face, it is a brighter one, because a line of
 * twenty-pixel text in a console is a line that belongs to a different
 * program. What separates the parts is weight, colour and the gutter, and all
 * three come from the tokens the window already has.
 */
function Answer({ text }: { readonly text: string }) {
  const read = useMemo(() => lines(text), [text]);

  return (
    <div className="text-fg-secondary">
      {read.map((line, index) => (
        <AnswerLine key={index} line={line} />
      ))}
    </div>
  );
}

function AnswerLine({ line }: { readonly line: Line }) {
  switch (line.kind) {
    case "blank":
      return <div aria-hidden className="h-[var(--console-cell-h)]" />;

    case "rule":
      // One cell tall, like every other line, with the rule drawn through the
      // middle of it: a separator that took its own height would break the
      // grid the whole canvas is measured in.
      return (
        <div
          aria-hidden
          className="flex items-center"
          style={{ height: "var(--console-cell-h)" }}
        >
          <span className="h-px w-full bg-separator" />
        </div>
      );

    case "code":
      return (
        <pre className="overflow-x-auto rounded-(--radius-control) bg-hover px-[var(--console-cell-w)] text-fg">
          <code>{line.text}</code>
        </pre>
      );

    case "heading":
      return (
        <div
          className={cn(
            "break-words",
            line.level === 1 ? "font-semibold text-fg" : "text-fg",
          )}
        >
          <Pieces pieces={line.pieces} />
        </div>
      );

    case "quote":
      return (
        <div className="flex items-start">
          <span
            aria-hidden
            className="w-[calc(var(--console-cell-w)*2)] shrink-0 text-fg-tertiary"
          >
            ▍
          </span>
          <span className="min-w-0 flex-1 break-words text-fg-tertiary">
            <Pieces pieces={line.pieces} />
          </span>
        </div>
      );

    case "bullet":
      return (
        <div
          className="flex items-start"
          style={{
            paddingLeft: `calc(var(--console-cell-w) * ${line.depth * 2})`,
          }}
        >
          <span
            aria-hidden
            className="w-[calc(var(--console-cell-w)*2)] shrink-0 text-fg-tertiary"
          >
            {line.marker}
          </span>
          <span className="min-w-0 flex-1 break-words">
            <Pieces pieces={line.pieces} />
          </span>
        </div>
      );

    case "text":
      return (
        <div className="break-words">
          <Pieces pieces={line.pieces} />
        </div>
      );
  }
}

function Pieces({ pieces }: { readonly pieces: readonly Piece[] }) {
  return (
    <>
      {pieces.map((piece, index) => (
        <span
          key={index}
          className={cn(
            piece.code &&
              "rounded-(--radius-control) bg-hover px-[calc(var(--console-cell-w)*0.5)] text-fg",
            piece.strong && "font-semibold text-fg",
            piece.emphasis && "italic",
          )}
        >
          {piece.text}
        </span>
      ))}
    </>
  );
}
