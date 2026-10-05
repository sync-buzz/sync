"use client";

/**
 * A passage somebody commented on.
 *
 * Nothing but a background and a click target, and that is the whole of the
 * design: the marked words are the control. Every document editor a reader has
 * used works this way, and the alternative was tried and was worse — a small
 * glyph drawn after the passage put an uneditable element *inside* the sentence
 * once per text node, so a commented paragraph came back as a row of icons with
 * the highlight broken around each of them, and the marks multiplied with every
 * bold word the passage happened to contain.
 *
 * A decoration is cut to each text node by the plugin, so one comment on one
 * paragraph is several of these side by side. They have to read as one mark:
 * hence a background and no border, no radius on the inner edges and nothing
 * between them. The background is the only thing carried, so nothing here can
 * change the colour, weight or spacing of the prose underneath.
 *
 * The passage the reader has open is the same colour, stronger. Opening one is
 * not a state of the document, so it does not get a colour of its own.
 */

import { PlateLeaf, type PlateLeafProps } from "platejs/react";

import { useCommentCards } from "@/lib/editor/comment-view";
import { cn } from "@/lib/utils";

export function CommentMark(props: PlateLeafProps) {
  const cards = useCommentCards();
  const leaf = props.leaf as { commentKey?: string; commentExact?: boolean };
  const key = leaf.commentKey ?? null;
  const open = key !== null && cards.active === key;

  return (
    <PlateLeaf
      {...props}
      as="span"
      attributes={{
        ...props.attributes,
        // The passage is the control, so the click is on the words themselves.
        // The caret still lands where it was clicked — this is a span inside
        // editable text, not a button over it — which is what makes reading a
        // comment and fixing the sentence it is about one gesture apart.
        onClick: key === null ? undefined : () => cards.open(key),
        "data-comment-mark": key ?? undefined,
        title: key === null ? undefined : "Commented — click to read",
      }}
      className={cn(
        "cursor-default bg-comment-mark",
        open && "bg-comment-mark-open",
        // A passage whose words were edited after the comment was left. Said
        // with a line rather than a second colour: the claim is weaker, not
        // different in kind, and the list beside the text says it in words.
        leaf.commentExact === false && "underline decoration-dotted underline-offset-2",
      )}
    >
      {props.children}
    </PlateLeaf>
  );
}
