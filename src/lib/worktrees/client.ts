/**
 * The window's route to a project's disposable working trees.
 *
 * A tree is a place to work that can be thrown away: it is made from the
 * project's `HEAD`, detached, so nothing is added to the repository while an
 * agent works in it. The name of the branch is asked for only when somebody
 * decides to keep the work — branch conventions belong to whoever owns the
 * repository, and a name invented here would turn up in their `git branch` as
 * something they did not choose.
 *
 * What that buys is reversibility, not safety. An agent working in a tree has a
 * shell like any other, and `docs/background.md` §9 does not promise a sandbox.
 * What is true is narrower and still worth having: the files it edited are
 * files nobody else is looking at, and undoing all of it is one gesture.
 *
 * Every function is one `invoke` into `src-tauri/src/worktree.rs`, which owns
 * git and decides where trees live. In particular a path is never a way to
 * choose a location: naming an existing tree is checked against git's own list,
 * so a caller cannot raise an agent in an arbitrary directory by calling it a
 * working tree.
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * What went wrong, with the kind the command layer gave it.
 *
 * The kinds are open on purpose — a caller switches on the ones it means to
 * handle and shows the message for the rest, which is what keeps a new refusal
 * from arriving as a blank screen.
 */
export class WorktreeError extends Error {
  readonly kind: string;

  constructor(kind: string, message: string) {
    super(message);
    this.name = "WorktreeError";
    this.kind = kind;
  }
}

/** One working tree of a project. */
export interface Worktree {
  /** Where it is, and how every other call names it. */
  readonly path: string;
  /**
   * The branch the work is aimed at: what the project was on when the tree was
   * made, or the branch the tree is checked out on when somebody made it
   * themselves. `undefined` when neither had one.
   *
   * Not a promise to merge there — nothing merges — but it is what a person
   * chooses between trees by.
   */
  readonly base?: string;
  /** The commit it started from. */
  readonly baseCommit: string;
  /**
   * Where it is now. Equal to `baseCommit` while nothing has been committed in
   * it, which is how a menu says *empty* without reading its history.
   */
  readonly head: string;
}

/**
 * Where a conversation is to be held.
 *
 * `"new"` makes one. An existing tree is named by its path, and two
 * conversations in one tree are allowed: carrying on work an agent left
 * half-done is the ordinary reason to pick one that is already there.
 */
export type WorktreeChoice = "new" | { readonly path: string };

/**
 * Every working tree this project has, the project's own excluded.
 *
 * Also the answer to whether trees are possible here at all: a folder that is
 * not a repository, or a machine with no git, refuses rather than answering an
 * empty list. A caller offering the choice can ask once and leave the gesture
 * out when this throws.
 */
export function worktreesIn(project: string): Promise<readonly Worktree[]> {
  return call<readonly Worktree[]>("worktree_list", { project });
}

/**
 * Keep the work: give the tree's commit the name a person chose.
 *
 * The name is git's to accept — `feature/NIK-42`, `wip`, whatever the
 * repository's convention is — and it is refused here only when git refuses it,
 * when that name is taken, or when nothing was committed in the tree, which
 * would be a branch pointing at the commit the work started from.
 *
 * The tree stays where it is and stays detached, so the new branch is free for
 * whoever named it to check out.
 */
export function adoptWorktree(args: {
  project: string;
  path: string;
  branch: string;
}): Promise<void> {
  return call<void>("worktree_adopt", args);
}

/**
 * Throw the tree away.
 *
 * **Commits made in it go with it** unless {@link adoptWorktree} named them
 * first, and files it never committed go too. Whatever offers this says so
 * before calling it: this is the deletion the whole arrangement exists to make
 * possible, and it is still a deletion.
 */
export function discardWorktree(args: { project: string; path: string }): Promise<void> {
  return call<void>("worktree_discard", args);
}

/** Where this installation makes trees, each project in a directory of its own. */
export function worktreeLocation(): Promise<string> {
  return call<string>("worktree_location");
}

/**
 * Make trees somewhere else, or pass `null` to go back to the default.
 *
 * Answers with the location in force afterwards, so a screen shows what is true
 * rather than what it asked for.
 */
export function setWorktreeLocation(path: string | null): Promise<string> {
  return call<string>("worktree_set_location", { path });
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (typeof error === "object" && error !== null && "kind" in error && "message" in error) {
      const failure = error as { kind: string; message: string };
      throw new WorktreeError(failure.kind, failure.message);
    }
    throw error;
  }
}
