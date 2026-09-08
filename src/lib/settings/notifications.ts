"use client";

/**
 * When Sync interrupts, as the window asks about it.
 *
 * Like the voice beside it and unlike the appearance, none of this is in the
 * window's own storage: a banner is raised with every window closed, which is
 * the case it exists for, so the choice lives in `notifications.json` in this
 * installation's configuration directory and is read in Rust. What crosses here
 * is a typed view of it.
 *
 * Nothing here sends one. The banner is raised where the event happens and
 * nowhere else — a window that could ask for a notification would be a window
 * that could only ask while it was open.
 */

import { invoke } from "@tauri-apps/api/core";

/** Which of the three interruptions a person wants. */
export interface NotificationSettings {
  /** An agent is waiting for permission, and the turn has stopped until it is answered. */
  readonly asking: boolean;
  /** A turn ended. */
  readonly finished: boolean;
  /** A session could not be raised, or it fell over. */
  readonly failed: boolean;
}

export function loadNotifications(): Promise<NotificationSettings> {
  return invoke<NotificationSettings>("notifications_settings");
}

/**
 * Write the three switches down.
 *
 * The whole preference rather than the one that changed, because that is what
 * the command takes: three controls patching one member each are three ways for
 * the file to end up describing a state nobody chose.
 */
export function chooseNotifications(
  settings: NotificationSettings,
): Promise<NotificationSettings> {
  return invoke<NotificationSettings>("notifications_choose", { settings });
}
