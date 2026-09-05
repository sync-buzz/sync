/**
 * The frontend's only route to the file system.
 *
 * Choosing a folder is a native open panel owned by Tauri's dialog plugin;
 * everything said about that folder afterwards comes from the Rust commands in
 * `src-tauri/src/project.rs`, which are the only thing in the application that
 * runs `git`. The window holds no path policy of its own.
 */

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  FolderProbe,
  OpenProject,
  ProjectFailure,
  ProjectSettings,
  ProjectSettingsProbe,
  ProjectView,
  ProjectViewChange,
  RecentProject,
  RegisteredProject,
  Registration,
} from "./types";

/**
 * A project failure, thrown with its `kind` intact.
 *
 * Tauri rejects with whatever the command returned, which loses the type. This
 * restores it, the same way `MemoryError` does for the memory commands.
 */
export class ProjectError extends Error implements ProjectFailure {
  readonly kind: ProjectFailure["kind"];

  constructor(failure: ProjectFailure) {
    super(failure.message);
    this.name = "ProjectError";
    this.kind = failure.kind;
  }
}

export function isProjectFailure(error: unknown): error is ProjectError {
  return error instanceof ProjectError;
}

async function call<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "kind" in error &&
      "message" in error
    ) {
      throw new ProjectError(error as ProjectFailure);
    }
    throw error;
  }
}

/**
 * Ask for a folder with the system's open panel.
 *
 * Returns `null` when the panel is dismissed, which is an outcome rather than
 * an error: nothing has happened and nothing needs saying about it.
 */
export async function chooseFolder(): Promise<string | null> {
  const chosen = await open({
    directory: true,
    multiple: false,
    title: "Open Project",
  });
  return typeof chosen === "string" ? chosen : null;
}

/**
 * Ask for files inside the project with the system's open panel.
 *
 * A path in a record is a path in this repository, so the panel opens at the
 * project and what comes back is made relative to it. A file chosen from
 * outside the project is answered as the absolute path it is: refusing it here
 * would be this window deciding what a claim may be scoped to, and the store is
 * where that belongs.
 */
export async function chooseProjectFiles(
  projectPath: string,
): Promise<readonly string[]> {
  const chosen = await open({
    directory: false,
    multiple: true,
    defaultPath: projectPath,
    title: "Choose Files",
  });
  const paths =
    chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
  const root = projectPath.endsWith("/") ? projectPath : `${projectPath}/`;
  return paths.map((path) =>
    path.startsWith(root) ? path.slice(root.length) : path,
  );
}

/**
 * Ask for a directory inside the project with the system's open panel.
 *
 * What comes back is relative to the repository root, because that is what a
 * type's storage is written in: an absolute path would name one person's disk
 * and travel to a colleague as nonsense. A directory chosen outside the project
 * has no relative form at all, so it is answered as `null` — attaching one
 * would be describing a folder this repository does not contain.
 */
export async function chooseProjectFolder(
  projectPath: string,
): Promise<string | null> {
  const chosen = await open({
    directory: true,
    multiple: false,
    defaultPath: projectPath,
    title: "Choose Folder",
  });
  if (typeof chosen !== "string") return null;
  const root = projectPath.endsWith("/") ? projectPath : `${projectPath}/`;
  if (chosen === projectPath || chosen === root) return "";
  return chosen.startsWith(root) ? chosen.slice(root.length) : null;
}

/** Describe a folder without changing it. */
export function probeFolder(path: string): Promise<FolderProbe> {
  return call<FolderProbe>("project_probe", { path });
}

/** Make a folder a Git repository, then describe it again. */
export function initializeRepository(path: string): Promise<FolderProbe> {
  return call<FolderProbe>("project_initialize_repository", { path });
}

/**
 * Where this project's code came from, as `origin` names it.
 *
 * Asked rather than carried on `OpenProject`, and that is deliberate. The shell
 * itself never needs it — it would be a field every construction site of an
 * open project had to answer, including the one in the opening flow describing
 * a folder that is not a repository yet — and it is a fact that changes without
 * the project being reopened: somebody adds an `origin` and the answer is
 * different a second later.
 *
 * Whole and unparsed, in git's own spelling. What a URL means belongs to
 * whoever reads it: this build does not know what GitHub is.
 *
 * `null` for a repository with no `origin`, which is an ordinary state.
 */
export function projectRemote(path: string): Promise<string | null> {
  return call<string | null>("project_remote", { path });
}

/**
 * What the project already calls itself, if it has been opened before.
 *
 * This is what decides whether the opening flow asks anything: a repository
 * whose memory carries a project record answers for itself.
 */
export function loadProjectSettings(
  path: string,
): Promise<ProjectSettingsProbe> {
  return call<ProjectSettingsProbe>("project_settings_load", { project: path });
}

/**
 * Open one registered project, having asked it what it calls itself.
 *
 * The registry's name is what a person chose from, and the project's own record
 * is what the window is titled and addressed by afterwards — the same two the
 * Mac's opening flow reads, minus every step of it that is about a directory.
 *
 * **Memory that would not answer is refused rather than opened.** A project's
 * record is written whole, so a window holding an empty list of extensions
 * because the memory was busy would, on the first install, take every other
 * extension out of that project for everybody. No record at all is a different
 * answer and an ordinary one: a registered repository that declares nothing.
 *
 * @throws ProjectError when the project's memory would not answer.
 */
export async function openRegistered(
  key: string,
  name: string,
): Promise<OpenProject> {
  const known = await loadProjectSettings(key);
  if (known.memoryError !== null) {
    // The computer's own words and no kind of ours. Nothing branches on this —
    // what a person is shown is the sentence the memory refused with, where
    // they asked for the project.
    throw new Error(known.memoryError);
  }
  return {
    ...(known.settings ?? {
      name,
      identifier: key,
      description: "",
      language: "en",
      installed: [],
    }),
    path: key,
  };
}

/**
 * The project this phone was last looking at, and where it is written down.
 *
 * Only a phone has these: on a Mac a window is the application and a reload is
 * something a person did, while here the system reloads the webview on its own
 * — coming back from the background, or reclaiming the content process — and
 * everything the window was holding goes with it. Which project somebody had
 * open is the piece of that they would notice.
 *
 * The key and nothing else. What the project is called and what it declares are
 * read from the computer the same way they are read when somebody taps a row,
 * so a project renamed or removed while this phone was away is answered by the
 * computer rather than out of a copy that went stale in a pocket.
 */
export function heldPlace(): Promise<string | null> {
  return call<string | null>("place_held", {});
}

/** Write down where this phone is, or that it is nowhere. */
export function holdPlace(project: string | null): Promise<void> {
  return call<void>("place_hold", { project });
}

/** The projects this installation answers for. */
export function registeredProjects(): Promise<readonly RegisteredProject[]> {
  return call<readonly RegisteredProject[]>("projects_registered", {});
}

/**
 * Register a project, or find out which one already answers to its identifier.
 *
 * Nothing is written when the answer carries `takenBy`: which project gives way
 * is a person's decision, and inventing a name here would be the machine-local
 * identifier the whole scheme exists to avoid.
 */
export function registerProject(
  project: RegisteredProject,
): Promise<Registration> {
  return call<Registration>("project_register", { project });
}

/**
 * Forget the project at `path`: out of the menu, and no longer answered for.
 *
 * One gesture rather than two, because a person asking to be rid of a project
 * does not distinguish between the menu they can see and the registry they
 * cannot. Answers with the recent list, which is what the window redraws.
 */
export function forgetProject(path: string): Promise<readonly RecentProject[]> {
  return call<readonly RecentProject[]>("project_forget", { path });
}

/**
 * The identifier a project of this name would get.
 *
 * Asked for rather than derived here: the rule lives in one place, and a window
 * that computed its own would eventually compute a different one.
 */
export function suggestProjectIdentifier(name: string): Promise<string> {
  return call<string>("project_identifier_suggest", { name });
}

/** Write the project's record, creating its memory on the first write. */
export function saveProjectSettings(
  path: string,
  settings: ProjectSettings,
): Promise<void> {
  return call<void>("project_settings_save", { project: path, settings });
}

/** The projects this installation has opened, most recent first. */
export function loadRecentProjects(): Promise<readonly RecentProject[]> {
  return call<RecentProject[]>("recent_projects_load", {});
}

/** Move a project to the front of that list. */
export function recordRecentProject(
  project: RecentProject,
): Promise<readonly RecentProject[]> {
  return call<RecentProject[]>("recent_projects_record", { project });
}

/** What this installation shows of a project, as opposed to what it holds. */
export function loadProjectView(path: string): Promise<ProjectView> {
  return call<ProjectView>("project_view_load", { project: path });
}

/**
 * Remember it. The answer is the whole view as it now stands, so callers can
 * trust it — including the half of it they did not write.
 */
export function saveProjectView(
  path: string,
  view: ProjectViewChange,
): Promise<ProjectView> {
  return call<ProjectView>("project_view_save", { project: path, view });
}
