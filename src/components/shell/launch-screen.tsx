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
 */
export function LaunchScreen({ isLoading }: { isLoading: boolean }) {
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

      <div className="flex flex-col items-center gap-3">
        {/*
          A track that is always there and a segment that only moves. With
          reduced motion the segment is dropped rather than frozen mid-track,
          where it would read as a progress bar stuck at a third.
        */}
        <div className="h-0.5 w-32 overflow-hidden rounded-full bg-selected">
          <div className="h-full w-1/3 animate-[launch-progress_1.4s_var(--motion-ease)_infinite] rounded-full bg-fg-tertiary motion-reduce:hidden" />
        </div>

        <p role="status" className="text-xs text-fg-tertiary">
          Starting
        </p>
      </div>
    </div>
  );
}
