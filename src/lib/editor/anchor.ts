/**
 * Where something is in a body, said in the words of the body itself.
 *
 * A position cannot be stored. A path into the document dies on the first
 * paragraph added above it, a character offset dies on the first edit above it,
 * and an offset into the Markdown dies on the first save, which reflows the
 * body and respells its bullets. So nothing here stores an address: it stores a
 * quote with the text on either side of it, and works the address out again
 * every time the record is opened. That is the W3C Web Annotation model, and it
 * is what Hypothesis and Apache Annotator anchor with.
 *
 * Everything in this module is a function of strings and plain objects. The
 * editor's own types are deliberately absent — what is anchored is text, the
 * reading view has no editor at all, and a module that imported one could not
 * be run by `node --test`.
 *
 * The offsets are into the *flat* text of the document — every text node in
 * order, blocks separated by a newline — and never into the Markdown. The two
 * differ by exactly the normalisation a save performs, which is why the flat
 * text is the one that survives it: `*` becoming `-` and a soft-wrapped
 * paragraph becoming one line are both spellings of a block, and a spelling is
 * what the flat text has already dropped.
 */

import search from "approx-string-match";

/**
 * How much text on either side of the quote is kept.
 *
 * A quote alone locates nothing: one word appears in a document dozens of
 * times. Its neighbours are what make it one place, and 32 characters is the
 * length Hypothesis settled on — long enough to separate two occurrences of an
 * ordinary phrase, short enough that editing the next sentence does not cost
 * the anchor.
 */
const CONTEXT = 32;

/**
 * Where the anchor was, as the text read then.
 *
 * `start` and `end` are a hint rather than the answer: checked first because
 * the overwhelmingly common case is a document nobody has touched, and thrown
 * away the moment the quote is not standing there.
 */
export interface TextAnchor {
  readonly quote: string;
  readonly prefix: string;
  readonly suffix: string;
  readonly start: number;
  readonly end: number;
}

/**
 * Where the anchor is now.
 *
 * `exact` is false when the quote itself had to be matched approximately —
 * somebody edited the words under it. The interface says so rather than
 * pretending the note still sits on what it was written about.
 */
export interface Located {
  readonly start: number;
  readonly end: number;
  readonly exact: boolean;
}

/** A text node, as both the editor's value and the reading view spell one. */
export interface TextNode {
  readonly text: string;
}

/** Anything holding text: a paragraph, a list item, a table cell. */
export interface ParentNode {
  readonly children: readonly DocumentNode[];
}

export type DocumentNode = TextNode | ParentNode;

/** Which text node a stretch of the flat text came from. */
export interface TextSpan {
  readonly path: readonly number[];
  /** Where this node's text begins in the flat text. */
  readonly at: number;
  readonly length: number;
}

/** The document as one string, with the way back to the nodes it came from. */
export interface FlatText {
  readonly text: string;
  readonly spans: readonly TextSpan[];
}

/** A place in the document, spelled the way the editor spells one. */
export interface DocumentPoint {
  readonly path: readonly number[];
  readonly offset: number;
}

export interface DocumentRange {
  readonly anchor: DocumentPoint;
  readonly focus: DocumentPoint;
}

function isText(node: DocumentNode): node is TextNode {
  return typeof (node as TextNode).text === "string";
}

/**
 * The document as one string, and the map back.
 *
 * Blocks are separated by a newline and nothing else — no bullet, no `#`, no
 * cell divider. Those are spellings, and a spelling that reached this string
 * would be a spelling an anchor could be lost to.
 */
export function flatten(nodes: readonly DocumentNode[]): FlatText {
  const spans: TextSpan[] = [];
  let text = "";

  const walk = (node: DocumentNode, path: readonly number[]) => {
    if (isText(node)) {
      spans.push({ path, at: text.length, length: node.text.length });
      text += node.text;
      return;
    }
    node.children.forEach((child, index) => {
      // A newline before every block after the first. Inline nodes — the marks
      // inside a sentence — are text and get none, or every bold word would
      // read as its own paragraph to an anchor.
      if (index > 0 && !isText(child)) text += "\n";
      walk(child, [...path, index]);
    });
  };

  nodes.forEach((node, index) => {
    if (index > 0) text += "\n";
    walk(node, [index]);
  });

  return { text, spans };
}

/**
 * The point an offset falls on.
 *
 * An offset can land on a separator — the newline between two blocks belongs to
 * no text node — so `after` decides which way it leans: the start of a range
 * leans into the block ahead of it, the end into the block behind. Leaning both
 * ways the same would put the end of a selection at the start of the next
 * paragraph, which is a highlight one character into a line nobody marked.
 */
export function pointAt(
  flat: FlatText,
  offset: number,
  after = true,
): DocumentPoint | null {
  if (flat.spans.length === 0) return null;

  for (const span of flat.spans) {
    const ends = span.at + span.length;
    if (offset < span.at) {
      // Inside a separator. Leaning forward lands at the head of this node;
      // leaning back is handled by the previous node having already matched.
      return after ? { path: span.path, offset: 0 } : null;
    }
    if (offset < ends || (offset === ends && !after)) {
      return { path: span.path, offset: offset - span.at };
    }
    if (offset === ends && after) {
      // Prefer the head of the next node when there is one: a caret at the join
      // between two marks belongs to the text that follows it.
      continue;
    }
  }

  const last = flat.spans[flat.spans.length - 1];
  return { path: last.path, offset: last.length };
}

/**
 * Where a place in the document falls in the flat text.
 *
 * The way in, used once: somebody selects words and the selection — two points
 * in the tree — becomes two offsets, which is the only form an anchor is ever
 * stored in. `null` for a path this document does not have, which is a selection
 * that has already moved on.
 */
export function offsetAt(flat: FlatText, point: DocumentPoint): number | null {
  for (const span of flat.spans) {
    if (
      span.path.length === point.path.length &&
      span.path.every((step, index) => step === point.path[index])
    ) {
      return span.at + Math.min(point.offset, span.length);
    }
  }
  return null;
}

/** The two points a located anchor covers. */
export function rangeAt(flat: FlatText, at: Located): DocumentRange | null {
  const anchor = pointAt(flat, at.start, true);
  const focus = pointAt(flat, at.end, false);
  if (!anchor || !focus) return null;
  return { anchor, focus };
}

/**
 * What is stored for a stretch of text somebody selected.
 *
 * The quote and its neighbours are cut from the flat text, so they carry no
 * Markdown and are comparable against a document that has since been saved.
 */
export function describeAnchor(
  text: string,
  start: number,
  end: number,
): TextAnchor {
  const from = Math.max(0, Math.min(start, text.length));
  const to = Math.max(from, Math.min(end, text.length));
  return {
    quote: text.slice(from, to),
    prefix: text.slice(Math.max(0, from - CONTEXT), from),
    suffix: text.slice(to, to + CONTEXT),
    start: from,
    end: to,
  };
}

function commonSuffix(left: string, right: string): number {
  let count = 0;
  while (
    count < left.length &&
    count < right.length &&
    left[left.length - 1 - count] === right[right.length - 1 - count]
  ) {
    count += 1;
  }
  return count;
}

function commonPrefix(left: string, right: string): number {
  let count = 0;
  while (count < left.length && count < right.length && left[count] === right[count]) {
    count += 1;
  }
  return count;
}

/** How well the text around a candidate reads like the text around the anchor. */
function context(text: string, anchor: TextAnchor, start: number, end: number): number {
  return (
    commonSuffix(text.slice(0, start), anchor.prefix) +
    commonPrefix(text.slice(end), anchor.suffix)
  );
}

/**
 * Where this anchor is in the text now, or `null` when it is nowhere.
 *
 * Three attempts, in this order, stopping at the first that answers.
 *
 * 1. The quote standing at the offset it was stored at, with the same
 *    neighbours. This is what an untouched document answers, and it costs one
 *    string comparison.
 * 2. The quote anywhere in the text, scored by how much of its neighbours it
 *    kept and broken by distance from where it was. This is the paragraph that
 *    moved, and the reason the neighbours are stored at all.
 * 3. An approximate match, allowing a quarter of the quote to differ. This is
 *    the typo somebody fixed inside the marked words, and it is the only answer
 *    that comes back with `exact: false`.
 *
 * Returning `null` is a real answer rather than a failure: the passage is gone,
 * and the note that was written about it has to say so instead of being drawn
 * over words it was never about.
 */
export function locateAnchor(text: string, anchor: TextAnchor): Located | null {
  const length = anchor.quote.length;
  if (length === 0) return null;

  const hinted = anchor.start;
  if (
    text.slice(hinted, hinted + length) === anchor.quote &&
    text.slice(Math.max(0, hinted - anchor.prefix.length), hinted) === anchor.prefix
  ) {
    return { start: hinted, end: hinted + length, exact: true };
  }

  let best: Located | null = null;
  let bestScore = -1;
  let bestDistance = Number.POSITIVE_INFINITY;

  for (let at = text.indexOf(anchor.quote); at !== -1; at = text.indexOf(anchor.quote, at + 1)) {
    const score = context(text, anchor, at, at + length);
    const distance = Math.abs(at - hinted);
    if (score > bestScore || (score === bestScore && distance < bestDistance)) {
      best = { start: at, end: at + length, exact: true };
      bestScore = score;
      bestDistance = distance;
    }
  }
  if (best) return best;

  // A quarter of the quote, and never zero: a five-character quote with no
  // allowance at all would already have been found by the loop above.
  const allowed = Math.max(1, Math.floor(length / 4));
  const matches = search(text, anchor.quote, allowed);
  if (matches.length === 0) return null;

  let chosen = matches[0];
  let chosenScore = context(text, anchor, chosen.start, chosen.end);
  for (const match of matches.slice(1)) {
    const score = context(text, anchor, match.start, match.end);
    const better =
      match.errors < chosen.errors ||
      (match.errors === chosen.errors &&
        (score > chosenScore ||
          (score === chosenScore &&
            Math.abs(match.start - hinted) < Math.abs(chosen.start - hinted))));
    if (better) {
      chosen = match;
      chosenScore = score;
    }
  }

  return { start: chosen.start, end: chosen.end, exact: false };
}
