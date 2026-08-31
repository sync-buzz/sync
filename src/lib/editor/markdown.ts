/**
 * The one place Markdown becomes blocks, and blocks become Markdown again.
 *
 * The store holds a record's body as Markdown, so the editor is not the format:
 * it is a view of it that has to hand the text back. Everything about that round
 * trip lives here, because a serialiser configured in two places is two formats
 * with one name.
 *
 * Three rules the numbers below are chosen to keep:
 *
 * 1. **A save rewrites the body, so it must not change what the body says.**
 *    What survives verbatim is content; what does not survive is a reason to
 *    refuse to edit the record at all, which is what [`fidelity`] decides.
 * 2. **Formatting is not content.** A soft-wrapped paragraph, `*` for a bullet
 *    and `-` for a bullet are the same claim, so the editor normalises them and
 *    the first save reflows the file. That is stated rather than hidden.
 * 3. **The store's own conventions win over the serialiser's defaults.** Sync
 *    writes `[[wikilinks]]` and lists with `-`, and a round trip that escaped
 *    the first and rewrote the second would be the interface correcting the
 *    project's own corpus.
 */

import { MarkdownPlugin } from "@platejs/markdown";
import type { SlateEditor, Value } from "platejs";
import remarkGfm from "remark-gfm";

/**
 * A Markdown syntax tree node, as much of one as this module touches.
 *
 * Deliberately structural rather than imported: `mdast` is a transitive
 * dependency of the Markdown plugin, and typing one walk over `children` is not
 * worth adding it to this package's own list.
 */
interface MarkdownNode {
  type: string;
  value?: string;
  children?: MarkdownNode[];
}

/**
 * Collapse the newlines inside a paragraph, the way every Markdown renderer
 * does.
 *
 * A body wrapped at eighty columns holds a newline in the middle of a sentence.
 * Without this the editor reads each one as a line break the person typed, and
 * the next save writes it back as a hard break — `\` at the end of every line —
 * which turns one paragraph into eight lines that render as eight lines. That is
 * the one thing in the round trip that changes what the body *says* rather than
 * how it is spelled, so it is corrected on the way in.
 *
 * Only `text` nodes are touched. Fenced code, inline code and HTML carry their
 * newlines as a value of their own and keep them.
 */
function remarkCollapseSoftWraps() {
  return (tree: MarkdownNode) => {
    const walk = (node: MarkdownNode) => {
      if (node.type === "text" && node.value !== undefined) {
        node.value = node.value.replace(/[ \t]*\n[ \t]*/g, " ");
      }
      for (const child of node.children ?? []) walk(child);
    };
    walk(tree);
  };
}

/**
 * Read `<br>` back as the line break it is — and write it back the same way.
 *
 * A line break inside a table cell cannot be a newline: a GFM row is one line.
 * `<br/>` is the only spelling there is, which is what the serialiser produces
 * and what the store therefore holds. Reading it as raw HTML made the editor
 * refuse its own output; reading it as a break without also *writing* it as one
 * was worse, because the default rendering of a break inside a cell is nothing
 * at all — the round trip silently dropped every line somebody had typed there.
 *
 * So the two halves are stated together, here and in [`WRITE`], and neither is
 * correct without the other. Consecutive breaks arrive as one HTML node —
 * `<br/><br/><br/>` — so the node becomes as many breaks as it spells.
 */
const ONLY_BREAKS = /^(?:\s*<br\s*\/?>\s*)+$/i;

function remarkBreaksFromHtml() {
  return (tree: MarkdownNode) => {
    const walk = (node: MarkdownNode) => {
      const children = node.children;
      if (children === undefined) return;
      for (let index = children.length - 1; index >= 0; index -= 1) {
        const child = children[index];
        if (child.type === "html" && ONLY_BREAKS.test(child.value ?? "")) {
          const breaks = (child.value ?? "").match(/<br\s*\/?>/gi)?.length ?? 1;
          children.splice(
            index,
            1,
            ...Array.from({ length: breaks }, () => ({ type: "break" })),
          );
          continue;
        }
        walk(child);
      }
    };
    walk(tree);
  };
}

/** What the Markdown plugin is configured with, for both directions. */
export const MARKDOWN_OPTIONS = {
  remarkPlugins: [remarkGfm, remarkCollapseSoftWraps, remarkBreaksFromHtml],
};

/**
 * `withoutMdx` is not a preference. The plugin's MDX pass rewrites an HTML
 * comment into `{/* … *\/}`, which is MDX for a comment and Markdown for
 * visible text — a record edited once would show its comments to everybody.
 */
const READ = { withoutMdx: true } as const;

/**
 * How the body is spelled on the way out.
 *
 * `bullet: "-"` and `rule: "-"` are the corpus's own conventions rather than the
 * serialiser's defaults, and `emphasis: "_"` with `strong: "*"` is the pairing
 * that leaves `**bold**` and `_italic_` looking like what everything else in
 * this repository writes.
 */
const WRITE = {
  // An empty block is the space a person leaves themselves while writing, not a
  // paragraph the record holds. Preserved, it is written as a zero-width space —
  // invisible in the window and invisible in the corpus, which is the worst place
  // for something to be.
  preserveEmptyParagraphs: false,
  /**
   * The one block that is in the document and is not part of it.
   *
   * A picture being written into the working tree stands in the body as a
   * placeholder until the file exists. It has no Markdown spelling — it is not
   * content, it is a job in progress — and the serialiser has no rule for it, so
   * reaching one threw and took the save with it. Every keystroke during an
   * upload was a failed serialisation.
   *
   * Left out rather than given a spelling: a save while a picture is still
   * being written should store the body as it stands, and the picture arrives
   * in the next save, a moment later, as the `![](…)` it becomes.
   */
  disallowedNodes: ["placeholder"] as string[],
  remarkStringifyOptions: {
    bullet: "-",
    emphasis: "_",
    strong: "*",
    fences: true,
    rule: "-",
    resourceLink: false,
    handlers: {
      /**
       * The other half of `remarkBreaksFromHtml`.
       *
       * Outside a table a break is a backslash and a newline, which is what the
       * serialiser does anyway. Inside a cell there is no newline to be had —
       * one row is one line — so it is `<br/>`, the same spelling this file
       * reads back.
       *
       * The test is the ancestor stack rather than the immediate parent, and
       * that is the whole of why the first attempt at this did nothing: the
       * editor wraps a cell's content in a paragraph, so a break's parent is
       * that paragraph and never the cell. The version that checked the parent
       * shipped, looked right, and wrote `\` and a newline into the middle of a
       * table row — which is not a line break in a cell but a broken table.
       */
      break: (
        _node: unknown,
        _parent: unknown,
        state: { stack: readonly string[] },
      ) => (state.stack.includes("tableCell") ? "<br/>" : "\\\n"),
    },
  },
} as const;

/**
 * A wikilink is Sync's own syntax, and the serialiser has never heard of it.
 *
 * `mdast` escapes a leading bracket because a bracket starts a link, so
 * `[[a-decision]]` comes back as `\[\[a-decision]]` — valid Markdown that
 * renders the same and breaks every convention the corpus and the agents share.
 * Undoing exactly that one escape is safe in a way undoing escapes in general is
 * not: `[[a]]` re-parses as the text `[[a]]`, so the round trip is stable, while
 * an unescaped `_` could turn a literal underscore into emphasis.
 */
const ESCAPED_WIKILINK = /\\\[\\\[([^\]\n]+)]]/g;

/** Markdown from the store, as blocks the editor can hold. */
export function blocksFromMarkdown(editor: SlateEditor, markdown: string): Value {
  return editor.getApi(MarkdownPlugin).markdown.deserialize(markdown, READ);
}

/** The blocks in the editor, as the Markdown the store will hold. */
export function markdownFromBlocks(editor: SlateEditor, value?: Value): string {
  const markdown = editor
    .getApi(MarkdownPlugin)
    .markdown.serialize(value === undefined ? WRITE : { ...WRITE, value });
  return markdown.replace(ESCAPED_WIKILINK, "[[$1]]");
}

/**
 * Whether this body can be edited without losing part of it.
 *
 * The editor holds blocks, not text: anything it cannot represent is dropped on
 * the way in and gone on the way out. So a record is round-tripped before it is
 * offered as editable, and one that would not survive is shown as it is stored,
 * with the reason. A read-only record is a small disappointment; a record whose
 * footnotes disappeared because somebody fixed a typo in it is a lost claim.
 *
 * Two things are checked, and neither is a guess about intent:
 *
 * - **Raw HTML.** A comment or a tag comes back escaped, which turns something
 *   invisible into visible text. There is no rendering of it this editor can
 *   promise, so it does not offer to.
 * - **Every word.** The round trip is compared word for word against the source
 *   with the Markdown punctuation removed. A word that went missing is content
 *   the editor cannot carry; punctuation that moved is formatting, which it may.
 */
export type Fidelity = { editable: true } | { editable: false; reason: string };

/**
 * Raw HTML that would stop doing something if it were escaped.
 *
 * This has been wrong in both directions. It began anchored to the start of a
 * line, which missed the commonest raw HTML there is — a `<br>` in the middle
 * of a sentence. Widening it to any angle bracket then made it wrong the other
 * way, and worse: `<Workspace>` in a paragraph about a component is a record
 * describing software, which is most of what this application is for, and those
 * records stopped being editable.
 *
 * The distinction is not "is it a tag" but "does escaping it lose anything".
 * A round trip escapes a leading bracket, so `<Workspace>` comes back as
 * `\<Workspace>` — which renders as the same visible text it always was, and
 * that is formatting, which the editor is allowed to change. A `<br>`, a
 * `<sup>` or a comment is different: it *did* something, and escaped it does
 * nothing but show its own source. So only names the platform actually renders
 * count, and a comment counts because it is invisible until it is escaped.
 */
/**
 * `br` is deliberately absent. It is the one tag this editor both writes and
 * reads — a line break in a table cell has no other spelling — so treating it
 * as raw HTML made the editor refuse its own output.
 */
const HTML_ELEMENTS = new Set([
  "a", "abbr", "audio", "b", "bdi", "bdo", "blockquote", "button",
  "canvas", "caption", "cite", "code", "col", "colgroup", "data", "datalist",
  "dd", "del", "details", "dfn", "dialog", "div", "dl", "dt", "em", "embed",
  "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4",
  "h5", "h6", "header", "hr", "i", "iframe", "img", "input", "ins", "kbd",
  "label", "legend", "li", "main", "mark", "menu", "meter", "nav", "object",
  "ol", "optgroup", "option", "output", "p", "picture", "pre", "progress", "q",
  "rp", "rt", "ruby", "s", "samp", "script", "section", "select", "small",
  "source", "span", "strong", "style", "sub", "summary", "sup", "svg", "table",
  "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "tr",
  "track", "u", "ul", "var", "video", "wbr",
]);

/** Every tag-shaped run in the body, whatever it names. */
const TAG_SHAPED = /<(!--)|<\/?([a-zA-Z][a-zA-Z0-9-]*)(?=[\s/>])/g;

function holdsRawHtml(markdown: string): boolean {
  for (const match of markdown.matchAll(TAG_SHAPED)) {
    if (match[1] !== undefined) return true;
    if (HTML_ELEMENTS.has((match[2] ?? "").toLowerCase())) return true;
  }
  return false;
}

/**
 * The body with its code removed, which is where a tag is not raw HTML.
 *
 * A record explaining a component holds `<button>` inside a fence, and a fence
 * is carried through the round trip as its own text — the editor never parses
 * what is inside one. Refusing to edit a document because it quotes HTML would
 * make the documents most likely to be edited the ones that cannot be.
 */
function withoutCode(markdown: string): string {
  return markdown
    .replace(/^ {0,3}(```|~~~)[\s\S]*?^ {0,3}\1[^\n]*$/gm, "")
    .replace(/`[^`\n]*`/g, "");
}

/**
 * A picture standing in the middle of a sentence, rather than alone on a line.
 *
 * Checked directly instead of being left to the round trip, because the round
 * trip does not notice: Plate's serialiser wraps a picture in a paragraph of
 * its own, so `Inline ![icon](./i.png) inside.` comes back as three blocks with
 * the spaces written as `&#x20;` — every word survives, which is all
 * [`missingWords`] can see, and the sentence is broken anyway.
 *
 * That is Plate's behaviour and it predates this editor holding pictures at
 * all: a body like this was already being rewritten by any save. What is new is
 * that it is now refused instead, which is what this module does with every
 * other body it cannot carry.
 *
 * A picture alone on its line is the ordinary case — a diagram between two
 * paragraphs — and it round-trips exactly, so it is not touched here.
 */
const PICTURE = /!\[[^\]]*]\([^)\s]*\)/;

function holdsInlinePicture(markdown: string): boolean {
  return markdown
    .split("\n")
    .some((line) => PICTURE.test(line) && line.replace(PICTURE, "").trim() !== "");
}

export function fidelity(editor: SlateEditor, markdown: string): Fidelity {
  if (markdown.trim() === "") return { editable: true };

  if (holdsInlinePicture(withoutCode(markdown))) {
    return {
      editable: false,
      reason:
        "It holds a picture inside a sentence, and saving would move the picture onto a line of its own and break the sentence around it.",
    };
  }

  if (holdsRawHtml(withoutCode(markdown))) {
    return {
      editable: false,
      reason:
        "It holds raw HTML, which this editor would turn into visible text rather than render.",
    };
  }

  let roundTripped: string;
  try {
    roundTripped = markdownFromBlocks(editor, blocksFromMarkdown(editor, markdown));
  } catch {
    return {
      editable: false,
      reason: "Its Markdown could not be read as blocks.",
    };
  }

  const lost = missingWords(markdown, roundTripped);
  if (lost !== null) {
    return {
      editable: false,
      reason: `It uses Markdown this editor does not hold — “${lost}” would be lost by editing it.`,
    };
  }

  return { editable: true };
}

/**
 * The first word the round trip dropped, or `null` when it dropped none.
 *
 * Markdown punctuation is removed from both sides first, because that is exactly
 * what the editor is allowed to change: a bullet that was `*` and is now `-`, an
 * escape the serialiser added, a paragraph that stopped being wrapped. What it
 * is not allowed to change is a word.
 *
 * An escape is *deleted* rather than turned into a space, and that difference is
 * the whole of whether this holds: punctuation stands between words, an escape
 * stands inside one. A body written elsewhere may escape what this serialiser
 * does not — a colon after `https` is the common one, because it stops a bare
 * link becoming an autolink — and the round trip drops that escape, which is
 * formatting and allowed. Spelled as a space it is not: `https\://example.com`
 * becomes two words where the round trip has one, `https` is reported lost, and
 * a record that would have survived editing untouched is refused.
 */
function missingWords(before: string, after: string): string | null {
  const remaining = new Map<string, number>();
  for (const word of words(after)) {
    remaining.set(word, (remaining.get(word) ?? 0) + 1);
  }
  for (const word of words(before)) {
    const left = remaining.get(word) ?? 0;
    if (left === 0) return word;
    remaining.set(word, left - 1);
  }
  return null;
}

function words(markdown: string): string[] {
  return markdown
    .replace(/\\/g, "")
    .replace(/[#*_`>|~\-[\]()!]/g, " ")
    .split(/\s+/)
    .filter(Boolean);
}
