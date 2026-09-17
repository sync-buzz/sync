/**
 * What a process said, read back.
 *
 * Every case here is something a real programme prints: `git` colours a
 * branch, `cargo` redraws one line, a shell sets the window's title. None of
 * it is invented — a fixture nobody's terminal produces would confirm the
 * reading rather than test it.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { painted } from "@/lib/console/ansi";

/** Spelled rather than typed, so this file itself holds no control bytes. */
const E = "\u001b";
const BELL = "\u0007";

const said = (text: string) =>
  painted(text)
    .map((span) => span.text)
    .join("");

describe("colour comes from the window's own sixteen", () => {
  test("one of the eight is a token", () => {
    assert.deepEqual(
      painted(`${E}[31mbroken${E}[0m`).map((span) => [span.text, span.fg]),
      [["broken", "var(--ansi-1)"]],
    );
  });

  test("a bright one is the token eight further on", () => {
    assert.equal(painted(`${E}[92mok${E}[0m`)[0]?.fg, "var(--ansi-10)");
  });

  test("the first sixteen of the palette are the same tokens", () => {
    assert.equal(painted(`${E}[38;5;9mx${E}[0m`)[0]?.fg, "var(--ansi-9)");
  });

  test("past the sixteen the palette is worked out, not themed", () => {
    // 208 is the orange a test runner warns in. Fixed by the standard: every
    // terminal shows the same one, so there is nothing for a theme to answer.
    assert.equal(painted(`${E}[38;5;208mx${E}[0m`)[0]?.fg, "rgb(255 135 0)");
  });

  test("the grey ramp is worked out too", () => {
    assert.equal(painted(`${E}[38;5;232mx${E}[0m`)[0]?.fg, "rgb(8 8 8)");
  });

  test("a colour the programme named itself is taken as given", () => {
    assert.equal(painted(`${E}[38;2;10;20;30mx${E}[0m`)[0]?.fg, "rgb(10 20 30)");
  });

  test("every one of the sixteen lands on its own token", () => {
    // The whole mapping at once, because it breaks silently: a name this does
    // not recognise is drawn in no colour at all rather than in the wrong one,
    // and nothing says so.
    const eight = [30, 31, 32, 33, 34, 35, 36, 37];
    for (const [at, code] of eight.entries()) {
      assert.equal(painted(`${E}[${code}mx${E}[0m`)[0]?.fg, `var(--ansi-${at})`);
      assert.equal(
        painted(`${E}[${code + 60}mx${E}[0m`)[0]?.fg,
        `var(--ansi-${at + 8})`,
      );
    }
  });

  test("a background is read as well as a foreground", () => {
    const span = painted(`${E}[41mx${E}[0m`)[0];
    assert.equal(span?.bg, "var(--ansi-1)");
    assert.equal(span?.fg, null);
  });

  test("what carries no colour carries none", () => {
    assert.equal(painted("plain")[0]?.fg, null);
  });
});

describe("weight and the rest", () => {
  test("bold and underline arrive together", () => {
    const span = painted(`${E}[1;4mheading${E}[0m`)[0];
    assert.equal(span?.bold, true);
    assert.equal(span?.underline, true);
  });

  test("what follows the reset carries nothing", () => {
    assert.equal(painted(`${E}[1mbold${E}[0mplain`).at(-1)?.bold, false);
  });
});

describe("what is not for a reader is dropped", () => {
  test("a shell setting the window title prints nothing", () => {
    assert.equal(said(`${E}]0;~/Projects${BELL}$ ls`), "$ ls");
  });

  test("a title ended the other way is dropped too", () => {
    assert.equal(said(`${E}]0;here${E}\\after`), "after");
  });

  test("clearing a line and hiding the cursor leave only the words", () => {
    assert.equal(said(`${E}[2K${E}[?25lCompiling${E}[?25h`), "Compiling");
  });
});

describe("a line written twice is the line it ended as", () => {
  test("a progress bar is its last state", () => {
    assert.equal(said("Building 50%\rBuilding 100%"), "Building 100%");
  });

  test("what was written before the last return is gone", () => {
    // Not what a terminal does with a shorter write, and deliberately so: the
    // sequence that erases the line first is one this cannot see, so a tail
    // kept here is a tail the terminal had already wiped.
    assert.equal(said("abcdef\rxy"), "xy");
  });

  test("colour set before the return still applies after it", () => {
    const spans = painted(`${E}[32mBuilding 50%\rBuilding 100%${E}[0m`);
    assert.equal(spans.map((span) => span.text).join(""), "Building 100%");
    assert.equal(spans[0]?.fg, "var(--ansi-2)");
  });

  test("no escape byte survives the reading", () => {
    // What the length arithmetic used to do: measure a string with escapes in
    // it and print the middle of one.
    assert.ok(!said(`${E}[32mABCDEFGHIJ\rxy`).includes("["));
    assert.equal(said(`${E}[32mABCDEFGHIJ\rxy`), "xy");
  });

  test("only the line it happened on is affected", () => {
    assert.equal(said("one\ntwo\rTWO\nthree"), "one\nTWO\nthree");
  });

  test("a line with no carriage return is left alone", () => {
    assert.equal(said("ordinary output"), "ordinary output");
  });
});

describe("nothing a reader wants is lost", () => {
  for (const text of ["", "plain", "two\nlines"]) {
    test(`the words survive ${JSON.stringify(text)}`, () => {
      assert.equal(said(text), text);
    });
  }

  test("the words survive being coloured", () => {
    assert.equal(
      said(`${E}[32mok${E}[0m and ${E}[31mbroken${E}[0m`),
      "ok and broken",
    );
  });
});
