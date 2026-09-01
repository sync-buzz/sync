"use client";

import type { ComponentType, ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * The parts a phone screen is built from, so that the arrangement being judged
 * is the arrangement and not four slightly different navigation bars.
 *
 * None of this is the shell's chrome and none of it should become it. The
 * desktop bands are 34 px high, hold a title and at most one control, and are
 * addressed by role; these are 44 points high because that is the size of a
 * finger, they carry a control on each side because a phone has nowhere else
 * to put one, and they are addressed by nothing — the prototype is thrown away
 * and the numbers in it are an argument, not an interface.
 *
 * Colour comes from the same tokens the columns use, so both appearances hold
 * without a single value being invented here. Type does not: the interface
 * scale in `src/app/globals.css` runs 11 px to 15 px, which is desktop density
 * read at arm's length. A phone is read at a foot and tapped rather than
 * clicked, so the sizes below are stated locally and stay in this folder.
 */

/** Characters past which a screen's name gives way to the word `Back`. */
const BACK_LABEL_LIMIT = 14;

/**
 * The band above a screen, which on a phone is where the way back lives.
 *
 * Its height is the touch target plus whatever the hardware claims at the top
 * of the screen — a notch, an island, a status bar. `env()` is nothing in a
 * window on a desktop, so the same expression gives a plain 44 there and the
 * true inset on a device.
 */
export function NavBar({
  title,
  back,
  onBack,
  trailing,
}: {
  title: string;
  /** What tapping the leading control returns to. Absent on a root screen. */
  back?: string;
  onBack?: () => void;
  trailing?: ReactNode;
}) {
  return (
    <div
      className="shrink-0 border-b border-separator"
      style={{ paddingTop: "max(0px, env(safe-area-inset-top))" }}
    >
      <div className="flex h-11 items-center gap-1 px-1">
        <div className="flex min-w-0 flex-1 justify-start">
          {back ? (
            <button
              type="button"
              onClick={onBack}
              className="-ml-1 flex h-11 min-w-11 items-center gap-0.5 rounded-lg pr-2 pl-1 text-[17px] leading-[22px] text-focus active:bg-hover"
            >
              <ChevronLeft className="size-5 shrink-0" strokeWidth={2.25} />
              {/* The name of what you are going back to, and the word `Back`
                  when that name is too long to fit beside a title. That
                  substitution is the platform's, and leaving it out is how a
                  bar ends up with a truncated word on each side and no room
                  for either. */}
              <span className="truncate">
                {back.length > BACK_LABEL_LIMIT ? "Back" : back}
              </span>
            </button>
          ) : null}
        </div>

        {/* Centred and allowed to be cut rather than to push its neighbours:
            the two controls are the only way off this screen, and a long title
            must not be able to take one of them away. */}
        <h1 className="min-w-0 shrink truncate px-1 text-[17px] leading-[22px] font-semibold">
          {title}
        </h1>

        <div className="flex min-w-0 flex-1 items-center justify-end gap-1">
          {trailing}
        </div>
      </div>
    </div>
  );
}

/** A control in a navigation bar or a toolbar, sized to a finger. */
export function BarButton({
  label,
  icon: Icon,
  onPress,
  className,
}: {
  /** Read out, and shown when there is no icon to stand for it. */
  label: string;
  icon?: ComponentType<{ className?: string }>;
  onPress?: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      aria-label={Icon ? label : undefined}
      className={cn(
        "flex h-11 min-w-11 items-center justify-center gap-1 rounded-lg px-2 text-[17px] leading-[22px] text-focus active:bg-hover",
        className,
      )}
    >
      {Icon ? <Icon className="size-5" /> : label}
    </button>
  );
}

/**
 * One screen: a bar, a scroller, and optionally the strip of controls that
 * acts on what the scroller holds.
 *
 * The strip is at the foot rather than at the head, which is where the desktop
 * column keeps the same controls — both for the same reason, that they belong
 * to the list rather than to the window. On a phone the foot is also the only
 * part of a tall screen a thumb reaches.
 */
export function Screen({
  className,
  bar,
  toolbar,
  foot,
  children,
}: {
  className?: string;
  bar: ReactNode;
  /** Controls that act on what the scroller holds, as one band. */
  toolbar?: ReactNode;
  /** Anything else that stays at the foot, drawn as it is given. */
  foot?: ReactNode;
  children: ReactNode;
}) {
  const standing =
    toolbar !== undefined ? (
      <div className="flex h-11 items-center gap-1 px-1">{toolbar}</div>
    ) : (
      foot
    );

  return (
    <div className={cn("flex h-full min-h-0 flex-col", className)}>
      {bar}
      {/* `clip`, not `hidden`: a hidden box is still a scrollport that has lost
          its bar, and the window's own rule about that is stated in full in
          `docs/design-foundation.md` §"Panel roles and priority". It holds
          here for the same reason. */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-clip overscroll-contain">
        {children}
        {standing ? null : <SafeAreaFoot />}
      </div>
      {standing ? (
        // Outside the scroller, or the space the hardware claims would be
        // reachable only by scrolling to it, and the row above it would sit
        // against the foot of the screen looking cut off.
        <div className="shrink-0 border-t border-separator">
          {standing}
          <SafeAreaFoot />
        </div>
      ) : null}
    </div>
  );
}

/**
 * The space the hardware takes at the foot of the screen, which on a device
 * with a home indicator is where the gesture lives. Nothing may sit in it, and
 * a list that ends exactly at it reads as a list that has been cut off.
 */
function SafeAreaFoot() {
  return (
    <div
      className="shrink-0"
      style={{ height: "max(20px, env(safe-area-inset-bottom))" }}
    />
  );
}

/**
 * A row in a list, at the height of a finger.
 *
 * The chevron is the phone's whole answer to "does this go somewhere": a
 * desktop row says so by opening a column beside it, and a row that opens a
 * screen has to say so before it is tapped.
 */
export function Row({
  icon: Icon,
  label,
  detail,
  badge,
  leadsOn,
  selected,
  onPress,
}: {
  icon?: ComponentType<{ className?: string }>;
  label: string;
  detail?: string;
  /** A count, or a dot when there is news and no number for it. */
  badge?: number | "dot";
  leadsOn?: boolean;
  selected?: boolean;
  onPress?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      className={cn(
        "flex min-h-11 w-full items-center gap-3 px-4 py-2 text-left",
        // Selection is a surface and a weight, and nothing else — the rule the
        // window keeps, and the one that survives greyscale.
        selected ? "bg-selected font-semibold" : "active:bg-hover",
      )}
    >
      {Icon ? <Icon className="size-5 shrink-0 text-fg-secondary" /> : null}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[17px] leading-[22px]">
          {label}
        </span>
        {detail ? (
          <span className="block truncate text-[15px] leading-[20px] text-fg-secondary">
            {detail}
          </span>
        ) : null}
      </span>
      {badge !== undefined ? <Badge value={badge} /> : null}
      {leadsOn ? (
        <ChevronRight className="size-4 shrink-0 text-fg-tertiary" />
      ) : null}
    </button>
  );
}

/**
 * What a row says about its own contents. A figure is how many there are and a
 * dot is that something happened — two claims, never each other, and the
 * window's rule for them is the same at this width. Neither is coloured: a
 * count is information, and colour here is kept for status and destruction.
 */
function Badge({ value }: { value: number | "dot" }) {
  if (value === "dot") {
    return (
      <span
        aria-label="Something new"
        className="size-2 shrink-0 rounded-full bg-fg-tertiary"
      />
    );
  }
  return (
    <span className="shrink-0 text-[15px] leading-[20px] text-fg-tertiary tabular-nums">
      {value > 99 ? "99+" : value}
    </span>
  );
}

/** The hairline between rows, inset to where the text starts. */
export function RowSeparator({ inset = 16 }: { inset?: number }) {
  return (
    <div
      className="h-px bg-separator"
      style={{ marginLeft: `${inset}px` }}
      aria-hidden
    />
  );
}

/**
 * A block of placeholder text.
 *
 * The prototype measures an arrangement, so its content is bars rather than
 * invented sentences: a surface that is not settled is left empty and labelled
 * instead of filled with something plausible, and a screen of convincing prose
 * would be read as a proposal about what the screen is for.
 */
export function TextPlaceholder({ widths }: { widths: readonly number[] }) {
  return (
    <div className="space-y-2" aria-hidden>
      {widths.map((width, index) => (
        <div
          key={index}
          className="h-3 rounded-sm bg-selected"
          style={{ width: `${width}%` }}
        />
      ))}
    </div>
  );
}
