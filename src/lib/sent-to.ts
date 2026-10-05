"use client";

/**
 * A window told to be somewhere it is not.
 *
 * Two things ask this of a window, and neither of them is the window: a banner
 * clicked in another application — `@/lib/attention` — and a `sync://` address
 * followed from anywhere on the machine — `@/lib/deep-link`. What they have in
 * common is everything about the mechanism and nothing about the subject, which
 * is why the mechanism is here and each subject is next door.
 *
 * What the two share is the awkward part, and it is awkward for one reason: the
 * ask arrives from outside this window, possibly before the window existed and
 * possibly before this *launch* did. So it is fetched rather than delivered —
 * Rust leaves it on a shelf and the window takes it off — and it is fetched
 * twice: once on the way up, because an ask answered by building a window has
 * already happened by the time that window is listening, and once per event, for
 * the ones that arrive while it is already there.
 *
 * What arrives is an address rather than an instruction. It says which project,
 * and which record where there is one, and the window decides what that means
 * for what it is showing — exactly as it decides what a search result means.
 * Rust has already chosen this window and put it in front; the rest is here.
 */

import { useEffect, useState } from "react";
import type { AreaIntent } from "@/lib/area-intent";
import { device } from "@/lib/device";
import { openRegistered } from "@/lib/project/client";
import type { OpenProject } from "@/lib/project/types";
import { windowRole } from "@/lib/settings/window";

/** A record, at the length finding it again takes. */
export interface SentRecord {
  readonly key: string;
  readonly kind: string;
}

/** Where something outside this window was pointing. */
export interface SentTo {
  /** The project's working tree, which is what a window has open. */
  readonly project: string;
  /** The record to open there, where the ask names one. */
  readonly record: SentRecord | null;
}

/**
 * Whether any of this means anything here.
 *
 * Both asks are the Mac's. A banner is raised by the application that serves
 * agents with every window closed, an address is registered by an installed
 * bundle, and a phone has neither those nor a second window to be chosen
 * between. So the phone neither says what it holds nor listens, rather than
 * calling a command its own application does not implement.
 */
export function addressable(): boolean {
  return (
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in window &&
    device() === "computer" &&
    windowRole() === "main"
  );
}

/**
 * The last ask left for this window, or `null` where there has been none.
 *
 * `collect` takes it off the shelf, and `event` is what says there is something
 * to take. The fetch is what empties the shelf, so an ask cannot be answered
 * twice — and a fresh object comes back every time, which is what makes two
 * asks about the same record two asks rather than one. It is the same rule an
 * area intent keeps, for the same reason.
 */
export function useSentTo<Sent extends SentTo>(
  event: string,
  collect: () => Promise<Sent | null>,
): Sent | null {
  const [sent, setSent] = useState<Sent | null>(null);

  useEffect(() => {
    if (!addressable()) return;

    let listening = true;
    let stop: (() => void) | undefined;

    const take = async () => {
      try {
        const waiting = await collect();
        if (listening && waiting !== null) setSent(waiting);
      } catch (error) {
        // An ask that cannot be collected leaves the window where it is, which
        // is in front of the person who made it. Reported rather than escalated,
        // and never allowed to take a render down with it.
        console.error("What this window was sent to could not be read.", error);
      }
    };

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen(event, () => void take());
      // Unmounted while the listener was being registered, which is a window
      // closing on the same frame it was told something. Dropping it here is
      // what keeps a closed window from being handed an ask.
      if (listening) stop = unlisten;
      else unlisten();
      await take();
    })();

    return () => {
      listening = false;
      stop?.();
    };
    // The collector is a fresh closure on every render and the event never
    // changes, so the effect is keyed on neither: re-running it would drop the
    // listener and re-fetch the shelf on every render of the window.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return sent;
}

/**
 * Do what the ask asked of this window.
 *
 * Two steps, and the first is the one a person notices: the project is opened,
 * where it is not open already. Rust chose this window for exactly that — it
 * either holds the project, holds nothing, or was made for this — so opening it
 * here takes nothing away from anybody.
 *
 * The second is what the window is then asked to show, and it is exactly as
 * modest as the ask: the record, where the ask names one.
 */
export function useShown(
  sent: SentTo | null,
  project: OpenProject | null,
  enter: (project: OpenProject) => void,
): AreaIntent | null {
  // The last ask this has acted on, taken during the render that has it rather
  // than in an effect after — the same reading the window gives the place a
  // phone was restored to, and for the same reason: an effect draws one frame of
  // where the person was before deciding where they are.
  const [answered, setAnswered] = useState<SentTo | null>(null);
  if (sent !== null && sent !== answered) setAnswered(sent);

  const path = project?.path ?? null;

  /**
   * The ask this window has finished with, and what the last one asked it to
   * show.
   *
   * **An ask moves the window once.** Without this it stands for as long as the
   * window does, and the effect below runs again on every change of project — so
   * somebody who chose another project from the switcher was carried straight
   * back to the one they were sent to, with nothing on screen saying why. The
   * ask is spent instead: by the project being open, which is it answered, or by
   * the open failing, which is it answered as well as it can be.
   *
   * Held rather than derived, because it outlives the ask that produced it: the
   * same person coming back to this project later is not the ask happening
   * again.
   */
  const [spent, setSpent] = useState<SentTo | null>(null);
  const [intent, setIntent] = useState<AreaIntent | null>(null);
  if (answered !== null && answered !== spent && path === answered.project) {
    setSpent(answered);
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
    // Already here, or already answered. Rust chose this window because it holds
    // the project, or holds nothing, so the first is the ordinary case rather
    // than a shortcut.
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
        // closed, moved or forgotten since the ask was made — and they all end
        // at a window that is now in front of the person, showing the list of
        // projects, which is a screen that explains itself.
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

/**
 * The latest of two asks, for a window that can be sent somewhere by either.
 *
 * Identity rather than truthiness: an area applies an intent it has not applied
 * yet, so what has to be returned is whichever object changed last — and
 * preferring one source over the other would mean an address followed while a
 * banner's intent was still standing was swallowed, or the reverse.
 */
export function useLatestIntent(
  ...intents: readonly (AreaIntent | null)[]
): AreaIntent | null {
  // Every slot starts empty rather than at what the first render held: a source
  // that already had an intent on the first render would otherwise never be
  // read, and which of them can is not this function's business.
  const [seen, setSeen] = useState<readonly (AreaIntent | null)[]>(() =>
    intents.map(() => null),
  );
  const [latest, setLatest] = useState<AreaIntent | null>(null);

  const moved = intents.findIndex((intent, at) => intent !== seen[at]);
  if (moved !== -1) {
    setSeen(intents);
    setLatest(intents[moved]);
  }

  return latest;
}
