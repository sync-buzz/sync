/**
 * What an agent's answer is made of, read line by line.
 *
 * Agents answer in markdown — headings, lists, fenced code, a bold phrase — and
 * a block that printed all of it as one flat run is a block nobody reads twice.
 * This turns that into lines the canvas can draw in its own grid.
 *
 * **Line by line, and deliberately not a parser.** A full markdown reader
 * resolves link references, nested blockquotes, setext headings and tables, and
 * the shape it hands back is a tree meant for a document. What the console
 * needs is the opposite: a flat run of lines, each a whole number of cells
 * tall, in a monospaced column. Everything this cannot read stays exactly as it
 * was typed, which is the behaviour a console should have anyway — text it does
 * not understand is still text somebody wanted to see.
 *
 * **No library, and no `dangerouslySetInnerHTML`.** What is folded here came
 * out of somebody else's process, and the one arrangement that cannot go wrong
 * is the one where it is never HTML at any point.
 */

/** A stretch of a line, and how it is set. */
export interface Piece {
  readonly text: string;
  /** In the monospaced face, against a faint ground: `` `like this` ``. */
  readonly code: boolean;
  readonly strong: boolean;
  readonly emphasis: boolean;
}

/** One line of an answer, in the shape the canvas draws. */
export type Line =
  | { readonly kind: "text"; readonly pieces: readonly Piece[] }
  | {
      readonly kind: "heading";
      /** One, two or three. Deeper headings are drawn as the third. */
      readonly level: 1 | 2 | 3;
      readonly pieces: readonly Piece[];
    }
  | {
      readonly kind: "bullet";
      /** What stands in the gutter: a dot, or the number somebody wrote. */
      readonly marker: string;
      /** How far in it sits, in list levels rather than in spaces. */
      readonly depth: number;
      readonly pieces: readonly Piece[];
    }
  | { readonly kind: "quote"; readonly pieces: readonly Piece[] }
  /** A fenced block, kept whole: every line of it, exactly as it arrived. */
  | { readonly kind: "code"; readonly text: string }
  | { readonly kind: "rule" }
  | { readonly kind: "blank" };

const FENCE = /^\s*(?:```|~~~)/u;
const HEADING = /^(#{1,6})\s+(.*)$/u;
const BULLET = /^(\s*)([-*+])\s+(.*)$/u;
const NUMBERED = /^(\s*)(\d{1,3})[.)]\s+(.*)$/u;
const QUOTE = /^\s*>\s?(.*)$/u;
const RULE = /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/u;

/** How many spaces of indent make one level of list. */
const INDENT = 2;

/**
 * The marks that are read inside a line, longest first.
 *
 * Code before the rest, and that is the whole of the precedence: what is inside
 * backticks is quoted, so `**` in there is two asterisks somebody meant to
 * show.
 */
const MARKS: readonly { readonly mark: string; readonly sets: keyof Style }[] = [
  { mark: "`", sets: "code" },
  { mark: "**", sets: "strong" },
  { mark: "__", sets: "strong" },
  { mark: "*", sets: "emphasis" },
  { mark: "_", sets: "emphasis" },
];

interface Style {
  code: boolean;
  strong: boolean;
  emphasis: boolean;
}

const PLAIN: Style = { code: false, strong: false, emphasis: false };

/**
 * Reads the marks inside one line.
 *
 * A mark with nothing closing it is not a mark: `2 * 3 = 6` has to survive, and
 * so does an agent apologising with a lone underscore. The closing one is
 * looked for before anything is set, so an opening that never closes stays a
 * character like any other.
 */
export function pieces(line: string): readonly Piece[] {
  const found: Piece[] = [];
  let plain = "";
  let at = 0;

  const keep = (text: string, style: Style) => {
    if (text.length === 0) return;
    const last = found.at(-1);
    if (
      last !== undefined &&
      last.code === style.code &&
      last.strong === style.strong &&
      last.emphasis === style.emphasis
    ) {
      found[found.length - 1] = { ...last, text: last.text + text };
      return;
    }
    found.push({ text, ...style });
  };

  while (at < line.length) {
    const mark = MARKS.find((candidate) => line.startsWith(candidate.mark, at));
    if (mark === undefined) {
      plain += line[at];
      at += 1;
      continue;
    }

    const opens = at + mark.mark.length;
    const closes = line.indexOf(mark.mark, opens);
    // Nothing closes it, or it closes immediately: both are characters rather
    // than marks, and `**` around nothing is what an agent writes when it means
    // two asterisks.
    if (closes === -1 || closes === opens) {
      plain += line[at];
      at += 1;
      continue;
    }

    keep(plain, PLAIN);
    plain = "";
    const inside = line.slice(opens, closes);
    // Code is quoted whole; everything else may hold further marks.
    if (mark.sets === "code") {
      keep(inside, { ...PLAIN, code: true });
    } else {
      for (const piece of pieces(inside)) {
        keep(piece.text, { ...piece, [mark.sets]: true } as Style);
      }
    }
    at = closes + mark.mark.length;
  }

  keep(plain, PLAIN);
  return found;
}

/** Reads a whole answer. */
export function lines(answer: string): readonly Line[] {
  const read: Line[] = [];
  const source = answer.replace(/\r\n?/gu, "\n").split("\n");

  let fenced: string[] | null = null;
  for (const line of source) {
    if (FENCE.test(line)) {
      if (fenced === null) {
        fenced = [];
      } else {
        read.push({ kind: "code", text: fenced.join("\n") });
        fenced = null;
      }
      continue;
    }
    if (fenced !== null) {
      fenced.push(line);
      continue;
    }

    if (line.trim().length === 0) {
      read.push({ kind: "blank" });
      continue;
    }
    if (RULE.test(line)) {
      read.push({ kind: "rule" });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading !== null) {
      const depth = heading[1]?.length ?? 1;
      read.push({
        kind: "heading",
        level: depth >= 3 ? 3 : depth === 2 ? 2 : 1,
        pieces: pieces(heading[2] ?? ""),
      });
      continue;
    }

    const quote = QUOTE.exec(line);
    if (quote !== null) {
      read.push({ kind: "quote", pieces: pieces(quote[1] ?? "") });
      continue;
    }

    const numbered = NUMBERED.exec(line);
    if (numbered !== null) {
      read.push({
        kind: "bullet",
        marker: `${numbered[2]}.`,
        depth: Math.floor((numbered[1]?.length ?? 0) / INDENT),
        pieces: pieces(numbered[3] ?? ""),
      });
      continue;
    }

    const bullet = BULLET.exec(line);
    if (bullet !== null) {
      read.push({
        kind: "bullet",
        // One glyph whichever of the three was typed: what the author chose
        // between `-`, `*` and `+` is not information, and a list that changed
        // glyph half way down reads as two lists.
        marker: "•",
        depth: Math.floor((bullet[1]?.length ?? 0) / INDENT),
        pieces: pieces(bullet[3] ?? ""),
      });
      continue;
    }

    read.push({ kind: "text", pieces: pieces(line) });
  }

  // A fence that never closed is not an error to report: the agent was cut off
  // mid-answer, and what it had written is still what it wrote.
  if (fenced !== null) read.push({ kind: "code", text: fenced.join("\n") });

  return read;
}
