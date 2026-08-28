"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { AppHeader } from "@/components/shell/app-header";
import { EXTENSIONS_AREA } from "@/components/shell/areas";
import { EXTENSIONS_AREA_MODULE } from "@/components/shell/extensions-area";
import { openers } from "@/components/shell/opening";
import { PrimarySidebar } from "@/components/shell/primary-sidebar";
import type { ProjectSetup } from "@/components/shell/project-setup";
import { RecordLinks } from "@/components/shell/record-links";
import { SearchPalette } from "@/components/shell/search-palette";
import { SyncSheet } from "@/components/shell/sync-sheet";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { useMemoryMenu } from "@/lib/app-menu";
import type { AreaIntent } from "@/lib/area-intent";
import { CompositionProvider, useComposition } from "@/lib/composition";
import type { AreaModule } from "@/lib/extension-host/activate";
import { useAreas, type MountedArea } from "@/lib/extension-host/areas";
import { useSectionOrder } from "@/lib/project/use-section-order";
import { BadgeScope } from "@/lib/extension-api/badge";
import {
  mergeBadges,
  useDeclaredBadges,
  useLiveBadges,
} from "@/lib/extension-host/badges";
import {
  PackagesProvider,
  usePackagesState,
} from "@/lib/extension-host/packages";
import { updatesFor, useCachedIndex } from "@/lib/extension-host/updates";
import { useSyncState } from "@/lib/memory/use-sync-state";
import type { OpenProject } from "@/lib/project/types";
import { FRAMES } from "@/lib/shell-frames";
import {
  PANEL_GEOMETRY,
  PANEL_IDS,
  WORKSPACE_MIN_WIDTH,
  useShellLayout,
  type CollapsiblePanelRole,
} from "@/lib/shell-layout";
import { cn } from "@/lib/utils";

/**
 * The window with a project open.
 *
 * It knows three things and no more: which area is selected, which areas have
 * been visited, and where the columns are. What is *in* the columns is the
 * area's, and this file cannot name a single thing an area shows — no type, no
 * record, no catalogue entry. An area arriving from an extension is one this
 * file has never heard of, so anything it knew about one of them would be a
 * thing the next one could not have.
 *
 * **An area is never unmounted.** It is mounted the first time it is selected
 * and hidden from then on. That is what makes leaving and coming back cost
 * nothing: the selection, the open record, the caret and the scroll position
 * are all still there because nothing tore them down. Restoring them instead
 * would mean every extension had to implement restoring them, and the ones that
 * forgot would look like our bug.
 *
 * The two costs of that are paid rather than ignored. An area not selected is
 * told so and freezes — no reads, no scans, no menu — and an area never visited
 * is never mounted at all, so what is installed costs nothing until it is
 * opened.
 *
 * The frame the selected area declared decides which columns it can use. A
 * column its frame does not have is held closed rather than removed: a hidden
 * area has to keep the DOM it built, and taking the panel out of the tree would
 * take that with it.
 */
export function ProjectWindow({
  project,
  setup,
  onProjectChanged,
}: {
  project: OpenProject;
  setup: ProjectSetup;
  /** Installing or removing an extension changes what the project is. */
  onProjectChanged: (project: OpenProject) => void;
}) {
  // What this machine has unpacked, read once for the whole window. A
  // declaration in the project's record resolves against this list, and both
  // the catalogue and the sections read the same copy of it.
  const packages = usePackagesState();
  const composition = useComposition(project, packages, onProjectChanged);

  // The sections this project has, which is what running its packages produced.
  // Nothing in this file decides what they are, and nothing in it could: the
  // catalogue at the foot of the column is the only area the window owns.
  const { sections: brought, isLoading } = useAreas(project, packages);
  // And the order somebody put them in, which is this Mac's business rather
  // than the project's: the declaration decides what the sections are, and a
  // person decides where they sit. Applied here rather than in the sidebar so
  // that the section which opens by default is the one at the top of the
  // column — the first row is what a person means by "first".
  const { sections, arrange } = useSectionOrder(project.path, brought);
  const mounted = useMemo(
    () => new Map([...sections, CATALOGUE].map((area) => [area.key, area])),
    [sections],
  );

  // Which section somebody chose, and `null` until somebody has.
  //
  // Nothing is chosen while the packages are being read and their modules run,
  // and that is deliberate: opening the catalogue and then jumping to a section
  // as it arrives is two windows in the first second, and choosing a section
  // before knowing there are none is choosing one that does not exist.
  const [chosen, setChosen] = useState<string | null>(null);
  const activeKey =
    chosen !== null && mounted.has(chosen)
      ? chosen
      : isLoading
        ? null
        : (sections[0]?.key ?? CATALOGUE.key);

  // In order of first visit rather than of selection: the providers are nested
  // in this order, and reordering them would unmount everything below the one
  // that moved — which is the one thing this whole arrangement exists to avoid.
  const [visited, setVisited] = useState<readonly string[]>([]);

  // Recorded during the render that shows it rather than in an effect after it.
  // The area has to be in the list by the time the layers below are built, and
  // an effect runs after they have been built once — which would mount the
  // first section one render late, with the panels empty in between.
  if (activeKey !== null && !visited.includes(activeKey)) {
    setVisited([...visited, activeKey]);
  }

  const selectArea = useCallback((key: string) => {
    setChosen(key);
  }, []);

  // Whether the palette is up, and what the last thing asked of an area was.
  //
  // The ask is held rather than fired and forgotten, because the area it is
  // addressed to may not be mounted yet: selecting it and handing it the intent
  // happen in the same commit, and a provider mounting with an intent already
  // in its props is the case that has to work. It is kept afterwards for the
  // same reason it is safe to — an area applies an object once, so holding the
  // last one costs nothing and re-selecting an area does not re-open anything.
  const [searching, setSearching] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);

  // The nodes an area draws its columns into, one per column of the frame.
  //
  // Held as state rather than as refs because a portal can only be rendered
  // once its node exists: the first render has no panels yet, and it is the
  // node arriving that has to bring the columns with it.
  const [slots, setSlots] = useState<AreaSlots>(EMPTY_SLOTS);
  const slotRefs = useMemo(() => {
    const attach = (column: AreaColumn) => (element: HTMLDivElement | null) =>
      setSlots((current) =>
        current[column] === element
          ? current
          : { ...current, [column]: element },
      );
    return {
      Navigator: attach("Navigator"),
      Workspace: attach("Workspace"),
      Inspector: attach("Inspector"),
    };
  }, []);
  const [asked, setAsked] = useState<{
    readonly areaKey: string;
    readonly intent: AreaIntent;
  } | null>(null);

  const show = useCallback(
    (areaKey: string, intent: AreaIntent) => {
      selectArea(areaKey);
      setAsked({ areaKey, intent });
    },
    [selectArea],
  );

  // Search is the one command in the title bar, so it is the one shortcut the
  // window claims for itself. Bound here rather than in the menu bar: the menu
  // belongs to whichever area is selected, and this belongs to none of them.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k")
        return;
      // Something nearer the caret has already answered this — the editor's own
      // `⌘K` makes a link out of selected words. The window listens last, so
      // the way to defer to it is to notice that it acted: opening the palette
      // on top of a panel that just appeared would be two answers to one key.
      if (event.defaultPrevented) return;
      event.preventDefault();
      setSearching(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // What the project declares can change under the window — removing the
  // extension whose area is open is the ordinary case — so a selection that no
  // longer names anything falls back the same way the first one is decided.
  const frame = FRAMES[(activeKey === null ? undefined : mounted.get(activeKey))?.frame ?? CATALOGUE.frame];

  // Which section opens which kind, bound to what this window is actually
  // running. Built here because it is the one place that holds all three
  // answers at once: what is unpacked, what the project declares, and what
  // started.
  const opener = useMemo(
    () => openers(packages.all, sections, composition.installed),
    [composition.installed, packages.all, sections],
  );

  // What each section has that is worth a look, from the two sources there are.
  // The counting needs the opener above it — a badge naming no kinds counts
  // what its own section opens, which is the lookup the palette already asks —
  // and the reports need to be held above the areas that make them and the
  // column that draws them, which is here and nowhere else.
  const declared = useDeclaredBadges(project.path, sections, opener);
  const live = useLiveBadges();
  const badges = useMemo(
    () => mergeBadges(declared, live.reported),
    [declared, live.reported],
  );

  // Whether anything this project runs has a newer version published. Read from
  // what the last fetch cached rather than fetched here: a mark on one row is
  // not worth turning every launch into a request, and the catalogue is where
  // somebody asks what exists.
  //
  // Only what could actually be moved to counts. A newer version this build is
  // too old to run is a sentence on that extension's card, said once — a dot
  // for it would stand until somebody updated the application, and a mark that
  // is permanently on is not news.
  const listed = useCachedIndex();
  const updates = useMemo(
    () =>
      [...updatesFor(project.installed, packages, listed).values()].filter(
        (update) => update.refusal === null,
      ).length,
    [listed, packages, project.installed],
  );

  const unavailable = useMemo(() => {
    const roles: CollapsiblePanelRole[] = [];
    if (!frame.navigator) roles.push("contextNavigator");
    if (!frame.inspector) roles.push("contextInspector");
    return roles;
  }, [frame]);

  const {
    groupRef,
    groupElementRef,
    panelRefs,
    stages,
    intended,
    collapsed,
    canOpen,
    reportPanelSize,
    togglePanel,
  } = useShellLayout(unavailable);

  // Read once the project is open and again when the window is returned to.
  // It scopes every column at once, so it belongs to the window rather than
  // to any area inside it.
  const sync = useSyncState(project.path);

  // The window's own, not any area's: synchronisation is true of the whole
  // project whichever area is showing.
  useMemoryMenu(
    useMemo(
      () => ({
        fetch: sync.transport?.remoteConfigured ? sync.fetchNow : null,
        publish: sync.transport?.remoteConfigured ? sync.publishNow : null,
        busy: sync.busy !== null,
      }),
      [sync.busy, sync.fetchNow, sync.publishNow, sync.transport],
    ),
  );

  const navigatorLeads = !collapsed.contextNavigator;

  const columns = (
    <>
      <AppHeader
        project={project}
        setup={setup}
        layout={{ collapsed, canOpen, onTogglePanel: togglePanel }}
        onSearch={() => setSearching(true)}
        sync={sync}
        onOpenSync={() => setSyncOpen(true)}
      />

      <ResizablePanelGroup
        orientation="horizontal"
        groupRef={groupRef}
        elementRef={groupElementRef}
        className="min-h-0 flex-1"
      >
        <ResizablePanel
          id={PANEL_IDS.primarySidebar}
          panelRef={panelRefs.primarySidebar}
          collapsible
          // The width the column folds to, which is the first step of the fold
          // and not a width it can be dragged to: a collapsible panel jumps
          // between its minimum and its folded width as the edge passes the
          // halfway point between them, so the rail is a position rather than a
          // range and its icons are never stretched by a column between the
          // two. The second step declares a folded width of nothing, so that
          // the same edge — and the control in the title bar — can close the
          // column outright.
          collapsedSize={
            intended.primarySidebar === "hidden"
              ? 0
              : PANEL_GEOMETRY.primarySidebar.railWidth
          }
          defaultSize={PANEL_GEOMETRY.primarySidebar.preferredWidth}
          minSize={PANEL_GEOMETRY.primarySidebar.minWidth}
          maxSize={PANEL_GEOMETRY.primarySidebar.maxWidth}
          groupResizeBehavior="preserve-pixel-size"
          onResize={(size) => reportPanelSize("primarySidebar", size.inPixels)}
          className="overflow-hidden bg-sidebar"
        >
          {collapsed.primarySidebar ? null : (
            <PrimarySidebar
              sections={sections}
              badges={badges}
              updates={updates}
              activeAreaKey={activeKey}
              rail={stages.primarySidebar === "rail"}
              onSelectArea={selectArea}
              onArrange={arrange}
            />
          )}
        </ResizablePanel>

        <PanelEdge muted={collapsed.primarySidebar} />

        <ResizablePanel
          id={PANEL_IDS.contextNavigator}
          panelRef={panelRefs.contextNavigator}
          collapsible
          collapsedSize={0}
          defaultSize={PANEL_GEOMETRY.contextNavigator.preferredWidth}
          minSize={PANEL_GEOMETRY.contextNavigator.minWidth}
          maxSize={PANEL_GEOMETRY.contextNavigator.maxWidth}
          groupResizeBehavior="preserve-pixel-size"
          onResize={(size) => reportPanelSize("contextNavigator", size.inPixels)}
          className="relative overflow-hidden bg-panel"
        >
          <AreaSlot attach={slotRefs.Navigator} />
        </ResizablePanel>

        <PanelEdge muted={!navigatorLeads} />

        <ResizablePanel
          id={PANEL_IDS.workspace}
          minSize={WORKSPACE_MIN_WIDTH}
          className="relative overflow-hidden bg-workspace"
        >
          <AreaSlot attach={slotRefs.Workspace} />
        </ResizablePanel>

        <PanelEdge muted={collapsed.contextInspector} />

        <ResizablePanel
          id={PANEL_IDS.contextInspector}
          panelRef={panelRefs.contextInspector}
          collapsible
          collapsedSize={0}
          defaultSize={PANEL_GEOMETRY.contextInspector.preferredWidth}
          minSize={PANEL_GEOMETRY.contextInspector.minWidth}
          maxSize={PANEL_GEOMETRY.contextInspector.maxWidth}
          groupResizeBehavior="preserve-pixel-size"
          onResize={(size) => reportPanelSize("contextInspector", size.inPixels)}
          className="relative overflow-hidden bg-panel"
        >
          <AreaSlot attach={slotRefs.Inspector} />
        </ResizablePanel>
      </ResizablePanelGroup>

      <SearchPalette
        project={project}
        opener={opener}
        open={searching}
        onOpenChange={setSearching}
        onShow={show}
      />

      <SyncSheet open={syncOpen} onOpenChange={setSyncOpen} sync={sync} />
    </>
  );

  // Every visited area's provider, one inside the next, each rendering its own
  // columns into the panels. `reduceRight` so that the first area visited ends
  // up outermost and the nesting order matches `visited`.
  //
  // Each provider's children are its own columns *and then* the next provider,
  // in that order. Visiting a new area therefore only ever appends a second
  // child to the innermost provider, which is the one arrangement that leaves
  // every area already mounted exactly where it was. Nesting the panels inside
  // instead — which is what this looked like before — moved them one level
  // deeper on every first visit, and React answers a change of element type by
  // rebuilding the subtree: the whole panel group, and every column in it, was
  // being thrown away and made again.
  // A section can leave under the window — removing the extension that brought
  // it is the ordinary case — and its key stays in the visited list rather than
  // being pruned: if the same extension comes back, it comes back where it was,
  // and nothing mounted after it moves a level in the tree.
  const areaLayers = visited.reduceRight<ReactNode>((children, key) => {
    const area = mounted.get(key);
    if (area === undefined) return children;

    const columns = (
      <>
        <AreaColumns
          areaKey={key}
          module={area.module}
          active={key === activeKey}
          slots={slots}
        />
        {children}
      </>
    );

    // An area with nothing to share between its columns need not write an
    // empty wrapper, and one that has state cannot do without one.
    const { Provider } = area.module;
    const layer =
      Provider === undefined ? (
        columns
      ) : (
        <Provider
          key={key}
          project={project}
          active={key === activeKey}
          intent={asked?.areaKey === key ? asked.intent : null}
        >
          {columns}
        </Provider>
      );

    // Around the whole layer, because an area is as likely to hold what it
    // would report in its provider as in one of its columns. It encloses every
    // area visited after this one as well, and that is not a leak: each of
    // those opens a scope of its own around everything of its own, and the
    // nearer one is the one a hook finds.
    return (
      <BadgeScope key={key} areaKey={key} report={live.report}>
        {layer}
      </BadgeScope>
    );
  }, null);

  return (
    <PackagesProvider packages={packages}>
      <CompositionProvider value={composition}>
        {/* Above every area, because a link may point at a kind belonging to a
            section this one has never heard of. */}
        <RecordLinks project={project} opener={opener} onShow={show}>
          {/* The window's own chrome, outside every area and never rebuilt. */}
          {columns}
          {areaLayers}
        </RecordLinks>
      </CompositionProvider>
    </PackagesProvider>
  );
}

/**
 * The catalogue, in the shape a section brought by a package has.
 *
 * The one area the window owns, and it is expressed as one of the others so
 * that there is a single path for mounting a section. Its id is the shell's own
 * and names no extension — it is where a person decides which extensions there
 * are, which is why it is here and not in a package.
 */
const CATALOGUE: MountedArea = {
  key: EXTENSIONS_AREA.id,
  extensionId: EXTENSIONS_AREA.id,
  label: EXTENSIONS_AREA.label,
  description: EXTENSIONS_AREA.description,
  frame: EXTENSIONS_AREA.frame,
  icon: EXTENSIONS_AREA.icon,
  development: false,
  // Nothing to count. What this section holds is packages rather than records,
  // so there is no question about the corpus to ask on its behalf — and the one
  // mark it is going to want is the update the registry has, which is a
  // different thing arriving with `docs/extensions.md` §7.
  badge: null,
  module: EXTENSIONS_AREA_MODULE,
};

/** The columns of the frame, in the order the window draws them. */
const AREA_COLUMNS = ["Navigator", "Workspace", "Inspector"] as const;

type AreaColumn = (typeof AREA_COLUMNS)[number];

/** The node each column is drawn into, once that node exists. */
type AreaSlots = Record<AreaColumn, HTMLDivElement | null>;

const EMPTY_SLOTS: AreaSlots = {
  Navigator: null,
  Workspace: null,
  Inspector: null,
};

/**
 * The node an area's columns are drawn into, inside one panel.
 *
 * A child of the panel rather than the panel's own element, because a panel is
 * two elements and only the inner one is the column: the outer one — the one
 * `elementRef` hands back — carries neither the position nor the clipping the
 * class list asks for, so a column portalled into it sized itself against the
 * window instead of against its own column, and the three of them lay on top of
 * each other over the whole slab.
 *
 * It fills its panel and is positioned, which is what lets a column be laid out
 * against the column it is in and nothing else.
 */
function AreaSlot({
  attach,
}: {
  attach: (element: HTMLDivElement | null) => void;
}) {
  return <div ref={attach} className="absolute inset-0" />;
}

/**
 * One area's columns, each drawn into the panel that holds it.
 *
 * Portals, because the two things this has to satisfy pull in opposite
 * directions: a column has to be a descendant of its area's provider to see
 * what the area knows, and the panels have to be a fixed part of the window so
 * that installing or opening an area never rebuilds them. A portal renders
 * where the provider is and appears where the panel is, which is the only
 * arrangement that gives both.
 *
 * Every visited area draws its columns and all but the selected one are hidden.
 * Hidden with `visibility` rather than `display`, deliberately: a column
 * removed from the layout loses where it was scrolled to, and being returned to
 * the middle of a list one had scrolled through is exactly the annoyance this
 * arrangement is meant to remove.
 *
 * A column its frame has no component for contributes nothing, which is why
 * this can be asked of every area rather than only of the compatible ones.
 */
function AreaColumns({
  areaKey,
  module,
  active,
  slots,
}: {
  areaKey: string;
  module: AreaModule;
  active: boolean;
  slots: AreaSlots;
}) {
  return (
    <>
      {AREA_COLUMNS.map((column) => {
        const Column = module[column];
        const slot = slots[column];
        if (!Column || !slot) return null;

        return createPortal(
          <div
            // Hidden from assistive technology as well as from the eye: a
            // screen reader walking three areas' worth of columns would be
            // reading a window nobody is looking at.
            aria-hidden={!active}
            inert={!active}
            className={cn("absolute inset-0", active ? null : "invisible")}
          >
            <Column />
          </div>,
          slot,
          `${areaKey}-${column}`,
        );
      })}
    </>
  );
}

/**
 * The boundary between two columns: a structural edge that happens to be
 * draggable, not a decorative divider. Double-clicking it restores the
 * neighbouring panel to its default width.
 *
 * An edge beside a collapsed column stops drawing its hairline but stays in
 * place. A collapsed column is zero pixels wide, so both of its edges would
 * otherwise sit against each other and draw a line that divides nothing — but
 * removing one from the tree is not the way to fix that. Collapsing by dragging
 * ends with the edge under the pointer being the one that would disappear, and
 * the panel library still holds a pointer capture on it; taking it out of the
 * DOM mid-gesture throws `InvalidStateError`. It also has to stay for the
 * column to be draggable back out at all.
 */
function PanelEdge({ muted }: { muted?: boolean }) {
  return (
    <ResizableHandle
      className={cn(
        "transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-fg-tertiary",
        muted ? "bg-transparent" : "bg-separator",
      )}
    />
  );
}
