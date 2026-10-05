"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import { useFocusReturn } from "@/lib/use-focus-return";

import {
  attachFolder,
  countRecordsOfKind,
  createMemoryDocument,
  createMemoryType,
  deleteMemoryDocuments,
  deleteMemoryType,
  documentDependents,
  isMemoryFailure,
  loadRecords,
  memoryTypes,
  openMemory,
  resolveUnmatched,
  scanFolders,
  updateMemoryType,
  type MemorySelection,
  type TypeDefinition,
} from "@/lib/memory/client";
import type {
  Dependents,
  MemoryCounts,
  MemoryDocument,
  MemoryRecord,
  MemoryType,
  MemoryView,
  ScanChange,
  ScanOutcome,
} from "@/lib/memory/types";

/**
 * The project's corpus, read from its own memory.
 *
 * This is the host's, not any one extension's. Every extension reads and writes
 * records through it, and what differs between them is only which selection
 * they ask for — a kind of their own, a freshness, the whole store. A screen
 * that listed decisions and a screen that listed review findings would
 * otherwise each grow their own copy of this, and the second copy is where the
 * two would start disagreeing about what "loading" means.
 *
 * Two questions, asked separately because they change at different rates. The
 * types are the project's schema: they are read when the project opens and
 * change only when something publishes a different corpus. The view is what the
 * selection currently holds, and is re-read whenever the selection moves.
 *
 * Opening the memory is part of the first read. `memory_open` publishes the
 * type corpus, and the engine validates every write against it — so a project
 * carried to a machine running a newer Sync has its definitions brought up to
 * date by the act of looking at it.
 */

/** The states that mean a claim stopped matching the code. */
export const ATTENTION_STATES = ["stale", "invalid"] as const;

/**
 * What a kind is called, wherever one is shown.
 *
 * Every column that names a type goes through here, so the window has one
 * answer rather than one per surface. A kind the corpus no longer defines is
 * shown as the identifier itself: a record of a type the project has removed
 * still says what it was written as, and inventing a name for it would be the
 * window making something up.
 */
export function typeName(types: readonly MemoryType[], kind: string): string {
  return types.find((type) => type.kind === kind)?.title ?? kind;
}

/**
 * How much of a selection is read at once.
 *
 * The engine's own ceiling, and the reason reading a selection is a sequence of
 * reads rather than one. A caller asking for more than this would be answered
 * with this anyway, so it is what a page is here whatever was asked for — which
 * is what makes where the next page starts arithmetic rather than a second
 * question for the store.
 */
export const PAGE_LIMIT = 200;

export interface Corpus {
  /**
   * The revision everything here was read at: a commit on the project's memory
   * refs, which is a fact about the store rather than about the code branch.
   */
  readonly revision: string | null;
  /**
   * Every type the project holds, including the ones this window is not
   * listing: the filter that hides them has to offer them back.
   */
  readonly types: readonly MemoryType[];
  /** Counts over the whole corpus, not over the page. */
  readonly counts: MemoryCounts;
  /** The rows of the current selection that have been read so far. */
  readonly records: readonly MemoryRecord[];
  /**
   * How many rows the selection holds, read or not.
   *
   * What a header says, and it says it from the first page: a number that grew
   * as somebody scrolled would be the window reporting its own progress as a
   * fact about the project. It is the store's answer rather than the length of
   * anything here, which is what makes it agree with the count on the row in
   * the navigator that this selection was reached from.
   */
  readonly total: number;
  /** True when the selection holds more rows than have been read. */
  readonly hasMore: boolean;
  /**
   * The kinds left out of all of this. Echoed back because a column showing
   * nothing has to be able to say whether that is the project's answer or its
   * own filter's.
   */
  readonly hidden: readonly string[];
  /** True while the store has not yet answered for this selection at all. */
  readonly isLoading: boolean;
  /**
   * True while a further page of the selection already on screen is being read.
   *
   * Separate from `isLoading`, because they are two different states to draw:
   * one is a column with nothing in it, the other is a list somebody is reading
   * down while the rest of it arrives.
   */
  readonly isReadingMore: boolean;
  /**
   * Read the next page onto the end of what is held.
   *
   * Adds; it never replaces. Somebody reaching the end of the list is still
   * reading it, and rows arriving above where they are looking would move the
   * thing they were about to click. Asking when the store has already said the
   * selection is whole does nothing, so a list may ask as often as it likes.
   */
  readonly readMore: () => void;
  /**
   * Why memory could not be read, in words, or `null`.
   *
   * An empty project and an unreachable engine are different answers, and the
   * column says which one it got instead of showing an empty list for both.
   */
  readonly error: string | null;
  readonly reload: () => void;
  /**
   * Add a type to the project's corpus. Rejects with the engine's own words, so
   * the form that asked can say what went wrong where it was asked.
   */
  readonly createType: (type: TypeDefinition) => Promise<void>;
  /**
   * Redefine a type the project holds. The kind names which one and does not
   * change: it is what every record of the type carries, and the store has no
   * rename.
   */
  readonly updateType: (type: TypeDefinition) => Promise<void>;
  /**
   * Remove a type and every record written as it, answering with how many went.
   * Everything else the column shows is re-read: this is the one write here
   * that changes the counts as well as the corpus.
   */
  readonly deleteType: (kind: string) => Promise<number>;
  /**
   * How many records one type holds, asked of the store. What a confirmation
   * needs before it can name a number it is about to destroy.
   */
  readonly countRecords: (kind: string) => Promise<number>;
  /**
   * Create an empty record of one of the project's types and answer with it.
   *
   * The title is left empty: the record is about to be opened with the caret in
   * its title field, and a stored "Untitled" would be a word somebody has to
   * delete before they can write their own.
   *
   * `folder` absent files it where the type does by default — the root of its
   * storage, or no folder at all for a type whose documents are its records.
   * Somebody looking at a folder means that folder, and a record that appeared
   * somewhere else would be the window ignoring where they were standing.
   */
  readonly createRecord: (kind: string, folder?: string) => Promise<MemoryDocument>;
  /**
   * Delete records, all of them or none. Everything the column shows is re-read
   * afterwards, because the counts and the page both described a corpus that no
   * longer exists.
   */
  readonly deleteRecords: (keys: readonly string[]) => Promise<void>;
  /** What holds on to a record: what links to it, and what mentions it. */
  readonly dependentsOf: (key: string) => Promise<Dependents>;
  /**
   * Files the last scan could not attribute to a record, each carrying the
   * records it could be.
   *
   * The one part of an attached folder that cannot be settled without a person.
   * `UnmatchedFiles` states why, beside the question it asks.
   */
  readonly unmatched: readonly ScanChange[];
  /**
   * Answer one of them. `adopt` names the record the file turned out to be —
   * the record keeps its key, so every link pointing at it survives — and
   * omitting it says the file is a document in its own right.
   */
  readonly resolveUnmatched: (file: ScanChange, kind: string, adopt?: string) => Promise<void>;
}

/**
 * One answer, and the question it answers.
 *
 * The key is what makes "still loading" a derived fact rather than a flag that
 * has to be set and cleared: as long as the answer in hand was read for a
 * different selection, the column is waiting.
 */
interface Answer {
  readonly key: string;
  /** How many pages of that question are in `records`. */
  readonly pages: number;
  readonly revision: string | null;
  readonly counts: MemoryCounts;
  readonly total: number;
  readonly records: readonly MemoryRecord[];
  readonly hasMore: boolean;
  readonly error: string | null;
}

const NOTHING: Omit<Answer, "key"> = {
  pages: 0,
  revision: null,
  counts: { total: 0, byKind: {}, byFreshness: {} },
  total: 0,
  records: [],
  hasMore: false,
  error: null,
};

/**
 * @param active False while the area holding this is mounted but not selected.
 *   Such an area is frozen rather than torn down: it stops reading the store
 *   and stops watching for the window regaining focus, and goes on holding what
 *   it last read. Without this, ten installed areas would be ten scans of the
 *   working tree every time somebody switches back to the application — the
 *   cost of keeping state would exceed what keeping it is worth.
 */
export function useCorpus(
  projectPath: string,
  selection: MemorySelection = {},
  hidden: readonly string[] = [],
  active = true,
): Corpus {
  const [types, setTypes] = useState<readonly MemoryType[]>([]);
  const [typesError, setTypesError] = useState<string | null>(null);
  const [answer, setAnswer] = useState<Answer>({ key: "", ...NOTHING });
  const [attempt, setAttempt] = useState(0);
  /**
   * What is in hand, mirrored where the read can see it.
   *
   * The read is a loop over pages and has to know where the last one stopped;
   * taking that from the state it is itself setting would make the effect
   * depend on its own result and run again after every page. A ref is the
   * ordinary answer to that, and it is exactly as authoritative — nothing else
   * writes it, and it is written in the same breath as the state.
   */
  const held = useRef<{
    question: string;
    key: string;
    pages: number;
    hasMore: boolean;
    records: readonly MemoryRecord[];
  }>({ question: "", key: "", pages: 0, hasMore: false, records: [] });
  // What the last scan could not decide. Held here rather than derived from the
  // corpus because it is not in the corpus: a file nothing could be matched to
  // has no record, which is precisely the state somebody has to resolve.
  const [unmatched, setUnmatched] = useState<readonly ScanChange[]>([]);

  const reload = useCallback(() => setAttempt((count) => count + 1), []);

  const rememberQuestions = useCallback((scan: ScanOutcome) => {
    setUnmatched(scan.changes.filter((change) => change.change === "unmatched"));
    return scan;
  }, []);

  const rescan = useCallback(async () => {
    try {
      const scan = rememberQuestions(await scanFolders(projectPath));
      // A scan that wrote something changed the corpus under everything on
      // screen. One that only found a question did not, and re-reading for it
      // would redraw the column to show the same rows.
      if (scan.applied > 0) reload();
    } catch {
      // A folder that cannot be scanned is not a reason to stop showing the
      // corpus: everything except the bodies of its documents is still true,
      // and the engine reports the folder's own trouble through `doctor`.
    }
  }, [rememberQuestions, projectPath, reload]);

  const resolve = useCallback(
    async (file: ScanChange, kind: string, adopt?: string) => {
      // Thrown rather than returned: the row that asked has already put itself
      // in its working state, and a silent return leaves that row waiting on an
      // answer that will never come. A scan change without these is the engine
      // contradicting itself, which is worth saying out loud.
      if (file.locator === undefined || file.contentHash === undefined) {
        throw new Error(
          "The scan reported a file with no path or no digest, so there is nothing to write.",
        );
      }
      rememberQuestions(
        await resolveUnmatched(
          projectPath,
          {
            locator: file.locator,
            contentHash: file.contentHash,
            kind,
          },
          adopt,
        ),
      );
      reload();
    },
    [rememberQuestions, projectPath, reload],
  );

  const createType = useCallback(
    async (type: TypeDefinition) => {
      // Where the documents live decides which write this is. A type whose
      // bodies are its records is created empty — the command answers with the
      // corpus as it now stands, so the list is replaced rather than re-read,
      // and the counts are untouched because a type created a moment ago has
      // nothing in it.
      //
      // A type over a folder of the repository is the opposite: the documents
      // already exist, so creating it declares a storage, defines the type
      // *and* scans the folder, and everything on screen describes a corpus
      // from before that.
      const folder = type.storage?.folder ?? "";
      if (folder !== "") {
        const { types: published, scan } = await attachFolder(projectPath, {
          kind: type.kind,
          title: type.title,
          description: type.description,
          icon: type.icon,
          folder,
        });
        setTypes(published);
        setTypesError(null);
        rememberQuestions(scan);
        reload();
        return;
      }
      setTypes(await createMemoryType(projectPath, type));
      setTypesError(null);
    },
    [rememberQuestions, projectPath, reload],
  );

  const updateType = useCallback(
    async (type: TypeDefinition) => {
      // A definition changed, and nothing else did: the records of the type are
      // untouched, so the counts and the page in hand are still true.
      setTypes(await updateMemoryType(projectPath, type));
      setTypesError(null);
    },
    [projectPath],
  );

  const deleteType = useCallback(
    async (kind: string) => {
      const { types: remaining, removed } = await deleteMemoryType(projectPath, kind);
      setTypes(remaining);
      setTypesError(null);
      // Records went with it, so the counts and the current page describe a
      // corpus that no longer exists. This is the one type write that has to
      // ask the store everything again.
      reload();
      return removed;
    },
    [projectPath, reload],
  );

  const countRecords = useCallback(
    (kind: string) => countRecordsOfKind(projectPath, kind),
    [projectPath],
  );

  const createRecord = useCallback(
    async (kind: string, folder?: string) => {
      const created = await createMemoryDocument(projectPath, kind, "", folder);
      // One more record of one kind: the counts and the page both moved, and the
      // store is the only thing that knows what they are now.
      reload();
      return created;
    },
    [projectPath, reload],
  );

  const deleteRecords = useCallback(
    async (keys: readonly string[]) => {
      await deleteMemoryDocuments(projectPath, keys);
      reload();
    },
    [projectPath, reload],
  );

  const dependentsOf = useCallback(
    (key: string) => documentDependents(projectPath, key),
    [projectPath],
  );

  // The selection, encoded, because the caller builds a fresh object every
  // render and depending on the object itself would re-read the store on every
  // render. Normalised first so that two selections meaning the same thing are
  // the same string, and parsed back inside the effect — the effect then
  // depends on the encoding rather than on an identity that never repeats.
  const selectionKey = JSON.stringify(normalise(selection));
  // Sorted, so that hiding A and then B asks the same question as hiding B
  // and then A, instead of throwing away an answer for a reordering. Encoded
  // rather than joined: a kind name is whatever the store spells it, spaces
  // included, and a separator it could contain is a separator that will
  // eventually split one kind into two.
  const hiddenKey = JSON.stringify([...hidden].sort());
  // What is being asked, and one read of it. The two are separate because how
  // far down the list somebody has read belongs to the question and not to the
  // read: a write lands, everything is asked again, and they are still reading
  // the same list at the same depth. Only a different question starts over.
  const question = `${projectPath} ${selectionKey} ${hiddenKey}`;
  const key = `${question} ${attempt}`;

  // How many pages of the question have been asked for. Kept per question, so
  // choosing a different type starts at one page again rather than reading
  // three of a list nobody has scrolled.
  const [asked, setAsked] = useState<{ question: string; pages: number }>({
    question: "",
    pages: 1,
  });
  const wanted = asked.question === question ? asked.pages : 1;

  const readMore = useCallback(() => {
    setAsked((standing) => {
      // Nothing to add to: a different question is in hand, or the store has
      // already said this one is whole. Both are ordinary — a list asks
      // whenever its end is on the screen, and its end is on the screen for as
      // long as somebody sits at the bottom of a list that is finished.
      if (held.current.question !== question || !held.current.hasMore) {
        return standing;
      }
      const next = held.current.pages + 1;
      return standing.question === question && standing.pages >= next
        ? standing
        : { question, pages: next };
    });
  }, [question]);

  // The types the project holds, read once per project rather than on every
  // selection change or tab switch.
  //
  // Types change in two ways this window causes, and neither needs a re-read
  // here: a type write returns the fresh corpus and `setTypes` is called
  // directly from `createType`/`updateType`/`deleteType`. What is deliberately
  // not in the deps is `attempt` — it bumps on every record write, and a record
  // write does not move the types, so wiring it here made every created or
  // deleted document re-read the type list for nothing. `active` is in the deps
  // but gated by `typesLoadedFor`, so coming back to a project tab does not
  // re-read types that have not changed.
  const typesLoadedFor = useRef<string | null>(null);

  useEffect(() => {
    if (!active) return;
    if (typesLoadedFor.current === projectPath) return;
    let current = true;

    void (async () => {
      try {
        await openMemory(projectPath);
        const published = await memoryTypes(projectPath);
        if (!current) return;
        setTypes(published);
        setTypesError(null);
        typesLoadedFor.current = projectPath;
      } catch (failure) {
        if (!current) return;
        setTypes([]);
        setTypesError(explain(failure));
      }
    })();

    return () => {
      current = false;
    };
  }, [projectPath, active]);

  // Attached folders are reconciled when the project opens and whenever this
  // window comes back to the front. The second is the one that matters in
  // practice: somebody editing `setup.md` in their editor and switching
  // back expects Sync to have noticed, and `HEAD` moving is not what happened.
  //
  // Deliberately not on every read. The engine says so, and a scan walks a
  // directory: paying for it before each listing would make the column slower
  // the more documentation a project has.
  useEffect(() => {
    if (!active) return;

    // Wrapped rather than called outright: a scan walks the working tree and
    // answers later, which is what an effect is allowed to start — and what
    // distinguishes it from setting state as this render's conclusion.
    void (async () => {
      await rescan();
    })();
  }, [rescan, active]);

  useFocusReturn(() => {
    if (!active) return;
    void rescan();
  });

  // The selection, read a page at a time until as much of it as was asked for
  // is in hand.
  //
  // Sequential rather than at once, and that is not caution about the engine:
  // each page says whether there is another, so page three is a question only
  // page two can say is worth asking. A list of exactly two hundred records
  // would otherwise cost a second read that answers nothing.
  useEffect(() => {
    if (!active) return;
    // Already answered, or answered as far as the store goes. Without this the
    // effect would re-read the whole selection every time anything above it
    // rendered, because `wanted` and `key` are both unchanged by that.
    if (held.current.key === key && (held.current.pages >= wanted || !held.current.hasMore)) {
      return;
    }
    let current = true;

    // One page's worth of answer, put up as this hook's whole state. Written
    // out here because it is done from two places — after each page, and once
    // at the end for a re-read — and the two must not drift.
    const put = (pages: number, records: readonly MemoryRecord[], view: MemoryView) =>
      setAnswer({
        key,
        pages,
        revision: view.revision,
        counts: view.counts,
        // An engine older than this field states no total, and a member the
        // reader does not know is dropped on the way across rather than
        // refused — so what arrives is nothing at all, and a header drawn from
        // it would print the word `undefined` where a number belongs. The rows
        // in hand are the only count there is then. It is the one this header
        // showed before the store could answer the question, and reading the
        // list to its end still lands it on the truth.
        total: view.total ?? records.length,
        records,
        hasMore: view.hasMore,
        error: null,
      });

    void (async () => {
      const selection = JSON.parse(selectionKey) as MemorySelection;
      const kinds = JSON.parse(hiddenKey) as string[];
      // The engine will not answer with more than its own ceiling, so that is
      // what a page is here whatever the caller asked for — and where the next
      // one starts is then arithmetic rather than a member the store has to
      // send back.
      const size = Math.min(selection.limit ?? PAGE_LIMIT, PAGE_LIMIT);
      const first = selection.offset ?? 0;

      // Carrying on with a read already under way, rather than starting one.
      const carrying = held.current.key === key;
      // The same question, asked again: a write landed, or the window came
      // back. What is on screen is still what is being read, so the pages are
      // gathered and put up in one go — published as they arrived, the list
      // would shrink to its first page and grow back under somebody's eyes,
      // taking their place in it with it.
      const quietly = !carrying && held.current.question === question;

      let page = carrying ? held.current.pages : 0;
      let rows: readonly MemoryRecord[] = carrying ? held.current.records : [];
      let last: MemoryView | null = null;

      while (page < wanted) {
        let view: MemoryView;
        try {
          view = await loadRecords(
            projectPath,
            { ...selection, limit: size, offset: first + page * size },
            kinds,
          );
        } catch (failure) {
          if (!current) return;
          held.current = {
            question,
            key,
            pages: 0,
            hasMore: false,
            records: [],
          };
          setAnswer({ key, ...NOTHING, error: explain(failure) });
          return;
        }
        if (!current) return;

        rows = [...rows, ...view.records];
        last = view;
        page += 1;
        held.current = {
          question,
          key,
          pages: page,
          hasMore: view.hasMore,
          records: rows,
        };
        if (!quietly) put(page, rows, view);
        // The store has said this is the whole of it. Asking for the page after
        // it would be one read for no rows and a second chance to disagree
        // about how long the list is.
        if (!view.hasMore) break;
      }

      if (quietly && last !== null) put(page, rows, last);
    })();

    return () => {
      current = false;
    };
  }, [key, question, wanted, projectPath, selectionKey, hiddenKey, active]);

  return {
    revision: answer.revision,
    unmatched,
    resolveUnmatched: resolve,
    types,
    counts: answer.counts,
    records: answer.records,
    total: answer.total,
    hasMore: answer.hasMore,
    hidden,
    isLoading: answer.key !== key,
    // Only ever true of a list that is already on screen: while the first page
    // is in flight the answer in hand is for a different question, and that is
    // `isLoading` above.
    isReadingMore: answer.key === key && answer.hasMore && answer.pages < wanted,
    readMore,
    error: typesError ?? answer.error,
    reload,
    createType,
    updateType,
    deleteType,
    countRecords,
    createRecord,
    deleteRecords,
    dependentsOf,
  };
}

/**
 * One selection, spelled one way.
 *
 * Members in a fixed order and freshness sorted, so that asking for the same
 * thing twice produces the same string and the second ask is answered from what
 * is already in hand. Written out member by member rather than spread: the
 * order of keys is what makes the encoding stable, and a spread would hand that
 * to whoever built the object.
 */
function normalise(selection: MemorySelection): MemorySelection {
  const query: MemorySelection = {};
  if (selection.kind !== undefined) query.kind = selection.kind;
  if (selection.freshness !== undefined) {
    query.freshness = [...selection.freshness].sort();
  }
  // Every member the selection can carry has to be written out here, and the
  // folder is the one where forgetting is silent rather than loud: the encoding
  // is also this hook's cache key, so a dropped member does not merely stop
  // filtering — it makes two different folders one question, and the second one
  // asked is answered with the first one's records.
  if (selection.folder !== undefined) query.folder = selection.folder;
  if (selection.folderScope !== undefined) {
    query.folderScope = selection.folderScope;
  }
  // Sorted for the same reason freshness is, and with one extra consequence:
  // the engine intersects tags, so the order they were ticked in cannot change
  // the answer — and two people arriving at the same pair of tags from opposite
  // ends are then asking one question rather than two.
  if (selection.tags !== undefined) {
    query.tags = [...selection.tags].sort();
  }
  // Sorted, so that asking for status and then priority is the same question as
  // asking for priority and then status rather than a second read of the same
  // records. The same rule freshness above keeps, and for the same reason: this
  // encoding is the cache key.
  if (selection.fields !== undefined) {
    query.fields = [...selection.fields].sort();
  }
  if (selection.limit !== undefined) query.limit = selection.limit;
  if (selection.offset !== undefined) query.offset = selection.offset;
  return query;
}

/**
 * A failure in words a person can act on.
 *
 * The engine's `kind` is stable vocabulary, and the two states worth naming here
 * are the ones a person can do something about; everything else is reported in
 * the engine's own message rather than flattened into "something went wrong".
 *
 * A failure that is not the engine's is reported as it arrived, whatever shape
 * it has. Tauri rejects an unknown command with a plain string — which is what a
 * window running against an application binary older than itself gets, and it is
 * exactly the case where a generic sentence would waste somebody's afternoon.
 */
export function explain(failure: unknown): string {
  if (isMemoryFailure(failure)) {
    if (failure.kind === "sidecar") {
      return `The memory engine is not running: ${failure.message}`;
    }
    return failure.message;
  }
  if (failure instanceof Error) return failure.message;
  if (typeof failure === "string" && failure.trim() !== "") return failure;
  return "The project's memory did not answer.";
}
