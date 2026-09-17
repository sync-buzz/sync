/**
 * What a process printed, in the pieces it printed it in.
 *
 * A process talks to a terminal, and the pty this console runs one in says so:
 * `TERM=xterm-256color` and `COLORTERM=truecolor`, set in
 * `src-tauri/crates/sync-terminal/src/session.rs`. So `git status`, `ls` and a
 * test runner all answer in colour, and printing what they said without
 * reading it puts escape sequences on the screen instead of the words.
 *
 * **Colour comes from the window's own sixteen**, `--ansi-0` to `--ansi-15`,
 * declared in both themes and again at raised contrast. A palette carried in
 * here would be a second answer to what red is, and the one that is wrong is
 * whichever theme somebody is not looking at.
 *
 * **This is not a terminal.** It reads what a line looks like and nothing about
 * where a cursor is: a program that moves the cursor about, clears the screen
 * or draws a full-screen interface is not served by this and is not meant to
 * be.
 */

import anser from "anser";

/** One run of output that is set the same way throughout. */
export interface Span {
  readonly text: string;
  /** A CSS colour, or nothing for whatever the canvas sets. */
  readonly fg: string | null;
  readonly bg: string | null;
  readonly bold: boolean;
  readonly dim: boolean;
  readonly italic: boolean;
  readonly underline: boolean;
  readonly struck: boolean;
}

/**
 * The window's names for the eight and their bright halves, in the order the
 * standard puts them, which is the order the tokens are numbered in.
 */
const NAMED = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
];

/** The six levels a 256-colour cube is built from. */
const LEVELS = [0, 95, 135, 175, 215, 255];

/**
 * What one of the 256 is, for the ones the window has no token for.
 *
 * The first sixteen are the window's and are answered from its tokens. The
 * rest are a cube and a grey ramp, both fixed by the standard rather than by a
 * theme — there is nothing here for a theme to have an opinion about, and a
 * colour worked out here is the one every terminal shows.
 */
function ofPalette(index: number): string {
  if (index < 16) return `var(--ansi-${index})`;
  if (index > 231) {
    const grey = 8 + (index - 232) * 10;
    return `rgb(${grey} ${grey} ${grey})`;
  }
  const at = index - 16;
  const red = LEVELS[Math.floor(at / 36)] ?? 0;
  const green = LEVELS[Math.floor((at % 36) / 6)] ?? 0;
  const blue = LEVELS[at % 6] ?? 0;
  return `rgb(${red} ${green} ${blue})`;
}

/** What anser called a colour, as something CSS can be given. */
function colour(name: string | null, exact: string | null): string | null {
  if (name === null) return null;
  if (name === "ansi-truecolor") {
    // Apart from the palette because it is the one case with no index at all:
    // the program named the channels itself, and the window has no say.
    return exact === null ? null : `rgb(${exact.split(", ").join(" ")})`;
  }
  const palette = /^ansi-palette-(\d+)$/u.exec(name);
  if (palette !== null) return ofPalette(Number(palette[1]));

  const bright = name.startsWith("ansi-bright-");
  const plain = name.slice(bright ? "ansi-bright-".length : "ansi-".length);
  const at = NAMED.indexOf(plain);
  return at === -1 ? null : `var(--ansi-${at + (bright ? 8 : 0)})`;
}

/** Written as escapes, so no source file in this window holds a control byte. */
const ESC = "\u001b";
const BELL = "\u0007";

/**
 * The window's title, and anything else a programme says to the terminal
 * rather than to a reader.
 *
 * Dropped here because the reader below passes these through as text: a shell
 * that sets its title would print the title into the output, once per prompt.
 * What ends one is the bell or a string terminator, and both spellings are in
 * use.
 */
const ASIDE = new RegExp(
  `${ESC}\\][^${BELL}${ESC}]*(?:${BELL}|${ESC}\\\\)`,
  "gu",
);

/**
 * What a programme sets and then leaves set, so that it survives the line
 * being started again below.
 */
const COLOURS = new RegExp(`${ESC}\\[[0-9;]*m`, "gu");

/**
 * A line as it ended up, given that a carriage return starts it again.
 *
 * This is what a progress bar is made of: one line written over and over, each
 * time from the first column. Printed as it arrived it is every state the bar
 * was ever in, all on one line.
 *
 * **What came before the last return is dropped rather than written over.** A
 * terminal would keep whatever the shorter write did not reach, and this does
 * not, on purpose: a programme that rewrites a line almost always erases it
 * first, and the sequence that erases it is one this cannot see — the reader
 * below drops it with the rest of the cursor's business. Keeping a tail that
 * was erased puts words on the screen the terminal never showed, and half a
 * stale line is harder to read than a line that is simply right.
 *
 * Colour set before the return is carried across, because it was set and never
 * unset: dropping it would turn a coloured bar plain on its last frame.
 */
function overwritten(line: string): string {
  const cut = line.lastIndexOf("\r");
  if (cut === -1) return line;
  const carried = line.slice(0, cut).match(COLOURS)?.join("") ?? "";
  return carried + line.slice(cut + 1);
}

export function painted(text: string): readonly Span[] {
  const readable = text
    .replace(ASIDE, "")
    .split("\n")
    .map(overwritten)
    .join("\n");

  return anser
    .ansiToJson(readable, { use_classes: true, json: true, remove_empty: true })
    .map((run) => ({
      text: run.content,
      fg: colour(run.fg, run.fg_truecolor),
      bg: colour(run.bg, run.bg_truecolor),
      bold: run.decorations.includes("bold"),
      dim: run.decorations.includes("dim"),
      italic: run.decorations.includes("italic"),
      underline: run.decorations.includes("underline"),
      struck: run.decorations.includes("strikethrough"),
    }));
}
