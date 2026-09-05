"use client";

import { createPortal } from "react-dom";
import type { ReactNode } from "react";

import { useBandSlot } from "@/lib/shell-bands";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

/**
 * Shared chrome for the columns of the content slab.
 *
 * Every panel is a flush surface bounded by structural edges, never a floating
 * card: no shadow of its own, no corner radius, no inset margin. Each panel
 * owns its own scrolling so the window itself never scrolls.
 *
 * The header is one band at one height across all three columns, so its
 * hairline reads as a single line crossing the slab rather than three
 * unrelated ones. That is also why each column's header must say something
 * different: the navigator names the section, the workspace names what is
 * being shown of it, and the inspector names the object beside it.
 */

export function PanelSurface({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cn("flex h-full min-w-0 flex-col", className)}>
      {children}
    </section>
  );
}

export function PanelHeader({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div
      // A band rather than the list, which is a distinction only the phone
      // reads: there the column is a screen, and tapping something in the list
      // goes on to the workspace while tapping a control in a band stays put.
      // Marked on the band rather than on every row, because a package brings
      // its own rows and the bands are the shell's.
      data-panel-band="true"
      className="flex h-(--panel-header-height) shrink-0 items-center justify-between gap-2 border-b border-separator pr-2 pl-3"
    >
      <h2 className="truncate text-sm font-semibold text-fg-secondary">
        {title}
      </h2>
      {children}
    </div>
  );
}

export function PanelBody({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className={cn("p-3", className)}>{children}</div>
    </ScrollArea>
  );
}

/**
 * The strip along the bottom edge of a column, holding the controls that act on
 * what the column lists.
 *
 * This is where macOS puts them — the sidebar's own bottom bar, as in Mail,
 * Reminders, Music and Xcode's navigator — rather than in the column's header
 * or in the window toolbar: the header names the column, the toolbar acts on
 * the window, and a control inside the scroller leaves with the list it acts
 * on. It is one band at one height, stated here once, the way the header is.
 *
 * What acts on the *contents* of a list is not one of these. That command sits
 * beside the list's own title — the `+` next to a list's name in Reminders —
 * because it belongs to what is being shown rather than to the column showing
 * it.
 */
export function PanelFooter({ children }: { children: ReactNode }) {
  // Offered a band of its own — a phone's — the controls go there instead. What
  // they are and what they do is untouched; only where they stand differs.
  const band = useBandSlot();
  if (band !== null) return createPortal(children, band);

  return (
    <div
      data-panel-band="true"
      className="flex h-(--panel-header-height) shrink-0 items-center gap-1 border-t border-separator px-1.5"
    >
      {children}
    </div>
  );
}

/**
 * The quiet text a column shows while it has nothing to list. It states the
 * role of the column instead of simulating its future content.
 */
export function PanelPlaceholder({
  headline,
  detail,
}: {
  headline: string;
  detail?: string;
}) {
  return (
    <div className="max-w-[36ch] space-y-1.5">
      <p className="text-sm text-fg-secondary">{headline}</p>
      {detail ? <p className="text-xs text-fg-tertiary">{detail}</p> : null}
    </div>
  );
}
