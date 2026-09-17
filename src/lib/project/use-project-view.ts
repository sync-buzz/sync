"use client";

import { useCallback, useEffect, useState } from "react";

import { loadProjectView, saveProjectView } from "@/lib/project/client";
import type { ProjectView, ProjectViewChange } from "@/lib/project/types";

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
  return useExceptions(
    projectPath,
    (view) => view.hiddenTypes,
    (hiddenTypes) => ({ hiddenTypes }),
  );
}

/**
 * Which kinds the activity does not report.
 *
 * The same shape as the type filter, over a second field of the same file, so
 * the same control draws both: what a person is deciding is the same decision —
 * *not this kind, for me, on this machine* — asked once about a list and once
 * about a history.
 *
 * Held as the exceptions rather than as the selection, which is the half that
 * matters and the reason this is a hook rather than a stored array of watched
 * kinds. A project's kinds are invented long after this file was written, so a
 * stored selection would silently stop reporting every kind installed since —
 * and that failure looks exactly like nothing having happened.
 */
export function useWatchedKinds(projectPath: string): ProjectViewState {
  return useExceptions(
    projectPath,
    (view) => view.unwatchedKinds,
    (unwatchedKinds) => ({ unwatchedKinds }),
  );
}

/**
 * Which sections this person put away from the sidebar.
 *
 * The same shape as the type filter, over a third field of the same file, so
 * the same control draws both: what a person is deciding is the same decision —
 * *not this section, for me, on this machine* — asked once about the
 * navigator's list and once about the sidebar's.
 *
 * Held as the exceptions rather than as the selection, which is the half that
 * matters and the reason this is a hook rather than a stored array of visible
 * sections. A project's sections are installed after this file was written, so
 * a stored selection would silently hide every section installed since.
 */
export function useHiddenSections(projectPath: string): ProjectViewState {
  return useExceptions(
    projectPath,
    (view) => view.hiddenSections,
    (hiddenSections) => ({ hiddenSections }),
  );
}

/**
 * One list of kinds somebody excepted, read and written where those live.
 *
 * Both preferences are the same mechanism over two fields, and writing it twice
 * is how the second copy comes to behave differently from the first — over
 * something as quiet as whether a failed write is worth a message.
 */
function useExceptions(
  projectPath: string,
  read: (view: ProjectView) => readonly string[],
  write: (kinds: readonly string[]) => ProjectViewChange,
): ProjectViewState {
  const [stored, setStored] = useState<{
    path: string;
    hidden: readonly string[];
  }>({ path: "", hidden: NOTHING_HIDDEN });

  useEffect(() => {
    let current = true;

    void loadProjectView(projectPath).then(
      (view) => {
        if (current) setStored({ path: projectPath, hidden: read(view) });
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
    // `read` is a literal at both call sites and never changes between renders;
    // depending on it would restart this on every one of them.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectPath]);

  const hidden = stored.path === projectPath ? stored.hidden : NOTHING_HIDDEN;

  const remember = useCallback(
    (kinds: readonly string[]) => {
      setStored({ path: projectPath, hidden: kinds });
      void saveProjectView(projectPath, write(kinds)).catch(() => undefined);
    },
    // Same as above: the writer is a literal at each call site.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projectPath],
  );

  const toggle = useCallback(
    (kind: string) =>
      remember(
        hidden.includes(kind) ? hidden.filter((entry) => entry !== kind) : [...hidden, kind],
      ),
    [hidden, remember],
  );

  const showAll = useCallback(() => remember(NOTHING_HIDDEN), [remember]);

  const isHidden = useCallback((kind: string) => hidden.includes(kind), [hidden]);

  return { hidden, isHidden, toggle, showAll };
}
