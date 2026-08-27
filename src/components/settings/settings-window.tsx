"use client";

import { useState } from "react";
import { AgentsSection } from "@/components/settings/agents-section";
import { ServerSection } from "@/components/settings/server-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import {
  DEFAULT_SETTINGS_SECTION,
  SETTINGS_SECTIONS,
  type SettingsSectionId,
} from "@/components/settings/sections";
import { TypographySection } from "@/components/settings/typography-section";
import { VoiceSection } from "@/components/settings/voice-section";
import { SourceList } from "@/components/shell/source-list";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useWindowReveal } from "@/lib/window-reveal";

/**
 * The settings window.
 *
 * It is a window rather than a sheet because what it holds is true of the
 * installation and not of the window it was opened from: which agents this machine
 * connects, and which extensions it has. The shell reserves sheets for what
 * configures the window they slide out of, and macOS reserves `⌘,` for exactly
 * this — so this is the one and it opens where the system expects it to.
 *
 * It is a source list beside a column of settings, at the density of the rest
 * of the application, on the same token layer. It carries no frame and no slab:
 * the material is the main window's edge, and a second window wearing it would
 * turn a deliberate detail into a theme. The title bar is the system's, showing
 * the system's own word for this window, so nothing here re-states it.
 */
export function SettingsWindow() {
  // Built hidden by `settings_open`, for the reason the main window is: a
  // window that appears before its first frame is a flash of nothing. Closing
  // it is the menu bar's Close Window, which is the system's own command.
  useWindowReveal();

  const [sectionId, setSectionId] = useState<SettingsSectionId>(
    DEFAULT_SETTINGS_SECTION,
  );
  const section =
    SETTINGS_SECTIONS.find((entry) => entry.id === sectionId) ??
    SETTINGS_SECTIONS[0];

  return (
    <div className="flex h-full bg-workspace text-fg">
      <aside className="flex w-44 shrink-0 flex-col border-r border-separator bg-sidebar">
        <SourceList
          label="Settings"
          items={SETTINGS_SECTIONS}
          activeId={sectionId}
          onSelect={(id) => setSectionId(id as SettingsSectionId)}
        />
      </aside>

      <ScrollArea className="min-h-0 min-w-0 flex-1">
        <main className="flex flex-col gap-4 px-6 py-5">
          <header className="space-y-1">
            <h1 className="text-lg font-medium text-fg">{section.label}</h1>
            <p className="text-sm text-fg-secondary">{section.headline}</p>
          </header>

          {sectionId === "appearance" ? (
            <AppearanceSection />
          ) : sectionId === "text" ? (
            <TypographySection />
          ) : sectionId === "server" ? (
            <ServerSection />
          ) : sectionId === "voice" ? (
            <VoiceSection />
          ) : (
            <>
              <AgentsSection />

              <p className="text-xs text-fg-tertiary">
                Extensions are not here: one installs its types, its scripts and
                its screen into a project, so it is chosen from the
                project&apos;s own window — at the foot of the sidebar.
              </p>
            </>
          )}
        </main>
      </ScrollArea>
    </div>
  );
}
