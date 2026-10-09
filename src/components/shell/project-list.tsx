"use client";

import { useEffect, useRef, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";

import { edgeMask, useScrollFade } from "@/lib/mobile-scroll";
import { haptic } from "@/lib/haptic";

/**
 * The list a phone chooses a project from, and the root of everything above it.
 *
 * A list rather than a wheel. The wheel it replaces gave the enlargement a job
 * — the project in the middle was the only one the window said anything about
 * — and paid for it twice over: three names on a screen that holds twelve, and
 * a second control at the foot to open whichever one was at the mark. What a
 * phone does with a set of things to go into is a list, and tapping a row is
 * the whole of the gesture.
 *
 * **It is the first screen, and that is now load-bearing.** Everything a
 * person reaches in this application is pushed in front of it, so the bar at
 * the head of every screen after this one names it: there is one way out of
 * anywhere, it is always drawn, and it always ends here. What belongs to the
 * phone rather than to any project — which computer it dials, and forgetting
 * that computer — therefore lives here too, where it can be found without
 * knowing a gesture.
 *
 * The large title is the platform's and is drawn plainly rather than
 * collapsing as the list scrolls: there are a dozen projects at most, so the
 * room a collapsing title buys back is room nobody needed.
 */

export interface Listable {
  /** What the caller is handed back. Opaque here. */
  readonly key: string;
  readonly name: string;
  /** What is true of it, in a line, where the window knows anything. */
  readonly saying?: string;
}

export function ProjectList({
  projects,
  startAt,
  busy,
  failure,
  trailing,
  onOpen,
}: {
  projects: readonly Listable[];
  /**
   * The project this phone was in a moment ago, brought into view.
   *
   * Not selected and not moved to the top: where a project is in this list is
   * the computer's order, and a list that re-ordered itself around the last
   * thing touched is a list whose rows are never twice in the same place.
   * Scrolled to, so that a long list opens showing the likely one.
   */
  startAt?: string;
  /** A project is being opened, so nothing else may be started meanwhile. */
  busy?: boolean;
  /**
   * What the computer said when it refused, over the list.
   *
   * Over it rather than instead of it: a project that would not open leaves
   * the other projects standing, and a screen that replaced them with an
   * apology would take away the thing to try next.
   */
  failure?: string | null;
  /** One control that is not about a project, at the end of the bar. */
  trailing?: ReactNode;
  onOpen: (project: Listable) => void;
}) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const begun = useRef<HTMLButtonElement | null>(null);
  const edges = useScrollFade(scroller, "y");
  const mask = edgeMask("y", edges);

  // Once, on the way in. `startAt` does not change while this screen is up —
  // it is where the phone came *from* — so this is an arrival rather than
  // something that follows the list around.
  useEffect(() => {
    begun.current?.scrollIntoView({ block: "center" });
  }, []);

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-workspace text-fg">
      <div
        className="flex shrink-0 items-end gap-2 px-4 pb-2"
        style={{ paddingTop: "calc(var(--safe-top) + 8px)" }}
      >
        <h1
          className="min-w-0 flex-1 truncate text-[30px] leading-[36px] font-bold"
          // The one place a phone is allowed a text shadow, and only here:
          // the rule is that it goes on the large type and never on the body,
          // because the contrast floor is checked against the body and a
          // shadow is the first thing to break it.
          style={{ textShadow: "var(--phone-title-glow)" }}
        >
          Projects
        </h1>
        {trailing}
      </div>

      {failure === null || failure === undefined ? null : (
        <p className="shrink-0 px-4 pb-2 text-[13px] leading-[18px] text-fg-secondary">
          {failure}
        </p>
      )}

      <div
        ref={scroller}
        className="min-h-0 flex-1 overflow-y-auto overscroll-y-contain px-3 pb-3"
        style={{ maskImage: mask, WebkitMaskImage: mask }}
      >
        <div className="flex flex-col gap-1.5">
          {projects.map((project) => (
            <button
              key={project.key}
              ref={project.key === startAt ? begun : undefined}
              type="button"
              disabled={busy}
              onClick={() => {
                haptic();
                onOpen(project);
              }}
              // Raised out of the field rather than ruled off from its
              // neighbours: this design tells surfaces apart by depth, and a
              // list of hairlines between rows was the one shape the phone
              // does not draw.
              className="flex min-h-16 w-full items-center gap-3 rounded-(--radius-surface) px-4 py-3 text-left active:opacity-70 disabled:opacity-40"
              style={{
                background: "var(--phone-bar)",
                boxShadow: "var(--phone-lift)",
              }}
            >
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-[17px] leading-[22px] font-medium">
                  {project.name}
                </span>
                {project.saying === undefined ? null : (
                  <span className="truncate text-[13px] leading-[18px] text-fg-secondary">
                    {project.saying}
                  </span>
                )}
              </span>
              {/* The chevron says the row is a way in rather than a choice to
                  be confirmed — which is the whole difference between this and
                  the wheel it replaces, said in one glyph. */}
              <ChevronRight className="size-5 shrink-0 text-fg-tertiary" />
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
