"use client";

import { useState, type ReactNode } from "react";
import { AgentsSection } from "@/components/settings/agents-section";
import { RemoteSection } from "@/components/settings/remote-section";
import { ServerSection } from "@/components/settings/server-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import {
  DEFAULT_SETTINGS_SECTION,
  SETTINGS_SECTIONS,
  groupsOf,
  locateSettings,
  type SettingsSectionId,
} from "@/components/settings/sections";
import { TypographySection } from "@/components/settings/typography-section";
import { VaultSection } from "@/components/settings/vault-section";
import { NotificationsSection } from "@/components/settings/notifications-section";
import { VoiceSection } from "@/components/settings/voice-section";
import { WorktreesSection } from "@/components/settings/worktrees-section";
import { SourceList } from "@/components/shell/source-list";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useAskedSection } from "@/lib/settings/window";
import { useWindowReveal } from "@/lib/window-reveal";

/**
 * The screen behind each section, and which part of it to show.
 *
 * A table rather than a run of conditions, because what it holds is a fact
 * about the window — every section has a screen — and a chain of tests hides
 * that behind the order the tests happen to be in: the last branch of one is a
 * section by default rather than by name, which is a section nobody declared.
 * The type makes a section without a screen a compile error instead.
 *
 * Every screen is handed the part being shown, and almost every one ignores
 * it — a section with no parts is handed its own name. That is the cost of the
 * sections being one kind of thing: a screen that grew parts tomorrow would
 * read the argument it is already given rather than change the shape of this
 * table.
 */
const SCREENS: Record<SettingsSectionId, (group: string) => ReactNode> = {
  appearance: () => <AppearanceSection />,
  text: () => <TypographySection />,
  notifications: () => <NotificationsSection />,
  voice: () => <VoiceSection />,
  server: () => <ServerSection />,
  agents: (group) => <AgentsSection only={group} />,
  worktrees: () => <WorktreesSection />,
  remote: () => <RemoteSection />,
  vault: () => <VaultSection />,
};

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
 *
 * The column reads in two levels. Nine sections in four runs are four things to
 * take in rather than nine, and a section whose screen is several screenfuls
 * stacked names its parts, so one of them can be reached without reading past
 * the others. The parts are the column's business and not each screen's: a
 * screen that offered its own contents list would be a second place to navigate
 * from, three inches from the first.
 */
export function SettingsWindow() {
  // Built hidden by `settings_open`, for the reason the main window is: a
  // window that appears before its first frame is a flash of nothing. Closing
  // it is the menu bar's Close Window, which is the system's own command.
  useWindowReveal();

  // One name, whether it is a section's or one of its parts'. Two pieces of
  // state would be two that can disagree — a part of one section held while
  // another is shown — and there is no such place to be.
  const [selected, setSelected] = useState<string>(DEFAULT_SETTINGS_SECTION);
  // Where somebody was sent, when a control in the other window sent them
  // somewhere rather than just opening this one. A name this build does not
  // have is ignored: the list of sections is here, and a stranger's spelling of
  // one is not a reason to show a blank column.
  useAskedSection((asked) => {
    if (locateSettings(asked) !== null) setSelected(asked);
  });

  const found = locateSettings(selected) ?? {
    section: SETTINGS_SECTIONS[0],
    group: null,
  };
  const { section } = found;
  // A section with parts is not a screen, so a name that stands for one — the
  // first frame's default, or a control in the other window that knew only
  // which section it wanted — resolves to the first part rather than to
  // nothing. The alternative is a fourth kind of screen that exists only for
  // the moment before somebody clicks, which nobody would design on purpose.
  const group = found.group ?? groupsOf(section)[0] ?? null;

  return (
    <div className="flex h-full bg-workspace text-fg">
      <aside className="flex w-60 shrink-0 flex-col border-r border-separator bg-sidebar">
        <SourceList
          label="Settings"
          items={SETTINGS_SECTIONS.map((entry) => ({
            id: entry.id,
            label: entry.label,
            icon: entry.icon,
            band: entry.band,
            children: groupsOf(entry),
          }))}
          activeId={selected}
          onSelect={setSelected}
        />
      </aside>

      <ScrollArea className="min-h-0 min-w-0 flex-1">
        <main className="flex flex-col gap-4 px-6 py-5">
          <header className="space-y-1">
            {/* Which section this is a part of, said above the part rather
                than run into its name. A person who arrived by clicking a row
                indented under another already knows; a person sent here from
                the other window did not choose it at all, and the line is the
                only thing that tells them where they are standing. */}
            {group === null ? null : (
              <p className="text-xs text-fg-tertiary">{section.label}</p>
            )}
            <h1 className="text-lg font-medium text-fg">
              {group?.label ?? section.label}
            </h1>
            <p className="text-sm text-fg-secondary">
              {group?.headline ?? section.headline}
            </p>
          </header>

          {SCREENS[section.id](group?.id ?? section.id)}
        </main>
      </ScrollArea>
    </div>
  );
}
