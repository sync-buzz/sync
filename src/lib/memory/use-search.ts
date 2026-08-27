"use client";

import { useEffect, useState } from "react";

import { search } from "@/lib/memory/client";
import type { SearchHit } from "@/lib/memory/types";
import { explain } from "@/lib/memory/use-corpus";

/**
 * The corpus, asked a question.
 *
 * One hook for the whole of searching, shaped like `useCorpus` and for the same
 * reason: what is being asked is a value, the answer is state derived from it,
 * and whether the store is still thinking is the two of them disagreeing rather
 * than a flag somebody has to remember to clear.
 *
 * It never says how the store answered — the engine does. `mode` is `hybrid`
 * when vectors contributed and `fts` when BM25 answered alone, and `degraded`
 * is true only when no embedding model is installed at all. That is a normal
 * state of a normal installation, so what shows this states it plainly instead
 * of apologising for it.
 */

/** How many hits are read at once. A palette is a shortlist, not a report. */
export const SEARCH_LIMIT = 40;

/**
 * How long a person stops typing before the store is asked.
 *
 * Short enough that the list appears to follow the keys, long enough that
 * typing a word is one search rather than one per letter — each of which walks
 * an index and, on a project with vectors, embeds the query.
 */
const SETTLE_MS = 140;

export interface SearchAnswer {
  readonly hits: readonly SearchHit[];
  /** How many records the store holds for this question, page or no page. */
  readonly total: number;
  /** True when `total` is a floor: the store stopped counting at a thousand. */
  readonly totalCapped: boolean;
  readonly hasMore: boolean;
  /** `fts` when BM25 answered alone, `hybrid` when vectors contributed. */
  readonly mode: "fts" | "hybrid";
  /** True when there is no embedding model at all: words only, nothing broken. */
  readonly degraded: boolean;
  /** True while the answer in hand was read for a different question. */
  readonly isSearching: boolean;
  /** Why the store could not answer, in words, or `null`. */
  readonly error: string | null;
}

interface Answer extends Omit<SearchAnswer, "isSearching"> {
  /** The question this answers. */
  readonly key: string;
}

const NOTHING: Omit<Answer, "key"> = {
  hits: [],
  total: 0,
  totalCapped: false,
  hasMore: false,
  mode: "fts",
  degraded: false,
  error: null,
};

/**
 * @param kinds The types to search, or empty for all of them. The engine takes
 *   the set in one request, so the answer is one ranking rather than several
 *   that cannot be compared with each other — and the total describes the
 *   narrowed corpus rather than a page somebody filtered afterwards.
 * @param active False while nothing is asking. A closed palette holds its last
 *   answer and stops reading, the way an unselected area does.
 */
export function useSearch(
  projectPath: string,
  query: string,
  kinds: readonly string[] = [],
  active = true,
): SearchAnswer {
  const asked = query.trim();
  // Sorted, so that ticking two types in either order is one question and the
  // second ordering is answered from what is already in hand.
  const narrowed = [...kinds].sort().join(",");
  const key = `${projectPath} ${narrowed} ${asked}`;
  const [answer, setAnswer] = useState<Answer>({ key: "", ...NOTHING });

  useEffect(() => {
    // An empty question is not a search that found nothing, so the store is
    // never asked one: it is answered below, from nothing, rather than by
    // ranking the whole corpus against no words.
    if (!active || asked === "") return;

    let current = true;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const outcome = await search(projectPath, {
            query: asked,
            limit: SEARCH_LIMIT,
            ...(narrowed === "" ? {} : { kinds: narrowed.split(",") }),
          });
          if (!current) return;
          setAnswer({
            key,
            hits: outcome.hits,
            total: outcome.total,
            totalCapped: outcome.total_capped,
            hasMore: outcome.has_more,
            mode: outcome.mode,
            degraded: outcome.degraded,
            error: null,
          });
        } catch (failure) {
          if (!current) return;
          setAnswer({ key, ...NOTHING, error: explain(failure) });
        }
      })();
    }, SETTLE_MS);

    return () => {
      // Both halves: the timer for a search not yet started, the flag for one
      // already in flight whose answer would describe an older question.
      current = false;
      clearTimeout(timer);
    };
  }, [key, projectPath, asked, narrowed, active]);

  // Nothing asked is nothing held. Derived rather than stored, so the palette
  // emptying itself is a render and not a round trip through state.
  if (asked === "") return { ...NOTHING, isSearching: false };

  return { ...answer, isSearching: answer.key !== key };
}
