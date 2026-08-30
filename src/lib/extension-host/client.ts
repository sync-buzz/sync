"use client";

import { invoke } from "@tauri-apps/api/core";

import type {
  FieldDeclaration,
  RelationshipDeclaration,
} from "@/lib/memory/types";

/**
 * What this machine has installed, as Rust answers.
 *
 * The shapes mirror `sync-extensions`. They are written out rather than derived
 * because the boundary between the window and the desktop layer is exactly
 * where a field goes missing without an error — a lesson this project has paid
 * for once already — and two declarations that must agree are at least two
 * things a reader can compare.
 */

/** What the host must be able to do before a package is worth loading. */
export interface Engines {
  readonly syncApi: string;
}

/**
 * A count the host draws on a section's row, declared rather than reported.
 *
 * Nothing of the package runs to produce it, which is the point: a section is
 * mounted the first time it is visited, so a number only a mounted section
 * could raise would be missing in exactly the moment somebody needs it — the
 * first launch after opening a project.
 */
export interface ManifestBadge {
  /** Empty is every kind this section opens, which is the ordinary answer. */
  readonly kinds: readonly string[];
  /** Empty is every state. Checked against the four the engine derives. */
  readonly freshness: readonly string[];
}

/** One section an extension contributes to the window. */
export interface ManifestArea {
  readonly id: string;
  readonly label: string;
  readonly description: string;
  /** `browse`, `list`, `detail` or `single`, checked against the shell's set. */
  readonly frame: string;
  readonly icon: string | null;
  readonly badge: ManifestBadge | null;
}

export interface ManifestOpens {
  readonly kinds: readonly string[];
  /** Whether it opens the types the project made itself. */
  readonly projectTypes: boolean;
}

export interface Manifest {
  readonly manifestVersion: number;
  readonly id: string;
  readonly version: string;
  readonly name: string;
  readonly summary: string;
  readonly description: string;
  readonly icon: string | null;
  readonly engines: Engines;
  readonly capabilities: readonly string[];
  /**
   * Where it may reach, and empty is a package that reaches nothing.
   *
   * Mirrored here for the reason the rest of this shape is: a field that
   * crosses this boundary undeclared arrives as `undefined` without an error
   * anywhere, and this one decides whether a request leaves the machine.
   */
  readonly net: {
    readonly hosts: readonly string[];
    /**
     * Which of this machine's secrets go where, and in which header.
     *
     * Mirrored because the card says it before anybody installs: a person
     * agreeing to a package that sends one of their tokens somewhere is
     * agreeing to that, and it is not a thing to find out afterwards. The value
     * is never here and never in the window — it is read in Rust, put into the
     * request there, and the package that declared it does not hold one.
     */
    readonly secrets: readonly {
      readonly host: string;
      readonly header: string;
      readonly secret: string;
      readonly scheme: string | null;
    }[];
  };
  readonly requires: { readonly extensions: readonly string[] };
  readonly areas: readonly ManifestArea[];
  /** Paths inside the package, each a type definition. */
  readonly types: readonly string[];
  readonly opens: ManifestOpens;
  readonly prompt: string | null;
  readonly dependencies: { readonly npm: readonly string[] };
  readonly author: { readonly name: string; readonly url: string | null } | null;
  readonly license: string | null;
  readonly repository: string | null;
  /**
   * The built module, relative to the package root, or `null`.
   *
   * An extension is not necessarily a screen: one that publishes only a
   * vocabulary and a prompt has no code to run, and making it ship a stub
   * module would be a file whose only reader is the packer.
   */
  readonly ui: string | null;
  /**
   * The built service module, relative to the package root, or `null`.
   *
   * Where the package's handlers live: what it does when no screen is mounted.
   * Independent of `ui` — a package may have a screen and no handlers, or
   * handlers and no screen. The window never loads it: it is read by Rust and
   * runs in an isolate there.
   */
  readonly service: string | null;
  /** Handlers called because something happened to the package itself. */
  readonly lifecycle: { readonly installed: string | null };
  /**
   * What it offers an agent to call, and what an agent is told about each.
   *
   * Mirrored here because the window is what carries a declaration from the
   * package into the project's record: the server an agent reaches has no view
   * of the catalogue, so what it may call has to be written where the project
   * keeps it. A member missing from this shape arrives as `undefined` with no
   * error anywhere, which is how a tool comes to exist in a manifest and be
   * offered to nobody.
   *
   * `handler` is here because the shape is the manifest's and this mirrors it
   * whole; nothing in the window reads it, and it is not what travels into the
   * record — it is the package's own name for one of its functions.
   */
  readonly tools: readonly {
    readonly handler: string;
    readonly name: string;
    readonly description: string;
    readonly input: unknown;
  }[];
  /**
   * Handlers called because a clock struck, and how often.
   *
   * `every` is the string the author wrote — `30m`, `6h`, `1d` — rather than a
   * number of seconds, because it is also what the extension's page says. The
   * host refuses a manifest whose `every` is not one of those, so anything that
   * reaches here parses.
   */
  readonly schedule: readonly {
    readonly handler: string;
    /**
     * What this handler does, in the package author's words.
     *
     * Required of a package that asks for the clock, because nothing else on
     * the page can answer *what runs*: the handler's own name is the package's
     * internal name for one of its own functions and is not shown, and `every`
     * says only how often. The cadence beside it is the host's and is derived
     * from `every`, so the two cannot disagree about how often anything
     * happens.
     */
    readonly description: string;
    readonly every: string;
  }[];
  /**
   * The built stylesheet, relative to the package root, or `null`.
   *
   * A package carries the utility rules its own markup uses, because the
   * window's stylesheet holds only what the window's own source uses. It
   * carries no values — every rule refers to a variable the window defines.
   */
  readonly styles: string | null;
}

/**
 * Where a package came from, and therefore how much it is trusted.
 *
 * `seeded` is what shipped with the application: not fetched, as old as this
 * build, and replaced by an ordinary registry install the moment there is a
 * newer one. It is its own word rather than `registry` because the card is
 * where somebody reads what came through which door.
 */
export type ExtensionSource = "registry" | "file" | "folder" | "seeded";

/**
 * What is known about who produced it.
 *
 * Three states rather than a boolean: "not signed" and "signed by someone we
 * cannot verify" are different facts, and only one of them is suspicious.
 */
export type SignatureState = "valid" | "invalid" | "absent";

/**
 * One type a package publishes, as the engine's schema asks for it.
 *
 * Read out of the artefact by Rust rather than assembled here: a file inside an
 * artefact is reachable from the window only over `syncext://`, and fetching one
 * would mean widening the webview's `connect-src`. It also has to work for a
 * package that ships no code at all — an extension publishing only a vocabulary
 * never loads a module, and its types still have to reach the project.
 *
 * Forwarded to `publishExtensionTypes` unchanged. There is deliberately no
 * translation step: one spelling of a type definition between the file an author
 * writes and the transaction the engine runs.
 */
export interface PackagedType {
  readonly kind: string;
  readonly title: string;
  readonly description: string;
  readonly icon: string;
  /** The product fields records of this type carry. */
  readonly fields?: Readonly<Record<string, FieldDeclaration>>;
  /** The relations they may hold. A type declaring none cannot link at all. */
  readonly relationships?: Readonly<Record<string, RelationshipDeclaration>>;
  /** What an agent reads before writing one. */
  readonly guidance?: string;
}

export interface Pointer {
  readonly id: string;
  readonly version: string;
  readonly integrity: string | null;
  readonly source: ExtensionSource;
  readonly path: string | null;
  readonly signature: SignatureState;
}

export interface InstalledExtension {
  readonly manifest: Manifest;
  readonly pointer: Pointer;
  /**
   * The URL its module is imported from, and `null` for a package that ships
   * none. Built by Rust, never by the window.
   */
  readonly ui: string | null;
  /**
   * The URL its stylesheet is fetched from, and `null` when it ships none.
   * Built by Rust, never by the window, exactly like [`ui`].
   */
  readonly styles: string | null;
  /** The types installing it would publish. Empty is a real answer. */
  readonly types: readonly PackagedType[];
  /** What it tells an agent, whole, or `null` when it says nothing. */
  readonly prompt: string | null;
  /**
   * Why this package cannot be used, when it cannot.
   *
   * A package whose manifest read and whose type definitions did not is still
   * one somebody can see and remove, so it is listed with the reason rather
   * than dropped: a broken package presented as one that was never installed is
   * a different problem with a different answer.
   */
  readonly defect: string | null;
}

/** Installs a `.syncext` somebody chose in the open panel. */
export function installExtensionFile(path: string): Promise<InstalledExtension> {
  return invoke<InstalledExtension>("extension_install_file", { path });
}

/**
 * Points an id at a folder somebody is writing in.
 *
 * Unsigned by construction, and marked everywhere it appears. The path stays on
 * this machine: a project declares `{id, version}`, and an absolute path from
 * somebody else's disk in a shared record is noise at best.
 */
export function installExtensionFolder(path: string): Promise<InstalledExtension> {
  return invoke<InstalledExtension>("extension_install_folder", { path });
}

/** Everything this machine can load, whatever any project declares. */
export function installedExtensions(): Promise<InstalledExtension[]> {
  return invoke<InstalledExtension[]>("extension_list");
}

/**
 * Runs the handler an extension declares for an occasion, and answers what it
 * returned.
 *
 * `null` says the package declares nothing for this occasion, which is the
 * usual answer and not a failure. A rejection is the handler's own — it threw,
 * it ran past its limit, or the manifest and the module disagree — and the
 * words name the package first, because by the time anybody reads them that is
 * what they need to know.
 *
 * Nothing about the handler crosses this boundary but its answer. It ran in an
 * isolate in Rust with only what the host handed it, and this window neither
 * loaded it nor could have.
 */
export function callExtensionHandler(
  project: string,
  id: string,
  occasion: string,
  payload: unknown,
): Promise<unknown> {
  return invoke<unknown>("extension_handler_call", {
    project,
    id,
    occasion,
    payload,
  });
}

/** Stops serving an id on this machine. The artefact and its records stay. */
export function forgetExtension(id: string): Promise<void> {
  return invoke<void>("extension_forget", { id });
}

/**
 * Remember what this project declares, so the clock can find its handlers with
 * no repository opened.
 *
 * What a project declares lives in the project's own memory, which is behind a
 * repository the clock has no business opening — so the ids are copied into
 * this installation's configuration whenever a window opens the project. A
 * project this installation has never opened does not tick, and that is honest:
 * nothing here knows what it declares.
 *
 * **Every declared id, not only the ones with a schedule.** Which of them wants
 * the clock is in each package's manifest, which is already on this machine, so
 * filtering here would copy the manifest's answer into a file that exists
 * precisely so as not to hold one — and that copy would be wrong the moment a
 * package was updated or edited in its own folder.
 */
/**
 * Which extensions' clocks a person has switched off in this project.
 *
 * The exceptions, not the rule: installing an extension that declares a
 * schedule was the consent, so everything declared runs unless it is in this
 * list. Answering the other way round would be a second consent written down.
 */
export function switchedOffClocks(project: string): Promise<string[]> {
  return invoke<string[]>("schedule_switched_off", { project });
}

/**
 * Stop, or restart, one extension's clock in this project.
 *
 * A switch and not a gate. It takes back for one project
 * what installing the package agreed to, without removing the package, and it
 * says nothing about any other project the same extension is in.
 */
export function switchClock(
  project: string,
  id: string,
  on: boolean,
): Promise<void> {
  return invoke<void>("schedule_switch", { project, id, on });
}

export function rememberDeclaration(
  project: string,
  extensions: readonly string[],
): Promise<void> {
  return invoke<void>("schedule_remember", { project, extensions });
}

// ---------------------------------------------------------------------------
// The registry: what exists anywhere.
//
// The one outward connection this application makes, and it is made in Rust.
// The window's `connect-src` names itself and the IPC endpoint and nothing
// else, so nothing on a page can reach the network — what is reachable is a
// property of the build, with the hosts compiled into the binary.
// ---------------------------------------------------------------------------

/** Where an artefact is, and what it must hash to. */
export interface RegistryArtefact {
  readonly url: string;
  readonly sha256: string;
  readonly bytes: number;
}

/**
 * One extension as the index lists it, at the length a card needs.
 *
 * Deliberately not a manifest: what a card says is a subset of what a package
 * says, and the description, the changelog and the older versions are the
 * extension's own ledger, read when a page is opened rather than carried in the
 * file every window fetches at every launch.
 */
export interface ListedExtension {
  readonly id: string;
  readonly name: string;
  readonly summary: string;
  readonly icon: string | null;
  readonly version: string;
  /** The range of Sync's extension API it was written for. */
  readonly syncApi: string;
  readonly capabilities: readonly string[];
  readonly requires: readonly string[];
  /** The kinds it publishes — also what somebody searching for a word wants. */
  readonly publishes: readonly string[];
  readonly areas: readonly { readonly id: string; readonly label: string }[];
  /** Whether it tells an agent anything at all. */
  readonly prompt: boolean;
  readonly npm: readonly string[];
  readonly author: { readonly name: string; readonly url?: string | null } | null;
  readonly license: string | null;
  readonly repository: string | null;
  readonly artefact: RegistryArtefact;
}

export interface RegistryIndex {
  readonly formatVersion: number;
  readonly extensions: readonly ListedExtension[];
}

export interface FetchedRegistry {
  readonly answer: RegistryIndex;
  /**
   * True when this is what was on disk rather than what the network answered.
   *
   * The difference between *these are the extensions there are* and *these are
   * the extensions there were when this machine last had a network*, and a
   * catalogue that could not tell them apart would present the second as the
   * first.
   */
  readonly cached: boolean;
}

/** One published version of one extension, as its ledger records it. */
export interface RegistryRelease {
  readonly version: string;
  /**
   * The range of Sync's extension API *this version* was written for.
   *
   * Per version rather than per extension, and that is what makes an offer
   * honest: an extension whose newest release needs a Sync this build is below
   * is one whose older releases may still be perfectly installable.
   */
  readonly syncApi: string;
  readonly description: string;
  /** What changed, in the author's words. Empty for a first release. */
  readonly changelog: string;
  readonly artefact: RegistryArtefact;
}

/** Every version one extension has published, newest first. */
export interface RegistryLedger {
  readonly formatVersion: number;
  readonly id: string;
  readonly versions: readonly RegistryRelease[];
}

/**
 * What the registry says exists.
 *
 * Fetched with an ETag and cached, so this usually costs a 304 and a read from
 * disk. A failure with something cached answers with the cache and says so; a
 * failure with nothing cached is the network's own words.
 */
export function registryIndex(): Promise<FetchedRegistry> {
  return invoke<FetchedRegistry>("registry_index");
}

/**
 * What the last fetch left on the disk, without asking anybody anything.
 *
 * The window reads this when a project opens, so that the pinned Extensions row
 * can say there is something newer without every launch becoming a request.
 * That distinction is the whole reason it exists beside [`registryIndex`]:
 * that one is somebody opening the catalogue and asking what exists, this one
 * is the window reading what it was already told.
 *
 * `null` on a machine that has never fetched an index, which is not a failure —
 * there is nothing to say about updates yet, and the row says nothing.
 */
export function cachedRegistryIndex(): Promise<RegistryIndex | null> {
  return invoke<RegistryIndex | null>("registry_cached_index");
}

/**
 * Every version one extension has published, and what changed in each.
 *
 * A file of its own, fetched when a page is opened. The index carries the
 * newest version of everything; the changelog of every version of everything
 * would make the file every marketplace fetches grow without limit.
 */
export function registryLedger(id: string): Promise<RegistryLedger> {
  return invoke<RegistryLedger>("registry_ledger", { id });
}

/**
 * Points an id back at the artefact it was serving before an update.
 *
 * The rollback the last steps of an update need. Moving the pointer is not the
 * whole of applying one — the types are published into the project's memory and
 * the version is written into its record afterwards — so a failure in either
 * would otherwise leave a project declaring one version while this machine
 * serves another.
 */
export function repointExtension(pointer: Pointer): Promise<InstalledExtension> {
  return invoke<InstalledExtension>("extension_repoint", { pointer });
}

/**
 * Downloads what the index named and installs it.
 *
 * One call rather than two: there is nothing a person could do between them,
 * and a downloaded file nobody installed is litter with no owner. Whether this
 * build can run the package is decided **before** this is called, from the
 * `syncApi` range the index carries — which is why a card for a package this
 * build refuses offers no button rather than a button that fails.
 */
export function installFromRegistry(
  artefact: RegistryArtefact,
): Promise<InstalledExtension> {
  return invoke<InstalledExtension>("extension_install_registry", { artefact });
}
