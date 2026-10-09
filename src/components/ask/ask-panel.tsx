"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { AskLine } from "@/components/ask/ask-line";
import { AskProjects } from "@/components/ask/ask-projects";
import { ProjectWindow } from "@/components/shell/project-window";
import { useProjectSetup } from "@/components/shell/project-setup";
import {
  dismissPanel,
  panelHeight,
  panelProject,
  panelUse,
  useOpened,
} from "@/lib/ask/client";
import { openRegistered } from "@/lib/project/client";
import type { OpenProject, RecentProject } from "@/lib/project/types";

/**
 * The panel a key opens over whatever application is in front.
 *
 * **It is a surface, not a window.** The whole document is one slab the width
 * of the window, and the window is resized to it on every change — see
 * [`panelHeight`]. A transparent region the panel covers and does not draw is a
 * place where a click lands on nothing, which over somebody else's application
 * is a hole rather than a margin.
 *
 * **Two things it can be showing, and the project decides which.** A line with
 * the project's own panels under it, when it knows which project it is speaking
 * for; the list of projects, when it does not. Nothing is guessed: a line typed
 * into a panel that picked a project on its own would be work started in a
 * repository nobody named.
 *
 * **The panels under the line are the window's, run here.** The project is
 * opened the way a window opens one and handed to the same component — the
 * areas, their columns, their providers and everything the window knows about
 * them are built once and arranged three ways, of which this is the third. A
 * second host for this surface would be a second place every extension has to
 * work, and the one that is not tested is the one that breaks.
 *
 * **Every key it answers is a position rather than a character.** `event.code`
 * and never `event.key`, for the reason the shortcut that opens it is a code:
 * this surface is reached while working in another application, which is
 * exactly when somebody's layout is whatever that work needed.
 */
export function AskPanel() {
  /**
   * The project, or nothing, or *not asked yet*.
   *
   * Three states rather than two, and the third is what keeps the panel from
   * flashing the list of projects on the frame before the answer lands — which
   * would be the panel offering a decision it is about to withdraw.
   */
  const [speaking, setSpeaking] = useState<RecentProject | null | undefined>(
    undefined,
  );
  /** Whether the list is up, which Escape takes back when there is a project. */
  const [choosing, setChoosing] = useState(false);
  /**
   * How many times the key has opened this panel.
   *
   * The line reads it to take the caret and start again. A boolean would not
   * do: the panel is opened against the same project many times, and each of
   * those is the same value arriving twice.
   */
  const [opened, setOpened] = useState(0);
  /**
   * The project as a window holds it, which is what the panels under the line
   * need: what it declares, and what it answers to.
   *
   * Read from the project the panel speaks for rather than carried with it. The
   * answer to *which project* is a path and a name — that is all a key pressed
   * in another application can know — and what the project is composed of lives
   * in its own memory.
   */
  const [open, setOpen] = useState<OpenProject | null>(null);
  /**
   * What is in the line, held here because two things read it: the line draws
   * it, and the board under it is standing on the answer to it.
   */
  const [query, setQuery] = useState("");
  /**
   * Whether a conversation with an agent is standing in the line.
   *
   * What it decides is what the panel is showing at all: a conversation is read
   * on its own, with the project's panels taken away under it. Two subjects on
   * one surface the width of a sentence is neither of them, and the one
   * somebody is in the middle of is the one they addressed.
   */
  const [talking, setTalking] = useState(false);
  /**
   * The field, held here because two things reach for it: the line draws it,
   * and the board hands the keyboard back to it when it is done with a column.
   */
  const field = useRef<HTMLTextAreaElement | null>(null);
  const focusLine = useCallback(() => field.current?.focus(), []);
  // Opening a project is the flow a window uses, and it is here for one of its
  // steps: installing an extension from the catalogue in this surface opens a
  // project again. Nothing of its own is drawn — the sheets it raises belong to
  // a window, and this is not one.
  const setup = useProjectSetup({ onOpened: setOpen });

  /**
   * Take an answer about which project the panel is for.
   *
   * `null` arrives two ways and both mean the same thing here: nobody had a
   * project open, or the application could not say. There is nowhere in this
   * surface to put a message about the second, and the list of projects is
   * already what a person has to do next in either case.
   */
  const settle = useCallback((project: RecentProject | null) => {
    setSpeaking(project);
    setChoosing(project === null);
  }, []);

  // Asked once, on the way up. In the promise rather than in the effect's body
  // so the answer lands as its own update: the first frame of this panel is
  // drawn before Rust has replied, which is what the third state above is for.
  useEffect(() => {
    let reading = true;
    void panelProject().then(
      (project) => {
        if (reading) settle(project);
      },
      () => {
        if (reading) settle(null);
      },
    );
    return () => {
      reading = false;
    };
  }, [settle]);

  // And again each time the key opens it, rather than only on the first mount:
  // the panel is built once at launch and shown for the life of the process, so
  // what was in front has very probably changed since the last line.
  useOpened(() => {
    setOpened((count) => count + 1);
    void panelProject().then(settle, () => settle(null));
  });

  // A click that landed in another application puts the panel away. That is
  // what this system does with a surface like this, and the alternative — a
  // panel still floating over the thing somebody has gone back to — is the one
  // behaviour nobody would call native.
  useEffect(() => {
    const away = () => void dismissPanel();
    window.addEventListener("blur", away);
    return () => window.removeEventListener("blur", away);
  }, []);

  // The document says which window it is, so the stylesheet can take the
  // background off this one. In a layout effect because it has to be true of
  // the first frame: a frame with the window colour behind the slab is a frame
  // of an opaque rectangle where the material should be.
  useLayoutEffect(() => {
    document.documentElement.dataset.surface = "ask";
    return () => {
      delete document.documentElement.dataset.surface;
    };
  }, []);

  // What the panel speaks for, opened. Asked each time that changes rather
  // than once: the key is pressed against whatever window was in front, and a
  // project's declaration moves under the panel while it is hidden — somebody
  // installs an extension in the window, and the panels here are the project's
  // from before.
  const path = speaking?.path ?? null;
  const named = speaking?.name ?? null;
  // A project that is no longer the one the panel speaks for is dropped during
  // the render that lost it rather than in an effect after it: an effect would
  // leave one frame of the previous project's panels under a line that is
  // already about the next one.
  if (open !== null && open.path !== path) setOpen(null);
  useEffect(() => {
    if (path === null || named === null) return;
    let reading = true;
    void openRegistered(path, named).then(
      (project) => {
        if (reading) setOpen(project);
      },
      () => {
        // A project whose memory would not answer is a panel with a line and
        // no panels under it, which is what `null` already draws. The sentence
        // the memory refused with belongs where somebody opened the project.
        if (reading) setOpen(null);
      },
    );
    return () => {
      reading = false;
    };
  }, [named, path]);

  const slab = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = slab.current;
    if (element === null) return;

    const measure = () =>
      void panelHeight(element.getBoundingClientRect().height);
    // Observed rather than measured after each render: what changes the height
    // is a list arriving, a block growing and a textarea wrapping, and only one
    // of those is a render this component does.
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, []);

  const chose = useCallback(
    async (project: RecentProject) => {
      // Told to Rust before the line is drawn, so the next time the key is
      // pressed the panel speaks for what somebody chose rather than for
      // whatever window happens to be in front.
      await panelUse(project.path);
      setSpeaking(project);
      setChoosing(false);
      setOpened((count) => count + 1);
    },
    [],
  );

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.code === "Escape") {
      event.preventDefault();
      // Out of the list and back to the line, when there is a line to go back
      // to. Escape means *undo this step*, and dismissing the whole panel would
      // undo two.
      if (choosing && speaking !== null && speaking !== undefined) {
        setChoosing(false);
        setOpened((count) => count + 1);
        return;
      }
      void dismissPanel();
      return;
    }

    // The project, by the key every application on this system uses to open the
    // thing you are working on. It is a toggle for the same reason the panel's
    // own key is one.
    if (event.code === "KeyP" && event.metaKey) {
      event.preventDefault();
      if (choosing && speaking !== null && speaking !== undefined) {
        setChoosing(false);
        setOpened((count) => count + 1);
        return;
      }
      setChoosing(true);
    }
  };

  return (
    // The slab is the document. Rounded to the same radius AppKit rounds the
    // material behind it, and drawn with a hairline over it so the edge is
    // visible against a light desktop as well as a dark one.
    <div
      ref={slab}
      onKeyDown={onKeyDown}
      // The surface over the material, and both are needed. The material is
      // what blurs the desktop; this is what the text stands on — without it
      // the panel is a window onto somebody's wallpaper with words floating in
      // it, and nothing in this application is legible against an image. It is
      // the shade's own surface, which is already designed in both appearances
      // and already hardens to an opaque one for somebody who asked for less
      // transparency.
      style={{ backgroundColor: "var(--surface-console)" }}
      className="flex w-full flex-col overflow-hidden rounded-xl border border-separator-strong"
    >
      {speaking === undefined ? (
        // Nothing yet. Deliberately empty rather than a spinner: the answer is
        // one call to Rust and arrives inside a frame or two, and a surface
        // that flashed a progress glyph at every opening would be reporting a
        // wait nobody can feel.
        <div className="h-16" />
      ) : choosing || speaking === null ? (
        <AskProjects
          speaking={speaking}
          opened={opened}
          onChoose={(project) => void chose(project)}
        />
      ) : (
        <>
          <AskLine
            project={speaking}
            opened={opened}
            onField={(element) => {
              field.current = element;
            }}
            onQuery={setQuery}
            onTalking={setTalking}
            onChooseProject={() => setChoosing(true)}
          />
          {/* Keyed by the project, which is how its panels leave with it: the
              areas a project runs are what its own record declares, and this
              surface is not rebuilt when the project under it changes. */}
          {open === null ? null : (
            <ProjectWindow
              key={open.path}
              project={open}
              setup={setup}
              panel={{ query, talking, focusLine }}
              onProjectChanged={setOpen}
            />
          )}
        </>
      )}
    </div>
  );
}
