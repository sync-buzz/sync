import type { ComponentType, ReactNode } from "react";

import type { AreaIntent } from "@/lib/area-intent";
import type { OpenProject } from "@/lib/project/types";

/**
 * What an extension implements, and the whole of what the host calls.
 *
 * Separate from the rest of the surface because it points the other way.
 * Everything else in `extension-api` is something the window hands over; this
 * is the shape of what comes back, and an author writes it rather than calls
 * it. Keeping it in a module of its own is also what keeps the surface
 * acyclic — the loader needs these types and the loader is not part of what an
 * extension may import.
 *
 * ```ts
 * export default function activate({ id }: ExtensionHost): ActivationResult {
 *   return { memory: { Provider, Navigator, Workspace, Inspector } }
 * }
 * ```
 */

/**
 * What a request asks the other end to do.
 *
 * The protocol's own spelling, because that is what an author is reading in
 * somebody else's API documentation while they write this. A union rather than
 * a string: a verb this door cannot honour is a mistake the editor catches, and
 * the same list is checked again in Rust for a caller the editor never saw.
 */
export type NetMethod = "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE";

/**
 * One request, as the package states it.
 *
 * `fetch`'s vocabulary — `method`, `headers`, `body` — narrowed to what crosses
 * a process boundary, and stated as one object rather than a URL and an init.
 * That is the shape that actually crosses: the same members are read in Rust,
 * so there is one spelling of a request rather than one per surface, and
 * `method` sits beside `url` where a reader looks for it.
 *
 * **A member that is not here is refused rather than ignored.** An author will
 * reach for the parts of `fetch` this does not have — `signal`, `credentials`,
 * `redirect`, `mode` — and a member quietly dropped is a timeout somebody
 * believes they set. What comes back instead is a sentence naming it.
 */
export interface NetRequest {
  readonly url: string;
  /** `GET` when it is not said, as `fetch` reads it. */
  readonly method?: NetMethod;
  /**
   * Header names and values, as the package writes them.
   *
   * The four the transport writes for itself — `host`, `content-length`,
   * `connection`, `transfer-encoding` — are refused: a request that set its own
   * would disagree with itself, and the server would answer about something
   * else entirely.
   */
  readonly headers?: Readonly<Record<string, string>>;
  /** What is sent, for a method that carries one. Text, and at most 2 MB. */
  readonly body?: string;
}

/**
 * What came back from a host an extension declared, as it reads it.
 *
 * `fetch`'s vocabulary again, and every member is one a package cannot work
 * without. `status` and `ok` are the same fact at two widths — almost every
 * caller wants the second, and the one that does not needs the first exactly.
 * `headers` are where pagination and rate limits live, and a package polling
 * somebody else's tracker without `link` or `retry-after` either stops at the
 * first page or gets itself blocked.
 *
 * No `statusText`: HTTP/2 carries no reason phrase, so it would be a member
 * that is sometimes there — which is worse than one that never is.
 *
 * The body is text. What it means is the package's business — every API it
 * could have been installed to read has its own shape, and a host that parsed
 * one would be the window having an opinion about somebody else's JSON.
 */
export interface NetResponse {
  /**
   * Where the response came from, after any redirect.
   *
   * Not the URL that was asked for. A package that followed a redirect and then
   * builds its next request from the address it started at will keep being
   * redirected.
   */
  readonly url: string;
  readonly status: number;
  /** Whether the status is a successful one, as `fetch` derives it. */
  readonly ok: boolean;
  /** Names in lower case; a name the server repeated is joined with `, `. */
  readonly headers: Readonly<Record<string, string>>;
  readonly body: string;
}

/**
 * The one door out of this window, for the one package that declared it.
 *
 * Where it may reach is `net.hosts` in the package's own manifest, and the
 * check is Rust's: the window's `connect-src` does not name the outside at all,
 * so this is not a restraint on a package that could otherwise fetch — there is
 * no reach here to take away. Every redirect is checked again, so a hop off the
 * declared list is refused as firmly as the first request.
 *
 * **Reading and changing something are two agreements.** `net` is the first and
 * `net.write` is the second, and a package that asked only for the first is
 * refused — in words, at the call — the moment it uses a method that is not
 * `GET` or `HEAD`. The division is the protocol's: those two are defined as
 * safe, and a verb that is merely usually harmless is not something a person
 * can be asked to agree to.
 *
 * **Nothing is retried.** A request that timed out may have been performed, and
 * whether to send it again is a question only the package can answer.
 */
export interface ExtensionNet {
  /**
   * Makes one request, or rejects saying why it did not.
   *
   * A `404` is not a rejection: it is an answer to the question the package
   * asked, and it comes back as a status for the package to explain. What
   * rejects is a request that never happened — a host the manifest does not
   * name, a scheme that is not `https`, a verb the package did not ask to be
   * allowed, a response too large to read, or no network at all.
   */
  fetch(request: NetRequest): Promise<NetResponse>;
}

/**
 * The secrets this package keeps, and nobody else's.
 *
 * One namespace, and it is the package's own: the owner half of every entry is
 * the id resolved against what is installed on this machine, and a call says
 * only what it calls its own secret. A name that reads like a way out of the
 * namespace — a path, another package's id — is a name, and the entry it
 * addresses is still this package's.
 *
 * It reads and writes, because the flow that needs one needs the other: a
 * package that signs somebody in ends up holding a token nobody could have
 * typed, and the same package replaces it when it expires. It forgets too, so
 * that signing out is something the code can finish rather than a tidy-up left
 * to a person in the settings window.
 *
 * **A secret is never handed to an agent — not the value, not by any route.**
 * Not in a prompt, not in the environment a process is raised with, not as a
 * tool that answers with it. What an agent is given is a *method that does the
 * work*: sign this request, fetch this page, post this comment. The password
 * does the work and stays here; the agent gets the outcome.
 *
 * Sync does not check this and cannot. A value that has crossed into a
 * package's own JavaScript is that package's to pass on, and the call that
 * would pass it on is invisible to anything that reads a manifest — the same
 * reason `work.agent` is refused when it is called rather than when the file is
 * parsed. So this is a rule stated where its author reads it, and a check that
 * pretended to close it would be worse than saying so: an agent's transcript is
 * kept, read back, and sent to a model again, and a token that reaches one has
 * been published rather than leaked.
 */
export interface ExtensionVault {
  /**
   * Reads one of this package's secrets, or rejects saying why it did not.
   *
   * What rejects is nothing stored under that name, a store this machine will
   * not open, and a system asking somebody for permission that nobody answered.
   * The last one is a refusal in words rather than a wait, which is what makes
   * this callable by code running while its owner is asleep.
   */
  read(name: string): Promise<string>;
  /** Puts a secret in this package's namespace, or replaces the one there. */
  write(name: string, secret: string): Promise<void>;
  /** Takes one of this package's secrets out. */
  forget(name: string): Promise<void>;
}

/**
 * What an extension's module is handed when it starts.
 *
 * Its own id, and what its own manifest let it reach. An earlier draft also
 * handed over React and the surface, because the first spike had no other way
 * to get them across a module boundary; a built extension now writes ordinary
 * imports and the host resolves them, so passing the same objects a second time
 * would be a second way to reach them — and two ways is how one of them goes
 * stale.
 *
 * The id is here because it is the one thing a module cannot know about
 * itself: its own name is decided by the manifest beside it, and repeating it
 * in code is a copy that can disagree.
 *
 * `net` is here for the same reason, one step further on. Everything else on
 * the surface is a function a package imports, and a network call cannot be
 * one: what may be reached is that package's permission, so the call has to
 * carry which package is making it, and an id passed as an argument would be an
 * extension stating its own. It is handed over instead, already attributed —
 * the package holds what it was given, and there is nothing to hold for a
 * package whose manifest asked for nothing.
 *
 * `vault` is the same shape again and the reason is sharper: an id in the
 * argument list would be one package spelling another's namespace, which is the
 * whole of what a namespace is for.
 */
export interface ExtensionHost {
  readonly id: string;
  readonly net: ExtensionNet;
  readonly vault: ExtensionVault;
}

/**
 * What the window tells an area, and the only three things it tells it.
 *
 * Everything else an area shows, it fetches or holds itself. That is not
 * minimalism for its own sake: a prop the window passes is a decision the
 * window has made about what the area is for, and the window is the one file
 * that must not know what any area contains.
 */
export interface AreaProviderProps {
  readonly project: OpenProject;
  /**
   * False while the area is mounted but not the selected one.
   *
   * An area is mounted on first visit and never unmounted, so this is what tells
   * it to stop: no reads, no scans, no menu. It keeps everything it holds — the
   * selection, the open record, the caret, the scroll position — because coming
   * back to a window as it was left is what the arrangement is for.
   */
  readonly active: boolean;
  /**
   * What the window is asking this area to show, or `null` when it is asking
   * nothing.
   *
   * Only the area an intent was addressed to is given one, and it is given the
   * same object until the next ask. **Identity is the signal**: an area applies
   * an object it has not applied yet, so asking for the same record twice is
   * two objects and opens it twice — the second ask is somebody who wandered
   * off and wants it back, not a duplicate to swallow.
   */
  readonly intent?: AreaIntent | null;
  readonly children: ReactNode;
}

/**
 * One section: what holds its state, and one component per column of its frame.
 *
 * The provider is what makes three columns one area. It owns everything the
 * area is showing and everything it opens — its selection, its sheets, the File
 * commands it contributes — so selecting a different area takes all of it away
 * by unmounting rather than by the window remembering to clear anything. The
 * columns are separate components because the window renders them into three
 * different places in its panel tree; what holds them together is whatever the
 * provider puts in context, which is the area's own business and invisible to
 * the host.
 *
 * The provider is optional, because an area with nothing to share should not
 * have to write an empty wrapper to say so.
 *
 * Which columns to return is decided by the frame the manifest declared, and
 * getting it wrong is refused at load rather than trimmed: returning an
 * inspector for a `list` frame is code whose author believes it will be
 * rendered, and a panel that is empty because a component was dropped without a
 * word is an hour spent looking for the wrong bug.
 */
export interface AreaModule {
  readonly Provider?: ComponentType<AreaProviderProps>;
  /** Rendered in the navigator, for a frame that has one. */
  readonly Navigator?: ComponentType;
  readonly Workspace: ComponentType;
  /** Rendered in the inspector, for a frame that has one. */
  readonly Inspector?: ComponentType;
}

/**
 * What `activate` returns: one entry per area the manifest declared, by area id.
 *
 * An extension declaring exactly one area may return that area's module
 * directly. One area is the common case, and making it look like the general
 * one costs an author a wrapper object for no reason.
 */
export type ActivationResult = Readonly<Record<string, AreaModule>>;
