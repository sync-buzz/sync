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
import { useAddressed, useWindowHolds } from "@/lib/attention";
import { useLinked } from "@/lib/deep-link";
import { useLatestIntent } from "@/lib/sent-to";
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
  /** The last project this phone was in, for the list it comes back to. */
  const [leftFrom, setLeftFrom] = useState<string | null>(null);
  // Where this phone was before the system reloaded its webview, and where it
  // is now. Only a phone has either: on a Mac a reload is something a person
  // did, and the window they did it in is the one they get back.
  const place = usePlace(isPhone);
  // Entering a project and writing down that this is where the phone is are one
  // act, so they are one function. Two would be a navigation somebody could add
  // without the second half, which reads as working until the next reload.
  const enter = useCallback(
    (opened: OpenProject | null) => {
      // Which project the wheel is turned to when the list is next drawn.
      // Written on the way *in* rather than on the way out, and that is what
      // keeps it out of the closing path: the last project entered is the same
      // answer as the last project left, and asking for it here needs no
      // knowledge of what is standing open.
      if (opened !== null) setLeftFrom(opened.path);
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
  // Raised from the list of projects, which is the root of this phone and the
  // only screen this belongs on: what it holds is which computer the phone
  // dials, which is true of none of the projects on it. Held here rather than
  // in the list because what it can do — forget that computer — is a fact this
  // component renders around, and a project standing open has to go with it.
  const [settingsOpen, setSettingsOpen] = useState(false);
  const setup = useProjectSetup({ onOpened: enter });
  // The window is named after what it holds, for the lists the system draws of
  // it — the Dock icon's menu above all, which is where a second window is
  // asked for and where every window of an application is offered back.
  useWindowTitle(project?.name ?? null);
  // The same fact said to Rust rather than to the system, and for a different
  // purpose: a banner is raised with no window open and clicked from another
  // application, so something has to know which window that project belongs in
  // before there is anybody to ask.
  useWindowHolds(project?.path ?? null);
  // What such a click asked for, once this window has answered the half of it
  // that is opening the project. A `sync://` address followed from anywhere on
  // the machine is the same ask from another direction, and whichever of them
  // arrived last is what the window is showing: preferring one would swallow an
  // address followed while a banner's record was still open, or the reverse.
  const addressed = useAddressed(project, enter);
  const linked = useLinked(project, enter);
  const shown = useLatestIntent(addressed, linked);

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
            is always false.

            A phone that has a computer and is not reaching it is the same
            window for the same reason — everything it draws is read from that
            computer — so it stands here rather than in an element of its own,
            saying which of the two is true. `pairing.standing` has already
            waited out the drops that mend themselves; by the time it is not
            `null` there is something a person needs to know. */}
        <LaunchScreen
          isLoading={
            isLoading ||
            pairing.isAsking ||
            place.holding ||
            pairing.standing !== null
          }
          saying={
            pairing.standing === null
              ? "Starting"
              : pairing.standing.reaching
                ? "Connecting"
                : "Not connected"
          }
          trouble={pairing.standing?.trouble ?? null}
          onRetry={() => void pairing.reachNow()}
        />

        {pairing.needed ? (
          <PairingScreen pairing={pairing} />
        ) : project ? (
          <ProjectWindow
            project={project}
            setup={setup}
            shown={shown}
            onProjectChanged={enter}
            // Only where there is a list to go back to. A Mac closes a project
            // by closing its window, and a phone has neither a second window
            // nor a way to shut the one it has.
            onLeave={isPhone ? () => enter(null) : undefined}
          />
        ) : isPhone ? (
          // The same place in the composition and a different question, because
          // on a phone it is a different question. A Mac with no project open
          // asks which folder; a phone cannot ask that — it has no file system
          // to offer and no project of its own to make — so it asks which of
          // the computer's projects, and the toolbar goes with the folder
          // picker rather than being drawn empty above a list.
          <ProjectsScreen
            startAt={leftFrom}
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
