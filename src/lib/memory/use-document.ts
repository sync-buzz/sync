"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  isMemoryFailure,
  memoryDocument,
  reconcileMemory,
  updateMemoryDocument,
} from "@/lib/memory/client";
import type {
  DocumentPatch,
  EntityLink,
  MemoryDocument,
} from "@/lib/memory/types";

/**
 * How long typing has to stop before the record is written.
 *
 * Every save is a transaction and a commit on `refs/memory/*`, so this is the
 * difference between a history of what somebody wrote and a history of every
 * keystroke they took to write it. It is long enough to gather a sentence and
 * short enough that "Saved" arrives while the person is still looking at the word
 * they changed.
 */
const SAVE_DELAY_MS = 1200;

/**
 * A record as the window has it: what the store answered, with whatever has been
 * typed into it on top.
 *
 * This is what every control reads, so a tag added a moment ago is on screen
 * before the store has been told about it. It is not what gets written — that is
 * the patch, which carries only what changed.
 */
export interface DocumentDraft {
  readonly title: string;
  readonly content: string;
  readonly tags: readonly string[];
  readonly links: readonly EntityLink[];
  readonly scope: readonly string[];
  readonly observed: readonly string[];
  readonly archived: boolean;
  readonly fields: Readonly<Record<string, unknown>>;
}

/**
 * Where the open record stands with the store.
 *
 * `clean` is the resting state and says nothing on screen: an application that
 * announces "Saved" about a record nobody has touched is describing itself rather
 * than the work.
 */
export type SaveState =
  | { readonly status: "clean" }
  | { readonly status: "edited" }
  | { readonly status: "saving" }
  | { readonly status: "saved" }
  /**
   * The store refused the write.
   *
   * `kind` travels with the message because one refusal is answered by doing
   * something rather than by trying again: code history that was rewritten
   * refuses every write, identically, until it is reconciled — and a view that
   * only had the prose could offer nothing but a retry that cannot work.
   */
  | {
      readonly status: "failed";
      readonly message: string;
      readonly kind: string | null;
    };

/** Nothing has been typed into this record. */
const CLEAN: SaveState = { status: "clean" };
/**
 * What is on screen is what the store holds.
 *
 * Distinct from `clean` because it is worth saying once: every save is a commit
 * on the project's memory, and the person who just typed a sentence into a claim
 * an agent will read is owed the confirmation that it landed.
 */
const SAVED: SaveState = { status: "saved" };

/**
 * The record the window has open, read whole and written back as it is edited.
 *
 * Separate from the list it was opened from: a row carries what a row is scanned
 * for, and the body is read only when somebody asks to read it.
 *
 * Every piece of save state here is stamped with the record it belongs to. The
 * hook outlives the view — the window keeps one of it for the whole project — so
 * a write that lands after another record has been opened must not report itself
 * against the record now on screen.
 */
export interface OpenDocument {
  readonly document: MemoryDocument | null;
  readonly isLoading: boolean;
  /**
   * Why the record could not be read, or `null`. A key that no longer exists is
   * not an error — it comes back as a `null` document, which the view says
   * plainly.
   */
  readonly error: string | null;
  /** The record with the unwritten edits on top, or `null` with no record. */
  readonly draft: DocumentDraft | null;
  readonly save: SaveState;
  /**
   * Change what the patch names. Written after a pause, and on the way out.
   *
   * Everything except the body goes through here, because everything except the
   * body is either short to type or a single choice — and a patch in state is
   * what lets the panel show a tag the moment it is added.
   */
  readonly edit: (patch: DocumentPatch) => void;
  /**
   * Hand over a way to read the body, rather than the body.
   *
   * Serialising a document to Markdown on every keystroke would be work nobody
   * asked for, and putting the result in state would re-render the window around
   * the caret. The reader is kept in a ref and called once, when the write
   * happens.
   */
  readonly editBody: (read: () => string) => void;
  /**
   * Write what is waiting, now, and resolve when the store has answered.
   *
   * Awaited by whoever is leaving the record: the list they are going back to
   * has to be re-read against a store that already holds what they typed.
   * Resolves immediately when there is nothing waiting.
   */
  readonly write: () => Promise<void>;
  /**
   * Settle the one refusal a retry cannot: code history that was rewritten.
   *
   * A rebase, a reset or a replaced branch leaves the engine reconciling
   * against a commit this history does not descend from, and from then on it
   * refuses every write — the same refusal, however many times it is asked. So
   * this is not a retry with more patience: it tells the engine the new history
   * is the real one, and then writes what was waiting.
   *
   * Offered only for that refusal, and only from the record it happened in,
   * because it costs something: every record in the project becomes
   * `unverified`. Nothing written is lost by it.
   */
  readonly reconcile: () => Promise<void>;
  /**
   * Drop what is waiting for one record, because it no longer exists.
   *
   * Deleting a record a moment after typing into it would otherwise leave a
   * patch addressed to it, and leaving the record would send that patch to a
   * store that has nothing to apply it to. The write would be refused, which is
   * the right answer to the wrong question.
   */
  readonly forget: (key: string) => void;
}

/** What a patch is measured against, and what the panel falls back to. */
function draftOf(document: MemoryDocument): DocumentDraft {
  return {
    title: document.title,
    content: document.content,
    tags: document.tags,
    links: document.links,
    scope: document.scope,
    observed: document.observed,
    archived: document.archived,
    fields: document.fields,
  };
}

export function useDocument(
  projectPath: string,
  key: string | null,
): OpenDocument {
  const [state, setState] = useState<{
    key: string | null;
    document: MemoryDocument | null;
    error: string | null;
  }>({ key: null, document: null, error: null });
  const [save, setSave] = useState<{ key: string | null; state: SaveState }>({
    key: null,
    state: CLEAN,
  });
  /**
   * What has been changed and not yet written. In state rather than in a ref
   * because it is what the panel draws: a tag has to appear as it is typed.
   */
  const [patch, setPatch] = useState<{ key: string; patch: DocumentPatch }>({
    key: "",
    patch: {},
  });
  /**
   * A draft the store refused, kept so that closing the record and coming back
   * to it does not quietly discard what somebody wrote.
   */
  const [refused, setRefused] = useState<{
    key: string;
    patch: DocumentPatch;
    message: string;
    kind: string | null;
  } | null>(null);

  /** How to read the body, when the editor has changed it. */
  const body = useRef<{ key: string; read: () => string } | null>(null);
  /** What the store holds, so a change that cancels itself out is not a write. */
  const stored = useRef<{ key: string; draft: DocumentDraft } | null>(null);
  /** The patch as the flush needs it, without waiting for a render. */
  const waiting = useRef<{ key: string; patch: DocumentPatch }>({
    key: "",
    patch: {},
  });
  const writing = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushRef = useRef<() => Promise<void>>(async () => undefined);

  const schedule = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = null;
      void flushRef.current();
    }, SAVE_DELAY_MS);
  }, []);

  const flush = useCallback(async () => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    // A write is in flight; whatever arrived since is picked up by its tail.
    if (writing.current) return;

    const editedKey = waiting.current.key || body.current?.key;
    if (editedKey === undefined || editedKey === "") return;

    const whole: DocumentPatch = { ...waiting.current.patch };
    if (body.current?.key === editedKey) {
      (whole as { content?: string }).content = body.current.read();
    }

    // Only what actually differs from the store travels. Typing something and
    // taking it back is not an edit, and a patch that restated the record would
    // be a commit saying nothing.
    const changes =
      stored.current?.key === editedKey
        ? changed(whole, stored.current.draft)
        : whole;

    waiting.current = { key: "", patch: {} };
    body.current = null;
    setPatch({ key: "", patch: {} });

    if (Object.keys(changes).length === 0) {
      setSave({ key: editedKey, state: SAVED });
      return;
    }

    writing.current = true;
    setSave({ key: editedKey, state: { status: "saving" } });
    let written = false;
    try {
      const document = await updateMemoryDocument(
        projectPath,
        editedKey,
        changes,
      );
      written = true;
      setRefused((current) => (current?.key === editedKey ? null : current));
      // The store is the authority on what it now holds. The editor is not
      // re-seeded from this — it reads its text once, when it opens — so a write
      // coming back never moves the caret.
      if (document !== null) {
        stored.current = { key: editedKey, draft: draftOf(document) };
        setState((current) =>
          current.key === editedKey ? { ...current, document } : current,
        );
      }
      setSave({ key: editedKey, state: SAVED });
    } catch (failure) {
      const message = isMemoryFailure(failure)
        ? failure.message
        : "The record could not be written.";
      const kind = isMemoryFailure(failure) ? failure.kind : null;
      // The edits are not dropped. They go back to the waiting patch so the next
      // attempt has them, and into state so that leaving the record and
      // returning to it does not lose what was typed.
      waiting.current = { key: editedKey, patch: changes };
      setPatch({ key: editedKey, patch: changes });
      setRefused({ key: editedKey, patch: changes, message, kind });
      setSave({ key: editedKey, state: { status: "failed", message, kind } });
    } finally {
      writing.current = false;
    }

    // Only a write that landed goes round again. A failure that rescheduled
    // itself would be a retry loop nobody asked for and nobody can see.
    if (written && (waiting.current.key !== "" || body.current !== null)) {
      schedule();
    }
  }, [projectPath, schedule]);

  useEffect(() => {
    flushRef.current = flush;
  }, [flush]);

  useEffect(() => {
    if (key === null) return;
    let current = true;

    void (async () => {
      try {
        const document = await memoryDocument(projectPath, key);
        if (!current) return;
        if (document !== null) {
          stored.current = { key, draft: draftOf(document) };
        }
        setState({ key, document, error: null });
      } catch (failure) {
        if (current) {
          setState({
            key,
            document: null,
            error: isMemoryFailure(failure)
              ? failure.message
              : "The record could not be read.",
          });
        }
      }
    })();

    return () => {
      current = false;
    };
  }, [projectPath, key]);

  /**
   * Leaving a record writes it, and leaving means the key changed.
   *
   * Closing a record unmounts nothing: the area holding this hook goes on
   * showing its list, so without this a record left within the save delay was
   * written a second later — by which time the list behind it had already been
   * drawn from a store that still held it under its key, with no title to show.
   * The waiting patch still names the record being left, so the flush writes
   * that one and not whatever is opened next.
   *
   * The cleanup covers the other way out as well. This hook lives in the area
   * rather than in the window, so selecting a different area unmounts it, and
   * that is a person leaving a record just as much as closing it is.
   *
   * `flushRef` rather than `flush` on purpose: the effect must run for a change
   * of record, not for every new closure of the same write.
   */
  useEffect(() => {
    return () => {
      void flushRef.current();
    };
  }, [key]);

  // Focus leaving the application is the other moment worth writing at: a
  // person who has gone elsewhere should not be relying on coming back for the
  // last sentence they typed to exist.
  useEffect(() => {
    const write = () => void flush();
    window.addEventListener("blur", write);
    return () => {
      window.removeEventListener("blur", write);
      write();
    };
  }, [flush]);

  const mark = useCallback((edited: string) => {
    setSave((current) =>
      current.key === edited && current.state.status === "saving"
        ? current
        : { key: edited, state: { status: "edited" } },
    );
  }, []);

  const edit = useCallback(
    (next: DocumentPatch) => {
      if (key === null) return;
      const merged =
        waiting.current.key === key
          ? mergePatch(waiting.current.patch, next)
          : next;
      waiting.current = { key, patch: merged };
      setPatch({ key, patch: merged });
      mark(key);
      schedule();
    },
    [key, mark, schedule],
  );

  const editBody = useCallback(
    (read: () => string) => {
      if (key === null) return;
      body.current = { key, read };
      mark(key);
      schedule();
    },
    [key, mark, schedule],
  );

  const write = useCallback(() => flush(), [flush]);

  const reconcile = useCallback(async () => {
    if (key === null) return;
    setSave({ key, state: { status: "saving" } });
    try {
      await reconcileMemory(projectPath, true);
    } catch (failure) {
      // The rebuild itself was refused. Said in its own words rather than the
      // write's: what is on screen has to describe what just happened, and
      // repeating the earlier refusal would send somebody back to the button
      // that has already failed.
      const message = isMemoryFailure(failure)
        ? failure.message
        : "Memory could not be checked against the code again.";
      const kind = isMemoryFailure(failure) ? failure.kind : null;
      setRefused((current) =>
        current?.key === key ? { ...current, message, kind } : current,
      );
      setSave({ key, state: { status: "failed", message, kind } });
      return;
    }
    // What was refused is waiting, so this writes it. The record is re-read by
    // the write itself, which is what puts the store's answer back on screen.
    await flush();
  }, [key, projectPath, flush]);

  const forget = useCallback((gone: string) => {
    if (waiting.current.key === gone) {
      waiting.current = { key: "", patch: {} };
      if (timer.current !== null) {
        clearTimeout(timer.current);
        timer.current = null;
      }
    }
    if (body.current?.key === gone) body.current = null;
    if (stored.current?.key === gone) stored.current = null;
    setPatch((current) => (current.key === gone ? { key: "", patch: {} } : current));
    // The refusal goes too: it was kept so that coming back to the record would
    // not lose what was typed, and there is nothing to come back to.
    setRefused((current) => (current?.key === gone ? null : current));
    setSave((current) => (current.key === gone ? { key: null, state: CLEAN } : current));
  }, []);

  const document = state.key === key ? state.document : null;
  const refusedHere = refused?.key === key ? refused : null;
  const unwritten = patch.key === key ? patch.patch : null;

  const draft = useMemo(() => {
    if (document === null) return null;
    return { ...draftOf(document), ...(refusedHere?.patch ?? {}), ...(unwritten ?? {}) };
  }, [document, refusedHere, unwritten]);

  return {
    document,
    isLoading: key !== null && state.key !== key,
    error: state.key === key ? state.error : null,
    draft,
    save:
      save.key === key
        ? save.state
        : refusedHere === null
          ? CLEAN
          : {
              status: "failed",
              message: refusedHere.message,
              kind: refusedHere.kind,
            },
    edit,
    editBody,
    write,
    reconcile,
    forget,
  };
}

/** Later edits win, and product fields merge by name rather than wholesale. */
function mergePatch(base: DocumentPatch, next: DocumentPatch): DocumentPatch {
  const merged: DocumentPatch = { ...base, ...next };
  if (base.fields && next.fields) {
    return { ...merged, fields: { ...base.fields, ...next.fields } };
  }
  return merged;
}

/**
 * The members of a patch that actually differ from the record as stored.
 *
 * Compared as JSON because these are strings, string lists, a flag and whatever
 * a type declares — shapes with no identity worth preserving, where "the same
 * value" is the only question. A field the record does not carry and a field set
 * to null are the same absence, so neither counts as a change.
 */
function changed(patch: DocumentPatch, base: DocumentDraft): DocumentPatch {
  const result: Record<string, unknown> = {};
  const same = (left: unknown, right: unknown) =>
    JSON.stringify(left ?? null) === JSON.stringify(right ?? null);

  for (const [name, value] of Object.entries(patch)) {
    if (name === "fields") continue;
    if (!same(value, base[name as keyof DocumentDraft])) result[name] = value;
  }

  if (patch.fields) {
    const fields: Record<string, unknown> = {};
    for (const [name, value] of Object.entries(patch.fields)) {
      if (!same(value, base.fields[name])) fields[name] = value;
    }
    if (Object.keys(fields).length > 0) result.fields = fields;
  }

  return result as DocumentPatch;
}
