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
import type { CatalogueEntry } from "@/lib/extension-host/catalogue";
import type { McpTransportSpec } from "@/lib/extension-host/client";
import { writeSecret } from "@/lib/settings/vault";
import { said } from "@/lib/refusal";

/**
 * The secrets an MCP server asks for, collected before it is added.
 *
 * A server's transport names the secrets it needs — an env var for stdio, a
 * header for HTTP — and the values live in the vault under `mcp-{id}/{secret}`,
 * not in the project record. This sheet is where a person pastes them: it
 * writes each to the keychain, then hands back to the composition to record the
 * declaration. A secret left blank is not written, and the server is still
 * added — it will be skipped at session start until the secret is filled in
 * through Settings.
 */
export function McpCredentialSheet({
  entry,
  open,
  onOpenChange,
  onAdded,
}: {
  entry: CatalogueEntry;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called with the secrets already in the vault; records the declaration. */
  onAdded: () => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="mcp-credential-lead">
        <SheetHeader>
          <SheetTitle>Add {entry.name}</SheetTitle>
        </SheetHeader>
        {open ? (
          <CredentialForm entry={entry} onAdded={onAdded} onDone={() => onOpenChange(false)} />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/** One secret the transport names, flattened to what a field needs. */
interface SecretField {
  readonly name: string;
  readonly secret: string;
  readonly description: string;
}

/** The secrets a transport asks for, regardless of which kind it is. */
function secretsOf(spec: McpTransportSpec): readonly SecretField[] {
  switch (spec.type) {
    case "stdio":
      return (spec.env ?? []).map((e) => ({
        name: e.name,
        secret: e.secret,
        description: e.description,
      }));
    case "http":
    case "sse":
      return (spec.headers ?? []).map((h) => ({
        name: h.name,
        secret: h.secret,
        description: h.description,
      }));
  }
}

function CredentialForm({
  entry,
  onAdded,
  onDone,
}: {
  entry: CatalogueEntry;
  onAdded: () => Promise<void>;
  onDone: () => void;
}) {
  const spec = entry.transport;
  // The catalogue only opens this sheet for an MCP entry, so a transport is
  // present; the fallback keeps the type honest if a non-MCP entry ever reach.
  const fields = spec === null ? [] : secretsOf(spec);
  const [values, setValues] = useState<Record<string, string>>({});
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setFailure(null);
    try {
      // Each secret the person filled goes to the vault under the server's own
      // owner key. A blank field is left unwritten, and the server is added
      // regardless — it will be skipped at session start until the secret is
      // provided through Settings.
      const owner = `mcp-${entry.id}`;
      for (const field of fields) {
        const value = (values[field.secret] ?? "").trim();
        if (value === "") continue;
        await writeSecret(owner, field.secret, value);
      }
      await onAdded();
      onDone();
    } catch (refused) {
      setFailure(said(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-3">
        <SheetDescription id="mcp-credential-lead">
          {fields.length === 0
            ? `Adds ${entry.name} to this project. It asks for no secrets, so there is nothing to enter.`
            : `Adds ${entry.name} to this project. The secrets go to this Mac's keychain and are never shown again.`}
        </SheetDescription>

        {fields.map((field) => (
          <div key={field.secret} className="flex flex-col gap-1.5">
            <label htmlFor={`mcp-${field.secret}`} className="text-sm font-medium text-fg">
              {field.name}
            </label>
            <input
              id={`mcp-${field.secret}`}
              type="password"
              autoComplete="off"
              value={values[field.secret] ?? ""}
              onChange={(event) => {
                setValues((all) => ({ ...all, [field.secret]: event.target.value }));
                setFailure(null);
              }}
              className="rounded-(--radius-control) border border-separator bg-panel px-2.5 py-1.5 text-sm text-fg outline-none focus:border-fg-tertiary"
            />
            {field.description === "" ? null : (
              <p className="text-xs text-fg-tertiary">{field.description}</p>
            )}
          </div>
        ))}

        {failure === null ? null : (
          <p className="font-mono text-xs leading-4 text-danger">{failure}</p>
        )}
      </div>

      <SheetFooter>
        <Button variant="ghost" size="sm" disabled={isBusy} onClick={onDone}>
          Cancel
        </Button>
        <Button size="sm" disabled={isBusy} onClick={() => void submit()}>
          {isBusy ? "Adding…" : "Add"}
        </Button>
      </SheetFooter>
    </>
  );
}
