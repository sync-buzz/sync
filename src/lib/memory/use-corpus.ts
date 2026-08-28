"use client";

import { useCallback, useEffect, useState } from "react";

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
export function typeName(
  types: readonly MemoryType[],
  kind: string,
): string {
  return types.find((type) => type.kind === kind)?.title ?? kind;
}

/**
 * How much of a selection is read at once.
 *
 * The engine's own ceiling. Nothing in the column pages yet, so a selection
 * larger than this is reported as having more rather than presented as if this
 * were all of it.
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
  /** The rows of the current selection. */
  readonly records: readonly MemoryRecord[];
  /** True when the selection holds more rows than were read. */
  readonly hasMore: boolean;
  /**
   * The kinds left out of all of this. Echoed back because a column showing
   * nothing has to be able to say whether that is the project's answer or its
   * own filter's.
   */
  readonly hidden: readonly string[];
  /** True while the store has not yet answered for this selection. */
  readonly isLoading: boolean;
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
  readonly createRecord: (
    kind: string,
    folder?: string,
  ) => Promise<MemoryDocument>;
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
  readonly resolveUnmatched: (
    file: ScanChange,
    kind: string,
    adopt?: string,
  ) => Promise<void>;
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
  readonly revision: string | null;
  readonly counts: MemoryCounts;
  readonly records: readonly MemoryRecord[];
  readonly hasMore: boolean;
  readonly error: string | null;
}

const NOTHING: Omit<Answer, "key"> = {
  revision: null,
  counts: { total: 0, byKind: {}, byFreshness: {} },
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
  // What the last scan could not decide. Held here rather than derived from the
  // corpus because it is not in the corpus: a file nothing could be matched to
  // has no record, which is precisely the state somebody has to resolve.
  const [unmatched, setUnmatched] = useState<readonly ScanChange[]>([]);

  const reload = useCallback(() => setAttempt((count) => count + 1), []);

  const rememberQuestions = useCallback((scan: ScanOutcome) => {
    setUnmatched(
      scan.changes.filter((change) => change.change === "unmatched"),
    );
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
      const { types: remaining, removed } = await deleteMemoryType(
        projectPath,
        kind,
      );
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
  const key = `${projectPath} ${selectionKey} ${hiddenKey} ${attempt}`;

  useEffect(() => {
    if (!active) return;
    let current = true;

    void (async () => {
      try {
        await openMemory(projectPath);
        const published = await memoryTypes(projectPath);
        if (!current) return;
        setTypes(published);
        setTypesError(null);
      } catch (failure) {
        if (!current) return;
        setTypes([]);
        setTypesError(explain(failure));
      }
    })();

    return () => {
      current = false;
    };
  }, [projectPath, attempt, active]);

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

    const onFocus = () => void rescan();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [rescan, active]);

  useEffect(() => {
    if (!active) return;
    let current = true;

    void (async () => {
      try {
        const view_ = await loadRecords(
          projectPath,
          JSON.parse(selectionKey) as MemorySelection,
          JSON.parse(hiddenKey) as string[],
        );
        if (!current) return;
        setAnswer({
          key,
          revision: view_.revision,
          counts: view_.counts,
          records: view_.records,
          hasMore: view_.hasMore,
          error: null,
        });
      } catch (failure) {
        if (!current) return;
        setAnswer({ key, ...NOTHING, error: explain(failure) });
      }
    })();

    return () => {
      current = false;
    };
  }, [key, projectPath, selectionKey, hiddenKey, active]);

  return {
    revision: answer.revision,
    unmatched,
    resolveUnmatched: resolve,
    types,
    counts: answer.counts,
    records: answer.records,
    hasMore: answer.hasMore,
    hidden,
    isLoading: answer.key !== key,
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
