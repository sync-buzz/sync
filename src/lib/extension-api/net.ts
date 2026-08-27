"use client";

import { invoke } from "@tauri-apps/api/core";

import type { ExtensionNet, NetAnswer } from "@/lib/extension-api/contract";

/**
 * Reading something nobody in this window wrote.
 *
 * Here rather than beside the rest of the surface's functions because of the
 * one thing that makes it different: **every other call an extension makes is
 * about the project, and this one is about the extension.** What may be reached
 * is a sentence in the package's own manifest, so the request has to arrive in
 * Rust attributed to a package, and an id passed as an argument by whoever
 * called would be an extension naming its own permission.
 *
 * So this is not exported. The host builds one of these per package while it is
 * activating it — the id is closed over, out of the manifest the store
 * resolved — and hands it to the module as `host.net`. A package holds what it
 * was given; there is no function on the surface it could call instead, and
 * nothing it can pass to reach anywhere its card did not say.
 *
 * The check is not here either, and deliberately: this builds a request and
 * says what came back. Rust reads the manifest off the artefact on this
 * machine, refuses a host it does not name and refuses every redirect the same
 * way. A check in the window would be a check inside the thing being checked.
 */
export function netFor(id: string): ExtensionNet {
  return {
    read: (url: string) => invoke<NetAnswer>("extension_fetch", { id, url }),
  };
}
