"use client";

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { chooseFolder } from "@/lib/project/client";
import { setWorktreeLocation, worktreeLocation } from "@/lib/worktrees/client";

/**
 * Where working trees are made.
 *
 * A working tree is a copy of a project to work in and throw away: an agent
 * raised in one edits files nobody else is looking at, and undoing all of it is
 * one gesture rather than a review of what changed under somebody's own open
 * files. Which disk has room for them is a fact about this machine and not
 * about any project — a path remembered in a repository would be wrong on the
 * next machine that cloned it — so the choice lives here, with the rest of what
 * is true of this installation.
 *
 * The section is one setting because that is what there is. It says what the
 * trees cost to make and what they do not carry, because the difference is not
 * visible from the path: a tree is the project's files at its last commit, and
 * nothing that was uncommitted and nothing that was built.
 */
export function WorktreesSection() {
  const [location, setLocation] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

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

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </section>
  );
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
