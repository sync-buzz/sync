"use client";

import { FolderOpen } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { loadRecentProjects, registeredProjects } from "@/lib/project/client";
import type { RecentProject } from "@/lib/project/types";
import { cn } from "@/lib/utils";

/**
 * Which project the panel speaks for, chosen out of the ones this machine
 * answers for.
 *
 * **It comes first when nothing is open.** A line has to be about a repository,
 * and the panel is reached with every window closed as readily as with one in
 * front — so on a machine sitting in the menu bar this is the panel's first
 * screen rather than an option in it. Nothing is picked for somebody: a filter
 * with one match still waits for Return.
 *
 * **Recently opened first, then everything registered.** The two lists answer
 * different questions — *what was I just working on* and *what can an agent
 * reach here* — and the order is the first of them, because somebody who opened
 * a project this morning is overwhelmingly asking about it again. What the
 * recent list has dropped is still here, further down, which is the whole
 * reason both are read.
 */
export function AskProjects({
  speaking,
  opened,
  onChoose,
}: {
  /** What it speaks for now, so the list can mark it. `null` when nothing is. */
  readonly speaking: RecentProject | null;
  /** Bumped each time the key opened the panel: the filter starts again. */
  readonly opened: number;
  readonly onChoose: (project: RecentProject) => void;
}) {
  const [projects, setProjects] = useState<readonly RecentProject[]>([]);
  /**
   * What is typed into the filter, which opening it belongs to, and where the
   * cursor stands.
   *
   * One piece of state rather than three, and compared during the render that
   * brings a new opening rather than reset afterwards in an effect — the
   * arrangement `console-input.tsx` uses for the same reason: a filter left
   * over from last time hides the project somebody came back for, and clearing
   * it a frame later means drawing that frame.
   */
  const [typed, setTyped] = useState({ opened, filter: "", at: 0 });
  if (typed.opened !== opened) setTyped({ opened, filter: "", at: 0 });
  const { filter } = typed;
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let reading = true;
    void Promise.all([loadRecentProjects(), registeredProjects()])
      .then(([recent, registered]) => {
        if (!reading) return;
        const seen = new Set(recent.map((project) => project.path));
        setProjects([
          ...recent,
          // Named by what the window calls it rather than by its identifier:
          // the identifier is what agents address, and a person reading a list
          // is not addressing anything yet.
          ...registered
            .filter((project) => !seen.has(project.path))
            .map((project) => ({ path: project.path, name: project.name })),
        ]);
      })
      .catch(() => {
        // A machine that cannot say which projects it answers for leaves an
        // empty list, which says the same thing without a second surface to
        // say it in.
      });
    return () => {
      reading = false;
    };
  }, []);

  // The caret, every time the key opens the panel. Taking focus is not state,
  // so it is the one part of starting again that belongs in an effect.
  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, [opened]);

  const shown = useMemo(() => {
    const wanted = filter.trim().toLowerCase();
    if (wanted.length === 0) return projects;
    return projects.filter(
      (project) =>
        project.name.toLowerCase().includes(wanted) ||
        // The path as well as the name, because two repositories are very often
        // called the same thing and the folder is what tells them apart.
        project.path.toLowerCase().includes(wanted),
    );
  }, [filter, projects]);

  const cursor = Math.min(typed.at, Math.max(0, shown.length - 1));
  const moveTo = (at: number) => setTyped({ ...typed, at });

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.code === "ArrowDown") {
      event.preventDefault();
      moveTo(cursor + 1 >= shown.length ? 0 : cursor + 1);
      return;
    }
    if (event.code === "ArrowUp") {
      event.preventDefault();
      moveTo(cursor - 1 < 0 ? Math.max(0, shown.length - 1) : cursor - 1);
      return;
    }
    if (event.code === "Enter") {
      event.preventDefault();
      const chosen = shown[cursor];
      if (chosen !== undefined) onChoose(chosen);
    }
    // Escape is the panel's, and `⌘P` is too. Neither is stopped here, so both
    // reach the surface that owns them.
  };

  return (
    <>
      <div className="flex shrink-0 items-center gap-3 px-5 py-4">
        {/* What is being asked for, drawn once. The panel has no toolbar and
            no heading, so this glyph is the whole of what says which of the two
            screens somebody is looking at. */}
        <FolderOpen
          aria-hidden
          className="size-5 shrink-0 text-fg-tertiary"
          strokeWidth={1.5}
        />
        <input
          ref={field}
          value={filter}
          onChange={(event) =>
            setTyped({ opened, filter: event.target.value, at: 0 })
          }
          onKeyDown={onKeyDown}
          // Said out loud rather than drawn as a label: the surface is one line
          // and a heading over it would be a word somebody reads once.
          aria-label="Choose a project"
          placeholder={
            speaking === null
              ? "Which project?"
              : `Which project? (${speaking.name})`
          }
          className="min-w-0 flex-1 bg-transparent text-lg text-fg outline-none placeholder:text-fg-tertiary"
          spellCheck={false}
          autoComplete="off"
        />
      </div>

      {shown.length > 0 && (
        <ul
          // Its own scroller with a ceiling, so a machine with forty projects
          // does not open a panel the height of the screen.
          className="max-h-80 overflow-y-auto overscroll-contain border-t border-separator px-3 py-2"
          role="listbox"
          aria-label="Projects"
        >
          {shown.map((project, index) => (
            <li key={project.path}>
              <button
                type="button"
                role="option"
                aria-selected={index === cursor}
                // The pointer moves the cursor rather than selecting on hover:
                // the keyboard and the mouse address one position, so a click
                // and Return do the same thing.
                onMouseMove={() => moveTo(index)}
                onClick={() => onChoose(project)}
                className={cn(
                  "flex w-full items-baseline gap-4 rounded-md px-3 py-2 text-left",
                  index === cursor && "bg-selected",
                )}
              >
                <span className="w-40 shrink-0 truncate text-sm text-fg">
                  {project.name}
                </span>
                <span className="min-w-0 flex-1 truncate text-sm text-fg-tertiary">
                  {project.path}
                </span>
                {speaking?.path === project.path && (
                  <span className="shrink-0 text-xs text-fg-tertiary">
                    current
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
