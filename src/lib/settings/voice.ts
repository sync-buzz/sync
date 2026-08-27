"use client";

/**
 * What Sync says out loud, as the window asks about it.
 *
 * Unlike the appearance and the typography beside it, none of this is in the
 * window's own storage. A voice has to be readable when there is no window at
 * all — a handler woken by the clock speaks with every window closed — so the
 * choice lives in `voice.json` in this installation's configuration directory
 * and is read in Rust. What crosses here is a typed view of it.
 *
 * The list of voices is the system's and can change while this window is open:
 * somebody downloads an Enhanced voice in System Settings, and the next read
 * has one more. So nothing is cached beyond a render — the page asks when it
 * opens, and after every choice it makes.
 */

import { invoke } from "@tauri-apps/api/core";

/** How good a voice sounds, as the platform grades it. */
export type VoiceQuality = "standard" | "enhanced" | "premium";

export interface Voice {
  /** The platform's own identifier, and what a preference stores. */
  readonly id: string;
  /** What a person reads: `Milena`, `Daniel`. */
  readonly name: string;
  /** A BCP-47 tag — `ru-RU`, `en-GB`. What the list groups by. */
  readonly language: string;
  readonly quality: VoiceQuality;
}

/** An engine this build knows about, and whether it is here. */
export interface VoiceEngine {
  readonly id: string;
  readonly label: string;
  /** Why it cannot be used on this machine, or `null` when it can. */
  readonly absent: string | null;
}

export interface VoiceSettings {
  readonly engine: string;
  /** `null` is a real answer: the system speaks in whatever it is set to. */
  readonly voice: string | null;
  /** A multiplier over normal speech, where `1` is the platform's normal. */
  readonly rate: number;
  /**
   * Whether an agent connected to Sync may speak.
   *
   * The one choice here that starts off. A package that can speak was installed
   * from a card that said so, and that card is the consent; an agent was
   * connected from a page that said nothing of the kind, so this is the first
   * moment anybody could agree to it.
   */
  readonly agents: boolean;
}

export interface VoiceStatus {
  readonly engines: readonly VoiceEngine[];
  readonly voices: readonly Voice[];
  readonly settings: VoiceSettings;
  /** Why there is nothing to choose from, when there is nothing. */
  readonly failure: string | null;
}

/** The bounds the engine will clamp to anyway, stated where the control is. */
export const SLOWEST = 0.5;
export const FASTEST = 2;

export function loadVoice(): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("voice_status");
}

/**
 * Write a choice down, and answer with what the page should now show.
 *
 * The whole preference rather than a field, because the answer includes the
 * voices of whichever engine is now chosen — a page that patched one field
 * would have to ask again to draw the list beside it.
 */
export function chooseVoice(settings: VoiceSettings): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("voice_choose", { settings });
}

/** Say something in the voice this machine is set to. */
export function speak(text: string, interrupt = true): Promise<void> {
  return invoke<void>("voice_speak", { text, interrupt });
}

/** Stop what is being said and drop what is waiting. */
export function stopSpeaking(): Promise<void> {
  return invoke<void>("voice_stop");
}

/**
 * What a language tag is called, in the reader's own window.
 *
 * `Intl.DisplayNames` is the platform's answer and is already in every webview,
 * so a table of forty language names is not written here to go stale. A tag it
 * cannot name is shown as itself, which is better than "Unknown": `ru-RU` at
 * least tells somebody what they are looking at.
 */
export function languageNamed(tag: string): string {
  try {
    const named = new Intl.DisplayNames(undefined, { type: "language" }).of(tag);
    return named ?? tag;
  } catch {
    return tag;
  }
}
