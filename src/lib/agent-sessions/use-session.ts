"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { WorktreeChoice } from "@/lib/worktrees/client";

import {
  Channel,
  cancel as cancelTurn,
  chooseMode,
  chooseOption,
  closeSession,
  forgetSession,
  openSession,
  prompt as sendPrompt,
  respondToPermission,
  subscribe,
  unsubscribe,
  type OpenedSession,
  type PastedContent,
  type SessionAbout,
  type SessionConfigOption,
  type SessionEvent,
  type SessionMode,
} from "./client";
import {
  EMPTY_TRANSCRIPT,
  foldTranscript,
  withDropped,
  type Transcript,
} from "./transcript";

/**
 * One conversation, as a screen reads and drives it.
 *
 * The session itself is in Rust and is addressed by key. This hook is a view of
 * it: it subscribes, folds the events into something readable, and hands back
 * the four things a person can do. Unmounting stops the watching and nothing
 * else — the agent goes on working, and remounting is handed everything that
 * happened meanwhile.
 */
export interface AgentSession {
  readonly key: string | null;
  readonly transcript: Transcript;
  /** What the agent lets a person choose, the model among it. */
  readonly configuration: readonly SessionConfigOption[];
  /**
   * The modes it works in — empty from an agent that has none.
   *
   * Which one is current is not here: it is `transcript.mode`, because two
   * things say it — the state the agent stated and its own
   * `current_mode_update` — and one field written twice in sequence is one
   * answer, where two fields would be two.
   */
  readonly modes: readonly SessionMode[];
  /** True while a turn is being sent or run. */
  readonly isWorking: boolean;
  /**
   * Runs one turn. `attachments` are absolute paths the agent reads itself;
   * `images` are pasted pictures, which have no path because they have no file.
   */
  readonly prompt: (
    text: string,
    attachments?: readonly string[],
    images?: readonly PastedContent[],
  ) => Promise<void>;
  readonly cancel: () => Promise<void>;
  /** Answers the open question. `null` withdraws it. */
  readonly answer: (optionId: string | null) => Promise<void>;
  readonly choose: (configId: string, valueId: string) => Promise<void>;
  /** Puts the session into one of {@link AgentSession.modes}. */
  readonly setMode: (modeId: string) => Promise<void>;
}

/**
 * What has been read, and which session it was read from.
 *
 * The key is held *with* the reading rather than beside it, so that switching
 * conversations needs nothing cleared: a reading whose key is not the one being
 * asked for is simply not this session's, and the empty transcript is what
 * shows until the new subscription has said otherwise. Clearing it in an effect
 * instead would render one frame of the previous conversation under the new
 * one's name.
 */
interface Reading {
  readonly key: string | null;
  readonly transcript: Transcript;
  readonly configuration: readonly SessionConfigOption[];
  readonly modes: readonly SessionMode[];
}

/** One array, so an absent configuration is the same value every time. */
const NO_CONFIGURATION: readonly SessionConfigOption[] = [];

/** The same, for an agent that offers no modes. */
const NO_MODES: readonly SessionMode[] = [];

const NOTHING_READ: Reading = {
  key: null,
  transcript: EMPTY_TRANSCRIPT,
  configuration: NO_CONFIGURATION,
  modes: NO_MODES,
};

export function useAgentSession(key: string | null): AgentSession {
  const [read, setRead] = useState<Reading>(NOTHING_READ);
  // The fold runs against the latest transcript without the effect below having
  // to re-subscribe every time one arrives — a re-subscription would replay the
  // whole history into a transcript that already holds it.
  const held = useRef<Transcript>(EMPTY_TRANSCRIPT);
  const transcript = read.key === key ? read.transcript : EMPTY_TRANSCRIPT;
  // Its own memo, so that a reading which belongs to another session hands back
  // the same empty array every render rather than a new one — otherwise every
  // consumer of this hook re-renders on every render of it.
  const configuration = useMemo(
    () => (read.key === key ? read.configuration : NO_CONFIGURATION),
    [read, key],
  );
  const modes = useMemo(
    () => (read.key === key ? read.modes : NO_MODES),
    [read, key],
  );

  useEffect(() => {
    held.current = EMPTY_TRANSCRIPT;
    if (key === null) return;

    let watching = true;
    const events = new Channel<SessionEvent>();
    events.onmessage = (event) => {
      if (!watching) return;
      held.current = foldTranscript(held.current, event);
      const folded = held.current;
      setRead((previous) => ({
        key,
        transcript: folded,
        configuration: nextConfiguration(
          previous.key === key ? previous.configuration : NO_CONFIGURATION,
          event,
        ),
        // Only a mode event restates the list. An agent moving itself between
        // modes says so with `current_mode_update`, which changes which one is
        // current and not what there is to choose from.
        modes:
          event.kind === "modes"
            ? event.modes.availableModes
            : previous.key === key
              ? previous.modes
              : NO_MODES,
      }));
    };

    subscribe(key, events)
      .then((dropped) => {
        if (!watching || dropped === 0) return;
        held.current = withDropped(held.current, dropped);
        const folded = held.current;
        setRead((previous) => ({ ...previous, key, transcript: folded }));
      })
      .catch(() => {
        // The session is gone. Nothing to watch and nothing to report that the
        // screen does not already know from its own list.
      });

    return () => {
      watching = false;
      void unsubscribe(key);
    };
  }, [key]);

  const prompt = useCallback(
    async (
      text: string,
      attachments: readonly string[] = [],
      images: readonly PastedContent[] = [],
    ) => {
      // A turn has to carry something. An attached file or a pasted picture is
      // something: "look at this" is a whole request, and refusing it because
      // the field was empty would be this window deciding what counts as asking.
      if (
        key === null ||
        (text.trim() === "" && attachments.length === 0 && images.length === 0)
      ) {
        return;
      }
      // Nothing is added to the transcript here. The host records what was said
      // before it sends it, so the line arrives back on the subscription like
      // everything else — which is what makes it survive leaving the section.
      await sendPrompt(key, text, attachments, images);
    },
    [key],
  );

  const cancel = useCallback(async () => {
    if (key !== null) await cancelTurn(key);
  }, [key]);

  const answer = useCallback(
    async (optionId: string | null) => {
      const question = held.current.question;
      if (key === null || question === null) return;
      await respondToPermission(key, question.requestId, optionId);
    },
    [key],
  );

  const choose = useCallback(
    async (configId: string, valueId: string) => {
      if (key === null) return;
      const restated = await chooseOption(key, configId, valueId);
      setRead((previous) => ({ ...previous, key, configuration: restated }));
    },
    [key],
  );

  // Nothing is written here beyond the list. Which mode is now current arrives
  // on the subscription — the host restates the whole state after the agent
  // agrees — so setting it here as well would be this screen answering a
  // question the session has already answered.
  const setMode = useCallback(
    async (modeId: string) => {
      if (key === null) return;
      const restated = await chooseMode(key, modeId);
      setRead((previous) => ({ ...previous, key, modes: restated.availableModes }));
    },
    [key],
  );

  return useMemo(
    () => ({
      key,
      transcript,
      configuration,
      modes,
      isWorking: transcript.status === "working",
      prompt,
      cancel,
      answer,
      choose,
      setMode,
    }),
    [key, transcript, configuration, modes, prompt, cancel, answer, choose, setMode],
  );
}

/**
 * The configuration after one event.
 *
 * Two things restate it, and both have to be heard. `session/new` and
 * `session/set_config_option` come through as a configuration event; an agent
 * that changes its own options mid-session says so as a `config_option_update`,
 * and a window that ignored those would keep offering a model the agent has
 * already moved off.
 */
function nextConfiguration(
  current: readonly SessionConfigOption[],
  event: SessionEvent,
): readonly SessionConfigOption[] {
  if (event.kind === "configuration") return event.options;
  if (event.kind !== "update" || event.update !== "config_option_update") return current;

  const payload = event.payload as Record<string, unknown>;
  // Either the whole set, or one option to put back in its place. Both shapes
  // have been seen; neither is worth guessing wrong about.
  const whole = payload.configOptions;
  if (Array.isArray(whole)) return whole as SessionConfigOption[];

  const one = (payload.configOption ?? payload) as SessionConfigOption;
  if (typeof one.id !== "string") return current;
  const at = current.findIndex((option) => option.id === one.id);
  if (at === -1) return [...current, one];
  return current.map((option, index) => (index === at ? one : option));
}

/** Raising an agent, for a screen that is about to watch what it says. */
export async function startSession(args: {
  agentId: string;
  cwd: string;
  model?: string | null;
  /** The record it is being opened under, for a screen that opened it from one. */
  about?: SessionAbout | null;
  /**
   * Where to work: the project itself when this is absent, a working tree made
   * now (`"new"`), or one that already exists, by its path.
   *
   * Chosen when the conversation is opened and fixed for its life — the
   * directory has gone to the agent by the time there is anything to change it
   * from.
   */
  worktree?: WorktreeChoice | null;
}): Promise<OpenedSession> {
  return openSession(args);
}

/** Stopping an agent, and keeping what it said. */
export async function stopSession(key: string): Promise<void> {
  return closeSession(key);
}

/** Deleting a conversation, stopping its agent first if it is still running. */
export async function deleteSession(key: string): Promise<void> {
  return forgetSession(key);
}
