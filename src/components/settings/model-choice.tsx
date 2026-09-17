"use client";

import { useCallback, useEffect, useState } from "react";

import { FIELD, messageOf } from "@/components/settings/shared";
import {
  loadModelChoice,
  setModelChoice,
  type ModelChoiceStatus,
  type Provider,
} from "@/lib/settings/model-choice";
import { cn } from "@/lib/utils";

/**
 * Which agent this installation works through, raised on the window's behalf
 * for work nobody is looking at.
 *
 * A provider that cannot be chosen stays in the menu, disabled, with the reason
 * after its name. Left out, somebody who has it installed would read the gap
 * as *Sync has never heard of it* and go looking for the wrong thing.
 */
export function ModelChoice() {
  const [status, setStatus] = useState<ModelChoiceStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const settle = useCallback((answered: ModelChoiceStatus) => {
    setStatus(answered);
  }, []);

  useEffect(() => {
    let live = true;
    void loadModelChoice()
      .then((answered) => {
        if (live) settle(answered);
      })
      .catch((error: unknown) => {
        if (live) setFailure(messageOf(error, "The choice could not be made."));
      });
    return () => {
      live = false;
    };
  }, [settle]);

  const write = useCallback(
    (cloudAgent: string | null) => {
      setBusy(true);
      setFailure(null);
      void setModelChoice({ cloudAgent })
        .then(settle)
        .catch((error: unknown) => setFailure(messageOf(error, "The choice could not be made.")))
        .finally(() => setBusy(false));
    },
    [settle],
  );

  if (status === null) {
    return (
      <p className="text-xs text-fg-tertiary">{failure ?? "Reading where models come from…"}</p>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <Providers
        offered={status.cloud}
        chosen={status.cloudChosen}
        disabled={busy}
        onChoose={(id) => write(id)}
      />

      <p className="max-w-[64ch] text-xs text-fg-tertiary">{said(status, busy)}</p>

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </div>
  );
}

/**
 * Which provider runs the turns, out of the list this build offers.
 *
 * A provider that cannot be chosen stays in the menu, disabled, with the reason
 * after its name. Left out, somebody who has it installed would read the gap
 * as *Sync has never heard of it* and go looking for the wrong thing.
 */
function Providers({
  offered,
  chosen,
  disabled,
  onChoose,
}: {
  offered: readonly Provider[];
  chosen: string | null;
  disabled: boolean;
  onChoose: (id: string) => void;
}) {
  if (offered.length === 0) {
    return <p className="text-xs text-fg-tertiary">No provider this build can raise was found.</p>;
  }

  return (
    <select
      aria-label="Provider"
      disabled={disabled}
      value={chosen ?? ""}
      onChange={(event) => {
        if (event.target.value !== "") onChoose(event.target.value);
      }}
      className={cn(FIELD, "w-full max-w-[42ch] disabled:opacity-50")}
    >
      {chosen === null ? <option value="">No provider chosen</option> : null}
      {offered.map((provider) => (
        <option key={provider.id} value={provider.id} disabled={!provider.eligible}>
          {provider.eligible ? provider.name : `${provider.name} — ${provider.refusal ?? ""}`}
        </option>
      ))}
    </select>
  );
}

/**
 * What state this decision is in, in a sentence.
 *
 * Everything it distinguishes it distinguishes in words. Somebody reading it is
 * deciding whether their next turn runs, and *answering* against *not
 * answering* is exactly the difference a green dot would be carrying alone.
 */
function said(status: ModelChoiceStatus, busy: boolean): string {
  if (busy) return "Writing the choice down…";

  if (status.cloudChosen === null) {
    return "Until a provider is chosen, no screen can be built over its tools.";
  }
  const file = status.cloud.find((provider) => provider.id === status.cloudChosen)?.configuration;
  return file === null || file === undefined
    ? "Turns run wherever that provider's account runs them, and are charged to it."
    : `Turns are charged to that provider's account, and its tools are read from ${file}.`;
}
