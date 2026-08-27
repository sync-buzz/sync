"use client";

/**
 * Asking the editor what it would do to a body, without showing it one.
 *
 * The workspace has to decide whether a record is editable *before* it renders
 * anything, and the answer depends on the same plugins the editor is built from
 * — so this is one editor instance, built once and never mounted, used to round
 * trip the Markdown and compare. Reusing it is safe because the only thing asked
 * of it is the Markdown API, which reads its arguments and keeps nothing.
 */

import { createPlateEditor } from "platejs/react";

import { fidelity, type Fidelity } from "@/lib/editor/markdown";
import { EDITOR_PLUGINS } from "@/lib/editor/plugins";

let probe: ReturnType<typeof createPlateEditor> | null = null;

/** Whether this body survives being edited, and why not when it does not. */
export function editorHolds(markdown: string): Fidelity {
  probe ??= createPlateEditor({ plugins: EDITOR_PLUGINS });
  return fidelity(probe, markdown);
}

/**
 * The verdicts reached about records that are open, by key.
 *
 * The question is asked about the body a record was *opened* with, and answered
 * once. Asking it again about the body the store answers a save with made it a
 * question a person could fail while typing: write a line the round trip would
 * not survive, wait for the save, and the editor would be replaced mid-sentence
 * by the reading view, with the caret gone.
 *
 * A verdict is dropped when the record is closed, so opening it again asks
 * about whatever it holds by then — an agent may have rewritten it — and the
 * map never grows past what the window has open.
 */
const decided = new Map<string, Fidelity>();

/** Whether this record can be edited, decided when it was opened. */
export function editorHoldsRecord(key: string, markdown: string): Fidelity {
  const known = decided.get(key);
  if (known !== undefined) return known;
  const verdict = editorHolds(markdown);
  decided.set(key, verdict);
  return verdict;
}

/** Forget the verdict for one record, because it is no longer open. */
export function forgetEditability(key: string): void {
  decided.delete(key);
}
