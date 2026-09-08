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
  historyBefore,
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
  precede,
  withDropped,
  withEarlier,
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
  /**
   * Reads further back, for a screen somebody has scrolled to the top of.
   *
   * A conversation arrives at its end — see `subscribe` — so `transcript.earlier`
   * is where this reading begins and this is what moves it back. It answers
   * when the reading has actually grown, so a screen may await it; asking again
   * while one is in flight does nothing, and asking at the start of the
   * conversation does nothing either.
   */
  readonly loadEarlier: () => Promise<void>;
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

/**
 * How long a reading may wait for a frame that never comes.
 *
 * Folding an event and drawing what it folded into are separated on purpose,
 * and the separation is the whole of why a long conversation opens at once
 * rather than pouring in. The backlog is replayed **one event per message**,
 * and a channel hands each of them over in a task of its own — so a screen that
 * drew on every message drew the whole conversation once per event in it, which
 * is quadratic in its length and is what a person feels as the section
 * hesitating. The events are cheap; the drawing is not.
 *
 * A frame is the right clock for it. Everything that arrives inside one is one
 * render, and nothing can be seen more often than that anyway. But a window
 * that is hidden, minimised or fully occluded is given no frames at all while
 * its agents go on working, so a timer runs beside the frame and whichever
 * comes first draws — otherwise a conversation held in the background would
 * arrive all at once on being looked at, which is the same failure moved.
 */
const DRAW_WITHIN_MS = 100;

export function useAgentSession(key: string | null): AgentSession {
  const [read, setRead] = useState<Reading>(NOTHING_READ);
  // What the events add up to, whether or not it has been drawn yet. The fold
  // runs against this without the effect below having to re-subscribe every
  // time one arrives — a re-subscription would replay the whole history into a
  // transcript that already holds it — and it is the whole reading rather than
  // the transcript alone because what a person chose is written here too.
  const held = useRef<Reading>(NOTHING_READ);
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
    held.current = {
      key,
      transcript: EMPTY_TRANSCRIPT,
      configuration: NO_CONFIGURATION,
      modes: NO_MODES,
    };
    if (key === null) return;

    let watching = true;
    let frame: number | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;

    /** Puts everything folded so far on the screen, in one render. */
    const draw = () => {
      if (frame !== null) cancelAnimationFrame(frame);
      if (timer !== null) clearTimeout(timer);
      frame = null;
      timer = null;
      if (watching) setRead(held.current);
    };

    /** Asks for that render, if one is not already asked for. */
    const drawSoon = () => {
      if (frame !== null || timer !== null) return;
      frame = requestAnimationFrame(draw);
      timer = setTimeout(draw, DRAW_WITHIN_MS);
    };

    const events = new Channel<SessionEvent>();
    events.onmessage = (event) => {
      if (!watching) return;
      held.current = {
        key,
        transcript: foldTranscript(held.current.transcript, event),
        configuration: nextConfiguration(held.current.configuration, event),
        // Only a mode event restates the list. An agent moving itself between
        // modes says so with `current_mode_update`, which changes which one is
        // current and not what there is to choose from.
        modes: event.kind === "modes" ? event.modes.availableModes : held.current.modes,
      };
      drawSoon();
    };

    subscribe(key, events)
      .then((began) => {
        if (!watching) return;
        held.current = {
          ...held.current,
          transcript: withEarlier(
            withDropped(held.current.transcript, began.dropped),
            began.earlierThan,
          ),
        };
        // Drawn here rather than left to the schedule above, because the
        // backlog has been handed over by the time this settles and there may
        // be nothing further coming: a dormant conversation emits no events of
        // its own, and one waiting for a next event would stay blank.
        draw();
      })
      .catch(() => {
        // The session is gone. Nothing to watch and nothing to report that the
        // screen does not already know from its own list.
      });

    return () => {
      watching = false;
      if (frame !== null) cancelAnimationFrame(frame);
      if (timer !== null) clearTimeout(timer);
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

  // Whether a page is already on its way. A ref rather than state because
  // nothing is drawn differently for it: it exists to stop a list that is
  // sitting at the top from asking for the same page on every frame.
  const fetching = useRef(false);

  const loadEarlier = useCallback(async () => {
    if (key === null || fetching.current) return;
    fetching.current = true;
    try {
      // A page is counted in events and a reading in blocks, and the two are
      // nowhere near the same number: a run of tool updates is one line, and a
      // whole page of them would leave the screen exactly as it was — with
      // somebody still at the top of it, having asked for more and been given
      // nothing. So this asks again until the reading grew, or until there is
      // nothing earlier left to ask for.
      const was = held.current.transcript.entries.length;
      while (held.current.key === key) {
        const before = held.current.transcript.earlier;
        if (before === null) break;
        const page = await historyBefore(key, before);
        // The conversation was changed under this while the page was in flight.
        // What came back belongs to a transcript nothing is reading any more.
        if (held.current.key !== key) return;
        const older = page.events.reduce(foldTranscript, EMPTY_TRANSCRIPT);
        held.current = {
          ...held.current,
          transcript: precede(held.current.transcript, older.entries, page.earlierThan),
        };
        if (held.current.transcript.entries.length > was) break;
      }
      setRead(held.current);
    } catch (error) {
      // A page that could not be read leaves the reading exactly where it is,
      // `earlier` included — so reaching the top again asks for it again. It is
      // reported to the console rather than to the person, and never thrown:
      // this is reached by scrolling, and somebody who scrolled to the top of a
      // conversation did not ask a question that deserves an error over the
      // conversation itself. Whatever earlier pages did arrive are drawn.
      console.warn("What was said earlier could not be read.", error);
      if (held.current.key === key) setRead(held.current);
    } finally {
      fetching.current = false;
    }
  }, [key]);

  const answer = useCallback(
    async (optionId: string | null) => {
      const question = held.current.transcript.question;
      if (key === null || question === null) return;
      await respondToPermission(key, question.requestId, optionId);
    },
    [key],
  );

  const choose = useCallback(
    async (configId: string, valueId: string) => {
      if (key === null) return;
      const restated = await chooseOption(key, configId, valueId);
      // Into the held reading first, so that the next event folded on top of it
      // carries the choice forward. Written straight to the screen as well,
      // because nothing else is going to arrive on account of it.
      if (held.current.key !== key) return;
      held.current = { ...held.current, configuration: restated };
      setRead(held.current);
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
      if (held.current.key !== key) return;
      held.current = { ...held.current, modes: restated.availableModes };
      setRead(held.current);
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
      loadEarlier,
      answer,
      choose,
      setMode,
    }),
    [
      key,
      transcript,
      configuration,
      modes,
      prompt,
      cancel,
      loadEarlier,
      answer,
      choose,
      setMode,
    ],
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
  /**
   * The conversation this one is being delegated from, by the agent's own id
   * for it — `acpSession`, as a row and a pointer alike carry it.
   *
   * A conversation opened this way is filed where its parent is: what it is
   * about and who ordered it are read from the parent rather than taken beside
   * this, so a package cannot file work under a record it has nothing to do
   * with. A chain of them is two conversations deep, and a third is refused.
   */
  parent?: string | null;
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
