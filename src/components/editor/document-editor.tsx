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

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { Plate, PlateContent, usePlateEditor } from "platejs/react";

import { FormatToolbar } from "@/components/editor/format-toolbar";
import { KindMark } from "@/components/shell/entity-marks";
import { blocksFromMarkdown, markdownFromBlocks } from "@/lib/editor/markdown";
import { EDITOR_PLUGINS } from "@/lib/editor/plugins";
import { showNativeContextMenu } from "@/lib/native-menu";

export function DocumentEditor({
  opening,
  icon,
  note,
  autoFocusTitle,
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

      <div className="relative mt-6">
        <Plate
          editor={editor}
          onValueChange={() => onBody(() => markdownFromBlocks(editor))}
        >
          <FormatToolbar />
          <PlateContent
            className="prose-blocks outline-none [&_[data-slate-placeholder]]:text-fg-tertiary"
            placeholder="Write the body. Press / to insert a block."
            onContextMenu={editingMenu}
          />
        </Plate>
      </div>
    </div>
  );
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
