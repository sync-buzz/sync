"use client";

import * as React from "react";
import { Dialog as SheetPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/**
 * A sheet: a modal that belongs to this window and to nothing else.
 *
 * macOS attaches this kind of modal to the window it acts on rather than
 * floating it in the middle of the screen, so it slides out from under the
 * title bar and the title bar stays where it was. Opening a project is exactly
 * that: it configures *this* window, it cannot be left half-done in the
 * background, and there is nothing behind it to interact with meanwhile.
 *
 * The scrim covers the slab and not the frame. The frame is the window's edge,
 * not its content; dimming it would say the desktop is modal too.
 */

function Sheet({ ...props }: React.ComponentProps<typeof SheetPrimitive.Root>) {
  return <SheetPrimitive.Root data-slot="sheet" {...props} />;
}

function SheetContent({
  className,
  children,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Content>) {
  return (
    <SheetPrimitive.Portal>
      <SheetPrimitive.Overlay
        data-slot="sheet-overlay"
        className="fixed inset-(--window-inset) z-40 rounded-(--radius-window) bg-scrim duration-(--motion-duration) data-closed:animate-out data-closed:fade-out-0 data-open:animate-in data-open:fade-in-0"
      />
      <SheetPrimitive.Content
        data-slot="sheet-content"
        className={cn(
          // Anchored under the title bar, centred on the window, and never
          // wider than the slab it slides out of.
          "fixed top-[calc(var(--window-inset)+var(--header-height))] left-1/2 z-50 w-[min(560px,calc(100vw-var(--window-inset)*2-64px))] max-h-[calc(100vh-var(--window-inset)*2-var(--header-height)-40px)] -translate-x-1/2",
          "flex flex-col overflow-hidden rounded-b-(--radius-surface) bg-raised text-fg shadow-(--shadow-content)",
          "duration-(--motion-duration) data-closed:animate-out data-closed:fade-out-0 data-closed:slide-out-to-top-4 data-open:animate-in data-open:fade-in-0 data-open:slide-in-from-top-4",
          className,
        )}
        {...props}
      >
        {children}
      </SheetPrimitive.Content>
    </SheetPrimitive.Portal>
  );
}

/** The title band. One line, named for the task, never for the step. */
function SheetHeader({
  className,
  ...props
}: React.ComponentProps<"header">) {
  return (
    <header
      data-slot="sheet-header"
      className={cn(
        "flex h-(--panel-header-height) shrink-0 items-center border-b border-separator px-4",
        className,
      )}
      {...props}
    />
  );
}

function SheetTitle({
  className,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Title>) {
  return (
    <SheetPrimitive.Title
      data-slot="sheet-title"
      className={cn("truncate text-sm font-semibold text-fg-secondary", className)}
      {...props}
    />
  );
}

function SheetDescription({
  className,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Description>) {
  return (
    <SheetPrimitive.Description
      data-slot="sheet-description"
      className={cn("text-sm text-fg-secondary", className)}
      {...props}
    />
  );
}

/**
 * The action band. Buttons sit at the trailing edge with the one that
 * continues the task last, as they do in a native sheet.
 */
function SheetFooter({ className, ...props }: React.ComponentProps<"footer">) {
  return (
    <footer
      data-slot="sheet-footer"
      className={cn(
        "flex shrink-0 items-center gap-3 border-t border-separator px-4 py-3",
        className,
      )}
      {...props}
    />
  );
}

export {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
};
