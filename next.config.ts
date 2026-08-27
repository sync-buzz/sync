import type { NextConfig } from "next";

/**
 * Sync ships as a Tauri desktop application. Next.js is the UI build system,
 * never a server: the app is exported to static assets that Tauri embeds and
 * serves from the app bundle. Every feature that needs a Node.js runtime after
 * packaging (SSR, Route Handlers, Server Actions, proxy/middleware, image
 * optimization) is therefore unavailable by construction.
 */
const nextConfig: NextConfig = {
  output: "export",
  images: {
    // No image optimizer exists at runtime in a packaged desktop app.
    unoptimized: true,
  },
};

export default nextConfig;
