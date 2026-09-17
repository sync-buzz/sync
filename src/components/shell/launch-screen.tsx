import { cn } from "@/lib/utils";

/**
 * What the window shows while the shell is still loading.
 *
 * It is deliberately the same slab the application is: the window opens once,
 * at its final size, on its final surface, and what changes is only that the
 * interface arrives inside it. No logo, no wordmark treatment, no colour that
 * the shell does not otherwise use — the product has no brand mark yet, and a
 * launch screen is the last place to invent one.
 *
 * The element stays mounted and fades out rather than unmounting, so the
 * interface is never revealed by a jump cut. Once it is out of the way it stops
 * taking pointer events and is hidden from assistive technology, which reads
 * the interface underneath instead.
 *
 * **On a phone it stands for a second reason, and it is the same reason.** The
 * window draws and a computer answers, so a phone that cannot reach its
 * computer has a window that cannot be used — which is what this element
 * already means. So a connection being made says *Connecting* where a launch
 * says *Starting*, and a connection that will not be made stops the bar and
 * says what refused. Nothing new is drawn and no height is taken from anything:
 * a phone whose connection drops and mends has an interface that never moved,
 * and a phone whose computer has gone is told so by the one surface in this
 * window that already stands in front of everything.
 */
export function LaunchScreen({
  isLoading,
  saying = "Starting",
  trouble = null,
  onRetry,
}: {
  isLoading: boolean;
  /**
   * The word under the bar. One word about what is happening, never about
   * what went wrong — the sentence for that is `trouble`, and it is set at a
   * different size in a different place because it is a different kind of
   * thing to read.
   */
  saying?: string;
  /**
   * Whoever refused, in their own words, or `null` while nothing has.
   *
   * Its presence is what stops the bar: a track still travelling under a
   * sentence about a computer that cannot be reached reads as a window that
   * has not noticed.
   */
  trouble?: string | null;
  /** Offered only where pressing it would do something. */
  onRetry?: () => void;
}) {
  return (
    <div
      aria-hidden={!isLoading}
      className={cn(
        "absolute inset-0 z-20 flex flex-col items-center justify-center gap-6 bg-workspace transition-opacity duration-300 ease-shell",
        // Optical centre: a block of text reads as centred slightly above the
        // geometric middle, and the window is empty enough here to show it.
        "pb-(--header-height)",
        !isLoading && "pointer-events-none opacity-0",
      )}
    >
      <p className="text-display font-medium tracking-tight text-fg">Sync</p>

      <div className="flex max-w-[34ch] flex-col items-center gap-3 px-6">
        {/*
          A track that is always there and a segment that only moves. With
          reduced motion the segment is dropped rather than frozen mid-track,
          where it would read as a progress bar stuck at a third. A stopped
          connection drops it for the same reason: nothing is travelling.
        */}
        <div className="h-0.5 w-32 overflow-hidden rounded-full bg-selected">
          {trouble === null ? (
            <div className="h-full w-1/3 animate-[indeterminate-progress_1.4s_var(--motion-ease)_infinite] rounded-full bg-fg-tertiary motion-reduce:hidden" />
          ) : null}
        </div>

        <p role="status" className="text-xs text-fg-tertiary">
          {saying}
        </p>

        {/* The words belong to whoever refused, and the window adds none of
            its own. What it adds is the one thing it knows and they do not:
            that this phone is still trying, which is what the button shortens
            rather than starts. */}
        {trouble !== null ? (
          <p className="text-center text-xs leading-[16px] text-fg-secondary">
            {trouble}
          </p>
        ) : null}

        {trouble !== null && onRetry ? (
          <button
            type="button"
            onClick={onRetry}
            className="mt-1 h-9 rounded-(--radius-control) bg-selected px-4 text-[13px] text-fg active:opacity-70"
          >
            Try again
          </button>
        ) : null}
      </div>
    </div>
  );
}
