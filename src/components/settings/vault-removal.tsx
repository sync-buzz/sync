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
import type { VaultEntry } from "@/lib/settings/vault";

/**
 * Taking a secret out, and what taking it out does not do.
 *
 * A sheet, because this is the irreversible one and the shell asks before those
 * rather than after. There is no archive here to lead with instead: a secret is
 * held or it is not, and the value cannot be read back to put it in again —
 * not by this window, which is never handed one, and not by Sync at all once
 * the keychain no longer has it.
 *
 * The sentence that earns the sheet is the second one. Forgetting a token is
 * not revoking it: whatever issued it goes on honouring it, and somebody who
 * came here because a token leaked has done nothing about the leak. Saying so
 * is the difference between a confirmation and a warning.
 */
export function VaultRemovalSheet({
  open,
  onOpenChange,
  entry,
  onForget,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The entry about to go, or `null` while the sheet is closed. */
  entry: VaultEntry | null;
  onForget: (entry: VaultEntry) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="vault-removal-lead">
        <SheetHeader>
          <SheetTitle>Forget secret</SheetTitle>
        </SheetHeader>
        {open && entry ? (
          <Confirmation
            entry={entry}
            onForget={onForget}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function Confirmation({
  entry,
  onForget,
  onDone,
}: {
  entry: VaultEntry;
  onForget: (entry: VaultEntry) => Promise<void>;
  onDone: () => void;
}) {
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const forget = async () => {
    setBusy(true);
    try {
      await onForget(entry);
      onDone();
    } catch (refused) {
      setFailure(refused instanceof Error ? refused.message : String(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
        <SheetDescription id="vault-removal-lead">
          <span className="font-medium text-fg">{entry.name}</span>, kept for{" "}
          <span className="font-mono">{entry.owner}</span>, leaves this
          Mac&apos;s keychain.
        </SheetDescription>

        <p className="text-sm text-fg-secondary">
          It cannot be put back from here. Sync is never handed the value, so
          there is nothing to restore it from — storing it again means having it
          from wherever it came from in the first place.
        </p>

        <p className="text-sm text-fg-secondary">
          Whatever issued it still honours it. This takes the copy off this Mac
          and does not revoke anything, so a secret that got out is still out
          until it is revoked where it was issued.
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
          onClick={() => void forget()}
          disabled={isBusy}
          className="min-w-28"
        >
          {isBusy ? "Forgetting…" : "Forget secret"}
        </Button>
      </SheetFooter>
    </>
  );
}
