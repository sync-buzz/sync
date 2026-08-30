/**
 * What this machine holds in the system keychain, and how one entry changes.
 *
 * Reading a secret is deliberately not here. The window never shows one, so a
 * function in this file that returned a value would exist only to be misused.
 * There is a read in `src-tauri/src/vault.rs`, and it is the other half of that
 * module: a package asking for its own secret, behind a capability, with the
 * owner taken from what this machine has installed. Nothing in the settings
 * window is that caller, and nothing here calls it.
 *
 * Which keychain is opened, and how an owner and a name are joined into an
 * entry that cannot reach outside Sync's own, is decided in Rust. This module
 * can ask for *this owner's entry, by that name*, and nothing else is
 * reachable from the interface.
 */

import { invoke } from "@tauri-apps/api/core";

/** One secret's address: whose it is and what they call it. Never its value. */
export interface VaultEntry {
  readonly owner: string;
  readonly name: string;
}

/**
 * How long the store holds what it is given, as the store itself states it.
 *
 * Asked rather than assumed: a store that keeps an entry until somebody deletes
 * it and a store that loses it at the next reboot are both correct, and the
 * difference is the whole of what a person needs to know before typing a token
 * into one.
 */
export type Persistence =
  | "untilDeleted"
  | "untilLogout"
  | "untilReboot"
  | "whileRunning"
  | "unknown";

/** Every secret Sync holds, whoever it belongs to. */
export function loadVaultEntries(): Promise<VaultEntry[]> {
  return invoke<VaultEntry[]>("vault_entries", {});
}

/** Put a secret in, or replace the one that is there. */
export function writeSecret(
  owner: string,
  name: string,
  secret: string,
): Promise<void> {
  return invoke<void>("vault_write", { owner, name, secret });
}

/** Take a secret out. */
export function forgetSecret(owner: string, name: string): Promise<void> {
  return invoke<void>("vault_forget", { owner, name });
}

/** What this machine's store promises about how long an entry lasts. */
export function vaultPersistence(): Promise<Persistence> {
  return invoke<Persistence>("vault_storage", {});
}
