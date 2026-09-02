/**
 * Who may talk to this Mac from somewhere else, and what it is called there.
 *
 * A different question from the Vault beside it, and the reason they are two
 * sections rather than one: a secret in the vault is what a package uses to
 * reach *out*, and this is who may reach *in*. Somebody looking for the second
 * would not go looking in a list of packages' keys.
 *
 * A device's secret is answered exactly once, by the call that mints it, and
 * there is no function here that reads one back. That is not an omission to be
 * filled in later: a window able to show a paired device's key would be a
 * window that exports every device on this Mac, and the only moment the value
 * is any use is the moment it is being put into the device it belongs to.
 */

import { invoke } from "@tauri-apps/api/core";

/** One device somebody paired, as this Mac remembers it. Never its secret. */
export interface RemoteDevice {
  /** What the person called it. Theirs, and never matched on. */
  readonly name: string;
  /** What names its entry in the keychain, and what it is revoked by. */
  readonly fingerprint: string;
  /** Seconds since the epoch. */
  readonly pairedAt: number;
  /** When it last came in, or `null` if it never has. */
  readonly lastSeen: number | null;
}

/** What remote access is doing, and who is admitted to it. */
export interface RemoteStatus {
  readonly enabled: boolean;
  /**
   * What a device dials to reach this Mac, once the engine has a door open.
   *
   * `null` while remote access is off, and also for the moment between turning
   * it on and the door binding — which is why the section says which of the two
   * it is instead of drawing an empty field.
   */
  readonly endpoint: string | null;
  readonly devices: readonly RemoteDevice[];
  /** Why the engine is not holding what this side believes it sent. */
  readonly failure: string | null;
}

/** A device that has just been paired, and the one time its secret is legible. */
export interface PairedDevice {
  readonly device: RemoteDevice;
  readonly secret: string;
  readonly endpoint: string | null;
  /**
   * The address and the key as one payload, for the code the device reads.
   *
   * Composed in Rust, in the crate the phone shares, so that both ends spell
   * the format once. `null` where this Mac has no address yet — half a payload
   * would be a code that cannot work, pointed at somebody's camera.
   */
  readonly pairing: string | null;
}

export function loadRemoteStatus(): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_status", {});
}

/**
 * Turn remote access on or off.
 *
 * It restarts the memory engine, because what a machine is called on the
 * network is settled when its door is opened and cannot be handed to a process
 * that is already running. Everything with a window open reconnects.
 */
export function enableRemoteAccess(enabled: boolean): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_enable", { enabled });
}

/** Pair a device, and answer with the secret it is to hold. */
export function pairRemoteDevice(name: string): Promise<PairedDevice> {
  return invoke<PairedDevice>("remote_pair", { name });
}

/** Stop admitting a device. */
export function revokeRemoteDevice(fingerprint: string): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_revoke", { fingerprint });
}

/**
 * When a device was paired, as a date.
 *
 * A date rather than *3 days ago*, and the difference is what the two columns
 * are for: when it was paired is a fact somebody looks up — *is this the one I
 * set up at the office* — and how long ago that was answers nothing.
 */
export function paired(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * A time, as somebody reading a list wants it rather than as a date.
 *
 * *Just now* and *3 days ago* are what the column is for — whether a device is
 * still in use, and how long a stranger's copy of a key has been quiet. An
 * exact timestamp answers neither without arithmetic.
 */
export function when(seconds: number | null): string {
  if (seconds === null) return "Never";
  const ago = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (ago < 60) return "Just now";
  if (ago < 3600) return `${Math.floor(ago / 60)} min ago`;
  if (ago < 86_400) return `${Math.floor(ago / 3600)} h ago`;
  const days = Math.floor(ago / 86_400);
  return days === 1 ? "Yesterday" : `${days} days ago`;
}
