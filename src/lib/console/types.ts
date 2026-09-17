/**
 * What the console is made of: a block, what a block shows, and a tab.
 *
 * None of it names a verb, and that is the constraint the whole console is
 * built to keep. The shell holds no subject matter, so a type here that knew
 * about agents, or records, or a shell pipeline would be the first place the
 * emptiness leaked — and it would leak in the one file every other one imports.
 *
 * A result is **data plus the name of a view**, never a node. A node handed
 * over by whatever produced it is a producer that knows about this window: its
 * theme, its width, its grid, its accessibility rules. Keeping the view a name
 * leaves all of that here, which is what makes one result render the same in
 * both appearances and under increased contrast.
 */

/**
 * A block's result, in the shape the canvas can draw.
 *
 * One view for now, and the union is written as a union all the same: the
 * others — rows, a list, a tree, a diff, a record, a choice, a stream — arrive
 * beside it, and a member added to a union is a compile error at every place
 * that draws one. A single interface widened later is not.
 */
export type Presentation =
  | {
      readonly view: "text";
      /**
       * The text as it arrived, escape sequences included. Nothing here strips
       * or interprets them: colour is the canvas's to resolve against the
       * appearance, so a producer that wrote red gets this window's red rather
       * than its own.
       */
      readonly text: string;
    }
  | {
      /**
       * What a process printed, as it printed it.
       *
       * Its own view rather than `text` with a flag, for the reason the answer
       * has one: this is a stream that arrives in pieces and ends with a code,
       * and the block drawing it has to know that the thing it is drawing is
       * still going.
       */
      readonly view: "stream";
      readonly text: string;
      /** How it ended, or `null` while it has not. */
      readonly code: number | null;
    }
  | {
      /**
       * What an agent answered, which is markdown.
       *
       * Its own view rather than a flag on `text`, because the two are read by
       * opposite rules: this one takes `#` for a heading and `*` for a mark,
       * and the output of a process takes both for characters. One view doing
       * both would have to guess which it was holding, and the guess would be
       * wrong exactly where it costs most — a listing of filenames with
       * asterisks in them, quietly set in italics.
       */
      readonly view: "markdown";
      readonly text: string;
    };

/**
 * Where a block stands.
 *
 * `done` draws no glyph at all. Success is the ordinary outcome, and a tick on
 * every line that worked turns the canvas into a checklist of things nobody
 * needed to be told.
 */
export type BlockState = "waiting" | "running" | "done" | "failed";

/** What was typed, where it stands, and what came back. */
export interface Block {
  readonly id: string;
  /** The line as the person wrote it, unnormalised — it is theirs, not ours. */
  readonly typed: string;
  readonly state: BlockState;
  readonly result: Presentation | null;
  readonly startedAt: number;
  /** When it stopped, or `null` while it has not. */
  readonly endedAt: number | null;
}

/**
 * One tab: an independent history, a working directory of its own, and the
 * blocks that happened in it.
 *
 * Tabs are linear and never split. Dividing a tab into panes is what terminal
 * emulators fought for, and it settles nothing here: blocks are already a
 * vertical stream, so a split would put two streams side by side and halve the
 * width each one has to say anything in.
 */
export interface Tab {
  readonly id: string;
  /** The directory a command in this tab would run in. */
  readonly cwd: string;
  /**
   * What somebody called this tab, or nothing while it is called after what it
   * is doing.
   *
   * Two fields rather than one that starts out computed, because the
   * distinction is the behaviour: an unnamed tab keeps following the work, and
   * a named one has been taken out of that arrangement on purpose. Writing the
   * computed name into the field on the first command would quietly freeze
   * every tab at whatever was typed into it first.
   */
  readonly name: string | null;
  readonly blocks: readonly Block[];
  /** Typed but not submitted, kept per tab so switching away holds the line. */
  readonly draft: string;
  /** Submitted lines, newest last. */
  readonly history: readonly string[];
}

/**
 * What a tab is called: what somebody named it, or what it is busy with.
 *
 * A tab named for its number tells a person the one thing they can already see.
 * The verb last typed is what they were doing, and before anything has been
 * typed the directory is — a fresh tab is named after the folder it opens in.
 * A name given by hand outranks all of that and never changes under them.
 */
export function tabLabel(tab: Tab): string {
  if (tab.name !== null) return tab.name;

  const last = tab.blocks.at(-1);
  if (last !== undefined) {
    const verb = last.typed.trim().split(/\s+/u)[0];
    if (verb !== undefined && verb.length > 0) return verb;
  }

  const name = tab.cwd.split("/").filter(Boolean).at(-1);
  return name ?? "/";
}

/**
 * How long a block took, or nothing when there is nothing worth saying.
 *
 * A block refused before it ran took no time, and `0 ms` beside it is a
 * measurement of nothing dressed as a fact. Under a millisecond the honest
 * answer is silence.
 */
export function blockDuration(block: Block): string | null {
  if (block.endedAt === null) return null;
  const ms = block.endedAt - block.startedAt;
  if (ms < 1) return null;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}

/** Whether anything in this tab is still going, which the strip marks. */
export function tabIsBusy(tab: Tab): boolean {
  return tab.blocks.some(
    (block) => block.state === "running" || block.state === "waiting",
  );
}

/** What a verb did, in the shape a block is made from. */
export interface Outcome {
  readonly state: BlockState;
  readonly result: Presentation;
}
