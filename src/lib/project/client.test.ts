/**
 * What `loadProjectView` shares across the hooks that mount when a project
 * opens, held against the sharing itself.
 *
 * Four hooks read the same preference file on mount, and without the
 * in-flight promise each would fire its own IPC for the same thing. These
 * tests hold the dedup to the behaviour the rest of the window depends on:
 * one round-trip for concurrent callers, and a fresh one after a write.
 */

import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { after, beforeEach, describe, test } from "node:test";

const EMPTY_VIEW = {
  sections: [],
  hiddenTypes: [],
  unwatchedKinds: [],
  hiddenSections: [],
  dismissed: [],
};

let invokeLog: { name: string; args: unknown }[] = [];
let response: unknown = EMPTY_VIEW;

declare global {
  var __invokeLog: { name: string; args: unknown }[] | undefined;
  var __response: unknown;
}

registerHooks({
  load(url, _context, next) {
    if (url.includes("@tauri-apps/api/core")) {
      return {
        format: "module",
        source: `
          export async function invoke(name, args) {
            globalThis.__invokeLog?.push({ name, args });
            return globalThis.__response ?? {};
          }
        `,
        shortCircuit: true,
      };
    }
    return next(url, _context);
  },
});

const { loadProjectView, saveProjectView } = await import("@/lib/project/client");

beforeEach(() => {
  invokeLog = [];
  response = { ...EMPTY_VIEW };
  globalThis.__invokeLog = invokeLog;
  globalThis.__response = response;
});

after(() => {
  delete globalThis.__invokeLog;
  delete globalThis.__response;
});

describe("loadProjectView dedup", () => {
  test("concurrent calls make one IPC round-trip", async () => {
    const [a, b] = await Promise.all([loadProjectView("/test"), loadProjectView("/test")]);

    const loads = invokeLog.filter((c) => c.name === "project_view_load");
    assert.equal(loads.length, 1, "two concurrent calls should share one IPC");
    assert.deepEqual(a, response);
    assert.deepEqual(b, response);
  });

  test("a settled call is re-fetched on the next invocation", async () => {
    await loadProjectView("/test");
    await loadProjectView("/test");

    const loads = invokeLog.filter((c) => c.name === "project_view_load");
    assert.equal(loads.length, 2, "after settling, a new call should be made");
  });

  test("saveProjectView invalidates the cache", async () => {
    await loadProjectView("/test");
    await saveProjectView("/test", { sections: ["x"] });
    await loadProjectView("/test");

    const loads = invokeLog.filter((c) => c.name === "project_view_load");
    assert.equal(loads.length, 2, "a load after a save should not hit the cache");
  });
});
