"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import type { LucideIcon } from "lucide-react";

import { kindIcon } from "@/components/shell/entity-marks";
import {
  ActivationFailure,
  activate,
  unavailableFor,
  type AreaModule,
  type LoadedArea,
} from "@/lib/extension-host/activate";
import type { ManifestBadge } from "@/lib/extension-host/client";
import type { Packages } from "@/lib/extension-host/packages";
import type { OpenProject } from "@/lib/project/types";
import type { FrameId } from "@/lib/shell-frames";
import { said } from "@/lib/refusal";

/**
 * The sections a project has, built by running what it declared.
 *
 * This replaces the constant the shell used to hold. An area is no longer
 * something the build knows about and looks up — it is what a manifest declared
 * and a module returned, so the window can draw a section it has never heard
 * of and cannot draw one nobody installed.
 *
 * **Order is the project's, until somebody arranges it.** Sections come out of
 * here in the order the project's own record declares its extensions — and,
 * within one extension, in the order its manifest lists its areas — so two
 * people who opened the same repository start from the same column. Where they
 * put the rows afterwards is theirs and stays on their own machine, which is
 * `useSectionOrder` in `src/lib/project/use-section-order.ts` and not anything
 * this module knows about: what the sections *are* is the project's answer, and
 * where they sit is a person's.
 *
 * Activation is asynchronous and the window is drawn before it finishes, so
 * there is a moment with no sections. That moment is honest — the disk is being
 * read and modules are being fetched — and it is the reason [`Areas.isLoading`]
 * exists: choosing which section to open before knowing what there is would
 * open the catalogue and then jump.
 */

/** One section of the window, ready to mount. */
export interface MountedArea {
  /**
   * `<extension>/<area>`, and unique in a window.
   *
   * Composite because an area id is only unique inside its extension: two
   * extensions may both call a section `browse`, and neither is wrong. The
   * window addresses sections by this, so nothing above here needs to know it
   * is made of two parts.
   */
  readonly key: string;
  readonly extensionId: string;
  readonly label: string;
  readonly description: string;
  readonly frame: FrameId;
  /** Resolved from the name the manifest gave; neutral when it names none. */
  readonly icon: LucideIcon;
  /** The package is being read from a working folder rather than an artefact. */
  readonly development: boolean;
  /**
   * The count this section asked the host to draw on its row, or `null`.
   *
   * A question rather than an answer: what it selects is here, and how many
   * there are is `useDeclaredBadges`, because the corpus changes on its own
   * clock and the manifest does not change at all.
   */
  readonly badge: ManifestBadge | null;
  readonly module: AreaModule;
}

/** An extension the project declared and this window is not running. */
export interface AreaFailure {
  readonly extensionId: string;
  readonly name: string;
  readonly message: string;
}

/**
 * A section this project has and this machine has nothing to run.
 *
 * Everything a row needs and no module, because there is no module: the
 * manifest is read, the package is not started, and what is drawn is a name a
 * person recognises with the reason it cannot be opened. That is the whole
 * point of carrying it rather than dropping it — a project's sections are the
 * same everywhere it is open, and one that silently had fewer of them on a
 * phone would read as a project that had lost something.
 */
export interface UnavailableArea {
  /** `<extension>/<area>`, as a mounted one is keyed. */
  readonly key: string;
  readonly extensionId: string;
  readonly label: string;
  /** Resolved from the name the manifest gave; neutral when it names none. */
  readonly icon: LucideIcon;
  /** Why this machine cannot open it, in a sentence to show beside the name. */
  readonly reason: string;
}

export interface Areas {
  /** In the order the project declares its extensions. */
  readonly sections: readonly MountedArea[];
  /** What the project declared and this window could not run, with reasons. */
  readonly failures: readonly AreaFailure[];
  /**
   * What the project declared, correctly, and this machine cannot show.
   *
   * Empty on a computer, which is why nothing on the Mac reads it: every
   * capability the surface names is kept there. It is the phone's list, and it
   * is built here rather than in the phone's own window because what a project
   * has is one question with one answer, whatever is drawing it.
   */
  readonly unavailable: readonly UnavailableArea[];
  /** True until every declared extension has been tried. */
  readonly isLoading: boolean;
}

const NOTHING: Areas = {
  sections: [],
  failures: [],
  unavailable: [],
  isLoading: true,
};

/**
 * What identifies one run of one package.
 *
 * An activation is cached against this, and the cache is what keeps a section
 * mounted: calling a module's `activate` again returns different component
 * objects, React sees a different type, and the whole area is torn down and
 * rebuilt — losing exactly the state this window's mounting rules exist to
 * keep. The version is in the key because an update is a different package, and
 * the module URL is in it because a folder's contents can change under a
 * version that did not — the URL carries the served file's modification time,
 * which is what makes that true rather than merely intended.
 */
function runOf(id: string, version: string, ui: string | null): string {
  return `${id}@${version}#${ui ?? ""}`;
}

export function useAreas(project: OpenProject, packages: Packages): Areas {
  const [areas, setAreas] = useState<Areas>(NOTHING);

  // Activations already made, by run. Held in a ref rather than in state
  // because it is a cache and not something to draw: writing to it must not
  // schedule a render, and reading it must give the same components back.
  const activations = useRef(
    new Map<string, readonly LoadedArea[] | ActivationFailure>(),
  );

  const declared = project.installed.map((entry) => entry.id).join();

  useEffect(() => {
    let current = true;

    const run = async () => {
      const sections: MountedArea[] = [];
      const failures: AreaFailure[] = [];
      const unavailable: UnavailableArea[] = [];

      for (const id of declared === "" ? [] : declared.split(",")) {
        const packaged = packages.byId(id);
        if (packaged === null) {
          // Declared by the project and absent from this machine. Not a
          // failure to shout about here: the catalogue is where a person is
          // told what their project asks for and what they have.
          continue;
        }

        const { manifest, pointer } = packaged;

        // Asked before the module is fetched rather than caught after it: a
        // package whose calls have nothing behind them on this machine would
        // load, mount, draw and fail at the first thing a person did with it.
        // The rows it would have brought are kept, named and unopenable.
        const elsewhere = unavailableFor(packaged);
        if (elsewhere !== null) {
          for (const area of manifest.areas) {
            unavailable.push({
              key: `${manifest.id}/${area.id}`,
              extensionId: manifest.id,
              label: area.label,
              icon: kindIcon(area.icon),
              reason: elsewhere,
            });
          }
          continue;
        }

        const key = runOf(manifest.id, manifest.version, packaged.ui);
        let outcome = activations.current.get(key);

        if (outcome === undefined) {
          try {
            outcome = (await activate(packaged)).areas;
            activations.current.set(key, outcome);
          } catch (refused) {
            outcome =
              refused instanceof ActivationFailure
                ? refused
                : new ActivationFailure(id, said(refused));
            activations.current.set(key, outcome);
          }
        }

        if (!current) return;

        if (outcome instanceof ActivationFailure) {
          failures.push({
            extensionId: id,
            name: manifest.name,
            message: outcome.message,
          });
          continue;
        }

        for (const area of outcome) {
          sections.push({
            key: `${area.extensionId}/${area.areaId}`,
            extensionId: area.extensionId,
            label: area.label,
            description: area.description,
            frame: area.frame,
            icon: kindIcon(area.icon),
            development: pointer.source === "folder",
            badge: area.badge,
            module: area.module,
          });
        }
      }

      if (current) setAreas({ sections, failures, unavailable, isLoading: false });
    };

    void run();
    return () => {
      current = false;
    };
  }, [declared, packages]);

  // The packages are still being read, so nothing has been decided yet.
  return useMemo(
    () => (packages.isLoading ? NOTHING : areas),
    [areas, packages.isLoading],
  );
}
