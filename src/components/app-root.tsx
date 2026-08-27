"use client";

import { AppShell } from "@/components/shell/app-shell";
import { SettingsWindow } from "@/components/settings/settings-window";
import { useAppearance } from "@/lib/settings/appearance";
import { useTypography } from "@/lib/settings/typography";
import { useWindowRole } from "@/lib/settings/window";

/**
 * What this document is showing.
 *
 * Sync opens the same exported document in two windows, and the window's own
 * label says which one it is: the project window, or the settings window. That
 * is the whole of the decision — the two share the token layer and the controls
 * and nothing else, so neither is a mode of the other.
 */
export function AppRoot() {
  // Both windows are painted from the same token layer, so both apply the
  // appearance — and they apply it here, above whichever one this is, so it
  // lands before either has rendered anything.
  useAppearance();
  // The same for how a record's text is set. The settings window needs it as
  // much as the project window does: the preview beside the controls is the
  // real prose surface, so it has to be reading the same variables.
  useTypography();

  return useWindowRole() === "settings" ? <SettingsWindow /> : <AppShell />;
}
