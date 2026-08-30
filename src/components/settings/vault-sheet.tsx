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
 * Putting a secret in, and replacing the one that is there.
 *
 * A sheet rather than a form under the list, for the reason a folder is named
 * in one: three fields standing open in a settings pane are a page of a website
 * in a window, and the pane's job is to show what is held. The gesture that
 * adds to a list belongs beside the list; what it *asks* belongs in a sheet.
 *
 * One sheet for both, because it is the same question. What differs is how much
 * of it has already been answered: a replacement arrived from a row, so the row
 * has already said whose entry it is and what it is called, and asking again
 * would be the window forgetting what somebody just pointed at. It states them
 * and asks for the value alone.
 */
export function VaultSheet({
  open,
  onOpenChange,
  replacing,
  owners,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The entry whose value is being replaced, or `null` when storing a new one. */
  replacing: VaultEntry | null;
  /**
   * The packages that already own something here.
   *
   * Offered rather than left blank, because a package named a second time and
   * spelled differently is a second entry that nothing will ever read. It is
   * not the list of what is installed on this Mac: an extension belongs to the
   * project that declares it, so this window has no business listing them —
   * and a token may quite reasonably be stored before its package exists.
   */
  owners: readonly string[];
  /** Answers when the keychain has it, or throws what refused. */
  onSubmit: (owner: string, name: string, secret: string) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="vault-sheet-lead">
        <SheetHeader>
          <SheetTitle>
            {replacing ? "Replace secret" : "Store a secret"}
          </SheetTitle>
        </SheetHeader>
        {/* Mounted only while it is open, so a value typed and abandoned does
            not sit in this component waiting for the next visit. */}
        {open ? (
          <SecretForm
            replacing={replacing}
            owners={owners}
            onSubmit={onSubmit}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function SecretForm({
  replacing,
  owners,
  onSubmit,
  onDone,
}: {
  replacing: VaultEntry | null;
  owners: readonly string[];
  onSubmit: (owner: string, name: string, secret: string) => Promise<void>;
  onDone: () => void;
}) {
  const [owner, setOwner] = useState(replacing?.owner ?? "");
  const [name, setName] = useState(replacing?.name ?? "");
  const [secret, setSecret] = useState("");
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const trimmed = { owner: owner.trim(), name: name.trim() };
  // The separator is what lets an owner be read back off an entry, so a name
  // carrying one is refused in Rust. Saying so here means the refusal arrives
  // while the caret is still in the field rather than after the round trip.
  const ambiguous = trimmed.owner.includes("/");
  const canSubmit =
    trimmed.owner !== "" &&
    trimmed.name !== "" &&
    secret !== "" &&
    !ambiguous &&
    !isBusy;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    try {
      await onSubmit(trimmed.owner, trimmed.name, secret);
      // Gone from this component the moment the keychain has it.
      setSecret("");
      onDone();
    } catch (refused) {
      setFailure(refused instanceof Error ? refused.message : String(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-3">
        <SheetDescription id="vault-sheet-lead">
          {replacing
            ? `Replaces what ${replacing.owner} keeps under ${replacing.name}. The value that is there cannot be read back, here or anywhere, so nothing compares them.`
            : "It goes into this Mac's own keychain under Sync's name. It is never shown here again, and no window is ever handed it."}
        </SheetDescription>

        {replacing ? null : (
          <>
            <Field label="Package" htmlFor="vault-owner">
              <input
                id="vault-owner"
                autoFocus
                list="vault-owners"
                value={owner}
                onChange={(event) => {
                  setOwner(event.target.value);
                  setFailure(null);
                }}
                placeholder="publisher.package"
                className={FIELD}
              />
              <datalist id="vault-owners">
                {owners.map((known) => (
                  <option key={known} value={known} />
                ))}
              </datalist>
              {ambiguous ? (
                <p className="text-xs text-fg-tertiary">
                  No slash in a package name — it is what separates the package
                  from what it calls the secret.
                </p>
              ) : null}
            </Field>

            <Field label="Name" htmlFor="vault-name">
              <input
                id="vault-name"
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                  setFailure(null);
                }}
                placeholder="api-token"
                className={FIELD}
              />
              <p className="text-xs text-fg-tertiary">
                What the package asks for it by. It is in the package&apos;s own
                documentation, and typing a different one stores something
                nothing will read.
              </p>
            </Field>
          </>
        )}

        <Field label="Secret" htmlFor="vault-secret">
          <input
            id="vault-secret"
            type="password"
            autoFocus={replacing !== null}
            value={secret}
            onChange={(event) => {
              setSecret(event.target.value);
              setFailure(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submit();
            }}
            className={FIELD}
          />
        </Field>

        {failure ? <p className="text-xs text-warning">{failure}</p> : null}
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          onClick={() => void submit()}
          disabled={!canSubmit}
          className="min-w-28"
        >
          {replacing
            ? isBusy
              ? "Replacing…"
              : "Replace secret"
            : isBusy
              ? "Storing…"
              : "Store secret"}
        </Button>
      </SheetFooter>
    </>
  );
}

/** One labelled question, at the shape every sheet in this window asks one. */
function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <label
        htmlFor={htmlFor}
        className="text-sm font-medium text-fg-secondary"
      >
        {label}
      </label>
      {children}
    </div>
  );
}

const FIELD =
  "h-(--control-height-lg) w-full rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-base text-fg placeholder:text-fg-tertiary";
