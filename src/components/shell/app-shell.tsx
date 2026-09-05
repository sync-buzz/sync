"use client";

import { useCallback, useState } from "react";
import { AppHeader } from "@/components/shell/app-header";
import { LaunchScreen } from "@/components/shell/launch-screen";
import {
  ProjectSetupSheet,
  useProjectSetup,
} from "@/components/shell/project-setup";
import { PairingScreen } from "@/components/shell/pairing-screen";
import { ProjectsScreen } from "@/components/shell/projects-screen";
import { SettingsSheet } from "@/components/shell/mobile-settings";
import { ProjectWindow } from "@/components/shell/project-window";
import { WelcomeScreen } from "@/components/shell/welcome";
import type { OpenProject } from "@/lib/project/types";
import { usePlace } from "@/lib/project/use-place";
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
  // Where this phone was before the system reloaded its webview, and where it
  // is now. Only a phone has either: on a Mac a reload is something a person
  // did, and the window they did it in is the one they get back.
  const place = usePlace(isPhone);
  // Entering a project and writing down that this is where the phone is are one
  // act, so they are one function. Two would be a navigation somebody could add
  // without the second half, which reads as working until the next reload.
  const enter = useCallback(
    (opened: OpenProject | null) => {
      setProject(opened);
      place.hold(opened);
    },
    [place],
  );
  // Read during the render that has the answer rather than in an effect after
  // it, the same way the pairing reset below is: an effect would draw the list
  // of projects for one frame under somebody coming back to their work.
  //
  // Once per project, and the second piece of state is what makes it once.
  // Without it, closing the project would put the person straight back into
  // it: what this restored from is still the answer, and *no project open* is
  // exactly the condition it fires on.
  const [returnedTo, setReturnedTo] = useState<string | null>(null);
  if (place.restored !== null && place.restored.path !== returnedTo) {
    setReturnedTo(place.restored.path);
    setProject(place.restored);
  }
  // Raised over whatever the window is showing, from either of the two screens
  // that belong to the window rather than to a package. Held here rather than
  // in each of them because it is one sheet about one phone, and because what
  // it can do — forget the computer — is a fact this component renders around.
  const [settingsOpen, setSettingsOpen] = useState(false);
  const setup = useProjectSetup({ onOpened: enter });
  // The window is named after what it holds, for the lists the system draws of
  // it — the Dock icon's menu above all, which is where a second window is
  // asked for and where every window of an application is offered back.
  useWindowTitle(project?.name ?? null);

  // The computer was forgotten, so everything that was read from it goes with
  // it: the sheet that did it, and the project it was raised over. The project
  // is dropped rather than left standing because it would otherwise come back
  // the moment this phone was paired to a *different* computer, under a key
  // that machine may never have heard of.
  //
  // Read during the render that shows the pairing screen rather than in an
  // effect after it, the way the phone's own window reads an intent: an effect
  // would draw the settings sheet over the pairing screen for one frame.
  if (pairing.needed && (settingsOpen || project !== null)) {
    setSettingsOpen(false);
    setProject(null);
  }

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
        <LaunchScreen
          isLoading={isLoading || pairing.isAsking || place.holding}
        />

        {pairing.needed ? (
          <PairingScreen pairing={pairing} />
        ) : project ? (
          <ProjectWindow
            project={project}
            setup={setup}
            onProjectChanged={enter}
            // Only where there is a list to go back to. A Mac closes a project
            // by closing its window, and a phone has neither a second window
            // nor a way to shut the one it has.
            onLeave={isPhone ? () => enter(null) : undefined}
            onOpenSettings={
              isPhone ? () => setSettingsOpen(true) : undefined
            }
          />
        ) : isPhone ? (
          // The same place in the composition and a different question, because
          // on a phone it is a different question. A Mac with no project open
          // asks which folder; a phone cannot ask that — it has no file system
          // to offer and no project of its own to make — so it asks which of
          // the computer's projects, and the toolbar goes with the folder
          // picker rather than being drawn empty above a list.
          <ProjectsScreen
            onOpened={enter}
            onOpenSettings={() => setSettingsOpen(true)}
          />
        ) : (
          <>
            <AppHeader project={null} setup={setup} />
            <WelcomeScreen setup={setup} />
          </>
        )}

        {/* Inside the slab and over everything in it, which is what makes it a
            sheet rather than a screen: the window it was raised from stays
            visible above it. Only on a phone — a Mac reaches the same settings
            as a window of its own, and drawing this there would be two answers
            to one question. */}
        {isPhone ? (
          <SettingsSheet
            open={settingsOpen}
            pairing={pairing}
            onClose={() => setSettingsOpen(false)}
          />
        ) : null}
      </div>

      <ProjectSetupSheet setup={setup} />
    </div>
  );
}
