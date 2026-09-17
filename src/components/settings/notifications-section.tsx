"use client";

import { useCallback, useEffect, useState } from "react";

import {
  chooseNotifications,
  loadNotifications,
  type NotificationSettings,
} from "@/lib/settings/notifications";
import { messageOf, Segment, Setting } from "@/components/settings/shared";

/**
 * When Sync interrupts, and what it will not interrupt for.
 *
 * Three switches, and they are three different interruptions rather than one
 * loudness dial: somebody blocked on you, work you can now read, and something
 * to fix. A person who wants the first and not the second is not asking for a
 * compromise.
 *
 * **There is no switch for "only when I am away", because that is not a
 * preference.** A banner is raised only when no window of Sync is in front —
 * always, and with nothing here to turn it off. Interrupting somebody about a
 * conversation they are already looking at is not a setting anybody would
 * choose; it is the thing this whole feature has to avoid to be worth having.
 *
 * **And no switch for whether a banner is shown at all.** macOS answers that,
 * in its own Notifications settings, and a second answer here would be a
 * checkbox that says On over a system that is refusing. The line at the foot
 * says where the real one is, and claims nothing about what it currently says.
 * Nothing here asks the platform either — `UNUserNotificationCenter` would
 * answer, and the answer changes in another application while this page is
 * open, so a state drawn from it is right until somebody walks over to System
 * Settings and stays wrong afterwards with no event to correct it.
 */
export function NotificationsSection() {
  const [settings, setSettings] = useState<NotificationSettings | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void loadNotifications().then(
      (answer) => {
        if (live) setSettings(answer);
      },
      (error: unknown) => {
        if (live) setFailure(messageOf(error, "Sync could not write the setting down."));
      },
    );
    return () => {
      live = false;
    };
  }, []);

  const choose = useCallback((next: NotificationSettings) => {
    setBusy(true);
    setFailure(null);
    void chooseNotifications(next)
      .then(setSettings, (error: unknown) =>
        setFailure(messageOf(error, "Sync could not write the setting down.")),
      )
      .finally(() => setBusy(false));
  }, []);

  return (
    <section className="flex flex-col gap-5">
      {WHEN.map((occasion) => (
        <Setting key={occasion.id} label={occasion.label} detail={occasion.detail}>
          <div role="radiogroup" aria-label={occasion.label} className="flex gap-1">
            {[
              { label: "Off", wanted: false },
              { label: "On", wanted: true },
            ].map((option) => (
              <Segment
                key={option.label}
                label={option.label}
                isSelected={settings?.[occasion.id] === option.wanted}
                disabled={busy || !settings}
                onSelect={() => settings && choose({ ...settings, [occasion.id]: option.wanted })}
              />
            ))}
          </div>
        </Setting>
      ))}

      <p className="max-w-[64ch] text-xs text-fg-tertiary">
        A banner appears only while no Sync window is in front. Whether it appears at all is
        macOS&apos;s to decide, under Notifications in System Settings.
      </p>

      {failure ? <p className="text-sm text-warning">{failure}</p> : null}
    </section>
  );
}

/**
 * The three, in the order they matter to somebody away from the desk.
 *
 * Waiting first because it is the only one where nothing happens until the
 * person comes back: a finished turn and a fallen-over session are both
 * already over.
 */
const WHEN = [
  {
    id: "asking",
    label: "Waiting for an answer",
    detail:
      "An agent asked permission for something and stopped. Nothing else happens in that conversation until it is answered.",
  },
  {
    id: "finished",
    label: "Finished",
    detail: "A turn ended and there is something to read.",
  },
  {
    id: "failed",
    label: "Stopped",
    detail:
      "An agent could not be raised, or its process died. The reason is in the conversation, not in the banner.",
  },
] as const satisfies readonly {
  id: keyof NotificationSettings;
  label: string;
  detail: string;
}[];
