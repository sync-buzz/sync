"use client";

import { useMemo } from "react";

import { refuseUnrunnable, unavailableFor } from "@/lib/extension-host/activate";
import type {
  InstalledExtension,
  ListedExtension,
  McpTransportSpec,
} from "@/lib/extension-host/client";
import type { Packages } from "@/lib/extension-host/packages";
import { refuseIncompatible, unavailableHere } from "@/lib/extension-api/version";

/**
 * What the catalogue has to show, and where each entry comes from.
 *
 * There used to be a constant here — `EXTENSIONS`, three entries, every
 * description and icon written by hand in the shell. It is gone, and what
 * replaces it is not a smaller list: it is two questions asked of two different
 * things and joined.
 *
 * - **What can this machine load?** The packages unpacked in the artefact
 *   directory. Each carries its own name, summary, version and everything else
 *   the card says, because a package describes itself — the shell has never
 *   read any of it before.
 * - **What does this project declare?** Ids in the project's own record. Every
 *   one of them belongs in the catalogue whether or not this machine has
 *   anything to satisfy it: a project composed elsewhere is the ordinary way to
 *   meet an id with no package, and answering it with silence would present a
 *   missing dependency as a project that never asked for one.
 *
 * The join is why an entry can be *declared and absent*, which is the state
 * neither list can express alone and the one a person most needs named.
 *
 * **Everything is shown, and the card says what state it is in.** An earlier
 * draft left a package this build will not run out of what there was to choose
 * from, on the argument that a card somebody cannot choose is a refusal to a
 * question they asked in good faith. That argument is right about a product
 * that has not shipped and wrong about a package already on the disk: it is
 * theirs, it takes up room, and removing it is something they may want to do.
 * It was survivable only while a second panel listed the disk in full — and
 * that panel is gone, so hiding one here would now hide it everywhere.
 *
 * The split is between the two columns rather than inside this list:
 *
 * - [`Catalogue.installed`] is what the navigator lists, and a row there means
 *   one thing — *this is a part of this window*. Declared by the project,
 *   answered by a package, and runnable in this build.
 * - [`Catalogue.entries`] is what the marketplace lays out as cards, and a card
 *   means *this is something a project could install*. That includes the two
 *   states a row cannot express without lying: unpacked and never asked for,
 *   and asked for and absent.
 *
 * **The third source is the registry** — what exists *anywhere*, rather than
 * what is on this machine or what this project asked for. It widens the
 * marketplace and touches the navigator not at all, which follows from the rule
 * above rather than being a separate decision: a row is what the project runs,
 * and nothing the registry lists is running until somebody installs it.
 *
 * A registry entry is answered for without a package, and everything a card
 * needs is in the index — the name, the summary, the version, what it would
 * publish, and the `syncApi` range that decides whether this build could run it
 * at all. That last one is why a card can say *needs a newer Sync* about
 * something nobody has downloaded: the refusal is computed from what the index
 * carries, so nothing is fetched to find out that it could not be used.
 */

/** One entry of the catalogue, from whichever of the two sources it came from. */
export interface CatalogueEntry {
  readonly id: string;
  /** What the package calls itself, or the bare id when there is no package. */
  readonly name: string;
  /**
   * The version this entry is about: the package's, or — for a declaration
   * nothing satisfies — the version the project asked for.
   */
  readonly version: string;
  /**
   * The icon name a card or a row draws, resolved once from whichever source
   * describes this entry — the manifest for a package, the index for a
   * registry listing, `null` for a declaration nothing satisfies.
   *
   * Carried here rather than reached for through `packaged?.manifest.icon ??
   * listed?.icon` at every site that draws one, because that chain is the
   * symptom of two sources and a card is not the place to decide between them.
   * An MCP server has no manifest, so its icon is the index's; a package has
   * one, so its icon is the manifest's; the decision is made once, here.
   */
  readonly icon: string | null;
  /**
   * One line of what this entry is, resolved from the same source as
   * [`icon`](Self::icon). Empty for a declaration nothing satisfies — the
   * card says so in its own words rather than inventing a summary.
   */
  readonly summary: string;
  /** The package on this machine, or `null` when nothing answers to the id. */
  readonly packaged: InstalledExtension | null;
  /**
   * What the registry says about it, or `null` when the registry does not list
   * it — which is every package that arrived from a file or a folder.
   *
   * Present *and* [`packaged`] present is the ordinary installed case, and it
   * is what an update will be read from: two versions of one id, one on the
   * disk and one in the index.
   */
  readonly listed: ListedExtension | null;
  /** Whether the project's own record names it. */
  readonly declared: boolean;
  /**
   * Why it cannot run here, or `null`. Asked of the package rather than of the
   * activation, so a card says *needs a newer Sync* without anything being run.
   */
  readonly unrunnable: string | null;
  /**
   * Why this machine cannot show it, though nothing about it is wrong.
   *
   * Separate from [`unrunnable`] because they mean opposite things about the
   * project. That one is a package this project should not be carrying; this
   * one is a package it is right to carry and that this screen has nothing to
   * run — a shell on a phone. So it greys a card out and leaves every button on
   * it alone: installing one from a phone is a decision about a repository that
   * a computer will honour.
   */
  readonly unavailable: string | null;
  /**
   * What kind of extension it is, for grouping in the marketplace.
   *
   * `null` for a package that is not in the registry — one installed from a
   * file or a folder — and for a registry entry whose author did not set one.
   * The marketplace puts entries with no category under *Other*.
   */
  readonly category: string | null;
  /**
   * How to reach an MCP server, when this entry names one rather than a
   * package.
   *
   * Present turns this into an MCP entry: it is added by descriptor rather than
   * fetched, and a card offers *Add* rather than *Install*. `null` for a code
   * package. The spec carries the descriptions a credential prompt needs; the
   * project record drops them when it stores one.
   */
  readonly transport: McpTransportSpec | null;
}

/**
 * A declaration this machine cannot satisfy, as an entry.
 *
 * The id is the whole of what is known — the name, the summary and the
 * description all live in a package nobody has — so the id is what it is
 * called. Inventing a title from it would be the window describing something it
 * has never seen.
 */
function undeliverable(id: string, version: string): CatalogueEntry {
  return {
    id,
    name: id,
    version,
    icon: null,
    summary: "",
    packaged: null,
    listed: null,
    declared: true,
    unrunnable: null,
    unavailable: null,
    category: null,
    transport: null,
  };
}

function entryOf(
  packaged: InstalledExtension,
  declared: boolean,
  listed: ListedExtension | null,
): CatalogueEntry {
  return {
    id: packaged.manifest.id,
    name: packaged.manifest.name,
    version: packaged.manifest.version,
    icon: packaged.manifest.icon ?? listed?.icon ?? null,
    summary: packaged.manifest.summary || listed?.summary || "",
    packaged,
    listed,
    declared,
    unrunnable: refuseUnrunnable(packaged),
    unavailable: unavailableFor(packaged),
    category: listed?.category ?? null,
    transport: null,
  };
}

/**
 * An MCP server the registry lists, as a catalogue entry.
 *
 * Delivered by descriptor rather than by package: there is nothing on the disk
 * to read a manifest from, and nothing to refuse — an MCP server has no
 * `syncApi` range and no capabilities, so `unrunnable` and `unavailable` are
 * both `null`. What it has is a `transport`, and a card offers *Add* rather
 * than *Install* on the strength of it.
 */
function mcpOf(listed: ListedExtension, declared: boolean): CatalogueEntry {
  return {
    id: listed.id,
    name: listed.name,
    version: listed.version,
    icon: listed.icon,
    summary: listed.summary,
    packaged: null,
    listed,
    declared,
    unrunnable: null,
    unavailable: null,
    category: listed.category,
    transport: listed.transport,
  };
}

/**
 * An entry the registry lists and this machine does not have.
 *
 * Answered for entirely from the index. Whether this build could run it is
 * decided here rather than after a download, because the index carries the
 * `syncApi` range and the capabilities and those are the whole of the question
 * — a card that offered to fetch something it would then refuse would spend
 * somebody's network to tell them no.
 */
function availableOf(listed: ListedExtension): CatalogueEntry {
  // An MCP server is delivered by its descriptor, so there is nothing to fetch
  // and nothing to refuse on the way to fetching it.
  if (listed.transport !== null) return mcpOf(listed, false);
  return {
    id: listed.id,
    name: listed.name,
    version: listed.version,
    icon: listed.icon,
    summary: listed.summary,
    packaged: null,
    listed,
    declared: false,
    unrunnable: refuseIncompatible({
      syncApi: listed.syncApi,
      capabilities: [...listed.capabilities],
    }),
    // The index carries the capabilities, so a card can say *not from a phone*
    // about something nobody has downloaded — which is the same economy the
    // line above is for, and the same one it would break to abandon here.
    unavailable: unavailableHere({
      syncApi: listed.syncApi,
      capabilities: [...listed.capabilities],
    }),
    category: listed.category,
    transport: null,
  };
}

export interface Catalogue {
  /** Every entry, declared ones first, in the order the project declares them. */
  readonly entries: readonly CatalogueEntry[];
  readonly byId: (id: string) => CatalogueEntry | null;
  /**
   * What the project actually runs, in its own order — the navigator's list.
   *
   * All three conditions, because a row in that column claims the window has
   * this section: the project asked for it, a package answers to the name, and
   * this build will load it. Anything short of that is a card and not a row.
   */
  readonly installed: readonly CatalogueEntry[];
}

/**
 * @param declared The project's own declarations, in its own order, so that the
 *   catalogue is ordered the way the sidebar is rather than by whatever order
 *   the artefact directory happened to be read in.
 */
export function useCatalogue(
  packages: Packages,
  declared: readonly { readonly id: string; readonly version: string }[],
  /** What the registry lists, and empty until its answer has arrived. */
  listed: readonly ListedExtension[] = [],
): Catalogue {
  const declaredKey = declared.map((entry) => `${entry.id}@${entry.version}`).join();

  return useMemo(() => {
    const asked = declaredKey === "" ? [] : declaredKey.split(",");
    const ids = new Set(asked.map((one) => one.slice(0, one.lastIndexOf("@"))));
    const inRegistry = new Map(listed.map((one) => [one.id, one]));

    const mine: CatalogueEntry[] = asked.map((one) => {
      const at = one.lastIndexOf("@");
      const id = one.slice(0, at);
      const packaged = packages.byId(id);
      const listedEntry = inRegistry.get(id) ?? null;
      // A declared MCP server has no package on the disk and is not missing —
      // its descriptor is what delivers it. `undeliverable` is for an id
      // nothing satisfies, which is a different state.
      if (packaged === null && listedEntry?.transport) return mcpOf(listedEntry, true);
      return packaged === null
        ? undeliverable(id, one.slice(at + 1))
        : entryOf(packaged, true, listedEntry);
    });

    const theirs = packages.all
      .filter((packaged) => !ids.has(packaged.manifest.id))
      .map((packaged) => entryOf(packaged, false, inRegistry.get(packaged.manifest.id) ?? null))
      .sort((one, two) => one.name.localeCompare(two.name));

    // Last, and only what neither of the two lists above already answered for.
    // A package on this machine describes itself better than an index entry
    // does — it is the manifest rather than a summary of one — so where both
    // exist the package wins and the index becomes what is *known about* it.
    const held = new Set([...mine, ...theirs].map((entry) => entry.id));
    const available = listed
      .filter((one) => !held.has(one.id))
      .map(availableOf)
      .sort((one, two) => one.name.localeCompare(two.name));

    const entries = [...mine, ...theirs, ...available];
    return {
      entries,
      byId: (id: string) => entries.find((entry) => entry.id === id) ?? null,
      // A declared MCP server is installed by descriptor rather than by
      // package, so `packaged === null` is not the test for one. An entry that
      // carries a transport is as much a part of what this project runs as one
      // a package answers to, and the navigator lists it beside the rest.
      installed: mine.filter(
        (entry) =>
          (entry.packaged !== null || entry.transport !== null) && entry.unrunnable === null,
      ),
    };
  }, [declaredKey, listed, packages]);
}
