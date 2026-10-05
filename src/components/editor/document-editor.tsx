"use client";

/**
 * One record, open and editable.
 *
 * There is no edit mode. A record opens as the text it is, the caret goes where
 * it was clicked, and typing changes it — which is the only version of this that
 * matches how a person reads a claim, notices it is wrong, and fixes the part
 * that is wrong. A button that turned reading into editing would ask them to
 * declare an intention they have already acted on.
 *
 * The page is the reading view's geometry exactly: the same measure, the same
 * margins, the same type. What is added is a caret, a list `/` opens, and a
 * toolbar over a selection.
 *
 * The title is part of the surface for the same reason. It is stored beside the
 * body in the same record, so it is written in the same transaction, and a page
 * whose text can be corrected but whose first line cannot would be two rules in
 * one column.
 */

import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { Plate, PlateContent, usePlateEditor } from "platejs/react";

import { FormatToolbar } from "@/components/editor/format-toolbar";
import { CommentCard } from "@/components/editor/comment-card";
import { KindMark } from "@/components/shell/entity-marks";
import type { TextAnchor } from "@/lib/editor/anchor";
import { CommentCardsProvider, useReportComments } from "@/lib/editor/comment-view";
import type { DocumentComment } from "@/lib/editor/comments";
import { blocksFromMarkdown, markdownFromBlocks } from "@/lib/editor/markdown";
import { EDITOR_PLUGINS } from "@/lib/editor/plugins";
import { useComments } from "@/lib/editor/use-comments";
import { showNativeContextMenu } from "@/lib/native-menu";

/** What a view that keeps no comments has. Shared, so its identity is stable. */
const NO_COMMENTS: readonly DocumentComment[] = [];

export function DocumentEditor({
  opening,
  icon,
  note,
  autoFocusTitle,
  comments,
  onCommentWrite,
  onCommentRewrite,
  onCommentResolve,
  onCommentMoved,
  onTitle,
  onBody,
}: {
  /**
   * The title and body the editor opens with. Read once — this component is
   * mounted per record — so a save echoing the store back never moves the caret.
   */
  opening: { title: string; content: string };
  /** The mark for this record's type, from the project's own corpus. */
  icon: string | null | undefined;
  /**
   * What is worth saying about this record before its text.
   *
   * There is one of these: the project's own record, whose title and body are
   * the project's name and description. A person editing what looks like an
   * ordinary claim should know that this one is what the window is named after.
   */
  note?: string;
  /**
   * True for a record that was created a moment ago.
   *
   * A record is created empty and named afterwards, so the caret starts where
   * the naming happens. Anything else would ask a person who just said "new
   * decision" to find the one field on the page that is waiting for them.
   */
  autoFocusTitle?: boolean;
  /**
   * The comments this record carries, and the three things that can happen to one.
   *
   * All four are absent where comments have nowhere to be kept, and then the page
   * is exactly what it was before them: no command over a selection, no shading,
   * nothing in the column beside it. The shell holds no comments of its own — what
   * a comment is stored as is a decision of whoever opened this view.
   */
  comments?: readonly DocumentComment[];
  onCommentWrite?: (anchor: TextAnchor, body: string) => void;
  /** Change what an existing comment says. Its passage is unaffected. */
  onCommentRewrite?: (key: string, body: string) => void;
  onCommentResolve?: (key: string) => void;
  onCommentMoved?: (
    moves: readonly { readonly key: string; readonly anchor: TextAnchor }[],
  ) => void;
  onTitle: (title: string) => void;
  /**
   * Called on every change to the body with a way to read it back.
   *
   * A thunk rather than the Markdown: serialising a document on every keystroke
   * would be work nobody asked for, and the only moment the Markdown is needed
   * is the moment it is written.
   */
  onBody: (read: () => string) => void;
}) {
  const [title, setTitle] = useState(opening.title);
  const titleRef = useRef<HTMLTextAreaElement>(null);

  const editor = usePlateEditor({
    plugins: EDITOR_PLUGINS,
    value: (editor) => blocksFromMarkdown(editor, opening.content),
  });

  // Comments are only offered where there is somewhere to put one. A record opened
  // by something that keeps no comments gets the page it has always had.
  //
  // One empty list rather than a new one each render: what is placed from it is
  // reported to the column beside the text, and a report is a write into that
  // column's state — a fresh `[]` every render would be a fresh report every
  // render, which is a loop with nothing in it changing.
  const kept = comments ?? NO_COMMENTS;
  const wanted = onCommentWrite !== undefined;
  const surface = useRef<HTMLDivElement>(null);
  const page = useComments({ editor, surface, comments: kept, onCommentWrite, onCommentMoved });
  const open = openCard(page.placed, page.active);

  /**
   * Resolving a comment, as a function that does not change.
   *
   * What is reported to the column beside the text is a write into that column's
   * own state, so anything in it that is newly made on every render is a loop:
   * report, render, report. The prop this stands in for is an inline arrow in
   * whoever opened this view — which is ordinary React and not theirs to fix —
   * so the identity is held here, and the current prop is read when it is called.
   */
  const resolveComment = useRef(onCommentResolve);
  useEffect(() => {
    resolveComment.current = onCommentResolve;
  }, [onCommentResolve]);
  const resolve = useCallback((key: string) => resolveComment.current?.(key), []);

  const reveal = useCallback(
    (key: string) => {
      page.cards.open(key);
      // The passage may be off screen. The mark is drawn beside it and is the
      // only part of a highlight with a node of its own, so it is what there is
      // to scroll to.
      window.document
        .querySelector(`[data-comment-mark="${key}"]`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
    },
    [page.cards],
  );

  // What the column beside the text lists, and what a row in it does. Withdrawn
  // while comments are not on offer, so that column says nothing about them.
  useReportComments(
    useMemo(
      () => (wanted ? { placed: page.placed, active: page.active, reveal, resolve } : null),
      [wanted, page.placed, page.active, reveal, resolve],
    ),
  );

  useEffect(() => {
    if (autoFocusTitle) titleRef.current?.focus();
  }, [autoFocusTitle]);

  // A claim's title is often a sentence, so the field grows instead of scrolling
  // sideways: a title you have to scroll to read is one the window is hiding.
  useEffect(() => {
    const field = titleRef.current;
    if (!field) return;
    field.style.height = "auto";
    field.style.height = `${field.scrollHeight}px`;
  }, [title]);

  const onTitleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // A title is one line however long it is, so Return leaves it rather than
    // putting a newline in the middle of the name of a claim.
    if (event.key === "Enter") {
      event.preventDefault();
      editor.tf.focus({ edge: "startEditor" });
    }
  };

  return (
    <div className="prose-surface mx-auto px-8 py-8">
      <div className="flex items-start gap-3">
        <KindMark icon={icon} className="mt-1.5" />
        <textarea
          ref={titleRef}
          rows={1}
          value={title}
          spellCheck={false}
          aria-label="Title"
          onChange={(event) => {
            setTitle(event.target.value);
            onTitle(event.target.value);
          }}
          onKeyDown={onTitleKeyDown}
          onContextMenu={editingMenu}
          className="min-w-0 flex-1 resize-none overflow-hidden bg-transparent text-[1.85em] leading-tight font-semibold text-balance text-fg outline-none placeholder:text-fg-tertiary"
          placeholder="Untitled"
        />
      </div>

      {note ? (
        <p className="mt-4 rounded-(--radius-control) bg-panel px-3 py-2 text-xs text-fg-tertiary">
          {note}
        </p>
      ) : null}

      <div ref={surface} className="relative mt-6">
        <Plate
          editor={editor}
          onValueChange={({ value }) => {
            onBody(() => markdownFromBlocks(editor));
            page.reflow(value);
          }}
        >
          <CommentCardsProvider value={page.cards}>
            <FormatToolbar onComment={wanted ? page.start : undefined} />
            <PlateContent
              className="prose-blocks outline-none [&_[data-slate-placeholder]]:text-fg-tertiary"
              placeholder="Write the body. Press / to insert a block."
              onContextMenu={editingMenu}
            />
            {/* One card at a time, under the text rather than over it: a card
                floating on the words it is about covers the sentence somebody is
                reading it against. The list in the context column is where every
                comment of the record is at once. */}
            {page.draft ? (
              <CommentCard
                drafting
                top={page.top}
                quote={page.draft.anchor.quote}
                body=""
                onSave={page.keep}
                onDismiss={page.cards.dismiss}
              />
            ) : null}
            {open ? (
              <CommentCard
                key={open.comment.key}
                top={page.top}
                quote={open.comment.anchor.quote}
                body={open.comment.body}
                onSave={(body) => onCommentRewrite?.(open.comment.key, body)}
                onResolve={() => onCommentResolve?.(open.comment.key)}
                onDismiss={page.cards.dismiss}
              />
            ) : null}
          </CommentCardsProvider>
        </Plate>
      </div>
    </div>
  );
}

/** The comment whose card is open, if one is. */
function openCard(
  placed: readonly { comment: DocumentComment }[],
  active: string | null,
): { readonly comment: DocumentComment } | null {
  if (active === null) return null;
  return placed.find((one) => one.comment.key === active) ?? null;
}

/**
 * The secondary button in text belongs to the system.
 *
 * These are the system's own implementations of Cut, Copy, Paste and Select All,
 * not ours under its labels — the same predefined items the menu bar claims, and
 * the same reason: in a webview they only work fully once a menu has claimed
 * them. Outside Tauri nothing is suppressed, so a browser keeps its own menu
 * rather than being given none.
 */
function editingMenu(event: { preventDefault: () => void }): void {
  showNativeContextMenu(event, [
    { predefined: "Cut" },
    { predefined: "Copy" },
    { predefined: "Paste" },
    "separator",
    { predefined: "SelectAll" },
  ]);
}
