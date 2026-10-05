"use client";

/**
 * What a window is told when somebody clicks a banner.
 *
 * Sync raises banners from Rust, with no window open and none required — see
 * `src-tauri/src/attention.rs`. A click on one is therefore the one thing that
 * arrives at a window without the window having asked: it happened in another
 * application, possibly before this window existed, possibly before this
 * *launch* did.
 *
 * What arrives is an address rather than an instruction. It says which project
 * the conversation is in, which conversation it is, and which record it is
 * being held under where it is being held under one — and the window decides
 * what that means for what it is showing, exactly as it decides what a search
 * result means. Rust has already chosen this window and put it in front; the
 * rest is the window's.
 *
 * The two halves are here together because they are one bargain: a window says
 * what project it has open so that Rust can pick it, and hears about a click
 * because it was picked. Neither half is worth anything alone.
 *
 * How a window collects something left for it, and what it does with it, is
 * `@/lib/sent-to` — a banner is one of the two things that send a window
 * somewhere, and `@/lib/deep-link` is the other. What is here is what a banner
 * is: what it carries, and that it carries a conversation nothing can open.
 */

import { useEffect } from "react";
import { command } from "@/lib/command";
import type { AreaIntent } from "@/lib/area-intent";
import type { OpenProject } from "@/lib/project/types";
import {
  addressable,
  useSentTo,
  useShown,
  type SentRecord,
  type SentTo,
} from "@/lib/sent-to";

/** The event Rust sends the window it chose. */
const SHOWN = "attention://shown";

/** A record, at the length finding it again takes. */
export type AddressedRecord = SentRecord;

/** Where a banner was pointing. */
export interface Address extends SentTo {
  /**
   * The conversation, by the application's own name for it.
   *
   * Carried and not acted on: no section of the shell draws a conversation, and
   * which one does is a package's claim. It is here because a banner is *about*
   * one — the notification says so in its own lines — and because the day
   * something can open one, this is what it will be opened from.
   */
  readonly conversation: string;
}

/**
 * Tell Rust what this window has open.
 *
 * Every change, including to nothing: a window that went back to the list of
 * projects is a window that can be given one, and one that says nothing at all
 * is a window Rust will not put somebody else's project into.
 */
export function useWindowHolds(project: string | null): void {
  useEffect(() => {
    if (!addressable()) return;

    void (async () => {
      try {
        await command("window_holds", { project });
      } catch (error) {
        // The window works unaddressed: a banner clicked for its project opens
        // another window rather than finding this one. Reported rather than
        // escalated, and never allowed to take a render down with it.
        console.error("The window could not say what it holds.", error);
      }
    })();
  }, [project]);
}

/**
 * The last banner clicked at this window, or `null` where none has been.
 *
 * The mechanism is `useSentTo`; what is a banner's own is the shelf it reads —
 * `notifications_addressed`, which is emptied by the read — and the event Rust
 * sends the window it chose.
 */
export function useBannerAddress(): Address | null {
  return useSentTo(SHOWN, () =>
    command<Address | null>("notifications_addressed"),
  );
}

/**
 * Do what a click on a banner asked of this window.
 *
 * The project is opened and the record the conversation is being held under is
 * what the window is asked to show, where there is one — see `useShown`. The
 * conversation itself is not opened, and nothing here pretends otherwise: no
 * section of the shell draws one.
 */
export function useAddressed(
  project: OpenProject | null,
  enter: (project: OpenProject) => void,
): AreaIntent | null {
  return useShown(useBannerAddress(), project, enter);
}
