"use client";

import { useEffect, useState } from "react";

import { command } from "@/lib/command";

/**
 * The files a path in the line could mean.
 *
 * **The freedom is the person's, not this module's.** What can be read is the
 * tab's working folder and what is under it; going anywhere else is `cd`, which
 * somebody types. That line is held in the host — this only asks, and asks
 * about one folder at a time.
 *
 * Nothing is cached. Somebody completes a path a few times an hour, and a cache
 * would be a list that disagrees with a file they made a second ago.
 */

/** One entry of a folder, as the host answers it. */
interface Entry {
  readonly name: string;
  readonly folder: boolean;
}

/** What is never offered until it is named outright. */
const BURIED = new Set(["node_modules", "target", "dist", ".git"]);

function folder(tab: string, path: string): Promise<Entry[]> {
  return command<Entry[]>("console_folder", { tab, path });
}

/** Tell the host where this tab is working, and hear back where that is. */
export function goTo(tab: string, path: string): Promise<string> {
  return command<string>("console_cd", { tab, path });
}

/**
 * Open a tab with the host, and take the name it hands back.
 *
 * The name is the host's to mint: one chosen here would be one anything in this
 * webview could choose again, and two windows would meet on it.
 */
export function openTabWith(project: string): Promise<string> {
  return command<string>("console_tab", { project });
}

/** Close a tab with the host, so the folder it held is forgotten. */
export function closeTabWith(tab: string): Promise<void> {
  return command<void>("console_tab_close", { tab });
}

/** The folder a half-written path is inside, and the part being narrowed. */
function split(word: string): { readonly at: string; readonly wanted: string } {
  const cut = word.lastIndexOf("/");
  return {
    // A path that is only `/` is the working folder itself: the slash opens a
    // path rather than naming the root of the disk, which is not somewhere
    // this can read anyway.
    at: cut <= 0 ? "" : word.slice(0, cut),
    wanted: word.slice(cut + 1).toLowerCase(),
  };
}

/**
 * What stands inside the path being typed, best first.
 *
 * Folders come back with a trailing slash — taking one continues the walk
 * instead of ending it, which is the same gesture the memory tree uses one
 * module over.
 */
export function useFiles(
  tab: string,
  cwd: string,
  word: string,
): readonly { readonly token: string; readonly hint: string }[] {
  // What was read and which folder it came from, kept together so that a list
  // never belongs to a folder it was not read from — the two apart is how a
  // completion shows one folder's names under another's path for a frame.
  const [held, setHeld] = useState<{
    readonly at: string;
    readonly entries: readonly Entry[];
  }>({ at: "", entries: [] });
  const { at, wanted } = split(word);
  const asking = word.startsWith("/") ? (at.length === 0 ? cwd : at) : null;

  useEffect(() => {
    if (asking === null) return;
    let reading = true;
    void folder(tab, asking)
      .then((entries) => {
        if (reading) setHeld({ at: asking, entries });
      })
      .catch(() => {
        // Outside the tab's folder, or gone. Offering nothing is the honest
        // answer: the line is still whatever somebody typed, and `!` will say
        // what the system says when they run it.
        if (reading) setHeld({ at: asking, entries: [] });
      });
    return () => {
      reading = false;
    };
  }, [asking, tab]);

  // Only what was read from the folder being asked about. A list from the
  // folder before it would draw names that are not there, under a path that is.
  if (asking === null || held.at !== asking) return [];
  const base = asking;

  return held.entries
    .filter((entry) => {
      if (!entry.name.toLowerCase().startsWith(wanted)) return false;
      // Hidden files and the folders nothing is read out of are offered only
      // once somebody has started typing their name. Without this the first
      // completion in any repository is a thousand lines of `node_modules`.
      const buried = entry.name.startsWith(".") || BURIED.has(entry.name);
      return !buried || wanted.length > 0;
    })
    .map((entry) => ({
      // An absolute path, because the same word has to be right for a process
      // running in this folder and for an agent running in the project's.
      token: `${base}/${entry.name}${entry.folder ? "/" : ""}`,
      hint: entry.folder ? "folder" : "file",
    }));
}
