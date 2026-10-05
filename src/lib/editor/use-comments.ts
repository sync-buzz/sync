"use client";

/**
 * The comments of one open body: where they are, which card is open, and what a
 * save owes the store.
 *
 * It sits between the editor and the two places a comment is seen — the shaded
 * passage under the words and the list in the context column — because all three
 * need the same answer to one question: given the document as it stands right
 * now, which passage does each comment cover?
 *
 * That answer is **recomputed from the text** on every change rather than kept as
 * a pointer that is nudged along. It costs one walk of the document per change,
 * which for a record of any plausible size is well under a millisecond, and it
 * buys the one property worth having: a highlight cannot drift out of step with
 * the stored anchor, because it *is* the stored anchor, resolved again.
 *
 * A comment whose passage moved is written back after the same pause as the body,
 * through the same idea of what a save is — typing stops, and what changed is
 * committed. A record where nobody touched a marked passage writes nothing.
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import type { PlateEditor } from "platejs/react";

import {
  describeAnchor,
  flatten,
  offsetAt,
  type DocumentNode,
  type TextAnchor,
} from "@/lib/editor/anchor";
import { CommentPlugin, type CommentPlacement } from "@/lib/editor/comment-plugin";
import { movedComments, placeComments, type DocumentComment, type PlacedComment } from "@/lib/editor/comments";
import { SAVE_DELAY_MS } from "@/lib/memory/use-document";

/** The editor's value, in the shape the anchoring functions read. */
function nodesOf(editor: PlateEditor): readonly DocumentNode[] {
  return editor.children as unknown as readonly DocumentNode[];
}

/** What the plugin shades: one range per comment whose passage is still there. */
function placementsOf(placed: readonly PlacedComment[]): CommentPlacement[] {
  const placements: CommentPlacement[] = [];
  for (const one of placed) {
    if (!one.range || !one.at) continue;
    placements.push({
      key: one.comment.key,
      // Slate's paths are mutable arrays; the anchoring module states its own as
      // read-only, because nothing in it is allowed to move a node.
      range: {
        anchor: { path: [...one.range.anchor.path], offset: one.range.anchor.offset },
        focus: { path: [...one.range.focus.path], offset: one.range.focus.offset },
      },
      exact: one.at.exact,
    });
  }
  return placements;
}

export interface CommentDraft {
  readonly anchor: TextAnchor;
}

/**
 * How far down the page a card belongs.
 *
 * Measured against the surface the card is positioned in, because the passage may
 * be anywhere in a document of any length: a card pinned to the foot of the page
 * would be a remark about a sentence four screens above it. Null when there is
 * nothing on screen to measure — which is what an orphaned comment is.
 */
function topWithin(surface: HTMLElement | null, rect: DOMRect | null): number | null {
  if (!surface || !rect) return null;
  return rect.top - surface.getBoundingClientRect().top;
}

/** Where the words somebody has selected are, right now. */
function selectionRect(): DOMRect | null {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const rect = selection.getRangeAt(0).getBoundingClientRect();
  return rect.height === 0 && rect.width === 0 ? null : rect;
}

export function useComments({
  editor,
  surface,
  comments,
  onCommentWrite,
  onCommentMoved,
}: {
  editor: PlateEditor;
  /** What a card's position is measured against: the page the text is set on. */
  surface: RefObject<HTMLElement | null>;
  comments: readonly DocumentComment[];
  onCommentWrite?: (anchor: TextAnchor, body: string) => void;
  onCommentMoved?: (
    moves: readonly { readonly key: string; readonly anchor: TextAnchor }[],
  ) => void;
}) {
  // The document as it stands, which is what placing a comment is a function of.
  // Held here rather than read from the editor on render: the editor is mutable
  // and React would have no reason to recompute anything when it changed.
  const [value, setValue] = useState<readonly DocumentNode[]>(() => nodesOf(editor));
  const [active, setActive] = useState<string | null>(null);
  const [draft, setDraft] = useState<CommentDraft | null>(null);
  const [top, setTop] = useState<number | null>(null);

  const placed = useMemo(() => placeComments(value, comments), [value, comments]);

  /**
   * The two callbacks, as functions that do not change.
   *
   * Whoever opens this view passes inline arrows — ordinary React, and not
   * something a caller should have to think about. Depended on directly they
   * would restart the save pause on every render of that caller, which is a comment
   * whose new position is never written while anything else on screen is busy.
   */
  const told = useRef({ onCommentWrite, onCommentMoved });
  useEffect(() => {
    told.current = { onCommentWrite, onCommentMoved };
  }, [onCommentWrite, onCommentMoved]);

  useEffect(() => {
    editor.setOption(CommentPlugin, "placements", placementsOf(placed));
    // Decorations are computed per node and cached; the ranges changing under
    // them is the one thing the editor cannot notice by itself.
    editor.api.redecorate();
  }, [editor, placed]);

  useEffect(() => {
    const moves = movedComments(placed);
    if (moves.length === 0) return;
    const timer = setTimeout(() => told.current.onCommentMoved?.(moves), SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [placed]);

  /** Told by the editor on every change to the body. */
  const reflow = useCallback((next: readonly DocumentNode[]) => setValue(next), []);

  /**
   * Start a comment on what is selected.
   *
   * The selection becomes an anchor immediately — a quote with its neighbours —
   * so what the comment is about is settled before anybody types a word of it, and
   * the caret moving into the card cannot change it.
   */
  const start = useCallback(() => {
    const selection = editor.selection;
    if (!selection) return;

    const flat = flatten(nodesOf(editor));
    const from = offsetAt(flat, selection.anchor);
    const to = offsetAt(flat, selection.focus);
    if (from === null || to === null || from === to) return;

    setTop(topWithin(surface.current, selectionRect()));
    setActive(null);
    setDraft({
      anchor:
        from < to ? describeAnchor(flat.text, from, to) : describeAnchor(flat.text, to, from),
    });
  }, [editor, surface]);

  const keep = useCallback(
    (body: string) => {
      if (!draft) return;
      told.current.onCommentWrite?.(draft.anchor, body);
    },
    [draft],
  );

  const cards = useMemo(
    () => ({
      active,
      open: (key: string) => {
        const mark = window.document.querySelector(`[data-comment-mark="${key}"]`);
        setTop(topWithin(surface.current, mark?.getBoundingClientRect() ?? null));
        setDraft(null);
        setActive(key);
      },
      dismiss: () => {
        setDraft(null);
        setActive(null);
      },
    }),
    [active, surface],
  );

  return { placed, active, draft, top, cards, reflow, start, keep };
}
