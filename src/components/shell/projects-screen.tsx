"use client";

import { useCallback, useEffect, useState } from "react";
import { FolderGit2, SlidersHorizontal } from "lucide-react";

import { BarButton, WindowBar } from "@/components/shell/mobile-chrome";
import { openRegistered, registeredProjects } from "@/lib/project/client";
import type { OpenProject } from "@/lib/project/types";
import { said } from "@/lib/refusal";

/**
 * The window with a computer and no project open, on a phone.
 *
 * The Mac's version of this screen offers a folder picker and the projects this
 * installation opened before. Neither is here, and neither is missing: a phone
 * is not let near a file system, so what it chooses between is the projects the
 * computer already answers for, by the keys that computer registered them
 * under. There is nothing to create here and nothing to browse — a project
 * comes into existence on the machine that holds its repository.
 *
 * **A project is named and not located.** The Mac's recent list shows a path
 * beside each name because two folders of the same name are otherwise the same
 * row. Here there cannot be two: a key is unique to the machine by
 * construction, and the door does not send a path at all.
 *
 * The list is read once, when this screen appears. A project registered on the
 * computer while somebody is looking at their phone is a rare enough event to
 * cost a pull rather than a subscription, and there is nothing here to pull
 * yet — so it is read again by leaving the screen and coming back, which is
 * what closing a project already does.
 */
export function ProjectsScreen({
  onOpened,
  onOpenSettings,
}: {
  onOpened: (project: OpenProject) => void;
  /**
   * What this phone is, in the band under the list.
   *
   * This is the root of the phone, and until now it was the one screen with no
   * way off it but into a project: somebody whose computer had stopped
   * answering could read that there were no projects and had nothing to do
   * about it. What the band leads to is where they can see what this phone
   * dials, and take it off that computer.
   */
  onOpenSettings: () => void;
}) {
  const [projects, setProjects] = useState<readonly Listed[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);

  useEffect(() => {
    let listening = true;
    void registeredProjects().then(
      (listed) => {
        if (listening) setProjects(listed);
      },
      (refused: unknown) => {
        if (!listening) return;
        setProjects([]);
        setFailure(said(refused));
      },
    );
    return () => {
      listening = false;
    };
  }, []);

  /**
   * Open one, having asked the project what it calls itself.
   *
   * The registry's name is what the person picked from, and the project's own
   * record is what the window is titled and addressed by afterwards — the same
   * two the Mac's opening flow reads, minus every step of it that is about a
   * directory. A project whose record cannot be read is opened under the name
   * the registry has: the window is honest either way, and refusing to open a
   * project because its memory is busy would be a phone with a list it cannot
   * use.
   */
  const open = useCallback(
    async (project: Listed) => {
      setOpening(project.path);
      setFailure(null);
      try {
        // The same function the window calls when it comes back to a project by
        // itself, after the system reloaded the webview. One opening flow, so a
        // project reached the second way is the same project in every respect
        // as one somebody tapped.
        onOpened(await openRegistered(project.path, project.name));
      } catch (refused: unknown) {
        setFailure(said(refused));
      } finally {
        setOpening(null);
      }
    },
    [onOpened],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-workspace">
      <div
        className="flex min-h-0 flex-1 flex-col overflow-y-auto px-4"
        // The head a phone keeps for itself, asked for rather than measured. The
        // foot is not padded here any more: the band below is outside this
        // scroller and keeps its own clearance, so a list long enough to scroll
        // ends against a bar rather than against the hardware.
        style={{ paddingTop: "max(1rem, env(safe-area-inset-top))" }}
      >
        <h1 className="px-2 pt-2 pb-4 text-2xl font-semibold text-fg">Projects</h1>

        {projects === null ? null : projects.length === 0 ? (
          <Nothing failure={failure} />
        ) : (
          <ul className="flex flex-col gap-px">
            {projects.map((project) => (
              <li key={project.path}>
                <button
                  type="button"
                  disabled={opening !== null}
                  onClick={() => void open(project)}
                  className="flex w-full items-center gap-3 rounded-(--radius-control) px-2 py-3 text-left transition-colors duration-(--motion-duration-fast) ease-shell active:bg-hover disabled:opacity-50"
                >
                  <span
                    aria-hidden="true"
                    className="flex size-8 shrink-0 items-center justify-center rounded-(--radius-surface) border border-separator-strong bg-panel text-fg-secondary"
                  >
                    <FolderGit2 className="size-4" />
                  </span>
                  <span className="min-w-0 flex-1 truncate text-base text-fg">
                    {project.name}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}

        {failure !== null && projects !== null && projects.length > 0 ? (
          <p className="px-2 pt-4 text-sm text-danger">{failure}</p>
        ) : null}
      </div>

      {/* One control, at the end of the screen — the same place, with the same
          icon, that the list of a project's sections keeps it in. Two roots and
          one habit: a person learns where this is once. */}
      <div
        className="shrink-0 border-t border-separator"
        style={{ paddingBottom: "env(safe-area-inset-bottom, 0px)" }}
      >
        <WindowBar>
          {/* The leading end, empty. A band with one control puts it at the
              trailing end, which is where the same control is on the screen a
              project opens on to. */}
          <span aria-hidden />
          <BarButton
            label="Settings"
            icon={SlidersHorizontal}
            onPress={onOpenSettings}
          />
        </WindowBar>
      </div>
    </div>
  );
}

/**
 * What the registry says a project is called, and the handle to ask about it.
 *
 * The shape is the Mac's registry entry, and `path` carries the key rather than
 * a directory — which is what the window has always treated it as: a handle it
 * passes back unread.
 */
interface Listed {
  readonly path: string;
  readonly name: string;
  readonly identifier: string;
}

/**
 * The computer has no projects, or would not say.
 *
 * Two sentences rather than one, because they are answered in two different
 * places: an empty registry is fixed on the computer, and a refusal is the
 * computer's own words about why it could not be read.
 */
function Nothing({ failure }: { failure: string | null }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <p className="text-base text-fg">
        {failure === null
          ? "That computer holds no projects"
          : "The projects could not be read"}
      </p>
      <p className="max-w-[38ch] text-sm text-fg-secondary">
        {failure ??
          "Open a folder as a project on the computer, and it will be here."}
      </p>
    </div>
  );
}
