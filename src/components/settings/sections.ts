import {
  AudioLines,
  Bot,
  Palette,
  Radio,
  Type,
  type LucideIcon,
} from "lucide-react";

/**
 * The sections of the settings window.
 *
 * Settings are the installation's: what is true of this machine whatever project
 * open, and whatever project is not. Four things are — how the window is
 * painted, how a record's text is set, which agents reach Sync, and what it
 * says out loud.
 *
 * Text is its own section rather than a part of Appearance, because the two are
 * different questions asked by different people. Appearance is what the window
 * looks like; Text is whether somebody can read for an hour without their eyes
 * hurting, and that belongs beside its own preview rather than under a heading
 * about colour.
 *
 * Voice is here rather than in a project's window for the same test: a voice
 * belongs to these speakers. A colleague who clones the project has a different
 * set of voices installed, so a choice that travelled would name one they do not
 * have — and the machine speaks with every window closed, which is the case a
 * project's window could not configure at all.
 *
 * Extensions are deliberately *not* here. An extension installs types, scripts
 * and a screen into a project, so it is chosen while a project is open, from
 * the project's own window. Settings would have made it a property of this machine,
 * which is the one thing it is not.
 *
 * The same rule the main window's sidebar follows applies: a section is one
 * with a screen behind it. There is no "General" because there is nothing
 * general left to decide — the layout is rebuilt from its defaults on every
 * launch and has no preference to store.
 */
export interface SettingsSection {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  /** The sentence under the section's name, in the window's own voice. */
  readonly headline: string;
}

export const SETTINGS_SECTIONS = [
  {
    id: "appearance",
    label: "Appearance",
    icon: Palette,
    headline: "How the window is painted.",
  },
  {
    id: "text",
    label: "Text",
    icon: Type,
    headline: "How a record's text is set, wherever one is read or written.",
  },
  {
    id: "server",
    label: "Server",
    icon: Radio,
    headline: "One server answers for every project it holds.",
  },
  {
    id: "agents",
    label: "Agents",
    icon: Bot,
    headline: "Agents reach a project's knowledge through Sync.",
  },
  {
    id: "voice",
    label: "Voice",
    icon: AudioLines,
    headline: "What Sync says out loud, and in whose voice.",
  },
] as const satisfies readonly SettingsSection[];

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

export const DEFAULT_SETTINGS_SECTION: SettingsSectionId = "appearance";
