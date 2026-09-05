"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { heldPlace, holdPlace, openRegistered } from "@/lib/project/client";
import type { OpenProject } from "@/lib/project/types";

/**
 * Where this phone was, restored before it is offered a list to choose from.
 *
 * **The webview reloads without anybody asking it to.** On a Mac a window is the
 * application and a reload is a key somebody pressed; here the system does it —
 * a phone comes back from the background, iOS reclaims the content process —
 * and everything React was holding goes with it. Of what is lost, one piece is
 * a person's own: the project they had open. Losing it puts them back at a list
 * they chose from an hour ago, for a reason they cannot see.
 *
 * So the key is kept in Rust, beside the pairing, and read back here before the
 * list is drawn. The project itself is not kept: it is opened by asking the
 * computer, exactly as tapping a row does, so a project renamed or removed while
 * this phone was in a pocket is answered by the computer rather than out of a
 * copy that went stale in it.
 *
 * A restore that fails is not reported. The reasons are all ordinary — the
 * computer is asleep, the project was closed on it, the network is not there
 * yet — and every one of them ends at the list of projects, which is a screen
 * that explains itself. An error above it would be explaining a thing the person
 * did not ask for.
 *
 * `holding` is what keeps the list from flashing up for one frame on the way
 * back in.
 */
export function usePlace(enabled: boolean): {
  /** The project this phone was in, once it has been opened again. */
  readonly restored: OpenProject | null;
  /** Whether the answer is still being waited for. */
  readonly holding: boolean;
  /** Write down where the phone is now, or that it is nowhere. */
  readonly hold: (project: OpenProject | null) => void;
} {
  const [restored, setRestored] = useState<OpenProject | null>(null);
  const [holding, setHolding] = useState(enabled);

  useEffect(() => {
    if (!enabled) return;
    let listening = true;
    void (async () => {
      try {
        const key = await heldPlace();
        // The key is all that was kept, so it stands in for the name as well:
        // a project whose own record answers replaces it in the same breath,
        // and one whose record is absent is a project that declares nothing and
        // is called what the computer registered it as.
        if (key !== null) {
          const opened = await openRegistered(key, key);
          if (listening) setRestored(opened);
        }
      } catch {
        // Ordinary, and answered by the list. See above.
      } finally {
        if (listening) setHolding(false);
      }
    })();
    return () => {
      listening = false;
    };
  }, [enabled]);

  const hold = useCallback(
    (project: OpenProject | null) => {
      if (!enabled) return;
      // Nothing is awaited and nothing is reported. What this buys is one tap
      // saved after a reload, and what refusing to navigate over a keychain
      // that would not answer would cost is the navigation.
      void holdPlace(project?.path ?? null).catch(() => {});
    },
    [enabled],
  );

  // Memoised, because the shell builds a callback out of `hold` and passes it
  // to screens: a fresh record every render would be a fresh callback every
  // render, and every screen holding one re-rendering with it.
  return useMemo(
    () => ({ restored, holding, hold }),
    [restored, holding, hold],
  );
}
