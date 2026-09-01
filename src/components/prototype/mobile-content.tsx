"use client";

import { Circle, Square, Triangle } from "lucide-react";
import { EXTENSIONS_AREA } from "@/components/shell/areas";
import {
  Row,
  RowSeparator,
  TextPlaceholder,
} from "@/components/prototype/mobile-chrome";

/**
 * What the prototype puts on its screens, and why none of it means anything.
 *
 * The shell names no language, no file type and no section, so a prototype of
 * the shell cannot name one either — the moment this file invents a plausible
 * subject, the arrangement is judged as an arrangement *for that subject* and
 * the thing being tested has quietly changed. The rows below are shapes and
 * ordinals: enough of them to scroll, varied enough in length to break a
 * layout that only works on short words, and about nothing at all.
 *
 * The one real name here is the row pinned at the foot of the first screen. It
 * is the window's own and is read from where the window keeps it, rather than
 * copied — a prototype that redrew it would be measuring a drawing.
 */

/** The first screen: what this project has, as the window's own column lists. */
export const SECTIONS = [
  { key: "one", label: "Section one", icon: Circle, badge: 12 as const },
  { key: "two", label: "Section two", icon: Square, badge: "dot" as const },
  { key: "three", label: "Section three", icon: Triangle, badge: undefined },
] as const;

export function SectionRows({
  activeKey,
  onOpen,
}: {
  activeKey: string | null;
  onOpen: (key: string) => void;
}) {
  return (
    <div>
      {SECTIONS.map((section, index) => (
        <div key={section.key}>
          {index > 0 ? <RowSeparator inset={48} /> : null}
          <Row
            icon={section.icon}
            label={section.label}
            badge={section.badge}
            selected={activeKey === section.key}
            leadsOn
            onPress={() => onOpen(section.key)}
          />
        </div>
      ))}
    </div>
  );
}

/** What a screen opened from the first one is called, pinned row included. */
export function labelOfSection(key: string | null): string {
  if (key === EXTENSIONS_AREA.id) return EXTENSIONS_AREA.label;
  return SECTIONS.find((one) => one.key === key)?.label ?? SECTIONS[0].label;
}

/**
 * The row at the foot of the first screen, pinned as it is on a desktop: the
 * sections above it are whatever is installed, and this row is where a person
 * decides that. Last in the scroller instead, it would read as the end of a
 * list it is not part of, and it would scroll away from the one place it is
 * always meant to be.
 */
export function PinnedRow({ onOpen }: { onOpen: (key: string) => void }) {
  return (
    <Row
      icon={EXTENSIONS_AREA.icon}
      label={EXTENSIONS_AREA.label}
      leadsOn
      onPress={() => onOpen(EXTENSIONS_AREA.id)}
    />
  );
}

/** The rows of the middle screen, and the titles the screen after it takes. */
export const ITEMS = [
  "Item one",
  "Item two, which carries a longer name than the rest of them",
  "Item three",
  "Item four",
  "Item five",
  "Item six",
  "Item seven",
  "Item eight",
  "Item nine",
  "Item ten",
  "Item eleven",
  "Item twelve",
] as const;

/** The middle screen of a frame that has one: what the section holds. */
export function ItemRows({
  activeIndex,
  onOpen,
}: {
  activeIndex: number | null;
  onOpen: (index: number) => void;
}) {
  return (
    <div>
      {ITEMS.map((label, index) => (
        <div key={label}>
          {index > 0 ? <RowSeparator /> : null}
          <Row
            label={label}
            detail={index % 3 === 0 ? "Second line" : undefined}
            selected={activeIndex === index}
            leadsOn
            onPress={() => onOpen(index)}
          />
        </div>
      ))}
    </div>
  );
}

/** The screen every frame has. */
export function WorkspaceBody({ title }: { title: string }) {
  return (
    <div className="space-y-5 px-4 py-4">
      <div className="space-y-2">
        <h2 className="text-[22px] leading-[28px] font-semibold">{title}</h2>
        <p className="text-[15px] leading-[20px] text-fg-secondary">
          Placeholder. This screen exists to be the width and the height of a
          workspace on a phone, and to be scrolled past the bar above it.
        </p>
      </div>
      <TextPlaceholder widths={[100, 96, 88, 100, 64]} />
      <TextPlaceholder widths={[92, 100, 78]} />
      <TextPlaceholder widths={[100, 84, 96, 58]} />
      <TextPlaceholder widths={[88, 100, 92, 70]} />
    </div>
  );
}

/** What is true of what the workspace is showing. */
export function InspectorBody() {
  const properties = [
    ["First property", "A value"],
    ["Second property", "Another value"],
    ["Third property", "A rather longer value than the others"],
    ["Fourth property", "A value"],
  ] as const;

  return (
    <div>
      {properties.map(([name, value], index) => (
        <div key={name}>
          {index > 0 ? <RowSeparator /> : null}
          <div className="flex min-h-11 items-center gap-3 px-4 py-2">
            <span className="w-[40%] shrink-0 text-[15px] leading-[20px] text-fg-secondary">
              {name}
            </span>
            <span className="min-w-0 flex-1 text-[17px] leading-[22px]">
              {value}
            </span>
          </div>
        </div>
      ))}
      <RowSeparator />
      <div className="space-y-2 px-4 py-4">
        <p className="text-[13px] leading-[18px] text-fg-tertiary">
          Everything above is invented and says nothing about the product.
        </p>
      </div>
    </div>
  );
}
