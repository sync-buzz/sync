"use client";

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { edgeMask, useScrollAt, useScrollFade } from "@/lib/mobile-scroll";

/**
 * The wheel a phone chooses a project with.
 *
 * A wheel rather than a list, and the difference is not decoration. A list of
 * names is read; a wheel has one name *at* it, and everything else is context
 * for that one. So the enlargement is given a job — the project in the middle
 * is the only one the window says anything about, and the rest are names
 * waiting their turn. An enlargement that only enlarged would be the screen
 * shouting at a row for no reason.
 *
 * The button at the foot says which project it opens rather than the word
 * `Select`, because by the time a thumb reaches it the wheel has scrolled and
 * the eye is at the bottom of the screen. Naming the project is what makes the
 * control answerable without looking back up. Tapping the name in the middle
 * does the same thing: the button exists for the thumb, not instead of the
 * list.
 */

/** How tall one name is in the wheel. Fixed, so the maths below needs no DOM. */
const ITEM = 88;

/** As faint as a name is allowed to get before it stops passing contrast. */
const FAINTEST = 0.62;

/** One name on the wheel, and what the window can say about it. */
export interface Turnable {
  /** What the caller is handed back. Opaque here. */
  readonly key: string;
  readonly name: string;
  /**
   * What is true of this project, in a line — and nothing while nobody has
   * asked. It is said about the one in the middle only, which is what gives
   * the enlargement a job beyond being large.
   */
  readonly saying?: string;
}

export function ProjectWheel({
  projects,
  startAt,
  busy,
  failure,
  trailing,
  onOpen,
}: {
  projects: readonly Turnable[];
  /**
   * Which name the wheel is turned to when it appears, and nothing to mean the
   * first.
   *
   * A wheel that always starts at the top starts with its whole upper half
   * empty, because there is nothing above the first name — half a screen of
   * black under one word, which reads as a screen that has not finished
   * loading. Turned to the project somebody just left, it opens with names
   * above and below the one in the middle: a wheel that looks like a wheel,
   * and the name most likely to be wanted already at the mark.
   */
  readonly startAt?: string;
  /** A project is being opened, so nothing else may be started meanwhile. */
  busy?: boolean;
  /**
   * What the computer said when it refused, drawn above the band.
   *
   * Over the list rather than instead of it: a project that would not open
   * leaves the other projects standing, and a screen that replaced them with
   * an apology would take away the thing to try next.
   */
  failure?: string | null;
  /** One control that is not about a project, at the end of the band. */
  trailing?: ReactNode;
  onOpen: (project: Turnable) => void;
}) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const at = useScrollAt(scroller, "y");
  /**
   * Half a screen of nothing at each end, so the first and the last name can
   * reach the middle. Measured rather than written as `50%`, which is the trap
   * this began in: a percentage padding is a percentage of the box's *width*,
   * whichever edge it is on, so the wheel centred itself against 390 points on
   * a screen 844 tall and every name settled a third of the way up.
   */
  const [reach, setReach] = useState(0);

  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null) return;
    const measure = () => setReach(Math.max(0, (box.clientHeight - ITEM) / 2));
    measure();
    const watching = new ResizeObserver(measure);
    watching.observe(box);
    return () => watching.disconnect();
  }, []);

  /**
   * Turn the wheel to where it starts, once the padding above it exists.
   *
   * After the measurement rather than with it: the scroll position being asked
   * for is a position inside a box whose top half is that padding, and a box
   * that has not got it yet cannot be scrolled that far. Once only — a person
   * turning the wheel and a list arriving again would otherwise fight, and the
   * hand has to win.
   */
  const turned = useRef(false);
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null || reach === 0 || turned.current) return;
    const index = projects.findIndex((one) => one.key === startAt);
    turned.current = true;
    if (index > 0) box.scrollTo({ top: index * ITEM });
  }, [projects, startAt, reach]);

  // What the wheel does at its two ends, which is what every scroller on this
  // phone does: the names nearest the edge go out of light rather than being
  // cut by it. The button below is a surface with an edge of its own, and a
  // name sliced in half against it read as a list that had been trimmed to fit.
  const edges = useScrollFade(scroller, "y");
  const mask = edgeMask("y", edges);

  // Which name the wheel is at, and how far each one is from it — in items
  // rather than in pixels, because what reads it is a scale.
  const middle = at / ITEM;
  const chosen = projects[clamp(Math.round(middle), 0, projects.length - 1)];

  return (
    <div
      className="flex min-h-0 flex-1 flex-col bg-workspace text-fg"
      // The room the field is in: a breath of light at the top edge in the
      // dark appearance, nothing at all on paper. It is what keeps a black
      // screen from reading as a void rather than as a surface — see the
      // horizon in `mobile.css`.
      style={{ backgroundImage: "var(--phone-horizon)" }}
    >
      <p
        className="shrink-0 px-6 pb-2 text-xs tracking-[0.2em] text-fg-tertiary uppercase"
        style={{ paddingTop: "max(20px, var(--safe-top))" }}
      >
        Projects
      </p>

      <div
        ref={scroller}
        className="min-h-0 flex-1 snap-y snap-mandatory overflow-y-auto overscroll-contain"
        style={{
          paddingBlock: reach,
          scrollbarWidth: "none",
          maskImage: mask,
          WebkitMaskImage: mask,
        }}
      >
        {projects.map((project, index) => {
          const away = Math.abs(index - middle);
          return (
            <button
              key={project.key}
              type="button"
              disabled={busy}
              onClick={() => onOpen(project)}
              className="flex w-full snap-center flex-col items-start justify-center px-6 text-left"
              style={{
                height: ITEM,
                // Nothing is animated here: the finger is the clock. A
                // transition on top of a scroll position would lag behind the
                // hand by exactly its own duration.
                // Depth is carried by size rather than by fading, which is
                // the design's own rule and here it is also the accessible
                // one: a name dimmed far enough to read as *behind* is a name
                // that fails contrast. So the scale does most of the work.
                transform: `scale(${1 - clamp(away, 0, 2) * 0.19})`,
                transformOrigin: "left center",
                // And the fading stops where it stops being text. Measured,
                // not judged: over white, this tier at 0.55 comes to 4.0:1 and
                // at 0.62 to 5.1:1, so the floor is the second number. A name
                // two turns away is context somebody is scanning, not
                // decoration.
                opacity: Math.max(FAINTEST, 1 - clamp(away, 0, 2) * 0.19),
              }}
            >
              <span
                className="w-full truncate text-[28px] leading-[34px] font-semibold"
                style={{
                  // Only on the one in the middle, and only where the
                  // appearance has a glow at all — on white it is `none` and
                  // this line costs nothing.
                  textShadow:
                    away < 0.5 ? "var(--phone-title-glow)" : undefined,
                }}
              >
                {project.name}
              </span>
              <span
                className="w-full truncate text-[13px] leading-[18px] text-fg-tertiary"
                // What the window knows about this project, and it says it
                // about one project at a time. Faded by distance rather than
                // removed, so the row keeps its height and the snap keeps its
                // rhythm.
                style={{ opacity: clamp(1 - away * 2.5, 0, 1) }}
              >
                {project.saying ?? ""}
              </span>
            </button>
          );
        })}
      </div>

      {/* The band a thumb rests on, lifted off the field rather than divided
          from it by a line. The lift is the phone's own token: on black it is
          a faint light along the top edge, on white a soft shadow, and in both
          it says the same thing — this is nearer than what it covers. */}
      <div
        className="shrink-0 px-4 pt-3"
        style={{
          paddingBottom: "max(16px, var(--safe-bottom))",
          background: "var(--phone-bar)",
          boxShadow: "var(--phone-lift)",
        }}
      >
        {failure ? (
          <p className="px-1 pb-3 text-[13px] leading-[18px] text-danger">
            {failure}
          </p>
        ) : null}

        <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={busy || chosen === undefined}
          onClick={() => chosen && onOpen(chosen)}
          className="flex h-13 min-w-0 flex-1 items-center justify-center rounded-(--radius-surface) px-4 text-[17px] leading-[22px] font-semibold active:opacity-80"
          // Inverted, and it is the only inverted thing in the design. A dent
          // was tried first and read as *already pressed*: sinking is how this
          // interface says chosen, and what this control says is do it. On a
          // field that is off, the one lit rectangle is unmistakably the
          // action — and the same swap in the light appearance gives the same
          // reading upside down.
          // `--text-primary` and `--surface-workspace` rather than the theme's
          // own `--color-fg` and `--color-workspace`, and the difference is a
          // trap worth stating: `@theme inline` *substitutes* values into the
          // utilities it generates, so the `--color-*` names it emits carry
          // whatever the light desk appearance said at build time and never
          // move again. A utility class is fine — it was compiled against the
          // reference. A hand-written `var(--color-…)` is not.
          style={{
            background: "var(--text-primary)",
            color: "var(--surface-workspace)",
          }}
        >
          <span className="truncate">
            {chosen === undefined ? "Open" : `Open ${chosen.name}`}
          </span>
        </button>
        {trailing}
        </div>
      </div>
    </div>
  );
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
