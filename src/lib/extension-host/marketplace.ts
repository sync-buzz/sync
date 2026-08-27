"use client";

import { useCallback, useEffect, useState } from "react";

import {
  registryIndex,
  type FetchedRegistry,
  type ListedExtension,
} from "@/lib/extension-host/client";

/**
 * What exists anywhere, as the window asks it.
 *
 * The third of the catalogue's questions, and the only one whose answer is not
 * on this machine. The other two — what is unpacked, and what the project
 * declares — are a directory and a record; this one is a file over the network,
 * and everything about *how* it is read was decided in Rust: which hosts, the
 * ETag, the cache, the ceiling on a download. What is left here is when to ask,
 * and what to say while the answer has not come.
 *
 * **Asked once per window, the first time the catalogue is opened.** Not at
 * launch, because somebody who never opens Extensions never asked for a
 * request; not on every visit, because the answer is a file that changes when
 * somebody publishes rather than when somebody looks. There is nothing here
 * that arranges that: an area is mounted the first time it is selected and
 * never unmounted, so *being called at all* is the first visit and staying
 * mounted is every visit after it. Coming back shows what was read, and the
 * control on the marketplace is how a person asks again.
 *
 * **A failure is not an empty marketplace.** Rust falls back to whatever was
 * cached and says so, so the ordinary offline case arrives as a list with
 * [`Marketplace.cached`] set rather than as an error. What reaches
 * [`Marketplace.failure`] is the case where there is no network *and* nothing
 * was ever cached — a first launch without one — and then the honest thing on
 * the page is the network's own words rather than an empty grid.
 *
 * **Asking and remembering are two different things, and only one of them dials
 * out.** The window reads what this left on the disk when a project opens, so
 * that the pinned Extensions row can say something is newer — see
 * `useCachedIndex`. That is a read of what somebody already asked for; this is
 * the asking, and it stays where the person doing it is.
 */
export interface Marketplace {
  /** What the registry lists, and empty until the first answer arrives. */
  readonly listed: readonly ListedExtension[];
  /**
   * True when this is what was cached rather than what the network answered.
   *
   * The difference between *these are the extensions there are* and *these were
   * the extensions when this machine last had a network*, which the page says
   * out loud rather than absorbing.
   */
  readonly cached: boolean;
  /** True until the answer to the current ask has arrived. */
  readonly isLoading: boolean;
  /** Why there is nothing at all, in the network's words, or `null`. */
  readonly failure: string | null;
  /** Ask again. What the control on the marketplace does. */
  readonly reload: () => void;
}

const NOTHING: readonly ListedExtension[] = [];

/** What one ask answered with, and which ask it was. */
interface Answered {
  readonly attempt: number;
  readonly fetched: FetchedRegistry | null;
  readonly failure: string | null;
}

export function useMarketplace(): Marketplace {
  const [attempt, setAttempt] = useState(0);
  const [answered, setAnswered] = useState<Answered | null>(null);

  useEffect(() => {
    let current = true;
    void registryIndex().then(
      (fetched) => {
        if (current) setAnswered({ attempt, fetched, failure: null });
      },
      (refused: unknown) => {
        // Rust has already fallen back to the cache where there was one, so
        // reaching here means there is nothing at all to show.
        if (current) {
          setAnswered({
            attempt,
            fetched: null,
            failure: refused instanceof Error ? refused.message : String(refused),
          });
        }
      },
    );

    return () => {
      current = false;
    };
  }, [attempt]);

  const reload = useCallback(() => setAttempt((one) => one + 1), []);

  return {
    listed: answered?.fetched?.answer.extensions ?? NOTHING,
    cached: answered?.fetched?.cached ?? false,
    // Derived rather than held, so nothing is written to state as this effect
    // starts: an ask is outstanding exactly while the last answer is not this
    // one's.
    isLoading: answered?.attempt !== attempt,
    failure: answered?.failure ?? null,
    reload,
  };
}
