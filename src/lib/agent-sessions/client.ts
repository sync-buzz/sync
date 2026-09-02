/**
 * The window's only route to a running agent.
 *
 * Every function here is one `invoke` into the Rust command layer, which owns
 * the process, the protocol and the transcript. The window holds no connection
 * and no retry policy: a session outlives the screen that opened it, so a screen
 * that owned the connection would end the conversation by being navigated away
 * from.
 *
 * The word *agent* means something narrower here than it does in settings.
 * There, an agent is a client that connects **to** Sync and we write our server
 * into its configuration. Here it is a process Sync **drives** over ACP — which
 * is why Claude Desktop, Cursor, VS Code and Zed are not in this list: they are
 * applications, not processes with a protocol on their standard input.
 */

import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { Worktree, WorktreeChoice } from "@/lib/worktrees/client";

/** One agent, and whether this machine can raise it. */
export interface AgentDescriptor {
  readonly id: string;
  readonly name: string;
  /** Whether the executable was found. A missing agent is still listed. */
  readonly available: boolean;
  /** What is missing, when it is not available. */
  readonly unavailableReason: string | null;
  /** Whether a full turn was ever run against it for real. */
  readonly verified: boolean;
  readonly unverifiedReason: string | null;
  /**
   * How it is reached: natively, through a third-party adapter, or through a
   * bridge of ours. Worth showing because it explains a slow first launch — an
   * adapter is fetched before a single frame is written.
   */
  readonly transport: "native" | "adapter" | "bridge" | "unknown";
  /**
   * Whether a model can be chosen when it is raised. The other way — the agent
   * listing its models in protocol — is not knowable until a session exists.
   */
  readonly takesModelAtLaunch: boolean;
}

/** Where a session is. */
export type SessionStatus =
  /** The process is being raised and the session opened. */
  | "starting"
  /** Open, and not in a turn. */
  | "ready"
  /** A turn is running. */
  | "working"
  /**
   * A turn has been said into it and is waiting its turn to run. What a
   * conversation delegated from one that already has a delegated run under it
   * says until that one is finished.
   */
  | "queued"
  /** Stopped on a question only a person can answer. */
  | "asking"
  /** Ended by itself, or its process died. */
  | "ended"
  /** Could not be raised, or fell over. */
  | "failed";

/** A running session, as the window lists it. */
export interface SessionRow {
  readonly key: string;
  readonly agentId: string;
  readonly agentName: string;
  /**
   * Whether the agent said at `initialize` that it reads images. Measured:
   * Claude, Codex, OpenCode and Gemini do; Grok does not.
   */
  readonly acceptsImages: boolean;
  /**
   * What the conversation is called: the first words said in it, or whatever
   * somebody renamed it to. `null` before anything has been said, which is when
   * the agent's name is the only thing there is to call it.
   */
  readonly title: string | null;
  /**
   * The project this conversation belongs to.
   *
   * Beside `cwd` rather than instead of it, because they answer two questions:
   * this is whose conversation it is, and `cwd` is where the agent is working.
   * They differ exactly when the work is being done in a disposable tree, so a
   * screen picking out its own conversations matches on this — matching on
   * `cwd` loses every conversation in a tree the moment it is made.
   */
  readonly project: string;
  readonly cwd: string;
  readonly status: SessionStatus;
  readonly openedAtMs: number;
  /**
   * Who asked for this conversation, when it was not a person.
   *
   * `undefined` is a person, and it is the ordinary answer — the field is
   * absent rather than null on a conversation somebody started by typing.
   *
   * This is what makes a list of conversations honest. A session an extension
   * ordered is an ordinary session in every other respect: it is in this list,
   * it can be watched and stopped, and its title is derived from the first
   * words said in it — which were written by a handler. Without this a person
   * waking to find three conversations they did not start has nothing to tell
   * them apart from their own.
   *
   * It is also how an extension finds the sessions it ordered itself: match
   * `source.extensionId` against its own id. There is no second call for that,
   * deliberately — a list already answers it.
   */
  readonly source?: SessionSource;
  /**
   * The working tree this conversation is being held in, when it is not being
   * held in the project's own.
   *
   * `undefined` is the ordinary answer. When it is there, it is what both
   * gestures a tree offers are addressed by — keeping the work under a name,
   * and throwing it away — and it is why the row carries the tree rather than
   * only the directory: `cwd` says where the agent is working, this says that
   * the place is disposable and where it came from.
   */
  readonly worktree?: Worktree;
  /**
   * The record this conversation is being held under, when there is one.
   *
   * Beside `source` rather than inside it, because *who asked* and *what it is
   * about* are two questions and only the first of them has a person as an
   * ordinary answer. A conversation somebody opened from a task has no orderer
   * and is still about that task, so a list that grouped by who asked would
   * leave every one of those in the same undifferentiated heap.
   *
   * Set when the session is opened and never edited, which is what lets a list
   * group by it: a row that changed group while somebody was reading it was the
   * mistake a `Running`/`Not running` split already made once.
   */
  readonly about?: SessionAbout;
  /**
   * The agent's own id for this session, once the agent has given one.
   *
   * What a row is named by when another row names it. A pointer has always
   * been addressed this way and a live row by this run's key, and the two were
   * never comparable — which stopped being good enough when a conversation
   * began naming the one it came out of.
   */
  readonly acpSession?: string;
  /**
   * The conversation this one was delegated from, by that conversation's own
   * agent id.
   *
   * Read against {@link SessionRow.acpSession} of the other conversations,
   * whichever half of the list they came from. A parent nothing in the list
   * names is drawn as no parent at all: pointers prune, and a child may outlive
   * the row above it.
   */
  readonly parent?: string;
}

/**
 * The record a conversation is being held under.
 *
 * Three members and each is load-bearing: the key is what a list groups by, the
 * kind is what opening the record takes beside it — an area lists records by
 * type and cannot find out which of its own lists a key belongs in without
 * reading the record first — and the title is what a heading says.
 *
 * The title is what the record was called when the work began, so a heading is
 * drawn without reading the corpus for every row of a list that is polled every
 * few seconds. It goes stale the way `extensionName` does, and in the same
 * direction: a record renamed later is called what it was called here until
 * something is opened about it again.
 */
export interface SessionAbout {
  readonly key: string;
  readonly kind: string;
  readonly title: string;
}

/**
 * Who ordered a session, when it was not a person at the keyboard.
 *
 * Set when the work was ordered and never edited afterwards, which is what lets
 * it be believed: it says what asked for this, not what somebody later decided
 * it was about.
 */
export interface SessionSource {
  /**
   * The order this conversation came out of, as `work.order` answered it.
   *
   * Who asked is not enough on its own: one handler may ask three times, and
   * three rows carrying the same extension and handler are three rows nothing
   * can tell apart. This is the token the host handed the package when it
   * ordered, so a package can say *this* conversation is task 42 rather than
   * *one of these three is*.
   */
  readonly work: string;
  /**
   * The extension whose handler ordered it, by its manifest id.
   *
   * What a package matches against to find its own work, and what a list groups
   * by. Paired with `extensionName` the way `agentId` is paired with
   * `agentName`: an id is what things are equal by, a name is what a heading
   * says.
   */
  readonly extensionId: string;
  /**
   * What that extension is called, so a heading can be drawn without asking
   * anything else what it is called.
   *
   * What it was called when the work was ordered. A package that renames itself
   * later does not rewrite what it already asked for.
   */
  readonly extensionName: string;
  /** The handler that ordered it, by the name an occasion calls. */
  readonly handler: string;
  /** What it was about, as a record key, when the orderer named one. */
  readonly about?: string;
}

/** What opening a session answered with. */
export interface OpenedSession {
  readonly key: string;
  readonly agentName: string;
  readonly agentVersion: string | null;
  readonly configuration: readonly SessionConfigOption[] | null;
  /** The modes it works in, or `null` from an agent that has none. */
  readonly modes: SessionModeState | null;
}

/**
 * One thing a session's configuration lets a person choose.
 *
 * This is the protocol's own shape, not ours. A model is one of these with
 * `category: "model"`, and it is the same mechanism on every agent that offers
 * the choice at all — which is the reason the window does not carry a table of
 * which model belongs to whom.
 */
export interface SessionConfigOption {
  readonly id: string;
  readonly name: string;
  readonly description?: string | null;
  readonly category?: string | null;
  readonly type?: string;
  readonly currentValue?: string;
  readonly options?:
    | readonly SessionConfigValue[]
    | readonly { readonly name: string; readonly options: readonly SessionConfigValue[] }[];
}

export interface SessionConfigValue {
  readonly value: string;
  readonly name: string;
  readonly description?: string | null;
}

/**
 * One way an agent can be asked to behave — Claude Code's Plan, Accept Edits
 * and Default among them.
 *
 * The protocol's own shape, like the configuration above it, and separate from
 * it for the same reason the two are separate in the protocol: an agent may
 * state either without the other. A mode is not a model and not a setting; it
 * is how much the agent may do without asking, which is why it is the choice a
 * person changes most often and the one that belongs nearest to what they are
 * typing.
 */
export interface SessionMode {
  readonly id: string;
  readonly name: string;
  readonly description?: string | null;
}

/** The modes an agent offers, and the one it is in. */
export interface SessionModeState {
  readonly currentModeId: string;
  readonly availableModes: readonly SessionMode[];
}

/** One thing that happened in a session. */
export type SessionEvent =
  | {
      readonly kind: "status";
      readonly seq: number;
      readonly atMs: number;
      readonly status: SessionStatus;
      readonly detail: string | null;
    }
  | {
      /**
       * What a person said. Recorded by the host rather than by the protocol:
       * ACP has no notification for what the client sent, so nothing would ever
       * arrive to carry it, and keeping it in the screen's own state loses it
       * the moment the screen is unmounted.
       */
      readonly kind: "prompt";
      readonly seq: number;
      readonly atMs: number;
      readonly text: string;
      /** The files sent with it, as absolute paths. */
      readonly attachments: readonly string[];
      /** The images pasted into it, by the id the session holds them under. */
      readonly images: readonly PastedImage[];
    }
  | {
      readonly kind: "update";
      readonly seq: number;
      readonly atMs: number;
      /** The `sessionUpdate` discriminator, when the payload carried one. */
      readonly update: string | null;
      /** Whether Rust's compiled protocol types could read it. */
      readonly recognized: boolean;
      readonly payload: Record<string, unknown>;
      /**
       * Whether this arrived while the agent was replaying a loaded session
       * rather than saying something new.
       *
       * It decides one thing, and it is the thing that puts a person into a
       * resumed conversation: `user_message_chunk` is the agent quoting what
       * somebody typed, which during a live turn is the same sentence Sync
       * already recorded when it was sent. Folded on a replay, ignored
       * otherwise.
       *
       * Absent on a history recorded before this existed, which reads as
       * `false` — the honest answer for events that were live when they
       * happened.
       */
      readonly replayed?: boolean;
    }
  | {
      readonly kind: "permission";
      readonly seq: number;
      readonly atMs: number;
      readonly requestId: number;
      readonly toolName: string | null;
      readonly request: PermissionRequest;
    }
  | {
      readonly kind: "permissionSettled";
      readonly seq: number;
      readonly atMs: number;
      readonly requestId: number;
      readonly chosen: string | null;
    }
  | {
      readonly kind: "configuration";
      readonly seq: number;
      readonly atMs: number;
      readonly options: readonly SessionConfigOption[];
    }
  | {
      readonly kind: "modes";
      readonly seq: number;
      readonly atMs: number;
      readonly modes: SessionModeState;
    };

/**
 * An image pasted into a conversation, as the window is told about it.
 *
 * The bytes are not here. They are in the session, under `id`, for as long as
 * the conversation lives — nothing is written to disk, and nothing survives the
 * application closing. {@link sessionImage} is how they are fetched to draw.
 */
export interface PastedImage {
  readonly id: string;
  readonly name: string;
  readonly mimeType: string;
  readonly bytes: number;
}

/**
 * A picture the agent sent, as it survives in the transcript.
 *
 * What is left where the base64 was. The host takes an image block's bytes out
 * of the update before it records it — a session's history is replayed whole to
 * every screen that comes back to the conversation, and a picture left in it
 * would be paid for on every one of them — and puts them in the session under
 * `imageId`, which {@link sessionImage} fetches by.
 *
 * `imageId` is `null` when the conversation could not keep it: there is one
 * ceiling on what a conversation holds in pictures, and what was pasted into it
 * counts against the same one. The block stays either way, because a turn that
 * lost its picture entirely is a turn in which the agent answered with nothing.
 */
export interface SentImage {
  readonly imageId: string | null;
  readonly mimeType: string;
  /** How many bytes it was, whether or not it is held. */
  readonly bytes: number;
}

/** An image on its way into a prompt. `data` is base64, with no `data:` prefix. */
export interface PastedContent {
  readonly name: string;
  readonly mimeType: string;
  readonly data: string;
}

/**
 * A question the agent is waiting on, in the agent's own words.
 *
 * `options` are passed through in the order and with the kinds the agent sent
 * them. They are not normalised and must not be: one measured agent offers no
 * "allow always" at all and another puts "reject once" first, and a window that
 * tidied those would be offering buttons the agent will not accept.
 */
export interface PermissionRequest {
  readonly options: readonly {
    readonly optionId: string;
    readonly name: string;
    readonly kind: string;
  }[];
  readonly toolCall?: {
    readonly title?: string;
    readonly kind?: string;
    readonly locations?: readonly { readonly path: string }[];
    readonly rawInput?: unknown;
    readonly _meta?: { readonly message?: string };
  };
}

/** A failure from the command layer, with the kind Rust gave it. */
export class SessionError extends Error {
  readonly kind: string;

  constructor(kind: string, message: string) {
    super(message);
    this.name = "SessionError";
    this.kind = kind;
  }
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (typeof error === "object" && error !== null && "kind" in error && "message" in error) {
      const failure = error as { kind: string; message: string };
      throw new SessionError(failure.kind, failure.message);
    }
    throw error;
  }
}

/** One agent's adapter package, and whether it is ready to run without a fetch. */
export interface AdapterState {
  readonly agentId: string;
  readonly package: string;
  readonly version: string;
  readonly ready: boolean;
}

/** What each adapter is, and whether it has been downloaded. */
export function agentAdapters(): Promise<AdapterState[]> {
  return call<AdapterState[]>("agent_adapters");
}

/**
 * Downloads what the agents need, at the versions this build pins.
 *
 * Called when the extension is installed, so that the first conversation is not
 * what pays for it. Failing is not fatal and must not block the install: a
 * machine that was offline simply pays for it at the first launch instead,
 * which is what every launch cost before this existed.
 */
export function prepareAdapters(): Promise<void> {
  return call<void>("agent_adapters_prepare");
}

/** Deletes what was downloaded. Called when the extension is removed. */
export function forgetAdapters(): Promise<void> {
  return call<void>("agent_adapters_forget");
}

/** Every agent, and whether this machine can raise it. */
export function agentCatalog(): Promise<AgentDescriptor[]> {
  return call<AgentDescriptor[]>("session_catalog");
}

/** Everything running right now, across every extension. */
export function liveSessions(): Promise<SessionRow[]> {
  return call<SessionRow[]>("session_live");
}

/**
 * Raises an agent and opens a session in it.
 *
 * `model` is only used by the agents that take one when they are raised; the
 * rest advertise theirs in the session's configuration, where
 * {@link chooseOption} is how one is picked.
 */
export function openSession(args: {
  agentId: string;
  cwd: string;
  model?: string | null;
  /**
   * The record this conversation is being opened under, for a screen that
   * opened it from one. Only the caller can answer it: somebody pressing
   * `Send to agent` is standing in a record, and nothing below this line can
   * find that out afterwards.
   */
  about?: SessionAbout | null;
  /**
   * Where to work: the project itself when this is absent, a tree made now, or
   * one that is already there.
   *
   * Answered when the conversation is opened and never afterwards. The
   * directory goes to the agent in `session/new` and it reads files from there,
   * so a caller that could move it later would be moving the ground under an
   * agent that has already answered about what it found.
   */
  worktree?: WorktreeChoice | null;
  /**
   * The conversation this one is being delegated from, by the agent's own id
   * for it — `acpSession` on the row or the pointer.
   *
   * What the work is *about* and who ordered it are not passed beside this:
   * both are read from the parent, so a delegated conversation is filed where
   * its parent is and no caller can file one anywhere else.
   */
  parent?: string | null;
}): Promise<OpenedSession> {
  return call<OpenedSession>("session_open", {
    agentId: args.agentId,
    cwd: args.cwd,
    model: args.model ?? null,
    worktree: args.worktree ?? null,
    // One value, because they are one answer: where this conversation belongs.
    // Rust takes them together and reads the record off the parent when both
    // are given, so a delegated conversation is filed where its parent is.
    under: { about: args.about ?? null, parent: args.parent ?? null },
  });
}

/**
 * Watches a session: everything recorded so far, then everything after.
 *
 * Resolves with how many events had already fallen off the front of the
 * session's history, which is never silently zero — a transcript that begins in
 * the middle has to say so rather than read as the whole conversation.
 */
export function subscribe(key: string, events: Channel<SessionEvent>): Promise<number> {
  return call<number>("session_subscribe", { key, events });
}

/**
 * One conversation this machine can ask an agent to hand back.
 *
 * A pointer, not a transcript: which agent, in which directory, and the agent's
 * own id for the session. The words are still with the agent and come back
 * through `session/load`.
 */
export interface RememberedConversation {
  /** The agent's own id for the session. Stable across runs; the live key is not. */
  readonly acpSession: string;
  readonly agentId: string;
  readonly agentName: string;
  readonly cwd: string;
  /**
   * The working tree it was held in, when it was held in one.
   *
   * Kept with the pointer because resuming has to land in the same files, and a
   * tree is the one part of a conversation somebody can delete from underneath
   * it: a pointer naming a tree that is gone is refused rather than quietly
   * resumed in the project.
   */
  readonly worktree?: Worktree;
  readonly title: string | null;
  readonly openedAtMs: number;
  readonly lastSeenMs: number;
  /**
   * Who asked for this conversation, when it was not a person.
   *
   * The same shape a live row carries, so a list built from both can group and
   * label them without caring which half a conversation came from. It matters
   * most here, in fact: a conversation ordered overnight has *finished* by the
   * time somebody looks, so the row they see in the morning is this one and not
   * the live one.
   */
  readonly source?: SessionSource;
  /**
   * The record the conversation was held under, when it was held under one.
   *
   * Carried here for the reason `source` is: a dormant row and a live one are
   * the same conversation at two moments, and one that lost its heading when
   * its agent stopped would move up the list under somebody reading it.
   */
  readonly about?: SessionAbout;
  /**
   * The conversation this one was delegated from, by that conversation's own
   * agent id. Written down rather than held in memory, so a tree does not
   * flatten when the application is restarted.
   */
  readonly parent?: string;
  /** The record it was kept as, when somebody kept it on this machine. */
  readonly recordKey?: string;
}

/**
 * The conversations this machine can continue, for one project.
 *
 * Not what is running — what is *resumable*. An entry whose `acpSession` no
 * live row carries is a conversation from a previous run of the application.
 */
export function rememberedConversations(
  project: string,
): Promise<RememberedConversation[]> {
  return call<RememberedConversation[]>("session_remembered", { project });
}

/**
 * Stops offering a conversation. What the agent holds is untouched.
 *
 * A pointer outlives the thing it points at: an agent prunes its own sessions,
 * and one it has dropped will not come back however often it is asked. Without
 * this such a row could be neither continued nor removed.
 */
export function forgetRememberedConversation(
  project: string,
  acpSession: string,
): Promise<void> {
  return call<void>("session_forget_remembered", { project, acpSession });
}

/**
 * Continues a conversation: raises its agent and asks for the session back.
 *
 * The agent replays what was said, so the session this opens arrives with its
 * transcript already in it. The key is new — this run has never seen the
 * conversation — while the conversation and its pointer are the same ones.
 *
 * Rejects with `agent_session_load` when the agent no longer holds the session.
 * That one cannot be known before asking, and it is the caller's cue to
 * continue from a kept transcript instead of from the agent.
 */
export function resumeSession(
  project: string,
  acpSession: string,
): Promise<OpenedSession> {
  return call<OpenedSession>("session_resume", { project, acpSession });
}

/**
 * Says which record a conversation was kept as, so the record can be continued
 * on this machine later.
 *
 * Answers whether there was a pointer to say it of. `false` is not a failure: a
 * conversation kept in the same run it was opened in may have none yet.
 */
export function conversationKeptAs(
  key: string,
  recordKey: string,
): Promise<boolean> {
  return call<boolean>("session_kept_as", { key, recordKey });
}

/**
 * The pointer for a kept record, when this machine holds one.
 *
 * `null` is the ordinary answer rather than a failure, and it is the whole of
 * the "is this conversation mine?" test: a record written by somebody else, or
 * on another machine, has no pointer here. What the window offers then is
 * continuing from the transcript in the record.
 */
export function conversationForRecord(
  project: string,
  recordKey: string,
): Promise<RememberedConversation | null> {
  return call<RememberedConversation | null>("session_for_record", {
    project,
    recordKey,
  });
}

/**
 * Everything a session has said so far, read once and not watched.
 *
 * The window subscribes to the conversation it has open and to no other, so a
 * command that acts on some *other* row — keeping it, above all — has no
 * transcript of it to write. This is where that transcript comes from, and it
 * carries the same `dropped` count a subscription reports.
 */
export function sessionBacklog(
  key: string,
): Promise<{ events: SessionEvent[]; dropped: number }> {
  return call<{ events: SessionEvent[]; dropped: number }>("session_backlog", { key });
}

/** Stops watching. The session goes on running. */
export function unsubscribe(key: string): Promise<void> {
  return call<void>("session_unsubscribe", { key });
}

/**
 * Runs one turn.
 *
 * Returns when the prompt is on its way, not when the turn ends: a turn may
 * take tens of minutes, and everything it produces arrives on the subscription.
 *
 * `attachments` are absolute paths. They are sent as resource links and read by
 * the agent itself — this window never opens them, which is why attaching one
 * needs no filesystem permission it does not already have.
 */
export function prompt(
  key: string,
  text: string,
  attachments: readonly string[],
  images: readonly PastedContent[],
): Promise<void> {
  return call<void>("session_prompt", { key, text, attachments, images });
}

/**
 * One pasted image, for something that is about to draw it.
 *
 * Fetched when it is drawn rather than carried on the subscription: a history
 * is replayed whole to every screen that returns to a conversation, and a
 * picture in it would be paid for on every one of them.
 */
export function sessionImage(
  key: string,
  id: string,
): Promise<{ readonly mimeType: string; readonly data: string }> {
  return call("session_image", { key, id });
}

/**
 * Save one of a conversation's pictures to a file, with the system's panel.
 *
 * It exists because the webview's own image menu does not work here and cannot
 * be made to. `Save Image` and `Open Image in New Window` are drawn by WebKit
 * on any `img`, and both are dead in this window: the source is a `data:` URL,
 * saving one needs a download handler the shell does not install, and opening
 * one is a navigation the content security policy refuses. Two menu items that
 * look like the system offering something and then do nothing are worse than no
 * menu at all, so the picture is given a menu of ours — native, like every
 * other context menu here — and this is what its one command calls.
 *
 * The bytes never come back through the window. It has them as base64 to draw
 * with, and writing a file from that would mean decoding what was encoded for a
 * different purpose; the panel answers with a path and Rust writes the bytes it
 * already holds.
 *
 * Answers whether a file was written: `false` is the person having dismissed
 * the panel, which is not a failure and is not reported as one.
 */
export async function saveSessionImage(
  key: string,
  id: string,
  suggestedName: string,
): Promise<boolean> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({ defaultPath: suggestedName });
  if (path === null) return false;
  await call<void>("session_image_save", { key, id, path });
  return true;
}

/**
 * A file name for a picture that has none, from what it is.
 *
 * A conversation's pictures are not files and have no names — the one the agent
 * sent never had one, and every browser invents the same name for a pasted one.
 * So the panel is given something to start from rather than an empty field, and
 * the extension is taken from the media type because saving a PNG as `.jpg` is
 * a file that will not open where it lands.
 */
export function imageFileName(mimeType: string, called?: string): string {
  const extension = mimeType.startsWith("image/")
    ? mimeType.slice("image/".length).split("+")[0].toLowerCase()
    : "png";
  const stem = called?.replace(/\.[^.]+$/, "").trim();
  return `${stem !== undefined && stem !== "" ? stem : "Picture"}.${extension === "jpeg" ? "jpg" : extension}`;
}

/**
 * The kinds of image a person attaches on purpose.
 *
 * The panel's first filter rather than its only one: what an agent can be given
 * is any file it can read, and a person who wants to hand it a log has not made
 * a mistake. Images lead because they are the thing that cannot be pasted into
 * the field as text, which is what makes attaching them the point of this at
 * all.
 */
const IMAGE_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "heic",
  "svg",
  "bmp",
  "tiff",
  "tif",
];

/**
 * Ask for files to attach with the system's open panel.
 *
 * What comes back is absolute and stays absolute. A path in a record is made
 * relative to the repository because the record travels with it; this one is
 * handed to another process on this machine, which resolves it against its own
 * directory — so relative here would name a different file, or none.
 *
 * The panel opens at the project because that is where a person is working, and
 * it is not confined to it: an agent may perfectly well be asked about a
 * screenshot on the desktop.
 */
export async function chooseAttachments(
  defaultPath: string,
): Promise<readonly string[]> {
  const chosen = await open({
    directory: false,
    multiple: true,
    defaultPath,
    title: "Attach Files",
    filters: [
      { name: "Images", extensions: IMAGE_EXTENSIONS },
      { name: "All Files", extensions: ["*"] },
    ],
  });
  if (chosen === null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

/**
 * Renames a conversation. An empty name clears the one there is, and the next
 * thing said derives another.
 */
export function renameSession(key: string, title: string): Promise<void> {
  return call<void>("session_rename", { key, title });
}

/** Asks the agent to stop the turn it is running. */
export function cancel(key: string): Promise<void> {
  return call<void>("session_cancel", { key });
}

/** Answers a question. `null` withdraws it, which the agent hears as a cancel. */
export function respondToPermission(
  key: string,
  requestId: number,
  optionId: string | null,
): Promise<void> {
  return call<void>("session_permission_respond", { key, requestId, optionId });
}

/**
 * Puts the session into one of the modes the agent advertised.
 *
 * Answers with the whole mode state rather than an acknowledgement, for the
 * same reason {@link chooseOption} does: the control is drawn from what came
 * back, so nothing has to guess whether the change took.
 */
export function chooseMode(key: string, modeId: string): Promise<SessionModeState> {
  return call<SessionModeState>("session_set_mode", { key, modeId });
}

/** Chooses one of the session's configuration options — a model among them. */
export function chooseOption(
  key: string,
  configId: string,
  valueId: string,
): Promise<SessionConfigOption[]> {
  return call<SessionConfigOption[]>("session_set_option", { key, configId, valueId });
}

/**
 * Stops a session's agent, and keeps the conversation.
 *
 * Two commands rather than one, because they are two intentions: the process is
 * spending money and ending it is urgent, while what it said may still be being
 * read. Taking the transcript away as a side effect of stopping the process
 * would be the application deciding a person was finished with it.
 */
export function closeSession(key: string): Promise<void> {
  return call<void>("session_close", { key });
}

/** Deletes a conversation, stopping its agent first if it is still running. */
export function forgetSession(key: string): Promise<void> {
  return call<void>("session_forget", { key });
}

export { Channel };
