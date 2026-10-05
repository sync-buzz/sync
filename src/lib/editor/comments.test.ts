/**
 * Where the comments of one body end up, and in what order they are read.
 *
 * The anchoring itself is held next door, in `anchor.test.ts`. What is held
 * here is the part the interface depends on: the list is in the order of the
 * page, a comment whose passage is gone keeps its place in the list rather than
 * disappearing from it, and a comment that moved is offered a new anchor only when
 * the new one says something different.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { describeAnchor, flatten, type DocumentNode } from "@/lib/editor/anchor";
import { movedComments, placeComments, sameAnchor, type DocumentComment } from "@/lib/editor/comments";

const para = (text: string): DocumentNode => ({ children: [{ text }] });

const BODY: readonly DocumentNode[] = [
  para("The window and the engine agree on a shape."),
  para("An unknown member is dropped without an error."),
];

/** A comment on the words `unknown member`, as leaving one would store it. */
function commentOn(nodes: readonly DocumentNode[], quote: string, body: string): DocumentComment {
  const text = flatten(nodes).text;
  const at = text.indexOf(quote);
  return { key: `comment-${quote.replace(/\W+/g, "-")}`, body, anchor: describeAnchor(text, at, at + quote.length) };
}

describe("where the comments of a body are", () => {
  test("each comment carries the passage it covers now", () => {
    const comment = commentOn(BODY, "unknown member", "Which member? Name it.");
    const [placed] = placeComments(BODY, [comment]);
    assert.ok(placed.at);
    assert.equal(flatten(BODY).text.slice(placed.at.start, placed.at.end), "unknown member");
    assert.deepEqual(placed.range?.anchor, { path: [1, 0], offset: "An ".length });
  });

  test("the list is in the order of the page, not the order it was written in", () => {
    const later = commentOn(BODY, "unknown member", "Second in the text.");
    const earlier = commentOn(BODY, "agree on a shape", "First in the text.");
    const placed = placeComments(BODY, [later, earlier]);
    assert.deepEqual(
      placed.map((one) => one.comment.body),
      ["First in the text.", "Second in the text."],
    );
  });

  test("a comment whose passage is gone keeps its place in the list, at the end of it", () => {
    const standing = commentOn(BODY, "unknown member", "Still about something.");
    const orphan = commentOn(BODY, "agree on a shape", "About a sentence somebody removed.");
    const edited = [para("The window and the engine were rewritten entirely."), BODY[1]];

    const placed = placeComments(edited, [orphan, standing]);
    assert.equal(placed.length, 2);
    assert.equal(placed[1].comment.body, "About a sentence somebody removed.");
    assert.equal(placed[1].at, null);
    assert.equal(placed[1].range, null);
    // And the one that is still there is unaffected by its neighbour's loss.
    assert.ok(placed[0].at);
  });

  test("a body with no comments places nothing rather than failing to", () => {
    assert.deepEqual(placeComments(BODY, []), []);
  });
});

describe("what a save has to write", () => {
  test("a passage that moved is described from the text as it now reads", () => {
    const comment = commentOn(BODY, "unknown member", "Which member?");
    const moved = [para("A sentence added above."), ...BODY];
    const placed = placeComments(moved, [comment]);

    const [write] = movedComments(placed);
    assert.equal(write.key, comment.key);
    assert.equal(write.anchor.quote, "unknown member");
    assert.notEqual(write.anchor.start, comment.anchor.start);
    assert.equal(sameAnchor(write.anchor, comment.anchor), false);
  });

  test("a record nobody moved a passage in costs no writes at all", () => {
    const comment = commentOn(BODY, "unknown member", "Which member?");
    assert.deepEqual(movedComments(placeComments(BODY, [comment])), []);
  });

  test("an orphaned comment is left as it was, because its quote is all that is left of the passage", () => {
    const orphan = commentOn(BODY, "agree on a shape", "About a sentence somebody removed.");
    const edited = [para("Rewritten entirely."), BODY[1]];
    const placed = placeComments(edited, [orphan]);
    assert.equal(placed[0].fresh, null);
    assert.deepEqual(movedComments(placed), []);
  });
});
