"use client";

import { Circle, Hexagon, Octagon, Square, Triangle } from "lucide-react";
import { ACTIVITY_AREA, EXTENSIONS_AREA } from "@/components/shell/areas";

/**
 * What the prototype puts inside the columns, and why none of it means
 * anything.
 *
 * The shell names no language, no file type and no section, so a prototype of
 * the shell cannot name one either — the moment this file invents a plausible
 * subject, the arrangement is judged as an arrangement *for that subject* and
 * the thing being tested has quietly changed. The rows below are shapes and
 * ordinals: enough of them to scroll, varied enough in length to break a
 * layout that only works on short words, and about nothing at all.
 *
 * The only real names here are the two the window owns at either end of the
 * band, and they are read from where the window keeps them rather than copied
 * — a prototype that redrew them would be measuring a drawing.
 *
 * **Nothing here is drawn with the desk's parts.** It began that way, borrowing
 * the window's rows, and what came out was a list of hairline-separated rows
 * with a chevron on each — a settings screen in a black skin. The separations
 * this design uses are space and depth, so a row is a block with air around it
 * and the chosen one is pressed into the field. Whether the window's own rows
 * should follow is the question these screens exist to answer.
 */

/**
 * The sections a project brought, as the band at the foot lists them.
 *
 * Five rather than three, because the band scrolls and a band that always fits
 * has not been tested: with the window's own two at either end this is seven
 * across a screen that holds five.
 */
export const SECTIONS = [
  { key: "one", label: "Section one", icon: Circle, badge: 12 as const },
  { key: "two", label: "Section two", icon: Square, badge: "dot" as const },
  { key: "three", label: "Section three", icon: Triangle, badge: undefined },
  { key: "four", label: "Section four", icon: Hexagon, badge: undefined },
  { key: "five", label: "Section five", icon: Octagon, badge: 2 as const },
] as const;

/** What a column is showing, where nothing in it is named yet. */
export function labelOfSection(key: string | null): string {
  if (key === EXTENSIONS_AREA.id) return EXTENSIONS_AREA.label;
  if (key === ACTIVITY_AREA.id) return ACTIVITY_AREA.label;
  return SECTIONS.find((one) => one.key === key)?.label ?? SECTIONS[0].label;
}

/** The rows of the first column, and the titles the one after it takes. */
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

/** What the section holds, where the frame has a column for it. */
export function ItemRows({
  activeIndex,
  onOpen,
}: {
  activeIndex: number | null;
  onOpen: (index: number) => void;
}) {
  return (
    <div className="flex flex-col gap-1 px-3 py-3">
      {ITEMS.map((label, index) => {
        const active = activeIndex === index;
        return (
          <button
            key={label}
            type="button"
            onClick={() => onOpen(index)}
            className="flex min-h-16 w-full flex-col justify-center gap-0.5 rounded-[16px] px-4 py-3 text-left"
            style={
              active
                ? {
                    background: "var(--phone-sunken)",
                    boxShadow: "var(--phone-sunken-shadow)",
                  }
                : undefined
            }
          >
            <span className="w-full truncate text-[17px] leading-[22px]">
              {label}
            </span>
            {index % 3 === 0 ? (
              <span className="w-full truncate text-[13px] leading-[18px] text-fg-tertiary">
                Second line
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

/** The column every frame has. */
export function WorkspaceBody({ title }: { title: string }) {
  return (
    <div className="space-y-6 px-5 pt-6 pb-10">
      <div className="space-y-2">
        {/* Large, and the first thing on the screen: with no bar above it,
            the name of what is being read is the screen's own opening line
            rather than a caption over the top of it. */}
        <h2
          className="text-[30px] leading-[36px] font-semibold tracking-[-0.01em]"
          style={{ textShadow: "var(--phone-title-glow)" }}
        >
          {title}
        </h2>
        <p className="text-[15px] leading-[21px] text-fg-secondary">
          Placeholder. This screen exists to be the width and the height of a
          workspace on a phone, and to be scrolled under the band at its foot.
        </p>
      </div>
      <TextPlaceholder widths={[100, 96, 88, 100, 64]} />
      <TextPlaceholder widths={[92, 100, 78]} />
      <TextPlaceholder widths={[100, 84, 96, 58]} />
      <TextPlaceholder widths={[88, 100, 92, 70]} />
    </div>
  );
}

/**
 * What is true of what the workspace is showing.
 *
 * A name above its value rather than beside it, which is what the width buys:
 * a value that runs long wraps into the column instead of being squeezed into
 * the half of it a label left over.
 */
export function InspectorBody() {
  const properties = [
    ["First property", "A value"],
    ["Second property", "Another value"],
    ["Third property", "A rather longer value than the others"],
    ["Fourth property", "A value"],
  ] as const;

  return (
    <div className="flex flex-col gap-5 px-5 pt-6 pb-10">
      {properties.map(([name, value]) => (
        <div key={name} className="flex flex-col gap-1">
          <span className="text-[11px] tracking-[0.16em] text-fg-tertiary uppercase">
            {name}
          </span>
          <span className="text-[17px] leading-[23px]">{value}</span>
        </div>
      ))}
      <p className="text-[13px] leading-[18px] text-fg-tertiary">
        Everything above is invented and says nothing about the product.
      </p>
    </div>
  );
}

/**
 * Text-shaped nothing: lines of the right height, in the right rhythm, saying
 * nothing at all.
 *
 * Written here rather than borrowed from the window, because what it is for is
 * to be the weight of prose on the screen, and that weight is this design's
 * scale rather than the desk's.
 */
function TextPlaceholder({ widths }: { widths: readonly number[] }) {
  return (
    <div className="flex flex-col gap-2.5">
      {widths.map((width, index) => (
        <div
          key={index}
          className="h-3 rounded-full bg-fg/8"
          style={{ width: `${width}%` }}
        />
      ))}
    </div>
  );
}
