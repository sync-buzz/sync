"use client";

import { useCallback, useState } from "react";
import { Info, ListFilter, Plus, Search, SlidersHorizontal } from "lucide-react";
import type { FrameId } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";
import {
  BarButton,
  NavBar,
  Screen,
  Sheet,
  Stack,
} from "@/components/shell/mobile-chrome";
import {
  ITEMS,
  InspectorBody,
  ItemRows,
  PinnedRow,
  SectionRows,
  labelOfSection,
  WorkspaceBody,
} from "@/components/prototype/mobile-content";
import {
  hasInspector,
  levelsOf,
  type InspectorPresentation,
  type MobileLevel,
} from "@/lib/mobile-geometry";

/**
 * The window, at 390 points.
 *
 * The columns a frame declares are drawn one at a time and reached in order, so
 * what was a row of panels becomes a stack of screens. What has to be judged by
 * eye is the moving between them, which is why the push is a real one — it can
 * be dragged back from the leading edge and abandoned halfway, the way it can
 * everywhere else on the platform. A prototype whose transitions only ever
 * complete would be answering an easier question than the one that was asked.
 *
 * The state below is one screen deep on purpose. Which section, which item and
 * how far in is the whole of it; nothing is fetched, nothing is remembered, and
 * a screen exists only while it is on the stack. The window has good reasons to
 * keep an area mounted for ever, and none of them apply to a drawing.
 */
export function MobilePhone({
  frame,
  inspector,
}: {
  frame: FrameId;
  inspector: InspectorPresentation;
}) {
  const levels = levelsOf(frame, inspector);

  const [reached, setReached] = useState(0);
  // Clamped rather than reset: a frame changed under a stack deeper than the
  // new frame goes must not leave the phone on a screen that frame does not
  // have, and returning to the root on every change would make the frames
  // above the device harder to compare with each other.
  const depth = Math.min(reached, levels.length - 1);

  const [section, setSection] = useState<string | null>(null);
  const [item, setItem] = useState<number | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);

  const push = useCallback(() => setReached(depth + 1), [depth]);
  const pop = useCallback(() => setReached(Math.max(0, depth - 1)), [depth]);

  const sectionLabel = labelOfSection(section);
  const itemLabel = ITEMS[item ?? 0] ?? ITEMS[0];

  /** What a screen is called, which is also what the way back to it is called. */
  const titleOf = (level: MobileLevel): string => {
    switch (level) {
      case "sections":
        return "Project";
      case "navigator":
        return sectionLabel;
      case "workspace":
        return levels.includes("navigator") ? itemLabel : sectionLabel;
      case "inspector":
        return "Properties";
    }
  };

  const inspectorControl = !hasInspector(frame) ? null : (
    <BarButton
      label="Properties"
      icon={Info}
      onPress={inspector === "sheet" ? () => setSheetOpen(true) : push}
    />
  );

  const screenOf = (level: MobileLevel, index: number) => {
    const bar = (
      <NavBar
        title={titleOf(level)}
        back={index > 0 ? titleOf(levels[index - 1]) : undefined}
        onBack={pop}
        trailing={
          level === "sections" ? (
            <BarButton label="Search" icon={Search} />
          ) : level === "workspace" ? (
            inspectorControl
          ) : null
        }
      />
    );

    switch (level) {
      case "sections":
        return (
          <Screen
            className="bg-sidebar"
            bar={bar}
            foot={
              <PinnedRow
                onOpen={(key) => {
                  setSection(key);
                  push();
                }}
              />
            }
          >
            <SectionRows
              activeKey={section}
              onOpen={(key) => {
                setSection(key);
                push();
              }}
            />
          </Screen>
        );
      case "navigator":
        return (
          <Screen
            className="bg-panel"
            bar={bar}
            // The division the desktop column keeps at its foot: what acts on
            // the list on the leading edge, what decides how it is shown on the
            // trailing one, and nothing here writing what the list contains.
            toolbar={
              <>
                <BarButton label="Add" icon={Plus} />
                <BarButton label="Arrange" icon={SlidersHorizontal} />
                <BarButton label="Filter" icon={ListFilter} className="ml-auto" />
              </>
            }
          >
            <ItemRows
              activeIndex={item}
              onOpen={(index) => {
                setItem(index);
                push();
              }}
            />
          </Screen>
        );
      case "workspace":
        return (
          <Screen className="bg-workspace" bar={bar}>
            <WorkspaceBody title={titleOf("workspace")} />
          </Screen>
        );
      case "inspector":
        return (
          <Screen className="bg-panel" bar={bar}>
            <InspectorBody />
          </Screen>
        );
    }
  };

  return (
    <div className="relative h-full overflow-clip bg-workspace text-fg">
      {/* The screen a sheet is raised over shrinks back and takes its corners
          with it, so the two read as a stack seen edge on. Without it a sheet
          is a panel that appeared, and the screen under it looks merely
          covered rather than set down. */}
      <div
        className={cn(
          "h-full origin-top overflow-clip transition-[transform,border-radius] duration-[400ms] ease-shell motion-reduce:transition-none",
          sheetOpen && "translate-y-1 scale-[0.94] rounded-lg",
        )}
      >
        <Stack depth={depth} onPop={pop}>
          {levels.map(screenOf)}
        </Stack>
      </div>

      {hasInspector(frame) && inspector === "sheet" ? (
        <Sheet
          open={sheetOpen}
          title="Properties"
          onClose={() => setSheetOpen(false)}
        >
          <div className="h-full overflow-y-auto overscroll-contain">
            <InspectorBody />
          </div>
        </Sheet>
      ) : null}
    </div>
  );
}
