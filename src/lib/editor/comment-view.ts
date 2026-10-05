"use client";

/**
 * How a comment reaches the two places it is read from.
 *
 * A comment appears twice and is stored once. In the text it is a shaded passage
 * with a mark after it, and clicking that raises a card — that is the page's own
 * business, and the first context here carries it. Beside the text it is a row
 * in the context column, which is a different panel in a different part of the
 * tree, and the second context is how the page tells that column what it is
 * holding.
 *
 * The second one is shaped like `TableCommandsProvider` next door, for the same
 * reason: what is known inside the editor has to be acted on outside it, and a
 * panel reaching down into the editor's state would be two components owning one
 * fact. The page reports, whoever owns the area holds, and the column reads.
 */

import { createContext, useContext, useEffect } from "react";

import type { PlacedComment } from "@/lib/editor/comments";

/** What the page lets somebody do to the comment under the pointer. */
export interface CommentCards {
  /** The comment whose card is open, drawn stronger in the text. */
  readonly active: string | null;
  readonly open: (key: string) => void;
  readonly dismiss: () => void;
}

const Cards = createContext<CommentCards>({
  active: null,
  open: () => undefined,
  dismiss: () => undefined,
});

export const CommentCardsProvider = Cards.Provider;

/** Read by the shaded passage in the text, which is four components down. */
export function useCommentCards(): CommentCards {
  return useContext(Cards);
}

/**
 * What the open record's comments are, for the column beside it.
 *
 * `reveal` scrolls the page to a comment's passage and opens its card, which is
 * what a row in that column does when it is chosen. A comment with no passage left
 * has nothing to scroll to, and the row says so instead of moving the page.
 */
export interface CommentsOnPage {
  readonly placed: readonly PlacedComment[];
  readonly active: string | null;
  readonly reveal: (key: string) => void;
  /** Archive it: the comment leaves the text and stays in memory. */
  readonly resolve: (key: string) => void;
}

const Report = createContext<(page: CommentsOnPage | null) => void>(() => undefined);

export const CommentsProvider = Report.Provider;

/**
 * Report this page's comments while it is the page.
 *
 * Two effects rather than one, and the division is what keeps this from looping.
 * Reporting is a write into somebody else's state, so it renders them; if the
 * withdrawal were the same effect's cleanup, every change would report twice —
 * `null`, then the page — and each of those rounds is a render that can produce
 * another page. The withdrawal belongs to leaving, and leaving happens once.
 *
 * Leaving does have to take them with it: a column listing the comments of a record
 * nobody has open would offer to scroll a page that is not there.
 */
export function useReportComments(page: CommentsOnPage | null): void {
  const report = useContext(Report);

  useEffect(() => {
    report(page);
  }, [report, page]);

  useEffect(() => () => report(null), [report]);
}
