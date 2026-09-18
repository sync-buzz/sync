"use client";

import { invoke } from "@tauri-apps/api/core";

/**
 * A tap the hardware makes, for a gesture the person made.
 *
 * Nothing on a Mac, which has no haptic engine. On a phone this is the
 * platform's own feedback for selection — the same feel the system gives a
 * picker landing on a row — fired for switching a section in the band and for
 * moving the pager to a different column. The command is a no-op on a machine
 * that is not a phone, so the check here is only to keep the call off the wire
 * on a desk and in a browser.
 *
 * Fire-and-forget: the hardware answers, and the window waits for nothing.
 */
export function haptic() {
  if (typeof window !== "undefined" && window.__SYNC_DEVICE__ === "phone") {
    invoke("haptic").catch(() => {});
  }
}
