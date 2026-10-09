"use client";

import { Channel } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  type PermissionRequest,
  type SessionEvent,
  type SessionRow,
  cancel,
  closeSession,
  prompt,
  respondToPermission,
  subscribe,
  unsubscribe,
} from "@/lib/agent-sessions/client";
import { EMPTY_TRANSCRIPT, type Transcript, foldTranscript } from "@/lib/agent-sessions/transcript";
import { command } from "@/lib/command";
import { type Named, refusal } from "@/lib/console/resolve";
import type { BlockState, Outcome, Presentation } from "@/lib/console/types";

/**
 * The work a console has addressed, from the moment it is asked for to the
 * moment there is something to print.
 *
 * **The transcript is watched and not shown.** What the agent thought, which
 * tools it ran and how many times it changed its mind are the conversation's,
 * and the conversation is in the area that draws conversations. What comes back
 * here is the last thing it said — one block, in the order things finished.
 *
 * That is the line between this and the chat, and it is drawn here rather than
 * argued about: nothing in this module can grow into a second transcript
 * viewer, because nothing in it keeps one after the work ends.
 *
 * **A name is an address, and the address is the session's own title.** Not a
 * map held by the window: a window reloads and the work goes on, so a name that
 * lived here would be an address that stops resolving while the thing it names
 * is still running. What is held here is a shortcut to a lookup, and it is
 * refilled from the host whenever it misses.
 */

/**
 * A piece of work that is still going, as the shelf above the input line draws
 * it.
 *
 * **What it is doing, and never what it is thinking.** The one line here is the
 * title of the tool call the agent is in, which arrives on the subscription as
 * a title already — there is nothing to invent and nothing to summarise.
 * Reasoning is the conversation's, and the conversation is elsewhere.
 */
export interface Running {
  /**
   * This window's name for the work, minted when it was asked for.
   *
   * Not the session key: a line put on the shelf the moment Return is pressed
   * has no session yet, and a row that appeared a second later would miss the
   * one thing the shelf is for — saying that the line was taken.
   */
  readonly id: string;
  /** What it is addressed by. */
  readonly name: string;
  /** The tool it is in, or nothing before it has run one. */
  readonly doing: string | null;
  readonly startedAt: number;
  /** Whether it is waiting on an answer only a person can give. */
  readonly asking: boolean;
  /** Whether it is waiting for an agent to come free rather than working. */
  readonly queued: boolean;
}

/**
 * How many pieces of work may hold an agent at once.
 *
 * A process per session is how the host raises one, so this is a count of
 * processes: ten lines addressed in an afternoon would otherwise be ten agents
 * resident at the same time. Four is a default and not a measurement — what it
 * costs on this machine has not been measured, and the number moves when it is.
 *
 * What is over the ceiling waits rather than being refused. Somebody who typed
 * a line meant it, and a console that dropped the fifth because four were busy
 * would make them watch the shelf to know when to type again.
 */
const AT_ONCE = 4;

/** A question one piece of work is stopped on, and what may answer it. */
interface Question {
  readonly requestId: number;
  readonly options: PermissionRequest["options"];
}

/** A piece of work that has finished, in the shape a block is made from. */
export interface Finished {
  /** The tab it was asked for in, which is where its block belongs. */
  readonly tabId: string;
  readonly typed: string;
  readonly state: BlockState;
  readonly result: Presentation;
  readonly startedAt: number;
  readonly endedAt: number;
}

/** Start a piece of work under a name, and answer with the session it raised. */
function startWork(project: string, name: string, text: string): Promise<SessionRow> {
  return command<SessionRow>("console_work_start", { project, name, text });
}

/**
 * What the console is running in this project, finished ones included.
 *
 * Exported because a name is resolved against this list wherever it is read,
 * and there is one list: the panel showing the conversation behind a name it
 * was given asks the same question this file asks before it addresses one.
 */
export function works(project: string): Promise<SessionRow[]> {
  return command<SessionRow[]>("console_works", { project });
}

/**
 * What a refusal from the host actually said.
 *
 * A command that fails answers with the shape Rust threw — `{ kind, message }`
 * — and not with an `Error`, so a handler that only understood `Error` would
 * print its own guess instead of the one sentence that was written for the
 * person: *no agent is chosen*, *that agent is not installed*, *it would not
 * start*. Three different problems, three different next moves, and all three
 * would read as *that could not be started*.
 *
 * This window has been here before, in fifteen places at once, printing
 * `[object Object]`.
 */
function said(error: unknown): string {
  if (typeof error === "string" && error.trim().length > 0) return error;
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === "string" && message.trim().length > 0) return message;
  }
  return "that could not be started";
}

/**
 * Matched without regard to case, which is the rule the chat already keeps.
 * `@Review` and `@review` are one name to whoever typed them, and treating them
 * as two would raise a second agent for a shift key.
 */
function sameName(a: string | null, b: string): boolean {
  return (a ?? "").trim().toLowerCase() === b.trim().toLowerCase();
}

/**
 * The last thing the agent said **in this turn**, which is the whole of what is
 * printed.
 *
 * `since` is the index in the transcript where this turn began — everything
 * before it was replayed from a conversation already had. Without it, a turn
 * that has not yet folded its own answer (events arriving out of order on the
 * subscription) would settle with the previous turn's answer, and a person
 * asking a second question would be handed back the first one.
 *
 * An answer with nothing in it is not an empty string: an agent that ran tools
 * and said nothing has done something, and a block showing a blank line would
 * read as a console that lost it.
 */
function answered(transcript: Transcript, since: number): string | null {
  for (let index = transcript.entries.length - 1; index >= since; index -= 1) {
    const entry = transcript.entries[index];
    if (entry?.voice === "agent") {
      const text = entry.text.trim();
      return text.length === 0 ? null : text;
    }
  }
  return null;
}

/** The tool the agent is in, which is the whole of what the shelf says. */
function doingNow(transcript: Transcript): string | null {
  for (let index = transcript.entries.length - 1; index >= 0; index -= 1) {
    const entry = transcript.entries[index];
    if (entry?.voice === "tool") return entry.title;
  }
  return null;
}

/**
 * What went wrong, in the words it went wrong in.
 *
 * The agent's own trouble before this module's: a session that fell over says
 * why on its own transcript, and a sentence written here would be a guess
 * standing in front of the answer.
 */
function trouble(transcript: Transcript): string | null {
  for (let index = transcript.entries.length - 1; index >= 0; index -= 1) {
    const entry = transcript.entries[index];
    if (entry?.voice === "trouble") return entry.text;
  }
  return null;
}

export function useWorks(
  project: string,
  finished: (work: Finished) => void,
): {
  /**
   * Address a piece of work by name: the first line under a name starts it,
   * every line after it says something more into the same one.
   */
  readonly address: (tabId: string, name: string, text: string) => void;
  /** What is going, oldest first, for the shelf to draw. */
  readonly running: readonly Running[];
  /** Carry out one of the window's own verbs, and say how it went. */
  readonly perform: (verb: string, args: readonly string[]) => Promise<Outcome>;
  /**
   * Every name this project answers to, as the input line offers them.
   *
   * Read from the host rather than remembered here, and refreshed whenever the
   * shelf changes: a name is an address, the addresses belong to the project,
   * and a window that offered only what it had typed itself would forget them
   * all on reload while the work went on running.
   */
  readonly names: readonly Named[];
} {
  const [running, setRunning] = useState<readonly Running[]>([]);
  const [names, setNames] = useState<readonly Named[]>([]);
  // Held in a ref so that work already running is not watching a callback from
  // the render it was started in — the tab it belongs to and the line it came
  // from are its own, and the only thing it needs from outside is somewhere to
  // deliver.
  const deliver = useRef(finished);
  useEffect(() => {
    deliver.current = finished;
  }, [finished]);

  /** Names this window has already resolved, lowered. A cache, not the truth. */
  const known = useRef(new Map<string, string>());

  /**
   * Names being raised right now.
   *
   * Two lines addressed to an unused name in the same breath both miss the
   * cache and both miss the host's list, and the project ends with two pieces
   * of work under one name — of which only the second is addressable. The
   * second line waits for the first to have a session and then speaks into it.
   */
  const raising = useRef(new Map<string, Promise<string>>());

  /** What each piece of work is stopped on, while it is stopped on one. */
  const asked = useRef(new Map<string, Question>());

  /**
   * How many agents are held right now, and what is waiting for one.
   *
   * In refs rather than in the shelf's state, and the reason is timing: a line
   * typed twice in one frame would read the same count twice from state and
   * raise two agents past the ceiling. What the shelf holds is for drawing;
   * this is for deciding.
   */
  const held = useRef(0);
  const waiting = useRef<(() => void)[]>([]);

  /**
   * How to end a watch from outside it.
   *
   * `close` kills the process, and a watch waiting for a turn to finish would
   * wait for one that is never coming. The block it owes still has to be
   * printed, so the way to end it is kept where a verb can reach it.
   */
  const stoppers = useRef(new Map<string, (why: string) => void>());

  /**
   * How many lines this console has handed to an agent.
   *
   * Only ever appended to a shelf row's identity, so that two lines addressed
   * to one name in the same millisecond are two rows rather than one.
   */
  const minted = useRef(0);

  // What is being watched, so that unmounting stops watching it. The sessions
  // go on running — they belong to the project, not to this screen — but a
  // window closed while three were going would otherwise leave three channels
  // reading into a component nobody can see.
  const watching = useRef(new Set<string>());
  useEffect(() => {
    const open = watching.current;
    return () => {
      for (const key of open) void unsubscribe(key);
      open.clear();
    };
  }, []);

  // Asked once when the console appears and again whenever something starts or
  // finishes — the two moments the set of names can have changed. A poll would
  // ask a hundred times between them for an answer that changes twice a day.
  useEffect(() => {
    let reading = true;
    void works(project)
      .then((rows) => {
        if (!reading) return;
        const seen = new Set<string>();
        const called: Named[] = [];
        for (const row of rows) {
          const name = row.title?.trim() ?? "";
          if (name.length === 0 || seen.has(name.toLowerCase())) continue;
          seen.add(name.toLowerCase());
          // What it is doing, in the words the host uses for it. A list of
          // names with nothing beside them is a list somebody has to remember
          // rather than read.
          called.push({ name, hint: row.status });
        }
        setNames(called);
      })
      .catch(() => {
        // The list is an offer, not an answer. A console that refused to take
        // a line because it could not read the names would be worse than one
        // that offers none.
      });
    return () => {
      reading = false;
    };
  }, [project, running.length]);

  const address = useCallback(
    (tabId: string, name: string, text: string) => {
      const startedAt = Date.now();
      const typed = `@${name} ${text}`;
      // Minted here rather than taken from the session, because the shelf has
      // to show this line before there is a session to take one from.
      const id = `${name}-${startedAt}-${minted.current}`;
      minted.current += 1;

      // On the shelf in the same frame the line left the input. This is the
      // whole of what tells a person their line was taken: no block is drawn
      // until there is an answer, so without this the console would look like
      // it had swallowed what they typed.
      // Queued from the start when four are already holding an agent. The row
      // stands on the shelf either way — what a person needs to know is that
      // their line was taken, and whether it is waiting is the second thing.
      setRunning((shelf) => [
        ...shelf,
        {
          id,
          name,
          doing: null,
          startedAt,
          asking: false,
          queued: shelf.filter((one) => !one.queued).length >= AT_ONCE,
        },
      ]);
      const change = (how: (work: Running) => Running) => {
        setRunning((shelf) => shelf.map((work) => (work.id === id ? how(work) : work)));
      };
      const leave = () => {
        setRunning((shelf) => shelf.filter((work) => work.id !== id));
      };

      /** An agent has come free: take the next line that was waiting for one. */
      const release = () => {
        held.current = Math.max(0, held.current - 1);
        const next = waiting.current.shift();
        if (next === undefined) return;
        held.current += 1;
        next();
      };

      const done = (state: BlockState, result: Presentation) => {
        leave();
        release();
        deliver.current({
          tabId,
          typed,
          state,
          result,
          startedAt,
          endedAt: Date.now(),
        });
      };

      /**
       * Follow one piece of work until it has something to say.
       *
       * A subscription replays what the session already holds before it carries
       * anything new, so the replay is folded and not acted on: a piece of work
       * spoken to a second time holds the whole of the first answer, and a rule
       * that read a status out of it would print that answer again the moment
       * the second question was asked.
       *
       * `awaited` is which side of the turn this is on. Work just started has
       * been asked already and its `working` is in the replay; work being asked
       * again has not, and its turn begins after the replay ends.
       */
      const follow = (key: string, awaited: boolean) => {
        let transcript = EMPTY_TRANSCRIPT;
        let working = false;
        let settled = false;
        let live = false;
        /**
         * Where the transcript ended when the replay did, so `answered` searches
         * only what this turn said. Without it, a turn whose own answer has not
         * arrived yet settles with the previous one's.
         */
        let sinceTurn = 0;
        /**
         * Whether a live (non-replayed) `agent_message_chunk` has been folded
         * for this turn.
         *
         * `sinceTurn` is set in the `.then()` callback, but `evaluateJavaScript`
         * does not guarantee that backlog events are processed before it. A
         * replayed `agent_message_chunk` arriving after `live = true` would be
         * folded past `sinceTurn`, and `answered` would return the previous
         * turn's text. This flag is the backstop: it is set only by a live
         * update, and `finishIfReady` will not settle with text unless it is
         * true.
         */
        let saidThisTurn = false;
        /** A terminal status that arrived while the replay was still coming. */
        let ended: "failed" | "ready" | "ended" | null = null;
        /**
         * A turn that ended without folding any of its own agent text.
         *
         * The `ready` status and the `agent_message_chunk` updates that precede
         * it travel as separate events, and on a multi-threaded host they can
         * arrive out of order: the status reaches `onmessage` before the text
         * does. Settling immediately would print the previous turn's answer (or
         * nothing), so the turn is held open briefly — if the text arrives
         * within the grace window, it is used; if not, the turn genuinely had
         * nothing to say.
         */
        let pendingReady = false;
        let pendingTimer: ReturnType<typeof setTimeout> | null = null;

        const clearPending = () => {
          pendingReady = false;
          if (pendingTimer !== null) {
            clearTimeout(pendingTimer);
            pendingTimer = null;
          }
        };

        const settle = (state: BlockState, result: Presentation) => {
          if (settled) return;
          settled = true;
          clearPending();
          // Not unsubscribing here: the call is fire-and-forget, and a
          // `session_unsubscribe` that lands after the next `session_subscribe`
          // clears that subscription's sink and forwarder, losing every event
          // the turn would have heard. The next `subscribe` replaces both, and
          // the forwarder from the settled turn exits when its sender is
          // dropped. The unmount effect below unsubscribes whatever is still
          // held.
          stoppers.current.delete(key);
          asked.current.delete(key);
          done(state, result);
        };
        stoppers.current.set(key, (why) => settle("failed", refusal(why)));

        /**
         * Answers this turn and settles, or holds the turn open if the agent's
         * text has not arrived yet.
         */
        const finishIfReady = () => {
          if (!pendingReady && !(working && live)) return;
          // A turn that has not produced its own agent text cannot settle
          // with text — `answered` might find the previous turn's words,
          // because `sinceTurn` is unreliable when backlog events arrive out
          // of order. Wait for a live `agent_message_chunk` or the grace
          // timer.
          if (saidThisTurn) {
            const said = answered(transcript, sinceTurn);
            if (said !== null) {
              settle("done", { view: "markdown", text: said });
              return;
            }
          }
          // The text may still be in flight. Hold the turn open for a brief
          // grace window; `onmessage` checks again each time an update folds.
          if (!pendingReady) {
            pendingReady = true;
            pendingTimer = setTimeout(() => {
              pendingTimer = null;
              if (!pendingReady || settled) return;
              pendingReady = false;
              settle("failed", refusal("the agent finished without saying anything"));
            }, 200);
          }
        };

        const events = new Channel<SessionEvent>();
        events.onmessage = (event) => {
          transcript = foldTranscript(transcript, event);
          if (event.kind === "permission") {
            asked.current.set(key, {
              requestId: event.requestId,
              options: event.request.options,
            });
          }
          if (event.kind === "permissionSettled") asked.current.delete(key);
          if (live) {
            const doing = doingNow(transcript);
            const asking = event.kind === "status" ? event.status === "asking" : undefined;
            change((work) =>
              work.doing === doing && (asking === undefined || work.asking === asking)
                ? work
                : { ...work, doing, asking: asking ?? work.asking },
            );
          }
          // Track whether the agent has spoken in this turn. A live
          // `agent_message_chunk` (not from the backlog) is the only thing
          // that sets it — `answered` is not trusted without it, because
          // `sinceTurn` is unreliable when backlog events arrive out of order.
          if (
            event.kind === "update" &&
            event.update === "agent_message_chunk" &&
            event.replayed !== true
          ) {
            saidThisTurn = true;
          }
          // While waiting for delayed agent text after "ready", any non-status
          // event might be the chunk that carries it.
          if (pendingReady && saidThisTurn) {
            const said = answered(transcript, sinceTurn);
            if (said !== null) {
              settle("done", { view: "markdown", text: said });
              return;
            }
          }
          if (event.kind !== "status") return;

          // "working" is needed from both replay and live: a new session whose
          // turn started before this side subscribed has it only in the backlog,
          // and without it `ready` cannot end the turn. It sets `working` and
          // clears any pending grace window regardless of origin.
          if (event.status === "working") {
            working = true;
            // A live "working" starts a new turn: reset the flag that says
            // the agent has spoken in this one. Replayed "working" (from
            // the backlog) does not reset it — that turn is already over.
            if (event.replayed !== true) {
              saidThisTurn = false;
            }
            clearPending();
            return;
          }

          // A replayed terminal status that reached here after `live = true` —
          // the ordering of `evaluateJavaScript` is not guaranteed — is from the
          // previous turn, not this one. It must not end the turn or settle with
          // the previous answer. Only recorded for the post-subscribe check
          // when it arrived before `live`.
          if (event.replayed === true) {
            if (!live) {
              if (
                event.status === "failed" ||
                event.status === "ready" ||
                event.status === "ended"
              ) {
                ended = event.status;
              }
            }
            return;
          }

          // What the replay carried is where the work has been rather than
          // where it is going — but an end in it is still an end: a turn that
          // failed fast is over before this side finished subscribing, and
          // discarding that leaves a row on the shelf for ever and an agent
          // held against the ceiling. Kept, and acted on once the replay is
          // through.
          if (!live) {
            if (event.status === "failed" || event.status === "ready" || event.status === "ended") {
              ended = event.status;
            }
            return;
          }
          if (event.status === "failed") {
            settle(
              "failed",
              refusal(
                trouble(transcript) ?? event.detail ?? "the agent stopped without saying why",
              ),
            );
            return;
          }
          // `ready` is also what a session says before it has been asked
          // anything, so an end is only an end once a turn has been seen to
          // start. Without that, every piece of work would print an empty block
          // the moment its agent came up.
          if (working && (event.status === "ready" || event.status === "ended")) {
            finishIfReady();
          }
        };

        watching.current.add(key);
        return subscribe(key, events).then(() => {
          live = true;
          // The turn this line is waiting for has not been asked for yet, so
          // whatever the replay said about a turn was about the last one.
          // Everything folded so far is the previous conversation: this is where
          // this turn's answer will begin.
          if (awaited) {
            working = false;
            sinceTurn = transcript.entries.length;
            return;
          }
          // Work already asked for, and already over by the time this side
          // began listening.
          if (ended === "failed") {
            settle(
              "failed",
              refusal(trouble(transcript) ?? "the agent stopped without saying why"),
            );
            return;
          }
          if (ended !== null && working) {
            finishIfReady();
          }
        });
      };

      /**
       * The session this name stands for, or nothing when it stands for none.
       *
       * The cache first and the host second, because a name can also belong to
       * work this window never started — another window's, or its own from
       * before a reload.
       */
      const resolve = async (): Promise<string | null> => {
        const lowered = name.trim().toLowerCase();
        const remembered = known.current.get(lowered);
        if (remembered !== undefined) return remembered;

        const starting = raising.current.get(lowered);
        if (starting !== undefined) return starting;

        const running = await works(project);
        for (const row of running) {
          if (sameName(row.title, name)) {
            known.current.set(lowered, row.key);
            return row.key;
          }
        }
        return null;
      };

      // Which session this line ended up watching, so that a failure after the
      // watch began can be ended through it rather than beside it.
      let watched: string | null = null;

      const begin = () => {
        change((work) => (work.queued ? { ...work, queued: false } : work));
        void resolve()
          .then(async (key) => {
            if (key !== null) {
              // Said into the work that already has this name — living or
              // finished. A conversation that has stopped takes another turn, and
              // that is the whole of what "ask it something more" means.
              //
              // `prompt` before `follow`: the prompt waits for the previous
              // turn to finish (the session's turn lock), so by the time
              // `follow` subscribes, `Working` has been emitted for this turn
              // and the backlog cannot carry a stale `ready` or
              // `agent_message_chunk` from the previous one.
              watched = key;
              await prompt(key, text, [], []);
              await follow(key, false);
              return;
            }
            const lowered = name.trim().toLowerCase();
            const started = startWork(project, name, text).then((row) => {
              known.current.set(lowered, row.key);
              raising.current.delete(lowered);
              return row;
            });
            raising.current.set(
              lowered,
              started.then((row) => row.key),
            );
            const row = await started.catch((error: unknown) => {
              raising.current.delete(lowered);
              throw error;
            });
            watched = row.key;
            await follow(row.key, false);
          })
          .catch((error: unknown) => {
            // Through the watch's own ending when there is one, because `done`
            // on its own delivers a block the watch can deliver again: the
            // subscription is still live, the next status settles it, and the
            // shelf loses its row twice while the count of held agents drifts
            // below what is actually running.
            const ending = stoppers.current.get(watched ?? "");
            if (ending !== undefined) {
              ending(`console: ${said(error)}`);
              return;
            }
            // Refused before anything was raised — no agent chosen, a session
            // that would not start. Said in the words the host used: it knows
            // which of those it was and this does not.
            done("failed", refusal(`console: ${said(error)}`));
          });
      };

      // Held below the ceiling by this one place. A line over it waits in the
      // order it was typed: somebody who asked for three things expects the
      // third to happen third.
      if (held.current < AT_ONCE) {
        held.current += 1;
        begin();
      } else {
        waiting.current.push(begin);
      }
    },
    [project],
  );

  /**
   * The work a verb is about, by the name it was given.
   *
   * Refused rather than guessed when the name resolves to nothing: a verb that
   * silently did nothing is a verb somebody types twice.
   */
  const find = useCallback(
    async (name: string | undefined): Promise<string | Outcome> => {
      if (name === undefined || name.trim().length === 0) {
        return { state: "failed", result: refusal("console: name a piece of work") };
      }
      const wanted = name.replace(/^@/u, "");
      const lowered = wanted.trim().toLowerCase();
      const remembered = known.current.get(lowered);
      if (remembered !== undefined) return remembered;

      const running = await works(project);
      const match = running.find((row) => sameName(row.title, wanted));
      if (match === undefined) {
        return {
          state: "failed",
          result: refusal(`console: no work is called @${wanted}`),
        };
      }
      known.current.set(lowered, match.key);
      return match.key;
    },
    [project],
  );

  /**
   * Carry out one of the window's own verbs.
   *
   * Every one of them is a command that already exists in the host, and this is
   * where the name somebody typed becomes the session key that command takes.
   * Nothing here decides policy: `stop` interrupts a turn and `close` ends a
   * process because those are two commands in `sessions.rs`, and joining them
   * into one verb would be this window deciding that somebody who was tired of
   * waiting also wanted the conversation gone.
   */
  const perform = useCallback(
    async (verb: string, args: readonly string[]): Promise<Outcome> => {
      if (verb === "works") {
        const rows = await works(project);
        if (rows.length === 0) {
          return { state: "done", result: refusal("no work in this project") };
        }
        return {
          state: "done",
          result: refusal(
            rows.map((row) => `@${row.title ?? row.agentName}  ${row.status}`).join("\n"),
          ),
        };
      }

      const found = await find(args[0]);
      if (typeof found !== "string") return found;
      const key = found;

      switch (verb) {
        case "stop":
          await cancel(key);
          return { state: "done", result: refusal("asked it to stop") };

        case "close": {
          await closeSession(key);
          // The block this work still owes is printed here rather than left to
          // a turn that is never going to end now.
          stoppers.current.get(key)?.("closed");
          for (const [name, held] of known.current) {
            if (held === key) known.current.delete(name);
          }
          return { state: "done", result: refusal("closed") };
        }

        case "allow":
        case "deny": {
          const question = asked.current.get(key);
          if (question === undefined) {
            return {
              state: "failed",
              result: refusal("console: it is not waiting on a question"),
            };
          }
          // What an agent calls its own options is the agent's business, and
          // the kinds it states them under are the protocol's. Read the kind
          // rather than the wording: `allow_once` and `allow_always` are both
          // yes, and a window matching on names would answer the wrong one the
          // first time an agent said `Yes, go ahead`.
          const wanted = verb === "allow" ? "allow" : "reject";
          const option =
            question.options.find((one) => one.kind.startsWith(wanted)) ??
            (verb === "deny" ? null : undefined);
          if (option === undefined) {
            return {
              state: "failed",
              result: refusal("console: it offered no such answer"),
            };
          }
          await respondToPermission(key, question.requestId, option?.optionId ?? null);
          asked.current.delete(key);
          return {
            state: "done",
            result: refusal(option === null ? "withdrawn" : option.name),
          };
        }

        default:
          return {
            state: "failed",
            result: refusal(`console: ${verb} could not be run`),
          };
      }
    },
    [find, project],
  );

  return { address, running, names, perform };
}
