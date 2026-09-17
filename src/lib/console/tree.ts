/**
 * The one reading of a typed line.
 *
 * There used to be four — what a line means, where its steps divide, how it is
 * coloured, what it could become — agreed by hand and drifting apart the first
 * time one of them learned something the others did not. The one that drifts is
 * the one nobody is testing, and a line coloured as an address but run as a
 * refusal is a window lying about what Return will do.
 *
 * The grammar is in `console.grammar` and the parser beside it is generated
 * from it. This module is the only thing that touches either: everything else
 * asks the questions below.
 */

import type { SyntaxNode, Tree } from "@lezer/common";

import { parser } from "@/lib/console/console.parser";

/**
 * The line last read, kept because three of the four readers ask about the
 * same line in the same frame — one to colour it, one to offer what it could
 * become, one to draw the ghost of the first offer. One entry rather than a
 * map: what is asked about is always the line being typed, and a line that is
 * no longer being typed is never asked about again.
 */
let last: { readonly line: string; readonly tree: Tree } | null = null;

export function read(line: string): Tree {
  if (last !== null && last.line === line) return last.tree;
  const tree = parser.parse(line);
  last = { line, tree };
  return tree;
}

/** The steps of a line, in order, as nodes. */
export function stepsOf(line: string): readonly SyntaxNode[] {
  return read(line).topNode.getChildren("Step");
}

/**
 * Every part of the line that carries text, in the order it is written.
 *
 * Leaves rather than nodes, because what is wanted is the line back in pieces:
 * an `Arg` and the `Word` inside it are the same characters twice, and a
 * caller drawing both would draw the line twice.
 */
export function leavesOf(line: string): readonly SyntaxNode[] {
  const found: SyntaxNode[] = [];
  read(line).iterate({
    enter: (node) => {
      // The line itself is not one of its own parts. It has no children when
      // there is nothing on it, and taking it for a leaf then is how an empty
      // line came to have a word in it.
      if (node.node.parent === null) return;
      if (node.node.firstChild !== null) return;
      // Nothing to draw and nothing to complete: what error recovery leaves
      // behind where it expected something and found the end of the line.
      if (node.from === node.to) return;
      found.push(node.node);
    },
  });
  return found;
}

/**
 * Parts that touch, grouped into the words they spell.
 *
 * What separates one word from the next is a space and nothing else, so `@rev`
 * is a mark and a name that touch, `plan-` is a word and a dash that touch, and
 * `find plan-` is two words rather than three parts. The parser divides finer
 * than a reader does — it has to, because `->` must never be half of a word —
 * and this is where its parts are put back into what somebody typed.
 *
 * One rule in one place, deliberately. Asked twice — once for the caret and
 * once for a step's arguments — and the two would part company the first time
 * one of them learned about a new kind of part.
 */
export function joined(
  parts: readonly SyntaxNode[],
): readonly (readonly SyntaxNode[])[] {
  const words: SyntaxNode[][] = [];
  for (const part of parts) {
    const running = words.at(-1);
    if (running !== undefined && running[running.length - 1]?.to === part.from) {
      running.push(part);
    } else {
      words.push([part]);
    }
  }
  return words;
}

/** What a word covers, what it says, and what it is made of. */
export interface Word {
  readonly from: number;
  readonly to: number;
  readonly text: string;
  readonly parts: readonly SyntaxNode[];
}

function spans(line: string, word: readonly SyntaxNode[]): Word {
  const from = word[0]?.from ?? 0;
  const to = word[word.length - 1]?.to ?? 0;
  // Sliced from the line rather than joined from the parts, so that a word is
  // always exactly what was typed — a gap between two parts of one word would
  // otherwise go missing, and there is nowhere for a caller to notice.
  return { from, to, text: line.slice(from, to), parts: word };
}

/** The words a step is made of, which is its verb and its arguments. */
export function argsOf(line: string, holder: SyntaxNode): readonly Word[] {
  return joined(holder.getChildren("Arg")).map((word) => spans(line, word));
}

/**
 * The word the caret is in.
 *
 * The caret is at the end of the line and is not passed in: the input is one
 * line, Return sends it, and there is nowhere else for it to be. When that
 * stops being true this takes a position and nothing else here changes.
 */
export function wordAt(line: string): Word | null {
  const word = joined(leavesOf(line)).at(-1);
  // Ending in a space is ending a word: what comes after it is the next one,
  // which nobody has started typing.
  if (word === undefined) return null;
  const held = spans(line, word);
  return held.to === line.length ? held : null;
}

/** Whether this node stands inside a step that was handed to a shell. */
export function insideShell(node: SyntaxNode): boolean {
  for (let at: SyntaxNode | null = node; at !== null; at = at.parent) {
    if (at.name === "Shell") return true;
  }
  return false;
}

/** The text a node covers. */
export function textOf(line: string, node: SyntaxNode): string {
  return line.slice(node.from, node.to);
}
