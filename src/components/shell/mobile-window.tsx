"use client";

import {
  useCallback,
  useMemo,
  useState,
  type PointerEvent,
} from "react";

import {
  ColumnBand,
  PageMarks,
  Pager,
  type Page,
} from "@/components/shell/mobile-pager";
import { SectionsBar, type Section } from "@/components/shell/mobile-sections";
import { Shade, usePullDown } from "@/components/shell/mobile-shade";
import { ProgressLine } from "@/components/shell/progress-line";
import type { MountedArea, UnavailableArea } from "@/lib/extension-host/areas";
import type { BadgeCount, Badges } from "@/lib/extension-host/badges";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import type { OpenProject } from "@/lib/project/types";
import { BandSlotsProvider } from "@/lib/shell-bands";
import { FRAMES } from "@/lib/shell-frames";

/**
 * The window with a project open, at the width of a phone.
 *
 * The Mac shows the columns of a frame side by side and lets a person fold the
 * ones they are not using. That arrangement has a floor — the workspace alone
 * asks for 500 points before anything stands beside it — and a phone is 390.
 * Below the floor the columns do not get tighter, they get *taken away*.
 *
 * So the same columns are arranged in the one way a phone has for more content
 * than fits: side by side in time. They become the pages of a pager, in the
 * order they stand in on a desk — what lists, then what is shown, then what is
 * true of it — and a swipe moves between them. Not a stack of pushes: a push
 * says *deeper*, and these columns are not deeper than each other.
 *
 * What is always drawn is the band of sections at the foot, because it is the
 * only chrome a thumb uses more than a few times an hour. Everything else that
 * belongs to the window rather than to a section — searching the corpus, what
 * the memory is doing, this phone's settings, the way back out — is pulled
 * down from the top in a shade.
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
  onOpenSettings,
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
   * one column is attached to and they are handed to three different pages.
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
  const [shadeOpen, setShadeOpen] = useState(false);
  /** Where the pager is, in pages, for the marks the band draws. */
  const [at, setAt] = useState(0);
  const [goto, setGoto] = useState<{ page: number; id: number } | null>(null);
  const frame = FRAMES[active?.frame ?? catalogue.frame];

  // Where each column's foot is drawn — the band at the bottom of its page
  // rather than a strip inside the column. Held as state for the reason the
  // window holds its panels that way: a portal needs its node to exist before
  // anything can be put through it.
  const [bands, setBands] = useState<{
    Navigator: HTMLElement | null;
    Workspace: HTMLElement | null;
  }>({ Navigator: null, Workspace: null });
  const bandRefs = useMemo(() => {
    const attach =
      (column: "Navigator" | "Workspace") => (element: HTMLElement | null) =>
        setBands((current) =>
          current[column] === element
            ? current
            : { ...current, [column]: element },
        );
    return { Navigator: attach("Navigator"), Workspace: attach("Workspace") };
  }, []);

  const pull = usePullDown({
    // The list under the finger, asked whether it has anywhere to scroll back
    // to. A list halfway down is a list being read, and what it is doing with
    // this gesture is scrolling back up — so the shade waits its turn.
    //
    // Found by walking up from what was touched rather than by asking the page,
    // because the page does not know: what scrolls is inside a column, and the
    // column belongs to a package.
    enabled: atTopOfWhateverScrolls,
    onOpen: () => setShadeOpen(true),
  });

  const band = useMemo(
    () => bandOf({ activity, sections, unavailable, catalogue, badges, updates, unseen }),
    [activity, sections, unavailable, catalogue, badges, updates, unseen],
  );

  const open = useCallback(
    (key: string) => {
      onSelectArea(key);
      setShadeOpen(false);
    },
    [onSelectArea],
  );

  /**
   * What lies to the left of the section being shown, in the band's own order.
   *
   * The sections a phone cannot run are stepped over rather than counted: they
   * are in the band so that a person can see the project is whole, and moving
   * onto one would be arriving at a section that refuses to draw.
   */
  const before = useMemo(() => {
    const reachable = band.filter((one) => one.unavailable !== true);
    const at = reachable.findIndex((one) => one.key === active?.key);
    return at > 0 ? reachable[at - 1] : null;
  }, [band, active?.key]);

  // Something was addressed at the area, so the area is what has to be looked
  // at. The window has already selected it; this is the half of that a phone
  // needs and a Mac does not — on a Mac the workspace is on the screen already.
  //
  // Read during the render that shows it rather than in an effect after it: an
  // effect would draw the page the person was on for one frame and then move
  // out from under them.
  const [answered, setAnswered] = useState(intent);
  if (intent !== answered) {
    setAnswered(intent);
    if (intent !== null) {
      setGoto((asked) => ({
        page: frame.navigator ? 1 : 0,
        id: (asked?.id ?? 0) + 1,
      }));
    }
  }

  // Choosing a row moves the pager on to what it opens. Read from the click
  // rather than told by the area, and that is the whole point: an area is a
  // package that has never heard of a phone. What the shell can see is that
  // something in a list was activated, and at this width that *is* what going
  // on to it means.
  //
  // Not the bands, though. The foot of a column holds controls that act on the
  // list — filtering it, adding to it — and a filter that threw the screen away
  // as it was applied would be unusable.
  const advance = useCallback(() => {
    setGoto((asked) => ({ page: 1, id: (asked?.id ?? 0) + 1 }));
  }, []);

  const pages: Page[] = [
    ...(frame.navigator
      ? [
          {
            key: "navigator",
            body: <AreaSlot attach={attachNavigator} onActivate={advance} />,
            band: <ColumnBand attach={bandRefs.Navigator} />,
          },
        ]
      : []),
    {
      key: "workspace",
      body: <AreaSlot attach={attachWorkspace} />,
      band: <ColumnBand attach={bandRefs.Workspace} />,
    },
    ...(frame.inspector
      ? [{ key: "inspector", body: <AreaSlot attach={attachInspector} /> }]
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
        {...pull.handlers}
      >
        {/* The window's own report that it is waiting, on the top edge of the
            screen rather than under a bar — there is no bar. It is the one
            thing drawn over the hardware's own inset, because a line two
            points tall under a notch is a line nobody sees. */}
        <div className="pointer-events-none absolute inset-x-0 top-0 z-30 h-0.5">
          <ProgressLine />
        </div>

        <Pager
          pages={pages}
          // The section to the left, or the way out where this is the leftmost
          // one. Naming it on the threshold is what makes the gesture readable
          // before it is finished: the strip says where the hand is going.
          behind={before?.label ?? "Projects"}
          returning={active?.key ?? catalogue.key}
          goto={goto}
          onPosition={setAt}
          onBehind={() => {
            if (before !== null) open(before.key);
            else onLeave?.();
          }}
          // Nothing behind the leftmost section on a machine that cannot leave
          // a project, which is every machine but a phone.
          hasBehind={before !== null || onLeave !== undefined}
        />

        <SectionsBar
          sections={band}
          activeKey={active?.key ?? null}
          marks={<PageMarks count={pages.length} at={at} />}
          onChoose={open}
        />

        <Shade
          open={shadeOpen}
          pulled={pull.pulled}
          project={project.name}
          sync={sync}
          onOpenSync={onOpenSync}
          onSearch={onSearch}
          onOpenSettings={onOpenSettings}
          onClose={() => setShadeOpen(false)}
          onLeave={onLeave}
        />
      </div>
    </BandSlotsProvider>
  );
}

/**
 * Whether the thing under this finger is already at the top of its own scroll.
 *
 * Walks up from what was touched to the first box that has somewhere to go,
 * because that is the box the gesture belongs to. Reaching the top of the tree
 * without finding one means nothing scrolls here, and the shade may have the
 * gesture.
 */
function atTopOfWhateverScrolls(event: PointerEvent<HTMLElement>): boolean {
  let node = event.target instanceof Element ? event.target : null;
  while (node !== null) {
    if (node instanceof HTMLElement && node.scrollHeight > node.clientHeight) {
      const how = getComputedStyle(node).overflowY;
      if (how === "auto" || how === "scroll") return node.scrollTop <= 0;
    }
    node = node.parentElement;
  }
  return true;
}

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

/** A count the window holds, in the two shapes a section draws. */
function counted(badge: BadgeCount | undefined): number | "dot" | undefined {
  if (badge === undefined) return undefined;
  return badge.kind === "dot" ? "dot" : badge.value;
}
