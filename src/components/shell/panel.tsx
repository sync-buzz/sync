"use client";

import { createPortal } from "react-dom";
import type { ReactNode } from "react";

import type { LucideIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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
 *
 * None of that is true at the width of a phone, where the three columns are
 * three screens one in front of the next: there is no line crossing anything,
 * and nothing standing beside a column to be told apart from. So both bands
 * give their controls to the one strip the phone keeps for them, and the
 * header's title — which on a desk says which of three columns this is — is
 * left out rather than repeated under the name of the project.
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
  // Offered a band of its own — a phone's — the controls go there and the
  // title does not go anywhere. On a desk the header names the column because
  // three columns stand side by side and each has to say which it is; on a
  // phone the column *is* the screen, and what it is called is already said
  // twice over it — by the section lit in the band at the foot, and by the
  // project named in the bar at the head. A third copy between them is the
  // window saying one thing three times, under a hairline this design does not
  // draw anywhere else.
  //
  // The controls are a different matter and are kept: they act on what the
  // column lists, and they join whatever its foot put in the same band, which
  // is the one strip of column controls a phone has.
  const band = useBandSlot();
  // `?? null` rather than a branch that returns nothing: a header with no
  // controls still renders, into a band that hides itself when nothing was
  // put in it. The difference is the signature — a component that can answer
  // `null` is a different shape on the extension surface, and this one has no
  // reason to be.
  if (band !== null) return createPortal(children ?? null, band);

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
 * One command in a bottom bar, in the weight that band is drawn at.
 *
 * Tertiary until it is pointed at, like the pinned row in the sidebar beside
 * it: the bar is furniture, and a control at full weight in it reads as the
 * loudest thing in a column whose subject is the list above. The name is a
 * tooltip rather than a `title`, because `title` is the system's own and
 * arrives late enough that people stop waiting for it.
 *
 * Here rather than in the one column that first needed it, because a second
 * column needs it now: two drawings of one control is how a bar comes to have
 * two weights, two sizes and two ideas of where its label goes.
 */
export function FooterAction({
  icon: Icon,
  label,
  disabled,
  onSelect,
}: {
  icon: LucideIcon;
  label: string;
  disabled?: boolean;
  onSelect: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={label}
          disabled={disabled}
          onClick={onSelect}
          className="text-fg-tertiary hover:text-fg"
        >
          <Icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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
