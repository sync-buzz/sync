"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import {
  completions as candidates,
  intent,
  refusal,
  steps,
  THEN,
  type Suggestion,
} from "@/lib/console/resolve";
import type { Block, BlockState, Presentation, Tab } from "@/lib/console/types";
import { SYNC, corpusVerb, inside, useCorpus } from "@/lib/console/corpus";
import {
  closeTabWith,
  goTo,
  openTabWith,
  useFiles,
} from "@/lib/console/files";
import { collect, useShell } from "@/lib/console/shell";
import { type Finished, type Running, useWorks } from "@/lib/console/works";

/**
 * What the console is, minus everything that draws it.
 *
 * Tabs, the blocks in them, what is half-typed and where the history cursor
 * stands — all of it behind one hook, so that the shade, the strip, the canvas
 * and the input line each read the part they show and none of them owns it.
 * Four components sharing one fact is the arrangement where the fact belongs
 * to none of them.
 *
 * **It belongs to the window and goes with the project.** A tab is a working
 * directory and a history inside one project, and neither means anything in the
 * next one. Nothing here is written to disk: the height of the shade is the
 * hand's and survives a launch, what was typed into a project is the project's
 * and does not.
 *
 * The reducer is exported beside the hook because every transition worth
 * checking — closing the last tab, recalling past the end of the history,
 * submitting nothing — is a plain function of state and an action, and reaching
 * those through a rendered component would be testing React rather than this.
 */

export interface ConsoleState {
  readonly tabs: readonly Tab[];
  /** The tab on screen, or nothing when the console holds none. */
  readonly active: Tab | null;
  /**
   * The work still going, whichever tab it was asked for in.
   *
   * Of the project rather than of the tab, because that is what it is: closing
   * a tab does not stop an agent, and a shelf that emptied when somebody
   * switched tabs would say work had ended when it had not.
   */
  readonly running: readonly Running[];
  /** What the line in the tab on screen could become, best first. */
  readonly completions: readonly Suggestion[];
}

export interface ConsoleCommands {
  /** Make a tab and select it. The console with no tabs gets its first here. */
  readonly open: () => void;
  readonly select: (id: string) => void;
  /** Select by position, which is what `⌘1`…`⌘9` address. */
  readonly selectAt: (index: number) => void;
  /** Select the neighbour, wrapping at both ends. */
  readonly step: (by: 1 | -1) => void;
  readonly close: (id: string) => void;
  /** Name a tab by hand, or `null` to let it follow its work again. */
  readonly rename: (id: string, name: string | null) => void;
  readonly setDraft: (value: string) => void;
  /** Resolve what is typed and add the block it produced. */
  readonly submit: () => void;
  /**
   * Run a line that was already run, from the block that holds it.
   *
   * It produces a second block rather than replacing the first: what happened
   * happened, and a console that rewrote its own history would be a console
   * nobody could read back.
   */
  readonly rerun: (line: string) => void;
  /** Walk the history into the input line. */
  readonly recall: (direction: "older" | "newer") => void;
}

export type Console = ConsoleState & ConsoleCommands;

interface State {
  readonly tabs: readonly Tab[];
  readonly activeId: string | null;
  /**
   * How far back the history has been walked, counted from the newest line, or
   * nothing while the input line holds something typed rather than recalled.
   *
   * One cursor rather than one per tab, and it resets on every switch: walking
   * back through a history, leaving for another tab and coming back is a person
   * who has started again. What they *typed* is kept — that is the draft, and
   * it is per tab.
   */
  readonly recalled: number | null;
  /**
   * How many identifiers this console has handed out.
   *
   * In the state rather than in a variable beside it, and that is not tidiness:
   * a counter in the module is a side effect inside a reducer, and a reducer
   * with one is not the plain function everything here relies on it being. It
   * showed as a defect the moment the module was replaced under a live state —
   * the count started again and handed out identifiers the canvas was already
   * drawing.
   */
  readonly minted: number;
  /**
   * The tokens this console has used, newest first.
   *
   * A token rather than a whole line: what is offered again is `@review` or
   * `stop`, and a history of lines is already kept per tab for the arrows. It
   * is of the console rather than of a tab, because a name addressed in one
   * tab is the same piece of work in the next one.
   */
  readonly recent: readonly string[];
}

export type Action =
  | { readonly type: "open"; readonly cwd: string }
  | { readonly type: "select"; readonly id: string }
  | { readonly type: "selectAt"; readonly index: number }
  | { readonly type: "step"; readonly by: 1 | -1 }
  | { readonly type: "close"; readonly id: string }
  | {
      readonly type: "rename";
      readonly id: string;
      /** `null` gives the tab back to being named after what it is doing. */
      readonly name: string | null;
    }
  | { readonly type: "draft"; readonly value: string }
  /** Work somewhere else, which is the one step outside the project. */
  | { readonly type: "cwd"; readonly id: string; readonly path: string }
  /**
   * A line that was over before it began: what it resolved to is carried here
   * rather than worked out below, so that what a line *means* is decided in one
   * place and this one only files the outcome.
   */
  | {
      readonly type: "submit";
      readonly at: number;
      readonly line: string;
      readonly state: BlockState;
      readonly result: Presentation;
    }
  /**
   * A line that went to an agent. It leaves the input line and joins the
   * history, and no block is drawn: what came back is drawn when it comes back,
   * so the canvas reads in the order things finished rather than in the order
   * they were asked for.
   */
  | { readonly type: "accepted"; readonly line: string }
  /** What an agent came back with, filed in the tab it was asked for in. */
  | { readonly type: "finished"; readonly work: Finished }
  /**
   * A process that has started. Its block is drawn straight away and filled in
   * as it prints — the opposite of work, and for a plain reason: a process is
   * printing now, and a console that held its output back until it exited
   * would be a console that cannot be watched.
   */
  | {
      readonly type: "started";
      readonly id: string;
      readonly tabId: string;
      readonly line: string;
      readonly at: number;
    }
  /** More of what a process has printed, or the code it ended with. */
  | {
      readonly type: "updated";
      readonly id: string;
      readonly state: BlockState;
      readonly result: Presentation;
      readonly at: number;
    }
  | { readonly type: "recall"; readonly direction: "older" | "newer" };

const EMPTY: State = {
  tabs: [],
  activeId: null,
  recalled: null,
  minted: 0,
  recent: [],
};

/** How many tokens are remembered for ordering. Past this nobody is choosing. */
const REMEMBERED = 20;

/**
 * The first word of a line, which is the token that was used.
 *
 * `@review` and `stop` both come back whole; anything else — a line handed to
 * a process, a refusal — is not something to offer again and answers `null`.
 */
function used(line: string): string | null {
  const first = line.trim().split(/\s+/u)[0] ?? "";
  if (first.length === 0 || first.startsWith("!")) return null;
  return first;
}

/** Puts a token at the front of what was used, without repeating it. */
function remember(recent: readonly string[], line: string): readonly string[] {
  const token = used(line);
  if (token === null) return recent;
  return [token, ...recent.filter((one) => one !== token)].slice(0, REMEMBERED);
}

/**
 * Identity for tabs and blocks, counted rather than random.
 *
 * These are only ever compared with each other inside one window:
 * `crypto.randomUUID` would buy uniqueness across machines that nothing here
 * asks for, and it is not available while a page is being prerendered.
 */
function mint(minted: number, prefix: string): string {
  return `${prefix}${minted + 1}`;
}

function freshTab(id: string, cwd: string): Tab {
  return {
    id,
    cwd,
    name: null,
    blocks: [],
    draft: "",
    history: [],
  };
}

function replace(tabs: readonly Tab[], next: Tab): readonly Tab[] {
  return tabs.map((tab) => (tab.id === next.id ? next : tab));
}

/**
 * A line joins the history unless it is the one already at the end of it.
 *
 * A line repeated is not two entries. Shells differ on this and the useful
 * reading wins: a history is the distinct things somebody asked for, not a
 * transcript of their keystrokes.
 */
function remembered(
  history: readonly string[],
  line: string,
): readonly string[] {
  return history.at(-1) === line ? history : [...history, line];
}

export function reduce(state: State, action: Action): State {
  const active = state.tabs.find((tab) => tab.id === state.activeId) ?? null;

  switch (action.type) {
    case "open": {
      const tab = freshTab(mint(state.minted, "tab"), action.cwd);
      return {
        ...state,
        tabs: [...state.tabs, tab],
        activeId: tab.id,
        recalled: null,
        minted: state.minted + 1,
      };
    }

    case "select":
      if (action.id === state.activeId) return state;
      return { ...state, activeId: action.id, recalled: null };

    case "selectAt": {
      const tab = state.tabs[action.index];
      // A position nothing occupies is not an error to report. `⌘4` with three
      // tabs open is a finger that missed, and the answer to a finger that
      // missed is for nothing to happen.
      if (tab === undefined || tab.id === state.activeId) return state;
      return { ...state, activeId: tab.id, recalled: null };
    }

    case "step": {
      if (state.tabs.length < 2 || active === null) return state;
      const from = state.tabs.indexOf(active);
      const to = (from + action.by + state.tabs.length) % state.tabs.length;
      return { ...state, activeId: state.tabs[to]!.id, recalled: null };
    }

    case "close": {
      const tabs = state.tabs.filter((tab) => tab.id !== action.id);
      if (tabs.length === state.tabs.length) return state;
      // Closing the one that was selected selects the one that took its place,
      // and the last one when it was last — the same rule every tab strip has,
      // and the one that keeps the eye where the hand left it.
      if (action.id !== state.activeId) return { ...state, tabs };
      const was = state.tabs.findIndex((tab) => tab.id === action.id);
      const next = tabs[Math.min(was, tabs.length - 1)] ?? null;
      return { ...state, tabs, activeId: next?.id ?? null, recalled: null };
    }

    case "rename": {
      const tab = state.tabs.find((one) => one.id === action.id);
      if (tab === undefined) return state;
      // A name of nothing but spaces is somebody clearing the field, which is
      // how a tab is handed back to being named after its work. Storing it
      // would leave a tab with a blank label and no way to tell why.
      const name = action.name?.trim();
      return {
        ...state,
        tabs: replace(state.tabs, {
          ...tab,
          name: name === undefined || name.length === 0 ? null : name,
        }),
      };
    }

    case "draft": {
      if (active === null) return state;
      return {
        ...state,
        tabs: replace(state.tabs, { ...active, draft: action.value }),
        // Typing leaves the history: what is in the line is now theirs, and
        // the next press of Up starts again from the newest line rather than
        // from wherever the last walk stopped.
        recalled: null,
      };
    }

    case "submit": {
      if (active === null) return state;
      const block: Block = {
        id: mint(state.minted, "block"),
        typed: action.line,
        state: action.state,
        result: action.result,
        startedAt: action.at,
        // Resolved before anything ran, so it is over at the moment it began.
        // A duration of zero is the truth here rather than a missing value.
        endedAt: action.at,
      };

      return {
        ...state,
        tabs: replace(state.tabs, {
          ...active,
          blocks: [...active.blocks, block],
          draft: "",
          history: remembered(active.history, action.line),
        }),
        recalled: null,
        minted: state.minted + 1,
        recent: remember(state.recent, action.line),
      };
    }

    case "cwd": {
      const tab = state.tabs.find((one) => one.id === action.id);
      if (tab === undefined) return state;
      return {
        ...state,
        tabs: replace(state.tabs, { ...tab, cwd: action.path }),
      };
    }

    case "accepted": {
      if (active === null) return state;
      return {
        ...state,
        tabs: replace(state.tabs, {
          ...active,
          draft: "",
          history: remembered(active.history, action.line),
        }),
        recalled: null,
        recent: remember(state.recent, action.line),
      };
    }

    case "finished": {
      const { work } = action;
      // Filed where it was asked for. A tab that has been closed since takes
      // its work's answer with it: the conversation is still there to open,
      // and printing into a tab somebody did not ask in would put an answer
      // under a line that is not above it.
      const tab = state.tabs.find((one) => one.id === work.tabId);
      if (tab === undefined) return state;

      const block: Block = {
        id: mint(state.minted, "block"),
        typed: work.typed,
        state: work.state,
        result: work.result,
        startedAt: work.startedAt,
        endedAt: work.endedAt,
      };
      return {
        ...state,
        tabs: replace(state.tabs, { ...tab, blocks: [...tab.blocks, block] }),
        minted: state.minted + 1,
      };
    }

    case "started": {
      const tab = state.tabs.find((one) => one.id === action.tabId);
      if (tab === undefined) return state;
      const block: Block = {
        id: action.id,
        typed: action.line,
        state: "running",
        result: null,
        startedAt: action.at,
        endedAt: null,
      };
      return {
        ...state,
        tabs: replace(state.tabs, { ...tab, blocks: [...tab.blocks, block] }),
      };
    }

    case "updated": {
      const tab = state.tabs.find((one) =>
        one.blocks.some((block) => block.id === action.id),
      );
      if (tab === undefined) return state;
      return {
        ...state,
        tabs: replace(state.tabs, {
          ...tab,
          blocks: tab.blocks.map((block) =>
            block.id === action.id
              ? {
                  ...block,
                  state: action.state,
                  result: action.result,
                  endedAt:
                    action.state === "running" ? block.endedAt : action.at,
                }
              : block,
          ),
        }),
      };
    }

    case "recall": {
      if (active === null || active.history.length === 0) return state;

      const back = state.recalled;
      const depth =
        action.direction === "older"
          ? Math.min((back ?? -1) + 1, active.history.length - 1)
          : (back ?? 0) - 1;

      // Walked forward past the newest line: back to what they were typing.
      // The draft is empty rather than restored, because submitting is what
      // cleared it and a walk should end where the walk started.
      if (depth < 0) {
        return {
          ...state,
          tabs: replace(state.tabs, { ...active, draft: "" }),
          recalled: null,
        };
      }

      const line = active.history[active.history.length - 1 - depth]!;
      return {
        ...state,
        tabs: replace(state.tabs, { ...active, draft: line }),
        recalled: depth,
      };
    }
  }
}

/**
 * The console of one project.
 *
 * `root` is where a fresh tab opens. It is passed in rather than read here, so
 * that this module holds no opinion about what a project is — it is given a
 * directory and knows nothing else about the window it serves.
 */
export function useConsole(
  root: string,
  /**
   * Show a document, and put the console away.
   *
   * Passed in rather than done here, because what *opening* a record means is
   * the area's answer and not this module's: the console holds no opinion about
   * which section draws which kind, exactly as it holds none about what a
   * project is.
   */
  onOpen: (address: string) => void,
): Console {
  const [state, dispatch] = useReducer(reduce, EMPTY);

  const active = useMemo(
    () => state.tabs.find((tab) => tab.id === state.activeId) ?? null,
    [state.activeId, state.tabs],
  );

  const open = useCallback(() => dispatch({ type: "open", cwd: root }), [root]);
  const select = useCallback((id: string) => dispatch({ type: "select", id }), []);
  const selectAt = useCallback(
    (index: number) => dispatch({ type: "selectAt", index }),
    [],
  );
  const step = useCallback((by: 1 | -1) => dispatch({ type: "step", by }), []);
  const close = useCallback((id: string) => dispatch({ type: "close", id }), []);
  const rename = useCallback(
    (id: string, name: string | null) => dispatch({ type: "rename", id, name }),
    [],
  );
  const setDraft = useCallback(
    (value: string) => dispatch({ type: "draft", value }),
    [],
  );
  const corpus = useCorpus(root);
  /**
   * What stands inside the path being typed.
   *
   * Worked out here rather than in the completion rule, because it is the one
   * thing in a line that depends on the project's memory rather than on the
   * grammar: what a slash offers is a step into a tree, and the tree is read
   * from the corpus.
   */
  /**
   * Tabs the host has been told the folder of.
   *
   * A tab opens in the project, and the host cannot know that until it is
   * told: what may be read is checked against what the host holds, so a tab it
   * has never heard of can read nothing. Told once per tab rather than on every
   * keystroke.
   */
  const placed = useRef(new Map<string, string>());
  const [held, setHeld] = useState<ReadonlyMap<string, string>>(new Map());
  useEffect(() => {
    for (const tab of state.tabs) {
      if (placed.current.has(tab.id)) continue;
      // Claimed before the answer comes back, so that a second render does not
      // open a second tab with the host for the same one.
      placed.current.set(tab.id, "");
      void openTabWith(tab.cwd)
        .then((name) => {
          placed.current.set(tab.id, name);
          setHeld(new Map(placed.current));
        })
        .catch(() => {
          // A project folder that cannot be opened is a window with bigger
          // problems than completion, and it says so everywhere else already.
          placed.current.delete(tab.id);
        });
    }
    for (const [id, name] of placed.current) {
      if (state.tabs.some((tab) => tab.id === id)) continue;
      placed.current.delete(id);
      if (name.length > 0) void closeTabWith(name);
    }
  }, [state.tabs]);

  /** What the host calls the tab on screen, or nothing until it has answered. */
  const named = active === null ? "" : (held.get(active.id) ?? "");

  const word = useMemo(
    () => (active?.draft ?? "").trimStart().split(/\s+/u).at(-1) ?? "",
    [active?.draft],
  );
  const files = useFiles(named, active?.cwd ?? root, word);
  const paths = useMemo(() => {
    if (word.startsWith(SYNC)) {
      return inside(word, corpus).map((one) => ({
        name: one.token,
        hint: one.hint,
      }));
    }
    return files.map((one) => ({ name: one.token, hint: one.hint }));
  }, [corpus, files, word]);

  const { address, running, names, perform } = useWorks(
    root,
    useCallback((work: Finished) => dispatch({ type: "finished", work }), []),
  );

  const { run: runShell } = useShell(
    root,
    useCallback(
      (id: string, state: BlockState, result: Presentation) =>
        dispatch({ type: "updated", id, state, result, at: Date.now() }),
      [],
    ),
  );

  /**
   * Identity for a block that is filled in as it runs.
   *
   * Its own counter because the block has to be named *before* the reducer sees
   * it — what comes back later is addressed by that name — and the reducer's
   * own counter is only reachable from inside it. A different prefix is what
   * keeps the two from ever meeting.
   */
  const started = useRef(0);

  // Where a line is read and where it is sent, which is one step rather than
  // two: what a line means decides which of them happens, and a reducer cannot
  // raise an agent.
  /**
   * How much of one step's output is carried into the next.
   *
   * From the end rather than the start: a build says what went wrong last, a
   * diff ends with what it could not do, and a log is read from the bottom. A
   * hundred thousand characters of it in a prompt is a bill somebody did not
   * agree to, so the cut is named in the line itself rather than made quietly.
   */
  const CARRIED = 8000;

  /**
   * A line of several steps, carried out in order.
   *
   * Only the last step may be an agent, and that is a limit rather than a
   * design: what the steps before it produce is text, which an agent takes as
   * material — and an agent in the middle would have to be waited for by
   * whatever came next, which is a conversation rather than a pipeline.
   */
  const chain = useCallback(
    async (tabId: string, tab: string, parts: readonly string[]) => {
      let carried = "";
      for (const [index, step] of parts.entries()) {
        const last = index === parts.length - 1;
        const what = intent(step);

        if (last && what.kind === "address") {
          const said =
            carried.length === 0
              ? what.text
              : `${what.text}\n\n${carried.length > CARRIED ? `… earlier output dropped\n${carried.slice(-CARRIED)}` : carried}`;
          address(tabId, what.name, said);
          return;
        }

        if (what.kind === "shell") {
          carried = await collect(root, tab, what.line);
          continue;
        }

        if (what.kind === "verb") {
          const outcome =
            (await corpusVerb(what.name, what.args, root, corpus)) ??
            (await perform(what.name, what.args));
          carried =
            outcome.result.view === "text" ? outcome.result.text : carried;
          continue;
        }

        throw new Error(
          `console: ${THEN} carries a process or a verb into an agent, and nothing else`,
        );
      }

      // Every step ran and none of them was an agent: what the last one printed
      // is the answer, and it is printed rather than carried anywhere.
      dispatch({
        type: "finished",
        work: {
          tabId,
          typed: parts.join(` ${THEN} `),
          state: "done",
          result: refusal(carried),
          startedAt: Date.now(),
          endedAt: Date.now(),
        },
      });
    },
    [address, corpus, perform, root],
  );

  const run = useCallback(
    (tabId: string, line: string) => {
      const typed = line.trim();
      // Return on an empty line is not a command that did nothing. It is the
      // gesture for *give me a fresh line*, and the canvas stays as it was.
      if (typed.length === 0) return;

      // A line of several steps is carried out as one thing, so nothing is
      // drawn until the last of them has something to say.
      const parts = steps(typed);
      if (parts.length > 1) {
        const startedAt = Date.now();
        void chain(tabId, named, parts).catch((error: unknown) => {
          dispatch({
            type: "finished",
            work: {
              tabId,
              typed,
              state: "failed",
              result: refusal(
                error instanceof Error
                  ? error.message
                  : "console: that did not run",
              ),
              startedAt,
              endedAt: Date.now(),
            },
          });
        });
        dispatch({ type: "accepted", line: typed });
        return;
      }

      const what = intent(typed);
      if (what.kind === "address") {
        // Nothing is translated on the way out: what was built by walking the
        // tree is already the address an agent's tools take.
        address(tabId, what.name, what.text);
        dispatch({ type: "accepted", line: typed });
        return;
      }

      if (what.kind === "verb" && what.name === "cd") {
        const asked = what.args[0] ?? "";
        const at2 = Date.now();
        if (asked.length === 0) {
          dispatch({
            type: "submit",
            at: at2,
            line: typed,
            state: "failed",
            result: refusal("console: name a folder to work in"),
          });
          return;
        }
        // The host answers where that turned out to be, because the host is
        // what holds it: a tab's folder decides what may be read, so the window
        // keeping its own copy would be a second answer to the question the
        // boundary is checked against.
        void goTo(named, asked)
          .then((at) => {
            dispatch({ type: "cwd", id: tabId, path: at });
            dispatch({
              type: "finished",
              work: {
                tabId,
                typed,
                state: "done",
                result: refusal(at),
                startedAt: at2,
                endedAt: Date.now(),
              },
            });
          })
          .catch((error: unknown) => {
            dispatch({
              type: "finished",
              work: {
                tabId,
                typed,
                state: "failed",
                result: refusal(
                  `console: ${typeof error === "string" ? error : "that folder could not be opened"}`,
                ),
                startedAt: at2,
                endedAt: Date.now(),
              },
            });
          });
        dispatch({ type: "accepted", line: typed });
        return;
      }

      if (what.kind === "verb") {
        const startedAt = Date.now();
        // A verb is a command of the host and answers in milliseconds, so its
        // block is filed when it answers rather than put on the shelf: the
        // shelf is for waiting somebody can feel.
        // Memory first, agents second, and a verb belongs to exactly one of
        // them: `corpusVerb` answers `null` for anything that is not its own,
        // which is how the two families are kept from having to know about
        // each other.
        void corpusVerb(what.name, what.args, root, corpus)
          .then((outcome) => outcome ?? perform(what.name, what.args))
          .then((outcome) =>
            dispatch({
              type: "finished",
              work: {
                tabId,
                typed,
                state: outcome.state,
                result: outcome.result,
                startedAt,
                endedAt: Date.now(),
              },
            }),
          )
          .catch(() => {
            dispatch({
              type: "finished",
              work: {
                tabId,
                typed,
                state: "failed",
                result: refusal(`console: ${what.name} could not be run`),
                startedAt,
                endedAt: Date.now(),
              },
            });
          });
        dispatch({ type: "accepted", line: typed });
        return;
      }

      if (what.kind === "open") {
        // Shown rather than said to anybody: the console hands the address to
        // whatever draws that kind and gets out of the way. Nothing is printed
        // — a block reporting that a document was opened would be a line about
        // something the person is now looking at.
        onOpen(what.address);
        dispatch({ type: "accepted", line: typed });
        return;
      }

      if (what.kind === "shell") {
        if (what.line.length === 0) {
          dispatch({
            type: "submit",
            at: Date.now(),
            line: typed,
            state: "failed",
            result: refusal("console: nothing to run"),
          });
          return;
        }
        started.current += 1;
        const id = `run${started.current}`;
        dispatch({ type: "started", id, tabId, line: typed, at: Date.now() });
        dispatch({ type: "accepted", line: typed });
        runShell(id, named, what.line);
        return;
      }

      dispatch({
        type: "submit",
        at: Date.now(),
        line: typed,
        state: "failed",
        result: refusal(what.why),
      });
    },
    [address, chain, corpus, named, onOpen, perform, root, runShell],
  );

  const submit = useCallback(() => {
    if (active === null) return;
    run(active.id, active.draft);
  }, [active, run]);

  const rerun = useCallback(
    (line: string) => {
      if (active === null) return;
      run(active.id, line);
    },
    [active, run],
  );
  const recall = useCallback(
    (direction: "older" | "newer") => dispatch({ type: "recall", direction }),
    [],
  );

  const completions = useMemo(
    () => candidates(active?.draft ?? "", names, paths, state.recent),
    [active?.draft, names, paths, state.recent],
  );

  return useMemo(
    () => ({
      tabs: state.tabs,
      active,
      running,
      completions,
      open,
      select,
      selectAt,
      step,
      close,
      rename,
      setDraft,
      submit,
      rerun,
      recall,
    }),
    [
      active,
      close,
      completions,
      running,
      open,
      recall,
      rename,
      rerun,
      select,
      selectAt,
      setDraft,
      state.tabs,
      step,
      submit,
    ],
  );
}
