"use client";

import { useState } from "react";
import { Check, Copy } from "lucide-react";

import { Button } from "@/components/ui/button";
import { PairingCode } from "@/components/settings/pairing-code";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import { type PairedDevice } from "@/lib/settings/remote";
import { said } from "@/lib/refusal";

/**
 * Pairing a device, in the two steps it actually has.
 *
 * A name goes in, and a code comes back **once**. The sheet stays open on the
 * second step until the person dismisses it, because there is nothing to come
 * back to: this Mac keeps the key in its keychain and has no way to show it
 * again, and a sheet that closed on its own would take the one legible copy
 * with it.
 *
 * **The code is the way to pair; the two strings under it are the fallback.**
 * A key is sixty-four characters of hex — copying it between two machines a
 * person is holding is the awkward part of this, and a camera does it in a
 * second. The strings stay for the case the camera cannot: a screen too far
 * away, a device with no camera, a person who would rather paste.
 *
 * A sheet rather than a pane, for the reason the vault's is: what a gesture
 * beside a list *asks* belongs in a sheet, and a form standing open in the
 * column would make the section a page rather than a list of what is held.
 */
export function RemotePairSheet({
  open,
  onOpenChange,
  onPair,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPair: (name: string) => Promise<PairedDevice>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="remote-pair-lead">
        <SheetHeader>
          <SheetTitle>Pair a device</SheetTitle>
        </SheetHeader>
        {open ? (
          <Pairing onPair={onPair} onDone={() => onOpenChange(false)} />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function Pairing({
  onPair,
  onDone,
}: {
  onPair: (name: string) => Promise<PairedDevice>;
  onDone: () => void;
}) {
  const [name, setName] = useState("");
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [paired, setPaired] = useState<PairedDevice | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  const pair = async () => {
    setBusy(true);
    setFailure(null);
    try {
      setPaired(await onPair(name));
    } catch (refused) {
      setFailure(said(refused));
    } finally {
      setBusy(false);
    }
  };

  const copy = (what: string, value: string) => {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(what);
      window.setTimeout(() => setCopied(null), 1200);
    });
  };

  if (paired) {
    return (
      <>
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
          <SheetDescription id="remote-pair-lead">
            <span className="font-medium text-fg">{paired.device.name}</span> is
            paired. Scan this with it now.
          </SheetDescription>

          {paired.pairing ? <PairingCode payload={paired.pairing} /> : null}

          <p className="text-sm text-fg-secondary">
            Or put these in by hand, if the camera cannot reach the screen.
          </p>

          <Copyable
            label="This Mac"
            value={paired.endpoint ?? "—"}
            copied={copied === "endpoint"}
            onCopy={() =>
              paired.endpoint && copy("endpoint", paired.endpoint)
            }
          />
          <Copyable
            label="Key"
            value={paired.secret}
            copied={copied === "secret"}
            onCopy={() => copy("secret", paired.secret)}
          />

          <p className="text-sm text-fg-secondary">
            The key is shown here and nowhere else, ever again. It is in this
            Mac&apos;s keychain now, and Sync has no command that reads one back
            — a device that loses it is paired again rather than reminded.
          </p>

          {paired.endpoint === null ? (
            <p className="text-xs text-warning">
              This Mac has no address yet: remote access is on but its door has
              not opened. The device is paired and will be admitted once it has.
            </p>
          ) : null}
        </div>

        <SheetFooter>
          <div className="min-w-0 flex-1" />
          <Button variant="outline" onClick={onDone} className="min-w-28">
            Done
          </Button>
        </SheetFooter>
      </>
    );
  }

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
        <SheetDescription id="remote-pair-lead">
          A name for the device, so that it can be told from the others when one
          of them is to be revoked.
        </SheetDescription>

        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-fg-secondary">Name</span>
          <input
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && name.trim() !== "") void pair();
            }}
            placeholder="My phone"
            className="h-7 rounded-(--radius-control) border border-separator-strong bg-panel px-2 text-sm text-fg outline-none focus-visible:border-accent"
          />
        </label>

        <p className="text-sm text-fg-secondary">
          The device gets a key of its own. Revoking it later stops that one
          device and touches nothing else on this Mac.
        </p>

        {failure ? <p className="text-xs text-warning">{failure}</p> : null}
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          onClick={() => void pair()}
          disabled={isBusy || name.trim() === ""}
          className="min-w-28"
        >
          {isBusy ? "Pairing…" : "Pair"}
        </Button>
      </SheetFooter>
    </>
  );
}

/** One value long enough that nobody is going to retype it. */
function Copyable({
  label,
  value,
  copied,
  onCopy,
}: {
  label: string;
  value: string;
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs text-fg-secondary">{label}</span>
      <div className="flex items-start gap-1.5">
        <code
          className={cn(
            "min-w-0 flex-1 rounded-(--radius-control) border border-separator-strong bg-panel px-2 py-1.5",
            "font-mono text-xs break-all text-fg",
          )}
        >
          {value}
        </code>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Copy the ${label.toLowerCase()}`}
          onClick={onCopy}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      </div>
    </div>
  );
}
