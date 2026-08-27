"use client";

/**
 * The list `/` opens, and the one menu in the window that is not the system's.
 *
 * A context menu belongs to the pointer's secondary button, which is a gesture
 * macOS already owns, so the shell shows the system's own — see
 * `src/lib/native-menu.ts`. This is a different thing: it is triggered by typing
 * inside the text, it filters as more is typed, and it inserts at the caret. A
 * native menu cannot be filtered by the keyboard while the caret stays where it
 * is, and a menu that opened next to the pointer would be answering a question
 * nobody asked with the pointer.
 *
 * Being drawn here is not a reason to look like something else. It wears the
 * surface, the corner, the ring, the padding and the row metrics of the shell's
 * own menus — `src/components/ui/dropdown-menu.tsx` is where those are decided,
 * and the classes below are that file's, read rather than re-chosen. It groups
 * what it offers the way a menu on this system does, and it says the Markdown
 * for each block where a menu says the key for each command.
 *
 * Nothing here is reachable only from this list: every block it inserts can also
 * be typed in Markdown, which is the spelling the store keeps anyway.
 *
 * What the person types is held in an `<input>` rather than in the record. A
 * query that lived in the document would be a paragraph that says `/head` for as
 * long as the menu is open, and every keystroke would be an edit to save.
 */

import {
  Fragment,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";

import { useComboboxInput, useHTMLInputCursorState } from "@platejs/combobox/react";
import { toggleCodeBlock } from "@platejs/code-block";
import {
  autoUpdate,
  flip,
  FloatingPortal,
  offset,
  shift,
  size,
  useFloating,
} from "@platejs/floating";
import { toggleList } from "@platejs/list";
import { TablePlugin } from "@platejs/table/react";
import {
  Code,
  Heading1,
  Heading2,
  Heading3,
  List,
  ListOrdered,
  ListTodo,
  Minus,
  Table,
  TextQuote,
  Type,
  type LucideIcon,
} from "lucide-react";
import { KEYS, PathApi } from "platejs";
import {
  PlateElement,
  useEditorRef,
  type PlateEditor,
  type PlateElementProps,
} from "platejs/react";

import { cn } from "@/lib/utils";

/**
 * One thing the list can insert.
 *
 * `keywords` are what else it answers to. `group` is what a separator is drawn
 * between, the way a menu on this system groups commands that are alternatives
 * to one another. `syntax` is the Markdown that does the same thing, shown
 * where a menu shows a shortcut — because it *is* the shortcut here: everything
 * in this list can be typed, and a menu that teaches its own shortcuts is the
 * one convention every menu on macOS keeps.
 */
interface Block {
  label: string;
  group: "text" | "list" | "block";
  icon: LucideIcon;
  syntax?: string;
  keywords: string[];
  run: (editor: PlateEditor) => void;
}

const BLOCKS: Block[] = [
  {
    label: "Text",
    group: "text",
    icon: Type,
    keywords: ["paragraph", "body"],
    run: (editor) => editor.tf.toggleBlock(KEYS.p),
  },
  {
    label: "Heading 1",
    group: "text",
    icon: Heading1,
    syntax: "#",
    keywords: ["h1", "title"],
    run: (editor) => editor.tf.toggleBlock(KEYS.h1),
  },
  {
    label: "Heading 2",
    group: "text",
    icon: Heading2,
    syntax: "##",
    keywords: ["h2", "section"],
    run: (editor) => editor.tf.toggleBlock(KEYS.h2),
  },
  {
    label: "Heading 3",
    group: "text",
    icon: Heading3,
    syntax: "###",
    keywords: ["h3"],
    run: (editor) => editor.tf.toggleBlock(KEYS.h3),
  },
  {
    label: "Bulleted list",
    group: "list",
    icon: List,
    syntax: "-",
    keywords: ["ul", "bullet", "unordered"],
    run: (editor) => toggleList(editor, { listStyleType: "disc" }),
  },
  {
    label: "Numbered list",
    group: "list",
    icon: ListOrdered,
    syntax: "1.",
    keywords: ["ol", "ordered", "number"],
    run: (editor) => toggleList(editor, { listStyleType: "decimal" }),
  },
  {
    label: "Task list",
    group: "list",
    icon: ListTodo,
    syntax: "- [ ]",
    keywords: ["todo", "checkbox", "check"],
    run: (editor) => toggleList(editor, { listStyleType: "todo" }),
  },
  {
    label: "Quote",
    group: "block",
    icon: TextQuote,
    syntax: ">",
    keywords: ["blockquote", "citation"],
    run: (editor) => editor.tf.toggleBlock(KEYS.blockquote),
  },
  {
    label: "Code",
    group: "block",
    icon: Code,
    syntax: "```",
    keywords: ["fence", "snippet", "pre"],
    run: (editor) => toggleCodeBlock(editor),
  },
  {
    label: "Table",
    group: "block",
    icon: Table,
    keywords: ["grid", "rows", "columns"],
    run: (editor) =>
      editor.getTransforms(TablePlugin).insert.table({
        colCount: 2,
        header: true,
        rowCount: 3,
      }),
  },
  {
    label: "Divider",
    group: "block",
    icon: Minus,
    syntax: "---",
    keywords: ["rule", "hr", "separator", "line"],
    run: (editor) =>
      editor.tf.insertNodes({
        children: [{ text: "" }],
        type: editor.getType(KEYS.hr),
      }),
  },
];

function matches(block: Block, query: string): boolean {
  if (query === "") return true;
  const needle = query.toLowerCase();
  return (
    block.label.toLowerCase().includes(needle) ||
    block.keywords.some((keyword) => keyword.startsWith(needle))
  );
}

export function SlashMenu(props: PlateElementProps) {
  const editor = useEditorRef();
  const inputRef = useRef<HTMLInputElement>(null);
  // The box that scrolls and the row that must stay inside it.
  const scroller = useRef<HTMLDivElement | null>(null);
  const activeRow = useRef<HTMLButtonElement | null>(null);
  // What the field points at, so that a screen reader following the caret is
  // told which block is about to be inserted rather than left to guess from a
  // list it cannot see the selection in.
  const listId = useId();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  /**
   * Where the `/` was, so that dismissing the list can put back what was typed.
   *
   * It has to be a point *before* this element and taken while the element still
   * exists: by the time the text goes back, the node holding the query has been
   * removed and the selection is no longer where the person was writing.
   */
  const insertPoint = useRef<ReturnType<
    typeof editor.api.pointRef
  > | null>(null);
  useEffect(() => {
    const path = editor.api.findPath(props.element);
    const point = path === undefined ? undefined : editor.api.before(path);
    const held = point === undefined ? null : editor.api.pointRef(point);
    insertPoint.current = held;
    return () => {
      if (insertPoint.current === held) insertPoint.current = null;
      held?.unref();
    };
  }, [editor, props.element]);

  const cursorState = useHTMLInputCursorState(inputRef);
  const { props: inputProps, removeInput } = useComboboxInput({
    autoFocus: true,
    cancelInputOnArrowLeftRight: true,
    cancelInputOnBackspace: true,
    cancelInputOnBlur: true,
    cancelInputOnDeselect: true,
    cancelInputOnEscape: true,
    cursorState,
    ref: inputRef,
    // Leaving the list is not the same as never having typed: what was typed
    // stays in the text, as the text it was. Backspace is the exception,
    // because there the person is deleting rather than dismissing.
    onCancelInput: (cause) => {
      if (cause === "backspace") return;
      editor.tf.insertText(`/${query}`, {
        at: insertPoint.current?.current ?? undefined,
      });
    },
  });

  const { floatingStyles, refs: anchor } = useFloating({
    middleware: [
      offset(4),
      flip({ padding: 8 }),
      shift({ padding: 8 }),
      // How tall the list may be is decided by the window rather than by a
      // number written here: a menu that ran past the bottom of the window
      // would hide the very rows the arrows were about to reach.
      size({
        padding: 8,
        apply: ({ availableHeight, elements }) => {
          elements.floating.style.maxHeight = `${Math.max(availableHeight, 96)}px`;
        },
      }),
    ],
    placement: "bottom-start",
    whileElementsMounted: autoUpdate,
  });

  const found = useMemo(
    () => BLOCKS.filter((block) => matches(block, query)),
    [query],
  );
  // The query may have moved under the cursor since it was placed.
  const selected = Math.min(active, Math.max(found.length - 1, 0));

  /**
   * Keep the selected row in view while the arrows move it.
   *
   * The list is taller than the box it is shown in, and a selection that walked
   * off the bottom edge left the arrows moving something nobody could see —
   * which reads as a menu that stopped responding rather than one that scrolled.
   *
   * The box is scrolled by hand rather than through `scrollIntoView`: that would
   * ask every scrollable ancestor to help, and the nearest one here is the
   * document the person is writing in. A menu must never move the text under it.
   */
  useEffect(() => {
    const box = scroller.current;
    const row = activeRow.current;
    if (box === null || row === null) return;

    const top = row.offsetTop;
    const bottom = top + row.offsetHeight;
    // The padding of the box is included on purpose: a row flush against the
    // edge of a menu reads as a row that is half cut off.
    if (top - 4 < box.scrollTop) box.scrollTop = Math.max(top - 4, 0);
    else if (bottom + 4 > box.scrollTop + box.clientHeight) {
      box.scrollTop = bottom + 4 - box.clientHeight;
    }
  }, [selected, found.length]);

  const insert = (block: Block) => {
    // Which block the `/` was typed in, held as a path ref because removing the
    // input and inserting a block both move the paths after it.
    const path = editor.api.findPath(props.element);
    const target = path === undefined ? null : editor.api.pathRef(path.slice(0, -1));

    removeInput(true);
    block.run(editor);

    // Some conversions leave the selection outside the block they converted —
    // an empty paragraph turned into a quote is one — and the caret ending up in
    // the paragraph above is the difference between typing into the block that
    // was just inserted and typing into the text before it. A transform that
    // placed the caret inside the new block itself, as inserting a table does,
    // is left alone.
    const block_path = target?.current ?? null;
    target?.unref();
    if (block_path === null) return;
    const inside =
      editor.selection !== null &&
      PathApi.isAncestor(block_path, editor.selection.anchor.path);
    if (inside) return;
    const end = editor.api.end(block_path);
    if (end !== undefined) editor.tf.select(end);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (found.length > 0) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        setActive((found.length + selected + step) % found.length);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        insert(found[selected]);
        return;
      }
    }
    inputProps.onKeyDown(event);
  };

  return (
    <PlateElement {...props} as="span">
      {/*
       * Everything the list is made of sits outside the editable text: the
       * element is an inline void, and a query that was part of the document
       * would leave the record holding `/quo` — and the caret confused about
       * which of the two texts it is in.
       */}
      <span contentEditable={false} ref={(node) => anchor.setReference(node)}>
        <span className="text-base text-fg-tertiary">/</span>
        {/* The field grows with what is typed by sitting over a copy of it. */}
        <span className="relative inline-block min-h-[1lh]">
          <span aria-hidden className="invisible whitespace-pre text-base">
            {query || "​"}
          </span>
          <input
            ref={inputRef}
            aria-label="Insert a block"
            role="combobox"
            // A list with nothing in it is not an open one, and pointing at a
            // list that is not drawn would be describing a menu that is not
            // there.
            aria-expanded={found.length > 0}
            aria-controls={found.length > 0 ? listId : undefined}
            aria-activedescendant={
              found.length > 0 ? `${listId}-${selected}` : undefined
            }
            className="absolute top-0 left-0 size-full bg-transparent text-base text-fg outline-none"
            value={query}
            onBlur={inputProps.onBlur}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
          />
        </span>
      </span>

      <FloatingPortal>
        {/*
         * The same surface, radius, ring, padding and row metrics as every
         * other menu in the window — see `src/components/ui/dropdown-menu.tsx`.
         * This list is drawn by hand because it has to filter under a caret
         * that stays put, not because it is a different kind of object, and a
         * menu that looked like one would be the only thing in the window
         * announcing that it was drawn by a web page.
         */}
        <div
          ref={(node) => {
            anchor.setFloating(node);
            scroller.current = node;
          }}
          style={floatingStyles}
          className="relative z-50 w-64 overflow-y-auto rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10"
        >
          {found.length === 0 ? (
            <p className="px-1.5 py-1 text-xs text-fg-tertiary">
              No block by that name.
            </p>
          ) : (
            <div
              role="listbox"
              id={listId}
              aria-label="Blocks"
              className="flex flex-col"
            >
              {found.map((block, index) => (
                <Fragment key={block.label}>
                  {/* Groups are separated the way a menu on this system
                      separates commands that are alternatives to one another.
                      Filtering can empty a group, so the rule is drawn from
                      what is left rather than from the list as written. */}
                  {index > 0 && found[index - 1].group !== block.group ? (
                    <hr
                      aria-hidden="true"
                      className="-mx-1 my-1 h-px border-0 bg-border"
                    />
                  ) : null}
                  <button
                    type="button"
                    role="option"
                    id={`${listId}-${index}`}
                    ref={index === selected ? activeRow : undefined}
                    aria-selected={index === selected}
                    // The caret is in the input; taking focus away from it here
                    // would cancel the list before the click could land.
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setActive(index)}
                    onClick={() => insert(block)}
                    className={cn(
                      "flex w-full cursor-default items-center gap-1.5 rounded-md px-1.5 py-1 text-left text-sm select-none",
                      // A menu item is the colour of the menu until it is the
                      // selected one, exactly as the shell's own menus are.
                      index === selected && "bg-accent text-accent-foreground",
                    )}
                  >
                    <block.icon
                      aria-hidden="true"
                      className={cn(
                        "size-4 shrink-0",
                        index === selected
                          ? "text-accent-foreground"
                          : "text-fg-tertiary",
                      )}
                    />
                    <span className="truncate">{block.label}</span>
                    {/* Where a menu says the key, this one says the Markdown.
                        It travels with the row rather than staying quiet under
                        the selection, the way a shortcut does. */}
                    {block.syntax ? (
                      <span
                        className={cn(
                          "ml-auto shrink-0 font-mono text-xs",
                          index === selected
                            ? "text-accent-foreground"
                            : "text-fg-tertiary",
                        )}
                      >
                        {block.syntax}
                      </span>
                    ) : null}
                  </button>
                </Fragment>
              ))}
            </div>
          )}
        </div>
      </FloatingPortal>

      {props.children}
    </PlateElement>
  );
}
