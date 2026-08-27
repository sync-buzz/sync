import type { InstalledExtension } from "@/lib/extension-host/client";
import type { MountedArea } from "@/lib/extension-host/areas";

/**
 * Which part of the window opens a record, and what to say when nothing does.
 *
 * A search result is a record of some kind, and the question the palette has to
 * answer before it can open one is whose record it is. The answer is the kind,
 * and only the kind: a package declares in its manifest which kinds it opens, so
 * `kind → extension` is a lookup rather than a guess. Nothing here looks at a
 * file name, a media type or a path, and that is the point — Sync did not
 * create the type and does not decide how its documents are read. An extension
 * that publishes a type of videos owns opening videos; until one is installed,
 * the honest answer is that this project cannot show them.
 *
 * Publishing a type and opening one are two contributions, not one. An
 * extension may bring a vocabulary and no screen — Project memory is exactly
 * that — and asking such an extension to open a record would answer "nothing
 * can show this" about a record every section could show. So `opens.kinds` is
 * separate from `types`, and an extension that publishes without opening simply
 * says nothing here.
 *
 * A kind nobody claims goes to whichever package declared `opens.projectTypes`.
 * That field is what retired `PROJECT_TYPES = "records"` from the shell: which
 * extension reads a project's own types used to be a constant naming one, and
 * is now a property an extension states about itself. Exactly one installed
 * extension should claim it, and what a project declares is preferred over what
 * merely happens to be unpacked — a project whose own types open in one section
 * on Monday and another on Tuesday is worse than either answer.
 *
 * What is consulted is every package on this machine rather than only the
 * declared ones, and that is what lets the palette name the extension a person
 * is missing instead of shrugging.
 */

/** An extension, at the length a sentence about a missing screen needs. */
export interface OpeningExtension {
  readonly id: string;
  readonly name: string;
}

export type Opening =
  /** The section to hand the record to, by the key the window addresses it by. */
  | { readonly outcome: "area"; readonly areaKey: string }
  /**
   * This machine holds what opens it and the project has not declared it. The
   * one state worth interrupting for: it is answerable, and the answer is one
   * click away in the section this names.
   */
  | { readonly outcome: "install"; readonly extension: OpeningExtension }
  /**
   * Nothing running in this window can show it — because nothing on this
   * machine opens that kind at all, or because what does is declared and did
   * not start.
   */
  | { readonly outcome: "unavailable"; readonly extension: OpeningExtension | null };

/** How a kind is answered for, bound to what this window is running. */
export type Opener = (kind: string) => Opening;

function named(extension: InstalledExtension): OpeningExtension {
  return { id: extension.manifest.id, name: extension.manifest.name };
}

/**
 * Binds the lookup to one window's packages and sections.
 *
 * A function rather than a table, because the answer depends on three things
 * that change independently: what is unpacked, what the project declares, and
 * which sections actually started. Everything that asks — the palette, a link
 * in a body — asks the same bound question, so two parts of the window cannot
 * disagree about who opens what.
 */
export function openers(
  packages: readonly InstalledExtension[],
  sections: readonly MountedArea[],
  /** The extension ids the project declares, in its own order. */
  installed: readonly string[],
): Opener {
  const declaredFirst = [
    ...packages.filter((entry) => installed.includes(entry.manifest.id)),
    ...packages.filter((entry) => !installed.includes(entry.manifest.id)),
  ];

  return (kind: string): Opening => {
    const owner =
      declaredFirst.find((entry) => entry.manifest.opens.kinds.includes(kind)) ??
      declaredFirst.find((entry) => entry.manifest.opens.projectTypes) ??
      null;

    if (owner === null) {
      // Nothing opens it. Whoever publishes it is still worth naming — that is
      // the difference between "install this" and "a newer Sync wrote this".
      const publisher =
        packages.find((entry) => entry.types.some((type) => type.kind === kind)) ??
        null;
      return {
        outcome: "unavailable",
        extension: publisher === null ? null : named(publisher),
      };
    }

    const section = sections.find(
      (area) => area.extensionId === owner.manifest.id,
    );
    if (section !== undefined) {
      return { outcome: "area", areaKey: section.key };
    }

    // On this machine, and not showing anything. Which of the two reasons it is
    // decides what a person can do about it.
    return installed.includes(owner.manifest.id)
      ? { outcome: "unavailable", extension: named(owner) }
      : { outcome: "install", extension: named(owner) };
  };
}
