"use client";

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { chooseFolder, registeredProjects } from "@/lib/project/client";
import {
  discardWorktree,
  setWorktreeLocation,
  worktreeLocation,
  worktreesIn,
  type Worktree,
} from "@/lib/worktrees/client";

/**
 * Where working trees are made, and what is in them.
 *
 * A working tree is a copy of a project to work in and throw away: an agent
 * raised in one edits files nobody else is looking at, and undoing all of it is
 * one gesture rather than a review of what changed under somebody's own open
 * files. Which disk has room for them is a fact about this machine and not
 * about any project — a path remembered in a repository would be wrong on the
 * next machine that cloned it — so the choice lives here, with the rest of what
 * is true of this installation.
 *
 * **The list is here because a tree outlives the conversation that made it.**
 * A conversation offers to throw its own tree away, and a conversation somebody
 * deleted takes that offer with it — so without this the trees left behind were
 * reachable from nowhere and the disk filled with copies of a repository nobody
 * could name. This is where they are named, and where they go.
 */
export function WorktreesSection() {
  const [location, setLocation] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [held, setHeld] = useState<readonly Held[] | null>(null);

  useEffect(() => {
    let live = true;
    void worktreeLocation()
      .then((path) => {
        if (live) setLocation(path);
      })
      .catch((error: unknown) => {
        if (live) setFailure(explain(error));
      });
    return () => {
      live = false;
    };
  }, []);

  /**
   * Every tree of every project this installation answers for.
   *
   * Asked project by project because that is the only way to ask: a tree
   * belongs to a repository, and git is what knows about it. A project that
   * refuses — moved, deleted, not a repository any more — contributes nothing
   * rather than a failure, because one unreadable project must not hide the
   * trees of the others.
   */
  const read = useCallback(async (): Promise<readonly Held[]> => {
    const projects = await registeredProjects();
    const answers = await Promise.all(
      projects.map(async (project) => ({
        project: project.name,
        path: project.path,
        trees: await worktreesIn(project.path).catch(() => [] as readonly Worktree[]),
      })),
    );
    return answers.filter((answer) => answer.trees.length > 0);
  }, []);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const answers = await read();
        if (live) setHeld(answers);
      } catch (error: unknown) {
        if (live) {
          setFailure(explain(error));
          setHeld([]);
        }
      }
    })();
    return () => {
      live = false;
    };
  }, [read]);

  const choose = useCallback(async (path: string | null) => {
    setBusy(true);
    setFailure(null);
    try {
      // Answered with what is in force afterwards rather than with what was
      // asked for: a path that could not be made is a refusal, and a section
      // that showed the request would be showing a location nothing uses.
      setLocation(await setWorktreeLocation(path));
    } catch (error: unknown) {
      setFailure(explain(error));
    } finally {
      setBusy(false);
    }
  }, []);

  const discard = useCallback(
    async (project: string, tree: Worktree) => {
      setBusy(true);
      setFailure(null);
      try {
        await discardWorktree({ project, path: tree.path });
        setHeld(await read());
      } catch (error: unknown) {
        setFailure(explain(error));
      } finally {
        setBusy(false);
      }
    },
    [read],
  );

  return (
    <section className="flex flex-col gap-3">
      <p className="text-sm text-fg-secondary">
        A conversation can be held in a working tree of its own: a copy of the
        project at its last commit, which an agent works in without touching
        what you have open. Each project keeps its trees in a directory of its
        own under this one.
      </p>

      <div className="flex items-center gap-3 rounded-(--radius-control) px-2 py-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-base text-fg">Location</p>
          <p className="truncate text-xs text-fg-tertiary">
            <span className="font-mono">{location ?? "…"}</span>
          </p>
        </div>

        <Button
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => {
            void (async () => {
              try {
                const chosen = await chooseFolder();
                // Dismissing the panel is an outcome, not a failure: nothing
                // was chosen and nothing needs saying about it.
                if (chosen !== null) await choose(chosen);
              } catch (error: unknown) {
                // A panel that never opened is a refusal like any other, and
                // one thrown away here is a button that does nothing with no
                // sentence anywhere saying why.
                setFailure(explain(error));
              }
            })();
          }}
        >
          Choose…
        </Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => void choose(null)}>
          Default
        </Button>
      </div>

      <p className="text-xs text-fg-tertiary">
        A tree starts from the project&apos;s last commit, so work you have not
        committed is not in it. Nothing that was built is either: dependencies
        and build output are not copied, and a tree is the source alone.
      </p>

      {held === null || held.length === 0 ? null : (
        <div className="flex flex-col gap-1">
          <h2 className="text-base font-medium text-fg">Trees on this machine</h2>
          {held.map((project) => (
            <div key={project.path} className="flex flex-col gap-px">
              <p className="truncate px-2 pt-2 text-xs font-medium text-fg-tertiary">
                {project.project}
              </p>
              <ul className="flex flex-col gap-px">
                {project.trees.map((tree) => (
                  <li
                    key={tree.path}
                    className="flex items-center gap-3 rounded-(--radius-control) px-2 py-2"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-base text-fg">
                        {tree.base ?? tree.baseCommit.slice(0, 7)}
                      </p>
                      <p className="truncate text-xs text-fg-tertiary">
                        <span className="font-mono">{tree.path}</span>
                      </p>
                    </div>
                    {/* What it holds, because it is the whole of what discarding
                        costs: a tree that was never committed in loses nothing,
                        and one that holds work loses it unless the branch was
                        named from the conversation first. */}
                    <span className="shrink-0 text-xs text-fg-tertiary">
                      {tree.head === tree.baseCommit ? "Empty" : "Holds work"}
                    </span>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy}
                      onClick={() => void discard(project.path, tree)}
                    >
                      Discard
                    </Button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </section>
  );
}

/** One project's trees, as this section draws them. */
interface Held {
  /** What the window calls the project. */
  readonly project: string;
  /** Its repository root, which every call about a tree takes. */
  readonly path: string;
  readonly trees: readonly Worktree[];
}

/**
 * A refusal in the words it arrived in — the path that could not be made is the
 * part somebody acts on, and a sentence of our own would drop it.
 */
function explain(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  if (error instanceof Error) return error.message;
  return "That location could not be used.";
}
