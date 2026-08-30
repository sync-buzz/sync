"use client"

import * as React from "react"
import { ScrollArea as ScrollAreaPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"

function ScrollArea({
  className,
  children,
  viewportRef,
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.Root> & {
  /**
   * The node that actually scrolls.
   *
   * `ref` on this component reaches the root, which is the box the panel is
   * measured as and never the one carrying a `scrollTop` — so a caller that has
   * to move the scroll itself, or read how far from the bottom it is, had no
   * way to reach the element that answers either question. Radix wraps the
   * viewport and this file is the only place that can hand it out; a caller
   * digging for it through the DOM would be tied to markup this file is free to
   * change.
   *
   * Handed out rather than acted on. Following a stream, holding a position,
   * restoring one — each is a decision about *what is being read*, and a
   * scroller knows nothing about that.
   */
  viewportRef?: React.Ref<HTMLDivElement>
}) {
  return (
    <ScrollAreaPrimitive.Root
      data-slot="scroll-area"
      className={cn("relative", className)}
      {...props}
    >
      {/* The viewport is as wide as the panel it is in, and so is what it
          scrolls. Radix wraps the children in a div of its own styled
          `min-width: 100%; display: table`, and a table is sized to its
          contents: the longest line in a list decides the width every row is
          laid out against. `w-full` then means the width of that line rather
          than of the column, `max-w-[65%]` has no definite width to be a
          percentage of, and `truncate` is given all the room it asks for — so
          nothing ever ends in an ellipsis and the column scrolls sideways
          instead. Overridden with `!` because those are inline styles, and
          block rather than removed because the div is the node Radix measures;
          a block fills its column on its own, so no width is stated for it.

          Content that is genuinely wider than the column — a table or a code
          block in a document — still scrolls: the scrollbar is derived from the
          viewport's own `scrollWidth`, which an overflowing child still grows.

          `overscroll-contain` is the other half of a panel owning its own
          scrolling: a scroller that has reached its end stops there rather than
          handing the rest of the gesture to whatever is behind it. Without it
          one flick through a list carries on into the surface underneath, and
          what moves is not the thing the gesture started on. */}
      <ScrollAreaPrimitive.Viewport
        ref={viewportRef}
        data-slot="scroll-area-viewport"
        className="size-full overscroll-contain rounded-[inherit] transition-[color,box-shadow] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1 [&>div]:block!"
      >
        {children}
      </ScrollAreaPrimitive.Viewport>
      <ScrollBar />
      <ScrollAreaPrimitive.Corner />
    </ScrollAreaPrimitive.Root>
  )
}

function ScrollBar({
  className,
  orientation = "vertical",
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>) {
  return (
    <ScrollAreaPrimitive.ScrollAreaScrollbar
      data-slot="scroll-area-scrollbar"
      data-orientation={orientation}
      orientation={orientation}
      className={cn(
        "flex touch-none p-px transition-colors select-none data-horizontal:h-2.5 data-horizontal:flex-col data-horizontal:border-t data-horizontal:border-t-transparent data-vertical:h-full data-vertical:w-2.5 data-vertical:border-l data-vertical:border-l-transparent",
        className
      )}
      {...props}
    >
      <ScrollAreaPrimitive.ScrollAreaThumb
        data-slot="scroll-area-thumb"
        className="relative flex-1 rounded-full bg-border"
      />
    </ScrollAreaPrimitive.ScrollAreaScrollbar>
  )
}

export { ScrollArea, ScrollBar }
