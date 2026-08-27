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
 * What came back from a host an extension declared, as it reads it.
 *
 * The status and the body, and nothing else. A package that can see the status
 * tells *this repository has no issues* apart from *this is asking for a
 * token*, which is the difference a person needs said; headers are the rest of
 * an HTTP conversation, and a surface that carried one would be a surface that
 * has to keep in step with a protocol.
 *
 * The body is text. What it means is the package's business — every API it
 * could have been installed to read has its own shape, and a host that parsed
 * one would be the window having an opinion about somebody else's JSON.
 */
export interface NetAnswer {
  readonly status: number;
  readonly body: string;
}

/**
 * The one door out of this window, for the one package that declared it.
 *
 * **It reads, and there is nothing here that writes.** No method, no body, no
 * header: a URL, and what comes back. A header is where a token goes and a body
 * is where an instruction goes, and a package installed to read something needs
 * neither — when one of them has a reason, it arrives as a decision rather than
 * as a field that was already on the surface.
 *
 * Where it may reach is `net.hosts` in the package's own manifest, and the
 * check is Rust's: the window's `connect-src` does not name the outside at all,
 * so this is not a restraint on a package that could otherwise fetch — there is
 * no reach here to take away. Every redirect is checked again, so a hop off the
 * declared list is refused as firmly as the first request.
 */
export interface ExtensionNet {
  /**
   * Reads one URL, or rejects saying why it did not.
   *
   * A `404` is not a rejection: it is an answer to the question the package
   * asked, and it comes back as a status for the package to explain. What
   * rejects is a request that never happened — a host the manifest does not
   * name, a scheme that is not `https`, an answer too large to read, or no
   * network at all.
   */
  read(url: string): Promise<NetAnswer>;
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
 */
export interface ExtensionHost {
  readonly id: string;
  readonly net: ExtensionNet;
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
