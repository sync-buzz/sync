"use client";

import { useCallback, useEffect, useState } from "react";
import { SlidersHorizontal } from "lucide-react";

import { ProjectList } from "@/components/shell/project-list";
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
  startAt,
  onOpened,
  onOpenSettings,
}: {
  /**
   * The project this phone was in a moment ago, brought into view when the
   * list appears rather than being left to be scrolled for.
   *
   * Held by the window above and not asked of the computer: what the computer
   * keeps is where the phone *is*, and this is a fact about where it has just
   * been. A phone whose webview was reloaded has neither, and opens at the top
   * — which is the same screen as before and no worse than it was.
   */
  startAt?: string | null;
  onOpened: (project: OpenProject) => void;
  /**
   * What this phone is, beside the title.
   *
   * This is the root of the phone and the only screen this belongs on. A
   * project pushed in front of it reaches it by its own back button, which is
   * drawn at every depth — so a person inside a project is one press from
   * seeing what this phone dials, and from taking it off that computer.
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

  if (projects === null) {
    // Nothing at all rather than an empty list: it arrives in a moment, and a
    // list that fills itself reads as a list that had lost something.
    return <div className="min-h-0 flex-1 bg-workspace" />;
  }

  if (projects.length === 0) {
    return <Nothing failure={failure} onOpenSettings={onOpenSettings} />;
  }

  return (
    <ProjectList
      projects={projects.map((project) => ({
        key: project.path,
        name: project.name,
      }))}
      startAt={startAt ?? undefined}
      busy={opening !== null}
      onOpen={(chosen) => {
        const project = projects.find((one) => one.path === chosen.key);
        if (project !== undefined) void open(project);
      }}
      // The one control that is not about a project, beside the title. This is
      // the root of the phone and the only screen the way off this application
      // belongs on: everything else is pushed in front of it and reaches it
      // with one press of a back button that is always drawn.
      trailing={
        <button
          type="button"
          aria-label="Settings"
          onClick={onOpenSettings}
          className="flex size-11 shrink-0 items-center justify-center rounded-(--radius-control) text-fg-secondary active:bg-hover"
        >
          <SlidersHorizontal className="size-5" />
        </button>
      }
      failure={failure}
    />
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
function Nothing({
  failure,
  onOpenSettings,
}: {
  failure: string | null;
  onOpenSettings: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 bg-workspace px-6 text-center">
      <p className="text-base text-fg">
        {failure === null
          ? "That computer holds no projects"
          : "The projects could not be read"}
      </p>
      <p className="max-w-[38ch] text-sm text-fg-secondary">
        {failure ??
          "Add a project on the computer, and it will be here."}
      </p>
      {/* The way off a screen that otherwise has none: somebody whose computer
          has stopped answering can read that there are no projects and change
          which computer this phone dials. */}
      <button
        type="button"
        onClick={onOpenSettings}
        className="mt-4 flex h-11 items-center gap-2 rounded-(--radius-control) px-3 text-[17px] leading-[22px] text-focus active:bg-hover"
      >
        <SlidersHorizontal className="size-5" />
        Settings
      </button>
    </div>
  );
}
