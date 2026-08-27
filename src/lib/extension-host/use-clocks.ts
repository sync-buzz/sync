/**
 * Which extensions' clocks are stopped in this project, and stopping one.
 *
 * The host keeps the answer, because the clock is the host's: it runs in the
 * process that survives every window being closed, and a switch a window
 * remembered would be forgotten by the only thing that reads it. So this is a
 * view of a file, in the shape `use-session.ts` established — the state is in
 * Rust and addressed by project, the hook is a view of it, and unmounting stops
 * the watching and nothing else.
 *
 * **The exceptions, not the rule.** An extension that declares a schedule runs
 * unless somebody switched it off here, so what is held is the ids that were
 * switched off. A list of what is *on* would be the second consent this design
 * refuses, written into a file: a project would stop ticking
 * the day something failed to write a `true` nobody had asked for.
 *
 * It is read once when the area is first shown rather than watched. Nothing but
 * this window changes it — the clock reads it and writes only its own stamps,
 * which this does not show — so there is nothing to hear about.
 */

import { useCallback, useEffect, useState } from "react";

import { switchClock, switchedOffClocks } from "@/lib/extension-host/client";

export interface Clocks {
  /** Whether this extension's schedule runs in this project. */
  readonly isOn: (id: string) => boolean;
  readonly switchTo: (id: string, on: boolean) => void;
  /** True while a switch is being written, so it cannot be asked twice. */
  readonly isBusy: boolean;
}

export function useClocks(project: string): Clocks {
  const [off, setOff] = useState<readonly string[]>([]);
  const [isBusy, setIsBusy] = useState(false);

  useEffect(() => {
    let current = true;
    void switchedOffClocks(project)
      .then((ids) => {
        if (current) setOff(ids);
      })
      // Quiet, and the cost is a switch drawn as On that is Off. The file is
      // unreadable only when the configuration directory is, which is a state
      // in which nothing else about this window works either.
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [project]);

  const switchTo = useCallback(
    (id: string, on: boolean) => {
      setIsBusy(true);
      // The control moves when the write lands, not when it is asked for. A
      // switch that moved first and moved back would be the window telling
      // somebody something happened and then taking it away — and this write is
      // one small file, not a network.
      void switchClock(project, id, on)
        .then(() =>
          setOff((was) =>
            on ? was.filter((each) => each !== id) : [...was, id],
          ),
        )
        .catch(() => {})
        .finally(() => setIsBusy(false));
    },
    [project],
  );

  return {
    isOn: useCallback((id: string) => !off.includes(id), [off]),
    switchTo,
    isBusy,
  };
}
