"use client";

import { useEffect } from "react";
import { windowRole } from "@/lib/settings/window";

/**
 * The native material behind the window.
 *
 * `src-tauri/tauri.conf.json` asks for the macOS `underWindowBackground`
 * material at launch, so the window already opens with it. It is the quietest
 * of the materials — meant to sit under a window's content rather than to be
 * looked at — which is what the frame wants: the tint above it stays the
 * colour, and the desktop only shifts its hue.
 *
 * This hook owns the two things a static configuration cannot do: it tells the
 * stylesheet that a material is really there — through `data-vibrancy` on the
 * root element, which is what lets the frame become translucent — and it
 * withdraws the material when the system asks for reduced transparency, then
 * restores it when that preference changes back.
 *
 * Reduced transparency is answered here rather than in CSS on purpose: the
 * material belongs to the window, not to a stylesheet. Making the surfaces
 * opaque while the blur stayed underneath would satisfy the media query and
 * ignore the person who set it.
 *
 * Outside Tauri — `pnpm dev` in a browser — nothing runs and nothing is marked,
 * so the shell keeps the opaque surfaces it is designed to fall back to.
 */
export function useWindowMaterial() {
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    // The material belongs to the main window. It is the window
    // `tauri.conf.json` asks for it, and the only one whose capability grants
    // `set-effects`; the settings window wears the system's own title bar over
    // an opaque surface and asks for none of it.
    //
    // The check is here rather than left to which component mounted, because
    // the document hydrates as the main window before the label corrects it —
    // so the settings window commits the shell once, and that one commit is
    // long enough to send a request the platform is right to refuse.
    if (windowRole() !== "main") return;

    const reducedTransparency = window.matchMedia(
      "(prefers-reduced-transparency: reduce)",
    );
    let current = true;

    async function apply() {
      const { Effect, EffectState, getCurrentWindow } = await import(
        "@tauri-apps/api/window"
      );
      const appWindow = getCurrentWindow();

      if (reducedTransparency.matches) {
        await appWindow.clearEffects();
        if (current) delete document.documentElement.dataset.vibrancy;
        return;
      }

      await appWindow.setEffects({
        effects: [Effect.UnderWindowBackground],
        state: EffectState.FollowsWindowActiveState,
      });
      if (current) document.documentElement.dataset.vibrancy = "on";
    }

    function update() {
      apply().catch((error: unknown) => {
        // The window keeps whatever material it launched with; the shell keeps
        // its opaque surfaces. Say so instead of leaving a silent mismatch.
        console.warn("Window material could not be updated.", error);
        if (current) delete document.documentElement.dataset.vibrancy;
      });
    }

    update();
    reducedTransparency.addEventListener("change", update);

    return () => {
      current = false;
      reducedTransparency.removeEventListener("change", update);
    };
  }, []);
}
