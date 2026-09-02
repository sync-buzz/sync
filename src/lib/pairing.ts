"use client";

/**
 * Whether this window has a computer to ask, and how it gets one.
 *
 * The phone draws and the computer answers, so a phone with no computer has
 * nothing to draw: this is the one question that stands in front of the two the
 * window normally has. On a Mac it is not a question at all — the machine the
 * window is running on is the machine that answers — so nothing here is asked
 * there, and the hook says so rather than inventing an answer.
 *
 * **The pairing format is not spelled here.** The computer composes the payload
 * and the phone's application reads it, both through the crate the two share;
 * what crosses this boundary is what a person read off the other screen. A
 * window that assembled `address` and `key` into a payload itself would be a
 * second speller of a format, and the two can disagree.
 */

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Format,
  checkPermissions,
  openAppSettings,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

import { useDevice } from "@/lib/device";
import { said } from "@/lib/refusal";

/** What the phone's application says about the computer it has, if any. */
export interface ChannelStatus {
  readonly paired: boolean;
  /** What it dials. `null` when there is nothing to dial. */
  readonly endpoint: string | null;
  /** Whether it is talking to it *now*, which is a different question. */
  readonly connected: boolean;
}

/** What a window with no computer can do about it. */
export interface Pairing {
  /** Still finding out. The window says it is starting while this holds. */
  readonly isAsking: boolean;
  /** Whether this window must be paired before it can show anything. */
  readonly needed: boolean;
  readonly isBusy: boolean;
  /** The refusal, in the words of whoever refused. */
  readonly failure: string | null;
  /** The camera was refused for good, so the way back is through Settings. */
  readonly cameraRefused: boolean;
  /** Read the code the computer is showing. */
  readonly readCode: () => Promise<void>;
  /** Put in the two strings under the code instead. */
  readonly pairByHand: (endpoint: string, secret: string) => Promise<void>;
  /** Take the person to the switch they turned off. */
  readonly openCameraSettings: () => Promise<void>;
}

export function usePairing(): Pairing {
  const isPhone = useDevice() === "phone";
  const [status, setStatus] = useState<ChannelStatus | null>(null);
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [cameraRefused, setCameraRefused] = useState(false);

  useEffect(() => {
    if (!isPhone) return;

    let listening = true;
    void invoke<ChannelStatus>("channel_status", {}).then(
      (answer) => {
        if (listening) setStatus(answer);
      },
      (refused: unknown) => {
        // A phone that cannot even be asked is a phone with no computer: the
        // screen that follows is the one that can do something about it.
        if (!listening) return;
        setStatus({ paired: false, endpoint: null, connected: false });
        setFailure(said(refused));
      },
    );
    return () => {
      listening = false;
    };
  }, [isPhone]);

  const readCode = useCallback(async () => {
    setBusy(true);
    setFailure(null);
    try {
      let permission = await checkPermissions();
      if (permission !== "granted" && permission !== "denied") {
        permission = await requestPermissions();
      }
      if (permission !== "granted") {
        setCameraRefused(true);
        return;
      }

      // The camera opens as its own view rather than behind the window. The
      // other way round asks the whole interface to be transparent so that
      // what is underneath can be seen, and this window is opaque by rule —
      // the glass here is a Mac window's edge, never a surface.
      const seen = await scan({ formats: [Format.QRCode] });
      setStatus(
        await invoke<ChannelStatus>("channel_pair", { payload: seen.content }),
      );
    } catch (refused: unknown) {
      setFailure(said(refused));
    } finally {
      setBusy(false);
    }
  }, []);

  const pairByHand = useCallback(async (endpoint: string, secret: string) => {
    setBusy(true);
    setFailure(null);
    try {
      setStatus(
        await invoke<ChannelStatus>("channel_pair_by_hand", {
          endpoint,
          secret,
        }),
      );
    } catch (refused: unknown) {
      setFailure(said(refused));
    } finally {
      setBusy(false);
    }
  }, []);

  const openCameraSettings = useCallback(async () => {
    await openAppSettings();
  }, []);

  return {
    isAsking: isPhone && status === null,
    needed: isPhone && status !== null && !status.paired,
    isBusy,
    failure,
    cameraRefused,
    readCode,
    pairByHand,
    openCameraSettings,
  };
}
