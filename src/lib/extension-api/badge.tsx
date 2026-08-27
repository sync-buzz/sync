"use client";

import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";

/**
 * What a section says about its own row, and how it reaches the window.
 *
 * Here rather than beside the host's counting, and for the reason `contract.ts`
 * is here: this is the surface, and the surface may not depend on the loader.
 * The host imports [`BadgeScope`] from it; an extension imports [`useBadge`],
 * and the context both use has to be the same object, so it is neither side's
 * to hold.
 */

/**
 * What an area reports about its own row, and `null` for nothing.
 *
 * `"some"` is the answer for a section that knows something is worth a look and
 * cannot put a number on it. A number is the number; zero is nothing, because a
 * mark that means none is a mark that means nothing.
 *
 * **Reporting nothing is not a report.** The declared count goes on showing
 * through it, which is what lets a section have both: Chat declares how many
 * conversations there are, so the row says so before a line of Chat has run and
 * goes on saying so while nobody is talking to an agent. What the area reports
 * takes over only while there is something it alone could know — a reply that
 * arrived while somebody was in another section, which is nowhere in the corpus
 * and cannot be counted from it. Composing the two is the area's own business:
 * it is mounted, it holds both numbers, and it decides which one its row should
 * say.
 */
export type BadgeReport = number | "some" | null;

/** How an area reaches the window's badges, and which row it is. */
interface BadgeChannel {
  readonly areaKey: string;
  readonly report: (areaKey: string, badge: BadgeReport) => void;
}

/**
 * Absent outside a window, which is the ordinary case for a component under
 * test or in a story. `useBadge` then does nothing rather than throwing: an
 * area that cannot draw a badge is still an area.
 */
const Channel = createContext<BadgeChannel | null>(null);

/**
 * Publishes the channel for one area's subtree.
 *
 * Wrapped around the whole of a layer — the provider and the columns — because
 * an area is as likely to hold its unread count in its provider as in a column.
 * The layers nest, so this also encloses every area visited after this one; it
 * is not a leak, because each of those opens a scope of its own before it
 * renders anything of its own, and the nearer one wins.
 */
export function BadgeScope({
  areaKey,
  report,
  children,
}: {
  areaKey: string;
  report: (areaKey: string, badge: BadgeReport) => void;
  children: ReactNode;
}) {
  const channel = useMemo(() => ({ areaKey, report }), [areaKey, report]);
  return <Channel.Provider value={channel}>{children}</Channel.Provider>;
}

/**
 * Say what this section's row should show, from inside the section.
 *
 * The half of a badge a manifest cannot express, and the one an agent's reply
 * needs: "it answered while you were in another section" is not a state of the
 * corpus and no query over it would find it.
 *
 * **A frozen area goes on reporting.** Selecting another section tells this one
 * to stop reading the store; it does not stop existing, and this is the one
 * channel it keeps — the whole point being to say something while nobody is
 * looking at it. That is a deliberate narrowing of the freeze rule rather than
 * an exception to it.
 *
 * One call per area. This is the row's whole answer rather than a contribution
 * to it, so two components reporting would be two answers and the later render
 * would win — which is a coin toss dressed as a rule.
 */
export function useBadge(report: BadgeReport): void {
  const channel = useContext(Channel);

  useEffect(() => {
    if (channel === null) return undefined;
    channel.report(channel.areaKey, report);
    // Cleared when the reporter goes, not when the area does: an area is never
    // unmounted, so the only way here is the extension leaving the project, and
    // a number outliving the section it was about would be a row that is not
    // there carrying news.
    return () => channel.report(channel.areaKey, null);
  }, [channel, report]);
}

