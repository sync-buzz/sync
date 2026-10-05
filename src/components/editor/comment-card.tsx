"use client";

/**
 * One comment, beside the passage it is about.
 *
 * It is a card over the text rather than a column in the margin, and that is
 * arithmetic rather than taste: the page is set to `--prose-measure` (680px) and
 * the workspace may be as narrow as `WORKSPACE_MIN_WIDTH` (500px), so at the
 * widths this window actually gets there is no margin to put anything in. The
 * card sits level with the passage against the page's trailing edge; the place
 * every comment of a record is read at once is the context column, which has
 * width of its own.
 *
 * **It opens as what it says, not as a field.** A comment is read far more often
 * than it is written, and a card that opened with a caret in a textarea invited
 * an edit nobody asked for — click the text to change it. A comment being
 * written is the other way round, and starts in the field.
 *
 * Two ways out, and they are different things. The close button and `Esc` put
 * the card away and leave the comment where it is. `Resolve` is done with it:
 * the record is archived, so it leaves the text and stays in the project's
 * memory, and the card closes because there is nothing left to look at.
 */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { Check, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

export function CommentCard({
  quote,
  body,
  onSave,
  onResolve,
  onDismiss,
  drafting,
  top,
}: {
  /** The passage this comment is about, shown so the card reads on its own. */
  quote: string;
  /** What the comment says. Empty for one being written. */
  body: string;
  /**
   * Keep what is in the field. Never called with the empty string: a card
   * dismissed with nothing typed leaves no comment behind, which is the only
   * way to change your mind about having started one.
   */
  onSave: (body: string) => void;
  /** Resolve it — the record is archived. Absent while one is being written. */
  onResolve?: () => void;
  onDismiss: () => void;
  /** True for a comment being written, which opens in the field. */
  drafting?: boolean;
  /**
   * How far down the page the passage is, measured by whoever opened the card.
   * Null puts the card under the text: there is nothing on screen to sit beside,
   * which is what a comment whose passage is gone is.
   */
  top: number | null;
}) {
  const [text, setText] = useState(body);
  const [editing, setEditing] = useState(drafting === true);
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (editing) field.current?.focus();
  }, [editing]);

  // The field grows with what is typed. A comment is a remark, and a scrollbar
  // inside a card this small is a scrollbar over four words.
  useEffect(() => {
    const area = field.current;
    if (!area) return;
    area.style.height = "auto";
    area.style.height = `${area.scrollHeight}px`;
  }, [text, editing]);

  const keep = () => {
    const kept = text.trim();
    if (kept && kept !== body.trim()) onSave(kept);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      keep();
      onDismiss();
      return;
    }
    // Return keeps it. A comment is one remark, not a document, and Shift-Return
    // is there for the second sentence.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      keep();
      onDismiss();
    }
  };

  return (
    <aside
      style={top === null ? undefined : { top }}
      className={cn(
        "z-40 w-72 rounded-(--radius-md) border border-separator bg-popover shadow-(--shadow-content)",
        // Inside the page, against its trailing edge, level with the passage.
        // Not hung outside the measure: every panel here clips rather than
        // scrolls, so a card reaching past the page would be cut off at the
        // narrow widths the workspace is allowed to have.
        top === null ? "mt-4" : "absolute right-0",
      )}
    >
      <header className="flex items-start gap-1 border-b border-separator px-2.5 py-1.5">
        <p className="min-w-0 flex-1 truncate text-[11px] leading-snug text-fg-tertiary">
          {quote}
        </p>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Close"
          className="-mr-1 size-5 shrink-0 text-fg-tertiary hover:text-fg"
          onClick={() => {
            keep();
            onDismiss();
          }}
        >
          <X className="size-3.5" />
        </Button>
      </header>

      <div className="px-2.5 py-2">
        {editing ? (
          <textarea
            ref={field}
            rows={1}
            value={text}
            spellCheck={false}
            aria-label="Comment on this passage"
            placeholder="What is worth saying about this passage."
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onKeyDown}
            onBlur={keep}
            className="w-full resize-none bg-transparent text-xs leading-relaxed text-fg-secondary outline-none placeholder:text-fg-tertiary"
          />
        ) : (
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="w-full cursor-text text-left text-xs leading-relaxed whitespace-pre-wrap text-fg-secondary"
          >
            {text || body}
          </button>
        )}
      </div>

      {onResolve ? (
        <footer className="flex items-center justify-end border-t border-separator px-2 py-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-1.5 text-[11px] text-fg-tertiary hover:text-fg"
                onClick={() => {
                  onResolve();
                  onDismiss();
                }}
              >
                <Check className="size-3" />
                Resolve
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              Takes it off the text. The comment is archived, not deleted, and
              stays in the project&apos;s memory.
            </TooltipContent>
          </Tooltip>
        </footer>
      ) : null}
    </aside>
  );
}
