"use client";

/**
 * A link that points inside the project: how it is spelled, what it resolves
 * to, and how following one gets out of the text and into the window.
 *
 * The markup is ordinary Markdown — `[text](url)`, nothing else — so a body
 * holding one of these is a body every other tool can read. There are two
 * spellings of the url, and which one a record uses is decided by where its own
 * body lives rather than by anybody's preference:
 *
 * - **A relative path**, exactly as GitHub reads one: `./setup.md`,
 *   `../adr/0007.md`, `/docs/index.md` from the repository root. This is the
 *   spelling for a document that is a file in an attached folder, and the
 *   reason it comes first is that it is not ours — a `docs/` folder already
 *   holds links people wrote for GitHub, and those are the links this makes
 *   work. Resolving one against the file it was written in is Markdown's own
 *   semantics, which is why that part is done here rather than asked of anyone.
 * - **`sync://<kind>/<key>`** for a record with no file, whose body lives in
 *   the corpus itself. There is no path to point at, so the record is named
 *   directly. A third segment in front of the kind names the project — the
 *   spelling an address takes when it leaves the window, where nothing knows
 *   which of the machine's projects was meant.
 *
 * The kind is in the second spelling deliberately. A record's kind is what
 * decides which part of the window opens it — `openerOf` in
 * `components/shell/opening.ts` is the whole of that rule — so a link carrying
 * it can be followed by a window that has never read the record, never loaded
 * the type and never opened the section. What is deliberately absent is the
 * *extension*: which extension publishes a kind is the catalogue's answer and
 * it changes when a project installs something, so a copy of it written into
 * somebody's prose would be a fact going out of date inside a sentence. The
 * kind is the namespace and the extension is derived from it at the click.
 *
 * Turning a resolved path into a record is a question for the store and is
 * asked of it, not derived here. The engine files every record under the
 * directory of its locator and publishes both, so the question is a narrow list
 * and an exact match — see `components/shell/record-links.tsx`. Deriving a key
 * from a path is the one thing this must never do; that is the engine's
 * arithmetic, and a copy of it in this window has already once overwritten a
 * record somebody else wrote.
 */

import { createContext, useContext, useMemo } from "react";

/** The scheme that names a record with no file of its own. */
export const RECORD_SCHEME = "sync";

/** What a link points at, when it points anywhere this window can reach. */
export type LinkTarget =
  /**
   * A record, named — and the project it is in, where the link says. `null` is
   * *this* project, which is what a link written inside one means and what every
   * body in every corpus already holds.
   */
  | {
      readonly at: "record";
      readonly key: string;
      readonly kind: string;
      readonly project: string | null;
    }
  /**
   * A file of an attached folder, by its path from the repository root. Which
   * record holds it is the store's to say, and is asked when it is followed.
   */
  | { readonly at: "file"; readonly locator: string; readonly kind: string }
  /** An address outside the project, for the system to open. */
  | { readonly at: "web"; readonly url: string };

/**
 * The addresses this window may hand to the system, and exactly the four the
 * capability grants: `http`, `https`, `mailto`, `tel`.
 *
 * Listed rather than inferred from "has a scheme". A body is somebody else's
 * text and may hold any scheme at all, including ones that would run something;
 * the capability refuses those anyway, and a link drawn as followable that the
 * boundary then refuses would be this window promising what it cannot do.
 */
const WEB_URL = /^(?:https?|mailto|tel):/i;

export function isWebUrl(url: string): boolean {
  return WEB_URL.test(url.trim());
}

/**
 * The record a `sync://` url names, or `null` for any other url.
 *
 * Two segments name a record and leave the project to be understood; three name
 * the project first, by the identifier it answers to on the machine the link is
 * followed on. The second spelling exists because the first cannot survive
 * leaving the window — see `src-tauri/src/links.rs`, which reads the same
 * grammar for an address arriving from anywhere else on the machine, and has to
 * read it identically: a link the two sides disagree about opens the wrong
 * record or none.
 *
 * Parsed rather than handed to `URL`, and the segments are what makes that
 * worth stating twice: a kind decides which section opens the record and an
 * identifier decides which project does, so neither may be quietly folded or
 * normalised by something that thinks it is looking at a host.
 *
 * A raw `/` inside a key is what the three-segment spelling costs, and it costs
 * nothing real: every segment of every address Sync writes is escaped — here and
 * in `sync-mcp`'s `link.rs` both — so a slash in a key arrives as `%2F` and
 * comes back out of the segment it was written in.
 */
export function recordTarget(
  url: string,
): { key: string; kind: string; project: string | null } | null {
  const trimmed = url.trim();
  const scheme = trimmed.indexOf("://");
  if (scheme === -1) return null;
  // The scheme is the one part read either way, because that is what a scheme
  // is. Everything after it is somebody's name for something.
  if (trimmed.slice(0, scheme).toLowerCase() !== RECORD_SCHEME) return null;

  // The query and the fragment are dropped: nothing in this grammar reads
  // either, and an address that came back from a chat client with something
  // appended still names the record it named. One trailing slash is forgiven for
  // the same reason, and a second one is not — by then it is not an address.
  const path = trimmed.slice(scheme + 3).split(/[?#]/, 1)[0];
  const written = path.endsWith("/") ? path.slice(0, -1) : path;

  const raw = written.split("/");
  if (raw.length < 2 || raw.length > 3) return null;
  if (raw.some((segment) => segment === "")) return null;

  try {
    const [first, second, third] = raw.map(decodeURIComponent);
    return third === undefined
      ? { project: null, kind: first, key: second }
      : { project: first, kind: second, key: third };
  } catch {
    // A url this window cannot decode is a url it does not own. Drawn as an
    // ordinary link, which is what it looks like to everything else too.
    return null;
  }
}

/**
 * How a record with no file is addressed from inside a body.
 *
 * Without a project, which is what a body is written with: the identifier is
 * what a project answers to on *this* machine, and a body travels with the
 * repository it is in. With one where the address is leaving the window — the
 * only thing that can follow a link from a terminal or another application is
 * an address that says which project it is about.
 */
export function recordHref({
  kind,
  key,
  project = null,
}: {
  kind: string;
  key: string;
  project?: string | null;
}): string {
  const named = project === null ? "" : `${encodeURIComponent(project)}/`;
  return `${RECORD_SCHEME}://${named}${encodeURIComponent(kind)}/${encodeURIComponent(key)}`;
}

/** The directory part of a repository path. `""` is the root. */
export function folderOf(locator: string): string {
  const cut = locator.lastIndexOf("/");
  return cut === -1 ? "" : locator.slice(0, cut);
}

/**
 * Whether a url is a path into this repository rather than an address outside
 * it: no scheme, not protocol-relative, and not a bare fragment.
 *
 * Deliberately shallow. It does not say whether the path resolves — that needs
 * the file the link is written in — only that it is the *kind* of url that
 * could. Two things ask, and both want exactly this much:
 *
 * - The link plugin, which refuses to make a link out of a url its `isUrl` does
 *   not recognise. Without this, `upsertLink` silently declines every relative
 *   path, so inserting one wrote nothing at all and typing `[text](./a.md)` by
 *   hand left the brackets on the page.
 * - Anything deciding whether to bother resolving.
 */
export function isProjectPath(url: string): boolean {
  const path = url.trim();
  if (path === "" || path.startsWith("#") || path.startsWith("//")) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(path);
}

/**
 * Where a relative url points, as a path from the repository root.
 *
 * GitHub's rules, because these are GitHub's links: a leading `/` is the
 * repository root, anything else is relative to the *directory of the file the
 * link is written in*, and `.` and `..` mean what they mean everywhere. The
 * query and the fragment are dropped — this window has nowhere to scroll to
 * yet, and a link to a heading should still open the document.
 *
 * `null` for anything that is not a path into this repository: a url with a
 * scheme, a protocol-relative url, a bare fragment, a path that climbs out
 * above the root, and — the one worth stating — *any* relative path in a
 * record whose body has no file. Such a record is nowhere in the working tree,
 * so there is no directory for `./` to be relative to, and guessing one would
 * be inventing a location for something that has none.
 */
export function resolveLocator(base: string | null, url: string): string | null {
  const path = url.split(/[?#]/, 1)[0].trim();
  if (path === "") return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(path) || path.startsWith("//")) return null;

  const fromRoot = path.startsWith("/");
  if (!fromRoot && base === null) return null;

  const segments = fromRoot
    ? []
    : folderOf(base ?? "")
        .split("/")
        .filter((segment) => segment !== "");

  for (const raw of path.split("/")) {
    if (raw === "" || raw === ".") continue;
    if (raw === "..") {
      // Above the repository root there is nothing this project can open, and
      // a path that climbs there is answered as what it is rather than clamped
      // to the root and opened as something else.
      if (segments.length === 0) return null;
      segments.pop();
      continue;
    }
    try {
      segments.push(decodeURIComponent(raw));
    } catch {
      segments.push(raw);
    }
  }

  return segments.length === 0 ? null : segments.join("/");
}

/**
 * How to write a path from one file of the repository to another.
 *
 * The inverse of [`resolveLocator`], and it has to stay the inverse: what this
 * writes into somebody's prose is what that has to read back, and the two
 * disagreeing would be links that work until they are reopened.
 *
 * A sibling is spelled `./name.md` rather than bare `name.md`. Both resolve the
 * same everywhere, and the explicit one cannot be mistaken for a scheme by
 * anything that reads Markdown less carefully than this does.
 *
 * Every segment is percent-encoded, and that is not tidiness. A Markdown
 * destination ends at the first space, so `docs/my notes.md` written plainly is
 * a link to `docs/my` followed by the visible text `notes.md)`, and a bracket
 * or a `#` in a file name goes wrong in its own way. Encoding is what every
 * renderer already understands, and [`resolveLocator`] decodes it back.
 */
export function relativeLocator(base: string, target: string): string {
  const from = folderOf(base).split("/").filter(Boolean);
  const to = target.split("/");
  const name = to.pop() ?? target;

  let shared = 0;
  while (shared < from.length && shared < to.length && from[shared] === to[shared]) {
    shared += 1;
  }

  const up = Array.from({ length: from.length - shared }, () => "..");
  const down = [...to.slice(shared), name].map(encodeSegment);
  const path = [...up, ...down].join("/");
  return up.length === 0 ? `./${path}` : path;
}

/**
 * One path segment, safe in a Markdown destination.
 *
 * `encodeURIComponent` leaves the brackets alone — they are legal in a URI —
 * and a bracket is exactly what delimits a Markdown link, so they are escaped
 * after it rather than left for a renderer to guess about.
 */
function encodeSegment(segment: string): string {
  return encodeURIComponent(segment)
    .replace(/\(/g, "%28")
    .replace(/\)/g, "%29");
}

/**
 * Reading and following the links in a body.
 *
 * One object rather than two calls, because the two halves have to agree: a
 * link is drawn as followable exactly when following it has somewhere to go,
 * and a component that could ask one question without the other would be free
 * to draw a promise the window cannot keep.
 *
 * `null` where nothing provides it — the settings window, a test — and every
 * link is then drawn as text, which is what it was before this existed.
 */
export interface RecordLinks {
  /**
   * What this url points at, given the file the body it is written in lives in.
   *
   * A web address is one of the three answers rather than an absence — see
   * `LinkTarget` — and `null` means *nowhere*: a path resolving outside every
   * attached folder, or a scheme the capability does not grant.
   */
  readonly targetOf: (url: string, base: string | null) => LinkTarget | null;
  readonly follow: (target: LinkTarget) => void;
  /**
   * A picture in a body, as something an `<img>` can take — or `null` when the
   * path is not a document of this project, is not on this branch, or is not
   * bytes at all.
   */
  readonly pictureFor: (
    url: string,
    base: string | null,
  ) => Promise<string | null>;
  /** The project whose records a link can point at, as the store names it. */
  readonly project: string;
  /**
   * How a link to this record is written from a body living at `base`.
   *
   * Asynchronous because the spelling depends on where the *target's* body
   * lives, and that is a fact about the record rather than about the search
   * result somebody clicked. One read, at the one moment it decides something.
   */
  readonly hrefTo: (
    key: string,
    kind: string,
    base: string | null,
  ) => Promise<string>;
  /** The mark a kind is drawn with, as the shell names icons. */
  readonly iconOf: (kind: string) => string | null;
}

const Links = createContext<RecordLinks | null>(null);

export const RecordLinksProvider = Links.Provider;

export function useRecordLinks(): RecordLinks | null {
  return useContext(Links);
}

/**
 * Showing one record, for a section that has a reason to point at another.
 *
 * The narrow half of what the shell's own bodies follow links with, and it is
 * narrow deliberately. What a section outside the shell needs is *show this
 * one*; the rest of that interface is how a body is parsed, where a picture
 * comes from and how a link is spelled from one record to another, none of
 * which is a question an area asks and all of which would be a wider agreement
 * than the one thing it wants.
 *
 * The kind travels with the key for the reason an intent carries it: which area
 * shows a record is decided by its type, and nothing can find that out from a
 * key without reading the record first.
 *
 * `null` where nothing can show anything — the settings window, a test — so a
 * section leaves the command out rather than drawing one that does nothing.
 */
export function useOpenRecord():
  | ((record: { key: string; kind: string }) => void)
  | null {
  const links = useContext(Links);
  return useMemo(
    () =>
      links === null
        ? null
        : ({ key, kind }) =>
            // No project: a section showing a record of the project it is
            // mounted in is the only record it can be asking for.
            links.follow({ at: "record", key, kind, project: null }),
    [links],
  );
}

/**
 * The record a link is being written *from*.
 *
 * Provided by whatever put a body on screen, because only it knows which record
 * that body is. Passing it down through the editor as a prop would put it in
 * the signature of every block that can hold a link.
 *
 * Two things need it, and they are the two halves of "from here":
 *
 * - **Where.** A relative link means nothing without the file it was written
 *   in — `./setup.md` is a different file in every folder.
 * - **Which.** A record must not be offered as a link target while it is the
 *   record being written. Nobody links a claim to itself, and a row for it in
 *   the list is a row that can only be a misclick.
 * - **Whose.** A picture pasted into a body has to go somewhere, and the answer
 *   is the storage of the type this record is — the one being edited. There is
 *   no other honest candidate: a project may have several attached folders, and
 *   choosing between them on somebody's behalf would put a file in a part of
 *   their repository they were not looking at.
 *
 * `null` where nothing provides it — the settings window, a test.
 */
export interface LinkOrigin {
  readonly key: string;
  /** The type this record is, which is also the storage a file dropped into it goes to. */
  readonly kind: string;
  /** What the record is called, which is what a nameless picture is named after. */
  readonly title: string;
  /** The file this body lives in, or `null` for a record that is not a file. */
  readonly locator: string | null;
}

const Origin = createContext<LinkOrigin | null>(null);

export const LinkOriginProvider = Origin.Provider;

export function useLinkOrigin(): LinkOrigin | null {
  return useContext(Origin);
}
