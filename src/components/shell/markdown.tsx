"use client";

import { Fragment, type ReactNode } from "react";

import { Picture } from "@/components/editor/picture";
import { useLinkOrigin, useRecordLinks } from "@/lib/record-link";

/**
 * A reading view for the Markdown a record holds.
 *
 * It was written as a placeholder for the editor and kept for the two records the
 * editor will not touch: the project's own, which is republished rather than
 * edited, and a body whose Markdown would not survive a round trip through
 * blocks. Both are shown exactly as they are stored, which is the only honest
 * thing to show when the alternative is an editor that would rewrite them.
 *
 * That is why it renders the same block structure at the same sizes as
 * `src/components/editor/nodes.tsx`, and why the two have to be changed together:
 * one record read and another edited must not look like two applications.
 *
 * The rule has teeth: every block the editor can insert has to be a block this
 * can draw. A table was missing here while `/` offered one, so a record holding
 * a table was shown as the pipes it is written with — the reading view failing
 * at the one job it has, on content this application had itself produced.
 *
 * Links here behave as they do in the editor, which is the same rule again: a
 * link into the project opens the record, and one to the web is handed to the
 * system rather than followed by this window. What is still inert is
 * `[[wikilinks]]`, which are the corpus's own prose convention and carry no
 * kind — there is nothing in one to route on.
 */
export function Markdown({
  children,
  plugins,
}: {
  children: string;
  plugins?: readonly MarkdownPlugin[];
}) {
  return (
    // The editor's geometry, and it has to stay the editor's: a record read and
    // a record written must not be two different documents. Both get it from
    // `.prose-surface`, which is why neither holds a size of its own.
    <div className="prose-blocks">
      {blocks(children).map((block, index) => {
        const drawn = drawnByPlugin(block, plugins);
        return drawn === null ? (
          <Block key={index} block={block} />
        ) : (
          <Fragment key={index}>{drawn}</Fragment>
        );
      })}
    </div>
  );
}

/**
 * Something that draws a block this module would otherwise draw itself.
 *
 * The seam is deliberately narrow: a plugin **replaces the drawing of a block**
 * and cannot change how the body is split into them. Parsing is what has to
 * agree with the editor — every block `/` can insert has to be a block this can
 * draw, and a plugin that could invent block kinds would be a second Markdown
 * dialect in one window. Drawing is where the interesting cases actually live:
 * a fenced block whose language means something, a table that wants a chart, a
 * quote that is really a callout.
 *
 * Plugins are asked in order and the first that answers wins, so a caller
 * decides precedence by how it lists them. Returning `null` means "not mine".
 */
export interface MarkdownPlugin {
  /** For diagnosis, and so a list of them reads as something. */
  readonly name: string;
  readonly render: (block: MarkdownBlock) => ReactNode | null;
}

function drawnByPlugin(
  block: Block,
  plugins: readonly MarkdownPlugin[] | undefined,
): ReactNode | null {
  if (plugins === undefined) return null;
  for (const plugin of plugins) {
    const drawn = plugin.render(block);
    if (drawn !== null && drawn !== undefined) return drawn;
  }
  return null;
}

/**
 * One block of a body, as this module reads it.
 *
 * Published so that a plugin can match on the same shapes this draws with,
 * rather than being handed a string and asked to parse it a second time — and
 * declared under the published name rather than aliased to one, because an
 * extension can only name what the boundary exports.
 */
export type MarkdownBlock =
  | { kind: "heading"; level: 1 | 2 | 3; text: string }
  | { kind: "picture"; url: string; alt: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; items: string[] }
  | { kind: "quote"; text: string }
  | { kind: "code"; text: string }
  | { kind: "table"; header: string[]; rows: string[][] }
  | { kind: "rule" };

/** The short name, for the dozen uses inside this file. */
type Block = MarkdownBlock;

/**
 * Split the body into blocks, one pass, top to bottom.
 *
 * A fenced block is taken whole and never looked inside: everything else here
 * would happily find a heading in a shell script.
 */
function blocks(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const found: Block[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (line.trim() === "") {
      index += 1;
      continue;
    }

    if (line.startsWith("```")) {
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].startsWith("```")) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      found.push({ kind: "code", text: body.join("\n") });
      continue;
    }

    if (/^ {0,3}(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      found.push({ kind: "rule" });
      index += 1;
      continue;
    }

    // A picture alone on its line is a block, the way the editor holds one. A
    // picture inside a sentence stays inline text here, which is what it is.
    const picture = /^\s*!\[([^\]]*)]\(([^)\s]+)\)\s*$/.exec(line);
    if (picture) {
      found.push({ kind: "picture", url: picture[2], alt: picture[1] });
      index += 1;
      continue;
    }

    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    if (heading) {
      found.push({
        kind: "heading",
        level: heading[1].length as 1 | 2 | 3,
        text: heading[2].trim(),
      });
      index += 1;
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      const body: string[] = [];
      while (index < lines.length && /^\s*>\s?/.test(lines[index])) {
        body.push(lines[index].replace(/^\s*>\s?/, ""));
        index += 1;
      }
      found.push({ kind: "quote", text: body.join(" ").trim() });
      continue;
    }

    // A table is what the editor's own `/` menu inserts, so a view that could
    // not read one showed the record's table as the pipes it is written with —
    // which is the reading view failing at the one job it has.
    if (line.trimStart().startsWith("|") && isRule(lines[index + 1])) {
      const header = cells(line);
      const rows: string[][] = [];
      index += 2;
      while (index < lines.length && lines[index].trimStart().startsWith("|")) {
        rows.push(cells(lines[index]));
        index += 1;
      }
      found.push({ kind: "table", header, rows });
      continue;
    }

    const bullet = /^\s*([-*+])\s+(.*)$/;
    const numbered = /^\s*\d+[.)]\s+(.*)$/;
    if (bullet.test(line) || numbered.test(line)) {
      const ordered = !bullet.test(line);
      const items: string[] = [];
      while (index < lines.length) {
        const item = ordered
          ? numbered.exec(lines[index])
          : bullet.exec(lines[index]);
        if (!item) break;
        // A bullet's text is its last capture either way: `-` captures the
        // marker first, a number does not.
        items.push(item[item.length - 1].trim());
        index += 1;
      }
      found.push({ kind: "list", ordered, items });
      continue;
    }

    const paragraph: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() !== "" &&
      !lines[index].startsWith("```") &&
      !/^(#{1,3})\s/.test(lines[index]) &&
      !/^\s*>\s?/.test(lines[index]) &&
      !bullet.test(lines[index]) &&
      !numbered.test(lines[index]) &&
      !(lines[index].trimStart().startsWith("|") && isRule(lines[index + 1]))
    ) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    found.push({ kind: "paragraph", text: paragraph.join(" ") });
  }

  return found;
}

/** The line under a table's first row, which is what makes it a table. */
function isRule(line: string | undefined): boolean {
  return line !== undefined && /^\s*\|?[\s:|-]*-[\s:|-]*\|?\s*$/.test(line) &&
    line.includes("-");
}

/** One row's cells, without the pipes that delimit them. */
function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

function Block({ block }: { block: Block }) {
  switch (block.kind) {
    case "heading": {
      // The record's own title is the page's `h1`, so its body starts one level
      // below it: a heading that only looks like one leaves a document nobody
      // can navigate with a screen reader.
      const Heading = (["h2", "h3", "h4"] as const)[block.level - 1];
      const size =
        block.level === 1
          ? "pt-[0.75em] text-[1.54em] leading-tight font-semibold"
          : block.level === 2
            ? "pt-[0.5em] text-[1em] font-semibold"
            : "pt-[0.25em] text-[0.92em] font-semibold";
      return (
        <Heading className={`${size} text-fg`}>{inline(block.text)}</Heading>
      );
    }
    case "paragraph":
      return (
        <p className="text-[1em] text-fg-secondary">
          {inline(block.text)}
        </p>
      );
    case "list":
      return (
        <ul
          className={
            block.ordered
              ? "list-decimal space-y-[0.375em] pl-[1.5em] text-[1em] text-fg-secondary marker:text-fg-tertiary"
              : "list-disc space-y-[0.375em] pl-[1.5em] text-[1em] text-fg-secondary marker:text-fg-tertiary"
          }
        >
          {block.items.map((item, index) => (
            <li key={index}>
              {inline(item)}
            </li>
          ))}
        </ul>
      );
    case "quote":
      return (
        <p className="border-l-2 border-separator-strong pl-[0.75em] text-[1em] text-fg-tertiary">
          {inline(block.text)}
        </p>
      );
    case "code":
      return (
        <pre className="overflow-x-auto rounded-(--radius-control) bg-panel p-[0.75em] font-mono text-[0.85em] text-fg-secondary">
          {block.text}
        </pre>
      );
    case "table":
      // The same treatment the editor draws, for the reason the rest of this
      // file exists: one record read and another edited must not look like two
      // applications.
      return (
        <table className="w-full table-fixed border-collapse text-[1em] text-fg-secondary">
          <tbody>
            <tr className="border-b border-separator">
              {block.header.map((cell, index) => (
                <th
                  key={index}
                  className="border-r border-separator px-2 py-1.5 text-left align-top font-semibold text-fg last:border-r-0"
                >
                  {inline(cell)}
                </th>
              ))}
            </tr>
            {block.rows.map((row, index) => (
              <tr key={index} className="border-b border-separator">
                {block.header.map((_, at) => (
                  <td
                    key={at}
                    className="border-r border-separator px-2 py-1.5 align-top last:border-r-0"
                  >
                    {inline(row[at] ?? "")}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      );
    case "picture":
      // The editor's own drawing, for the reason the rest of this file exists:
      // one record read and another edited must not look like two applications.
      return (
        <span className="block py-[0.5em]">
          <Picture url={block.url} alt={block.alt} />
        </span>
      );
    case "rule":
      return <hr className="border-separator" />;
  }
}

/** `code`, **strong**, *emphasis*, [text](url) and [[wikilinks]]. */
const INLINE =
  /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)|(\[\[[^\]]+\]\])|(\[[^\]]+\]\([^)]+\))/g;

function inline(text: string): ReactNode {
  // `<br>` is the only spelling a line break has inside a table cell, and it is
  // what this application's own serialiser writes there. Reading it as text
  // would show the record the tag instead of the break it stands for.
  if (/<br\s*\/?>/i.test(text)) {
    return text.split(/<br\s*\/?>/i).map((part, index) => (
      <Fragment key={index}>
        {index > 0 ? <br /> : null}
        {inline(part)}
      </Fragment>
    ));
  }

  const parts: ReactNode[] = [];
  let cursor = 0;

  for (const match of text.matchAll(INLINE)) {
    const start = match.index;
    if (start > cursor) parts.push(text.slice(cursor, start));
    parts.push(<Mark key={start} text={match[0]} />);
    cursor = start + match[0].length;
  }
  if (cursor < text.length) parts.push(text.slice(cursor));

  return parts.map((part, index) => <Fragment key={index}>{part}</Fragment>);
}

function Mark({ text }: { text: string }) {
  if (text.startsWith("`")) {
    return (
      <code className="rounded-(--radius-control) bg-hover px-1 py-0.5 font-mono text-[0.9em] text-fg">
        {text.slice(1, -1)}
      </code>
    );
  }
  if (text.startsWith("**")) {
    return <strong className="font-medium text-fg">{text.slice(2, -2)}</strong>;
  }
  if (text.startsWith("[[")) {
    // A reference to another record. Drawn as one, and inert until records can
    // open records — a link that looks live and does nothing is worse than one
    // that says what it is.
    return (
      <span className="text-fg underline decoration-separator-strong decoration-dotted underline-offset-2">
        {text.slice(2, -2)}
      </span>
    );
  }
  if (text.startsWith("[")) {
    const label = text.slice(1, text.indexOf("]"));
    const url = text.slice(text.indexOf("](") + 2, -1);
    return <InlineLink label={label} url={url} />;
  }
  return <em className="italic">{text.slice(1, -1)}</em>;
}

/**
 * The editor's own treatment of a link, and its own rule about which ones go
 * anywhere: one that points into this project is followed, everything else is
 * drawn and inert.
 *
 * A record this view is showing is one the editor refused — its Markdown would
 * not survive being written back — and that is a reason to read it rather than
 * a reason to make its links dead ends. Somebody following a chain of records
 * should not find it stops at the one document that happens to hold a footnote.
 */
function InlineLink({ label, url }: { label: string; url: string }) {
  const links = useRecordLinks();
  const base = useLinkOrigin()?.locator ?? null;
  const target = links?.targetOf(url, base) ?? null;

  if (target === null || links === null) {
    return (
      <span
        title={url}
        className="text-fg underline decoration-separator-strong underline-offset-2"
      >
        {label}
      </span>
    );
  }

  return (
    <a
      href={url}
      title={url}
      onClick={(event) => {
        event.preventDefault();
        links.follow(target);
      }}
      className="cursor-pointer text-fg underline decoration-separator-strong underline-offset-2 hover:decoration-fg"
    >
      {label}
    </a>
  );
}
