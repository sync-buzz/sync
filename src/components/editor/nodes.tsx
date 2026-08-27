"use client";

/**
 * What a block looks like while it is being written.
 *
 * Every component here is the reading view's treatment of the same block, at the
 * same size, on the same surface — see `src/components/shell/markdown.tsx`,
 * which still renders the records this editor will not touch. A record that
 * changed shape the moment it became editable would make editing feel like
 * opening a different application, and a person cannot trust what they are
 * reading if the reading is a preview of something else.
 *
 * So: one type scale, tokens only, and no editor chrome sitting in the text.
 * What the editor adds over the reading view is a caret — and, for the one block
 * that is more than the text inside it, the commands a table needs. Those are
 * the system's menu on a cell and `Format ▸ Table` in the menu bar, not controls
 * floating over the page: a row of buttons that appeared beside a table would be
 * editor chrome sitting in the text, which is the thing this file is against.
 */

import { useMemo, type MouseEvent } from "react";

import { isOrderedList } from "@platejs/list";
import { useTodoListElement, useTodoListElementState } from "@platejs/list/react";
import type { TImageElement, TLinkElement, TListElement } from "platejs";
import {
  PlateElement,
  PlateLeaf,
  useEditorRef,
  type PlateElementProps,
  type PlateLeafProps,
  type RenderNodeWrapper,
  useFocused,
  useReadOnly,
  useSelected,
} from "platejs/react";

import {
  tableCommands,
  useReportTableCommands,
} from "@/lib/editor/table-commands";
import { Picture } from "@/components/editor/picture";
import { showNativeContextMenu } from "@/lib/native-menu";
import { useLinkOrigin, useRecordLinks } from "@/lib/record-link";
import { cn } from "@/lib/utils";

/**
 * Every size in a record is a multiple of one number, and that number is the
 * person's.
 *
 * `1em` is the size they set, so a paragraph is exactly it and everything else
 * is a ratio away — see `.prose-surface` in `globals.css`. Written as ratios
 * rather than as the shell's `text-base`/`text-sm` steps because those are the
 * *interface's* scale, fixed in pixels at desktop density, and a record is not
 * a piece of interface: it is the thing the window is for, and it is read for
 * an hour at a time by somebody whose eyes are not ours.
 *
 * The line is theirs too, and the gap between blocks is half of it — one
 * decision, not two. A full line of air under a relaxed line made every
 * paragraph break read as a section.
 */
export function Paragraph(props: PlateElementProps) {
  return (
    <PlateElement
      {...props}
      className="text-[1em] text-fg-secondary"
    >
      {props.children}
    </PlateElement>
  );
}

/**
 * A heading in the body sits one level below the record's title, which is the
 * page's `h1`. The level the person chose is the level that is stored; the tag
 * is shifted so that a body cannot leave a document with two first-level
 * headings and no outline a screen reader can follow.
 */
const HEADING_TAG = ["h2", "h3", "h4", "h5", "h6", "h6"] as const;
/**
 * A heading carries the space above it, because that space is what says a
 * section started. With paragraphs half a line apart, the gap has to come from
 * somewhere, and putting it under the heading would separate it from the text
 * it introduces.
 */
const HEADING_SIZE = [
  "pt-[0.75em] text-[1.54em] leading-tight font-semibold",
  "pt-[0.5em] text-[1em] font-semibold",
  "pt-[0.25em] text-[0.92em] font-semibold",
  "pt-[0.25em] text-[0.92em] font-semibold",
  "pt-[0.25em] text-[0.92em] font-semibold",
  "pt-[0.25em] text-[0.92em] font-semibold",
] as const;

export function heading(level: 1 | 2 | 3 | 4 | 5 | 6) {
  function Heading(props: PlateElementProps) {
    return (
      <PlateElement
        {...props}
        as={HEADING_TAG[level - 1]}
        className={cn("text-fg", HEADING_SIZE[level - 1])}
      >
        {props.children}
      </PlateElement>
    );
  }
  Heading.displayName = `Heading${level}`;
  return Heading;
}

export function Blockquote(props: PlateElementProps) {
  return (
    <PlateElement
      {...props}
      as="blockquote"
      className="border-l-2 border-separator-strong pl-[0.75em] text-[1em] text-fg-tertiary"
    >
      {props.children}
    </PlateElement>
  );
}

/**
 * A rule is a void block: it holds no text, so the selection has to be visible
 * on the rule itself or selecting it looks like nothing happened.
 */
export function HorizontalRule(props: PlateElementProps) {
  const selected = useSelected();
  const focused = useFocused();
  const readOnly = useReadOnly();

  return (
    <PlateElement {...props} className="py-2">
      <div contentEditable={false}>
        <hr
          className={cn(
            "border-separator",
            selected && focused && "border-focus",
            !readOnly && "cursor-pointer",
          )}
        />
      </div>
      {props.children}
    </PlateElement>
  );
}

export function CodeBlock(props: PlateElementProps) {
  return (
    <PlateElement
      {...props}
      as="pre"
      className="overflow-x-auto rounded-(--radius-control) bg-panel p-[0.75em] font-mono text-[0.85em] text-fg-secondary"
    >
      {props.children}
    </PlateElement>
  );
}

export function CodeLine(props: PlateElementProps) {
  return <PlateElement {...props}>{props.children}</PlateElement>;
}

/**
 * A link, and the three places one can go.
 *
 * A relative path is a document of this project, resolved the way GitHub
 * resolves one; a `sync://` url names a record that has no file; and `http`,
 * `https`, `mailto` or `tel` is handed to the system, which is the only correct
 * thing to do with the web in a desktop window — a webview that followed it
 * would replace the application with a page.
 *
 * Which of the three a url is, is not decided here. `targetOf` answers it with
 * the project's attached folders in hand, so a path landing outside every one
 * of them — source, an image nobody attached — is drawn as the text it is, and
 * so is a scheme the capability does not grant.
 *
 * **Following one takes the platform's modifier.** This is text somebody is
 * writing, and in text a click means "put the caret here" — the first version
 * of this followed a plain click, which made a link the one word in a record
 * you could not select without leaving the page you were editing. Every editor
 * that has both gestures resolves it the same way, and the plugin's own panel
 * appears when the caret rests in a link, so nobody has to know the modifier to
 * reach where it goes.
 */
export function Link(props: PlateElementProps<TLinkElement>) {
  const links = useRecordLinks();
  const base = useLinkOrigin()?.locator ?? null;
  const target = links?.targetOf(props.element.url, base) ?? null;

  if (target === null || links === null) {
    return (
      <PlateElement
        {...props}
        as="span"
        attributes={{ ...props.attributes, title: props.element.url }}
        className="text-fg underline decoration-separator-strong underline-offset-2"
      >
        {props.children}
      </PlateElement>
    );
  }

  return (
    <PlateElement
      {...props}
      as="a"
      attributes={{
        ...props.attributes,
        href: props.element.url,
        title: props.element.url,
        onClick: (event: MouseEvent<HTMLElement>) => {
          // The href is there so that this is a link to a screen reader and to
          // the system's own menu on it. Letting the webview act on it would
          // navigate the window itself, which is the bug the shell has always
          // refused to ship — so the default goes whether it is followed or not.
          event.preventDefault();
          if (follows(event)) links.follow(target);
        },
      }}
      className="text-fg underline decoration-separator-strong underline-offset-2"
    >
      {props.children}
    </PlateElement>
  );
}

/**
 * Whether this click meant "go there".
 *
 * Command on a Mac, Control everywhere else, and the two are not interchangeable
 * on either: Control-click on macOS is the secondary button, so treating it as
 * "follow" would send somebody to another record while they were asking for a
 * menu.
 */
function follows(event: MouseEvent<HTMLElement>): boolean {
  const mac =
    typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);
  return mac ? event.metaKey : event.ctrlKey;
}

/**
 * A picture, as the one block whose content is not in the record at all.
 *
 * A void element: it holds no text, so the empty text node Slate requires is
 * kept out of the way and the selection has to be visible on the picture
 * itself, the same problem a rule has. The alt text is the node's `caption`,
 * which is what the Markdown serialiser reads back out as `![alt](url)`.
 */
export function Image(props: PlateElementProps<TImageElement & { caption?: { text?: string }[] }>) {
  const selected = useSelected({ suppressThrow: true });
  const focused = useFocused();
  // The alt text is the node's `caption`, which is where the serialiser both
  // reads it from and writes it back to. Flattened rather than rendered: alt
  // text is a string to every reader of it, marks and all.
  const alt = (props.element.caption ?? [])
    .map((node) => ("text" in node && typeof node.text === "string" ? node.text : ""))
    .join("");

  return (
    <PlateElement {...props} className="py-[0.5em]">
      <div contentEditable={false}>
        <Picture
          url={props.element.url}
          alt={alt}
          className={cn(selected && focused && "ring-2 ring-focus")}
        />
      </div>
      {props.children}
    </PlateElement>
  );
}

/**
 * A table, and the one block that is more than the text inside it.
 *
 * While it holds the caret it lends its commands to the window, which is what
 * puts them in `Format ▸ Table` where the keyboard can reach them. The same
 * seven are under the secondary button on any cell, drawn by the system.
 */
export function Table(props: PlateElementProps) {
  const editor = useEditorRef();
  // `Delete Table` is one of the commands this reports, so the table can be
  // gone while its own selector is still being asked whether it holds the
  // caret. A table that no longer exists holds nothing; it does not throw.
  const selected = useSelected({ suppressThrow: true });
  // Bound once per editor: the commands act on wherever the caret is, so they
  // do not change when it moves from one cell to the next, and the menu bar is
  // rebuilt only when the caret enters or leaves a table.
  const commands = useMemo(() => tableCommands(editor), [editor]);
  useReportTableCommands(selected ? commands : null);

  return (
    <PlateElement
      {...props}
      as="table"
      className="w-full table-fixed border-collapse text-[1em] text-fg-secondary"
    >
      <tbody>{props.children}</tbody>
    </PlateElement>
  );
}

export function TableRow(props: PlateElementProps) {
  return (
    <PlateElement {...props} as="tr" className="border-b border-separator">
      {props.children}
    </PlateElement>
  );
}

export function TableCell(props: PlateElementProps) {
  const onContextMenu = useCellMenu(props);

  return (
    <PlateElement
      {...props}
      as="td"
      attributes={{ ...props.attributes, onContextMenu }}
      className="border-r border-separator px-2 py-1.5 align-top last:border-r-0"
    >
      {props.children}
    </PlateElement>
  );
}

export function TableCellHeader(props: PlateElementProps) {
  const onContextMenu = useCellMenu(props);

  return (
    <PlateElement
      {...props}
      as="th"
      attributes={{ ...props.attributes, onContextMenu }}
      className="border-r border-separator px-2 py-1.5 text-left align-top font-semibold text-fg last:border-r-0"
    >
      {props.children}
    </PlateElement>
  );
}

/**
 * What the secondary button offers on a cell.
 *
 * It carries the editing commands the rest of the text carries — a cell is text
 * and the clipboard has to work in it — and then what can be done to the table
 * around it. The insertions and the removals are separated, the way this system
 * separates a destructive command from the rest, and title case is used because
 * the system draws this menu.
 *
 * Two things this has to do that the menu bar's copy does not:
 *
 * - **Move the caret into the cell first.** Every one of these transforms reads
 *   the selection, and a secondary click does not move it on its own — without
 *   this, `Delete Row` would delete whichever row was last typed in, which is
 *   the one thing a menu opened under the pointer must never do.
 * - **Keep the event.** The editor's own menu is bound to the surface around
 *   this cell, and letting the event reach it would replace the menu that was
 *   just built with one that knows nothing about tables.
 */
function useCellMenu(props: PlateElementProps) {
  const editor = useEditorRef();

  return (event: MouseEvent<HTMLElement>) => {
    const path = editor.api.findPath(props.element);
    if (path !== undefined) {
      const start = editor.api.start(path);
      if (start !== undefined) editor.tf.select(start);
    }

    const table = tableCommands(editor);
    const shown = showNativeContextMenu(event, [
      { predefined: "Cut" },
      { predefined: "Copy" },
      { predefined: "Paste" },
      "separator",
      { label: "Insert Row Above", onSelect: table.insertRowAbove },
      { label: "Insert Row Below", onSelect: table.insertRowBelow },
      { label: "Insert Column Before", onSelect: table.insertColumnBefore },
      { label: "Insert Column After", onSelect: table.insertColumnAfter },
      "separator",
      { label: "Delete Row", onSelect: table.deleteRow },
      { label: "Delete Column", onSelect: table.deleteColumn },
      { label: "Delete Table", onSelect: table.deleteTable },
    ]);

    // Only when this menu is the one being shown: where there is no native menu
    // — a browser during development — the event belongs to whoever else wants
    // it, and suppressing the system's own to then show nothing is worse than
    // either menu.
    if (shown) event.stopPropagation();
  };
}

/**
 * The marker beside a list item.
 *
 * A list here is a block that says which list style it carries, not a `<ul>`
 * holding `<li>`s — which is how the same body can be a heading, a paragraph or
 * a quote at the same indent, and how Tab and Shift-Tab move an item without
 * rebuilding a tree. The wrapper is what turns that back into one row of a list
 * for the browser, so the marker, the numbering and the checkbox are all drawn
 * where a person expects them.
 *
 * It replaces the plugin's own wrapper, which draws bullets and numbers and
 * nothing else: a task list would keep its `checked` state in the record and
 * show no box.
 */
export const ListWrapper: RenderNodeWrapper = ({ element }) => {
  if (!(element as TListElement).listStyleType) return;
  return ListRow;
};

function ListRow(props: PlateElementProps) {
  const element = props.element as TListElement;
  const { listStart, listStyleType } = element;

  if (listStyleType === "todo") return <TodoRow {...props} />;

  const List = isOrderedList(element) ? "ol" : "ul";
  return (
    <List
      className="relative m-0 p-0"
      start={listStart}
      style={{ listStyleType }}
    >
      <li className="marker:text-fg-tertiary">{props.children}</li>
    </List>
  );
}

/**
 * A task list item, with the box the system draws.
 *
 * `<input type="checkbox">` rather than a styled control: it is the one part of
 * a record that is a native widget in the first place, `color-scheme` already
 * follows the appearance, and a hand-drawn box would be the interface imitating
 * something the platform ships.
 */
function TodoRow(props: PlateElementProps) {
  const state = useTodoListElementState({ element: props.element });
  const { checkboxProps } = useTodoListElement(state);
  const readOnly = useReadOnly();
  const checked = props.element.checked === true;

  return (
    <ul className="relative m-0 p-0">
      <li className="list-none">
        <div contentEditable={false}>
          <input
            type="checkbox"
            checked={checkboxProps.checked}
            disabled={readOnly}
            aria-label="Done"
            onChange={(event) =>
              checkboxProps.onCheckedChange(event.target.checked)
            }
            onMouseDown={checkboxProps.onMouseDown}
            className="absolute top-1 -left-5 size-3.5 accent-fg"
          />
        </div>
        <span className={cn(checked && "text-fg-tertiary line-through")}>
          {props.children}
        </span>
      </li>
    </ul>
  );
}

export function Bold(props: PlateLeafProps) {
  return (
    <PlateLeaf {...props} as="strong" className="font-medium text-fg">
      {props.children}
    </PlateLeaf>
  );
}

export function Italic(props: PlateLeafProps) {
  return (
    <PlateLeaf {...props} as="em" className="italic">
      {props.children}
    </PlateLeaf>
  );
}

export function Strikethrough(props: PlateLeafProps) {
  return (
    <PlateLeaf {...props} as="del" className="line-through">
      {props.children}
    </PlateLeaf>
  );
}

export function InlineCode(props: PlateLeafProps) {
  return (
    <PlateLeaf
      {...props}
      as="code"
      className="rounded-(--radius-control) bg-hover px-1 py-0.5 font-mono text-[0.9em] text-fg"
    >
      {props.children}
    </PlateLeaf>
  );
}
