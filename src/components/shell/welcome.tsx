"use client";

import { FolderGit2 } from "lucide-react";

import { ErrorNote, type ProjectSetup } from "@/components/shell/project-setup";
import { Button } from "@/components/ui/button";
import { showNativeContextMenu } from "@/lib/native-menu";

/**
 * The window with no project open.
 *
 * It is the slab, empty, with the ways out of it: the folder picker, and the
 * projects this installation opened before. There are no columns because there
 * is nothing for them to be about — a sidebar listing sections of a project
 * that does not exist would be furniture arranged around an absence.
 *
 * The recent list appears only when there is one. An empty "Recents" heading
 * on a first launch would name a feature instead of showing one.
 */
export function WelcomeScreen({ setup }: { setup: ProjectSetup }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-5 bg-workspace px-8 pb-(--header-height)">
      <span
        aria-hidden="true"
        className="flex size-11 items-center justify-center rounded-(--radius-surface) border border-separator-strong bg-panel text-fg-secondary"
      >
        <FolderGit2 className="size-5" />
      </span>

      <div className="max-w-[46ch] space-y-1.5 text-center">
        <h1 className="text-lg font-medium text-fg">No project is open</h1>
        <p className="text-sm text-fg-secondary">
          Open any folder to work on it. Sync keeps what a project knows in its
          Git repository, so a folder that is not one yet is offered a repository
          when you open it.
        </p>
      </div>

      {/* Wide enough for both labels: this one is centred, so a resize moves
          the button out from under the pointer that just pressed it. */}
      <Button
        onClick={setup.begin}
        disabled={setup.isBusy}
        className="min-w-28"
      >
        {setup.isBusy ? "Opening…" : "Open Folder"}
      </Button>

      {setup.recent.length > 0 ? (
        <nav aria-label="Recent projects" className="w-full max-w-[52ch]">
          <p className="px-2 pb-1 text-xs font-semibold text-fg-tertiary">
            Recent
          </p>
          <ul className="flex flex-col gap-px">
            {setup.recent.map((entry) => (
              <li key={entry.path}>
                <button
                  type="button"
                  disabled={setup.isBusy}
                  onClick={() => setup.open(entry.path)}
                  onContextMenu={(event) =>
                    showNativeContextMenu(event, [
                      {
                        label: "Remove from Recents",
                        onSelect: () => setup.forget(entry.path),
                      },
                    ])
                  }
                  className="flex w-full items-baseline gap-2 rounded-(--radius-control) px-2 py-1.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover disabled:opacity-50"
                >
                  <span className="shrink-0 text-base text-fg">
                    {entry.name}
                  </span>
                  {/* The path is what tells two folders of the same name
                      apart, and it is the only thing here that ever does. */}
                  <span
                    className="min-w-0 flex-1 truncate text-right font-mono text-xs text-fg-tertiary"
                    title={entry.path}
                  >
                    {entry.path}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </nav>
      ) : null}

      {setup.stage === "closed" ? <ErrorNote message={setup.error} /> : null}
    </div>
  );
}
