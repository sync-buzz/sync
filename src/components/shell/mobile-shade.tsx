"use client";

import { useRef, useState, type PointerEvent } from "react";
import { FolderGit2, Search, SlidersHorizontal } from "lucide-react";
import { SyncIndicator } from "@/components/shell/sync-indicator";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import { cn } from "@/lib/utils";

/**
 * What belongs to the window rather than to any section: searching the whole
 * corpus, what the project's memory is doing, this phone's own settings, and
 * the way back out to the list of projects.
 *
 * On a desk all four live in the title bar, which costs nothing because the bar
 * is there regardless. A phone that kept a bar for them would spend the top
 * eleven per cent of every screen on four controls that are used a few times an
 * hour — so they are pulled down instead, from the top of whatever is on the
 * screen.
 *
 * **The gesture starts in the content, not at the edge.** iOS owns the top edge
 * — one swipe there reveals the system's own handle, a second opens Control
 * Centre — so a shade tied to the edge would be a shade that opens on the
 * second attempt or never. Pulling down from the top of a list that is already
 * at its top is the platform's own idiom for revealing search, which is what
 * this mostly is.
 */

/** How far down a finger has to travel before the shade is open, in points. */
const PULL = 150;

/**
 * The pull, as the screen it is pulled from sees it.
 *
 * Returned as handlers plus a fraction rather than as a component, because what
 * moves is the shade and what is *touched* is the content underneath it — two
 * different elements, and the fraction is what they agree on.
 */
export function usePullDown({
  enabled,
  onOpen,
}: {
  /**
   * Whether the thing under this finger has anywhere to scroll back to.
   *
   * Asked of the event rather than of the screen, because the screen does not
   * know which of its columns the finger landed in and the event does. A list
   * already at its top has nothing to give the gesture, which is exactly when
   * the gesture becomes the shade's.
   */
  enabled: (event: PointerEvent<HTMLElement>) => boolean;
  onOpen: () => void;
}) {
  const [pulled, setPulled] = useState(0);
  const from = useRef<number | null>(null);

  const onPointerDown = (event: PointerEvent<HTMLElement>) => {
    // A mouse can press and drag without meaning to scroll, and this prototype
    // is looked at with one as often as with a finger. Both are allowed; what
    // is not is starting a pull on a list that has somewhere to scroll back
    // to, which is what `enabled` answers.
    if (!enabled(event)) return;
    from.current = event.clientY;
  };

  const onPointerMove = (event: PointerEvent<HTMLElement>) => {
    if (from.current === null) return;
    const travelled = event.clientY - from.current;
    // Upward movement is not a smaller pull, it is a different gesture: the
    // finger is scrolling the list it started on. Abandon rather than clamp.
    if (travelled < -8) {
      from.current = null;
      setPulled(0);
      return;
    }
    // Resistance past the point of no return, so the movement keeps answering
    // the finger after the decision has already been made. A pull that stops
    // dead at 1 reads as a broken gesture.
    setPulled(Math.min(1.12, Math.max(0, travelled / PULL)));
  };

  const onPointerEnd = () => {
    if (from.current === null) return;
    from.current = null;
    if (pulled >= 1) onOpen();
    setPulled(0);
  };

  return {
    pulled,
    handlers: {
      onPointerDown,
      onPointerMove,
      onPointerUp: onPointerEnd,
      onPointerCancel: onPointerEnd,
    },
  };
}

/**
 * The shade itself, and the darkened screen behind it.
 *
 * It is given the pull so that it can arrive with the finger rather than after
 * it: at rest it sits its own height above the screen, and every point the
 * finger travels brings it down by one. Released short of the mark it goes
 * back, and that return is the only thing here that is animated — while a
 * finger is down, the finger is the animation.
 */
export function Shade({
  open,
  pulled,
  project,
  sync,
  onSearch,
  onOpenSync,
  onOpenSettings,
  onClose,
  onLeave,
}: {
  open: boolean;
  /** How far it has been dragged out, 0 to 1, while nothing is open yet. */
  pulled: number;
  /** What this shade is about, said once at the top rather than in the field. */
  project: string;
  sync: SyncStatus;
  onSearch: () => void;
  onOpenSync: () => void;
  /**
   * What this phone is. Absent on a machine that reaches the same settings as
   * a window of its own, which is every machine but a phone.
   */
  onOpenSettings?: () => void;
  onClose: () => void;
  /**
   * The way back to the list of projects, and the reason this row exists at
   * all: leaving is a gesture, and a gesture is invisible to somebody reading
   * the screen aloud. Absent where there is no list to go back to.
   */
  onLeave?: () => void;
}) {
  const shown = open ? 1 : Math.min(1, pulled);
  const settled = open || pulled === 0;

  return (
    <>
      <div
        aria-hidden={!open}
        onClick={onClose}
        className={cn(
          "absolute inset-0 z-10",
          settled && "transition-opacity ease-shell duration-(--motion-duration)",
          !open && pulled === 0 && "pointer-events-none",
        )}
        style={{
          opacity: shown,
          // Dimmed and blurred rather than tinted, because a tint has nothing
          // to work with here: the field is already black, and a black veil
          // over a black screen leaves the white text on it exactly as bright
          // as it was — which is what the first version did. Taking the
          // brightness down acts on what is actually there, and it reads the
          // same way in both appearances.
          backdropFilter: "brightness(0.4) blur(3px)",
        }}
      />

      <div
        className={cn(
          "absolute inset-x-0 top-0 z-20 overflow-clip rounded-b-[32px]",
          settled && "transition-transform ease-shell duration-(--motion-duration)",
        )}
        style={{
          // A material rather than a panel: what it covers stays faintly
          // present underneath, which is what keeps it from reading as a slab
          // dropped on top of the screen. The blur is of our own content, so
          // the rule that this window is not made of glass holds — the glass
          // in this design is the system's, and this is one surface over one
          // column of ours.
          background: "var(--phone-shade)",
          backdropFilter: "blur(24px) saturate(140%)",
          boxShadow: "var(--phone-shade-lift)",
          transform: `translateY(${(shown - 1) * 100}%)`,
        }}
      >
        <div
          className="px-5 pb-3"
          style={{ paddingTop: "max(16px, var(--safe-top))" }}
        >
          {/* Which project this is about, and what its memory is doing —
              together, because they are one sentence: *this project, in this
              state*. They were two loose lines before, one of them stranded at
              the bottom with nothing to belong to. */}
          <div className="flex h-6 items-center gap-3">
            <span className="min-w-0 flex-1 truncate text-[11px] tracking-[0.16em] text-fg-tertiary uppercase">
              {project}
            </span>
            {/* What the project's memory is doing, in the window's own words
                rather than this file's: the sentence, the colour and the
                silence belong to the indicator, and a shade that wrote its own
                would be a second place deciding what *behind* looks like. */}
            <SyncIndicator
              sync={sync}
              onOpen={onOpenSync}
              className="h-6 min-w-0 shrink px-0 text-[13px] leading-[18px]"
            />
          </div>

          {/* The one thing on this shade that is sunk into it, and the reason
              the hierarchy works: pressing in is what a field does. Three
              surfaces at the same depth was the flat grey slab. */}
          <button
            type="button"
            onClick={() => {
              onClose();
              onSearch();
            }}
            className="mt-4 flex h-14 w-full items-center gap-3 rounded-2xl px-4 text-left active:opacity-80"
            style={{
              background: "var(--phone-sunken)",
              boxShadow: "var(--phone-sunken-shadow)",
            }}
          >
            {/* The secondary tier rather than the tertiary one, and it is a
                measurement rather than a preference: over the sunken surface
                in the light appearance the tertiary grey comes to 3.9:1,
                under the 4.5 a phone is held to. This is the hint for the one
                control on the shade somebody is meant to use. */}
            <Search className="size-5 shrink-0 text-fg-secondary" />
            {/* `Search`, and not the name of the project. A placeholder labels
                the field; repeating the project here said what the line above
                already says, and said it badly — a long name arrived truncated
                inside the one control on the screen that is about typing. */}
            <span className="text-[17px] leading-[22px] text-fg-secondary">
              Search
            </span>
          </button>

          {/* Two places to go, in one row divided by a hairline. As tiles they
              were two empty rectangles with a glyph in the corner of each;
              what they are is two words, and the shape now says only that. */}
          <div className="mt-2 flex items-stretch">
            {onLeave ? (
              <ShadeAction
                icon={FolderGit2}
                label="Projects"
                onPress={() => {
                  onClose();
                  onLeave();
                }}
              />
            ) : null}
            {onLeave && onOpenSettings ? (
              <span aria-hidden className="my-3 w-px bg-separator" />
            ) : null}
            {onOpenSettings ? (
              <ShadeAction
                icon={SlidersHorizontal}
                label="Settings"
                onPress={() => {
                  onClose();
                  onOpenSettings();
                }}
              />
            ) : null}
          </div>

          {/* The handle, which is also the way back up. A shade with no visible
              way to close it is a shade people close by guessing — so it is
              drawn at the tier text is drawn at, not at the tier a hairline is:
              the previous one was there and could not be seen. */}
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="mx-auto flex h-11 w-24 items-end justify-center pb-2"
          >
            <span className="block h-1 w-10 rounded-full bg-fg-tertiary" />
          </button>
        </div>
      </div>
    </>
  );
}

/**
 * One of the two places the shade leads.
 *
 * Half the row, and the row is as tall as a finger. No surface of its own: on
 * a shade this size a third filled shape would put everything at the same
 * weight again, and what distinguishes these two from the field above them is
 * that they are words you press rather than a box you type into.
 */
function ShadeAction({
  icon: Icon,
  label,
  onPress,
}: {
  icon: typeof Search;
  label: string;
  onPress: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      className="flex h-14 flex-1 items-center justify-center gap-2.5 rounded-2xl active:bg-hover"
    >
      <Icon className="size-5 shrink-0 text-fg-secondary" />
      <span className="truncate text-[17px] leading-[22px] font-medium">
        {label}
      </span>
    </button>
  );
}
