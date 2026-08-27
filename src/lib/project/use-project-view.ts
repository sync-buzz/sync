"use client";

import { useCallback, useEffect, useState } from "react";

import { loadProjectView, saveProjectView } from "@/lib/project/client";

/**
 * Which of a project's types this window is showing.
 *
 * The preference belongs to the installation, not to the project, so it is read
 * from and written to the application's own configuration — see `ProjectView`
 * in `src/lib/project/types.ts` for why.
 *
 * It is held with the path it was read for, so that switching projects shows
 * every type until the new project's preference arrives rather than briefly
 * applying the last project's answer to this one.
 *
 * Writes are optimistic: ticking a checkbox changes the list at once and the
 * store is told afterwards. A preference that could not be written is worth
 * saying nothing about — the window is already showing what was asked for, and
 * the only cost is that the next launch starts from the old list.
 */
export interface ProjectViewState {
  /** Kinds the window is not listing. */
  readonly hidden: readonly string[];
  readonly isHidden: (kind: string) => boolean;
  readonly toggle: (kind: string) => void;
  readonly showAll: () => void;
}

const NOTHING_HIDDEN: readonly string[] = [];

export function useProjectView(projectPath: string): ProjectViewState {
  const [stored, setStored] = useState<{
    path: string;
    hidden: readonly string[];
  }>({ path: "", hidden: NOTHING_HIDDEN });

  useEffect(() => {
    let current = true;

    void loadProjectView(projectPath).then(
      (view) => {
        if (current) setStored({ path: projectPath, hidden: view.hiddenTypes });
      },
      // Outside Tauri, and on a first launch, there is nothing stored. Showing
      // every type is the honest answer to both.
      () => {
        if (current) setStored({ path: projectPath, hidden: NOTHING_HIDDEN });
      },
    );

    return () => {
      current = false;
    };
  }, [projectPath]);

  const hidden = stored.path === projectPath ? stored.hidden : NOTHING_HIDDEN;

  const remember = useCallback(
    (hiddenTypes: readonly string[]) => {
      setStored({ path: projectPath, hidden: hiddenTypes });
      void saveProjectView(projectPath, { hiddenTypes }).catch(() => undefined);
    },
    [projectPath],
  );

  const toggle = useCallback(
    (kind: string) =>
      remember(
        hidden.includes(kind)
          ? hidden.filter((entry) => entry !== kind)
          : [...hidden, kind],
      ),
    [hidden, remember],
  );

  const showAll = useCallback(() => remember(NOTHING_HIDDEN), [remember]);

  const isHidden = useCallback(
    (kind: string) => hidden.includes(kind),
    [hidden],
  );

  return { hidden, isHidden, toggle, showAll };
}
