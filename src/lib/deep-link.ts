"use client";

/**
 * A `sync://` address followed from outside this window.
 *
 * The scheme is registered with the system by the installed bundle, so a record
 * has an address the whole machine can follow: the same url in a terminal, in a
 * chat client or in somebody's notes is a link, and following one brings the
 * project it names to the front with the record open. Which window that is, and
 * which project the address meant, is decided in `src-tauri/src/links.rs` before
 * this window is asked anything — see it for the two spellings and for what an
 * address cannot do.
 *
 * What is here is the window's half: collecting what was left for it, and asking
 * the area that owns the record to show it. The mechanism is the one a banner's
 * click uses — `@/lib/sent-to` — because being sent somewhere by a link and
 * being sent somewhere by a banner are the same thing happening to a window.
 *
 * There is a second half to this file, and it is the one direction that does not
 * come from outside: a link in a body or a message can name a record of
 * *another* project, and choosing between windows is not something a webview
 * can do. So that click is handed back to Rust, through the same router the
 * system's own addresses go through.
 */

import { command } from "@/lib/command";
import type { AreaIntent } from "@/lib/area-intent";
import type { OpenProject } from "@/lib/project/types";
import { useSentTo, useShown, type SentTo } from "@/lib/sent-to";

/** The event Rust sends the window an address landed in. */
const FOLLOWED = "link://followed";

/**
 * Where an address was pointing.
 *
 * The record is never absent, unlike a banner's: an address names one, and one
 * that does not is not an address this application reads.
 */
export interface Followed extends SentTo {
  readonly record: NonNullable<SentTo["record"]>;
}

/**
 * The last address followed at this window, or `null` where there has been none.
 *
 * What is this one's own is the shelf it reads — `link_followed`, emptied by the
 * read — and the event. Following the same address twice is two asks, which is
 * what somebody who clicked the same link again meant.
 */
export function useFollowedLink(): Followed | null {
  return useSentTo(FOLLOWED, () => command<Followed | null>("link_followed"));
}

/**
 * Do what following an address asked of this window.
 *
 * The project is opened where it is not open already, and the record is what the
 * window is asked to show — see `useShown`, which is the same answer a banner's
 * click gets.
 */
export function useLinked(
  project: OpenProject | null,
  enter: (project: OpenProject) => void,
): AreaIntent | null {
  return useShown(useFollowedLink(), project, enter);
}

/**
 * Follow an address that belongs to another project.
 *
 * For a link inside a body or a message naming a record this window cannot
 * show. The window does not open another window — that is the router's
 * decision, and it is the same one an address arriving from the system gets —
 * so the url is handed over whole rather than taken apart here.
 *
 * It throws what the router refuses: an address naming a project this machine
 * does not answer for reaches nothing, and the caller has drawn the link as
 * followable by then.
 */
export async function followLink(url: string): Promise<void> {
  await command("link_follow", { url });
}
