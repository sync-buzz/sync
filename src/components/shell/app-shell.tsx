"use client";

import { useState } from "react";
import { AppHeader } from "@/components/shell/app-header";
import { LaunchScreen } from "@/components/shell/launch-screen";
import {
  ProjectSetupSheet,
  useProjectSetup,
} from "@/components/shell/project-setup";
import { PairingScreen } from "@/components/shell/pairing-screen";
import { ProjectsScreen } from "@/components/shell/projects-screen";
import { ProjectWindow } from "@/components/shell/project-window";
import { WelcomeScreen } from "@/components/shell/welcome";
import type { OpenProject } from "@/lib/project/types";
import { useAppMenu } from "@/lib/app-menu";
import { useDevice } from "@/lib/device";
import { usePairing } from "@/lib/pairing";
import { useWindowMaterial } from "@/lib/window-material";
import { useWindowReveal } from "@/lib/window-reveal";
import { useWindowTitle } from "@/lib/window-title";

/**
 * The application window.
 *
 * The window is a frame and a slab. The frame is the tinted material, visible
 * as a narrow border on all four sides; the slab is the entire interface —
 * toolbar and everything under it — as one opaque rounded surface inset into
 * it. Everything inside the slab is flush and opaque, so the only place glass
 * appears is the edge of the window.
 *
 * The slab is one element, so its rounding and its shadow are stated once and
 * survive any arrangement of columns: nothing has to be re-derived when a panel
 * collapses, or when there are no columns at all.
 *
 * What the slab holds is decided by one fact — whether a project is open. This
 * component owns that fact and the flow that changes it, and nothing else: the
 * two windows below own their own layout and selection state.
 *
 * On a phone one question stands in front of that fact: whether this window has
 * a computer to ask at all. It is asked only there, because on a Mac the
 * machine the window runs on is the machine that answers, and the hook returns
 * a settled no rather than a state the desktop has to render around.
 */
export function AppShell() {
  useWindowMaterial();
  // The menu bar belongs to the application rather than to a window, so the
  // settings window inherits it rather than building a second one. It is
  // installed here with nothing to create, and the open project replaces it
  // with a File menu of its own kinds: with no project there is no kind, and a
  // window that cannot make anything says so with a disabled command rather
  // than by leaving the command out.
  useAppMenu(null);
  const isLoading = useWindowReveal();
  const pairing = usePairing();
  const isPhone = useDevice() === "phone";

  const [project, setProject] = useState<OpenProject | null>(null);
  const setup = useProjectSetup({ onOpened: setProject });
  // The window is named after what it holds, for the lists the system draws of
  // it — the Dock icon's menu above all, which is where a second window is
  // asked for and where every window of an application is offered back.
  useWindowTitle(project?.name ?? null);

  return (
    <div className="h-full bg-window p-(--window-inset) text-fg">
      {/* `clip`, so the slab cannot hold a scroll offset of its own — the same
          reason `body` is clipped rather than hidden, which `globals.css`
          states in full: a hidden box goes on being scrolled by the browser,
          and nothing is left to scroll it back. */}
      <div className="relative flex h-full flex-col overflow-clip rounded-(--radius-window) shadow-(--shadow-content)">
        {/* A phone that does not yet know whether it has a computer is a
            window that is still starting, and says the one thing it can say.
            Nothing is held back to show it: on a Mac the second half of this
            is always false. */}
        <LaunchScreen isLoading={isLoading || pairing.isAsking} />

        {pairing.needed ? (
          <PairingScreen pairing={pairing} />
        ) : project ? (
          <ProjectWindow
            project={project}
            setup={setup}
            onProjectChanged={setProject}
          />
        ) : isPhone ? (
          // The same place in the composition and a different question, because
          // on a phone it is a different question. A Mac with no project open
          // asks which folder; a phone cannot ask that — it has no file system
          // to offer and no project of its own to make — so it asks which of
          // the computer's projects, and the toolbar goes with the folder
          // picker rather than being drawn empty above a list.
          <ProjectsScreen onOpened={setProject} />
        ) : (
          <>
            <AppHeader project={null} setup={setup} />
            <WelcomeScreen setup={setup} />
          </>
        )}
      </div>

      <ProjectSetupSheet setup={setup} />
    </div>
  );
}
