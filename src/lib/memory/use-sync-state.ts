"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import {
  fetchMemory,
  memoryStatus,
  pushMemory,
  rewindMemory,
  setMemoryRemote,
  syncState,
} from "@/lib/memory/client";
import type { Overlap, SyncState, TransportState } from "@/lib/memory/types";

/**
 * Whether the project's memory is in step with its remote.
 *
 * Two reads rather than one when a project opens, and the split is the point:
 * the count of unpublished records is computed locally and arrives at once,
 * while asking the remote is a network call that may take seconds or time out.
 * One read would have made the whole answer wait for the slowest half of it,
 * and the half worth having immediately is the one about your own writing.
 *
 * There is no live signal to hang this on. The engine knows when a revision
 * moves, but nothing carries that to the window, so the count is as of the last
 * read: when the project opened, when the window was returned to, and after an
 * exchange. A record written and not yet counted is the one inaccuracy here,
 * and it resolves the next time any of those happens.
 */
export interface SyncStatus {
  /** `null` until the first answer, which is not the same as "nothing to say". */
  readonly state: SyncState | null;
  /** The memory remote, and the code origin that can be offered in its place. */
  readonly transport: TransportState | null;
  /** An exchange in progress, which is the one thing an ellipsis means here. */
  readonly busy: Exchange | null;
  /**
   * What the store refused, in its own words, or `null`.
   *
   * Held rather than thrown: a refused exchange is reported where it was asked
   * for, and a command that quietly did nothing would be worse than one that
   * failed.
   */
  readonly error: string | null;
  /**
   * What the last fetch merged over, or `null`.
   *
   * Records where both sides had moved the same thing, and this side's version
   * was kept. Held rather than announced in passing: nothing was lost — the
   * other version is still a commit — but a person whose colleague's sentence
   * quietly vanished is owed the news, and the sheet is where it is read.
   */
  readonly overlaps: readonly Overlap[];
  /**
   * Where memory stood before the last fetch, while going back there is still
   * something anybody would mean.
   *
   * `null` once it has been used, once something has been written since, or
   * when no fetch has landed. Held rather than derived because after the merge
   * nothing else knows it: the revision it names is only reachable through
   * what that fetch reported.
   */
  readonly undoable: Undoable | null;
  /** Put memory back where the last fetch found it. */
  readonly undoFetch: () => void;
  /**
   * Read it again. `askRemote` lets the read touch the network; without it the
   * answer is local and says the remote was not asked.
   */
  readonly refresh: (askRemote?: boolean) => void;
  /** Bring what is on the remote here and merge it. */
  readonly fetchNow: () => void;
  /** Put what is here on the remote. */
  readonly publishNow: () => void;
  /** Point this project's memory at a remote. */
  readonly setRemote: (url: string) => Promise<void>;
  readonly dismissError: () => void;
  /** The overlap has been read, so the header stops carrying it. */
  readonly acknowledgeOverlaps: () => void;
}

/**
 * A fetch that can still be undone.
 *
 * Two revisions, not one. `to` is where memory stood before the merge, and
 * `from` is where the merge left it — which the engine checks against the tip,
 * so an undo asked for after somebody has written refuses rather than carrying
 * their record away.
 */
export interface Undoable {
  readonly to: string;
  readonly from: string;
}

/** Which direction is in flight. */
export type Exchange = "fetching" | "publishing";

export function useSyncState(projectPath: string): SyncStatus {
  const [state, setState] = useState<SyncState | null>(null);
  const [transport, setTransport] = useState<TransportState | null>(null);
  const [busy, setBusy] = useState<Exchange | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [overlaps, setOverlaps] = useState<readonly Overlap[]>([]);
  const [undoable, setUndoable] = useState<Undoable | null>(null);

  /**
   * Which project the answers on screen are about.
   *
   * The window is not remounted when somebody switches project, so without
   * this the previous project's count, remote and overlaps stay in the header
   * while the new one is being read — and an answer already in flight for the
   * old project lands on the new one as though it were about it. Every write
   * below is gated on this still naming the project it was asked for.
   */
  const asked = useRef(projectPath);
  const [about, setAbout] = useState(projectPath);
  if (about !== projectPath) {
    // Cleared while rendering rather than from an effect. An effect would run
    // *after* a frame in which the header still showed the previous project's
    // count and overlaps as though they were this one's — and that frame is the
    // whole of what this is for. React discards this render and starts again
    // with the new values, which is what the pattern is for.
    setAbout(projectPath);
    // `null` is what this hook says before its first answer, and that is
    // exactly the truth about a project it has not read yet.
    setState(null);
    setTransport(null);
    setBusy(null);
    setError(null);
    setOverlaps([]);
    setUndoable(null);
  }
  // The same fact where an answer arriving later can read it. What is on screen
  // is cleared above, synchronously; this is what stops a read already in
  // flight for the previous project from filling the gap with its answer.
  useEffect(() => {
    asked.current = projectPath;
  }, [projectPath]);

  const refresh = useCallback(
    (askRemote = false) => {
      const about = projectPath;
      const still = <T,>(apply: (value: T) => void) => (value: T) => {
        if (asked.current === about) apply(value);
      };
      // A status query that failed is not worth a message of its own: the
      // window is not broken, it simply has nothing new to say about the
      // remote. What is genuinely wrong — an unreachable remote — arrives as
      // an answer rather than as a rejection.
      void syncState(projectPath, askRemote).then(still(setState), () => undefined);
      void memoryStatus(projectPath).then(
        still((status: Awaited<ReturnType<typeof memoryStatus>>) =>
          setTransport(status.transport),
        ),
        () => undefined,
      );
    },
    [projectPath],
  );

  /**
   * One exchange at a time, and the state re-read from the store afterwards
   * rather than guessed from what was asked for. A push the policy blocked
   * leaves the count exactly where it was, and a window that had decremented
   * it optimistically would be lying about published work.
   */
  const exchange = useCallback(
    (direction: Exchange, run: () => Promise<unknown>) => {
      const about = projectPath;
      setError(null);
      setBusy(direction);
      // The offer ends with whatever happens next. A merge that has since been
      // published, or fetched over, is not the last thing that happened any
      // more — and `undoFetch` sets it again on its way through, which is the
      // one case where clearing it here is not the answer.
      setUndoable(null);
      void run()
        .then(
          () => undefined,
          (failure: unknown) => {
            if (asked.current === about) setError(messageOf(failure));
          },
        )
        .finally(() => {
          // An exchange that finishes after somebody has moved on says nothing
          // about where they are now, and the project it was about has already
          // been cleared.
          if (asked.current !== about) return;
          setBusy(null);
          refresh(true);
        });
    },
    [projectPath, refresh],
  );

  const fetchNow = useCallback(
    () =>
      exchange("fetching", async () => {
        const about = projectPath;
        const outcome = await fetchMemory(projectPath);
        if (asked.current !== about) return;
        setOverlaps(outcome.overlaps);
        // Only where going back would change something. A fetch that merged
        // nothing leaves memory where it already was, and an undo offered for
        // it would be a button that does nothing to a person who pressed it
        // meaning something.
        // Both revisions or neither: the undo needs somewhere to go back to
        // *and* the tip to check against, and an engine that answered only half
        // of that is one this build cannot safely undo against.
        const { localRevisionBefore: to, localRevisionAfter: from } = outcome;
        setUndoable(
          outcome.merged && to !== null && from !== null ? { to, from } : null,
        );
      }),
    [exchange, projectPath],
  );

  const publishNow = useCallback(
    () => exchange("publishing", () => pushMemory(projectPath)),
    [exchange, projectPath],
  );

  /**
   * Put memory back where the last fetch found it.
   *
   * Offered once and then withdrawn. What it undoes is a merge, and a merge
   * that has since been written on top of is not the last thing that happened
   * any more — going back would take somebody's own writing with it, which is
   * not what "undo the fetch" means to the person pressing it. So the offer
   * ends with the exchange that follows it, whichever direction that is.
   */
  const undoFetch = useCallback(() => {
    if (undoable === null) return;
    exchange("fetching", async () => {
      await rewindMemory(projectPath, undoable.to, undoable.from);
      setUndoable(null);
      setOverlaps([]);
    });
  }, [exchange, projectPath, undoable]);

  const setRemote = useCallback(
    async (url: string) => {
      setError(null);
      const next = await setMemoryRemote(projectPath, url);
      setTransport(next);
      refresh(true);
    },
    [projectPath, refresh],
  );

  const dismissError = useCallback(() => setError(null), []);
  const acknowledgeOverlaps = useCallback(() => setOverlaps([]), []);

  /**
   * What is true when the project opens, and the one fetch that happens by
   * itself.
   *
   * Two reads, because the halves cost different things: the count of
   * unpublished records is local and arrives at once, while asking the remote
   * is a network call that may time out. One read would have made the whole
   * answer wait for the slower half.
   *
   * The automatic fetch is decided where the remote's answer lands rather than
   * by an effect watching the state, so a later render cannot re-trigger it.
   *
   * That it happens only on open is deliberate, and narrower than "fetch
   * whenever something is waiting". A fetch is a write: it merges records into
   * the corpus the columns are already showing, and nothing carries "the
   * corpus changed" to a mounted list — so a fetch landing while somebody is
   * reading would leave them looking at a list that is quietly wrong. When a
   * project opens there is no such list yet, which is what makes this the one
   * moment it is safe to do without asking.
   */
  useEffect(() => {
    const about = projectPath;
    refresh(false);
    void syncState(projectPath, true).then((answer) => {
      if (asked.current !== about) return;
      setState(answer);
      if (answer.remote === "waiting") fetchNow();
    }, () => undefined);
  }, [fetchNow, projectPath, refresh]);

  /**
   * Returning to the window re-asks and does not fetch, for the reason above:
   * the indicator says `Updates waiting`, and taking them is one click away in
   * the sheet. A person who acts on that is looking at the window when the
   * lists change, which is the whole difference.
   *
   * There is no timer either. A background network call nobody asked for is
   * what this application is trying not to be, while somebody coming back to
   * it is an event that already means something.
   */
  useEffect(() => {
    const onFocus = () => refresh(true);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return {
    state,
    transport,
    busy,
    error,
    overlaps,
    undoable,
    undoFetch,
    refresh,
    fetchNow,
    publishNow,
    setRemote,
    dismissError,
    acknowledgeOverlaps,
  };
}

function messageOf(failure: unknown): string {
  return failure instanceof Error
    ? failure.message
    : "The exchange did not happen.";
}
