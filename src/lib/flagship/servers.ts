"use client";

/**
 * The servers the person already has, read off the flagship's own
 * configuration.
 *
 * The list a package is agreed against, and the only list there is: Sync holds
 * no servers of its own and no key of anybody's, so what a package may be
 * allowed to call is whatever the agent this installation works through was
 * already set up to reach.
 *
 * **No network and no model.** The file is read in Rust, so the rows are there
 * the moment a page is opened, the same from one launch to the next, and there
 * whether or not that program has ever been started.
 *
 * Nothing is cached beyond a render, for the reason the agreements beside it
 * are not: the file is the person's own and they may edit it while this window
 * is open, so each screen asks when it opens rather than trusting an answer
 * from earlier in the session.
 */

import { useEffect, useMemo, useState } from "react";

import { command } from "@/lib/command";
import { said } from "@/lib/refusal";

/**
 * How a server is reached, as its own entry spells it.
 *
 * Two shapes and an absence, which is what every one of the configuration
 * formats Sync reads comes down to: an address to dial, or a program to start.
 * `unstated` is an entry that says neither, and it is carried rather than
 * dropped — a server missing from a list reads as a server the person does not
 * have.
 */
export type Reach =
  | { readonly kind: "http"; readonly url: string }
  | { readonly kind: "process"; readonly command: string }
  | { readonly kind: "unstated" };

/** One server, exactly as the flagship's own configuration names it. */
export interface FlagshipServer {
  /** The key, spelled as that file spells it. */
  readonly name: string;
  readonly reach: Reach;
}

/** Why there is no list, in the words of whoever refused it. */
export interface ServerRefusal {
  /**
   * What was refused, as a name a screen can branch on, or `null`.
   *
   * One of these is not like the others: `no_flagship` is not a failure at all,
   * it is a machine where nobody has chosen an agent yet, and what it needs is
   * somewhere to go rather than a sentence about a file.
   */
  readonly kind: string | null;
  /** The refuser's own words. Never rewritten. */
  readonly message: string;
}

/** What a screen has of somebody's own servers. */
export interface FlagshipServers {
  readonly servers: readonly FlagshipServer[];
  /** True until the answer has arrived. */
  readonly isLoading: boolean;
  /** Why there is no list at all, or `null`. */
  readonly refusal: ServerRefusal | null;
}

/** The servers the chosen agent's own configuration names. */
export function loadFlagshipServers(): Promise<readonly FlagshipServer[]> {
  return command<readonly FlagshipServer[]>("flagship_servers", {});
}

const NOTHING: readonly FlagshipServer[] = [];

/** What the one ask answered with. */
interface Answered {
  readonly servers: readonly FlagshipServer[];
  readonly refusal: ServerRefusal | null;
}

/**
 * Ask once, when the screen that needs the list is mounted.
 *
 * No way to ask again, deliberately. What would change this answer is somebody
 * editing a file in their home directory in another application, which is not
 * something this window is told about — and a screen that re-read it on every
 * render would spend a file read per keystroke to be wrong slightly less often.
 */
export function useFlagshipServers(): FlagshipServers {
  const [answered, setAnswered] = useState<Answered | null>(null);

  useEffect(() => {
    let current = true;

    void loadFlagshipServers()
      .then((servers) => {
        if (current) setAnswered({ servers, refusal: null });
      })
      .catch((refused: unknown) => {
        if (current) {
          setAnswered({ servers: NOTHING, refusal: refusalOf(refused) });
        }
      });

    return () => {
      current = false;
    };
  }, []);

  return useMemo(
    () => ({
      servers: answered?.servers ?? NOTHING,
      // Derived rather than held, so nothing is written to state as the effect
      // starts: the ask is outstanding exactly while no answer has arrived.
      isLoading: answered === null,
      refusal: answered?.refusal ?? null,
    }),
    [answered],
  );
}

function refusalOf(refused: unknown): ServerRefusal {
  const kind =
    refused !== null &&
    typeof refused === "object" &&
    typeof (refused as { kind?: unknown }).kind === "string"
      ? (refused as { kind: string }).kind
      : null;

  return { kind, message: said(refused) };
}
