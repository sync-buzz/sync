import {
  AudioLines,
  Bell,
  Bot,
  GitBranch,
  KeyRound,
  Palette,
  Radio,
  Smartphone,
  Type,
  type LucideIcon,
} from "lucide-react";

/**
 * The sections of the settings window.
 *
 * Settings are the installation's: what is true of this machine whatever project
 * is open, and whatever project is not. Nine things are — how the window is
 * painted, how a record's text is set, what serves every project this machine
 * holds, which agent the window works through and which agents reach Sync,
 * which devices reach it from somewhere else,
 * where work happens when it happens somewhere disposable, what it says out
 * loud, and the secrets it keeps on a package's behalf.
 *
 * Remote Access is its own section rather than a part of Vault, and the two are
 * asked by different people about different things: the vault holds what a
 * package uses to reach *out*, and this decides who may reach *in*. Somebody
 * whose phone was stolen is looking for the second, and they would not think to
 * look for it in a list of packages' keys.
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

/**
 * The bands the sections are read in.
 *
 * Nine rows in one run are nine things to hold at once; in four runs of two or
 * three they are four. The division is by the question somebody arrived with —
 * what am I looking at, how does it reach me when I am not looking, what does
 * the work, who gets in and what is kept for them — rather than by how often a
 * section is opened, which would be an order that changes under people.
 *
 * A band is a heading and nothing else: it is never selected, never carries a
 * count, and has no screen behind it. That is what keeps it from reading as a
 * tenth section.
 */
const BANDS = {
  window: "Window",
  attention: "Attention",
  work: "Work",
  access: "Access",
} as const;

/**
 * One decision inside a section, named in the column beside it.
 *
 * A section that has these is not a screen itself — choosing it chooses the
 * first part. It earns them only where its parts are each a screenful. Agents
 * is the one that is: choosing a provider is one list; the servers a package
 * may call is a list; what is connected to Sync is three runs of clients with a
 * button each. Stacked they are a wall somebody scrolls past, and the column is
 * where that is answered — a group named there is a group you can go straight
 * to.
 *
 * The sections whose parts are a row of options each — Text, Notifications —
 * deliberately have none. Splitting a single radio group onto a screen of its
 * own would spend a row of the column to save no reading at all, and a column
 * that lists everything has stopped saying which things are large.
 *
 * The headline is the sentence the screen opens with, so it lives here rather
 * than beside the control: the column and the screen have to agree on what a
 * group is called, and two copies of a name is the pair that drifts.
 */
export interface SettingsGroup {
  /**
   * Addressed as `section/group`, because this id is also what one window sends
   * another to. A bare `servers` would sit one letter from the `server` section
   * in the same namespace, and the reader of a deep link would have no way to
   * tell which was meant.
   */
  readonly id: string;
  readonly label: string;
  readonly headline: string;
}

export interface SettingsSection {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  /** The sentence under the section's name, in the window's own voice. */
  readonly headline: string;
  /** Which run of the column this section is read in. */
  readonly band: string;
  /** The decisions inside it that are each worth a row, or none. */
  readonly groups?: readonly SettingsGroup[];
}

/**
 * The parts of Agents.
 *
 * Out here rather than inside the list because both the column and the screen
 * read them: the column draws the rows, and the screen draws the same names
 * over the same controls when all three are shown at once. Two copies of a
 * name is the pair that drifts, and a column offering a part the screen calls
 * something else is a window disagreeing with itself.
 */
export const AGENT_GROUPS = [
  {
    id: "agents/provider",
    label: "Provider",
    headline:
      "Which provider Sync works through by default. The console runs through it, and so does any work the window starts on its own — a panel that needs something from one of your own tools is answered in a turn nobody is watching. The provider's own account answers every turn and is charged for it; a local model, if you need one, is reached through the provider's own configuration rather than from here.",
  },
  {
    id: "agents/servers",
    label: "Servers packages may call",
    headline:
      "A package agrees with you on its own page before it calls anything. The agreement covers that server — every tool it publishes, including the ones that change things — because Sync cannot ask a server what it holds. Withdrawing one here refuses that package's next call until you agree again.",
  },
  {
    id: "agents/connected",
    label: "Connected to Sync",
    headline:
      "Sync serves every project from one address. Connecting writes a single server entry into the agent's own configuration — outside any repository, so nothing about it reaches your team — and disconnecting takes exactly that entry back out. Which project a call is about is the agent's to say on each call.",
  },
] as const satisfies readonly SettingsGroup[];

export const SETTINGS_SECTIONS = [
  {
    id: "appearance",
    label: "Appearance",
    icon: Palette,
    headline: "How the window is painted.",
    band: BANDS.window,
  },
  {
    id: "text",
    label: "Text",
    icon: Type,
    headline: "How a record's text is set, wherever one is read or written.",
    band: BANDS.window,
  },
  {
    id: "notifications",
    label: "Notifications",
    icon: Bell,
    headline: "What Sync says when you are looking at something else.",
    band: BANDS.attention,
  },
  {
    id: "voice",
    label: "Voice",
    icon: AudioLines,
    headline: "What Sync says out loud, and in whose voice.",
    band: BANDS.attention,
  },
  {
    id: "server",
    label: "Server",
    icon: Radio,
    headline: "One server answers for every project it holds.",
    band: BANDS.work,
  },
  {
    id: "agents",
    label: "Agents",
    icon: Bot,
    headline:
      "Which agent Sync works through, and which agents reach a project's knowledge through Sync.",
    band: BANDS.work,
    groups: AGENT_GROUPS,
  },
  {
    id: "worktrees",
    label: "Working trees",
    icon: GitBranch,
    headline: "Where a conversation works when it works somewhere disposable.",
    band: BANDS.work,
  },
  {
    id: "remote",
    label: "Remote Access",
    icon: Smartphone,
    headline: "Which devices may talk to this Mac from somewhere else.",
    band: BANDS.access,
  },
  {
    id: "vault",
    label: "Vault",
    icon: KeyRound,
    headline: "Secrets this machine keeps on a package's behalf.",
    band: BANDS.access,
  },
] as const satisfies readonly SettingsSection[];

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

export const DEFAULT_SETTINGS_SECTION: SettingsSectionId = "appearance";

/**
 * The section and the group a name stands for, or nothing where this build has
 * never heard of it.
 *
 * One function for both kinds of name because both arrive the same way: a row
 * of the column hands one back, and so does the other window when it sends
 * somebody here. A group's name resolves to its section as well, which is what
 * lets the screen say where it is without the caller having worked it out.
 */
export function locateSettings(
  name: string,
): { section: SettingsSectionEntry; group: SettingsGroup | null } | null {
  for (const section of SETTINGS_SECTIONS) {
    if (section.id === name) return { section, group: null };
    const group = groupsOf(section).find((entry) => entry.id === name);
    if (group !== undefined) return { section, group };
  }
  return null;
}

/** One of the sections above, with its own name rather than any string. */
export type SettingsSectionEntry = (typeof SETTINGS_SECTIONS)[number];

/**
 * The parts of a section, and none where it has none.
 *
 * Asked through a function because the list above is read as itself rather than
 * as a row of one interface — which is what gives every id its own type, and
 * what makes a section that declares no parts a shape with no such member at
 * all. One place to ask is better than that test spelled out wherever somebody
 * wants the parts.
 */
export function groupsOf(section: SettingsSectionEntry): readonly SettingsGroup[] {
  return "groups" in section ? section.groups : [];
}
