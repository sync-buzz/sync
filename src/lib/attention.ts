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
 */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AreaIntent } from "@/lib/area-intent";
import { device } from "@/lib/device";
import { openRegistered } from "@/lib/project/client";
import type { OpenProject } from "@/lib/project/types";
import { windowRole } from "@/lib/settings/window";

/** The event Rust sends the window it chose. */
const SHOWN = "attention://shown";

/** A record, at the length finding it again takes. */
export interface AddressedRecord {
  readonly key: string;
  readonly kind: string;
}

/** Where a banner was pointing. */
export interface Address {
  /** The project's working tree, which is what a window has open. */
  readonly project: string;
  /** The conversation, by the application's own name for it. */
  readonly conversation: string;
  /** The record it is being held under, where it is being held under one. */
  readonly record: AddressedRecord | null;
}

/**
 * Whether any of this means anything here.
 *
 * A banner is the Mac's — it is raised by the application that serves agents
 * with every window closed, and a phone has neither that application nor a
 * second window to be chosen between. So the phone neither says what it holds
 * nor listens for a click, rather than saying it to a command its own
 * application does not implement.
 */
function addressable(): boolean {
  return (
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in window &&
    device() === "computer" &&
    windowRole() === "main"
  );
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
        await invoke("window_holds", { project });
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
 * Asked for twice, and the first time is the one that matters: a banner is
 * shown when nothing of Sync is in front, which is most often when Sync has no
 * window at all — so the ordinary answer to a click is a window built to hold
 * it, and by the time that window is listening the click has already happened.
 * So it asks on the way up, and listens for the ones that arrive while it is
 * already there.
 *
 * The event carries nothing; both paths fetch. The fetch takes the address off
 * the shelf, so a click cannot be answered twice — and a fresh object comes
 * back every time, which is what makes clicking two banners about the same
 * conversation two asks rather than one. It is the same rule an area intent
 * keeps, for the same reason.
 */
export function useBannerAddress(): Address | null {
  const [address, setAddress] = useState<Address | null>(null);

  useEffect(() => {
    if (!addressable()) return;

    let listening = true;
    let stop: (() => void) | undefined;

    const collect = async () => {
      try {
        const waiting = await invoke<Address | null>("notifications_addressed");
        if (listening && waiting !== null) setAddress(waiting);
      } catch (error) {
        // A click that cannot be collected leaves the window where it is, which
        // is in front of the person who clicked. Reported rather than
        // escalated, and never allowed to take a render down with it.
        console.error("What a banner pointed at could not be read.", error);
      }
    };

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen(SHOWN, () => void collect());
      // Unmounted while the listener was being registered, which is a window
      // closing on the same frame it was told something. Dropping it here is
      // what keeps a closed window from being handed a click.
      if (listening) stop = unlisten;
      else unlisten();
      await collect();
    })();

    return () => {
      listening = false;
      stop?.();
    };
  }, []);

  return address;
}

/**
 * Do what a click on a banner asked of this window.
 *
 * Two steps, and the first is the one a person notices: the project the
 * conversation is in is opened, where it is not open already. Rust chose this
 * window for exactly that — it either holds the project, holds nothing, or was
 * made for this — so opening it here takes nothing away from anybody.
 *
 * The second is what the window is then asked to show, and it is deliberately
 * modest: the record the conversation is being held under, where there is one.
 * A conversation is not a thing the shell can open — no section here draws one,
 * and which section does is a package's claim — so what is answered is what
 * this window knows how to answer.
 */
export function useAddressed(
  project: OpenProject | null,
  enter: (project: OpenProject) => void,
): AreaIntent | null {
  const address = useBannerAddress();
  // The last click this has acted on, taken during the render that has it
  // rather than in an effect after — the same reading the window above gives
  // the place a phone was restored to, and for the same reason: an effect draws
  // one frame of where the person was before deciding where they are.
  const [answered, setAnswered] = useState<Address | null>(null);
  if (address !== null && address !== answered) setAnswered(address);

  const path = project?.path ?? null;

  /**
   * The click this window has finished with, and what the last one asked it to
   * show.
   *
   * **A click moves the window once.** Without this the address stands for as
   * long as the window does, and the effect below runs again on every change of
   * project — so somebody who chose another project from the switcher was
   * carried straight back to the one a banner had named, with nothing on screen
   * saying why. The address is spent instead: by the project being open, which
   * is the click answered, or by the open failing, which is the click answered
   * as well as it can be.
   *
   * Held rather than derived, because it outlives the address that produced it:
   * the same person coming back to this project later is not the banner asking
   * again. A second click on a banner is a fresh object off the shelf — see
   * `notifications_addressed` — so it is a fresh ask, which is the rule an area
   * intent keeps.
   */
  const [spent, setSpent] = useState<Address | null>(null);
  const [intent, setIntent] = useState<AreaIntent | null>(null);
  if (answered !== null && answered !== spent && path === answered.project) {
    setSpent(answered);
    // Deliberately modest: the record the conversation is being held under,
    // where there is one. A conversation is not a thing the shell can open.
    setIntent(
      answered.record === null
        ? null
        : {
            show: "record",
            key: answered.record.key,
            kind: answered.record.kind,
          },
    );
  }

  useEffect(() => {
    // Already here, or already answered. Rust chose this window because it
    // holds the project, or holds nothing, so the first is the ordinary case
    // rather than a shortcut.
    if (answered === null || answered === spent) return;
    if (path === answered.project) return;

    let listening = true;
    void (async () => {
      try {
        // The path is the key: it is what a project is registered under, and
        // what the phone's own place is held as.
        const opened = await openRegistered(answered.project, answered.project);
        if (listening) enter(opened);
      } catch {
        // Not reported. The reasons are the ordinary ones — the project was
        // closed, moved or forgotten since the banner was raised — and they all
        // end at a window that is now in front of the person, showing the list
        // of projects, which is a screen that explains itself.
        //
        // Spent all the same. What cannot be opened now will not open on the
        // next change of project either, and retrying there would put a failure
        // in the way of somebody navigating.
        if (listening) setSpent(answered);
      }
    })();

    return () => {
      listening = false;
    };
  }, [answered, enter, path, spent]);

  return intent;
}
