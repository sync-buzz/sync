/**
 * Turning a session's event stream into something a person can read.
 *
 * # The problem this module exists to solve
 *
 * ACP has **no message boundary**. An agent emits a run of `agent_message_chunk`
 * notifications and the protocol never says where one message ends and the next
 * begins — there is no start, no end and no id. A client that simply appends
 * every chunk to one block gets exactly what the previous version of Sync got:
 * a turn's worth of text, a tool call, more text and a long pause all welded
 * into a single paragraph, unreadable and impossible to tell apart.
 *
 * So the boundary is ours to decide, and it is decided here rather than in Rust:
 * a rule in the transport would have to be right about agents it has never met,
 * and this one is a reading decision that can be changed without touching a
 * connection.
 *
 * # The rule
 *
 * A block closes when any of four things happens:
 *
 * 1. **A pause.** More than {@link PAUSE_MS} between one chunk and the next.
 *    Streaming chunks arrive milliseconds apart, so a gap this long is the agent
 *    having stopped rather than the stream stuttering — and it is the case that
 *    made the old transcript unreadable.
 * 2. **Something else happened in between.** A tool call, a plan, a mode change:
 *    the agent went and did something, and what it says afterwards is about
 *    something new.
 * 3. **The voice changed.** Thinking and answering are two different things and
 *    are never one block.
 * 4. **The turn ended.**
 *
 * The pause is measured with the time Rust stamped on the event when it arrived,
 * never with the window's own clock. A screen that re-subscribes is handed the
 * whole history at once, and by the window's clock every event in it would be
 * simultaneous — so a reopened conversation would collapse into one block
 * exactly where a live one read correctly.
 */

import type {
  PastedImage,
  PermissionRequest,
  SentImage,
  SessionEvent,
  SessionStatus,
} from "./client";

/**
 * How long a gap has to be before it ends a block.
 *
 * Generous on purpose. A model streaming token by token can stall for a moment
 * without having finished a thought, and a block broken mid-sentence is a worse
 * failure than two paragraphs left joined.
 */
export const PAUSE_MS = 1_500;

/** What the agent said, or did, as one readable block. */
export type Entry =
  /** Something a person typed, and whatever they attached to it. */
  | {
      readonly id: string;
      readonly at: number;
      readonly voice: "person";
      readonly text: string;
      /** The files sent with it, as absolute paths. Empty when there were none. */
      readonly attachments: readonly string[];
      /** The images pasted into it. Empty when there were none. */
      readonly images: readonly PastedImage[];
    }
  /** The agent's answer. */
  | {
      readonly id: string;
      readonly at: number;
      /**
       * When the last chunk of this block landed.
       *
       * Kept apart from `at`, which is when the block started, because the pause
       * rule is about the gap since the last thing said. Measuring from the
       * start instead means a block that has been streaming for longer than the
       * pause splits on every further chunk, however fast they arrive — an
       * answer that runs for a minute becomes a hundred paragraphs.
       */
      readonly lastAt: number;
      readonly voice: "agent";
      readonly text: string;
    }
  /** The agent thinking out loud, which not every agent sends. */
  | {
      readonly id: string;
      readonly at: number;
      readonly lastAt: number;
      readonly voice: "thought";
      readonly text: string;
    }
  /**
   * A picture the agent answered with.
   *
   * Its own block rather than something hung off the message beside it, and for
   * the same reason a tool call is: the agent went and made something, and the
   * text before it and the text after it are two different things to say. It
   * also has to fold identically live and on a replay — an agent sends the
   * picture in the same run of chunks either way — and a block that joined the
   * open message would put it in a different place depending on how fast the
   * chunk before it arrived.
   */
  | {
      readonly id: string;
      readonly at: number;
      readonly voice: "picture";
      /** What the session holds the bytes under, or `null` when it could not. */
      readonly imageId: string | null;
      readonly mimeType: string;
      readonly bytes: number;
    }
  /** A tool the agent ran. */
  | {
      readonly id: string;
      readonly at: number;
      readonly voice: "tool";
      readonly title: string;
      readonly status: string;
      readonly toolCallId: string;
    }
  /** A plan the agent stated, in the shape it stated it. */
  | {
      readonly id: string;
      readonly at: number;
      readonly voice: "plan";
      readonly steps: readonly { readonly title: string; readonly status: string }[];
    }
  /** An update this build has no reading for. Shown rather than dropped. */
  | {
      readonly id: string;
      readonly at: number;
      readonly voice: "unread";
      readonly update: string | null;
      readonly payload: unknown;
    }
  /** Something went wrong, in the words it went wrong in. */
  | { readonly id: string; readonly at: number; readonly voice: "trouble"; readonly text: string };

/** A question waiting to be answered. */
export interface OpenQuestion {
  readonly requestId: number;
  readonly toolName: string | null;
  readonly request: PermissionRequest;
  readonly at: number;
}

/** What a turn cost, as the agent counted it. Not every agent reports any. */
export interface Usage {
  readonly totalTokens?: number;
  readonly inputTokens?: number;
  readonly outputTokens?: number;
  readonly thoughtTokens?: number | null;
  readonly cachedReadTokens?: number | null;
  readonly cachedWriteTokens?: number | null;
}

/** Everything a screen needs to draw one conversation. */
export interface Transcript {
  readonly entries: readonly Entry[];
  readonly status: SessionStatus;
  /** Why, for the states where the word is not enough. */
  readonly detail: string | null;
  /** The question the agent is stopped on, or `null`. */
  readonly question: OpenQuestion | null;
  /** How many events fell off the front of the session's history. */
  readonly dropped: number;
  /** The last figures the agent reported, or `null` if it reports none. */
  readonly usage: Usage | null;
  /** The mode the agent says it is in, for the agents that have modes. */
  readonly mode: string | null;
  /** How the last turn ended, in the protocol's word for it. */
  readonly stopReason: string | null;
}

export const EMPTY_TRANSCRIPT: Transcript = {
  entries: [],
  status: "starting",
  detail: null,
  question: null,
  dropped: 0,
  usage: null,
  mode: null,
  stopReason: null,
};

/** Mutable working copy — `fold` returns a fresh {@link Transcript} from it. */
interface Draft {
  entries: Entry[];
  status: SessionStatus;
  detail: string | null;
  question: OpenQuestion | null;
  dropped: number;
  usage: Usage | null;
  mode: string | null;
  stopReason: string | null;
  /** The block still being written into, if the next chunk may join it. */
  openVoice: "agent" | "thought" | null;
  /** When the last chunk of the open block arrived. */
  openAt: number;
}

/**
 * Applies one event to a transcript.
 *
 * Pure, and takes the whole transcript rather than a reducer's worth of it, so
 * a screen can replay a session's history through it and get exactly what a
 * live stream would have produced.
 */
export function foldTranscript(transcript: Transcript, event: SessionEvent): Transcript {
  const draft: Draft = {
    entries: [...transcript.entries],
    status: transcript.status,
    detail: transcript.detail,
    question: transcript.question,
    dropped: transcript.dropped,
    openVoice: openVoiceOf(transcript),
    openAt: lastSpokenAt(transcript),
    usage: transcript.usage,
    mode: transcript.mode,
    stopReason: transcript.stopReason,
  };

  switch (event.kind) {
    case "status": {
      draft.status = event.status;
      draft.detail = event.detail;
      // A turn that came back carries its reason in the same field. It is not
      // trouble and is not shown as a block — it belongs beside the agent, with
      // the rest of what is true of the conversation.
      if (event.status === "ready" && event.detail !== null) {
        draft.stopReason = event.detail;
        draft.detail = null;
      }
      // A turn that ended closes whatever was being written: the next thing the
      // agent says belongs to the next turn, however soon it comes.
      if (event.status !== "working") draft.openVoice = null;
      if (event.status === "failed" && event.detail !== null) {
        push(draft, {
          id: `e${event.seq}`,
          at: event.atMs,
          voice: "trouble",
          text: event.detail,
        });
      }
      break;
    }
    case "prompt":
      // Always its own block, and always closes whatever the agent was writing:
      // a person interrupting is the clearest boundary there is.
      push(draft, {
        id: `e${event.seq}`,
        at: event.atMs,
        voice: "person",
        text: event.text,
        // A history recorded before these fields existed carries neither, and a
        // block that read `undefined` here would throw where it is drawn.
        attachments: event.attachments ?? [],
        images: event.images ?? [],
      });
      break;
    case "update":
      applyUpdate(draft, event);
      break;
    case "permission":
      draft.question = {
        requestId: event.requestId,
        toolName: event.toolName,
        request: event.request,
        at: event.atMs,
      };
      break;
    case "permissionSettled":
      if (draft.question?.requestId === event.requestId) draft.question = null;
      break;
    case "configuration":
      // The configuration is not part of the reading — it is what the model
      // picker is drawn from, and the session carries it separately.
      break;
    case "modes":
      // The *list* is carried separately, like the configuration above. What is
      // folded here is which mode is current, because that is the one field a
      // transcript already holds — and holding it twice, once from here and
      // once from `current_mode_update`, would be two answers to keep in step.
      // Two writers of one field in a sequenced stream is not that: the later
      // event is simply the later truth.
      draft.mode = event.modes.currentModeId;
      break;
  }

  return {
    entries: draft.entries,
    status: draft.status,
    detail: draft.detail,
    question: draft.question,
    dropped: draft.dropped,
    usage: draft.usage,
    mode: draft.mode,
    stopReason: draft.stopReason,
  };
}

/**
 * The figures worth showing, as label and value.
 *
 * Only what the agent actually reported: a zero it never sent is a claim about
 * spending that nobody made.
 */
export function usageLines(usage: Usage | null): readonly { label: string; value: number }[] {
  if (usage === null) return [];
  const rows: { label: string; value: number }[] = [];
  const add = (label: string, value: number | null | undefined) => {
    if (typeof value === "number") rows.push({ label, value });
  };
  add("Total", usage.totalTokens);
  add("In", usage.inputTokens);
  add("Out", usage.outputTokens);
  add("Thinking", usage.thoughtTokens);
  add("Cached", usage.cachedReadTokens);
  return rows;
}

/** Records how much of a session's history was already gone when we subscribed. */
export function withDropped(transcript: Transcript, dropped: number): Transcript {
  return dropped === transcript.dropped ? transcript : { ...transcript, dropped };
}

function applyUpdate(draft: Draft, event: Extract<SessionEvent, { kind: "update" }>): void {
  const payload = event.payload as Record<string, unknown>;
  const id = `e${event.seq}`;

  if (!event.recognized) {
    // Rust could not read it, so neither can this. It is shown rather than
    // dropped: an update nobody models is exactly the thing worth seeing.
    draft.openVoice = null;
    push(draft, { id, at: event.atMs, voice: "unread", update: event.update, payload });
    return;
  }

  switch (event.update) {
    // The person's own words, and only when the agent is replaying a session it
    // loaded. Live, this is the agent quoting the message back — Sync recorded
    // that as a `prompt` the moment it was sent, and folding the echo as well
    // would print every sentence somebody typed twice. On a replay there is no
    // `prompt` to have recorded: the conversation is being handed back by the
    // agent, and without this it comes back with the agent talking to nobody.
    //
    // It is its own block rather than an appended chunk. A person interrupting
    // is the clearest boundary there is, which is the same rule `prompt`
    // follows, and it has to read identically — a replayed conversation that
    // grouped its blocks differently from the live one would look like a
    // different conversation.
    case "user_message_chunk": {
      if (event.replayed !== true) return;
      const said = textOf(payload.content);
      if (said === "") return;
      draft.openVoice = null;
      push(draft, {
        id,
        at: event.atMs,
        voice: "person",
        text: said,
        // A replay carries neither. What was attached was a path the agent was
        // handed and a picture that only ever lived in the session that is
        // gone — neither survives in what the agent kept.
        attachments: [],
        images: [],
      });
      return;
    }
    case "agent_message_chunk":
      // A chunk carries one content block, and it is a picture or it is words.
      if (pictureOf(draft, payload.content, event.atMs, id)) return;
      appendChunk(draft, "agent", textOf(payload.content), event.atMs, id);
      return;
    case "agent_thought_chunk":
      if (pictureOf(draft, payload.content, event.atMs, id)) return;
      appendChunk(draft, "thought", textOf(payload.content), event.atMs, id);
      return;
    case "tool_call":
    case "tool_call_update": {
      // Rule 2: the agent went and did something, so the text before and the
      // text after are two different things to say.
      draft.openVoice = null;
      const toolCallId = String(payload.toolCallId ?? id);
      const title = typeof payload.title === "string" ? payload.title : toolCallId;
      const status = typeof payload.status === "string" ? payload.status : "pending";
      // An update to a call already listed rewrites it in place rather than
      // adding a second row for the same piece of work.
      const at = draft.entries.findIndex(
        (entry) => entry.voice === "tool" && entry.toolCallId === toolCallId,
      );
      const entry: Entry = {
        id: at === -1 ? id : draft.entries[at].id,
        at: event.atMs,
        voice: "tool",
        title:
          at === -1 || title !== toolCallId
            ? title
            : (draft.entries[at] as Extract<Entry, { voice: "tool" }>).title,
        status,
        toolCallId,
      };
      if (at === -1) draft.entries.push(entry);
      else draft.entries[at] = entry;
      return;
    }
    case "usage_update":
      // What the turn cost, as the agent counted it. Two of the five measured
      // agents send none at all, which is why this is `null` rather than zero:
      // "nothing reported" and "nothing spent" are not the same statement.
      draft.usage = (payload.usage ?? payload) as Usage;
      return;
    case "current_mode_update":
      draft.mode = typeof payload.currentModeId === "string" ? payload.currentModeId : draft.mode;
      return;
    case "plan":
    case "plan_update": {
      draft.openVoice = null;
      const raw = Array.isArray(payload.entries) ? payload.entries : [];
      const steps = raw.map((step) => {
        const item = step as Record<string, unknown>;
        return {
          title: String(item.content ?? item.title ?? ""),
          status: String(item.status ?? "pending"),
        };
      });
      // One plan per turn: an agent restates the whole plan every time a step
      // moves, so a second row would be the same plan again.
      const at = draft.entries.findIndex((entry) => entry.voice === "plan");
      const entry: Entry = { id: at === -1 ? id : draft.entries[at].id, at: event.atMs, voice: "plan", steps };
      if (at === -1) draft.entries.push(entry);
      else draft.entries[at] = entry;
      return;
    }
    default:
      // A variant this build has no reading for but Rust could parse — a mode
      // change, a usage report. Not shown: it is bookkeeping, not conversation.
      return;
  }
}

/**
 * Adds text to the open block, or starts a new one.
 *
 * This is where rules 1 and 3 are applied: a different voice always starts a
 * block, and so does a gap longer than {@link PAUSE_MS}.
 */
function appendChunk(
  draft: Draft,
  voice: "agent" | "thought",
  text: string,
  at: number,
  id: string,
): void {
  if (text === "") return;
  const last = draft.entries.at(-1);
  const joins =
    draft.openVoice === voice &&
    last !== undefined &&
    (last.voice === "agent" || last.voice === "thought") &&
    last.voice === voice &&
    at - draft.openAt <= PAUSE_MS;

  if (joins && (last.voice === "agent" || last.voice === "thought")) {
    draft.entries[draft.entries.length - 1] = {
      ...last,
      text: last.text + text,
      lastAt: at,
    };
  } else {
    draft.entries.push({ id, at, lastAt: at, voice, text });
  }
  draft.openVoice = voice;
  draft.openAt = at;
}

function push(draft: Draft, entry: Entry): void {
  draft.entries.push(entry);
  draft.openVoice = null;
}

/** Whether the last entry is still open to being written into. */
function openVoiceOf(transcript: Transcript): "agent" | "thought" | null {
  const last = transcript.entries.at(-1);
  if (last === undefined) return null;
  return last.voice === "agent" || last.voice === "thought" ? last.voice : null;
}

/** When the open block was last written into — the clock the pause is measured on. */
function lastSpokenAt(transcript: Transcript): number {
  const last = transcript.entries.at(-1);
  if (last === undefined) return 0;
  return last.voice === "agent" || last.voice === "thought" ? last.lastAt : last.at;
}

/**
 * Pushes a block for a picture, and says whether the content was one.
 *
 * The bytes are already gone by the time this runs — the host moved them into
 * the session and left the id in their place — so what is folded here is a
 * pointer and a size, and the picture is fetched when something draws it.
 */
function pictureOf(draft: Draft, content: unknown, at: number, id: string): boolean {
  if (typeof content !== "object" || content === null) return false;
  const block = content as Record<string, unknown>;
  if (block.type !== "image") return false;
  const sent = block as unknown as SentImage;
  push(draft, {
    id,
    at,
    voice: "picture",
    imageId: typeof sent.imageId === "string" ? sent.imageId : null,
    mimeType: typeof sent.mimeType === "string" ? sent.mimeType : "image/png",
    bytes: typeof sent.bytes === "number" ? sent.bytes : 0,
  });
  return true;
}

/** The text of an ACP content block, for the kinds that carry any. */
function textOf(content: unknown): string {
  if (typeof content !== "object" || content === null) return "";
  const block = content as Record<string, unknown>;
  return typeof block.text === "string" ? block.text : "";
}

/**
 * The one option in a session's configuration that is the model, if it has one.
 *
 * By category rather than by name: the category is the protocol's own word for
 * it, and the names differ — one agent calls it "Model" and the next may not.
 */
export function modelOption<T extends { category?: string | null; type?: string }>(
  options: readonly T[] | null | undefined,
): T | null {
  return options?.find((option) => option.category === "model" && option.type === "select") ?? null;
}
