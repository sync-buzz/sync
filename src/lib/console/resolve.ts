import type { SyntaxNode } from "@lezer/common";

import type { Presentation } from "@/lib/console/types";
import {
  argsOf,
  insideShell,
  leavesOf,
  stepsOf,
  textOf,
  wordAt,
} from "@/lib/console/tree";

/**
 * What a typed line is, and there are exactly three answers.
 *
 * This is the one place a line is read, and it is deliberately the only one: a
 * second reading would be a second answer to *who owns this word*, and the two
 * would part company on the first line somebody typed in anger.
 *
 * **Nothing is guessed.** A process is raised only for a line that says so with
 * `!`, a verb runs only when it was declared, work is addressed only by name.
 * Reading *whatever looks like a command* out of ordinary writing is the one
 * mistake that cannot be taken back: what is handed to a shell has run by the
 * time anybody sees it was not meant as a command.
 */

/** A verb the console would run, as the empty tab lists it. */
export interface Verb {
  readonly name: string;
  /** One line, in the imperative: what running it does. */
  readonly summary: string;
}

/**
 * The verbs the window itself has.
 *
 * Two families, and both are the shell's for one reason: what they act on lives
 * in the core rather than in a package. Sessions are in
 * `src-tauri/src/sessions.rs`, so an agent is a mechanism of the shell; memory
 * is reached through `sync-memory`, which is the only door to the engine.
 *
 * **Not one of them names a kind, a language or a section.** `find` searches
 * whatever the project holds and `types` reads what that is — a verb that knew
 * one of them by name would be the shell learning a subject.
 *
 * Written once and read twice: this list is what an empty tab shows and what a
 * line is resolved against, so a verb that works and is not listed, or listed
 * and does not work, cannot happen.
 */
const OWN: readonly Verb[] = [
  { name: "cd", summary: "work somewhere else" },
  { name: "find", summary: "search the project's memory" },
  { name: "types", summary: "list the kinds this project holds" },
  { name: "folders", summary: "list the folders records are filed in" },
  { name: "works", summary: "list what is running" },
  { name: "stop", summary: "interrupt a turn, leaving the agent up" },
  { name: "close", summary: "end a piece of work and its agent" },
  { name: "allow", summary: "answer a question with yes" },
  { name: "deny", summary: "answer a question with no" },
];

export function verbs(): readonly Verb[] {
  // A package's verbs join these when there is a way for a package to declare
  // one. Until then the list is the window's own and says so by being short.
  return OWN;
}

/** Where a line goes. */
export type Intent =
  /** To an agent, under the name it is addressed by and answers to. */
  | {
      readonly kind: "address";
      readonly name: string;
      readonly text: string;
    }
  /** To a process, because the line asked for one by name. */
  | { readonly kind: "shell"; readonly line: string }
  /** To a verb, by the name it was declared under and what followed it. */
  | {
      readonly kind: "verb";
      readonly name: string;
      readonly args: readonly string[];
    }
  /** To the window: a document, which is shown rather than said to anybody. */
  | { readonly kind: "open"; readonly address: string }
  /** Nowhere, and the sentence a person is owed for it. */
  | { readonly kind: "none"; readonly why: string };

/**
 * What separates one step of a line from the next.
 *
 * **Not `|` and not `>`.** Inside `!` the line belongs to a shell, and the
 * shell's own marks have to keep working: `!git log | head` is one command with
 * a pipe in it, and `!cargo test > out.txt` writes a file. Reading either of
 * them out of the line would mean this window parsing shell — quoting,
 * `2>&1`, `>>` — and a second reading of one string parts company with the
 * first quietly.
 *
 * Two characters rather than one for the same reason, and `->` rather than
 * `=>` because it is the easier of the two to type.
 */
export const THEN = "->";

/**
 * A line, cut into the steps it is made of.
 *
 * One step is the ordinary case and comes back as a list of one, so a caller
 * has one shape to read rather than two.
 */
export function steps(line: string): readonly string[] {
  return stepsOf(line)
    .map((step) => textOf(line, step).trim())
    .filter((step) => step.length > 0);
}

/** The prefix that addresses a piece of work, as the chat addresses one. */
const AGENT_PREFIX = "@";

/** The mark that opens an address into the project's memory. */
const MEMORY_PREFIX = "sync://";

/** The mark that opens a path into the file system. */
const PATH_PREFIX = "/";

/** How many addresses one list offers before it stops being a list. */
const OFFERED = 50;

/**
 * Read a line.
 *
 * Takes what was typed and answers what it is. It starts nothing: a caller
 * decides what to do with each answer, and what cannot be carried out is
 * refused where it would have been carried out.
 */
export function intent(line: string): Intent {
  const step = stepsOf(line)[0];
  if (step === undefined) {
    return { kind: "none", why: "console: there is nothing on this line" };
  }

  const shell = step.getChild("Shell");
  if (shell !== null) {
    const said = shell.getChild("ShellText");
    return {
      kind: "shell",
      line: said === null ? "" : textOf(line, said).trim(),
    };
  }

  const addressed = step.getChild("Address");
  if (addressed !== null) {
    const named = addressed.getChild("Name");
    // Both halves are refused for one reason: a name is an address and the rest
    // is what is being said to it, so neither is optional and neither can be
    // guessed. An agent raised for `@` alone would be an agent raised by a
    // typing accident.
    if (named === null) {
      return { kind: "none", why: "console: nobody is addressed by @ alone" };
    }
    const name = textOf(line, named);
    const text = line.slice(named.to, addressed.to).trim();
    if (text.length === 0) {
      return {
        kind: "none",
        why: `console: nothing is being asked of ${AGENT_PREFIX}${name}`,
      };
    }
    return { kind: "address", name, text };
  }

  const words = step.getChild("Words");
  // Words rather than parts: the parser divides `plan-` into a word and a
  // dash, because `->` may never be half of a word, and an argument somebody
  // typed as one thing has to come back as one thing. `cd my-folder-` went to
  // `my-folder` while these were counted apart.
  const args = words === null ? [] : argsOf(line, words);
  const first = args[0];
  if (first === undefined) {
    return { kind: "none", why: "console: there is nothing on this line" };
  }

  // An address on its own is a place to go rather than something to say. The
  // same address inside a sentence is an argument and travels with it — what
  // separates them is whether anything else is on the line.
  if (
    args.length === 1 &&
    first.parts.length === 1 &&
    first.parts[0]?.firstChild?.name === "SyncAddress"
  ) {
    const address = first.text;
    return address.endsWith("/")
      ? {
          kind: "none",
          // A kind or a folder is a place in the tree with nothing at the end
          // of it: there is no document to show, and saying so is better than
          // opening whatever happened to be first inside it.
          why: "console: that is a folder — name a document in it",
        }
      : { kind: "open", address };
  }

  const name = first.text;
  if (verbs().some((verb) => verb.name === name)) {
    return { kind: "verb", name, args: args.slice(1).map((arg) => arg.text) };
  }

  // The console is not a chat, so ordinary writing is not an instruction to
  // anybody. Refused rather than sent, and the refusal teaches the one gesture
  // that would have worked — the same shape a shell's `command not found` has,
  // which is what a person already knows how to read.
  return {
    kind: "none",
    why: `console: no such command: ${name} — address an agent with @name`,
  };
}

/**
 * What the console says about a line it cannot carry out, in the form every
 * shell says it: what they typed, back at them, with nothing added.
 */
export function refusal(text: string): Presentation {
  return { view: "text", text };
}

/**
 * What one stretch of the line being typed is.
 *
 * A part of the line rather than a colour: what each of these is drawn in is
 * the canvas's business, and the palette it draws from is the sixteen the
 * window already declares for a terminal's output. One palette for what is
 * typed and what comes back — two would read as a fault the first time a
 * person saw the same word in two colours a line apart.
 */
export type Tone = "sigil" | "name" | "verb" | "shell" | "said";

/** A stretch of the line, and how it is set. */
export interface Painted {
  readonly text: string;
  readonly tone: Tone;
}

/**
 * Reads the line for colour.
 *
 * The same four forms {@link intent} reads, and deliberately the same order:
 * two readings of one line would drift, and the one that drifts is the one
 * nobody is testing — a line coloured as an address and run as a refusal is a
 * window lying about what Return will do.
 */
export function highlight(line: string): readonly Painted[] {
  if (line.length === 0) return [];

  const painted: Painted[] = [];
  let at = 0;
  const gap = (until: number) => {
    // Space between two parts, and whatever the parser could make nothing of.
    // Neither is drawn as a mistake: a line is refused when Return is pressed,
    // never while somebody is still typing it.
    if (until > at) painted.push({ text: line.slice(at, until), tone: "said" });
  };

  for (const leaf of leavesOf(line)) {
    gap(leaf.from);
    const text = textOf(line, leaf);
    if (leaf.name === "SyncAddress") {
      // The mark and what it points at are set apart, the same way the mark
      // before a name is: what the eye looks for is where the pointing starts.
      painted.push({ text: MEMORY_PREFIX, tone: "sigil" });
      painted.push({
        text: text.slice(MEMORY_PREFIX.length),
        tone: "name",
      });
    } else {
      painted.push({ text, tone: toneOf(line, leaf) });
    }
    at = leaf.to;
  }
  gap(line.length);

  // Run together what was set the same way. Two parts of one colour side by
  // side are one part that was cut for the parser's reasons rather than for a
  // reader's, and a canvas drawing each in its own element would break the
  // line where nothing is.
  return painted.reduce<Painted[]>((run, part) => {
    const before = run.at(-1);
    if (before !== undefined && before.tone === part.tone) {
      run[run.length - 1] = { text: before.text + part.text, tone: part.tone };
      return run;
    }
    run.push(part);
    return run;
  }, []);
}

/** How one part of the line is set. */
function toneOf(line: string, leaf: SyntaxNode): Tone {
  switch (leaf.name) {
    case "Bang":
    case "At":
    case "Then":
      return "sigil";
    case "ShellText":
      return "shell";
    case "Name":
      return "name";
    case "Word":
      return opensAStep(line, leaf.from) &&
        verbs().some((verb) => verb.name === textOf(line, leaf))
        ? "verb"
        : "said";
    default:
      return "said";
  }
}

/**
 * Whether this part of the line stands in the first word of its step.
 *
 * What a step opens with is what names it, and nothing else in it is a verb:
 * `find find` searches for the word, and the second one neither runs nor is
 * coloured as though it would. The same question answers what may be
 * completed, which is why it is asked once here rather than twice.
 */
function opensAStep(line: string, from: number): boolean {
  return stepsOf(line).some((step) => step.from === from);
}

/** A name this project answers to, and what it is doing. */
export interface Named {
  readonly name: string;
  /** One phrase: what it is, so a list of them can be read without asking. */
  readonly hint: string;
}

/** Something the line could become, spelled as it would appear in the line. */
export interface Suggestion {
  readonly token: string;
  readonly hint: string;
}

/**
 * What the line could become, best first.
 *
 * Offered from the moment the console opens rather than once somebody has
 * guessed the first letter: a console whose vocabulary appears only when you
 * already know it is a console that teaches nobody. An empty line offers
 * everything; a line being typed offers what still matches.
 *
 * Whole tokens — `@test` and not `test` — because two of the sources need
 * different marks in front of them, and a caller that had to remember which
 * would one day put `@` in front of a verb.
 *
 * Only the first word is completed. Past it the line is either an argument or
 * a sentence for an agent, and this window has no business finishing either.
 */
export function completions(
  line: string,
  names: readonly Named[],
  /** What memory answers to, for the word being typed after a slash. */
  paths: readonly Named[] = [],
  // What was used, newest first. Ordering by it rather than alphabetically is
  // the difference between a list somebody reads every time and one they stop
  // reading: what a person did last is what they are most likely to do next,
  // and it is the one ordering a window can know without being told.
  recent: readonly string[] = [],
): readonly Suggestion[] {
  const word = wordAt(line);

  // Inside `!` the line belongs to a process, and the words in it are the
  // machine's vocabulary rather than this window's. Asked of the first part,
  // because a word never straddles a step: what would divide one is a mark,
  // and every mark is a part of its own.
  const part = word?.parts[0] ?? null;
  if (part !== null && insideShell(part)) return [];

  const typed = word?.text ?? "";

  // An address is completed wherever it stands: what somebody points at is
  // usually in the middle of what they are saying to an agent, not at the
  // front of it. Already whole tokens, ordered by whoever read the tree —
  // folders before documents — and a second sort here would flatten that into
  // one alphabet.
  if (typed.startsWith(MEMORY_PREFIX) || typed.startsWith(PATH_PREFIX)) {
    const wanted = typed.toLowerCase();
    return paths
      .filter((path) => path.name.toLowerCase() !== wanted)
      .slice(0, OFFERED)
      .map((path) => ({ token: path.name, hint: path.hint }));
  }

  const addressed = names.map((named) => ({
    token: `${AGENT_PREFIX}${named.name}`,
    hint: named.hint,
  }));
  const own = verbs().map((verb) => ({
    token: verb.name,
    hint: verb.summary,
  }));

  const all = [...addressed, ...own];
  const rank = (token: string) => {
    const used = recent.indexOf(token);
    // Never used sorts after everything used, in the order the sources are
    // listed above: work before verbs, because work is what somebody is most
    // likely to be about to say something to.
    return used === -1 ? recent.length + all.length : used;
  };
  const ordered = [...all].sort((left, right) => {
    const between = rank(left.token) - rank(right.token);
    return between === 0 ? all.indexOf(left) - all.indexOf(right) : between;
  });

  // Nothing typed at all: the whole vocabulary, which is what an empty tab
  // shows. A console whose words appear only once you have guessed the first
  // letter is a console that teaches nobody.
  if (line.trim().length === 0) return ordered;

  // A line that ends in a space has ended a word, and the next one is not
  // being typed yet. Past the first word of a step there is nothing to offer
  // either: what follows a verb is an argument, and what follows a name is a
  // sentence for an agent.
  if (word === null || !opensAStep(line, word.from)) return [];

  const wanted = typed.toLowerCase();
  return ordered.filter((one) => {
    const lowered = one.token.toLowerCase();
    return lowered.startsWith(wanted) && lowered !== wanted;
  });
}

/**
 * What would be added if the first suggestion were taken, or nothing.
 *
 * Drawn at the end of the line in the faintest tone, which is how zsh and fish
 * say the same thing. It is the tail rather than the whole token, because the
 * part already typed is already on the screen — printing it twice is what makes
 * a ghost look like a rendering fault.
 */
export function ghost(chosen: string | null, line: string): string | null {
  if (chosen === null) return null;
  const word = wordAt(line)?.text ?? "";
  // A mark on its own is not a word yet. `@` matches every name and `/` every
  // record, so a ghost at that moment is whichever happened to sort first —
  // a whole path unrolling into the line before anybody has said anything
  // about which one they want.
  if (word.length < 2) return null;
  return chosen.startsWith(word) ? chosen.slice(word.length) : null;
}
