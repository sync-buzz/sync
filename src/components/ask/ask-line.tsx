"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { AskConversation, useConversation } from "@/components/ask/ask-conversation";
import { ConsoleCanvas } from "@/components/shell/console-canvas";
import { ConsoleShelf } from "@/components/shell/console-shelf";
import { dismissPanel } from "@/lib/ask/client";
import { ghost, intent } from "@/lib/console/resolve";
import { followLink } from "@/lib/deep-link";
import { useConsole } from "@/lib/console/state";
import type { RecentProject } from "@/lib/project/types";
import { cn } from "@/lib/utils";

/**
 * The line the panel takes, and what it could become.
 *
 * **A line is a question asked of the project, unless it is addressed.** What
 * is typed is handed up as it is typed, and what the project answers stands
 * under this on the board — `ask-board.tsx`, which owns every panel there is
 * and the keys that move between them. A line beginning with `@` is the other
 * thing: a name, and what is being asked of the agent that answers to it, read
 * by `src/lib/console/resolve.ts` and run by the same `useConsole` a project
 * window runs.
 *
 * **An addressed line keeps its own keys, and that is the whole division.**
 * The list under an `@` is the names the line could be addressed to, so the
 * arrows, Tab and Return belong to this file while one is being written.
 * Anything else and they belong to the board, which is standing on something a
 * person is navigating rather than on something they are completing.
 *
 * **It is not the console shade, and the difference is the shape rather than
 * the vocabulary.** The shade is a terminal — a grid, tabs, a scrollback, a
 * working directory drawn at the end of the line. This is one line at reading
 * size with a list under it, because it is reached mid-sentence from another
 * application and read in the second before somebody goes back to what they
 * were doing. One tab, no grid, no history of the day: a panel that kept one
 * would be a terminal that opens over other people's windows.
 */

/**
 * How many rows are offered at once, whichever list is under the line.
 *
 * A shortlist rather than a page of results. The panel is as tall as what it
 * is showing, so a generous list is a surface that covers the work it was
 * called over — and what is not in the first few rows is found by typing one
 * more word, which is faster than reading forty.
 */
export const OFFERED = 8;

export function AskLine({
  project,
  opened,
  onField,
  onQuery,
  onTalking,
  onChooseProject,
}: {
  /**
   * The project a line typed here is about, by path and by name.
   *
   * Less than the board under it holds, and deliberately: what this file does
   * with a project is run its console and name it on a control. What the
   * project *declares* is the board's business.
   */
  readonly project: RecentProject;
  /** Bumped each time the key opened the panel: the line starts again. */
  readonly opened: number;
  /**
   * The field, handed to the panel as it is made.
   *
   * The board under this takes the keyboard into a column and has to be able to
   * give it back, and what it needs is *that* element rather than a description
   * of where to find one. The element travels rather than the ref, because a
   * ref passed down is a ref written through from two files.
   */
  readonly onField: (element: HTMLTextAreaElement | null) => void;
  /** What is in the line, for the board under it. */
  readonly onQuery: (query: string) => void;
  /**
   * Whether a conversation is standing in this surface: something was asked of
   * an agent and has not been ended.
   *
   * Said upwards because it decides what the whole panel is. A conversation and
   * the project's own panels on one surface is two subjects at once, and the
   * one somebody is in the middle of is the conversation.
   */
  readonly onTalking: (talking: boolean) => void;
  readonly onChooseProject: () => void;
}) {
  const {
    tabs,
    active,
    running,
    completions,
    open: openTab,
    close: closeTab,
    setDraft,
    submit,
    rerun,
    recall,
  } = useConsole(project.path, (address) => {
    // An address on its own line is a place to go, and where it goes is a
    // window rather than this surface: the panel has nowhere to draw a record,
    // and the application already knows which window owns one. So the panel
    // follows the address exactly as another application would and puts itself
    // away.
    void followLink(address).finally(() => void dismissPanel());
  });

  // A console with no tab has nowhere to type. The panel is built at launch and
  // kept for the life of the process, so this runs once rather than on each
  // opening — and in an effect rather than during the render, because asking
  // for a tab is a dispatch and a dispatch while rendering is a render with a
  // side effect in it.
  useEffect(() => {
    if (tabs.length === 0) openTab();
  }, [openTab, tabs.length]);

  /**
   * Which of the offered tokens the arrows are on, and which opening that
   * position belongs to.
   *
   * Compared during the render that brings a new opening rather than reset
   * afterwards in an effect, as `console-input.tsx` does: a cursor left on the
   * fourth row of a list that has started again points at something nobody is
   * being offered.
   */
  const field = useRef<HTMLTextAreaElement>(null);
  const [picked, setPicked] = useState({ opened, at: 0 });
  if (picked.opened !== opened) setPicked({ opened, at: 0 });
  const setAt = (at: number) => setPicked({ opened, at });

  /**
   * The agent this line is talking to until somebody says otherwise.
   *
   * Typing a name on every line is typing the same word all evening, and the
   * whole of what this panel is for is the sentence somebody came to say. So a
   * name on its own fixes who is being addressed: what follows goes to them,
   * the conversation with them is what stands under the line, and Escape lets
   * go of them before it lets go of anything else.
   *
   * It is not the console's and is not written down anywhere: who you are
   * talking to is true of this surface while it is up, and a project opened in
   * the window is not a project somebody is mid-sentence with.
   */
  const [pinned, setPinned] = useState<{ project: string; name: string } | null>(null);
  // Let go of them when the panel is pointed at another project, during the
  // render that points it: an agent is addressed inside a project, and a name
  // carried across would send the next line to whoever answered to it there.
  if (pinned !== null && pinned.project !== project.path) setPinned(null);
  const talkingTo = pinned?.name ?? null;

  const value = active?.draft ?? "";
  /**
   * Whether this line is addressed to somebody rather than asked of the
   * project.
   *
   * The `@` at the front and nothing else. It is a prefix rather than a
   * reading of the whole line, deliberately: everything a person types is a
   * question about their project until they say otherwise, and *looks like a
   * command* is the one guess that cannot be taken back.
   */
  const mention = value.trimStart().startsWith("@");
  /**
   * The name being typed after the `@`, or nothing where the line is not
   * addressing anybody.
   *
   * The first word and no more, which is not a second reading of what the line
   * *means*: that is `intent` below, and it answers `none` for `@name` with
   * nothing asked of it yet — which is exactly the moment somebody wants to see
   * who they are about to talk to.
   */
  const typing = mention ? (value.trimStart().slice(1).split(/\s/u)[0] ?? "") : null;
  const addressed = typing ?? talkingTo;
  /**
   * Whether the line is a name and nothing else, which is how somebody says
   * who they are talking to.
   *
   * `@test` asks nothing of anybody — the console says so in as many words —
   * and that is exactly the moment to read it as *this is who I mean*.
   */
  const naming = mention && typing !== null && typing !== "" && value.trim() === `@${typing}`;
  // Only for an addressed line. What a question could become is the answer
  // itself, and the answer is on the board under this — a list of tokens over
  // it would be two things to read before the first row.
  const shown = mention ? completions.slice(0, OFFERED) : [];
  const cursor = Math.min(picked.at, Math.max(0, shown.length - 1));
  const tail = mention ? ghost(shown[cursor]?.token ?? null, value) : null;

  /**
   * What is shown of this tab: the conversation with whoever is being
   * addressed, or the whole of it when nobody is.
   *
   * Typing a name is asking *what have I said to them*, and a stream holding
   * four agents' answers in the order they arrived is the wrong answer to it.
   * Which blocks belong to a name is the console's own reading of the line that
   * made each one, rather than a second one written here.
   */
  const blocks = active?.blocks ?? [];
  const spoken =
    addressed === null || addressed === ""
      ? blocks
      : blocks.filter((block) => {
          const said = intent(block.typed);
          return said.kind === "address" && said.name === addressed;
        });
  /**
   * What has already been said to whoever is being addressed, read from the
   * session itself rather than from what this panel happens to have seen.
   *
   * A name is enough: somebody who said who they are talking to is asking to
   * be shown that conversation, and this surface is opened fresh many times
   * against work that has been going for hours.
   */
  const conversation = useConversation(
    project.path,
    addressed,
    `${blocks.length}:${running.length}`,
  );
  const said = conversation?.entries.length ?? 0;
  /**
   * Whether something has been asked of an agent and is still standing: a
   * block that came back, a conversation behind a name, or work that has not
   * finished.
   *
   * All three, because they are the same state at three moments — a line taken
   * has nothing to show yet, and a conversation that has been read still holds
   * what was said until somebody ends it.
   */
  const talking = said > 0 || spoken.length > 0 || running.length > 0;
  useEffect(() => {
    onTalking(talking);
  }, [onTalking, talking]);

  // What is in the line, said upwards as it changes.
  //
  // In an effect rather than from the field's own handler, because the draft is
  // the console's and moves without anybody typing: taking a completion
  // rewrites it, and submitting an addressed line empties it. Reporting only
  // the keystrokes would leave the board standing on an answer to a question
  // that is no longer in the line.
  useEffect(() => {
    // A line with somebody on the other end of it is a message, not a
    // question: searching the project for what is being said to an agent would
    // answer a question nobody asked, under a conversation they are in.
    onQuery(talkingTo === null ? value : "");
  }, [onQuery, talkingTo, value]);

  // Opening the panel again takes the caret and selects what was left in the
  // line, so the next thing typed replaces it. Not cleared: a line somebody
  // abandoned is occasionally the line they came back to finish, and selecting
  // it costs them one keystroke either way.
  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, [opened]);

  // The field grows with what is in it, up to the ceiling the stylesheet sets.
  // In a layout effect because the panel measures itself immediately after:
  // a height applied a frame later is a frame of the window at the wrong size.
  useLayoutEffect(() => {
    const element = field.current;
    if (element === null) return;
    element.style.height = "auto";
    element.style.height = `${element.scrollHeight}px`;
  }, [value]);

  /**
   * Take the line: fix who is being addressed, or say the thing.
   *
   * A line already carrying a name goes as it was written — somebody who typed
   * one means that one, pinned or not. Everything else is addressed to whoever
   * the panel is talking to, by composing the line the console would have been
   * handed: there is one reading of a line in this application, and this gives
   * it the line it reads rather than a second way in.
   */
  const send = () => {
    if (naming && typing !== null) {
      setPinned({ project: project.path, name: typing });
      setDraft("");
      return;
    }
    if (talkingTo !== null && !mention) {
      const said = value.trim();
      if (said === "") return;
      rerun(`@${talkingTo} ${said}`);
      return;
    }
    submit();
  };

  const take = () => {
    const chosen = shown[cursor];
    if (chosen === undefined) return;
    // The whole token replaces the word being typed, which is what the ghost
    // at the end of the line was promising.
    const word = value.split(/(\s+)/u);
    const last = word.at(-1) ?? "";
    setDraft(value.slice(0, value.length - last.length) + chosen.token + " ");
    setAt(0);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Escape undoes one step, which while a conversation is standing is the
    // conversation. Stopped here rather than left to the panel around this: the
    // panel's answer to Escape is to put the whole surface away, and that would
    // undo two.
    //
    // The tab is closed rather than emptied, and the effect above opens a fresh
    // one. Work still running is not stopped by it — ending a piece of work is
    // what the line's own verb is for — so a conversation ended while an agent
    // is still going leaves the shelf standing, which is the truth.
    if (event.code === "Escape" && (talkingTo !== null || talking)) {
      event.preventDefault();
      event.stopPropagation();
      // Letting go of the agent first: the conversation is still worth reading
      // after somebody has stopped talking to them, and ending it in the same
      // keystroke would throw away the thing they were looking at.
      if (talkingTo !== null) setPinned(null);
      else if (active !== null) closeTab(active.id);
      return;
    }

    if (event.code === "Tab" && mention && shown.length > 0) {
      event.preventDefault();
      take();
      return;
    }

    // A line going to somebody is taken here whether the name is in it or
    // standing in front of it. The list and its keys below belong to a name
    // being typed, and nothing else in this file does.
    if (talkingTo !== null && !mention && event.code === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
      return;
    }

    // Everything below belongs to an addressed line. The rest of the keys are
    // the board's, and the way to leave them to it is to not act on them: it
    // listens on the window and stands down on anything already answered here.
    if (!mention) return;

    if (event.code === "ArrowDown") {
      event.preventDefault();
      // Past the end of the list the arrows walk the history of what has been
      // addressed. The list is short and the history is deep, so leaving the
      // list is the ordinary way out of it.
      if (shown.length === 0) recall("newer");
      else setAt(cursor + 1 >= shown.length ? 0 : cursor + 1);
      return;
    }

    if (event.code === "ArrowUp") {
      event.preventDefault();
      if (shown.length === 0) recall("older");
      else setAt(cursor - 1 < 0 ? Math.max(0, shown.length - 1) : cursor - 1);
      return;
    }

    if (event.code === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
      setAt(0);
    }
    // Escape and `⌘P` belong to the panel around this and are not stopped here.
  };

  return (
    <>
      <div className="flex shrink-0 items-center gap-3 px-5 py-4">
        {/* The project, as the one thing a line typed here is about. A control
            rather than a label: it is also how the list of projects is reached
            without the keyboard, which is the whole of what a person can do in
            this surface besides type. */}
        <button
          type="button"
          onClick={onChooseProject}
          title="Choose a project (⌘P)"
          className="shrink-0 rounded-md bg-raised px-2 py-1 text-sm text-fg-secondary hover:bg-hover"
        >
          {project.name}
        </button>

        {/* Who the line is talking to, where somebody has said. Beside the
            project rather than inside the field: it is not text that can be
            edited away, and what it says is true of every line typed after it
            until it is let go of. */}
        {talkingTo === null ? null : (
          <button
            type="button"
            onClick={() => setPinned(null)}
            title={`Stop addressing ${talkingTo} (esc)`}
            // The tone of the chip beside it, and the face is what tells them
            // apart: a name is typed, and the project is read. Colour is kept
            // for status and for destruction in this window, and who somebody
            // is talking to is neither.
            className="shrink-0 rounded-md bg-raised px-2 py-1 font-mono text-sm text-fg-secondary hover:bg-hover"
          >
            @{talkingTo}
          </button>
        )}

        <div className="relative min-w-0 flex-1">
          <textarea
            ref={(element) => {
              field.current = element;
              onField(element);
            }}
            value={value}
            onChange={(event) => {
              setDraft(event.target.value);
              setAt(0);
            }}
            onKeyDown={onKeyDown}
            rows={1}
            aria-label="Search this project, or address an agent"
            placeholder={
              talkingTo === null
                ? "Search this project, or @ an agent"
                : `Say something to ${talkingTo}`
            }
            // The ceiling is six lines. Past that the panel is a document being
            // written in, and this is not where that happens.
            // `block`, which is what puts the text where it looks aligned. A
            // textarea is inline by default, so the box around it is a line box
            // — taller than the field by the descender it reserves — and
            // centring the row centred that box instead: the words sat two
            // points above the middle of the chip beside them.
            className="block max-h-32 w-full resize-none bg-transparent text-lg leading-snug text-fg outline-none placeholder:text-fg-tertiary"
            spellCheck={false}
            autoComplete="off"
          />
          {/* What taking the offer would add, in the faintest tone, drawn under
              a transparent field — the technique and its known weakness are
              `console-input.tsx`'s, and the geometry is named once here for the
              same reason: the layer and the field must agree exactly. */}
          {tail !== null && (
            <div
              aria-hidden
              className="pointer-events-none absolute inset-0 text-lg leading-snug whitespace-pre-wrap text-transparent"
            >
              {value}
              <span className="text-fg-tertiary">{tail}</span>
            </div>
          )}
        </div>
      </div>

      {mention && shown.length > 0 && (
        <ul
          className="border-t border-separator px-3 py-2"
          role="listbox"
          aria-label="Who this line could be addressed to"
        >
          {shown.map((one, index) => (
            <li key={one.token}>
              <button
                type="button"
                role="option"
                aria-selected={index === cursor}
                onMouseMove={() => setAt(index)}
                onClick={() => {
                  setAt(index);
                  take();
                  field.current?.focus();
                }}
                className={cn(
                  "flex w-full items-baseline gap-4 rounded-md px-3 py-2 text-left",
                  index === cursor && "bg-selected",
                )}
              >
                {/* The token in the monospace face and its one phrase beside it,
                    both at the interface size. A list read at a glance wants one
                    size and one rhythm: the face is what separates what is typed
                    from what is said about it. */}
                <span className="w-40 shrink-0 truncate font-mono text-sm text-fg">
                  {one.token}
                </span>
                <span className="min-w-0 flex-1 truncate text-sm text-fg-tertiary">
                  {one.hint}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {running.length > 0 && (
        <div className="border-t border-separator py-2">
          <ConsoleShelf running={running} />
        </div>
      )}

      {said > 0 && conversation !== null ? (
        // The session's own, which holds what was said in this panel as well:
        // a line addressed here is a turn of the same conversation, so drawing
        // both would be the same words twice under one name.
        <AskConversation transcript={conversation} />
      ) : spoken.length > 0 ? (
        // A ceiling rather than the whole answer. What an agent says at length
        // belongs in the window where a conversation is read; what is worth
        // having here is the first of it, in the second before somebody goes
        // back to the application they called this from.
        //
        // The box around it carries four points of its own, because the canvas
        // is written for the shade and brings the shade's margin with it: four
        // there and four here is the twenty this surface indents everything
        // else by, so what an agent said stands on the same left edge as the
        // line that asked it. Vertical room is this box's alone — the stream
        // inside ends flush against its own foot, which under a hairline reads
        // as text about to be cut off.
        <div className="flex max-h-72 flex-col border-t border-separator px-1 py-2">
          <ConsoleCanvas blocks={spoken} onRerun={rerun} />
        </div>
      ) : null}

      {/* What the keys do, said once and quietly.
          Not decoration: `⌘P` is the only thing in this surface that cannot be
          found by typing, and a shortcut nobody can discover is a shortcut
          nobody has. The row is the height of its text and sits under
          everything, where it is read on the first few openings and ignored
          afterwards. */}
      <div className="flex shrink-0 items-center gap-4 border-t border-separator px-5 py-2.5 text-xs text-fg-tertiary">
        <span>
          <kbd className="font-mono">⏎</kbd> open
        </span>
        <span>
          <kbd className="font-mono">←→</kbd> panels
        </span>
        <span>
          <kbd className="font-mono">@</kbd> ask an agent
        </span>
        <span>
          <kbd className="font-mono">⌘P</kbd> project
        </span>
        <span>
          <kbd className="font-mono">esc</kbd>{" "}
          {talkingTo !== null ? "leave" : talking ? "end" : "close"}
        </span>
      </div>

    </>
  );
}
