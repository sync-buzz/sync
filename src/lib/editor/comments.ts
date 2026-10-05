/**
 * The comments on an open body, and where each of them is now.
 *
 * A comment is not part of the body. It is stored beside it, and what ties the two
 * together is a quote rather than anything written into the text — see
 * `@/lib/editor/anchor` for why that is the only spelling that survives a save.
 * This module is the other half: taking the comments a record has and working out,
 * for the document as it stands right now, which passage each one covers.
 *
 * Kept free of React and of the editor library on purpose. Placing a comment is a
 * function of the document and the anchors, and a function is what can be run
 * by the test runner against the cases that matter — a paragraph that moved, a
 * quote that appears three times, a passage somebody deleted.
 */

import {
  describeAnchor,
  flatten,
  locateAnchor,
  rangeAt,
  type DocumentNode,
  type DocumentRange,
  type Located,
  type TextAnchor,
} from "@/lib/editor/anchor";

/**
 * One comment, as the store holds it.
 *
 * `key` is the key of the comment's own record, which is what every command here
 * refers to: closing one, revealing one, saying which one a highlight belongs
 * to. The body is the comment's text, and it is drawn nowhere near the page's own
 * type scale — a comment is a remark about the document, not a paragraph of it.
 */
export interface DocumentComment {
  readonly key: string;
  readonly body: string;
  readonly anchor: TextAnchor;
}

/**
 * A comment and the passage it covers today.
 *
 * `at` is `null` for a comment whose passage is gone. That is a state with a place
 * in the interface rather than a comment to drop: somebody wrote it about words
 * that are no longer there, and they are usually not the person now editing.
 */
export interface PlacedComment {
  readonly comment: DocumentComment;
  readonly at: Located | null;
  readonly range: DocumentRange | null;
  /**
   * The anchor this comment would be stored with if it were written now.
   *
   * Carried here because the flat text it is cut from has just been built: a
   * caller working it out again would walk the whole document a second time, on
   * every keystroke. `null` for a comment whose passage is gone — there is nothing
   * to describe, and overwriting its anchor would lose the only record of what
   * it was about.
   */
  readonly fresh: TextAnchor | null;
}

/**
 * Where every comment sits in this document.
 *
 * Ordered the way the comments are read — by where their passages are, with the
 * orphaned ones last. The list in the context column is this order, so that
 * walking it walks the page.
 */
export function placeComments(
  nodes: readonly DocumentNode[],
  comments: readonly DocumentComment[],
): readonly PlacedComment[] {
  const flat = flatten(nodes);

  return comments
    .map((comment) => {
      const at = locateAnchor(flat.text, comment.anchor);
      return {
        comment,
        at,
        range: at ? rangeAt(flat, at) : null,
        fresh: at ? describeAnchor(flat.text, at.start, at.end) : null,
      };
    })
    .sort((left, right) => {
      if (left.at === null && right.at === null) return 0;
      if (left.at === null) return 1;
      if (right.at === null) return -1;
      return left.at.start - right.at.start || left.at.end - right.at.end;
    });
}

/**
 * The comments whose stored anchor no longer describes where they are.
 *
 * What a save has to write, and nothing more: a record where nobody touched the
 * marked passages answers with an empty list, so ordinary editing costs no
 * writes to anything but the body. The quote is re-cut rather than kept —
 * a comment whose words were edited is about the words as they now read, and
 * keeping the old quote would have every later open match it approximately.
 */
export function movedComments(
  placed: readonly PlacedComment[],
): readonly { readonly key: string; readonly anchor: TextAnchor }[] {
  return placed
    .filter((one) => one.fresh !== null && !sameAnchor(one.fresh, one.comment.anchor))
    .map((one) => ({ key: one.comment.key, anchor: one.fresh as TextAnchor }));
}

/** Whether two anchors say the same thing, so a save can skip the ones that do. */
export function sameAnchor(left: TextAnchor, right: TextAnchor): boolean {
  return (
    left.quote === right.quote &&
    left.prefix === right.prefix &&
    left.suffix === right.suffix &&
    left.start === right.start &&
    left.end === right.end
  );
}
