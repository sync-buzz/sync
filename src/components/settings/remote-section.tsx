"use client";

import { useCallback, useEffect, useState } from "react";
import { Check, Copy, Minus, Plus, X } from "lucide-react";

import { PanelFooter } from "@/components/shell/panel";
import { RemotePairSheet } from "@/components/settings/remote-pair";
import { RemoteRemovalSheet } from "@/components/settings/remote-removal";
import { Button } from "@/components/ui/button";
import { showNativeContextMenu } from "@/lib/native-menu";
import { cn } from "@/lib/utils";
import {
  enableRemoteAccess,
  loadRemoteStatus,
  paired,
  pairRemoteDevice,
  revokeRemoteDevice,
  when,
  type RemoteDevice,
  type RemoteStatus,
} from "@/lib/settings/remote";

/**
 * Which devices may talk to this Mac, and how to stop one.
 *
 * A row is a device's name, the fingerprint it is revoked by, when it was
 * paired and when it was last here. Not its key: this section is never handed
 * one, so it could not show one if it decided to, and the only moment a key is
 * legible is the sheet that mints it.
 *
 * The two times are written differently on purpose. Paired is a date, because
 * it is looked up — *is this the one I set up at the office*. Last seen is *3
 * days ago*, because what is being asked of it is whether the device is still
 * in use, and a date makes somebody do the arithmetic to find out.
 *
 * **It is a list with a bar under it**, which is the shell's rule for a list
 * you add to and the one macOS keeps under every list in Settings. What the two
 * gestures ask is a sheet each, because both are irreversible in different
 * directions — one shows a key that will never be shown again, the other takes
 * one away for good.
 *
 * **Off is the state an installation arrives in, and turning it on restarts the
 * engine.** That is said next to the switch rather than discovered: what this
 * Mac is called on the network is settled when its door opens, so the process
 * has to come back to be given a name. It is the same sentence the port field
 * in Server carries, for the same mechanism.
 *
 * The address is shown whole and copied rather than typed. It is a public key
 * in text, nobody is going to retype it correctly, and it is not a secret —
 * knowing it buys nothing without a paired key.
 */
export function RemoteSection() {
  // `null` until the engine has answered. An installation with no devices and
  // one that has not been read are different claims.
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [selected, setSelected] = useState<string | null>(null);
  const [pairing, setPairing] = useState(false);
  const [revoking, setRevoking] = useState<RemoteDevice | null>(null);

  // A counter rather than a boolean, for the reason the vault's list uses one:
  // a read that started before a write finished would put its rows back.
  const [reading, setReading] = useState(0);
  const refresh = useCallback(() => setReading((count) => count + 1), []);

  const read = useCallback((answer: RemoteStatus) => {
    setStatus(answer);
    setFailure(answer.failure);
  }, []);

  useEffect(() => {
    let live = true;
    void loadRemoteStatus().then(
      (answer) => live && read(answer),
      (error: unknown) => live && setFailure(explain(error)),
    );
    return () => {
      live = false;
    };
  }, [reading, read]);

  const devices = status?.devices ?? [];
  const chosen =
    devices.find((device) => device.fingerprint === selected) ?? null;

  const [copied, setCopied] = useState(false);
  const copy = (value: string) => {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };

  const switchTo = (enabled: boolean) => {
    setBusy(true);
    setFailure(null);
    void enableRemoteAccess(enabled)
      .then(read, (error: unknown) => setFailure(explain(error)))
      .finally(() => setBusy(false));
  };

  const revoke = useCallback(
    async (device: RemoteDevice) => {
      const answer = await revokeRemoteDevice(device.fingerprint);
      setSelected(null);
      read(answer);
    },
    [read],
  );

  const menuFor = (device: RemoteDevice) => [
    { label: "Revoke Device", onSelect: () => setRevoking(device) },
  ];

  const on = status?.enabled === true;

  return (
    <section className="flex flex-col gap-5">
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-3">
          <span className="text-sm text-fg">
            {on ? "This Mac is reachable" : "This Mac is not reachable"}
          </span>
          <Button
            variant="outline"
            disabled={status === null || busy}
            onClick={() => switchTo(!on)}
          >
            {busy ? "Restarting…" : on ? "Turn off" : "Turn on"}
          </Button>
        </div>
        <p className="max-w-[64ch] text-sm text-fg-tertiary">
          Turning this on or off restarts the memory engine: what this Mac is
          called on the network is settled when its door opens. A paired device
          reaches it from anywhere, not only from this network.
        </p>
      </div>

      {on ? (
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-fg-secondary">This Mac</span>
          <div className="flex items-start gap-1.5">
            <code
              className={cn(
                "min-w-0 flex-1 rounded-(--radius-control) border border-separator-strong bg-panel px-2 py-1.5",
                "font-mono text-xs break-all",
                status?.endpoint ? "text-fg" : "text-fg-tertiary",
              )}
            >
              {status?.endpoint ??
                "No address yet — the door is opening, or it did not open."}
            </code>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Copy this Mac's address"
              disabled={!status?.endpoint}
              onClick={() => status?.endpoint && copy(status.endpoint)}
            >
              {copied ? <Check /> : <Copy />}
            </Button>
          </div>
        </div>
      ) : null}

      <div className="overflow-hidden rounded-(--radius-control) border border-separator-strong bg-panel">
        {status === null ? (
          <Nothing>
            The engine has not answered, so which devices are admitted is
            unknown. That is not the same as admitting none.
          </Nothing>
        ) : devices.length === 0 ? (
          <Nothing>
            No devices paired. Until one is, nothing off this Mac can reach it —
            whether remote access is on or off.
          </Nothing>
        ) : (
          <ul className="max-h-72 overflow-y-auto py-1">
            {devices.map((device) => {
              const isSelected = device.fingerprint === selected;
              return (
                <li key={device.fingerprint}>
                  {/* Selection is a surface shift and a weight change, as it is
                      everywhere else: still the obvious row in greyscale. */}
                  <button
                    type="button"
                    aria-pressed={isSelected}
                    onClick={() => setSelected(device.fingerprint)}
                    onContextMenu={(event) => {
                      setSelected(device.fingerprint);
                      showNativeContextMenu(event, menuFor(device));
                    }}
                    className={cn(
                      "flex w-full items-center gap-3 px-3 py-1.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell",
                      isSelected
                        ? "bg-selected font-medium text-fg"
                        : "text-fg hover:bg-hover",
                    )}
                  >
                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-sm">{device.name}</span>
                      <span className="truncate text-xs font-normal text-fg-tertiary">
                        <span className="font-mono">{device.fingerprint}</span>
                        {" · paired "}
                        {paired(device.pairedAt)}
                      </span>
                    </span>
                    <span className="w-24 shrink-0 text-right text-xs font-normal text-fg-tertiary">
                      {when(device.lastSeen)}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}

        <PanelFooter>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Pair a device"
            onClick={() => setPairing(true)}
          >
            <Plus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Revoke the selected device"
            disabled={chosen === null}
            onClick={() => chosen && setRevoking(chosen)}
          >
            <Minus />
          </Button>
        </PanelFooter>
      </div>

      {/* A command that did not happen says so, in the words it arrived in, and
          waits to be dismissed rather than fading on its own. */}
      {failure !== null && (
        <div className="flex max-w-[64ch] items-start gap-2 rounded-(--radius-control) border border-separator-strong bg-panel p-2.5">
          <p className="min-w-0 flex-1 text-sm text-warning">{failure}</p>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss"
            onClick={() => setFailure(null)}
          >
            <X />
          </Button>
        </div>
      )}

      <RemotePairSheet
        open={pairing}
        onOpenChange={(open) => {
          if (!open) {
            setPairing(false);
            refresh();
          }
        }}
        onPair={pairRemoteDevice}
      />

      <RemoteRemovalSheet
        open={revoking !== null}
        onOpenChange={(open) => {
          if (!open) setRevoking(null);
        }}
        device={revoking}
        onRevoke={revoke}
      />
    </section>
  );
}

/** What the box says instead of simulating rows it does not have. */
function Nothing({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-3 py-6 text-center text-sm text-fg-tertiary">{children}</p>
  );
}

/**
 * A refusal in the words it arrived in.
 *
 * The commands answer with a sentence written for a person — a keychain that
 * would not open, an engine that did not come back — and a sentence of our own
 * would drop exactly the part somebody acts on.
 */
function explain(error: unknown): string {
  if (typeof error === "string" && error.trim() !== "") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  return "Remote access could not be read.";
}
