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
  // The document is not a page to be scaled. Its own text sizes are a design
  // decision and its layout already fits the screen it is on, so a pinch and a
  // double tap have nothing to reveal — they only leave the interface offset
  // from the hardware, with a toolbar half off one edge and safe areas
  // measured against a frame that has moved.
  //
  // The second number is the one that matters, and it is not about pinching at
  // all: a phone magnifies the page whenever a field takes focus and its text
  // is smaller than the system's own reading size. Every field in this window
  // is, because the scale here is a Mac's. Held at one, there is nothing to
  // magnify into, so the field is reached without the screen jumping.
  //
  // Both say nothing on a Mac, whose webview has no viewport to scale. They
  // are stated in the document rather than applied by the phone because they
  // have to be true of the first frame, and the first frame is painted from
  // this export before any of its code runs.
  maximumScale: 1,
  userScalable: false,
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
