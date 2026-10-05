/**
 * What a `sync://` address is, held against the other side that reads it.
 *
 * The grammar is written twice on purpose — here for the window, and in
 * `src-tauri/src/links.rs` for an address arriving from anywhere else on the
 * machine — because the two run in different languages and must agree exactly:
 * a link one side reads as a record of another project and the other reads as a
 * kind opens the wrong thing, silently, in somebody's window. So both files
 * state the spellings in full rather than deriving them, and these are the
 * window's half of that statement.
 *
 * `named` in that module holds the same cases in Rust. A change to either that
 * is not made to both is what this exists to fail on.
 */

import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { recordHref, recordTarget } from "@/lib/record-link";

describe("what an address names", () => {
  test("two segments are a record of the project the reader has open", () => {
    assert.deepEqual(recordTarget("sync://decision/d-one"), {
      project: null,
      kind: "decision",
      key: "d-one",
    });
  });

  test("three segments name the project first", () => {
    assert.deepEqual(recordTarget("sync://SYNC/tasks.task/task-a92e5b"), {
      project: "SYNC",
      kind: "tasks.task",
      key: "task-a92e5b",
    });
  });

  test("the case of a segment survives, because every one of them decides something", () => {
    // The identifier decides which project opens it and the kind decides which
    // section does. Folding either is opening something else.
    assert.deepEqual(recordTarget("sync://SYNC/Decision/D-One"), {
      project: "SYNC",
      kind: "Decision",
      key: "D-One",
    });
  });

  test("the scheme is the one part read either way", () => {
    assert.notEqual(recordTarget("Sync://decision/d-one"), null);
    assert.notEqual(recordTarget("SYNC://decision/d-one"), null);
  });

  test("a segment is unescaped as far as it was escaped", () => {
    assert.deepEqual(recordTarget("sync://SYNC/a%20kind/d%281%29"), {
      project: "SYNC",
      kind: "a kind",
      key: "d(1)",
    });
  });

  test("a query or a fragment does not change which record it is", () => {
    assert.deepEqual(recordTarget("sync://SYNC/tasks.task/task-1?from=chat#top"), {
      project: "SYNC",
      kind: "tasks.task",
      key: "task-1",
    });
  });

  test("one trailing slash is forgiven and two are not", () => {
    assert.deepEqual(recordTarget("sync://decision/d-one/"), {
      project: null,
      kind: "decision",
      key: "d-one",
    });
    assert.equal(recordTarget("sync://decision/d-one//"), null);
  });

  test("what is not an address is drawn as the ordinary link it looks like", () => {
    assert.equal(recordTarget("https://example.com/decision/d-one"), null);
    assert.equal(recordTarget("./setup.md"), null);
    assert.equal(recordTarget("sync://decision"), null);
    assert.equal(recordTarget("sync://SYNC/tasks.task/task-1/extra"), null);
    assert.equal(recordTarget("sync://"), null);
    assert.equal(recordTarget("sync:///d-one"), null);
    assert.equal(recordTarget("sync://decision//d-one"), null);
  });
});

describe("how an address is written", () => {
  test("a body writes the project's own records without naming the project", () => {
    assert.equal(
      recordHref({ kind: "decision", key: "d-one" }),
      "sync://decision/d-one",
    );
  });

  test("an address leaving the window names the project", () => {
    assert.equal(
      recordHref({ project: "SYNC", kind: "tasks.task", key: "task-1" }),
      "sync://SYNC/tasks.task/task-1",
    );
  });

  test("what would end the url early is escaped, and what would not is left", () => {
    // A space in a kind would end a Markdown destination and a parenthesis would
    // close it, so both are escaped — and an ordinary key stays readable.
    assert.equal(
      recordHref({ project: "A B", kind: "a kind", key: "d(1)" }),
      "sync://A%20B/a%20kind/d(1)",
    );
    assert.equal(
      recordHref({ kind: "kind", key: "a-b_c.d" }),
      "sync://kind/a-b_c.d",
    );
  });

  test("what is written is what is read back", () => {
    const written = recordHref({
      project: "SYNC",
      kind: "a kind",
      key: "d/one",
    });
    assert.deepEqual(recordTarget(written), {
      project: "SYNC",
      kind: "a kind",
      key: "d/one",
    });
  });
});
