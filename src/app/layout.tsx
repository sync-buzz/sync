import type { Metadata, Viewport } from "next";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./globals.css";

export const metadata: Metadata = {
  title: "Sync",
  description: "Application shell for Sync.",
};

export const viewport: Viewport = {
  // Native scrollbars and form controls follow the system appearance.
  colorScheme: "light dark",
  // The document is given the whole screen, and hands back what the hardware
  // claims. Without this a webview is inset by the safe areas before it is
  // handed anything, which paints two bands of the page's own background
  // around the interface and reports every `env(safe-area-inset-*)` as zero —
  // so the one mechanism for keeping text out from under a notch is switched
  // off exactly where it is needed. It says nothing on a Mac, which has no
  // inset to cover.
  viewportFit: "cover",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en">
      <body>
        <TooltipProvider delayDuration={400} skipDelayDuration={200}>
          {children}
        </TooltipProvider>
      </body>
    </html>
  );
}
