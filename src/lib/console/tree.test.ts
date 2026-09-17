/**
 * What the grammar makes of a line, held against the line itself.
 *
 * Separate from the tests of what a line *means*: those say `find дом` is a
 * verb with an argument, these say it is one step of two words. A defect in
 * the grammar shows up here as a shape and there as a meaning, and having both
 * is what says which of the two moved.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { leavesOf, read, stepsOf, textOf, wordAt } from "@/lib/console/tree";

/** Every part of the line, named and spelled, which is the whole of a shape. */
const shape = (line: string) =>
  leavesOf(line).map((leaf) => `${leaf.name}:${textOf(line, leaf)}`);

/** Whether the parser had to invent anything to get to the end. */
const whole = (line: string) => !shape(line).some((part) => part.startsWith("⚠"));

describe("the four forms", () => {
  test("a mark hands the rest to a shell", () => {
    assert.deepEqual(shape("!git status"), ["Bang:!", "ShellText:git status"]);
  });

  test("a name and what is said to it are separate parts", () => {
    assert.deepEqual(shape("@review посмотри"), [
      "At:@",
      "Name:review",
      "Word:посмотри",
    ]);
  });

  test("an address is one part rather than a mark and a path", () => {
    assert.deepEqual(shape("sync://blocks/plan-console"), [
      "SyncAddress:sync://blocks/plan-console",
    ]);
  });

  test("a word opening a line is a word, and the grammar says nothing more", () => {
    // Whether `find` is a verb is a list held at run time. A grammar that knew
    // would be the shell learning a subject.
    assert.deepEqual(shape("find дом"), ["Word:find", "Word:дом"]);
  });

  test("an absolute path is told apart from a word", () => {
    assert.deepEqual(shape("cd /etc/hosts"), ["Word:cd", "FilePath:/etc/hosts"]);
  });
});

describe("a shell keeps its own marks", () => {
  test("a pipe is the shell's", () => {
    assert.deepEqual(shape("!git log | head"), [
      "Bang:!",
      "ShellText:git log | head",
    ]);
  });

  test("a redirect is the shell's", () => {
    assert.deepEqual(shape("!cargo test 2>&1"), [
      "Bang:!",
      "ShellText:cargo test 2>&1",
    ]);
  });

  test("a path inside a shell line is the file system's, not memory's", () => {
    // The one exception in the language, and it is kept by construction: the
    // parser is never in a state where an address could be read here.
    assert.deepEqual(shape("!cat /etc/hosts"), [
      "Bang:!",
      "ShellText:cat /etc/hosts",
    ]);
  });
});

describe("steps", () => {
  test("the mark divides, and only between steps", () => {
    assert.deepEqual(
      stepsOf("!git diff -> @review посмотри").map((step) =>
        textOf("!git diff -> @review посмотри", step).trim(),
      ),
      ["!git diff", "@review посмотри"],
    );
  });

  test("the mark divides without spaces around it", () => {
    assert.deepEqual(shape("find a->b"), ["Word:find", "Word:a", "Then:->", "Word:b"]);
  });

  test("a dash that is not the mark stays inside its word", () => {
    assert.deepEqual(shape("sync://blocks/plan-console"), [
      "SyncAddress:sync://blocks/plan-console",
    ]);
    assert.deepEqual(shape("plan-console"), ["Word:plan-console"]);
  });
});

describe("quotes are parts of their own", () => {
  test("a quoted run holds its spaces", () => {
    assert.deepEqual(shape('find "два слова"'), [
      "Word:find",
      'Quoted:"два слова"',
    ]);
  });

  test("single quotes do the same", () => {
    assert.deepEqual(shape("find 'два слова'"), [
      "Word:find",
      "Quoted:'два слова'",
    ]);
  });

  test("an escaped quote does not end the run", () => {
    assert.deepEqual(shape('find "a\\"b"'), ["Word:find", 'Quoted:"a\\"b"']);
  });
});

describe("Cyrillic is ordinary text", () => {
  for (const line of [
    "find консоль",
    "@обзор посмотри диф",
    "sync://blocks/план-консоли",
    "найди дом",
  ]) {
    test(`nothing is refused in ${JSON.stringify(line)}`, () => {
      assert.ok(whole(line), shape(line).join(" "));
    });
  }
});

describe("a line half typed is a state, not a mistake", () => {
  // Every prefix of a line somebody is about to finish. None of them may
  // produce an error node: a console that marked half a word as wrong would
  // be marking every word as wrong on the way to being right.
  for (const line of [
    "@review посмотри диф",
    "!git diff -> @review",
    "sync://blocks/plan-console",
    "cd /etc/hosts",
    'find "два слова"',
  ]) {
    test(`every prefix of ${JSON.stringify(line)} parses whole`, () => {
      for (let cut = 0; cut <= line.length; cut += 1) {
        const half = line.slice(0, cut);
        assert.ok(whole(half), `${JSON.stringify(half)} → ${shape(half).join(" ")}`);
      }
    });
  }

  test("a key halfway through keeps its dash", () => {
    assert.deepEqual(shape("plan-"), ["Word:plan", "Dashes:-"]);
    assert.equal(wordAt("plan-")?.text, "plan-");
  });

  test("a mark on its own is a mark", () => {
    assert.deepEqual(shape("@"), ["At:@"]);
    assert.deepEqual(shape("!"), ["Bang:!"]);
  });

  test("an empty line has no parts and no error", () => {
    assert.deepEqual(shape(""), []);
    assert.equal(read("").topNode.name, "Line");
  });
});

describe("the word the caret is in", () => {
  test("a run with nothing between it is one word", () => {
    assert.equal(wordAt("@rev")?.text, "@rev");
    assert.equal(wordAt("find дом")?.text, "дом");
    assert.equal(wordAt("@review открой sync://bl")?.text, "sync://bl");
  });

  test("a line ending in a space is between words", () => {
    assert.equal(wordAt("find "), null);
    assert.equal(wordAt(""), null);
  });
});
