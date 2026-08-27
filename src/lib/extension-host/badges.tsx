"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import type { Opener } from "@/components/shell/opening";
import type { BadgeReport } from "@/lib/extension-api/badge";
import type { MountedArea } from "@/lib/extension-host/areas";
import { listRecords } from "@/lib/memory/client";

/**
 * The counts the window draws on its section rows, asked of the corpus.
 *
 * This is the **declared** half of a badge and the only half that works for a
 * section nobody has opened. An area is mounted the first time it is selected,
 * so a count an area reported would be missing in exactly the moment a person
 * most needs it — the first launch after opening a project, when every section
 * is unvisited. Nothing of a package runs to produce a number here: what is
 * counted is in the manifest, and the host does the counting.
 *
 * **Which records are a section's own is not answered twice.** A badge that
 * names no kinds counts what its section *opens*, and which section opens a
 * kind is already one bound lookup in `opening.ts` — the same one the palette
 * asks before handing over a result. Asking it here as well is what keeps a
 * badge from claiming records its own section would refuse to show, and it is
 * what makes the badge work for the section that opens the project's own types:
 * those kinds are the project's, invented long after the manifest was written,
 * so no manifest could have listed them.
 *
 * **One call answers every section.** A listing's counts are over what its
 * filters selected rather than over its page, and they arrive broken down by
 * kind — so a single `limit: 1` listing per distinct freshness filter yields a
 * map every badge is then a sum over. Two sections watching the same states
 * cost one call between them, and a section watching every state costs the one
 * call with no filter.
 *
 * **It is read when the project opens and when the window is returned to**,
 * which is where `useSyncState` reads and for the same reason. That is not a
 * compromise on live-ness: freshness is derived by reconciling code history
 * against each record's scope, so what moves a count is somebody's commit
 * rather than somebody's keystroke, and coming back to the window is exactly
 * when that has happened. The runtime channel is what a badge with a faster
 * clock than that will use, and it belongs to a mounted area.
 *
 * Archived records are left out. Archiving takes a record out of the lists, so
 * counting one would put a number on a row in front of something the section
 * itself would not show.
 *
 * What is **not** subtracted is the kinds this window is not listing. That
 * preference belongs to the frame and each area holds its own copy of it, so
 * reading a third here would be a third answer, drifting from the other two the
 * moment somebody ticks a box. A badge therefore counts what the project holds
 * rather than what this window is showing of it — the narrower claim of the
 * two, and the one that cannot go stale.
 */

/** What a section's row shows, and a section with nothing to say has none. */
export type BadgeCount =
  /** How many there are. How it is abbreviated is the row's business. */
  | { readonly kind: "count"; readonly value: number }
  /**
   * Something happened and there is no number for it.
   *
   * The mark this system already has for *go and look*, so it is kept for that
   * and for nothing else — never for a count too large to print, which would be
   * one mark carrying two unrelated claims.
   */
  | { readonly kind: "dot" };

/** Badges by area key. A section with nothing to say is absent from it. */
export type Badges = ReadonlyMap<string, BadgeCount>;

const NOTHING: Badges = new Map();

/**
 * How a listing is asked for a count and nothing else.
 *
 * `limit: 1` rather than none, because the engine's own floor is one, and
 * `metadata_only` because no body is wanted: what is being read is the counts
 * beside the page rather than the page.
 */
const COUNT_ONLY = { limit: 1, metadata_only: true, archived: false } as const;

/** One question, and every section that asked it. */
interface Question {
  readonly freshness: readonly string[];
  readonly areas: readonly MountedArea[];
}

export function useDeclaredBadges(
  projectPath: string,
  sections: readonly MountedArea[],
  opener: Opener,
): Badges {
  const [badges, setBadges] = useState<Badges>(NOTHING);

  // The distinct questions, and which sections asked each of them. Derived from
  // the sections rather than kept beside them: two sections watching the same
  // states are one question, and a section arriving or leaving changes the set.
  const questions = useMemo(() => {
    const byFilter = new Map<string, { freshness: readonly string[]; areas: MountedArea[] }>();
    for (const area of sections) {
      if (area.badge === null) continue;
      const key = [...area.badge.freshness].sort().join(",");
      const asked = byFilter.get(key);
      if (asked === undefined) {
        byFilter.set(key, { freshness: area.badge.freshness, areas: [area] });
      } else {
        asked.areas.push(area);
      }
    }
    return [...byFilter.values()] as readonly Question[];
  }, [sections]);

  const count = useCallback(async (): Promise<Badges> => {
    const counted = new Map<string, BadgeCount>();

    await Promise.all(
      questions.map(async ({ freshness, areas }) => {
        let byKind: Readonly<Record<string, number>>;
        try {
          const listing = await listRecords(projectPath, {
            ...COUNT_ONLY,
            ...(freshness.length === 0 ? {} : { freshness: [...freshness] }),
          });
          byKind = listing.counts.by_kind;
        } catch {
          // A badge is not worth a banner, and one bad question is not worth
          // every other section's number. The states are the engine's
          // vocabulary and a manifest may name one this engine does not derive,
          // so a question it will not answer costs these sections their count
          // and leaves the rest alone.
          return;
        }

        for (const area of areas) {
          // Through the same reading as a live report, so the ceiling and
          // what none looks like are decided once for both sources.
          const drawn = drawnAs(sum(byKind, area, opener));
          if (drawn !== null) counted.set(area.key, drawn);
        }
      }),
    );

    return counted;
  }, [opener, projectPath, questions]);

  useEffect(() => {
    // Nothing to ask, so nothing is read and nothing is stored. What is
    // *shown* in that case is decided below rather than written here: a
    // section that stops declaring a badge should lose it in the render that
    // notices, not one render later.
    if (questions.length === 0) return undefined;

    let current = true;
    const read = () => {
      void count().then((counted) => {
        // Held rather than replaced when nothing moved. A read answers with a
        // fresh map every time, so storing it outright would re-render the
        // whole column on every return to the window whether or not a single
        // number had changed — and, because what this reads with is rebuilt
        // from what the window is running, a render that produced a read would
        // be a read that produced a render.
        if (current) setBadges((held) => (same(held, counted) ? held : counted));
      });
    };

    read();
    window.addEventListener("focus", read);
    return () => {
      current = false;
      window.removeEventListener("focus", read);
    };
  }, [count, questions.length]);

  return questions.length === 0 ? NOTHING : badges;
}

/** Whether two answers say the same thing about the same sections. */
function same(held: Badges, counted: Badges): boolean {
  if (held.size !== counted.size) return false;
  for (const [key, badge] of counted) {
    const before = held.get(key);
    if (before === undefined || before.kind !== badge.kind) return false;
    if (badge.kind === "count" && before.kind === "count" && before.value !== badge.value) {
      return false;
    }
  }
  return true;
}

/**
 * How many of a listing's records belong to one section.
 *
 * A badge naming kinds is a sum over those; a badge naming none is a sum over
 * whatever this section opens, which is the answer `opening.ts` already holds.
 */
function sum(
  byKind: Readonly<Record<string, number>>,
  area: MountedArea,
  opener: Opener,
): number {
  const declared = area.badge?.kinds ?? [];
  let total = 0;
  for (const [kind, held] of Object.entries(byKind)) {
    const mine =
      declared.length === 0 ? opens(opener, kind, area.key) : declared.includes(kind);
    if (mine) total += held;
  }
  return total;
}

/** Whether this section is the one that would open a record of that kind. */
function opens(opener: Opener, kind: string, areaKey: string): boolean {
  const opening = opener(kind);
  return opening.outcome === "area" && opening.areaKey === areaKey;
}

// ---------------------------------------------------------------------------
// The live half: merging in what a mounted area says about itself.
//
// The channel and the hook an area calls are `extension-api/badge.tsx`, because
// they are the surface and the surface may not reach into the loader. What is
// here is the window's half: holding the reports, and reading both sources into
// one mark per row.
// ---------------------------------------------------------------------------

/** The live reports, and the callback an area hands them in through. */
export interface LiveBadges {
  readonly reported: ReadonlyMap<string, BadgeReport>;
  readonly report: (areaKey: string, badge: BadgeReport) => void;
}

export function useLiveBadges(): LiveBadges {
  const [reported, setReported] = useState<ReadonlyMap<string, BadgeReport>>(
    new Map(),
  );

  const report = useCallback((areaKey: string, badge: BadgeReport) => {
    setReported((held) => {
      // Held rather than replaced when the answer has not moved. An area
      // re-reporting the same number on every render of its own is ordinary,
      // and a new map for each of them would re-render this window's column
      // for news that has not changed.
      const before = held.get(areaKey) ?? null;
      if (before === badge) return held;
      const next = new Map(held);
      if (badge === null) next.delete(areaKey);
      else next.set(areaKey, badge);
      return next;
    });
  }, []);

  return useMemo(() => ({ reported, report }), [report, reported]);
}

/**
 * One answer per row, from the two sources there are.
 *
 * A live report wins where there is one, because the area is mounted and knows
 * more than a query does. Where there is none the declared count shows through
 * — see [`BadgeReport`] for why that is the rule rather than the other one.
 */
export function mergeBadges(
  declared: Badges,
  reported: ReadonlyMap<string, BadgeReport>,
): Badges {
  if (reported.size === 0) return declared;

  const merged = new Map(declared);
  for (const [areaKey, badge] of reported) {
    const drawn = drawnAs(badge);
    if (drawn === null) merged.delete(areaKey);
    else merged.set(areaKey, drawn);
  }
  return merged;
}

/**
 * What a report becomes on a row.
 *
 * The one place either source is turned into a mark, so none is nothing in both
 * and a number is a number in both. **How large a number may be before it is
 * abbreviated is not decided here** — that is a fact about the space a row has,
 * and it belongs to the row.
 *
 * A dot is what a section says when it knows something is worth a look and
 * cannot put a number on it. It is never an abbreviation of a count: a dot on
 * this system means *something happened*, and a dot that sometimes meant "more
 * than ninety-nine of the usual thing" would be one mark for two claims.
 */
function drawnAs(badge: BadgeReport): BadgeCount | null {
  if (badge === null) return null;
  if (badge === "some") return { kind: "dot" };
  return badge > 0 ? { kind: "count", value: badge } : null;
}
