"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { loadProjectView, saveProjectView } from "@/lib/project/client";

/**
 * The order somebody put this project's sections in.
 *
 * The default is the project's: sections arrive in the order the project's own
 * record declares its extensions, so a repository nobody has arranged reads the
 * same on every machine. What this holds is the arrangement made on top of that
 * — one person moving a section to where they work — and it belongs to the
 * installation for the same reason the type filter does: dragging a row is not
 * a claim about the project, and writing one into `refs/memory/*` would commit
 * somebody's habits to a colleague's column.
 *
 * It is stored as keys rather than as positions, and it does not have to name
 * every section. A section installed since the last drag has never been placed,
 * and one whose extension failed to run this launch is not there to place — so
 * the stored list is reconciled against what the window actually mounted rather
 * than trusted as a description of it.
 *
 * Writes are optimistic, as the type filter's are: the column reorders at once
 * and the configuration is told afterwards. An arrangement that could not be
 * written costs the next launch, and nothing that is worth interrupting a drag
 * to say.
 */
export interface SectionOrder<T> {
  /** The sections, in the order to draw them. */
  readonly sections: readonly T[];
  /**
   * Somebody moved one. The argument is the whole column in its new order, by
   * key, because that is what was decided — a delta would leave this hook to
   * re-derive an arrangement the caller already has in hand.
   */
  readonly arrange: (keys: readonly string[]) => void;
}

/** Nothing arranged, which is where every project starts. */
const UNARRANGED: readonly string[] = [];

export function useSectionOrder<T extends { readonly key: string }>(
  projectPath: string,
  sections: readonly T[],
): SectionOrder<T> {
  // Held with the path it was read for, so switching projects draws the new
  // project's sections in their own order rather than briefly applying the
  // last one's arrangement to them.
  const [stored, setStored] = useState<{
    path: string;
    order: readonly string[];
  }>({ path: "", order: UNARRANGED });

  useEffect(() => {
    let current = true;

    void loadProjectView(projectPath).then(
      (view) => {
        if (current) setStored({ path: projectPath, order: view.sections });
      },
      // Outside Tauri, and on a first launch, there is nothing stored. The
      // order the project declares is the honest answer to both, and it is the
      // one this falls back to everywhere else as well.
      () => {
        if (current) setStored({ path: projectPath, order: UNARRANGED });
      },
    );

    return () => {
      current = false;
    };
  }, [projectPath]);

  const order = stored.path === projectPath ? stored.order : UNARRANGED;

  const arranged = useMemo(() => placed(sections, order), [order, sections]);

  const arrange = useCallback(
    (keys: readonly string[]) => {
      const next = keeping(keys, order);
      setStored({ path: projectPath, order: next });
      void saveProjectView(projectPath, { sections: next }).catch(
        () => undefined,
      );
    },
    [order, projectPath],
  );

  return { sections: arranged, arrange };
}

/**
 * The sections in the arranged order, with the unarranged ones after them.
 *
 * A section nobody has moved keeps the place the project's declaration gave it,
 * and follows everything that was moved. That is what installing an extension
 * does anyway — the declaration appends — so a new section arrives at the foot
 * of the list somebody arranged rather than in the middle of it.
 */
function placed<T extends { readonly key: string }>(
  sections: readonly T[],
  order: readonly string[],
): readonly T[] {
  // The same array back, so a project nobody has arranged does not hand its
  // sidebar a new list on every render.
  if (order.length === 0) return sections;

  const rank = new Map(order.map((key, index) => [key, index]));
  // Sorting is stable, so sections with no rank keep the order they arrived in.
  return [...sections].sort(
    (first, second) =>
      (rank.get(first.key) ?? order.length) -
      (rank.get(second.key) ?? order.length),
  );
}

/**
 * The new arrangement, keeping the keys this window has no section for.
 *
 * An extension that failed to run this launch has not been rearranged — it was
 * absent — and dropping its key would be this window deciding on its behalf
 * that it belongs at the bottom. Each one is put back where it was, as closely
 * as a list that changed underneath it allows.
 */
function keeping(
  keys: readonly string[],
  order: readonly string[],
): readonly string[] {
  const next = [...keys];
  order.forEach((key, index) => {
    if (!next.includes(key)) next.splice(Math.min(index, next.length), 0, key);
  });
  return next;
}
