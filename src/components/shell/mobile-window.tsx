"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Info, Search, SlidersHorizontal } from "lucide-react";

import {
  BarButton,
  NavBar,
  Row,
  RowSeparator,
  SafeAreaFoot,
  Screen,
  Sheet,
  Stack,
  WindowBar,
} from "@/components/shell/mobile-chrome";
import { SyncIndicator } from "@/components/shell/sync-indicator";
import type { MountedArea, UnavailableArea } from "@/lib/extension-host/areas";
import type { BadgeCount, Badges } from "@/lib/extension-host/badges";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import type { OpenProject } from "@/lib/project/types";
import { BandSlotsProvider } from "@/lib/shell-bands";
import { FRAMES } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";

/**
 * The window with a project open, at the width of a phone.
 *
 * The Mac shows the columns of a frame side by side and lets a person fold the
 * ones they are not using. That arrangement has a floor — the workspace alone
 * asks for 500 points before anything stands beside it — and a phone is 390.
 * Below the floor the columns do not get tighter, they get *taken away*: the
 * navigator collapses at 930 points of window and the inspector at 1230, so a
 * phone showing the desktop layout is a phone whose sections have no list and
 * nothing to say about what is open, with no gesture that could bring either
 * back.
 *
 * So the same columns are arranged in the one way a phone has for more content
 * than fits: depth. The frame's columns become the levels of a navigation
 * stack — sections, then the navigator, then the workspace — and the inspector
 * is raised over the workspace as a sheet, because it is read *about* whatever
 * the workspace is showing and pushing it would take that off the screen.
 *
 * **The columns themselves are untouched, and that is the point.** An area
 * draws into the same three slots by the same names; what differs is where the
 * slots are on the screen. An extension cannot tell a phone from a Mac, which
 * is what stops the mobile version from being a second product to maintain.
 */
export function MobileWindow({
  project,
  sections,
  unavailable,
  catalogue,
  badges,
  updates,
  active,
  attachNavigator,
  attachWorkspace,
  attachInspector,
  sync,
  intent,
  onSelectArea,
  onSearch,
  onOpenSync,
  onOpenSettings,
  onLeave,
}: {
  project: OpenProject;
  /** The sections this project's packages brought, in the order it declares. */
  sections: readonly MountedArea[];
  /**
   * The sections this project has and this phone has nothing to run.
   *
   * Drawn rather than left out, and that is the decision this member exists to
   * carry. A project is one repository, and its sections are the same wherever
   * it is open: a phone that silently showed fewer of them would be read as a
   * project that had lost something, and the person would go looking for what
   * they had done to it. So the row is here, in its place in the order, in the
   * tier the system uses for what cannot be tapped.
   */
  unavailable: readonly UnavailableArea[];
  /** The one section the window owns, drawn under the others rather than among them. */
  catalogue: MountedArea;
  badges: Badges;
  /** How many declared extensions have a newer version published. */
  updates: number;
  /** The section showing, and `null` while there is not one yet. */
  active: MountedArea | null;
  /**
   * Where each column of the frame is drawn.
   *
   * Three parameters rather than one object of three, because each is the node
   * one column is attached to and they are handed to three different screens.
   */
  attachNavigator: (element: HTMLDivElement | null) => void;
  attachWorkspace: (element: HTMLDivElement | null) => void;
  attachInspector: (element: HTMLDivElement | null) => void;
  sync: SyncStatus;
  /**
   * The last thing asked of an area — a link followed, a search result opened.
   *
   * Carried here because it moves the screen: something addressed at what an
   * area is showing has to arrive with the workspace in front of the person,
   * and on a Mac that needs no saying because the workspace is always in sight.
   */
  intent: unknown;
  onSelectArea: (key: string) => void;
  onSearch: () => void;
  onOpenSync: () => void;
  /**
   * What this phone is, which is not part of this project and is reached from
   * inside it anyway — the way a Mac reaches Settings from the menu bar with a
   * project open. The window above owns it, because forgetting the computer
   * from in there takes this window with it.
   *
   * Optional for the reason `onLeave` is, and it is the same reason: both are
   * things the window *above* this one can do, and both are handed down by the
   * phone's composition rather than assumed by this one.
   */
  onOpenSettings?: () => void;
  /** Back to the computer's list of projects. */
  onLeave?: () => void;
}) {
  const [depth, setDepth] = useState(0);
  const [inspecting, setInspecting] = useState(false);
  const frame = FRAMES[active?.frame ?? catalogue.frame];

  // Where each column's foot is drawn — the band at the bottom of its screen
  // rather than a strip inside the column. Held as state for the reason the
  // window holds its panels that way: a portal needs its node to exist before
  // anything can be put through it.
  const [bands, setBands] = useState<{
    Navigator: HTMLElement | null;
    Workspace: HTMLElement | null;
  }>({ Navigator: null, Workspace: null });
  const bandRefs = useMemo(() => {
    const attach = (column: "Navigator" | "Workspace") => (element: HTMLElement | null) =>
      setBands((current) =>
        current[column] === element ? current : { ...current, [column]: element },
      );
    return { Navigator: attach("Navigator"), Workspace: attach("Workspace") };
  }, []);

  const open = useCallback(
    (area: MountedArea) => {
      onSelectArea(area.key);
      // Straight to the workspace for a frame that lists nothing: a level with
      // an empty column in it is a tap that costs a screen and gives nothing.
      setDepth(FRAMES[area.frame].navigator ? NAVIGATOR : WORKSPACE);
    },
    [onSelectArea],
  );

  const pop = useCallback(() => {
    setDepth((at) =>
      at === WORKSPACE && !frame.navigator ? SECTIONS : Math.max(SECTIONS, at - 1),
    );
  }, [frame.navigator]);

  // Something was addressed at the area, so the area is what has to be looked
  // at. The window has already selected it; this is the half of that a phone
  // needs and a Mac does not — on a Mac the workspace is on the screen already.
  //
  // Read during the render that shows it rather than in an effect after it, the
  // way this window records a first visit: an effect would draw the screen the
  // person was on for one frame and then push over the top of it.
  const [answered, setAnswered] = useState(intent);
  if (intent !== answered) {
    setAnswered(intent);
    if (intent !== null) setDepth(WORKSPACE);
  }

  const sectionsScreen = (
    <Screen
      key="sections"
      className="bg-sidebar"
      bar={
        <NavBar
          title={project.name}
          // The way out of a project is the way in reversed. There is no other:
          // a phone has no window to close and no folder to pick instead.
          back={onLeave ? "Projects" : undefined}
          onBack={onLeave}
        />
      }
      // The window's own controls, at the end of the screen a thumb rests on.
      // They were in the trailing corner of the bar above, which is where a
      // Mac keeps them and the one place on a phone held in one hand that a
      // thumb has to be re-gripped to reach. This screen is the root of the
      // project and has no column in it, so the foot is free — one deeper it
      // belongs to whatever a package put in its band.
      foot={
        <WindowBar>
          <BarButton label="Search" icon={Search} onPress={onSearch} />
          {/* Between the two, and silent when a project's memory is in step
              with its remote. What is on either side of it is a control that
              is always here, so the state has the middle: it is the one thing
              in this band that appears because something is true. */}
          <SyncIndicator
            sync={sync}
            onOpen={onOpenSync}
            // `shrink` against the control's own `shrink-0`: what it says can
            // run to several words, and a band whose middle cannot give way is
            // a band that pushes a control off its own end.
            className="h-11 min-w-0 shrink px-3 text-[15px] leading-[20px]"
          />
          {onOpenSettings ? (
            <BarButton
              label="Settings"
              icon={SlidersHorizontal}
              onPress={onOpenSettings}
            />
          ) : null}
        </WindowBar>
      }
    >
      {sections.map((area, at) => (
        <div key={area.key}>
          {at === 0 ? null : <RowSeparator />}
          <Row
            icon={area.icon}
            label={area.label}
            badge={counted(badges.get(area.key))}
            leadsOn
            selected={area.key === active?.key}
            onPress={() => open(area)}
          />
        </div>
      ))}

      {/* Under the sections that work, in one band with the reason said once.
          Said once rather than on every row because it is one reason: what
          differs between them is which capability is missing, and that belongs
          on the extension's own page — where somebody is deciding about the
          package — rather than truncated into a list of names. */}
      {unavailable.length === 0 ? null : (
        <div className="mt-6 border-t border-separator">
          {unavailable.map((area, at) => (
            <div key={area.key}>
              {at === 0 ? null : <RowSeparator />}
              <Row icon={area.icon} label={area.label} disabled />
            </div>
          ))}
          <p className="px-4 py-2 text-[13px] leading-[18px] text-fg-tertiary">
            These need a computer. They are part of this project and they work
            where it is open on one.
          </p>
        </div>
      )}

      {/* Apart from the sections rather than last among them, the way the
          column on a Mac keeps it in a band of its own: the sections are what
          this project has, and this is where a person changes what it has. */}
      <div className="mt-6 border-t border-separator">
        <Row
          icon={catalogue.icon}
          label={catalogue.label}
          badge={updates > 0 ? updates : undefined}
          leadsOn
          selected={catalogue.key === active?.key}
          onPress={() => open(catalogue)}
        />
      </div>
    </Screen>
  );

  const navigatorScreen = (
    <AreaScreen
      key="navigator"
      className="bg-panel"
      bar={<NavBar title={active?.label ?? ""} back={project.name} onBack={pop} />}
      attach={attachNavigator}
      attachBand={bandRefs.Navigator}
      // Choosing something in the list goes on to it, which on a Mac needs no
      // saying: the workspace is already on the screen, and the column beside
      // it changes what it shows. Here they are two screens, and the second one
      // has to be pushed by somebody.
      //
      // It is read from the click rather than told by the area, and that is the
      // whole point: an area is a package that has never heard of a phone. What
      // the shell can see is that something in the list was activated, and that
      // is exactly what "go on to it" means at this width.
      //
      // Not the bands, though. The head and foot of a column hold controls that
      // act on the list — filtering it, adding to it — and a filter that threw
      // the screen away as it was applied would be unusable.
      onActivate={() => setDepth(WORKSPACE)}
    />
  );

  const workspaceScreen = (
    <AreaScreen
      key="workspace"
      className="bg-workspace"
      bar={
        <NavBar
          // No title: the workspace is showing the thing this screen is about,
          // and a bar naming the section again over the top of it would say
          // what is already on the screen twice.
          back={frame.navigator ? (active?.label ?? "") : project.name}
          onBack={pop}
          trailing={
            frame.inspector ? (
              <BarButton
                label="Details"
                icon={Info}
                onPress={() => setInspecting(true)}
              />
            ) : null
          }
        />
      }
      attach={attachWorkspace}
      attachBand={bandRefs.Workspace}
    />
  );

  return (
    <BandSlotsProvider value={bands}>
    <div className="relative min-h-0 flex-1">
      <Stack depth={depth} onPop={pop}>
        {[sectionsScreen, navigatorScreen, workspaceScreen]}
      </Stack>

      {/* Raised whatever the frame says, and empty when the frame has no
          inspector — the column has to keep the node it draws into for as long
          as the area is mounted, exactly as the Mac keeps a folded panel. What
          the frame decides is whether anything can raise it. */}
      <Sheet
        open={inspecting && frame.inspector}
        title="Details"
        onClose={() => setInspecting(false)}
      >
        <AreaSlot attach={attachInspector} />
      </Sheet>
    </div>
    </BandSlotsProvider>
  );
}

/** The levels of the stack, which are the columns of a frame in order. */
const SECTIONS = 0;
const NAVIGATOR = 1;
const WORKSPACE = 2;

/**
 * A screen whose whole body is one of an area's columns.
 *
 * Not `Screen`: that one scrolls what it is given, and a column does its own
 * scrolling — the window's rule everywhere, and the reason a column can hold a
 * list of ten thousand rows. What it needs instead is a box of a known size to
 * be positioned against, and the space the hardware claims at the foot kept out
 * of it.
 */
function AreaScreen({
  className,
  bar,
  attach,
  attachBand,
  onActivate,
}: {
  className?: string;
  bar: ReactNode;
  attach: (element: HTMLDivElement | null) => void;
  /** Where the column's own foot is drawn: the band under this screen. */
  attachBand?: (element: HTMLElement | null) => void;
  /** Something in the column's own list was activated, bands excepted. */
  onActivate?: () => void;
}) {
  const holding = useRef<HTMLDivElement | null>(null);

  // A DOM listener rather than `onClick`, and the reason is the arrangement
  // this whole file is built on: a column is rendered where its area is and
  // *portalled* into this box. React sends an event up the tree it rendered in,
  // which is the area's, so a handler written here would never be called — the
  // click reaches the window's own layer instead, several screens away.
  //
  // The document does not work that way. A native listener on the box sees
  // every click inside it, whichever component put the node there, and that is
  // exactly the question being asked: was something in *this column* chosen.
  useEffect(() => {
    const box = holding.current;
    if (box === null || onActivate === undefined) return;

    const noticed = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const acted = target.closest(ACTIVATED);
      // After the column has had it rather than before: the area decides what
      // was chosen, and this only decides where to look next.
      if (acted && !acted.closest("[data-panel-band]")) onActivate();
    };

    box.addEventListener("click", noticed);
    return () => box.removeEventListener("click", noticed);
  }, [onActivate]);

  return (
    <div className={cn("flex h-full min-h-0 flex-col", className)}>
      {bar}
      <div
        ref={holding}
        className="relative min-h-0 flex-1"
      >
        <AreaSlot attach={attach} />
      </div>

      {/* Outside the scroller, so the space the hardware claims is not
          something a person has to scroll to reach. The band draws nothing
          when the column put nothing in it — a rule of the box rather than a
          second piece of state, because what is in it arrives by portal and
          this file never sees it. */}
      {attachBand === undefined ? (
        <SafeAreaFoot />
      ) : (
        // The hardware's space is the band's own padding, so the band's
        // surface runs to the bottom edge of the screen and the home indicator
        // rests on it. The padding stays when the band draws nothing — the
        // column's list must clear the gesture either way — and it is stated
        // once here rather than as a spacer underneath, because two reserves
        // of the same strip is exactly how a bar ends up floating over a band
        // of nothing.
        <div
          className="shrink-0"
          style={{ paddingBottom: "env(safe-area-inset-bottom, 0px)" }}
        >
          <div
            ref={attachBand}
            className="flex min-h-11 items-center gap-1 border-t border-separator px-1 not-has-[*]:hidden"
          />
        </div>
      )}
    </div>
  );
}

/**
 * What counts as choosing something, in the vocabulary a column is built from.
 *
 * Wide rather than exact, because the rows are a package's and this file cannot
 * know what one is made of. Anything a person can activate at all is taken for
 * the row it usually is; a control that only changes the list is in a band, and
 * bands are excluded above.
 */
const ACTIVATED = "button, a[href], [role=option], [role=treeitem], [role=row]";

/**
 * The node one column is drawn into, filling whatever it is put in.
 *
 * The same arrangement the panels use on a Mac and for the same reason: a
 * column is laid out against the box it is in, so it needs one that is
 * positioned and of a known size rather than the screen.
 */
function AreaSlot({
  attach,
}: {
  attach: (element: HTMLDivElement | null) => void;
}) {
  return <div ref={attach} className="absolute inset-0" />;
}

/** A count the window holds, in the two shapes a row draws. */
function counted(badge: BadgeCount | undefined): number | "dot" | undefined {
  if (badge === undefined) return undefined;
  return badge.kind === "dot" ? "dot" : badge.value;
}
