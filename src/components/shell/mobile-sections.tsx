"use client";

import { useEffect, useRef, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

import { edgeMask, useScrollFade } from "@/lib/mobile-scroll";
import { haptic } from "@/lib/haptic";

/**
 * The sections, along the bottom, where a thumb is.
 *
 * This is a tab bar in the shape of one and not in the rule it follows. A tab
 * bar holds between two and five permanent destinations, decided by the
 * application; this holds however many sections the project's packages brought,
 * in the order the project declares, and nothing here knows what any of them
 * is. So it scrolls: the ones that do not fit are reached by pushing the band
 * sideways, which is the one thing a real tab bar must never need.
 *
 * What is kept from the platform is where it is and how big it is — the band a
 * thumb reaches without the hand being re-gripped, and rows no smaller than a
 * finger. What is dropped is the idea that the set is the build's to decide,
 * because in this shell it is not.
 *
 * **Chosen is drawn by taking light away.** The band stands above the field;
 * the section being shown sinks back into it. On a black screen that is the
 * only emphasis available that does not cost brightness — there is nothing
 * darker than the field to tint a row with, and a row made brighter would
 * compete with the work above it.
 */

/**
 * How much light a section that is not being shown keeps.
 *
 * Measured rather than chosen: at 0.55 the label comes to 3.8:1 over the band
 * in the light appearance, under the 4.5 a phone is held to. The chosen row is
 * told apart by the dent it sits in, so this tier does not have to be faint to
 * do its job — it only has to be quieter.
 */
const RESTING = 0.62;

export interface Section {
  readonly key: string;
  readonly label: string;
  readonly icon: LucideIcon;
  /** A count, a plain mark, or nothing at all. */
  readonly badge?: number | "dot";
  /**
   * This project has the section and this machine has nothing to run it with.
   *
   * Drawn in its place in the order and refusing the press, rather than left
   * out: a project is one repository, and a phone quietly showing fewer
   * sections than the desk reads as a project that has lost something.
   */
  readonly unavailable?: boolean;
}

/**
 * How much light a section keeps: full while it is being shown, quieter while
 * it is not, and quieter still where it cannot be pressed at all.
 *
 * The last tier is below the contrast floor on purpose, and it is the one place
 * in this design that is: what it marks is a control that refuses, and a
 * disabled control that reads as available is the worse failure. The name is
 * still legible, and what it needs is said in full where somebody decides
 * about the package.
 */
function opacityOf(active: boolean, unavailable?: boolean): number {
  if (unavailable) return 0.35;
  return active ? 1 : RESTING;
}

export function SectionsBar({
  sections,
  activeKey,
  marks,
  onChoose,
}: {
  sections: readonly Section[];
  /** The section being shown, or nothing while the project is still opening. */
  activeKey: string | null;
  /**
   * Which column of the section is showing, drawn on this band's own top edge.
   *
   * It belongs to the pager and is drawn here because this band is already the
   * furniture at the foot of the screen: a strip of its own between the two
   * cost sixteen points and read as a margin.
   */
  marks?: ReactNode;
  onChoose: (key: string) => void;
}) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const chosen = useRef<HTMLButtonElement | null>(null);
  // The band holds more sections than fit and says so at its ends: the row
  // going out of light is what a person reads as *there are more this way*.
  // Nothing else could say it here — an arrow is a control nobody may press,
  // and a line is the one thing this design does not draw.
  const edges = useScrollFade(scroller, "x");
  const mask = edgeMask("x", edges);

  // A section can be chosen from somewhere other than this band — a link
  // followed, a project restored — and a band that did not move would be
  // showing a chosen row that is off its own end.
  useEffect(() => {
    chosen.current?.scrollIntoView({
      behavior: "smooth",
      block: "nearest",
      inline: "nearest",
    });
  }, [activeKey]);

  return (
    <div
      className="relative shrink-0 overflow-clip"
      style={{
        // The hardware's strip, and nothing added to it. What is under the
        // home indicator is this band's own surface rather than a margin, so
        // the phone reads as an interface that runs to the edge of the glass.
        paddingBottom: "var(--safe-bottom)",
        background: "var(--phone-bar)",
        boxShadow: "var(--phone-lift)",
        // The one place this interface blurs anything, and it blurs its own
        // content rather than the desktop: a band lying over a column that
        // scrolls under it is the platform's own idiom, and what is behind it
        // here is ours to blur. Nothing else in the window is glass.
        backdropFilter: "blur(20px)",
        borderTop: "1px solid var(--phone-bar-edge)",
      }}
    >
      {/* The change itself, crossing the band once in the direction reading
          goes. Keyed on the section, which is what makes it happen at all: a
          new key is a new element, and an animation that runs on arrival runs
          exactly when the thing it is about has arrived.

          In the band rather than around the row that was chosen — light around
          a row this close to the bottom edge is cut in half by it. What it is
          worth saying twice: the row stays sunk after this has gone, so the
          state is in the depth and only the *change* is in the light. */}
      <span
        key={activeKey ?? ""}
        aria-hidden
        className="pointer-events-none absolute inset-y-0 left-0 z-10 w-1/3 animate-[phone-sweep_420ms_var(--motion-ease)_1] motion-reduce:hidden"
        style={{
          background: `linear-gradient(90deg, transparent, var(--phone-sweep), transparent)`,
        }}
      />

      {/* On the band's own edge rather than above it: the marks are two points
          of ink, and what they need is a place to be, not a zone. */}
      {marks === undefined ? null : (
        <div className="flex h-4 items-center justify-center">{marks}</div>
      )}

      <div
        ref={scroller}
        className="flex gap-1.5 overflow-x-auto px-2 pt-0.5"
        // Proximity rather than mandatory: this band is read as much as it is
        // chosen from, and a list that fights a person looking along it is a
        // list that has forgotten which of the two it is for.
        //
        // The mask is on the row rather than on the band around it, and that is
        // the whole reason it is a mask: the band has a surface of its own, and
        // a veil of the field laid over the top would fade the surface instead
        // of what is scrolling along it.
        style={{
          scrollSnapType: "x proximity",
          scrollbarWidth: "none",
          maskImage: mask,
          WebkitMaskImage: mask,
        }}
      >
        {sections.map((section) => {
          const active = section.key === activeKey;
          const Icon = section.icon;
          return (
            <button
              key={section.key}
              ref={active ? chosen : undefined}
              type="button"
              aria-pressed={active}
              disabled={section.unavailable}
              // Said rather than shown, because what is missing is not obvious
              // from a dimmed row: the section is here, this machine is not
              // where it works.
              title={section.unavailable ? `${section.label} needs a computer` : undefined}
              onClick={() => {
                haptic();
                onChoose(section.key);
              }}
              className="relative flex h-14 w-[72px] shrink-0 snap-center flex-col items-center justify-center gap-0.5 rounded-(--radius-surface) px-1 transition-colors duration-(--motion-duration-fast) ease-shell"
              style={
                active
                  ? {
                      background: "var(--phone-sunken)",
                      // The sinking alone. A halo was tried and taken out: a
                      // row at the foot of the screen has a hard edge a few
                      // points below it, and light that is cut off half way
                      // round reads as a rendering fault rather than as
                      // emphasis.
                      boxShadow: "var(--phone-sunken-shadow)",
                    }
                  : undefined
              }
            >
              <Icon
                className="size-[22px] shrink-0"
                style={{ opacity: opacityOf(active, section.unavailable) }}
              />
              <span
                className="w-full truncate text-center text-[11px] leading-[14px]"
                style={{ opacity: opacityOf(active, section.unavailable) }}
              >
                {section.label}
              </span>

              {section.badge === undefined ? null : (
                <span
                  className={
                    section.badge === "dot"
                      ? "absolute top-2.5 right-4 size-1.5 rounded-full"
                      : "absolute top-1 right-2 text-[10px] leading-none font-semibold tabular-nums"
                  }
                  // A count is ink and not a shape. Filled, it was the
                  // brightest thing on the screen — brighter than the section
                  // it belonged to, brighter than the work above the band —
                  // and it sat on top of its own icon. This design has one
                  // inverted surface and it is the button that opens a
                  // project; a badge claiming the same emphasis says that four
                  // changed records outrank everything a person came here to
                  // read.
                  //
                  // The primary tokens, not the theme's `--color-*` names:
                  // those are substituted at build time and stay on the light
                  // desk appearance for ever. See the note in the wheel.
                  style={
                    section.badge === "dot"
                      ? { background: "var(--text-primary)" }
                      : { color: "var(--text-primary)" }
                  }
                >
                  {section.badge === "dot" ? (
                    <span className="sr-only">Something new</span>
                  ) : (
                    section.badge
                  )}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
