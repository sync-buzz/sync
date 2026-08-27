"use client";

import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { ExtensionRemovalSheet } from "@/components/shell/extension-removal";
import { PanelSurface } from "@/components/shell/panel";
import {
  ExtensionInspector,
  ExtensionMarketplace,
  ExtensionNavigator,
  ExtensionPage,
} from "@/components/shell/extensions-view";
import type { AreaIntent } from "@/lib/area-intent";
import { useAppMenu } from "@/lib/app-menu";
import { useCompositionContext } from "@/lib/composition";
import type { AreaModule } from "@/lib/extension-host/activate";
import { useCatalogue, type Catalogue } from "@/lib/extension-host/catalogue";
import { useMarketplace, type Marketplace } from "@/lib/extension-host/marketplace";
import { usePackages } from "@/lib/extension-host/packages";
import { useClocks, type Clocks } from "@/lib/extension-host/use-clocks";
import { updatesFor, type AvailableUpdate } from "@/lib/extension-host/updates";
import type { OpenProject } from "@/lib/project/types";

/**
 * Extensions, as an area of the window.
 *
 * The same shape an extension's area has, and deliberately so: a provider
 * holding what the area has selected, and one component per column. It is the
 * shell's own — the one section that is not brought by anything installed —
 * and it is expressed as an [`AreaModule`] so that the window mounts it through
 * exactly the path it mounts everything else through. A second path for the
 * shell's own screen is how the first path stops being tested.
 *
 * It contributes no File commands, and says so rather than staying silent: an
 * area that did not mention the menu would leave the previous area's commands
 * installed, and `⌘N` would go on offering to write a record while the window
 * is showing a marketplace.
 */

interface ExtensionsContext {
  readonly catalogue: Catalogue;
  /**
   * The extension being shown, or `null` for the marketplace.
   *
   * `null` is a destination rather than an absence: the area opens on it, it
   * has a row of its own, and there is no state in which nothing is selected.
   * The id of an extension cannot collide with it, which is the other reason
   * it is not a sentinel string — an id comes out of a stranger's manifest.
   */
  /**
   * What the registry answered, and whether it answered from the network.
   *
   * On the context rather than read again in the column that draws it: two
   * reads would be two requests, and the second would be one nobody asked for.
   */
  readonly marketplace: Marketplace;
  /**
   * What the registry has that is newer than what this project runs, by id.
   *
   * Read from the index this area already fetched rather than asked for
   * separately: whether something is newer is a comparison between two version
   * strings, and both of them are already here. What *changed* in it is the
   * extension's own ledger, and that is fetched by the page that shows it.
   */
  readonly updates: ReadonlyMap<string, AvailableUpdate>;
  /**
   * Which extensions' clocks run in this project, and the switch that stops
   * one.
   *
   * On the context because the page draws the switch and the provider is where
   * the project's path is. Nothing else in the area asks: a row in the
   * navigator says what a project runs, not when.
   */
  readonly clocks: Clocks;
  readonly selectedId: string | null;
  readonly select: (id: string | null) => void;
  /** Ask about removing one, by id, or `null` to close the confirmation. */
  readonly askRemoval: (id: string | null) => void;
}

const Context = createContext<ExtensionsContext | null>(null);

function useExtensionsArea(): ExtensionsContext {
  const value = useContext(Context);
  if (value === null) {
    throw new Error(
      "An Extensions column was rendered outside the Extensions area.",
    );
  }
  return value;
}

export function ExtensionsAreaProvider({
  project,
  active,
  intent,
  children,
}: {
  project: OpenProject;
  /** False while this area is mounted but another one is selected. */
  active: boolean;
  /**
   * What the window is asking this area to show. Search sends one here when a
   * result belongs to an extension the project has not installed: the answer to
   * "nothing opens this" is the card that installs something that does.
   */
  intent?: AreaIntent | null;
  children: ReactNode;
}) {
  // What exists anywhere. Asked here rather than higher up because this is the
  // first place that only exists once somebody has opened the catalogue: an
  // area is mounted on first visit and never unmounted, so a request made in
  // this provider is a request that person asked for.
  const marketplace = useMarketplace();
  // What there is to show: the packages on this machine, joined with what the
  // project declares and with what the registry lists. Held here rather than in
  // a column, because all three read the same list and a second read is how two
  // of them come to disagree.
  const packages = usePackages();
  const catalogue = useCatalogue(
    packages,
    project.installed,
    marketplace.listed,
  );
  // The same question the pinned row in the sidebar asks, answered here from a
  // freshly fetched index rather than from the cached one: somebody looking at
  // this column has asked what exists, and this is the answer they asked for.
  const updates = useMemo(
    () => updatesFor(project.installed, packages, marketplace.listed),
    [marketplace.listed, packages, project.installed],
  );
  // Which extension the area is showing. Selection state — this run of the
  // window, not a project fact — and `null` until somebody chooses, which is
  // resolved to the first entry below rather than guessed at here.
  const [chosenId, setChosenId] = useState<string | null>(null);
  // The last ask this area has stopped honouring, and what it is showing while
  // one still stands. Search sends people here with a card in mind; selecting
  // another one is them answering that, and the catalogue stays where they left
  // it from then on.
  const [settled, setSettled] = useState<AreaIntent | null>(null);
  const asking =
    intent && intent !== settled && intent.show === "extension" ? intent : null;
  // An ask wins, then what somebody chose, then the marketplace. A chosen id
  // that no longer names anything — the package was removed while its page was
  // open — falls through to the same place, which is why removing an extension
  // lands somebody back where they can see what they have rather than on a page
  // about something that is gone.
  const chosen =
    chosenId !== null && catalogue.byId(chosenId) !== null ? chosenId : null;
  const selectedId = asking?.id ?? chosen;
  const setSelectedId = (id: string | null) => {
    setSettled(intent ?? null);
    setChosenId(id);
  };
  // Which extensions have had their clock stopped here. Held beside the
  // catalogue rather than inside the page, so that opening one extension's page
  // and then another's does not ask the host the same question twice.
  const clocks = useClocks(project.path);
  // Which extension a removal has been asked about. Removing destroys nothing,
  // and the sheet is there to say so with a number rather than to warn.
  const [removing, setRemoving] = useState<string | null>(null);
  const composition = useCompositionContext();

  // Nothing here writes a record or names a type. Stating that is what takes
  // the previous area's commands away.
  useAppMenu(
    {
      selected: null,
      createRecord: null,
      createType: null,
      table: null,
    },
    active,
  );

  return (
    <Context.Provider
      value={{
        catalogue,
        marketplace,
        updates,
        clocks,
        selectedId,
        select: setSelectedId,
        askRemoval: setRemoving,
      }}
    >
      {children}

      <ExtensionRemovalSheet
        open={removing !== null}
        onOpenChange={(isOpen) => setRemoving(isOpen ? removing : null)}
        extension={removing === null ? null : catalogue.byId(removing)}
        countRecords={composition.countRecords}
        onRemove={composition.remove}
      />
    </Context.Provider>
  );
}

export function ExtensionsNavigator() {
  const area = useExtensionsArea();

  return (
    <ExtensionNavigator
      installed={area.catalogue.installed}
      selectedId={area.selectedId}
      onSelect={area.select}
    />
  );
}

/**
 * The workspace: the marketplace, or one extension.
 *
 * There is no empty state. A machine with nothing unpacked and a project
 * declaring nothing is where everybody starts, and what it opens on is the
 * marketplace saying so — which is a screen with something to do on it, rather
 * than the blank column this used to draw while the only way to fill it was two
 * panels away.
 */
export function ExtensionsWorkspace() {
  const area = useExtensionsArea();
  const composition = useCompositionContext();
  const entry =
    area.selectedId === null ? null : area.catalogue.byId(area.selectedId);

  if (entry === null) {
    return (
      <ExtensionMarketplace
        entries={area.catalogue.entries}
        marketplace={area.marketplace}
        updates={area.updates}
        onOpen={area.select}
      />
    );
  }

  return (
    <ExtensionPage
      entry={entry}
      update={area.updates.get(entry.id) ?? null}
      // Moving to another published version, which is the same operation
      // whichever direction the number went.
      onChange={(artefact) => void composition.change(entry.id, artefact)}
      // The artefact only when this machine has not got the package: an entry
      // answered by something already unpacked is declared from that, whatever
      // the registry also says about it.
      onInstall={() =>
        void composition.install(
          entry.id,
          entry.packaged === null ? (entry.listed?.artefact ?? undefined) : undefined,
        )
      }
      onRemove={() => area.askRemoval(entry.id)}
      isBusy={composition.isBusy}
      failure={composition.failure}
      onDismissFailure={composition.dismissFailure}
      // A switch only where there is a clock to stop: the package is here, it
      // asks for one, and this project is the one running it. A card for
      // something nobody has installed still says that it runs on a clock —
      // that is what somebody is deciding about — and carries no control over a
      // schedule that is running nowhere.
      clock={
        entry.declared && (entry.packaged?.manifest.schedule.length ?? 0) > 0
          ? {
              isOn: area.clocks.isOn(entry.id),
              isBusy: area.clocks.isBusy,
              onChange: (on: boolean) => area.clocks.switchTo(entry.id, on),
            }
          : null
      }
    />
  );
}

/**
 * The inspector, which is the package rather than the product.
 *
 * Empty while the marketplace is open, and that is the honest answer: the
 * marketplace is about a set, and this column describes one thing. Filling it
 * with a summary of the set would give the same subject two columns and leave
 * the person nothing to compare.
 */
export function ExtensionsInspector() {
  const area = useExtensionsArea();
  const entry =
    area.selectedId === null ? null : area.catalogue.byId(area.selectedId);

  if (entry === null) return <PanelSurface className="bg-panel">{null}</PanelSurface>;

  return (
    <ExtensionInspector
      entry={entry}
      onRemove={() => area.askRemoval(entry.id)}
    />
  );
}

/**
 * The shell's own area, in the shape the window mounts an extension's in.
 *
 * The window holds one table of areas and this is an entry in it. It is built
 * here rather than in the window because what fills a frame's columns is the
 * area's business, and the window is the one file that must not know what any
 * of them contain.
 */
export const EXTENSIONS_AREA_MODULE: AreaModule = {
  Provider: ExtensionsAreaProvider,
  Navigator: ExtensionsNavigator,
  Workspace: ExtensionsWorkspace,
  Inspector: ExtensionsInspector,
};
