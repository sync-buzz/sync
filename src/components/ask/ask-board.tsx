"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

import { OFFERED } from "@/components/ask/ask-line";
import { AskResults } from "@/components/ask/ask-results";
import { PrimarySidebar } from "@/components/shell/primary-sidebar";
import { dismissPanel } from "@/lib/ask/client";
import { followLink } from "@/lib/deep-link";
import type { MountedArea } from "@/lib/extension-host/areas";
import type { Badges } from "@/lib/extension-host/badges";
import { useSearch } from "@/lib/memory/use-search";
import type { SearchHit } from "@/lib/memory/types";
import type { OpenProject } from "@/lib/project/types";
import { recordHref } from "@/lib/record-link";
import { FRAMES } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";

/**
 * What stands under the line, one panel at a time.
 *
 * **One track, two kinds of panel, and the whole arrangement is that they are
 * on the same track.** What the line produced is temporary — the records a
 * question matched, gone when the question is — and what the project *has* is
 * permanent: its sections, and the columns of whichever section is chosen. A
 * person moves between the two with one gesture in one direction, because
 * putting them on separate surfaces would make *leaving a search* a different
 * act from *leaving a column*, which is two things to learn for one journey.
 *
 * **The columns are the window's own, untouched.** An area draws into the same
 * three slots by the same names; what differs is that here only one of them is
 * on the screen at a time and the panel slides between them. An extension
 * cannot tell this surface from a window, which is the same bargain the phone
 * makes.
 *
 * **It is as tall as itself and never as tall as its contents.** The panel is
 * resized to what it is showing, so a region that grew with a list would move
 * the line a person is typing into — and a surface over somebody else's
 * application that changes size under their hands is the one thing this panel
 * must not do. So the height is fixed and each panel scrolls inside it.
 *
 * **Which rows the keyboard walks is decided by where the track is standing**,
 * and the division with the line above is one rule: a line being addressed to
 * an agent keeps its own keys, because the list under it is then the names it
 * could be addressed to. Anything else and the keys belong here.
 */

/** Where the track can stand. */
type Stop = "results" | "sections" | "navigator" | "workspace" | "inspector";

/** The permanent stops, in the order the window draws them, for `⌘1`…`⌘4`. */
const PERMANENT: readonly Stop[] = ["sections", "navigator", "workspace", "inspector"];

export function AskBoard({
  project,
  query,
  talking,
  focusLine,
  sections,
  hiddenAreas,
  badges,
  updates,
  unseen,
  active,
  attachNavigator,
  attachWorkspace,
  attachInspector,
  onSelectArea,
  onArrange,
  onHide,
  onShow,
}: {
  project: OpenProject;
  /**
   * What is in the line above, exactly as typed.
   *
   * The question and the state of the track are the same fact read twice: a
   * question standing in the line *is* the temporary panel being in front, so
   * there is nothing to keep in step and no way for the two to disagree.
   */
  query: string;
  /**
   * Whether a conversation is standing in the line above.
   *
   * The board is kept and hidden rather than taken out of the tree: the columns
   * of every area a person has opened are inside it, and an area is never
   * unmounted in this application — being returned to a list at the top when
   * one comes back is the annoyance that rule exists to prevent.
   */
  talking: boolean;
  /** Give the keyboard back to the line, having taken it into a column. */
  focusLine: () => void;
  sections: readonly MountedArea[];
  hiddenAreas: readonly MountedArea[];
  badges: Badges;
  updates: number;
  unseen: number | null;
  /** The section showing, and `null` before there is one. */
  active: MountedArea | null;
  attachNavigator: (element: HTMLDivElement | null) => void;
  attachWorkspace: (element: HTMLDivElement | null) => void;
  attachInspector: (element: HTMLDivElement | null) => void;
  onSelectArea: (key: string) => void;
  onArrange: (order: readonly string[]) => void;
  onHide: (key: string) => void;
  onShow: (key: string) => void;
}) {
  /**
   * Whether the line is addressed to an agent rather than asking the project.
   *
   * Read here as well as in the line, from the same text: the keys are divided
   * by it, and a flag passed down would be a second answer to a question the
   * text already answers.
   */
  const mention = query.trimStart().startsWith("@");
  const asking = mention ? "" : query.trim();
  const answer = useSearch(project.path, asking, [], asking !== "");

  /** Where the track stands when the line is not asking anything. */
  const [held, setHeld] = useState<Stop>("sections");
  const frame = active === null ? FRAMES.single : FRAMES[active.frame];

  // Only the stops that exist. A frame without a navigator has no list to
  // stand on, and a track with an empty panel in the middle of it is a
  // movement that answers nothing.
  const stops: readonly Stop[] = [
    ...(asking === "" ? [] : (["results"] as const)),
    "sections",
    ...(frame.navigator ? (["navigator"] as const) : []),
    "workspace",
    ...(frame.inspector ? (["inspector"] as const) : []),
  ];

  // Asking is standing on the answer. Derived rather than written down, so
  // clearing the line puts a person back where they were without anything
  // having to remember to put them there.
  const standing: Stop = asking === "" ? held : "results";
  const at = Math.max(
    0,
    stops.indexOf(stops.includes(standing) ? standing : "sections"),
  );

  const hits = answer.hits.slice(0, OFFERED);
  /**
   * Which row the keyboard is on, and what it was on when it got there.
   *
   * Compared during the render that moved rather than reset in an effect after
   * it: a cursor left on the fourth row of an answer to a different question
   * points at something nobody is being offered.
   */
  const [picked, setPicked] = useState({ of: "", row: 0 });
  const of = `${standing}:${asking}`;
  if (picked.of !== of) setPicked({ of, row: 0 });
  const rows = standing === "results" ? hits.length : 0;
  const row = Math.min(picked.row, Math.max(0, rows - 1));

  /**
   * The box each panel stands in, so the keyboard can be put inside one.
   *
   * A column is an extension's and the shell cannot walk its rows — what they
   * are, how many and which is current are facts only the column has. What the
   * shell *can* do is give it the focus, and every list in this application
   * marks its current row as the one tab stop it has: so the keyboard is handed
   * to that row and the column's own arrows take over from there.
   */
  const boxes = useRef<Partial<Record<Stop, HTMLDivElement | null>>>({});
  const root = useRef<HTMLDivElement>(null);

  // Moving the track gives the keyboard back to the line, wherever it had got
  // to. Arriving at a panel is not entering it: somebody who has just stepped
  // sideways is as likely to type their next question as to walk what is in
  // front of them, and a surface called by a keystroke has to be typeable at
  // every moment. Walking into the rows is the arrows' own job, below.
  useEffect(() => {
    if (!talking) focusLine();
  }, [focusLine, standing, talking]);

  /** Read this record in a window, and get out of the way. */
  const follow = (hit: SearchHit) => {
    // A record with no kind has no address, and the row says so before it is
    // pressed. The project is named in the address because the router decides
    // which window shows it, and the window in front while this panel is up is
    // the panel.
    if (hit.kind === null) return;
    void followLink(
      recordHref({ kind: hit.kind, key: hit.id, project: project.identifier }),
    ).finally(() => void dismissPanel());
  };

  // Every key is read as a position rather than as a character, for the reason
  // the shortcut that opens the panel is a position: this surface is reached
  // while working in another application, which is exactly when somebody's
  // layout is whatever that work needed.
  //
  // On the window rather than on this element, because the caret never leaves
  // the line: there is one field in this panel and it keeps the keyboard, so
  // what the arrows mean is decided by where the track is standing and not by
  // what has the focus.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Something nearer the caret has already answered this — a column's own
      // arrows, a row that took Return. The board listens on the window, which
      // is last, so deferring to it is noticing that it acted.
      if (event.defaultPrevented) return;

      // Nothing while a conversation is standing in front of the line: the
      // board is hidden then, and a track that went on answering the arrows
      // would be moving a surface nobody can see — and swallowing the keys of
      // the one they are looking at.
      if (talking) return;

      // Somebody writing in a column is writing there. A panel can hold a
      // field of its own — a message to an agent, a title being renamed — and
      // every key this listens for is a key that field needs: the arrows are
      // its caret, Return sends what is in it, and a character is a character.
      // Only the commands with a modifier are still the panel's, because
      // nothing a field does is spelled with one.
      const writing = document.activeElement;
      if (
        !event.metaKey &&
        writing !== null &&
        root.current?.contains(writing) === true &&
        editable(writing)
      ) {
        return;
      }

      // A key that would be a character belongs to the line wherever else the
      // keyboard happens to be. The default action is left alone, so the
      // character lands in the field that has just taken the focus — which is
      // what makes a column something a person types their way out of rather
      // than a place they have to find the way back from.
      if (
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        (event.key.length === 1 || event.code === "Backspace")
      ) {
        focusLine();
        return;
      }

      // Straight to a panel, by its place in the window. The one way back to
      // the sections that costs nothing from any depth — which is what somebody
      // leaving one section for another needs, and the thing a track of pushes
      // is otherwise worst at.
      if (event.metaKey) {
        const index = ["Digit1", "Digit2", "Digit3", "Digit4"].indexOf(event.code);
        const wanted = index === -1 ? undefined : PERMANENT[index];
        if (wanted !== undefined && stops.includes(wanted)) {
          event.preventDefault();
          setHeld(wanted);
        }
        return;
      }

      // Sideways is the track, and only with nothing in the line: with a
      // question standing there the arrows are the caret's, which is what
      // anybody typing expects of them. `⌘1`…`⌘4` is the way across from a
      // line somebody is still writing.
      if (query === "" && (event.code === "ArrowRight" || event.code === "ArrowLeft")) {
        const next = at + (event.code === "ArrowRight" ? 1 : -1);
        if (next < 0 || next >= stops.length) return;
        event.preventDefault();
        const wanted = stops[next];
        if (wanted !== undefined && wanted !== "results") setHeld(wanted);
        return;
      }

      // The rows of whatever is in front. An addressed line keeps its own: the
      // list under it is then the names it could be addressed to.
      if (mention) return;

      if (event.code === "ArrowDown" || event.code === "ArrowUp") {
        const step = event.code === "ArrowDown" ? 1 : -1;
        if (standing === "sections") {
          // The selection follows the keyboard, which is what a source list
          // does on this system. There is no second cursor to keep beside it:
          // a section highlighted but not selected would be a column showing
          // one thing and a list pointing at another.
          event.preventDefault();
          if (sections.length === 0) return;
          const where = sections.findIndex((area) => area.key === active?.key);
          // What is selected is not always in this list: the window's own two
          // sections are drawn by the column rather than brought by the
          // project. From one of those the arrows enter the list at the end
          // they came from rather than counting from a row that is not there.
          const next =
            where === -1
              ? sections.at(step > 0 ? 0 : -1)
              : sections.at((where + step + sections.length) % sections.length);
          if (next !== undefined) onSelectArea(next.key);
          return;
        }
        if (standing !== "results") {
          // A column's rows are a package's, and a list that walks itself has
          // already answered this — see the check at the top, which is how a
          // list with its own arrows keeps them. What is left is every other
          // column, where the rows are ordinary controls: the shell moves the
          // focus between them and the one in front answers Return itself.
          const walk = rowsOf(boxes.current[standing]);
          if (walk.length === 0) return;
          const where = walk.findIndex((candidate) => candidate === document.activeElement);
          const next =
            where === -1
              ? (step > 0 ? walk[0] : walk.at(-1))
              : walk[Math.min(Math.max(where + step, 0), walk.length - 1)];
          if (next === undefined) return;
          event.preventDefault();
          next.focus();
          next.scrollIntoView({ block: "nearest" });
          return;
        }
        if (rows === 0) return;
        event.preventDefault();
        setPicked({ of, row: (row + step + rows) % rows });
        return;
      }

      if (event.code === "Enter" && !event.shiftKey) {
        if (standing === "results") {
          const chosen = hits[row];
          if (chosen === undefined) return;
          event.preventDefault();
          follow(chosen);
          return;
        }
        // Into the section that is selected, which is what choosing a row in a
        // list means at this width — the push the phone makes for the same
        // reason.
        if (standing === "sections" && stops.length > 1) {
          event.preventDefault();
          setHeld(frame.navigator ? "navigator" : "workspace");
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const panels: Readonly<Record<Stop, ReactNode>> = {
    results:
      hits.length > 0 ? (
        <AskResults
          project={project.path}
          hits={hits}
          cursor={row}
          onCursor={(next) => setPicked({ of, row: next })}
          onOpen={follow}
        />
      ) : (
        <Quiet
          saying={answer.isSearching ? "Searching…" : "Nothing matched."}
        />
      ),
    sections: (
      <PrimarySidebar
        sections={sections}
        hiddenAreas={hiddenAreas}
        badges={badges}
        updates={updates}
        unseen={unseen}
        activeAreaKey={active?.key ?? null}
        onSelectArea={(key) => {
          onSelectArea(key);
          setHeld(frame.navigator ? "navigator" : "workspace");
        }}
        onArrange={onArrange}
        onHide={onHide}
        onShow={onShow}
      />
    ),
    navigator: <AreaSlot attach={attachNavigator} />,
    workspace: <AreaSlot attach={attachWorkspace} />,
    inspector: <AreaSlot attach={attachInspector} />,
  };

  return (
    <div
      ref={root}
      hidden={talking}
      className="flex shrink-0 flex-col border-t border-separator"
      style={{ height: "var(--ask-board-height)" }}
    >
      {/* Where the track is standing and what is either side of it. The one
          piece of furniture this surface has, and it is here because a track
          of one visible panel is otherwise a place with no map: the names say
          what the arrows reach, and each of them is also the way there without
          the keyboard. */}
      <nav
        aria-label="Panels"
        className="flex h-7 shrink-0 items-center gap-1 overflow-clip px-4 text-xs"
      >
        {stops.map((stop, index) => (
          <span key={stop} className="flex min-w-0 items-center gap-1">
            {index === 0 ? null : (
              <span aria-hidden className="text-fg-tertiary">
                ›
              </span>
            )}
            <button
              type="button"
              aria-current={stop === standing ? "true" : undefined}
              // A stop reached by the line is not reached by pressing its name:
              // what put the answer in front was a question, and taking it away
              // is clearing one.
              disabled={stop === "results"}
              onClick={() => setHeld(stop)}
              className={cn(
                "truncate rounded-(--radius-control) px-1 py-0.5",
                stop === standing
                  ? "text-fg"
                  : "text-fg-tertiary enabled:hover:text-fg-secondary",
              )}
            >
              {name(stop, active)}
            </button>
          </span>
        ))}
      </nav>

      <div className="relative min-h-0 flex-1 overflow-clip">
        <div
          className="flex h-full transition-transform duration-(--motion-duration) ease-shell"
          style={{ transform: `translateX(-${at * 100}%)` }}
        >
          {stops.map((stop) => (
            <div
              key={stop}
              ref={(element) => {
                boxes.current[stop] = element;
              }}
              // Hidden from assistive technology as well as from the eye, and
              // inert: a panel standing off-screen is one nobody is looking at,
              // and a column of an area is a whole screen of controls.
              aria-hidden={stop !== standing}
              inert={stop !== standing}
              className="relative h-full w-full shrink-0 overflow-clip"
            >
              {panels[stop]}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/**
 * Whether what has the keyboard is something being written in.
 *
 * Asked of the element itself rather than of a list of tag names kept
 * somewhere: a package can put any field in its column, and the three things
 * this asks are what the platform means by *a person is typing here*.
 */
function editable(node: Element): boolean {
  return (
    node instanceof HTMLElement &&
    (node.isContentEditable ||
      node instanceof HTMLInputElement ||
      node instanceof HTMLTextAreaElement)
  );
}

/**
 * What the keyboard can stand on inside a column, in the order it is drawn.
 *
 * Every ordinary control, because that is what a row of a column is: a package
 * draws a list as buttons, and a button is already the thing that answers
 * Return. The bands are left out — the head and the foot of a column hold the
 * controls that act *on* the list, and arrows that walked into them would be
 * walking out of what is being read.
 */
function rowsOf(box: HTMLElement | null | undefined): readonly HTMLElement[] {
  if (box === null || box === undefined) return [];
  const found = box.querySelectorAll<HTMLElement>(
    'a[href], button:not([disabled]), [role="option"], [role="treeitem"], [tabindex="0"]',
  );
  return [...found].filter(
    (row) => row.closest("[data-panel-band]") === null && row.offsetParent !== null,
  );
}


/**
 * What a stop is called in the row above the track.
 *
 * The navigator is named after the section it lists, which is what the window's
 * own header says in that column. The two beside it are named by what they are
 * for, because what a package is showing in them is the package's to know and a
 * guess would be wrong exactly where it was read most.
 */
function name(stop: Stop, active: MountedArea | null): string {
  switch (stop) {
    case "results":
      return "Results";
    case "sections":
      return "Sections";
    case "navigator":
      return active?.label ?? "Section";
    case "workspace":
      return "Content";
    case "inspector":
      return "Details";
  }
}

/**
 * The node a column of an area is drawn into.
 *
 * The same shape the window's panels give it — a positioned box filling what it
 * is in — because that is the whole of what a column is promised anywhere in
 * this application: its own box, and its own scrolling inside it.
 */
function AreaSlot({ attach }: { attach: (element: HTMLDivElement | null) => void }) {
  return <div ref={attach} className="absolute inset-0" />;
}

/**
 * One line, where there is nothing to list.
 *
 * Quiet and centred, the way the launch screen says it is starting: a panel
 * with an empty list in it has to say which empty it is — nobody answered yet,
 * or nothing answers — and a surface this small has room for the sentence and
 * nothing else.
 */
function Quiet({ saying }: { saying: string }) {
  return (
    <p className="flex h-full items-center justify-center px-6 text-center text-sm text-fg-tertiary">
      {saying}
    </p>
  );
}
