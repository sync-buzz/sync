"use client";

import { invoke } from "@tauri-apps/api/core";

import type { ExtensionVault } from "@/lib/extension-api/contract";

/**
 * A package's own corner of the system keychain.
 *
 * Here rather than beside the rest of the surface's functions for the reason
 * `net` is: **every other call an extension makes is about the project, and
 * this one is about the extension.** Whose secret is being asked for is not a
 * question the caller gets to answer — the owner is the id resolved against the
 * store in Rust — so the call has to arrive attributed rather than carrying an
 * id somebody wrote.
 *
 * So this is not exported. The host builds one per package while it is
 * activating it, with the id closed over, and hands it to the module as
 * `host.vault`. A package holds what it was given; there is no function on the
 * surface it could call instead, and nothing it can pass that would name
 * another package's namespace.
 *
 * The value comes back into JavaScript, which is what makes this different from
 * every other door here, and the rule that governs what happens to it next is
 * one no code can hold: see `ExtensionVault` in `contract.ts`.
 */
export function vaultFor(id: string): ExtensionVault {
  return {
    read: (name: string) => invoke<string>("extension_secret_read", { id, name }),
    write: (name: string, secret: string) =>
      invoke<void>("extension_secret_write", { id, name, secret }),
    forget: (name: string) => invoke<void>("extension_secret_forget", { id, name }),
  };
}
