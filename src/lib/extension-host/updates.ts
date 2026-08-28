"use client";

import { useEffect, useState } from "react";
import { gt } from "semver";

import {
  cachedRegistryIndex,
  registryLedger,
  type ListedExtension,
  type RegistryArtefact,
  type RegistryLedger,
} from "@/lib/extension-host/client";
import type { Packages } from "@/lib/extension-host/packages";
import { refuseIncompatible } from "@/lib/extension-api/version";

/**
 * What is newer than what a project is running, and whether it may be offered.
 *
 * The index is the source of truth for what exists: it carries the newest
 * version of every extension, so *whether* there is an update is answered
 * without fetching anything else. What changed in it is in the extension's own
 * ledger, which is a file per extension and is read when a page is opened.
 *
 * **Nothing updates itself.** Installing publishes type definitions into the
 * project's memory, which is a write to the repository — doing that while
 * nobody is looking is not an update, it is a commit somebody did not make. So
 * everything here answers a question and offers a button; none of it acts.
 *
 * **Two sources of the same index, and the difference is who asked.** The
 * catalogue fetches it, because opening a marketplace is somebody asking what
 * exists. The window reads what was cached, because a mark on one row is not
 * worth turning every launch into a request — see [`useCachedIndex`].
 */

/** One version, newer than what is installed, and what stands in its way. */
export interface AvailableUpdate {
  readonly id: string;
  /** What this machine is serving now. */
  readonly from: string;
  /** What the index says exists. */
  readonly to: string;
  /** Where to fetch it, which is what applying it needs. */
  readonly artefact: RegistryArtefact;
  /**
   * Why this build may not run the new version, or `null` when it may.
   *
   * The gate `docs/extensions.md` §9 asks for, and it is why an update is a
   * state rather than a button: a version whose `syncApi` range this build is
   * below is one the card names a Sync for, instead of offering a button that
   * would fail after a download. The extension is left exactly where it is.
   */
  readonly refusal: string | null;
}

/**
 * Whether an id may be offered a version from the registry at all.
 *
 * A folder is the one source that may not. It is read where it lies and its
 * files are whoever is writing them — offering to replace it with a published
 * artefact would offer to stop serving somebody's working copy from the one
 * screen that is meant to make writing an extension possible. Everything else
 * is a fixed artefact that the registry can have a newer version of, including
 * one that arrived as a file or came seeded with this build.
 */
function mayBeUpdated(source: string | undefined): boolean {
  return source !== undefined && source !== "folder";
}

/**
 * What the registry has that is newer, by id.
 *
 * Only for what the project actually declares. A package unpacked on this
 * machine and asked for by nobody is not something this project is running, so
 * a mark about it would be about somebody else's decision — and the marketplace
 * card for it says its version anyway.
 */
export function updatesFor(
  declared: readonly { readonly id: string }[],
  packages: Packages,
  listed: readonly ListedExtension[],
): ReadonlyMap<string, AvailableUpdate> {
  const inRegistry = new Map(listed.map((one) => [one.id, one]));
  const found = new Map<string, AvailableUpdate>();

  for (const { id } of declared) {
    const packaged = packages.byId(id);
    const newest = inRegistry.get(id);
    if (packaged === null || newest === undefined) continue;
    if (!mayBeUpdated(packaged.pointer.source)) continue;

    // `gt` rather than an inequality: `1.10.0` is newer than `1.9.0` and a
    // string comparison says the opposite. Both versions have been through a
    // manifest or an index that checked their shape, so neither throws here.
    if (!gt(newest.version, packaged.manifest.version)) continue;

    found.set(id, {
      id,
      from: packaged.manifest.version,
      to: newest.version,
      artefact: newest.artefact,
      refusal: refuseIncompatible({
        syncApi: newest.syncApi,
        capabilities: [...newest.capabilities],
      }),
    });
  }

  return found;
}

/**
 * What the last fetch left on the disk, read once when a project opens.
 *
 * This is the half of `docs/extensions.md` §9 that had nowhere to live: the
 * mark on the pinned Extensions row is for the person who has *not* opened the
 * catalogue, and the catalogue is where the index was being fetched. Reading
 * the cache is what squares those — a machine that has fetched an index knows
 * what it knows, and a machine that never has says nothing, which is the honest
 * state of it.
 *
 * Deliberately not a fetch. `useMarketplace` dials out because somebody opened
 * the marketplace and asked what exists; a window doing it at every launch
 * would be this application making a request on behalf of somebody who did not
 * ask, for the sake of one dot.
 *
 * A failure is silence. There is nothing a person can do about a cached file
 * that will not parse, and the answer to "is anything newer" is then the same
 * as the answer on a machine that never fetched one.
 */
export function useCachedIndex(): readonly ListedExtension[] {
  const [listed, setListed] = useState<readonly ListedExtension[]>([]);

  useEffect(() => {
    let current = true;
    void cachedRegistryIndex().then(
      (index) => {
        if (current && index !== null) setListed(index.extensions);
      },
      () => {
        // See above: nothing to say and nobody to say it to.
      },
    );
    return () => {
      current = false;
    };
  }, []);

  return listed;
}

/** Every version one extension published, as a page reads it. */
export interface Ledger {
  readonly ledger: RegistryLedger | null;
  readonly isLoading: boolean;
  /** Why there is no ledger, in the network's own words, or `null`. */
  readonly failure: string | null;
}

/**
 * The versions of one extension, fetched when its page is open.
 *
 * Per extension rather than with the index, because the index is the file every
 * window fetches and a changelog of every version of everything would make it
 * grow without limit. Cached in Rust with an `ETag` like the index, so a page
 * opened twice costs a 304.
 *
 * `null` for an id the registry does not list — a package installed from a file
 * or a folder — which is not a failure: there is no ledger to have, and the
 * page says nothing about versions rather than saying something went wrong.
 */
export function useLedger(id: string | null): Ledger {
  const [answered, setAnswered] = useState<{
    readonly id: string;
    readonly ledger: RegistryLedger | null;
    readonly failure: string | null;
  } | null>(null);

  useEffect(() => {
    if (id === null) return;
    let current = true;
    void registryLedger(id).then(
      (ledger) => {
        if (current) setAnswered({ id, ledger, failure: null });
      },
      (refused: unknown) => {
        if (current) {
          setAnswered({
            id,
            ledger: null,
            failure: refused instanceof Error ? refused.message : String(refused),
          });
        }
      },
    );
    return () => {
      current = false;
    };
  }, [id]);

  const answer = answered?.id === id ? answered : null;
  return {
    ledger: answer?.ledger ?? null,
    isLoading: id !== null && answer === null,
    failure: answer?.failure ?? null,
  };
}

/**
 * What the author said about one version, or `null` where they said nothing.
 *
 * A first release has no changelog and neither does a version published before
 * the ledger carried one, and both are answered with silence rather than with a
 * line saying there is nothing to read.
 */
export function changelogOf(ledger: RegistryLedger | null, version: string): string | null {
  const release = ledger?.versions.find((one) => one.version === version);
  const changelog = release?.changelog.trim() ?? "";
  return changelog === "" ? null : changelog;
}
