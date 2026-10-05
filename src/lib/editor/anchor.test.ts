/**
 * What an anchor survives, stated as the edits that have to be survived.
 *
 * Every case here is a thing somebody does to a record with a note already on
 * it. They are written as edits rather than as inputs because that is what
 * decides whether the feature works: an anchor is only as good as the number of
 * ordinary changes it walks through, and the interesting ones are all failures
 * of a naive implementation — a stored offset, a stored path, a bare quote with
 * no neighbours.
 *
 * The last two are the honest limits: a match found approximately says so, and
 * a passage that is gone answers nothing rather than answering somewhere.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  describeAnchor,
  flatten,
  locateAnchor,
  offsetAt,
  pointAt,
  rangeAt,
  type DocumentNode,
} from "@/lib/editor/anchor";

/** A paragraph, as both the editor's value and the reading view spell one. */
const para = (text: string): DocumentNode => ({ children: [{ text }] });

const BODY =
  "The engine answers the window, and it is not the engine that was built from this checkout. It says so on stdout at launch.";

/** A note on the second "engine", the one the sentence is actually about. */
const anchorOnSecondEngine = () => {
  const at = BODY.indexOf("engine", BODY.indexOf("engine") + 1);
  return describeAnchor(BODY, at, at + "engine".length);
};

describe("what is stored", () => {
  test("the quote carries the text on either side of it", () => {
    const anchor = anchorOnSecondEngine();
    assert.equal(anchor.quote, "engine");
    assert.ok(anchor.prefix.endsWith("it is not the "));
    assert.ok(anchor.suffix.startsWith(" that was built"));
  });

  test("a selection at the very start keeps an empty prefix rather than reaching before the text", () => {
    const anchor = describeAnchor(BODY, 0, 3);
    assert.equal(anchor.quote, "The");
    assert.equal(anchor.prefix, "");
  });
});

describe("what an anchor survives", () => {
  test("a document nobody touched", () => {
    const anchor = anchorOnSecondEngine();
    const at = locateAnchor(BODY, anchor);
    assert.deepEqual(at, { start: anchor.start, end: anchor.end, exact: true });
  });

  test("a paragraph added above it, which moves every offset", () => {
    const anchor = anchorOnSecondEngine();
    const moved = `A sentence nobody had written yet.\n${BODY}`;
    const at = locateAnchor(moved, anchor);
    assert.ok(at);
    assert.equal(at.exact, true);
    assert.equal(moved.slice(at.start, at.end), "engine");
    // The one the note was about, not the first one in the new text.
    assert.ok(moved.slice(0, at.start).endsWith("it is not the "));
  });

  test("a quote that appears three times, where only the neighbours separate them", () => {
    const anchor = anchorOnSecondEngine();
    const crowded = `The engine is a process.\n${BODY}\nThe engine is replaced from Settings.`;
    const at = locateAnchor(crowded, anchor);
    assert.ok(at);
    assert.ok(crowded.slice(0, at.start).endsWith("it is not the "));
    assert.ok(crowded.slice(at.end).startsWith(" that was built"));
  });

  test("a word changed inside the marked passage, which is the one answer that is not exact", () => {
    const quote = "the engine that was built from this checkout";
    const anchor = describeAnchor(BODY, BODY.indexOf(quote), BODY.indexOf(quote) + quote.length);
    const edited = BODY.replace("was built", "was rebuilt");
    const at = locateAnchor(edited, anchor);
    assert.ok(at);
    assert.equal(at.exact, false);
    assert.equal(edited.slice(at.start, at.end), "the engine that was rebuilt from this checkout");
  });

  test("the marked passage deleted, which is nowhere rather than somewhere", () => {
    const anchor = describeAnchor(BODY, BODY.indexOf("stdout"), BODY.indexOf("launch.") + "launch.".length);
    const cut = BODY.slice(0, BODY.indexOf(" It says so"));
    assert.equal(locateAnchor(cut, anchor), null);
  });

  test("the save that reflows the body, because the flat text never held the spelling", () => {
    // The same sentence before and after somebody emphasised two words in it:
    // one text node becomes three, which is the shape a stored path dies on and
    // this string does not notice. What a save respells — `*` to `-`, a soft
    // wrap to one line — is Markdown, and no bullet or newline of a block's own
    // reaches here to be respelled.
    const before = flatten([para("A claim"), para("one that moved on its own")]);
    const after = flatten([
      para("A claim"),
      { children: [{ text: "one that " }, { text: "moved" }, { text: " on its own" }] },
    ]);
    assert.equal(before.text, after.text);

    const anchor = describeAnchor(before.text, before.text.indexOf("moved"), before.text.indexOf("moved") + 5);
    const at = locateAnchor(after.text, anchor);
    assert.deepEqual(at, { start: anchor.start, end: anchor.end, exact: true });
    // And it still resolves to places the editor can be given.
    assert.deepEqual(rangeAt(after, at)?.anchor, { path: [1, 1], offset: 0 });
  });

  test("an empty quote anchors nothing, rather than anchoring everywhere", () => {
    assert.equal(locateAnchor(BODY, describeAnchor(BODY, 4, 4)), null);
  });
});

describe("the document as one string", () => {
  test("blocks are separated by a newline and by nothing else", () => {
    const flat = flatten([para("First"), para("Second")]);
    assert.equal(flat.text, "First\nSecond");
  });

  test("the marks inside a sentence are one line, because they are text", () => {
    const flat = flatten([{ children: [{ text: "a " }, { text: "bold" }, { text: " word" }] }]);
    assert.equal(flat.text, "a bold word");
  });

  test("a table cell is a block, so a note may be left inside one", () => {
    const table: DocumentNode = {
      children: [{ children: [{ children: [{ text: "Left" }] }, { children: [{ text: "Right" }] }] }],
    };
    const flat = flatten([table]);
    assert.equal(flat.text, "Left\nRight");

    const anchor = describeAnchor(flat.text, flat.text.indexOf("Right"), flat.text.length);
    const at = locateAnchor(flat.text, anchor);
    assert.ok(at);
    assert.deepEqual(rangeAt(flat, at)?.anchor, { path: [0, 0, 1, 0], offset: 0 });
  });
});

describe("the way back to the editor's own places", () => {
  const flat = flatten([{ children: [{ text: "a " }, { text: "bold" }, { text: " word" }] }, para("next")]);

  test("an offset inside a node is that node and the offset within it", () => {
    assert.deepEqual(pointAt(flat, 3), { path: [0, 1], offset: 1 });
  });

  test("the join between two marks leans forward for a start and back for an end", () => {
    assert.deepEqual(pointAt(flat, 6, true), { path: [0, 2], offset: 0 });
    assert.deepEqual(pointAt(flat, 6, false), { path: [0, 1], offset: 4 });
  });

  test("a selection becomes offsets, which is the only form an anchor is stored in", () => {
    assert.equal(offsetAt(flat, { path: [0, 1], offset: 2 }), 4);
    assert.equal(offsetAt(flat, { path: [1, 0], offset: 0 }), "a bold word\n".length);
  });

  test("a path this document does not have is nowhere, not offset zero", () => {
    assert.equal(offsetAt(flat, { path: [9, 0], offset: 0 }), null);
  });

  test("a range covering a whole block ends at the end of it, not at the start of the next", () => {
    const whole = rangeAt(flat, { start: 0, end: "a bold word".length, exact: true });
    assert.deepEqual(whole?.anchor, { path: [0, 0], offset: 0 });
    assert.deepEqual(whole?.focus, { path: [0, 2], offset: " word".length });
  });
});
