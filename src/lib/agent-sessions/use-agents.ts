"use client";

import { useCallback, useEffect, useState } from "react";

import {
  agentAdapters,
  agentCatalog,
  liveSessions,
  type AgentDescriptor,
  type SessionRow,
} from "./client";

/**
 * An agent, plus whether the package it is reached through has been downloaded.
 *
 * Two reads rather than one field, because they answer to different things: the
 * catalogue is about this build and this machine's PATH, and the adapter is
 * about a directory that install filled and removing the extension empties.
 * `null` means the agent needs no adapter at all — every native one, and Codex,
 * whose bridge is compiled in.
 */
export interface Agent extends AgentDescriptor {
  readonly adapterReady: boolean | null;
}

/**
 * The agents this machine can raise.
 *
 * Read once and kept: what is installed changes when somebody installs
 * something, which is not during a render. `reload` is for after they have.
 */
export function useAgents(): {
  readonly agents: readonly Agent[];
  readonly isLoading: boolean;
  readonly reload: () => void;
} {
  // One piece of state, so that "which read this is" and "what it found" cannot
  // disagree — and so nothing has to be reset synchronously when a reload
  // starts, which is what makes an effect re-render in a cascade.
  const [read, setRead] = useState<{ nonce: number; agents: readonly Agent[] } | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let current = true;
    Promise.all([agentCatalog(), agentAdapters()])
      .then(([list, adapters]) => {
        if (!current) return;
        setRead({
          nonce,
          agents: list.map((agent) => ({
            ...agent,
            adapterReady:
              adapters.find((adapter) => adapter.agentId === agent.id)?.ready ?? null,
          })),
        });
      })
      .catch(() => {
        // Nothing found is a real answer here: the catalogue lists an agent that
        // is not installed rather than omitting it, so an empty list means the
        // command itself did not answer.
        if (current) setRead({ nonce, agents: [] });
      });
    return () => {
      current = false;
    };
  }, [nonce]);

  return {
    agents: read?.agents ?? [],
    isLoading: read === null || read.nonce !== nonce,
    reload: useCallback(() => setNonce((n) => n + 1), []),
  };
}

/** How often the running list is re-read while something is being watched. */
const HEARTBEAT_MS = 2_000;

/**
 * Every agent running right now, across every extension.
 *
 * This is what answers the two questions a person asks about a process the
 * application started for them — is it still going, and how do I stop it — and
 * it has to come from Rust rather than from React state, because the screen that
 * started an agent may be the one that is no longer mounted.
 *
 * Polled rather than pushed. A status change reaches the screen watching that
 * session immediately, on its own subscription; this list is the overview, and
 * an overview that is a second or two behind is not wrong in any way a person
 * can act on. The alternative — a second event channel per window — would be a
 * parallel truth to keep in step with the first.
 */
export function useLiveSessions(active = true): {
  readonly sessions: readonly SessionRow[];
  readonly reload: () => void;
} {
  const [sessions, setSessions] = useState<readonly SessionRow[]>([]);

  const read = useCallback(() => {
    liveSessions()
      .then(setSessions)
      .catch(() => setSessions([]));
  }, []);

  useEffect(() => {
    if (!active) return;
    read();
    const timer = setInterval(read, HEARTBEAT_MS);
    return () => clearInterval(timer);
  }, [active, read]);

  return { sessions, reload: read };
}
