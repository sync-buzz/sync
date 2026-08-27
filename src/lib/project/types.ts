/**
 * What an open project is, and what has to be true before there is one.
 *
 * A project is a Git repository. That is not a preference: the store keeps its
 * knowledge in the repository's own refs, so a folder outside version control
 * has nowhere to put it, and the opening flow offers to make one rather than
 * carrying a second, weaker kind of project.
 */

/** A folder, as `project_probe` describes it. */
export interface FolderProbe {
  /** The folder that was chosen, absolute. */
  readonly path: string;
  /** The name to offer as the project's, taken from the folder. */
  readonly name: string;
  /**
   * The work tree the folder belongs to, or `null` when it belongs to none.
   * It differs from `path` when a folder inside a repository was chosen.
   */
  readonly repositoryRoot: string | null;
}

/**
 * A failure worth branching on.
 *
 * `not_a_directory` is a folder that moved between being chosen and being
 * read; `git_missing` means the application cannot see Git at all, which no
 * amount of retrying will fix; `git_failed` carries Git's own words.
 */
export interface ProjectFailure {
  readonly kind: "not_a_directory" | "git_missing" | "git_failed";
  readonly message: string;
}

/**
 * The language the project's knowledge is written in.
 *
 * It is asked for once, at the start, because it is a property of the project
 * rather than of the person reading it: claims, documents and specifications
 * are shared through the repository, and a store that mixes languages is a
 * store nobody can search.
 */
export const PROJECT_LANGUAGES = [
  { id: "en", label: "English" },
  { id: "de", label: "Deutsch" },
  { id: "fr", label: "Français" },
  { id: "es", label: "Español" },
  { id: "pt", label: "Português" },
  { id: "zh", label: "中文" },
  { id: "ja", label: "日本語" },
] as const;

export type ProjectLanguageId = (typeof PROJECT_LANGUAGES)[number]["id"];

export const DEFAULT_LANGUAGE_ID: ProjectLanguageId = "en";

/**
 * What a project calls itself.
 *
 * These live in the project's own memory, not on this machine, so a project
 * opened on a second computer is the same project rather than a folder that has
 * to be described again.
 */
export interface ProjectSettings {
  readonly name: string;
  /**
   * What this project is called by anyone referring to it — an agent naming
   * which project a call is about, a document mentioning a neighbour.
   *
   * Derived from the name when the project is created and fixed from then on:
   * it travels in the repository's own record, so two people who opened it
   * hold the same one, and renaming the project does not move it.
   */
  readonly identifier: string;
  /** Optional, and empty far more often than not. */
  readonly description: string;
  readonly language: ProjectLanguageId;
  /**
   * The extensions this project is composed of.
   *
   * The declaration travels with the repository; the code that satisfies it is
   * the machine's business. Empty is a real answer and the ordinary state of a
   * project somebody has just made: the window opens on the catalogue.
   */
  readonly installed: readonly InstalledExtension[];
}

/** One extension a project depends on, by identifier and version. */
export interface InstalledExtension {
  readonly id: string;
  /**
   * The version that was installed, not the one available now. An extension
   * that has moved on is something the window can notice and say.
   */
  readonly version: string;
  /**
   * What this extension tells an agent, in full, as the project stores it.
   *
   * Here rather than only in the build because the MCP server is a process of
   * its own and has no view of the catalogue: a prompt that stayed in the
   * window would be one only the window could read. Written on install and
   * rewritten whenever this build's text and the stored one disagree.
   *
   * Absent for an extension with nothing to say, which is most of them.
   */
  readonly prompt?: string;
  /**
   * The sha256 of the artefact this version resolved to.
   *
   * What turns the declaration into a lockfile: a release re-tagged under a
   * version somebody already has is detected rather than trusted. Absent for a
   * package with no fixed content to hash — a folder somebody is writing in —
   * and that absence is the honest answer rather than a gap to fill.
   */
  readonly integrity?: string;
  /**
   * Where the package came from: `registry`, `file`, `folder`, or `seeded`
   * for an archive the build shipped with.
   *
   * The difference between a dependency and somebody's working tree. A project
   * declaring one that came from a folder was composed against code being
   * written, and anybody opening it elsewhere deserves to know that before
   * wondering why a section is missing. The path is deliberately not here: it
   * belongs to one machine, and in a shared record it is noise at best.
   */
  readonly source?: string;
}

/** Whether a repository has been opened as a project before. */
/**
 * A project this installation answers for.
 *
 * Not the recent list: that is a menu of eight, and falling off it means
 * nobody opened the project lately. Falling out of this one would mean an agent
 * can no longer reach it.
 */
export interface RegisteredProject {
  /** The repository root, absolute. */
  readonly path: string;
  /** What the window calls it. */
  readonly name: string;
  /**
   * What agents call it **here**.
   *
   * Normally the identifier in the project's own record, which everyone who
   * opened that repository shares. It differs only where two repositories on
   * this machine derived the same one and a person said which of them answers
   * to something else locally.
   */
  readonly identifier: string;
}

/** What registering a project did. */
export interface Registration {
  /**
   * The project already answering to that identifier here, when there is one.
   * Nothing was written in that case.
   */
  readonly takenBy: RegisteredProject | null;
}

export interface ProjectSettingsProbe {
  /** Present when the project already exists and has nothing to be asked. */
  readonly settings: ProjectSettings | null;
  /**
   * Why memory could not be consulted, when it could not be. Absent settings
   * and an unreachable engine are different answers, and the flow says which
   * one it got.
   */
  readonly memoryError: string | null;
}

/** A project the window is open on. */
export interface OpenProject extends ProjectSettings {
  /** The repository root. Everything the project knows lives under it. */
  readonly path: string;
}

/**
 * What this installation shows of a project.
 *
 * Hiding a type is a decision about this window, not about the project: the
 * records stay where they are, agents go on writing them, and a colleague who
 * pulls the same repository sees every type. That is why it is stored beside
 * the recent list rather than in the project's memory.
 *
 * Arranging the sidebar is the same kind of decision, and it is here for the
 * same reason: somebody dragging a section to the top is saying where they
 * work, not what the project is.
 */
export interface ProjectView {
  /** Kinds the window does not list, as the store spells them. */
  readonly hiddenTypes: readonly string[];
  /**
   * The sections somebody arranged, by area key, in the order they put them.
   *
   * Not necessarily every section the project has: it is what was moved, and
   * anything installed since has never been placed. Resolving that against the
   * sections the window actually mounted is `use-section-order.ts`.
   */
  readonly sections: readonly string[];
}

/**
 * A change to that, which says only what it changed.
 *
 * The two halves are decided in two different columns and neither knows about
 * the other, so a write that carried the whole view would let the type filter
 * erase the sidebar's order on its way past. An absent field is left as stored;
 * an empty list is a list somebody emptied.
 */
export interface ProjectViewChange {
  readonly hiddenTypes?: readonly string[];
  readonly sections?: readonly string[];
}

/** A project this installation has opened before. */
export interface RecentProject {
  readonly path: string;
  readonly name: string;
}

/**
 * A stored language, as one this build knows.
 *
 * Memory holds whatever was written, which may be a language a later version
 * added or an older one spelled differently. Falling back is the right answer:
 * the project still opens, and it is named by something more important than
 * its language tag.
 */
export function asLanguageId(value: string): ProjectLanguageId {
  return (
    PROJECT_LANGUAGES.find((language) => language.id === value)?.id ??
    DEFAULT_LANGUAGE_ID
  );
}

export function languageLabel(id: ProjectLanguageId): string {
  return (
    PROJECT_LANGUAGES.find((language) => language.id === id)?.label ?? id
  );
}
