"use client";

import * as React from "react";

import * as api from "@/lib/extension-api";
import type {
  ActivationResult,
  AreaModule,
  AreaProviderProps,
  ExtensionHost,
} from "@/lib/extension-api/contract";
import { netFor } from "@/lib/extension-api/net";
import { refuseIncompatible } from "@/lib/extension-api/version";
import type {
  InstalledExtension,
  ManifestArea,
  ManifestBadge,
} from "@/lib/extension-host/client";
import { FRAMES, type FrameId } from "@/lib/shell-frames";

export type { ActivationResult, AreaModule, AreaProviderProps, ExtensionHost };

/**
 * Running an extension's code, and the four refusals before that happens.
 *
 * Order is the design. Each check is cheaper and more informative than the one
 * after it, and by the time anything is executed the only remaining failure is
 * the extension's own:
 *
 * 1. **Is it for this build?** `engines.syncApi` against `SYNC_API_VERSION`, and
 *    the capabilities against what this build publishes. Refused here, nothing
 *    runs — which is the point of putting the check before the import rather
 *    than around the first render.
 * 2. **Does it name shapes the shell draws?** A frame outside the closed set is
 *    a manifest written against a different host.
 * 3. **Did the module load?** A refusal by the policy or by CORS arrives as a
 *    `TypeError` naming neither, so the message is kept whole.
 * 4. **Does it fill the slots its frames asked for?** An area that returns an
 *    inspector for a `list` frame is refused rather than quietly trimmed. A
 *    panel that is empty because a component was dropped without a word is an
 *    hour spent looking for the wrong bug.
 */

/**
 * Where a module finds the window's objects while it is being evaluated.
 *
 * `activate(host)` is handed the same things, and it is not enough on its own:
 * an author writes `import { useState } from "react"` and
 * `import { Button } from "@sync-buzz/extension-api"`, and those imports are
 * resolved while the module is loading — before anything has been called. An
 * extension is built with both marked external and pointed at this object, so
 * it has to exist by the time the module is fetched.
 *
 * On the global rather than passed, because there is no way to pass anything to
 * a module during its own evaluation. It is written once and never changes:
 * every extension in a window is handed the same React and the same surface,
 * and the only thing that differs between them — which extension this is —
 * arrives as the argument to `activate`, where it can be.
 */
export interface HostRuntime {
  readonly React: typeof React;
  readonly api: typeof api;
}

/**
 * The name the built module looks the runtime up under.
 *
 * Spelled out here and in the extension build's shim, which is two places — and
 * they are two places on purpose: the shim is generated in another repository
 * against a published contract, so the name is part of that contract rather
 * than an implementation detail either side may change.
 */
const RUNTIME = "__syncExtensionHost__";

interface HostGlobal {
  [RUNTIME]?: HostRuntime;
}

/** Publishes the runtime, once, before any module is fetched. */
function publishRuntime(): void {
  const global = globalThis as HostGlobal;
  global[RUNTIME] ??= { React, api };
}

interface ExtensionModule {
  readonly default: (host: ExtensionHost) => ActivationResult | AreaModule;
}

/** One area, ready for the window to mount. */
export interface LoadedArea {
  readonly extensionId: string;
  readonly areaId: string;
  readonly label: string;
  readonly description: string;
  readonly frame: FrameId;
  readonly icon: string | null;
  /**
   * The count the host draws on this section's row, or `null` for none.
   *
   * Carried through untouched, freshness states included. Which words name a
   * state is the engine's — `Freshness` is deliberately open, because a newer
   * engine may derive one this build has no mark for — so a host that refused
   * an unfamiliar state here would be this build having an opinion about the
   * engine's vocabulary. A query the engine will not answer costs the section
   * its badge and nothing else.
   */
  readonly badge: ManifestBadge | null;
  readonly module: AreaModule;
}

export interface Activation {
  readonly extension: InstalledExtension;
  readonly areas: readonly LoadedArea[];
}

/** Why an extension is not running, in a sentence a person can act on. */
export class ActivationFailure extends Error {
  constructor(
    readonly extensionId: string,
    message: string,
  ) {
    super(message);
    this.name = "ActivationFailure";
  }
}

function isFrame(candidate: string): candidate is FrameId {
  return candidate in FRAMES;
}

/**
 * What a frame asks for, and what supplying anything else means.
 *
 * The workspace is required by every frame — that is the shell's rule, not this
 * function's. The other two are checked in both directions: a missing one leaves
 * a column the window would draw empty, and an extra one is code that will never
 * be rendered and whose author believes it will.
 */
function refuseMismatchedSlots(
  area: ManifestArea,
  frame: FrameId,
  module: AreaModule,
): string | null {
  const shape = FRAMES[frame];
  const named = `"${area.id}"`;

  if (typeof module.Workspace !== "function") {
    return `The area ${named} returned no Workspace, and every frame has one.`;
  }
  if (shape.navigator && typeof module.Navigator !== "function") {
    return `The area ${named} declares the "${frame}" frame, which has a navigator, and returned none.`;
  }
  if (!shape.navigator && module.Navigator !== undefined) {
    return `The area ${named} returned a Navigator, and the "${frame}" frame has no such column.`;
  }
  if (shape.inspector && typeof module.Inspector !== "function") {
    return `The area ${named} declares the "${frame}" frame, which has an inspector, and returned none.`;
  }
  if (!shape.inspector && module.Inspector !== undefined) {
    return `The area ${named} returned an Inspector, and the "${frame}" frame has no such column.`;
  }
  return null;
}

/**
 * Whether this build can run it at all, without loading anything.
 *
 * Separate from [`activate`] because the catalogue asks it of packages it is
 * only describing: a card says *needs a newer Sync* by asking this, and asks it
 * of things it has no intention of running.
 */
export function refuseUnrunnable(extension: InstalledExtension): string | null {
  // Asked first, because it is the one refusal that is already a sentence: the
  // package was read and something in it was not.
  if (extension.defect !== null) return extension.defect;

  const incompatible = refuseIncompatible({
    syncApi: extension.manifest.engines.syncApi,
    capabilities: extension.manifest.capabilities,
  });
  if (incompatible !== null) return incompatible;

  for (const area of extension.manifest.areas) {
    if (!isFrame(area.frame)) {
      return `The area "${area.id}" asks for a "${area.frame}" frame, and this build draws ${Object.keys(FRAMES).join(", ")}.`;
    }
  }
  return null;
}

/**
 * Loads an extension's module and returns its areas, or throws with the reason.
 *
 * The dynamic import carries both bundlers' ignore comments and they are
 * load-bearing: the URL is built by Rust at runtime and there is nothing on
 * disk for a build to resolve. Turbopack in particular will try, and fail, and
 * the failure will read as though the extension were at fault.
 */
/**
 * Adds a package's own stylesheet to the document, once.
 *
 * **Why a package has one at all.** Tailwind generates the rules it finds in
 * the source files it is told to read, and the window's build reads the
 * window's own `src`. An extension is not in it, so every utility it used that
 * the shell did not happen to use as well produced no rule — silently. The
 * section still mounted, still held its state and still answered the keyboard;
 * it was simply drawn without any of its own spacing, sizing or alignment. That
 * reads as a redesign, and it was an empty stylesheet. Chat lost its
 * proportions this way for a fortnight and the cause was invisible in every
 * file anybody thought to open.
 *
 * What it does **not** carry is a value. The rules refer to variables the
 * window defines on `:root`, so there is one design system in one place, and
 * retinting the window retints every extension in it without one of them being
 * rebuilt. That is the same division the module build makes: one copy of
 * anything with identity or a design in it, its own copy of anything that is a
 * pure rule.
 *
 * Keyed by URL, which already carries the version and the artefact digest, so
 * an update brings its own sheet and a reload of the same package does not add
 * a second. Nothing removes them: a stylesheet whose extension is no longer
 * mounted styles nothing, and taking one away while a frozen area still holds
 * its markup would undress a section a person is about to return to.
 */
const adopted = new Set<string>();

function adoptStyles(extension: InstalledExtension): Promise<void> {
  const href = extension.styles;
  if (href === null || adopted.has(href)) return Promise.resolve();
  adopted.add(href);

  return new Promise((settle) => {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = href;
    link.dataset.extension = extension.manifest.id;
    // Resolved either way. A stylesheet that will not load is a section drawn
    // without its own rules, which is worth seeing and reporting; refusing the
    // activation over it would take away a working section because its spacing
    // did not arrive.
    link.addEventListener("load", () => settle());
    link.addEventListener("error", () => {
      adopted.delete(href);
      console.warn(`The stylesheet of ${extension.manifest.id} did not load.`, href);
      settle();
    });
    document.head.append(link);
  });
}

export async function activate(extension: InstalledExtension): Promise<Activation> {
  const { id } = extension.manifest;

  const unrunnable = refuseUnrunnable(extension);
  if (unrunnable !== null) throw new ActivationFailure(id, unrunnable);

  // A package with no module is not a failure and not an oddity: an extension
  // that publishes a vocabulary and a prompt has nothing to run, and its types
  // reach the project without a line of its code being executed. The manifest
  // has already refused the other half of this — sections with nothing to draw
  // them — so an absent module means an absent area list.
  if (extension.ui === null) return { extension, areas: [] };

  // Before the fetch, not after it: the module's own imports of `react` and the
  // surface are resolved while it is being evaluated, which is over by the time
  // anything below this line runs.
  publishRuntime();

  // And before the module too, so the first frame it draws is already styled.
  // A stylesheet that arrived after the module would show one repaint of every
  // section on first visit, which is the one moment a person is looking at it.
  await adoptStyles(extension);

  let loaded: ExtensionModule;
  try {
    loaded = (await import(
      /* webpackIgnore: true */ /* turbopackIgnore: true */ /* @vite-ignore */
      extension.ui
    )) as ExtensionModule;
  } catch (refused) {
    throw new ActivationFailure(
      id,
      `Its code could not be loaded: ${refused instanceof Error ? refused.message : String(refused)}`,
    );
  }

  if (typeof loaded.default !== "function") {
    throw new ActivationFailure(
      id,
      "Its module exports no default function, which is what the host calls to start it.",
    );
  }

  let produced: ActivationResult | AreaModule;
  try {
    produced = loaded.default({ id, net: netFor(id) });
  } catch (threw) {
    throw new ActivationFailure(
      id,
      `It threw while starting: ${threw instanceof Error ? threw.message : String(threw)}`,
    );
  }

  // One area is the common case, and making it look like the general one costs
  // an author a wrapper object for no reason. A module that returns a Workspace
  // directly is read as the single area the manifest declared.
  const single = extension.manifest.areas.length === 1 && "Workspace" in produced;
  const byArea: ActivationResult = single
    ? { [extension.manifest.areas[0].id]: produced as AreaModule }
    : (produced as ActivationResult);

  const areas: LoadedArea[] = [];
  for (const area of extension.manifest.areas) {
    const produced_area = byArea[area.id];
    if (produced_area === undefined) {
      throw new ActivationFailure(
        id,
        `It declares the area "${area.id}" and returned nothing for it.`,
      );
    }
    if (!isFrame(area.frame)) {
      throw new ActivationFailure(id, `The area "${area.id}" asks for an unknown frame.`);
    }
    const mismatch = refuseMismatchedSlots(area, area.frame, produced_area);
    if (mismatch !== null) throw new ActivationFailure(id, mismatch);

    areas.push({
      extensionId: id,
      areaId: area.id,
      label: area.label,
      description: area.description,
      frame: area.frame,
      icon: area.icon,
      badge: area.badge,
      module: produced_area,
    });
  }

  return { extension, areas };
}
