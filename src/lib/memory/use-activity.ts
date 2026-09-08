"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { journal, memoryStatus } from "@/lib/memory/client";
import { useMemoryNotice } from "@/lib/memory/use-memory-notice";
import type { JournalChange, JournalEntry } from "@/lib/memory/types";
import { loadProjectView, saveProjectView } from "@/lib/project/client";
import type { DismissedChange } from "@/lib/project/types";

/**
 * What has happened to this project's memory since this person last looked.
 *
 * The engine keeps the history; what this holds is the part of it that is news
 * to somebody at this window. Three things are dropped, and each is dropped for
 * its own reason rather than to make the list shorter:
 *
 * - **Housekeeping.** Publishing a type corpus when a project opens and
 *   reconciling an attached folder are writes nobody performed. Reporting them
 *   would make the first launch of every project a list of things to read.
 * - **This window's own writes.** What somebody just did is not news to them,
 *   for the same reason Mail does not put sent messages in the unread count.
 * - **Changes already put away.** One row at a time, by name — the mark is one
 *   line across the whole history and cannot say *this one*. It is the same
 *   answer as the mark and a finer instrument, which is why both exist: see
 *   `ProjectView.dismissed`.
 * - **Kinds this person switched off.** The one filter that is a preference,
 *   and the one applied to the answer in hand rather than to the read: ticking
 *   a kind back on has to change the list and the count in the same frame, and
 *   a filter applied while reading would leave the two disagreeing until the
 *   next journal arrived. It is held as exceptions rather than as a selection —
 *   see `ProjectView.unwatchedKinds`.
 *
 * What survives is grouped **by record**, not by write. A record edited three
 * times is one thing to go and look at, and a count that said three would be
 * counting the agent's keystrokes rather than the person's work. The entry
 * keeps how many writes it stands for, so the list can say so without the
 * number on the sidebar meaning something different from the rows under it.
 *
 * **Where the reading mark lives.** `seenRevision` is in this installation's
 * own configuration, beside the type filter and the section order. In the
 * project's memory it would be one colleague's reading marking the whole
 * team's — and a change somebody else glanced at would go quiet for everybody.
 */

/** One record that changed, and what happened to it. */
export interface ActivityEntry {
  readonly key: string;
  readonly kind: string;
  /**
   * What it was called at the newest write that touched it.
   *
   * The history carries it, which is the only way a deleted record can be named
   * at all: there is nothing left to read by the time anybody looks.
   */
  readonly title: string | null;
  /** What the newest write did to it. */
  readonly change: JournalChange["change"];
  /** Whose hand the newest write was. */
  readonly source: JournalEntry["source"];
  /** When that write landed, in seconds since the epoch, UTC. */
  readonly at: number;
  /**
   * The revision of that newest write.
   *
   * What a change put away is remembered by, so that the same record written
   * *again* comes back rather than staying quiet — see `ProjectView.dismissed`.
   * A timestamp would not do it: two writes in one second are two revisions and
   * one clock reading.
   */
  readonly revision: string;
  /** How many writes this stands for. One, unless somebody kept working. */
  readonly writes: number;
}

export interface Activity {
  /** Newest first. */
  readonly entries: readonly ActivityEntry[];
  /**
   * True while the first answer for this project is still on its way.
   *
   * A window that drew `0` before asking would say *nothing has happened* to
   * somebody who has twenty things waiting, which is worse than saying nothing
   * at all.
   */
  readonly isLoading: boolean;
  /**
   * The history reached this build's page limit before it reached the revision
   * this person last saw. The list is what fits; there is more behind it.
   */
  readonly hasMore: boolean;
  /**
   * The journal could not be read, so this list says nothing about what has
   * happened — rather than saying that nothing has.
   *
   * The two were one state until an engine that could not answer left a screen
   * reading *no changes since you last looked* for a whole evening. An empty
   * list is a claim about the project; a failed read is a claim about this
   * window, and a screen that cannot tell them apart lies in exactly the case
   * somebody needs the truth.
   */
  readonly failed: boolean;
  /** Everything here has been looked at. Moves the mark, and empties the list. */
  readonly markSeen: () => void;
  /**
   * These have been looked at, and nothing else has.
   *
   * The mark does not move. It is one line across the whole history, so moving
   * it to put away a single change would put away everything older than that
   * change with it — which is the failure that looks like the command working.
   * What is remembered instead is the record and the write, by name.
   */
  readonly dismiss: (keys: readonly string[]) => void;
}

/**
 * How much history one answer carries.
 *
 * The engine clamps at 200 and this asks for the clamp: a person coming back
 * after a fortnight of an agent working is exactly who this is for, and a
 * smaller page would report `hasMore` to them on every ordinary morning.
 */
const PAGE = 200;

const NOTHING: readonly ActivityEntry[] = [];
const NONE_PUT_AWAY: readonly DismissedChange[] = [];

export function useActivity(
  projectPath: string,
  /**
   * Kinds this person does not want reported, as `useWatchedKinds` holds them.
   *
   * Passed in rather than read here, because the sidebar's figure and this
   * screen's list have to agree the instant somebody changes it — one reader of
   * the preference, one answer.
   */
  unwatched: readonly string[],
): Activity {
  /**
   * The answer, and which project it was read for.
   *
   * Held together rather than as separate pieces of state, the way the section
   * order is: switching projects then shows nothing while the new answer is on
   * its way, instead of briefly showing the last project's changes under the
   * new project's name. It is also what keeps this hook from having to reset
   * anything on the way past — a stale answer is one the render can recognise.
   */
  const [answer, setAnswer] = useState<{
    readonly path: string;
    readonly entries: readonly ActivityEntry[];
    readonly hasMore: boolean;
    readonly failed: boolean;
    /**
     * What was put away one row at a time, as the configuration holds it.
     *
     * In here with the list rather than beside it for the same reason
     * everything else is: it is read in the same breath as the list, it is
     * about the same project, and two pieces of state keyed by one path is how
     * a project switch comes to apply one project's answer to the other's.
     */
    readonly dismissed: readonly DismissedChange[];
  }>({
    path: "",
    entries: NOTHING,
    hasMore: false,
    failed: false,
    dismissed: NONE_PUT_AWAY,
  });
  /**
   * The revision the list in hand was read against.
   *
   * Kept so that marking as seen moves the mark to exactly what was shown, and
   * not to where memory stands at the moment of the click: a write can land
   * between the answer and the gesture, and taking the newer revision would
   * silence a change nobody has seen.
   */
  const shown = useRef<string | null>(null);
  /** Which project the read in flight is about, so a switch cannot land late. */
  const asked = useRef("");
  /**
   * What was last put away here, and whether the store has been told yet.
   *
   * A read that lands between putting a row away and the write settling would
   * otherwise hand back the list as it was a moment ago, and the row would
   * reappear for as long as it took the write to finish. So while anything is
   * in flight the list in hand is this window's own — it is the newer of the
   * two by construction, and the store is on its way to agreeing with it.
   */
  const putAway = useRef<{
    readonly list: readonly DismissedChange[];
    readonly writing: number;
  }>({ list: NONE_PUT_AWAY, writing: 0 });

  const read = useCallback(async (path: string) => {
    const view = await loadProjectView(path).catch(() => null);
    if (asked.current !== path) return;

    // Nobody has looked yet. The starting point is where memory stands now,
    // written down before anything is read: the alternative is reporting the
    // whole history of a project as news the first time it is opened.
    if (!view?.seenRevision) {
      const status = await memoryStatus(path);
      if (asked.current !== path) return;
      await saveProjectView(path, { seenRevision: status.revision }).catch(
        () => undefined,
      );
      if (asked.current !== path) return;
      shown.current = status.revision;
      putAway.current = { list: NONE_PUT_AWAY, writing: 0 };
      setAnswer({
        path,
        entries: NOTHING,
        hasMore: false,
        failed: false,
        dismissed: NONE_PUT_AWAY,
      });
      return;
    }

    const journalled = await journal(path, view.seenRevision, PAGE);
    if (asked.current !== path) return;
    shown.current = journalled.toRevision;
    const dismissed =
      putAway.current.writing > 0 ? putAway.current.list : (view.dismissed ?? NONE_PUT_AWAY);
    putAway.current = { ...putAway.current, list: dismissed };
    setAnswer({
      path,
      entries: collapse(journalled.entries),
      hasMore: journalled.hasMore,
      failed: false,
      dismissed,
    });
  }, []);

  /**
   * The same read, with the one answer an engine that will not talk can give.
   *
   * The row on the sidebar still carries no figure — there is nothing to count,
   * and a number nobody can act on is worse than none — but the screen says the
   * reading failed rather than that nothing happened, and the next return to
   * the window tries again. The reason goes to the console, because the
   * sentence a person is shown is deliberately short and whoever is debugging
   * wants more than it.
   */
  const reread = useCallback(
    async (path: string) => {
      try {
        await read(path);
      } catch (error) {
        if (asked.current !== path) return;
        console.warn("The activity could not be read.", error);
        setAnswer({
          path,
          entries: NOTHING,
          hasMore: false,
          failed: true,
          dismissed: NONE_PUT_AWAY,
        });
      }
    },
    [read],
  );

  useEffect(() => {
    asked.current = projectPath;
    // The names go with the project they were read for. A write still in
    // flight belongs to the project it was made in, and a list carried across
    // would suppress rows in the new one by key — a change quietly missing from
    // a screen whose whole job is not to miss one.
    putAway.current = { list: NONE_PUT_AWAY, writing: 0 };
    // And so does the mark, which is worse if it is carried: a revision belongs
    // to one project's memory, so marking the new project as read at the old
    // one's revision writes down a point in its history that does not exist —
    // and every read after it asks what has happened since a revision the
    // engine has never served. The window is left saying it cannot read the
    // history of a project nothing is wrong with.
    shown.current = null;
    void reread(projectPath);
  }, [projectPath, reread]);

  /**
   * The engine saying that something was written asks again, wherever the write
   * came from — an agent's tool, another window, a device. Without it the list
   * was right only as of the last time this person came back to the window,
   * which is the wrong answer for the one they are most likely to be waiting
   * for: an agent working while they watch.
   */
  useMemoryNotice(projectPath, () => {
    void reread(projectPath);
  });

  /**
   * Returning to the window asks again as well, and it is not made redundant by
   * the notice above: a machine with no resident engine has nothing to send one,
   * and a window that was asleep has no channel to have heard it on.
   *
   * There is no timer: a list that changes under somebody reading it is the
   * failure this avoids, and coming back is an event that already means
   * something.
   *
   * Both ways of hearing it, because they hear different things. The DOM's
   * `focus` fires when the *document* takes focus, which is not what happens
   * when somebody switches back to an application whose caret is already in a
   * field — the event goes to that element and does not bubble. The window's
   * own focus is the one that means "this application is in front", and it is
   * where the menu bar already listens for the same reason.
   */
  useEffect(() => {
    const again = () => {
      void reread(projectPath);
    };
    window.addEventListener("focus", again);

    let drop: (() => void) | null = null;
    let dropped = false;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const stop = await getCurrentWindow().onFocusChanged(
          ({ payload: focused }) => {
            if (focused) again();
          },
        );
        // The effect can be cleaned up before this resolves — switching
        // projects is exactly that — and a listener installed afterwards would
        // outlive the hook that asked for it.
        if (dropped) stop();
        else drop = stop;
      } catch {
        // Outside Tauri there is no window to follow. The DOM event above is
        // the whole of what a browser can offer, and it is enough there.
      }
    })();

    return () => {
      dropped = true;
      drop?.();
      window.removeEventListener("focus", again);
    };
  }, [projectPath, reread]);

  const markSeen = useCallback(() => {
    const revision = shown.current;
    if (!revision) return;
    putAway.current = { list: NONE_PUT_AWAY, writing: 0 };
    setAnswer({
      path: projectPath,
      entries: NOTHING,
      hasMore: false,
      failed: false,
      dismissed: NONE_PUT_AWAY,
    });
    // Optimistic, as the type filter's writes are: the column empties at once
    // and the configuration is told afterwards. A mark that could not be
    // written costs one repeated list, and nothing worth interrupting anybody
    // to say.
    //
    // The names go with the mark, in the same write. Everything they stand for
    // is behind the mark now, and a name left past that point would suppress
    // the *next* write to that record — a change nobody would ever be told
    // about, from a list that has no way of saying it is doing it.
    void saveProjectView(projectPath, {
      seenRevision: revision,
      dismissed: [],
    }).catch(() => undefined);
  }, [projectPath]);

  const dismiss = useCallback(
    (keys: readonly string[]) => {
      if (answer.path !== projectPath || keys.length === 0) return;

      const wanted = new Set(keys);
      const added = answer.entries
        .filter((entry) => wanted.has(entry.key))
        .map((entry) => ({ key: entry.key, revision: entry.revision }));
      if (added.length === 0) return;

      // What is already put away, minus anything the list no longer carries.
      //
      // A name that matches nothing in the history since the mark is a name
      // doing no work, and the list is written back whole every time — so
      // without this it would grow for the life of the project. It is only safe
      // where the answer is the whole answer: a truncated page is missing rows
      // that are still ahead of the mark, and pruning against it would put them
      // back on screen at the next read.
      const kept = answer.dismissed.filter(
        (change) =>
          !wanted.has(change.key) &&
          (answer.hasMore ||
            answer.entries.some((entry) => entry.key === change.key)),
      );
      const next = [...kept, ...added];

      putAway.current = {
        list: next,
        writing: putAway.current.writing + 1,
      };
      setAnswer((held) =>
        held.path === projectPath ? { ...held, dismissed: next } : held,
      );
      void saveProjectView(projectPath, { dismissed: next })
        .catch(() => undefined)
        .finally(() => {
          putAway.current = {
            ...putAway.current,
            writing: Math.max(0, putAway.current.writing - 1),
          };
        });
    },
    [answer, projectPath],
  );

  const current = answer.path === projectPath;
  const entries = useMemo(() => {
    if (!current) return NOTHING;
    // By the write and not by the record: a record put away and then written to
    // again is news again, which is the whole difference between this and the
    // mark. Matching on the key alone would silence it for good.
    const away = new Map(
      answer.dismissed.map((change) => [change.key, change.revision]),
    );
    return answer.entries.filter(
      (entry) =>
        !unwatched.includes(entry.kind) && away.get(entry.key) !== entry.revision,
    );
  }, [answer.dismissed, answer.entries, current, unwatched]);

  return {
    entries,
    isLoading: !current,
    hasMore: current && answer.hasMore,
    failed: current && answer.failed,
    markSeen,
    dismiss,
  };
}

/**
 * The writes, as records that changed.
 *
 * Newest first is how the history arrives and how this leaves it, so the first
 * write seen for a record is the newest one — which is why the later ones only
 * add to the count and never overwrite what is shown.
 */
function collapse(entries: readonly JournalEntry[]): readonly ActivityEntry[] {
  const collapsed = new Map<string, ActivityEntry>();
  for (const entry of entries) {
    if (entry.source === "housekeeping" || entry.source === "window") continue;
    for (const change of entry.changes) {
      const seen = collapsed.get(change.key);
      if (seen) {
        collapsed.set(change.key, { ...seen, writes: seen.writes + 1 });
        continue;
      }
      collapsed.set(change.key, {
        key: change.key,
        kind: change.kind,
        title: change.title,
        change: change.change,
        source: entry.source,
        at: entry.at_epoch_seconds,
        revision: entry.revision,
        writes: 1,
      });
    }
  }
  return [...collapsed.values()];
}
