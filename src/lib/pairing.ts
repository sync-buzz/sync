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
import { listen } from "@tauri-apps/api/event";
import { command } from "@/lib/command";
import {
  Format,
  checkPermissions,
  openAppSettings,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

import { useDevice } from "@/lib/device";
import { said } from "@/lib/refusal";

/**
 * The phone's application has moved between the states below.
 *
 * It carries nothing: the state arrives from one place, which is the command,
 * and an event with a payload beside it would leave the window deciding which
 * of the two was newer every time an answer and an event crossed.
 */
const REACHED = "channel://reach";

/**
 * How long a connection may be down before the window says so.
 *
 * **The wait exists because the common drop is not a fault.** iOS suspends this
 * application seconds after it goes into the background and takes the
 * connection with it, so every time somebody puts their phone down and picks it
 * up again the phone is briefly not talking to anything — and the keeper has it
 * back before a person could read a word about it. Said at once, what that
 * would look like is a screen about a lost computer flashing over the work of
 * anybody who so much as glanced at a notification.
 *
 * Long enough for a dial that is going to succeed, and short enough that
 * somebody whose computer has actually gone is not left tapping at a window
 * that answers nothing.
 */
const GRACE_MS = 1_500;

/** Where the phone stands with the computer it has. */
export type Reach =
  /** No computer to dial. The pairing screen is what answers this. */
  | "unpaired"
  /** Dialling now. Nothing has failed; nothing is established either. */
  | "reaching"
  /** There is a connection, and everything the window asks goes out on it. */
  | "talking"
  /** The last dial failed. The phone is still trying; `trouble` says why. */
  | "away";

/** What the phone's application says about the computer it has, if any. */
export interface ChannelStatus {
  readonly paired: boolean;
  /** What it dials. `null` when there is nothing to dial. */
  readonly endpoint: string | null;
  /** Whether it is talking to it *now*, which is a different question. */
  readonly reach: Reach;
  /**
   * Why the last dial failed, in the words of whoever refused it.
   *
   * Only ever set with `reach: "away"`, and never written here: it is the
   * door's refusal, or the address that names nothing reachable, or the two
   * builds that cannot speak to each other. A sentence composed in this file
   * would be the window claiming to know something about a failure it did not
   * make.
   */
  readonly trouble: string | null;
}

/**
 * What the window says about a connection that is not there.
 *
 * Two states and not one, because they ask opposite things of the person
 * reading them: *reaching* is wait, and *away* is this will not fix itself,
 * look at the computer. They were one — a phone that was not connected — and
 * a person meeting a laptop that had gone to sleep read the same silence as
 * somebody whose phone was three seconds into a perfectly ordinary dial.
 */
export interface Standing {
  /** Whether a dial is in flight, as against the last one having failed. */
  readonly reaching: boolean;
  /** Whoever refused, in their own words. `null` while nothing has refused. */
  readonly trouble: string | null;
}

/** What a window with no computer can do about it, and what it has when it has one. */
export interface Pairing {
  /** Still finding out. The window says it is starting while this holds. */
  readonly isAsking: boolean;
  /** Whether this window must be paired before it can show anything. */
  readonly needed: boolean;
  /**
   * The computer this phone has, and `null` before the application has said.
   *
   * The pairing screen needs none of this — it exists because there is nothing
   * here. It is carried for the screen on the other side of that: a phone that
   * is paired is a phone whose owner can ask *to what*, and answering that
   * anywhere else would mean a second reader of the same channel.
   */
  readonly computer: ChannelStatus | null;
  /**
   * Whether the window has something to say about the connection, and what.
   *
   * `null` is the ordinary case and means *nothing to report* — either the
   * phone is talking to its computer, or it stopped so recently that saying so
   * would be noise. What is here when it is not `null` is exactly what the
   * launch screen draws, which is why it is one member and not three: a screen
   * cannot be half way between *still trying* and *this is not working*.
   */
  readonly standing: Standing | null;
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
  /**
   * Ask the application again.
   *
   * Whether the computer is *answering* is not settled once: a laptop is shut,
   * a network changes, and the phone goes on holding the same key. So the one
   * screen that reports it asks when it is opened rather than showing what was
   * true at launch.
   */
  readonly refresh: () => Promise<void>;
  /**
   * Dial now rather than at the end of the wait the phone is in.
   *
   * Not a request for a connection — the phone is already working on one, and
   * has been since the last one dropped. What this shortens is the wait, which
   * grows to half a minute for a phone nobody is holding. Somebody who has just
   * woken their computer up should not have to sit through the rest of it.
   */
  readonly reachNow: () => Promise<void>;
  /**
   * Forget the computer: the connection and the key with it.
   *
   * The window ends up where a phone with no computer belongs — the pairing
   * screen — because that is what `needed` says once the answer comes back
   * unpaired. Nothing here navigates.
   */
  readonly forget: () => Promise<void>;
}

export function usePairing(): Pairing {
  const isPhone = useDevice() === "phone";
  const [status, setStatus] = useState<ChannelStatus | null>(null);
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [cameraRefused, setCameraRefused] = useState(false);

  const refresh = useCallback(async () => {
    if (!isPhone) return;
    try {
      setStatus(await command<ChannelStatus>("channel_status", {}));
    } catch (refused: unknown) {
      setFailure(said(refused));
    }
  }, [isPhone]);

  useEffect(() => {
    if (!isPhone) return;

    let listening = true;
    void command<ChannelStatus>("channel_status", {}).then(
      (answer) => {
        if (listening) setStatus(answer);
      },
      (refused: unknown) => {
        // A phone that cannot even be asked is a phone with no computer: the
        // screen that follows is the one that can do something about it.
        if (!listening) return;
        setStatus({
          paired: false,
          endpoint: null,
          reach: "unpaired",
          trouble: null,
        });
        setFailure(said(refused));
      },
    );
    return () => {
      listening = false;
    };
  }, [isPhone]);

  /**
   * The phone dials on its own now, so the window is told rather than asking.
   *
   * Without this the state would only ever be as new as the last thing
   * somebody pressed — and the whole point of a phone that reconnects by
   * itself is that nobody pressed anything. What arrives is a nudge with
   * nothing in it; the answer comes from the same command every other reader
   * here uses, which is what keeps this from being a second source of truth.
   */
  useEffect(() => {
    if (!isPhone) return;

    let listening = true;
    let stop: (() => void) | undefined;
    void listen(REACHED, () => void refresh()).then(
      (unlisten) => {
        if (listening) stop = unlisten;
        else unlisten();
      },
      () => {
        // A window that could not be subscribed still works: every screen
        // asks when it is opened, and the phone keeps dialling either way.
      },
    );
    return () => {
      listening = false;
      stop?.();
    };
  }, [isPhone, refresh]);

  /**
   * Cut the phone's wait short when the system hands this window back.
   *
   * iOS suspends the application in the background and the connection goes
   * with it, so *coming back* is exactly the moment a dial is worth making —
   * and it is a moment nothing else here can see. Without it somebody who
   * glanced away for a minute would return to a phone part-way through a
   * half-minute rest, with nothing to do but wait it out.
   */
  useEffect(() => {
    if (!isPhone) return;

    const woken = () => {
      if (document.visibilityState === "visible") {
        void command("channel_reach_now", {}).then(undefined, () => {
          // Nothing to say: the phone is dialling on its own schedule, and
          // this was only ever an attempt to bring the next one forward.
        });
      }
    };
    document.addEventListener("visibilitychange", woken);
    return () => document.removeEventListener("visibilitychange", woken);
  }, [isPhone]);

  const forget = useCallback(async () => {
    setBusy(true);
    setFailure(null);
    try {
      setStatus(await command<ChannelStatus>("channel_forget", {}));
    } catch (refused: unknown) {
      setFailure(said(refused));
    } finally {
      setBusy(false);
    }
  }, []);

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
        await command<ChannelStatus>("channel_pair", { payload: seen.content }),
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
        await command<ChannelStatus>("channel_pair_by_hand", {
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

  const reachNow = useCallback(async () => {
    if (!isPhone) return;
    setFailure(null);
    try {
      setStatus(await command<ChannelStatus>("channel_reach_now", {}));
    } catch (refused: unknown) {
      setFailure(said(refused));
    }
  }, [isPhone]);

  const reach = status?.reach ?? "unpaired";

  /**
   * Whether this window has ever had a connection under it.
   *
   * It decides whether the wait below applies, and the two moments it tells
   * apart are genuinely different. Before: the window is empty, the launch
   * screen is already standing, and holding it back for a second and a half
   * would show a list of nothing under somebody's first dial. After: there is
   * work on the screen, and covering it for a drop that mends itself is the
   * thing the wait exists to prevent.
   *
   * Read during the render that has the answer rather than in an effect after
   * it, the way this window reads anything that has to be true before a frame
   * is drawn.
   */
  const [wasTalking, setWasTalking] = useState(false);
  if (reach === "talking" && !wasTalking) setWasTalking(true);

  /**
   * This phone has a computer and is not talking to it.
   *
   * Both halves are load-bearing. A phone with no computer is not a phone with
   * a connection problem — it is the pairing screen's whole subject — and a
   * code that was just refused leaves a reason standing for a moment against a
   * computer this phone never got. Without `paired` here, that moment is the
   * launch screen raised over the one screen that could have fixed it.
   */
  const down =
    isPhone &&
    status?.paired === true &&
    (reach === "reaching" || reach === "away");

  // Whether the wait above has run out on the drop this phone is *in*. Put
  // back during the render that has the connection again, rather than from
  // the effect: a state set from an effect body is a render that happened for
  // no reason a person can see, and this one has a reason nobody can miss.
  const [waited, setWaited] = useState(false);
  if (!down && waited) setWaited(false);

  // The only thing here that genuinely has to be an effect, because the only
  // thing it is synchronising with is a clock. It is left alone while the drop
  // goes on — a dial that fails and is made again is one outage, and a timer
  // restarted on each attempt would be a wait that never runs out.
  useEffect(() => {
    if (!down || !wasTalking || waited) return;
    const waiting = setTimeout(() => setWaited(true), GRACE_MS);
    return () => clearTimeout(waiting);
  }, [down, wasTalking, waited]);

  const stalled = down && (!wasTalking || waited);

  return {
    isAsking: isPhone && status === null,
    needed: isPhone && status !== null && !status.paired,
    computer: status,
    // The state and the wait, folded into the one question the window asks:
    // is there something to say, and what. `reaching` rather than `!== "away"`
    // so that the screen reads as working while a dial is in flight, whichever
    // dial it is — the second attempt after a refusal is still an attempt.
    standing:
      stalled && status !== null
        ? { reaching: reach === "reaching", trouble: status.trouble }
        : null,
    isBusy,
    failure,
    cameraRefused,
    readCode,
    pairByHand,
    openCameraSettings,
    refresh,
    reachNow,
    forget,
  };
}
