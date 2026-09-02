"use client";

import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { type RemoteDevice } from "@/lib/settings/remote";
import { said } from "@/lib/refusal";

/**
 * Taking one device's access away.
 *
 * A sheet, because it cannot be undone: the key leaves this Mac's keychain and
 * there is nothing to put back — pairing the device again mints a different
 * one.
 *
 * The sentence that earns the sheet is the second one, and it is the opposite
 * of the vault's. Forgetting a secret there revokes nothing, and somebody who
 * came because a token leaked has done nothing about the leak. Here they have:
 * the device stops being admitted at its next attempt, and nothing else on this
 * Mac is touched — the other devices keep working, and no agent is
 * reconfigured.
 */
export function RemoteRemovalSheet({
  open,
  onOpenChange,
  device,
  onRevoke,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The device about to go, or `null` while the sheet is closed. */
  device: RemoteDevice | null;
  onRevoke: (device: RemoteDevice) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="remote-removal-lead">
        <SheetHeader>
          <SheetTitle>Revoke device</SheetTitle>
        </SheetHeader>
        {open && device ? (
          <Confirmation
            device={device}
            onRevoke={onRevoke}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function Confirmation({
  device,
  onRevoke,
  onDone,
}: {
  device: RemoteDevice;
  onRevoke: (device: RemoteDevice) => Promise<void>;
  onDone: () => void;
}) {
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const revoke = async () => {
    setBusy(true);
    try {
      await onRevoke(device);
      onDone();
    } catch (refused) {
      setFailure(said(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
        <SheetDescription id="remote-removal-lead">
          <span className="font-medium text-fg">{device.name}</span> stops being
          admitted, at its next attempt to connect.
        </SheetDescription>

        <p className="text-sm text-fg-secondary">
          Its key leaves this Mac&apos;s keychain and cannot be put back. Pairing
          the device again gives it a different one.
        </p>

        <p className="text-sm text-fg-secondary">
          Nothing else changes. Every other paired device goes on working, this
          Mac keeps the address it is known by, and no agent has to be connected
          again.
        </p>

        {failure ? <p className="text-xs text-warning">{failure}</p> : null}
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          variant="destructive"
          onClick={() => void revoke()}
          disabled={isBusy}
          className="min-w-28"
        >
          {isBusy ? "Revoking…" : "Revoke device"}
        </Button>
      </SheetFooter>
    </>
  );
}
