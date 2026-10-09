"use client";

import { useCallback, useMemo, useState } from "react";
import { Info, Search } from "lucide-react";

import { ColumnBand, Stack, type Screen } from "@/components/shell/mobile-stack";
import { SectionsBar, type Section } from "@/components/shell/mobile-sections";
import { SyncIndicator } from "@/components/shell/sync-indicator";
import { ProgressLine } from "@/components/shell/progress-line";
import type { MountedArea, UnavailableArea } from "@/lib/extension-host/areas";
import type { BadgeCount, Badges } from "@/lib/extension-host/badges";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import type { OpenProject } from "@/lib/project/types";
import { BandSlotsProvider, type AreaColumn } from "@/lib/shell-bands";
import { FRAMES } from "@/lib/shell-frames";
import { haptic } from "@/lib/haptic";

/**
 * The window with a project open, at the width of a phone.
 *
 * The Mac shows the columns of a frame side by side and lets a person fold the
 * ones they are not using. That arrangement has a floor — the workspace alone
 * asks for 500 points before anything stands beside it — and a phone is 390.
 * Below the floor the columns do not get tighter, they get *taken away*.
 *
 * So they are arranged the way this hardware's own platform arranges them when
 * a split view runs out of width: one in front of the next, as a stack. The
 * order is the desk's — what lists, then what is shown, then what is true of it
 * — and choosing a row pushes. At 390 points *deeper* and *narrower* are the
 * same thing, and a push is what the person holding the phone already knows how
 * to undo.
 *
 * **Every section keeps its own stack.** Glancing at another section and coming
 * back arrives at the record, not at the list it was chosen from, which is what
 * a tab bar means everywhere else on this platform.
 *
 * What is always drawn is the band of sections at the foot, because it is the
 * only chrome a thumb uses more than a few times an hour, and the bar at the
 * head, because the way back has to be *visible*: a phone whose only way out
 * was a gesture is a phone somebody can be stuck in.
 *
 * **The columns themselves are untouched, and that is the point.** An area
 * draws into the same three slots by the same names; what differs is where the
 * slots are on the screen. An extension cannot tell a phone from a Mac, which
 * is what stops the mobile version from being a second product to maintain.
 */
export function MobileWindow({
  project,
  activity,
  sections,
  unavailable,
  catalogue,
  badges,
  updates,
  unseen,
  active,
  attachNavigator,
  attachWorkspace,
  attachInspector,
  sync,
  intent,
  onSelectArea,
  onSearch,
  onOpenSync,
  onLeave,
}: {
  project: OpenProject;
  /**
   * The window's own section, first in the band and never moved.
   *
   * Where the column on a Mac puts it, and for the same reason: what a project
   * brought is a person's to arrange, and what the window owns is not.
   */
  activity: MountedArea;
  /** The sections this project's packages brought, in the order it declares. */
  sections: readonly MountedArea[];
  /**
   * The sections this project has and this phone has nothing to run.
   *
   * Drawn rather than left out, and that is the decision this member exists to
   * carry. A project is one repository, and its sections are the same wherever
   * it is open: a phone that silently showed fewer of them would be read as a
   * project that had lost something, and the person would go looking for what
   * they had done to it. So they are in the band, in their place in the order,
   * and cannot be pressed.
   */
  unavailable: readonly UnavailableArea[];
  /** The one section the window owns, drawn at the end of the band. */
  catalogue: MountedArea;
  badges: Badges;
  /** How many declared extensions have a newer version published. */
  updates: number;
  /**
   * How many records have changed since this person last looked, or `null`
   * while that is still being read.
   *
   * `null` and zero both draw nothing, exactly as on the Mac: a window that
   * printed `0` before it had asked would tell somebody with twenty things
   * waiting that nothing had happened, and a mark that is always there is not
   * news.
   */
  unseen: number | null;
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
  /** Back to the computer's list of projects. */
  onLeave?: () => void;
}) {
  /**
   * How deep each section's stack is standing, by the section's own key.
   *
   * Per section rather than one number for the window, and that is the half of
   * this arrangement a pager could not have. A record left open in one section
   * is still open when a person comes back to it from another — which is what
   * a tab bar means on this platform, and what made the old pager's rule
   * (every section opens at its first column) read as the window throwing work
   * away.
   *
   * A section not in the map is at its first screen, which is where a section
   * nobody has opened yet should be.
   */
  const [depths, setDepths] = useState<ReadonlyMap<string, number>>(new Map());
  const frame = FRAMES[active?.frame ?? catalogue.frame];
  const activeKey = active?.key ?? catalogue.key;

  // Where each column's foot is drawn — the band at the head of its screen
  // rather than a strip inside the column. Held as state for the reason the
  // window holds its panels that way: a portal needs its node to exist before
  // anything can be put through it.
  const [bands, setBands] = useState<Record<AreaColumn, HTMLElement | null>>({
    Navigator: null,
    Workspace: null,
    Inspector: null,
  });
  const bandRefs = useMemo(() => {
    const attach = (column: AreaColumn) => (element: HTMLElement | null) =>
      setBands((current) =>
        current[column] === element ? current : { ...current, [column]: element },
      );
    return {
      Navigator: attach("Navigator"),
      Workspace: attach("Workspace"),
      Inspector: attach("Inspector"),
    };
  }, []);

  const band = useMemo(
    () => bandOf({ activity, sections, unavailable, catalogue, badges, updates, unseen }),
    [activity, sections, unavailable, catalogue, badges, updates, unseen],
  );

  /** Stand this section's stack at a given screen. */
  const standAt = useCallback(
    (key: string, screen: number) => {
      setDepths((held) => {
        if ((held.get(key) ?? 0) === screen) return held;
        const next = new Map(held);
        next.set(key, screen);
        return next;
      });
    },
    [],
  );

  const open = useCallback(
    (key: string) => {
      // Pressing the section already showing is the platform's own shortcut
      // back to its root, and it is the one control on this screen that can do
      // it without a journey: a stack three deep is otherwise three presses
      // from the list it started at.
      if (key === activeKey) standAt(key, 0);
      else onSelectArea(key);
    },
    [activeKey, onSelectArea, standAt],
  );

  // Which screen of this frame the workspace is. The first one where the frame
  // has no column that lists — which is the whole of what "no navigator" means
  // at this width.
  const workspaceAt = frame.navigator ? 1 : 0;
  const depth = Math.min(
    depths.get(activeKey) ?? 0,
    workspaceAt + (frame.inspector ? 1 : 0),
  );

  // Something was addressed at the area, so the area is what has to be looked
  // at. The window has already selected it; this is the half of that a phone
  // needs and a Mac does not — on a Mac the workspace is on the screen already.
  //
  // Read during the render that shows it rather than in an effect after it: an
  // effect would draw the screen the person was on for one frame and then move
  // out from under them.
  const [answered, setAnswered] = useState(intent);
  if (intent !== answered) {
    setAnswered(intent);
    if (intent !== null) standAt(activeKey, workspaceAt);
  }

  // Choosing a row pushes on to what it opens. Read from the click rather than
  // told by the area, and that is the whole point: an area is a package that
  // has never heard of a phone. What the shell can see is that something in a
  // list was activated, and at this width that *is* what going on to it means.
  //
  // Not the bands, though. The foot of a column holds controls that act on the
  // list — filtering it, adding to it — and a filter that threw the screen away
  // as it was applied would be unusable.
  const advance = useCallback(() => {
    haptic();
    standAt(activeKey, workspaceAt);
  }, [activeKey, standAt, workspaceAt]);

  /** What the window says about itself, in the middle of the first bar. */
  const title = (
    <>
      <span className="w-full truncate text-center text-[17px] leading-[22px] font-semibold">
        {project.name}
      </span>
      {/* Under the name rather than beside it, which is where this platform
          puts a line about what a screen is doing — and the one place left for
          it now that the window has no chrome of its own. Silence is still a
          state: with nothing to say the indicator draws nothing, and the name
          sits alone and centred. */}
      <SyncIndicator
        sync={sync}
        onOpen={onOpenSync}
        className="h-4 min-w-0 px-0 text-[11px] leading-[14px]"
      />
    </>
  );

  /** Searching the whole corpus, which belongs to the project and not a column. */
  const search = (
    <button
      type="button"
      onClick={onSearch}
      aria-label="Search"
      className="flex size-11 shrink-0 items-center justify-center rounded-(--radius-control) active:opacity-60"
    >
      <Search className="size-[22px] text-fg-secondary" />
    </button>
  );

  /**
   * The way on to what is true of what is being shown.
   *
   * A control rather than a swipe, and that is the exchange this arrangement
   * makes: the one gesture on this screen means one thing — go back — so the
   * other direction needs somewhere to be pressed. It is the platform's own
   * answer for a column that describes what is in front of you.
   */
  const inspect = frame.inspector ? (
    <button
      type="button"
      onClick={() => {
        haptic();
        standAt(activeKey, workspaceAt + 1);
      }}
      aria-label="Details"
      className="flex size-11 shrink-0 items-center justify-center rounded-(--radius-control) active:opacity-60"
    >
      <Info className="size-[22px] text-fg-secondary" />
    </button>
  ) : null;

  const screens: Screen[] = [
    ...(frame.navigator
      ? [
          {
            key: "navigator",
            body: <AreaSlot attach={attachNavigator} onActivate={advance} />,
            band: <ColumnBand attach={bandRefs.Navigator} />,
            behind: "Projects",
            title,
            trailing: search,
          },
        ]
      : []),
    {
      key: "workspace",
      body: <AreaSlot attach={attachWorkspace} />,
      band: <ColumnBand attach={bandRefs.Workspace} />,
      // The section's own name where there is a list behind it, and the way out
      // of the project where the workspace *is* the first screen.
      behind: frame.navigator ? (active?.label ?? catalogue.label) : "Projects",
      ...(frame.navigator ? {} : { title }),
      trailing: (
        <>
          {frame.navigator ? null : search}
          {inspect}
        </>
      ),
    },
    ...(frame.inspector
      ? [
          {
            key: "inspector",
            body: <AreaSlot attach={attachInspector} />,
            band: <ColumnBand attach={bandRefs.Inspector} />,
            // Named by nobody, and deliberately: what the workspace is showing
            // is a package's to know, and a bar that guessed would be wrong
            // exactly where it was most read. The platform's own word for a
            // screen it cannot name is this one.
            behind: "Back",
          },
        ]
      : []),
  ];

  return (
    <BandSlotsProvider value={bands}>
      <div
        className="relative flex min-h-0 flex-1 flex-col overflow-clip bg-workspace"
        // A breath of light along the top edge in the dark appearance, and
        // nothing in the light one. What it buys is that the field reads as a
        // surface the work is standing on rather than as an absence behind it
        // — the phone's answer to a desk window's frame, which it has none of.
        style={{ backgroundImage: "var(--phone-horizon)" }}
      >
        {/* The window's own report that it is waiting, on the top edge of the
            screen rather than under a bar — the bar below starts under the
            hardware's inset. It is the one thing drawn over that inset,
            because a line two points tall under a notch is a line nobody
            sees. */}
        <div className="pointer-events-none absolute inset-x-0 top-0 z-30 h-0.5">
          <ProgressLine />
        </div>

        <Stack
          screens={screens}
          depth={depth}
          // Nothing behind the first screen on a machine that cannot leave a
          // project, which is every machine but a phone.
          canPop={onLeave !== undefined}
          onPop={() => {
            if (depth > 0) standAt(activeKey, depth - 1);
            else onLeave?.();
          }}
        />

        <SectionsBar sections={band} activeKey={active?.key ?? null} onChoose={open} />
      </div>
    </BandSlotsProvider>
  );
}

/**
 * The node one column is drawn into, filling whatever it is put in.
 *
 * The same arrangement the panels use on a Mac and for the same reason: a
 * column is laid out against the box it is in, so it needs one that is
 * positioned and of a known size rather than the screen.
 */
function AreaSlot({
  attach,
  onActivate,
}: {
  attach: (element: HTMLDivElement | null) => void;
  /** Something in this column's own list was chosen, its band excepted. */
  onActivate?: () => void;
}) {
  // A DOM listener rather than `onClick`, and the reason is the arrangement
  // this whole file is built on: a column is rendered where its area is and
  // *portalled* into this box. React sends an event up the tree it rendered
  // in, which is the area's, so a handler written here would never be called —
  // the click reaches the window's own layer instead, several screens away.
  //
  // The document does not work that way. A native listener on the box sees
  // every click inside it, whichever component put the node there, and that is
  // exactly the question being asked: was something in *this column* chosen.
  const watch = useCallback(
    (box: HTMLDivElement | null) => {
      attach(box);
      if (box === null || onActivate === undefined) return;
      const noticed = (event: MouseEvent) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        const acted = target.closest(ACTIVATED);
        // After the column has had it rather than before: the area decides
        // what was chosen, and this only decides where to look next.
        if (acted && !acted.closest("[data-panel-band]")) onActivate();
      };
      box.addEventListener("click", noticed);
      return () => box.removeEventListener("click", noticed);
    },
    [attach, onActivate],
  );

  return <div ref={watch} className="absolute inset-0" />;
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
 * The band the sections are drawn from: the window's own at either end, and
 * what the project brought between them.
 *
 * The sections a phone cannot run keep their place in that order rather than
 * being dropped to the end — where a section is, is the project's decision,
 * and this machine not being able to run one does not change it.
 */
function bandOf({
  activity,
  sections,
  unavailable,
  catalogue,
  badges,
  updates,
  unseen,
}: {
  activity: MountedArea;
  sections: readonly MountedArea[];
  unavailable: readonly UnavailableArea[];
  catalogue: MountedArea;
  badges: Badges;
  updates: number;
  unseen: number | null;
}): readonly Section[] {
  return [
    {
      key: activity.key,
      label: activity.label,
      icon: activity.icon,
      badge: unseen === null || unseen === 0 ? undefined : unseen,
    },
    ...sections.map((area) => ({
      key: area.key,
      label: area.label,
      icon: area.icon,
      badge: counted(badges.get(area.key)),
    })),
    ...unavailable.map((area) => ({
      key: area.key,
      label: area.label,
      icon: area.icon,
      unavailable: true,
    })),
    {
      key: catalogue.key,
      label: catalogue.label,
      icon: catalogue.icon,
      badge: updates > 0 ? updates : undefined,
    },
  ];
}

/** A count the window holds, in the two shapes a section draws. */
function counted(badge: BadgeCount | undefined): number | "dot" | undefined {
  if (badge === undefined) return undefined;
  return badge.kind === "dot" ? "dot" : badge.value;
}
