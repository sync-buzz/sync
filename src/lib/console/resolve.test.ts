/**
 * What a typed line is read as, held against the reading itself.
 *
 * These four functions are the console's whole understanding of what somebody
 * typed, and they are about to be rewritten onto a parse tree. That is what
 * this file is for: it says what the line means, never how the meaning is
 * arrived at, so the rewrite is held to the same answers rather than to the
 * same code.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  THEN,
  completions,
  ghost,
  highlight,
  intent,
  steps,
  verbs,
} from "@/lib/console/resolve";

describe("what a line is", () => {
  test("a line marked for a process keeps everything after the mark", () => {
    assert.deepEqual(intent("!git log | head -3"), {
      kind: "shell",
      line: "git log | head -3",
    });
  });

  test("an addressed line separates who from what", () => {
    assert.deepEqual(intent("@review посмотри диф"), {
      kind: "address",
      name: "review",
      text: "посмотри диф",
    });
  });

  test("a name with nothing said to it raises nobody", () => {
    assert.equal(intent("@review").kind, "none");
    assert.equal(intent("@").kind, "none");
  });

  test("an address alone is a document to open", () => {
    assert.deepEqual(intent("sync://blocks/plan-console"), {
      kind: "open",
      address: "sync://blocks/plan-console",
    });
  });

  test("an address with a sentence around it is not an opening", () => {
    assert.equal(intent("@review sync://blocks/plan-console").kind, "address");
  });

  test("an address ending in a separator names a place, not a document", () => {
    assert.equal(intent("sync://blocks/").kind, "none");
  });

  test("a declared verb carries its words as arguments", () => {
    assert.deepEqual(intent("find консоль шторка"), {
      kind: "verb",
      name: "find",
      args: ["консоль", "шторка"],
    });
  });

  test("an argument is what was typed, dashes and all", () => {
    // The parser divides `plan-` into a word and a dash, because `->` may
    // never be half of a word. Counted apart, `cd my-folder-` went to
    // `my-folder`.
    assert.deepEqual(intent("find plan-"), {
      kind: "verb",
      name: "find",
      args: ["plan-"],
    });
    assert.deepEqual(intent("cd my-folder-"), {
      kind: "verb",
      name: "cd",
      args: ["my-folder-"],
    });
  });

  test("a quoted argument is one argument", () => {
    const read = intent('find "два слова"');
    assert.deepEqual(read.kind === "verb" ? read.args : null, ['"два слова"']);
  });

  test("ordinary writing is refused rather than sent to anybody", () => {
    const read = intent("починить шторку");
    assert.equal(read.kind, "none");
    // The refusal teaches the gesture that would have worked. A sentence that
    // stopped at "no such command" would leave somebody guessing at the one
    // thing this window wanted them to type.
    assert.match(read.kind === "none" ? read.why : "", /@name/u);
  });

  test("surrounding space changes nothing", () => {
    assert.deepEqual(intent("   find дом  "), intent("find дом"));
  });

  test("an empty line is refused and raises nothing", () => {
    assert.equal(intent("").kind, "none");
  });
});

describe("a line cut into steps", () => {
  test("one step comes back as a list of one", () => {
    assert.deepEqual(steps("find дом"), ["find дом"]);
  });

  test("the separator divides and is not kept", () => {
    assert.deepEqual(steps(`!git diff ${THEN} @review посмотри`), [
      "!git diff",
      "@review посмотри",
    ]);
  });

  test("a shell's own marks are not separators", () => {
    // `|` and `>` belong to the process the line was handed to. Reading either
    // of them here would mean this window parsing shell.
    assert.deepEqual(steps("!cargo test 2>&1 | head"), ["!cargo test 2>&1 | head"]);
  });

  test("an empty step is dropped rather than carried", () => {
    assert.deepEqual(steps(`find дом ${THEN} ${THEN}`), ["find дом"]);
  });
});

describe("colour follows the same reading", () => {
  /** What the painted stretches spell, which has to be the line itself. */
  const spelled = (line: string) =>
    highlight(line)
      .map((painted) => painted.text)
      .join("");

  for (const line of [
    "",
    "!git status",
    "@review посмотри диф",
    "sync://blocks/plan-console",
    "find дом",
    "  find дом",
    `!git diff ${THEN} @review посмотри`,
    "полов",
  ]) {
    test(`nothing is lost or invented in ${JSON.stringify(line)}`, () => {
      assert.equal(spelled(line), line);
    });
  }

  test("the mark and what follows it are set apart", () => {
    assert.deepEqual(highlight("!git status"), [
      { text: "!", tone: "sigil" },
      { text: "git status", tone: "shell" },
    ]);
  });

  test("a declared verb is coloured as one and a word is not", () => {
    assert.equal(highlight("find дом")[0]?.tone, "verb");
    assert.equal(highlight("полы дом")[0]?.tone, "said");
  });

  test("half a name is half a word, not a mistake", () => {
    // A line is refused when Return is pressed, never while it is being typed.
    const painted = highlight("@revi");
    assert.deepEqual(
      painted.map((one) => one.tone),
      ["sigil", "name"],
    );
  });

  test("the separator stays quiet between two coloured steps", () => {
    const painted = highlight(`!git diff ${THEN} find дом`);
    assert.ok(painted.some((one) => one.text === THEN && one.tone === "sigil"));
  });
});

describe("what the line could become", () => {
  const named = [{ name: "review", hint: "reading the diff" }];

  test("an empty line offers the whole vocabulary", () => {
    const offered = completions("", named);
    assert.ok(offered.some((one) => one.token === "@review"));
    assert.ok(offered.some((one) => one.token === "find"));
    assert.equal(offered.length, named.length + verbs().length);
  });

  test("tokens are offered as they would be typed", () => {
    assert.ok(completions("@", named).every((one) => one.token.startsWith("@")));
  });

  test("a word already whole is not offered back", () => {
    assert.ok(completions("find", named).every((one) => one.token !== "find"));
  });

  test("what was used last is offered first", () => {
    const offered = completions("", named, [], ["types", "@review"]);
    assert.deepEqual(
      offered.slice(0, 2).map((one) => one.token),
      ["types", "@review"],
    );
  });

  test("inside a process the vocabulary is the machine's", () => {
    assert.deepEqual(completions("!git ch", named), []);
  });

  test("past the first word nothing is finished", () => {
    assert.deepEqual(completions("find дом", named), []);
  });

  test("an address is completed wherever it stands in the line", () => {
    const paths = [{ name: "sync://blocks/plan-console", hint: "a block" }];
    const offered = completions("@review открой sync://bl", named, paths);
    assert.deepEqual(
      offered.map((one) => one.token),
      ["sync://blocks/plan-console"],
    );
  });
});

describe("the ghost of the first suggestion", () => {
  test("only the tail is drawn, because the head is already on screen", () => {
    assert.equal(ghost("folders", "fol"), "ders");
  });

  test("a mark on its own is not a word yet", () => {
    assert.equal(ghost("@review", "@"), null);
  });

  test("a suggestion that does not continue the word draws nothing", () => {
    assert.equal(ghost("find", "types"), null);
  });
});
