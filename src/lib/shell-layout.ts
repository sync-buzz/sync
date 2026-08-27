"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  useGroupRef,
  usePanelRef,
  type GroupImperativeHandle,
  type PanelImperativeHandle,
} from "react-resizable-panels";
import type { RefObject } from "react";

/**
 * Layout state for the application shell.
 *
 * This module owns panel *behaviour* — roles, geometry, priority — and nothing
 * else. It is deliberately separate from product state: no domain model, no
 * backend. What it does keep is what a person did to the columns: which are
 * folded and how wide the rest were dragged, so that a window opened tomorrow
 * is the window that was closed today. Nothing of the project is in there.
 *
 * Panel geometry lives here rather than in the CSS token layer because it
 * drives the resizable panel group directly; duplicating it as CSS variables
 * would create two sources of truth for the same numbers.
 *
 * The layout is computed as a whole and applied in one operation, so what the
 * window shows is always a function of three inputs: the window width, the
 * panels folded on purpose, and the widths dragged by hand. Nothing is left to
 * the order in which individual panels happen to be told to resize.
 *
 * The named roles below are the extension point. Docking, presets and detached
 * windows can all be added by extending this module without touching the shell
 * components, which only ever address panels by role.
 */

export const COLLAPSIBLE_PANEL_ROLES = [
  "primarySidebar",
  "contextNavigator",
  "contextInspector",
] as const;

export type CollapsiblePanelRole = (typeof COLLAPSIBLE_PANEL_ROLES)[number];

export type PanelRole = CollapsiblePanelRole | "workspace";

/** Panel identifiers shared with the panel group in the DOM. */
export const PANEL_IDS = {
  primarySidebar: "primary-sidebar",
  contextNavigator: "context-navigator",
  workspace: "workspace",
  contextInspector: "context-inspector",
} as const satisfies Record<PanelRole, string>;

export interface CollapsiblePanelGeometry {
  readonly minWidth: number;
  readonly preferredWidth: number;
  readonly maxWidth: number;
  /**
   * The width at which the column keeps its rows but loses their labels: an
   * icon rail. `undefined` means the column has no such stage — it is either
   * open or closed, because what it lists has no icon to stand for it.
   *
   * The rail is a stage between open and closed rather than a narrower open
   * column, which is why it is one width rather than a range: everything in it
   * is centred on a single line of icons, and a rail two pixels wider would
   * only move that line off centre.
   */
  readonly railWidth?: number;
  /**
   * Panel-group width below which this panel gives up its space on its own.
   * Secondary panels collapse instead of letting every column shrink.
   * `undefined` means the panel is only ever closed on request.
   */
  readonly autoCollapseBelow?: number;
}

/**
 * The workspace is the dominant surface and is never given a maximum width.
 * Its minimum is the hard floor that every rule below protects.
 */
export const WORKSPACE_MIN_WIDTH = 500;

/**
 * Thresholds are derived from the widths below rather than guessed: a panel
 * gives up its space at exactly the width where keeping it would push the
 * workspace under `WORKSPACE_MIN_WIDTH`.
 *
 *   all four     200 + 220 + 500 + 300 + 3 edges = 1223
 *   no inspector       200 + 220 + 500 + 2 edges =  922
 *
 * Order matters: the shell reclaims width from the last role first, so the
 * list runs from the most durable column to the most optional one.
 */
export const PANEL_GEOMETRY: Record<
  CollapsiblePanelRole,
  CollapsiblePanelGeometry
> = {
  primarySidebar: {
    minWidth: 176,
    preferredWidth: 200,
    maxWidth: 280,
    railWidth: 52,
  },
  contextNavigator: {
    minWidth: 196,
    preferredWidth: 220,
    maxWidth: 340,
    autoCollapseBelow: 930,
  },
  contextInspector: {
    minWidth: 264,
    preferredWidth: 300,
    maxWidth: 440,
    autoCollapseBelow: 1230,
  },
};

export type CollapsedPanels = Record<CollapsiblePanelRole, boolean>;

/**
 * How much of a column is showing.
 *
 * Folding is two steps rather than one for the column that answers "where am
 * I": the first takes the labels away and leaves the icons, so the sections are
 * still there to be switched between, and the second takes the column away
 * altogether. A column with no `railWidth` has only the ends of that range.
 */
export const PANEL_STAGES = ["expanded", "rail", "hidden"] as const;

export type PanelStage = (typeof PANEL_STAGES)[number];

export type PanelStages = Record<CollapsiblePanelRole, PanelStage>;

/**
 * The stage a measured width puts a column in.
 *
 * The rail claims every width under the column's minimum rather than a band of
 * its own: a column narrower than the width it says it needs is one that cannot
 * show its labels, and that is the whole of what the rail is. It also means the
 * threshold is the same number the panel group already enforces, instead of a
 * second one that could disagree with it.
 */
function stageOfWidth(
  role: CollapsiblePanelRole,
  widthInPixels: number,
): PanelStage {
  if (widthInPixels === 0) return "hidden";
  const { minWidth, railWidth } = PANEL_GEOMETRY[role];
  if (railWidth !== undefined && widthInPixels < minWidth) return "rail";
  return "expanded";
}

/**
 * The stage a pointer at this distance from the leading edge of the panel group
 * is asking for.
 *
 * Each step claims everything up to the halfway point of the next one, so the
 * fold has no width between its steps: an edge dragged into the rail's range
 * *is* the rail, at one width, and the icons on it are never stretched by a
 * column caught between two stages. The same halfway rule is the one the panel
 * group applies to a collapsible panel, so the two agree about where a step
 * begins.
 */
function stageAtOffset(role: CollapsiblePanelRole, offset: number): PanelStage {
  const { minWidth, railWidth } = PANEL_GEOMETRY[role];
  if (railWidth === undefined) return offset < minWidth / 2 ? "hidden" : "expanded";
  if (offset < railWidth / 2) return "hidden";
  if (offset < (railWidth + minWidth) / 2) return "rail";
  return "expanded";
}

/** The columns that fold in steps, and whose edges therefore drive themselves. */
const STEPPED_ROLES = COLLAPSIBLE_PANEL_ROLES.filter(
  (role) => PANEL_GEOMETRY[role].railWidth !== undefined,
);

/**
 * How far either side of an edge counts as grabbing it. Wider than the edge
 * itself, and wider than the panel group's own hit area, so that no press near
 * a stepped edge is answered by the group's plain two-state drag instead.
 */
const EDGE_GRAB_RADIUS = 12;

/** The role a panel element belongs to, for the columns that can be folded. */
const ROLE_BY_PANEL_ID = new Map<string, CollapsiblePanelRole>(
  COLLAPSIBLE_PANEL_ROLES.map((role) => [PANEL_IDS[role], role]),
);

function roleOfPanel(element: Element | null | undefined) {
  // By id, not by the `data-panel` attribute: the panel group marks a panel
  // with `data-panel` and names it with `id`.
  if (!element?.hasAttribute("data-panel")) return null;
  return ROLE_BY_PANEL_ID.get(element.id) ?? null;
}

export interface ShellLayout {
  /** Attach to the `ResizablePanelGroup` via its `groupRef` prop. */
  readonly groupRef: RefObject<GroupImperativeHandle | null>;
  /**
   * Attach to the `ResizablePanelGroup` via its `elementRef` prop.
   *
   * A callback rather than a ref object, because the panel group is a node the
   * shell can replace: the layout has to follow whichever element is currently
   * in the document, and a ref object only ever reports the first one.
   */
  readonly groupElementRef: (element: HTMLDivElement | null) => void;
  /** Attach to the matching `ResizablePanel` via its `panelRef` prop. */
  readonly panelRefs: Record<
    CollapsiblePanelRole,
    RefObject<PanelImperativeHandle | null>
  >;
  /**
   * How much of each column is showing. `collapsed` is the same answer for the
   * controls that only ask whether a column is there at all.
   */
  readonly stages: PanelStages;
  /**
   * The stage each column is meant to be in, which is not always the one it is
   * showing: a fold in progress is measured, and this is what it is folding
   * towards. A panel declares the width it folds to from this.
   */
  readonly intended: PanelStages;
  readonly collapsed: CollapsedPanels;
  /**
   * False when the window is too narrow to hold the panel without pushing the
   * workspace under its minimum. Its control says so instead of doing nothing.
   */
  readonly canOpen: Record<CollapsiblePanelRole, boolean>;
  /** Report a size change coming from the panel group itself. */
  readonly reportPanelSize: (
    role: CollapsiblePanelRole,
    widthInPixels: number,
  ) => void;
  readonly togglePanel: (role: CollapsiblePanelRole) => void;
  readonly resetLayout: () => void;
}

const INITIAL_STAGES: PanelStages = {
  primarySidebar: "expanded",
  contextNavigator: "expanded",
  contextInspector: "expanded",
};

/** The stages put there on purpose. What is not named here was left alone. */
type ChosenStages = Partial<Record<CollapsiblePanelRole, PanelStage>>;

const NOTHING_CHOSEN: ChosenStages = {};

/** The widths given to columns by hand, by role. */
type ChosenWidths = Partial<Record<CollapsiblePanelRole, number>>;

/**
 * What a window remembers about its columns between runs: the stages folded to
 * on purpose, and the widths dragged by hand.
 *
 * It is kept in this window's own storage rather than in a file, for the same
 * reason the appearance is: the columns have to be laid out in the first frame
 * the window paints, and a value that arrives over IPC arrives after it.
 * Losing it costs the arrangement of three columns and nothing else — none of
 * the project is here.
 *
 * It belongs to the installation rather than to a project. A person arranges
 * the window they work in, not one window per folder, and a project opened for
 * the first time should look like the one they just closed.
 */
interface RememberedLayout {
  readonly stages: ChosenStages;
  readonly widths: ChosenWidths;
}

const NOTHING_REMEMBERED: RememberedLayout = { stages: {}, widths: {} };

const STORAGE_KEY = "sync.layout";

/**
 * Read back only what still makes sense. A stage no longer offered, a role no
 * longer here and a width outside what the column allows are all read as
 * nothing said about that column, because a stored value has no right to put
 * the window in a state the window cannot reach on its own.
 */
function readRememberedLayout(): RememberedLayout {
  if (typeof window === "undefined") return NOTHING_REMEMBERED;

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return NOTHING_REMEMBERED;

    const parsed = JSON.parse(raw) as Partial<RememberedLayout>;
    const stages: ChosenStages = {};
    const widths: ChosenWidths = {};

    for (const role of COLLAPSIBLE_PANEL_ROLES) {
      const { minWidth, maxWidth, railWidth } = PANEL_GEOMETRY[role];

      const stage = parsed.stages?.[role];
      if (
        stage !== undefined &&
        PANEL_STAGES.includes(stage) &&
        (stage !== "rail" || railWidth !== undefined)
      ) {
        stages[role] = stage;
      }

      const width = parsed.widths?.[role];
      if (typeof width === "number" && Number.isFinite(width)) {
        widths[role] = clamp(width, minWidth, maxWidth);
      }
    }

    return { stages, widths };
  } catch {
    // A window that cannot read what it wrote is a window with nothing
    // remembered, which is the state it was in the first time it opened.
    return NOTHING_REMEMBERED;
  }
}

function writeRememberedLayout(layout: RememberedLayout) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Storage a window is not allowed to write to costs it the arrangement of
    // its columns the next time it opens, and nothing while it is open. There
    // is nothing to report and nothing a person could do about it.
  }
}

/**
 * Why the layout is being computed, which is what decides whether a width
 * dragged by hand survives it.
 *
 * - `keep`: the ordinary pass. A hand-dragged width is kept unless the window
 *   is changing what the columns show, in which case they all return to their
 *   preferred widths rather than to arbitrary leftovers.
 * - `settle`: the pass at the end of a drag. Every width on screen is one
 *   somebody has just chosen, so all of them are kept.
 * - `normalize`: back to the preferred widths, whatever is on screen.
 */
type LayoutIntent = "keep" | "settle" | "normalize";

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

/**
 * The measured width of the panel group.
 *
 * A resize observer is used rather than the window's `resize` event because it
 * reports the width *after* the browser has laid the element out. Reacting to
 * the event instead would compute a layout for the new window width and apply
 * it while the group still had its old one.
 *
 * It takes the element itself rather than a ref to it, so that a group which is
 * replaced is measured again. Observing through a ref object would silently
 * leave the observer on the node that was taken out of the document, and that
 * node reports one last width of zero on its way out — which every threshold
 * below reads as a window too narrow to hold anything.
 *
 * It is `null` until the first measurement, which keeps the first client
 * render identical to the statically exported HTML.
 */
function useMeasuredWidth(element: HTMLElement | null) {
  const [width, setWidth] = useState<number | null>(null);

  useEffect(() => {
    if (!element) return;

    const observer = new ResizeObserver(([entry]) => {
      const measured = entry.contentRect.width;
      // Zero is the absence of a measurement, not a window with no width in
      // it. An element reports it while it is being laid out for the first
      // time and again while it is being removed; treating it as a real width
      // would answer "the window is too narrow" for the rest of the session.
      if (measured === 0) return;
      setWidth(measured);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);

  return width;
}

/**
 * @param unavailable Roles the selected area's frame does not use. They are
 *   held closed and cannot be opened — the column is still in the tree, because
 *   an area that is merely hidden must keep the DOM it built, and removing the
 *   panel would take that with it.
 */

export function useShellLayout(
  unavailable: readonly CollapsiblePanelRole[] = [],
): ShellLayout {
  const groupRef = useGroupRef();
  // State rather than a ref: replacing the group has to recompute the layout,
  // and a ref changing does not.
  const [groupElement, setGroupElement] = useState<HTMLDivElement | null>(null);

  const primarySidebar = usePanelRef();
  const contextNavigator = usePanelRef();
  const contextInspector = usePanelRef();

  const panelRefs = useMemo(
    () => ({ primarySidebar, contextNavigator, contextInspector }),
    [primarySidebar, contextNavigator, contextInspector],
  );

  /** What each column is showing, as measured. */
  const [stages, setStages] = useState<PanelStages>(() => INITIAL_STAGES);

  const collapsed = useMemo(
    () =>
      Object.fromEntries(
        COLLAPSIBLE_PANEL_ROLES.map((role) => [role, stages[role] === "hidden"]),
      ) as CollapsedPanels,
    [stages],
  );

  /**
   * The stage each column was put in on purpose, by its control or by the hand
   * that dragged its edge. A role absent from the record has been left alone
   * and is open.
   *
   * State rather than a ref, because the panel group has to be told what the
   * bottom of a column's range is *before* it is asked to go there: the width a
   * collapsible panel takes when it is folded is a prop, and a layout applied
   * in the same pass that changes it would be validated against the old one.
   */
  // Read once, on the way in. Whatever the window was left as is where it
  // starts, and from then on this hook is the only thing that decides.
  const [remembered] = useState(readRememberedLayout);
  const [chosen, setChosen] = useState<ChosenStages>(remembered.stages);

  /**
   * The width each column was dragged to, which stands in for its preferred
   * width from then on. It is what a column comes back to after being folded
   * and shown again, and what it opens at the next time the window is opened.
   */
  const chosenWidths = useRef<ChosenWidths>({ ...remembered.widths });

  /**
   * Whether what has just happened is worth remembering. Set by the things a
   * person does — folding a column, letting go of an edge, restoring a width —
   * and not by the hundred passes a single drag makes on its way there.
   */
  const shouldRemember = useRef(false);

  /**
   * The stage to come back to when a hidden column is shown again.
   *
   * A column folded to its rail and then hidden comes back as a rail: hiding is
   * the second step of the fold, and coming back to the step before it is the
   * only answer that makes it reversible.
   */
  const restoreTo = useRef(new Map<CollapsiblePanelRole, PanelStage>());

  // Encoded, and sorted so that naming the same two roles in the other order is
  // the same answer rather than a reason to lay the window out again.
  const unavailableKey = [...unavailable].sort().join();
  const unavailableRoles = useMemo(
    () => new Set(unavailableKey === "" ? [] : unavailableKey.split(",")),
    [unavailableKey],
  );

  const groupWidth = useMeasuredWidth(groupElement);

  const canOpen = useMemo(() => {
    const entries = COLLAPSIBLE_PANEL_ROLES.map((role) => {
      const threshold = PANEL_GEOMETRY[role].autoCollapseBelow;
      const fits =
        threshold === undefined ||
        groupWidth === null ||
        groupWidth >= threshold;
      // A column the frame does not use cannot be opened at all, and its
      // control says so the same way it does when the window is too narrow.
      return [role, fits && !unavailableRoles.has(role)] as const;
    });
    return Object.fromEntries(entries) as Record<CollapsiblePanelRole, boolean>;
  }, [groupWidth, unavailableRoles]);

  /**
   * The stage each column is meant to be in: what was chosen, once the frame
   * and the width of the window have had their say. This is what the layout is
   * computed from, and what each panel's folded width is declared from.
   */
  const intended = useMemo(() => {
    const entries = COLLAPSIBLE_PANEL_ROLES.map((role) => {
      // The frame's answer comes first: a column it does not use is closed
      // whatever the width says and whatever was chosen by hand, and the choice
      // is remembered for when an area that has the column is selected again.
      if (unavailableRoles.has(role)) return [role, "hidden"] as const;
      const choice = chosen[role];
      if (choice === "hidden") return [role, "hidden"] as const;
      if (!canOpen[role]) return [role, "hidden"] as const;
      return [role, choice ?? "expanded"] as const;
    });
    return Object.fromEntries(entries) as PanelStages;
  }, [canOpen, chosen, unavailableRoles]);

  /**
   * The width the hand is asking for, while an edge that drives itself is being
   * dragged. It stands in for the width the panel currently has, which during a
   * drag is one step behind the pointer.
   */
  const dragged = useRef<{
    role: CollapsiblePanelRole;
    width: number;
  } | null>(null);

  /**
   * The widths the other columns had when a drag started.
   *
   * Held for the length of the gesture because the panel group hands the space
   * a folding column gives up to whichever columns are beside it, and the next
   * pass would otherwise read that as a width somebody chose. One edge moving
   * is one column changing; the rest stay where they were put.
   */
  const frozen = useRef<Partial<Record<CollapsiblePanelRole, number>> | null>(
    null,
  );

  /** The stages of the previous pass, used to detect a transition. */
  const previousStages = useRef("");

  const applyLayoutRules = useCallback(
    (intent: LayoutIntent = "keep") => {
      const group = groupRef.current;
      if (!group || !groupElement) return false;

      // The edges between columns take real space; measure them rather than
      // assuming a width that a style change could invalidate.
      let edgeWidth = 0;
      for (const edge of groupElement.querySelectorAll("[data-separator]")) {
        edgeWidth += (edge as HTMLElement).offsetWidth;
      }

      const available = groupElement.clientWidth - edgeWidth;
      if (available <= 0) return false;

      const stagesNow = COLLAPSIBLE_PANEL_ROLES.map(
        (role) => `${role}:${intended[role]}`,
      ).join();
      const isTransition = stagesNow !== previousStages.current;
      previousStages.current = stagesNow;

      // Start from the preferred widths, keeping a width chosen by hand as
      // long as the window is not changing what the columns are showing.
      const widths = {} as Record<CollapsiblePanelRole, number>;
      for (const role of COLLAPSIBLE_PANEL_ROLES) {
        const { minWidth, preferredWidth, maxWidth, railWidth } =
          PANEL_GEOMETRY[role];

        if (intended[role] === "hidden") {
          widths[role] = 0;
          continue;
        }
        if (intended[role] === "rail" && railWidth !== undefined) {
          widths[role] = railWidth;
          continue;
        }
        const current =
          dragged.current?.role === role
            ? dragged.current.width
            : (frozen.current?.[role] ??
              panelRefs[role].current?.getSize().inPixels ??
              0);
        const keepCurrent =
          current > 0 &&
          (intent === "settle" || (intent === "keep" && !isTransition));
        widths[role] = clamp(
          keepCurrent ? current : (chosenWidths.current[role] ?? preferredWidth),
          minWidth,
          maxWidth,
        );
      }

      const sideTotal = () =>
        COLLAPSIBLE_PANEL_ROLES.reduce((sum, role) => sum + widths[role], 0);

      // Protect the workspace by taking width back from the most optional
      // column first, never by shrinking every column a little. A column
      // already folded to its rail has nothing left to give.
      let deficit = WORKSPACE_MIN_WIDTH - (available - sideTotal());
      for (const role of [...COLLAPSIBLE_PANEL_ROLES].reverse()) {
        if (deficit <= 0) break;
        if (widths[role] === 0 || intended[role] === "rail") continue;
        const slack = widths[role] - PANEL_GEOMETRY[role].minWidth;
        const taken = Math.min(slack, deficit);
        widths[role] -= taken;
        deficit -= taken;
      }

      const workspaceWidth = Math.max(available - sideTotal(), 0);
      const asPercentage = (width: number) => (width / available) * 100;

      group.setLayout({
        [PANEL_IDS.primarySidebar]: asPercentage(widths.primarySidebar),
        [PANEL_IDS.contextNavigator]: asPercentage(widths.contextNavigator),
        [PANEL_IDS.workspace]: asPercentage(workspaceWidth),
        [PANEL_IDS.contextInspector]: asPercentage(widths.contextInspector),
      });

      return isTransition;
    },
    [groupElement, groupRef, intended, panelRefs],
  );

  /**
   * A pass of the rules, and why it is being run.
   *
   * The pass is asked for rather than performed, and happens in an effect once
   * the render that carries the new choice has been committed. That is what
   * lets a column be hidden at all: the width it folds to is a prop of the
   * panel, and the group validates any layout it is given against the props it
   * currently has.
   *
   * The counter is what makes asking twice for the same thing happen twice —
   * restoring the default widths changes no choice, and would otherwise be a
   * request the effect could not see.
   */
  const pendingIntent = useRef<LayoutIntent>("keep");
  const [passes, setPasses] = useState(0);

  const requestLayout = useCallback((intent: LayoutIntent = "keep") => {
    pendingIntent.current = intent;
    setPasses((count) => count + 1);
  }, []);

  useEffect(() => {
    const intent = pendingIntent.current;
    pendingIntent.current = "keep";
    const changedStages = applyLayoutRules(intent);

    // A column that has just changed what it folds to is a change of
    // constraints as well as of layout, and the panel group answers one of
    // those on its own: it hands the space the column gave up to the columns
    // beside it, after this pass has already said where that space should go.
    // The pass is therefore made again on the next frame, when there is
    // something to correct. A drag corrects itself as it goes and is left
    // alone.
    if (!changedStages || intent !== "keep") return;
    const frame = requestAnimationFrame(() => applyLayoutRules("normalize"));
    return () => cancelAnimationFrame(frame);
  }, [applyLayoutRules, groupWidth, passes]);

  /**
   * Write down what the columns were left as.
   *
   * After the pass rather than with the choice, because a choice is made in
   * pieces — a stage here, a width there — and what is worth keeping is the
   * arrangement they add up to. The pass counter is in the dependencies so that
   * a drag which changed only a width is written down too.
   */
  useEffect(() => {
    if (!shouldRemember.current) return;
    shouldRemember.current = false;
    writeRememberedLayout({
      stages: chosen,
      widths: { ...chosenWidths.current },
    });
  }, [chosen, passes]);

  const reportPanelSize = useCallback(
    (role: CollapsiblePanelRole, widthInPixels: number) => {
      // What a column shows follows the width it actually has, not the width it
      // was asked to have: the panel group folds a column to its rail on its
      // own while the pointer is still down, and the labels have to go with it.
      const stage = stageOfWidth(role, widthInPixels);
      setStages((current) =>
        current[role] === stage ? current : { ...current, [role]: stage },
      );
    },
    [],
  );

  const togglePanel = useCallback(
    (role: CollapsiblePanelRole) => {
      // Nothing to toggle: the selected area's frame does not have this column.
      if (unavailableRoles.has(role)) return;

      if (intended[role] === "hidden") {
        if (!canOpen[role]) return;
        // Back to what it was before it was hidden, which for a column folded
        // to its rail is the rail.
        setChosen((current) => ({
          ...current,
          [role]: restoreTo.current.get(role) ?? "expanded",
        }));
      } else {
        restoreTo.current.set(role, intended[role]);
        setChosen((current) => ({ ...current, [role]: "hidden" }));
      }
      shouldRemember.current = true;
      requestLayout();
    },
    [canOpen, intended, requestLayout, unavailableRoles],
  );

  const resetLayout = useCallback(() => {
    restoreTo.current.clear();
    chosenWidths.current = {};
    setChosen(NOTHING_CHOSEN);
    shouldRemember.current = true;
    requestLayout("normalize");
  }, [requestLayout]);

  /**
   * The stage the columns either side of an edge are left in.
   *
   * A drag is a choice: a column folded by hand has to stay folded when the
   * window is next resized. Only the columns the edge actually moves are
   * recorded, so a column the window closed by itself is not mistaken for one
   * closed on purpose and still comes back when there is room for it again.
   */
  const settleRoles = useCallback(
    (roles: readonly CollapsiblePanelRole[]) => {
      const settled: ChosenStages = {};
      for (const role of roles) {
        const width = panelRefs[role].current?.getSize().inPixels ?? 0;
        const stage = stageOfWidth(role, width);
        settled[role] = stage;
        if (stage !== "hidden") restoreTo.current.set(role, stage);
        // Only an open column says anything about width. A rail is one width
        // and nothing is none, and neither should overwrite the width the
        // column is to come back to.
        if (stage === "expanded") chosenWidths.current[role] = width;
      }
      setChosen((current) => ({ ...current, ...settled }));
      shouldRemember.current = true;
      requestLayout("settle");
    },
    [panelRefs, requestLayout],
  );

  /**
   * Dragging the edge of a column that folds in steps.
   *
   * The panel group folds a collapsible column on its own, and it does it well
   * — but it knows one folded width, and this column has two: its rail and
   * nothing at all. So this edge is driven from here instead, and the group is
   * told the result.
   *
   * Taking the gesture from the group has to be done at the window, in the
   * capture phase: the group listens for a press on the document and decides
   * whether it was on an edge by where it landed rather than by what it landed
   * on, so an overlay of our own would not keep it out. Stopping the event
   * before the document sees it is the one place where this edge can be ours
   * and every other edge can stay the group's.
   *
   * Each step of the ladder is applied by choosing it rather than by setting a
   * width. The width a column folds to is a prop of its panel, and the group
   * validates any layout against the props it has: asking for nothing while the
   * panel still says it folds to a rail is answered with the rail. Choosing the
   * stage instead lets the prop and the layout arrive together, one render
   * apart, which is the order that works.
   */
  useEffect(() => {
    if (!groupElement) return;

    const edgeOf = (role: CollapsiblePanelRole) => {
      const panel = groupElement.querySelector(
        `[data-panel]#${PANEL_IDS[role]}`,
      );
      const edge = panel?.nextElementSibling;
      return edge instanceof HTMLElement && edge.hasAttribute("data-separator")
        ? edge
        : null;
    };

    /** How far the pointer is from the middle of an edge. */
    const distanceTo = (edge: Element, event: { clientX: number }) => {
      const rect = edge.getBoundingClientRect();
      return Math.abs(event.clientX - (rect.left + rect.right) / 2);
    };

    /**
     * The stepped edge under this press, if the press is on one and on no
     * nearer edge than it.
     *
     * A column folded to nothing leaves its own edge a pixel from its
     * neighbour's, and the wider reach this edge is grabbed by would otherwise
     * swallow presses meant for the column beside it — which, being closed, has
     * only that edge to be dragged back out by. The nearest edge is the one
     * being reached for, whichever of the two it belongs to.
     */
    const grabbedRole = (event: { clientX: number; clientY: number }) => {
      const band = groupElement.getBoundingClientRect();
      if (event.clientY < band.top || event.clientY > band.bottom) return null;

      let grabbed: CollapsiblePanelRole | null = null;
      let shortest = EDGE_GRAB_RADIUS;
      for (const role of STEPPED_ROLES) {
        const edge = edgeOf(role);
        if (!edge) continue;
        const distance = distanceTo(edge, event);
        if (distance > shortest) continue;
        grabbed = role;
        shortest = distance;
      }
      if (grabbed === null) return null;

      for (const edge of groupElement.querySelectorAll("[data-separator]")) {
        if (edge === edgeOf(grabbed)) continue;
        if (distanceTo(edge, event) < shortest) return null;
      }
      return grabbed;
    };

    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0) return;
      const role = grabbedRole(event);
      if (role === null) return;

      // The group is listening for this on the document; it never arrives.
      event.stopImmediatePropagation();
      event.preventDefault();

      const { minWidth, maxWidth } = PANEL_GEOMETRY[role];
      frozen.current = Object.fromEntries(
        COLLAPSIBLE_PANEL_ROLES.filter((other) => other !== role).map(
          (other) => [other, panelRefs[other].current?.getSize().inPixels ?? 0],
        ),
      );
      // The cursor belongs to the gesture rather than to what is under it, so
      // it is held for as long as the hand is down and given back after.
      const cursorWas = document.body.style.cursor;
      // The cursor the panel group itself shows over an edge, so that taking
      // the gesture from it does not change what the hand is looking at.
      document.body.style.cursor = "ew-resize";

      const onMove = (moveEvent: PointerEvent) => {
        const offset =
          moveEvent.clientX - groupElement.getBoundingClientRect().left;
        const stage = stageAtOffset(role, offset);
        dragged.current =
          stage === "expanded"
            ? { role, width: clamp(offset, minWidth, maxWidth) }
            : null;
        setChosen((current) =>
          current[role] === stage ? current : { ...current, [role]: stage },
        );
        requestLayout("settle");
      };

      const onUp = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onUp);
        document.body.style.cursor = cursorWas;
        dragged.current = null;
        frozen.current = null;
        // Through the same path as any other edge: the stage the column is left
        // in, the width it is left at, and both worth remembering.
        settleRoles([role]);
      };

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onUp);
    };

    // Restoring the default width is the group's gesture, and it stays the
    // group's — but the choice it undoes is held here, so it is undone here.
    const onDoubleClick = (event: MouseEvent) => {
      const role = grabbedRole(event);
      if (role === null) return;
      // The gesture means "this column, as it was designed", so the width given
      // to it by hand is given up along with the fold.
      delete chosenWidths.current[role];
      restoreTo.current.delete(role);
      setChosen((current) => ({ ...current, [role]: "expanded" }));
      shouldRemember.current = true;
      requestLayout("normalize");
    };

    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("dblclick", onDoubleClick, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("dblclick", onDoubleClick, true);
    };
  }, [groupElement, panelRefs, requestLayout, settleRoles]);

  /**
   * Dragging the edge of a column that does not fold in steps, and moving any
   * edge with the arrow keys. Both are the panel group's to carry out; what
   * they leave behind is a choice, and that is recorded here.
   */
  useEffect(() => {
    if (!groupElement) return;

    const rolesBeside = (edge: Element) =>
      [
        roleOfPanel(edge.previousElementSibling),
        roleOfPanel(edge.nextElementSibling),
      ].filter((role) => role !== null);

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const edge = target.closest("[data-separator]");
      if (!edge || !groupElement.contains(edge)) return;

      const touched = rolesBeside(edge);
      if (touched.length === 0) return;

      const settle = () => {
        window.removeEventListener("pointerup", settle);
        window.removeEventListener("pointercancel", settle);
        settleRoles(touched);
      };

      window.addEventListener("pointerup", settle);
      window.addEventListener("pointercancel", settle);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const edge = target.closest("[data-separator]");
      if (!edge) return;
      settleRoles(rolesBeside(edge));
    };

    groupElement.addEventListener("pointerdown", onPointerDown);
    groupElement.addEventListener("keyup", onKeyUp);
    return () => {
      groupElement.removeEventListener("pointerdown", onPointerDown);
      groupElement.removeEventListener("keyup", onKeyUp);
    };
  }, [groupElement, settleRoles]);

  return {
    groupRef,
    groupElementRef: setGroupElement,
    panelRefs,
    stages,
    intended,
    collapsed,
    canOpen,
    reportPanelSize,
    togglePanel,
    resetLayout,
  };
}
