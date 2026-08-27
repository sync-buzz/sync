"use client";

import { useCallback, useEffect, useState } from "react";

import { memoryFolders, type MemoryFolder } from "./client";
import { explain } from "./use-corpus";

/**
 * The project's folders, kept in step with the corpus.
 *
 * Its own hook rather than a member of `useCorpus`, because it is a different
 * question with a different answer. The corpus is what the project *knows*;
 * this is the shape it is filed in, and half of that shape is not in the corpus
 * at all — an empty directory of an attached folder is in no record, and a tree
 * built from a page of records would leave it out while a person looks straight
 * at it in Finder.
 *
 * Read whole rather than a level at a time. A type's folders are a list of short
 * strings, one call answers for all of them, and asking per level would make
 * opening a branch a round trip that can fail on its own. Whoever draws a
 * subtree slices this by prefix, which is what a path is for.
 *
 * One call per type, though, rather than one for the project. Folders are a
 * namespace every type shares, so a project-wide answer cannot say whose a
 * folder is — and a tree that hung all of them under every type would show each
 * folder several times, in places its records are not.
 */
export interface Folders {
  /** One type's folders, by kind. A type with none is absent rather than empty. */
  readonly byKind: ReadonlyMap<string, readonly MemoryFolder[]>;
  /** Why there are none, when that is the reason rather than the answer. */
  readonly error: string | null;
  readonly isLoading: boolean;
  /** Re-read now. For a caller that wrote something this hook cannot see. */
  readonly reload: () => void;
}

/**
 * @param revision The corpus revision this tree should agree with. Passed in
 *   rather than read here so the two answers on screen come from one moment:
 *   a hook watching the store on its own would redraw the tree a beat before or
 *   after the records beside it, and a folder appearing without its documents
 *   reads as a bug in the project rather than in the timing. `null` is the
 *   corpus before its first answer, and is read on rather than waited for — the
 *   folders are a separate question and do not need the records to have arrived
 *   for the tree to be true.
 * @param active False while the area holding this is mounted but not selected.
 *   Such an area keeps what it last read and stops asking — ten installed areas
 *   must not be ten reads of the working tree on every revision.
 */
export function useFolders(
  projectPath: string,
  kinds: readonly string[],
  revision: string | null,
  active = true,
): Folders {
  const [attempt, setAttempt] = useState(0);
  // What was asked for. Everything the answer has to agree with is in it, so
  // comparing it against what arrived is the whole of "is this still loading" —
  // no flag set from inside an effect, which would be a render caused by a
  // render.
  const wanted = [...kinds].sort().join("\u0001");
  const key = `${projectPath}\u0000${revision ?? ""}\u0000${attempt}\u0000${wanted}`;
  const [answer, setAnswer] = useState<Answer>({
    key: "",
    byKind: new Map(),
    error: null,
  });

  const reload = useCallback(() => setAttempt((count) => count + 1), []);

  useEffect(() => {
    if (!active) return;
    let current = true;

    void (async () => {
      try {
        const asked = wanted === "" ? [] : wanted.split("\u0001");
        const answers = await Promise.all(
          asked.map(
            async (kind) =>
              [
                kind,
                await memoryFolders(projectPath, undefined, false, kind),
              ] as const,
          ),
        );
        if (current) setAnswer({ key, byKind: new Map(answers), error: null });
      } catch (failure) {
        // The folders go with the reason. A tree left holding the last answer
        // while something says it could not be read is a tree inviting somebody
        // to click a folder that is not there any more.
        if (current) {
          setAnswer({ key, byKind: new Map(), error: explain(failure) });
        }
      }
    })();

    return () => {
      current = false;
    };
  }, [key, projectPath, wanted, active]);

  return {
    byKind: answer.byKind,
    error: answer.error,
    isLoading: answer.key !== key,
    reload,
  };
}

/** One answer, and the question it answers. */
interface Answer {
  readonly key: string;
  readonly byKind: ReadonlyMap<string, readonly MemoryFolder[]>;
  readonly error: string | null;
}

/**
 * The folders at or under `root`, as a tree of paths.
 *
 * Slicing by prefix rather than by anything the engine said: a folder path is a
 * path, and `docs/guides/api` is under `docs/guides` because it says so. The
 * boundary is checked on a segment — `docs/guides` does not contain
 * `docs/guides-old`, and a plain `startsWith` would say it does.
 */
export function foldersUnder(
  folders: readonly MemoryFolder[],
  root: string,
): readonly MemoryFolder[] {
  // `""` is the project root, and everything is under it. It is the root of a
  // type whose documents are its own records: there is no directory, so the
  // hierarchy starts at the top with whatever its records are filed in.
  if (root === "") return folders.filter((folder) => folder.path !== "");
  const prefix = `${root}/`;
  return folders.filter((folder) => folder.path.startsWith(prefix));
}

/** The last segment of a path, which is what a folder is called. */
export function folderName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** The path a folder is in, or `""` for one at the root. */
export function parentFolder(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut === -1 ? "" : path.slice(0, cut);
}
