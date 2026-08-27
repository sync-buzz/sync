"use client";

import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { activate } from "@/lib/extension-host/activate";
import {
  forgetExtension,
  installExtensionFile,
  installExtensionFolder,
  type InstalledExtension,
} from "@/lib/extension-host/client";
import { usePackages } from "@/lib/extension-host/packages";
import { cn } from "@/lib/utils";

/**
 * What this machine has unpacked, and what this project does with it.
 *
 * Two verbs live here and they are different, which is why nothing in the
 * window ever offers one button called Install:
 *
 * - **The machine.** A package is unpacked into the artefact directory, shared
 *   by every project. Adding one is a fact about the disk.
 * - **The project.** The project's own record declares an id and a version,
 *   which travels with the repository. That is what publishes the vocabulary
 *   and what a colleague who clones the folder resolves against their own disk.
 *
 * Collapsing them would make removing an extension from one project delete it
 * for every other, and make a package somebody is writing look installed
 * everywhere the moment it was unpacked once.
 *
 * This file used to draw a panel of its own, listing every unpacked package
 * under the inspector's facts. That panel is gone: the marketplace lays the
 * same packages out as cards, which is where somebody is already deciding
 * about them, and a second list of the same thing two columns apart was two
 * places to keep true. What is left is the parts both columns share — how a
 * package gets onto the machine, how it leaves, whether it runs, and how it is
 * described in one line.
 */

/** `null` while running, a sentence when it will not run, `""` when it does. */
export type Outcome = string | null;

/**
 * Adding a package to this machine, as the two sources there are.
 *
 * A hook rather than a component because the two halves of it belong in
 * different bands of a column: the controls sit in the footer, where macOS puts
 * what acts on a list, and a refusal has to be read where there is room to read
 * it. A component would have had to own both, and a column cannot nest one.
 *
 * The refusal matters more than it looks. Installing from a folder is the loop
 * an author works in — edit, reload, look — and the ordinary failure is their
 * own manifest. Swallowing it would send them to the console for a sentence the
 * window already has.
 */
export function useAddPackage(): {
  readonly add: (from: "file" | "folder") => Promise<void>;
  readonly busy: boolean;
  readonly failure: string | null;
} {
  const packages = usePackages();
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const add = useCallback(
    async (from: "file" | "folder") => {
      setBusy(true);
      setFailure(null);
      try {
        const chosen = await open(
          from === "file"
            ? {
                multiple: false,
                filters: [{ name: "Sync extension", extensions: ["syncext"] }],
              }
            : { multiple: false, directory: true },
        );
        if (typeof chosen !== "string") return;
        await (from === "file"
          ? installExtensionFile(chosen)
          : installExtensionFolder(chosen));
        await packages.reload();
      } catch (refused) {
        setFailure(refused instanceof Error ? refused.message : String(refused));
      } finally {
        setBusy(false);
      }
    },
    [packages],
  );

  return { add, busy, failure };
}

/**
 * Taking a package off this machine, which is not taking it out of a project.
 *
 * The artefact stays where it is; what goes is the pointer that made the id
 * resolve. Re-installing is therefore free and an update is reversible, and
 * that is why this is a separate verb from the project's own declaration
 * rather than a stronger version of it.
 */
export function useForgetPackage(): {
  readonly forget: (id: string) => Promise<void>;
  readonly busy: boolean;
  readonly failure: string | null;
} {
  const packages = usePackages();
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const forget = useCallback(
    async (id: string) => {
      setBusy(true);
      setFailure(null);
      try {
        await forgetExtension(id);
        await packages.reload();
      } catch (refused) {
        setFailure(refused instanceof Error ? refused.message : String(refused));
      } finally {
        setBusy(false);
      }
    },
    [packages],
  );

  return { forget, busy, failure };
}

/**
 * Whether each unpacked package actually runs, by id.
 *
 * Every package is activated as soon as it is listed, and the outcome is shown
 * rather than hidden: a package that unpacks and cannot run looks identical to
 * one that works until somebody opens its section, and the gap between those
 * two moments is where an author loses an afternoon. It is also the only place
 * a whole class of failure is ever seen — a module the webview refuses says
 * nothing anywhere else in the window.
 *
 * One package at a time, and a failure is that one's rather than the list's:
 * one package that cannot start must not hide the others.
 */
export function useActivationOutcomes(
  packages: readonly InstalledExtension[],
): Readonly<Record<string, Outcome>> {
  const [outcomes, setOutcomes] = useState<Readonly<Record<string, Outcome>>>(
    {},
  );

  useEffect(() => {
    let current = true;
    void (async () => {
      for (const extension of packages) {
        let outcome = "";
        try {
          await activate(extension);
        } catch (refused) {
          outcome = refused instanceof Error ? refused.message : String(refused);
        }
        if (!current) return;
        setOutcomes((all) => ({ ...all, [extension.manifest.id]: outcome }));
      }
    })();
    return () => {
      current = false;
    };
  }, [packages]);

  return outcomes;
}

/**
 * What a package brings, in one line.
 *
 * Both halves are said because either alone is a real answer: an extension may
 * bring sections and no vocabulary, a vocabulary and no sections, or — for the
 * one that only talks to agents — neither.
 */
export function describePackage(extension: InstalledExtension): string {
  const { areas } = extension.manifest;
  const parts: string[] = [];
  if (areas.length > 0) {
    parts.push(`brings ${areas.map((area) => area.label).join(", ")}`);
  }
  if (extension.types.length > 0) {
    parts.push(
      `publishes ${extension.types.length} ${extension.types.length === 1 ? "type" : "types"}`,
    );
  }
  if (parts.length === 0 && extension.prompt !== null) {
    parts.push("speaks to agents only");
  }
  return parts.length === 0
    ? "Runs, and adds nothing yet."
    : `Runs, and ${parts.join(" and ")}.`;
}

/**
 * Where a package came from, and what is known about its provenance.
 *
 * Said wherever a package is, because these are the facts trust is decided on.
 * *development* is said here rather than beside the section in the sidebar,
 * which is where it was first put and where it was rejected on sight: a sidebar
 * row is a name and a mark at 34 px, and a word that long hanging off it crowds
 * out the one thing the row is for.
 */
export function PackageTags({ extension }: { extension: InstalledExtension }) {
  const { manifest, pointer } = extension;

  return (
    <div className="flex flex-wrap items-center gap-1.5 text-xs text-fg-tertiary">
      <Tag>{pointer.source}</Tag>
      {pointer.source === "folder" ? (
        <Tag tone="warning">development</Tag>
      ) : null}
      {pointer.signature === "absent" ? <Tag>unsigned</Tag> : null}
      {pointer.signature === "invalid" ? (
        <Tag tone="danger">bad signature</Tag>
      ) : null}
      <span className="font-mono">needs {manifest.engines.syncApi}</span>
    </div>
  );
}

function Tag({
  children,
  tone,
}: {
  children: ReactNode;
  tone?: "warning" | "danger";
}) {
  return (
    <span
      className={cn(
        "rounded-(--radius-control) bg-hover px-1.5 py-0.5",
        tone === "warning" && "text-fg-secondary",
        tone === "danger" && "text-danger",
      )}
    >
      {children}
    </span>
  );
}
