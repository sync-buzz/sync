"use client";

/**
 * How a comment reaches the words it is about, without becoming one of them.
 *
 * The passage a comment covers is drawn as a **decoration**: a range Slate paints
 * over the text without touching the nodes underneath it. That is not tidiness,
 * it is the guarantee. The serialiser walks nodes, so while a comment lives only in
 * a decoration there is nothing for it to write — a comment cannot end up in the
 * body, or in the repository's own `.md` file, even by mistake. A mark or a node
 * of its own would be one edit away from exactly that, and somebody would find
 * out from a diff on a branch they were not looking at.
 *
 * What is decorated is worked out from the anchors elsewhere — see
 * `@/lib/editor/comments` — and handed to this plugin as ranges. The plugin's only
 * job is to cut those ranges to the text node being drawn, which is what Slate
 * asks for one node at a time.
 */

import { RangeApi, TextApi, type DecoratedRange, type TRange } from "platejs";
import { createPlatePlugin } from "platejs/react";

import { CommentMark } from "@/components/editor/comment-mark";

/** Where one comment's passage is in the document as it stands. */
export interface CommentPlacement {
  readonly key: string;
  readonly range: TRange;
  /**
   * False when the quote had to be matched approximately — the words under the
   * comment were edited. Drawn differently, because a comment about words that have
   * changed is a weaker claim than a comment about words that have not.
   */
  readonly exact: boolean;
}

export const CommentPlugin = createPlatePlugin({
  key: "comment",
  node: { isLeaf: true, component: CommentMark },
  // Only the placements. Which card is open is a fact about the page rather
  // than about the text, so it lives in the page's own context — putting it here
  // would recompute every decoration in the document each time one was opened.
  options: { placements: [] as readonly CommentPlacement[] },
  decorate: ({ editor, entry, getOptions }) => {
    const [node, path] = entry;
    if (!TextApi.isText(node)) return;

    const { placements } = getOptions();
    if (placements.length === 0) return;

    const own = editor.api.range(path);
    if (!own) return;

    const drawn: DecoratedRange[] = [];
    for (const placement of placements) {
      const hit = RangeApi.intersection(placement.range, own);
      if (!hit) continue;
      // Two comments on overlapping passages leave one leaf carrying both
      // decorations, and the key of the later one wins the click. The list in
      // the context column is where every comment of a passage is reachable, which
      // is why that is a wart rather than a hole.
      // Cast once, and here. Slate types a decoration as a range and nothing
      // else: what a decoration carries into a leaf is the application's own,
      // and naming these three in the library's `Range` would augment the type
      // for every other consumer of it in order to describe this one plugin.
      drawn.push({
        ...hit,
        comment: true,
        commentKey: placement.key,
        commentExact: placement.exact,
      } as DecoratedRange);
    }

    return drawn;
  },
});
