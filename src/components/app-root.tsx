"use client";

import { AskPanel } from "@/components/ask/ask-panel";
import { AppShell } from "@/components/shell/app-shell";
import { SettingsWindow } from "@/components/settings/settings-window";
import { useAppearance } from "@/lib/settings/appearance";
import { useTypography } from "@/lib/settings/typography";
import { useWindowRole } from "@/lib/settings/window";

/**
 * What this document is showing.
 *
 * Sync opens the same exported document in three windows, and the window's own
 * label says which one it is: the project window, the settings window, or the
 * panel a key opens over another application. That is the whole of the decision
 * — the three share the token layer and the controls and nothing else, so none
 * of them is a mode of another.
 */
export function AppRoot() {
  // Both windows are painted from the same token layer, so both apply the
  // appearance — and they apply it here, above whichever one this is, so it
  // lands before either has rendered anything.
  useAppearance();
  // The same for how a record's text is set. The settings window needs it as
  // much as the project window does: the preview beside the controls is the
  // real prose surface, so it has to be reading the same variables. The panel
  // reads it because what an agent answers is prose as well.
  useTypography();

  const role = useWindowRole();
  if (role === "settings") return <SettingsWindow />;
  if (role === "ask") return <AskPanel />;
  return <AppShell />;
}
