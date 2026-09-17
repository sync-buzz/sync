"use client";

import { useCallback, useMemo, useState } from "react";
import { ACTIVITY_AREA, EXTENSIONS_AREA } from "@/components/shell/areas";
import {
  ITEMS,
  InspectorBody,
  ItemRows,
  SECTIONS,
  WorkspaceBody,
  labelOfSection,
} from "@/components/prototype/mobile-content";
import {
  ColumnBand,
  PageMarks,
  Pager,
  type Page,
} from "@/components/shell/mobile-pager";
import { PanelFooter } from "@/components/shell/panel";
import { BandSlotsProvider, ColumnProvider } from "@/lib/shell-bands";
import { ProjectWheel } from "@/components/shell/project-wheel";
import { SectionsBar, type Section } from "@/components/shell/mobile-sections";
import { Shade, usePullDown } from "@/components/shell/mobile-shade";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import { FRAMES, type FrameId } from "@/lib/shell-frames";

/**
 * The window at 390 points, drawn with the window's own parts.
 *
 * That is the whole of what this prototype is for now that the arrangement has
 * been agreed: the pager, the band, the shade and the wheel are the shell's,
 * imported from where the phone actually uses them, and what this file supplies
 * is the one thing the shell gets from somewhere else — columns with something
 * in them. So the stand cannot drift from the product: a change to any of those
 * four shows up here on the next reload, and a change made only here does not
 * exist.
 *
 * It is a route rather than a screenshot because the questions it answers are
 * about movement: whether a swipe lands where a hand expects, whether the
 * threshold gives way at the right moment, whether the shade follows a finger.
 * None of those can be judged from a picture, and none of them can be judged on
 * a Mac without a phone in front of you — which is the other half of why this
 * exists: a simulator is not always at hand, and a browser at 390 points is.
 */
export function MobilePhone({
  frame,
  waiting,
}: {
  frame: FrameId;
  /**
   * Whether the window is drawn as waiting on the computer.
   *
   * A switch on the harness rather than a real wait, because a prototype asks
   * nothing of anybody: the line exists to be looked at in its place, which is
   * the one thing a drawing can settle about it.
   */
  waiting: boolean;
}) {
  const [project, setProject] = useState<string | null>(null);
  /** Which project the wheel is turned to when it is drawn again. */
  const [leftFrom, setLeftFrom] = useState<string | undefined>(undefined);
  const [section, setSection] = useState<string>(ACTIVITY_AREA.id);
  const [item, setItem] = useState(0);
  const [shadeOpen, setShadeOpen] = useState(false);
  const [goto, setGoto] = useState<{ page: number; id: number } | null>(null);
  const [at, setAt] = useState(0);

  const pull = usePullDown({
    enabled: (event) => {
      const target = event.target;
      const column =
        target instanceof Element ? target.closest("[data-scrolls]") : null;
      return column instanceof HTMLElement ? column.scrollTop <= 0 : true;
    },
    onOpen: () => setShadeOpen(true),
  });

  const sections = useMemo(() => bandOf(), []);
  const columns = FRAMES[frame];
  // The section to the left of the one showing, which is what the threshold
  // leads to everywhere but the first.
  const before = sections[sections.findIndex((one) => one.key === section) - 1];

  // The foot a package puts under its list, drawn the way the window draws one:
  // the package renders a `PanelFooter`, the shell offers a band, and the
  // controls appear there instead of in place. Without this the stand was
  // missing the one strip every real column has, which is exactly where the
  // gap being hunted turned out to live.
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

  const advance = useCallback((index: number) => {
    setItem(index);
    setGoto((asked) => ({ page: 1, id: (asked?.id ?? 0) + 1 }));
  }, []);

  if (project === null) {
    return (
      <ProjectWheel
        projects={PROJECTS}
        startAt={leftFrom}
        onOpen={(chosen) => {
          setLeftFrom(chosen.key);
          setProject(chosen.name);
        }}
      />
    );
  }

  const pages: Page[] = [
    ...(columns.navigator
      ? [
          {
            key: "navigator",
            body: (
              <ColumnProvider value="Navigator">
                <Scrolls>
                  <ItemRows activeIndex={item} onOpen={advance} />
                </Scrolls>
                <PanelFooter>
                  <button
                    type="button"
                    className="flex size-11 items-center justify-center rounded-(--radius-control) text-fg-secondary active:bg-hover"
                  >
                    +
                  </button>
                </PanelFooter>
              </ColumnProvider>
            ),
            band: <ColumnBand attach={bandRefs.Navigator} />,
          },
        ]
      : []),
    {
      key: "workspace",
      body: (
        <Scrolls>
          <WorkspaceBody
            title={columns.navigator ? ITEMS[item] : labelOfSection(section)}
          />
        </Scrolls>
      ),
      band: <ColumnBand attach={bandRefs.Workspace} />,
    },
    ...(columns.inspector
      ? [
          {
            key: "inspector",
            body: (
              <Scrolls>
                <InspectorBody />
              </Scrolls>
            ),
          },
        ]
      : []),
  ];

  return (
    <BandSlotsProvider value={bands}>
    <div
      className="relative flex h-full flex-col overflow-clip bg-workspace text-fg"
      // The same breath of light the window gives the field. Read from the
      // theme rather than written here, so the stand cannot be lit differently
      // from the product it is standing in for.
      style={{ backgroundImage: "var(--phone-horizon)" }}
      {...pull.handlers}
    >
      {waiting ? (
        <div
          role="progressbar"
          aria-label="Working"
          className="pointer-events-none absolute inset-x-0 top-0 z-30 h-0.5 overflow-hidden"
        >
          <div className="h-full w-1/3 bg-fg-tertiary animate-[indeterminate-progress_1.4s_var(--motion-ease)_infinite] motion-reduce:hidden" />
        </div>
      ) : null}

      <Pager
        pages={pages}
        behind={before?.label ?? "Projects"}
        returning={section}
        goto={goto}
        onPosition={setAt}
        onBehind={() => {
          if (before !== undefined) setSection(before.key);
          else setProject(null);
        }}
        hasBehind
      />

      <SectionsBar
        sections={sections}
        activeKey={section}
        marks={<PageMarks count={pages.length} at={at} />}
        onChoose={setSection}
      />

      <Shade
        open={shadeOpen}
        pulled={pull.pulled}
        project={project}
        sync={QUIET}
        onSearch={() => undefined}
        onOpenSync={() => undefined}
        onOpenSettings={() => undefined}
        onClose={() => setShadeOpen(false)}
        onLeave={() => setProject(null)}
      />
    </div>
    </BandSlotsProvider>
  );
}

/**
 * A column that scrolls itself, which in the window is the column's own doing.
 *
 * The pager hands a page a positioned box and asks nothing else of it, because
 * what goes in one is a package's and this shell does not scroll other people's
 * content. Here there is no package, so the stand supplies the scroller — and
 * marks it, so the pull-down can ask it whether it is at its top.
 */
function Scrolls({ children }: { children: React.ReactNode }) {
  return (
    <div data-scrolls className="absolute inset-0 overflow-y-auto overscroll-contain">
      {children}
    </div>
  );
}

/**
 * What the stand has instead of a computer's registry.
 *
 * Shapes and ordinals, as everywhere else in this folder: the moment a
 * plausible subject appears, the arrangement is judged as an arrangement *for
 * that subject*, and the thing being tested has quietly changed. The lengths
 * vary because a wheel that only ever holds short words is a wheel that has not
 * been tested.
 */
const PROJECTS = [
  { key: "one", name: "Project one", saying: "Four sections · in step" },
  {
    key: "two",
    name: "Project two, with a longer name",
    saying: "Two sections · three changes",
  },
  { key: "three", name: "Project three", saying: "Six sections · in step" },
  { key: "four", name: "Project four", saying: "One section · not read yet" },
  { key: "five", name: "Project five", saying: "Three sections · in step" },
] as const;

/**
 * A memory with nothing to report, which is what the indicator draws as
 * silence.
 *
 * Stated in full rather than cast through `unknown`: the shape is the window's,
 * and a stand that asserted its way past it would go on compiling after the
 * window had changed — which is the one thing this file exists not to do.
 */
const QUIET: SyncStatus = {
  state: null,
  transport: null,
  busy: null,
  error: null,
  overlaps: [],
  undoable: null,
  undoFetch: () => undefined,
  refresh: () => undefined,
  fetchNow: () => undefined,
  publishNow: () => undefined,
  setRemote: async () => undefined,
  dismissError: () => undefined,
  acknowledgeOverlaps: () => undefined,
};

/**
 * What the band holds, in the order the window puts it in.
 *
 * The two ends are the window's own and are read from where the window keeps
 * them rather than copied: what has changed since you last looked, and where
 * you decide what this project can do. Between them are the sections a project
 * brought, which here are shapes and ordinals.
 *
 * There are more of them than fit, and that is the measurement: a band that
 * only ever held four would be a tab bar that had not met a project yet.
 */
function bandOf(): readonly Section[] {
  return [
    {
      key: ACTIVITY_AREA.id,
      label: ACTIVITY_AREA.label,
      icon: ACTIVITY_AREA.icon,
      badge: 3,
    },
    ...SECTIONS.map((one) => ({
      key: one.key,
      label: one.label,
      icon: one.icon,
      badge: one.badge,
    })),
    {
      key: EXTENSIONS_AREA.id,
      label: EXTENSIONS_AREA.label,
      icon: EXTENSIONS_AREA.icon,
    },
  ];
}
