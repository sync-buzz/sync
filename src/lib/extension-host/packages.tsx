"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import {
  installedExtensions,
  type InstalledExtension,
} from "@/lib/extension-host/client";

/**
 * What is unpacked on this machine, read once and shared by everything that
 * asks.
 *
 * Two different questions are easy to confuse and this answers only the first.
 * *What can this machine load* is a fact about the disk and is the same for
 * every project open in every window. *What this project is composed of* is a
 * fact about a repository and lives in the project's own record — that one is
 * `composition.ts`, and it is a list of ids that this list has to satisfy.
 *
 * Held in one place because reading it twice is how two panels come to disagree
 * about whether something is installed: the catalogue would list a package the
 * sidebar had not seen, and neither would be wrong about what it had read.
 *
 * The list is deliberately not filtered by what any project declares. A package
 * a project has not asked for is still one a person can see, describe and
 * remove, and hiding it would make "install" mean two different things
 * depending on which panel it was clicked in.
 */
export interface Packages {
  /** Everything this machine can load, in no particular order. */
  readonly all: readonly InstalledExtension[];
  /** One by id, or `null` when this machine holds nothing under that name. */
  readonly byId: (id: string) => InstalledExtension | null;
  /** True until the first read has answered. */
  readonly isLoading: boolean;
  /** Why the disk could not be read, or `null`. */
  readonly failure: string | null;
  /** Re-read the disk. Installing, removing and reloading a folder all do. */
  readonly reload: () => Promise<void>;
}

const Context = createContext<Packages | null>(null);

/**
 * Reads what is on the disk and keeps it current.
 *
 * A failure here is held rather than thrown: an artefact directory that cannot
 * be read is a window with no sections, which is a state to describe rather
 * than a crash — and the catalogue is exactly where describing it belongs.
 */
export function usePackagesState(): Packages {
  const [all, setAll] = useState<readonly InstalledExtension[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [failure, setFailure] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setAll(await installedExtensions());
      setFailure(null);
    } catch (refused) {
      setFailure(refused instanceof Error ? refused.message : String(refused));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    // Wrapped rather than called directly: reading the disk is asynchronous,
    // and an effect that sets state on its own first tick makes the first paint
    // a render nobody asked for.
    const read = async () => {
      await reload();
    };
    void read();
  }, [reload]);

  return useMemo(
    () => ({
      all,
      byId: (id: string) =>
        all.find((entry) => entry.manifest.id === id) ?? null,
      isLoading,
      failure,
      reload,
    }),
    [all, isLoading, failure, reload],
  );
}

export function PackagesProvider({
  packages,
  children,
}: {
  packages: Packages;
  children: ReactNode;
}) {
  return <Context.Provider value={packages}>{children}</Context.Provider>;
}

/** What this machine has unpacked. Throws outside a window that has read it. */
export function usePackages(): Packages {
  const value = useContext(Context);
  if (value === null) {
    throw new Error(
      "Something asked what this machine has installed from outside a project window.",
    );
  }
  return value;
}
