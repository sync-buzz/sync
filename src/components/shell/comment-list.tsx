"use client";

/**
 * Every comment on the open record, in the order the page reads.
 *
 * This is where a comment is read in full. The card over the text is the glance you
 * get where you are looking; this column has width of its own, which the page
 * does not — `--prose-measure` is 680px and the workspace may be 500px, so there
 * is no margin on the page to list anything in.
 *
 * Choosing a row goes to its passage and opens its card. That is the one thing
 * this list does that the text cannot: a record with eight comments is eight
 * scattered shaded phrases, and finding the fourth one by scrolling is not
 * finding it.
 *
 * A comment whose passage is gone keeps its row, at the end, saying so. It is not
 * dropped and not hidden: somebody wrote it about words that are no longer there,
 * and they are usually not the person who removed them.
 */

import { Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { CommentsOnPage } from "@/lib/editor/comment-view";

export function CommentList({ comments }: { comments: CommentsOnPage }) {
  if (comments.placed.length === 0) return null;

  return (
    <section className="space-y-2">
      <h3 className="text-xs font-semibold text-fg-tertiary">Comments</h3>
      <ul className="space-y-1">
        {comments.placed.map((one) => {
          const orphaned = one.at === null;
          const active = comments.active === one.comment.key;

          return (
            <li key={one.comment.key} className="group/row relative">
              {/* Resolving from here as well as from the card, because this is
                  where somebody goes through what a record has collected — and
                  walking eight comments to the text and back to resolve each is
                  the reading this column exists to save. Shown on hover and on
                  keyboard focus, so a still list is a list of comments rather
                  than of buttons. */}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Resolve this comment"
                title="Resolve — archives it, keeping it in the project's memory"
                onClick={() => comments.resolve(one.comment.key)}
                className="absolute top-1 right-1 size-5 text-fg-tertiary opacity-0 transition-opacity group-hover/row:opacity-100 hover:text-fg focus-visible:opacity-100"
              >
                <Check className="size-3.5" />
              </Button>
              <button
                type="button"
                onClick={() => comments.reveal(one.comment.key)}
                aria-current={active ? "true" : undefined}
                className="w-full rounded-(--radius-control) px-2 py-1.5 pr-7 text-left hover:bg-hover aria-[current]:bg-selected"
              >
                <span className="block truncate text-[11px] text-fg-tertiary">
                  {orphaned ? "Passage gone" : one.comment.anchor.quote}
                </span>
                <span className="mt-0.5 line-clamp-2 block text-xs leading-snug text-fg-secondary">
                  {one.comment.body}
                </span>
                {orphaned ? (
                  <span className="mt-1 block text-[11px] leading-snug text-fg-tertiary">
                    The words this was about are not in the body any more. It
                    still says what it said: “{one.comment.anchor.quote}”.
                  </span>
                ) : one.at?.exact === false ? (
                  // Said out loud rather than drawn only in the text: a comment
                  // matched approximately is a weaker claim, and somebody acting
                  // on it from this list would otherwise never know.
                  <span className="mt-1 block text-[11px] leading-snug text-fg-tertiary">
                    The words under this comment have been edited since it was left.
                  </span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
      <p className="text-xs text-fg-tertiary">
        A comment is kept beside the body rather than in it, so nothing here is
        written into the document — including a document that is a file in the
        repository.
      </p>
    </section>
  );
}
