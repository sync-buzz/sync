"use client";

import { useCallback, useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

/**
 * What this machine serves, and to whom.
 *
 * Agents no longer start a copy of Sync each: they reach one server, on a port,
 * over every project this installation answers for. That has two consequences a
 * person can see, and this section is both of them — the address an agent is
 * pointed at, and the fact that closing the last window does not stop it.
 *
 * The token is shown rather than hidden behind a reveal. It is already in plain
 * files: Connect writes it into each agent's own configuration, and a secret
 * copied into four files on this disk is not made safer by being asterisks in
 * one window.
 */
interface ServerStatus {
  readonly port: number;
  readonly token: string;
  readonly url: string;
  readonly running: boolean;
  readonly failure: string | null;
}

export function ServerSection() {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [port, setPort] = useState("");
  const [atLogin, setAtLogin] = useState<boolean | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const read = useCallback((answer: ServerStatus) => {
    setStatus(answer);
    setPort(String(answer.port));
    setFailure(answer.failure);
  }, []);

  useEffect(() => {
    void invoke<ServerStatus>("server_status").then(read, (error: unknown) =>
      setFailure(messageOf(error)),
    );
    void isEnabled().then(setAtLogin, () => setAtLogin(null));
  }, [read]);

  const run = useCallback(
    (command: string, args?: Record<string, unknown>) => {
      setBusy(true);
      setFailure(null);
      void invoke<ServerStatus>(command, args)
        .then(read, (error: unknown) => setFailure(messageOf(error)))
        .finally(() => setBusy(false));
    },
    [read],
  );

  const copy = useCallback((what: string, value: string) => {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(what);
      setTimeout(() => setCopied((held) => (held === what ? null : held)), 1200);
    });
  }, []);

  const portChanged = status !== null && port !== String(status.port);

  return (
    <section className="flex flex-col gap-5">
      <Setting
        label="Address"
        detail={
          status?.running
            ? "Sync keeps serving while its windows are closed. It stops when you quit it from the menu bar."
            : "Not running. Agents connected to Sync cannot reach any project until it is."
        }
      >
        <div className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              status?.running ? "bg-success" : "bg-warning",
            )}
          />
          <code className="font-mono text-sm text-fg">
            {status?.url ?? "—"}
          </code>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Copy the address"
            disabled={!status}
            onClick={() => status && copy("url", status.url)}
          >
            {copied === "url" ? <Check /> : <Copy />}
          </Button>
        </div>
      </Setting>

      <Setting
        label="Port"
        detail="It is written into every agent's configuration, so changing it disconnects them until each is connected again."
      >
        <div className="flex items-center gap-2">
          <input
            aria-label="Port"
            value={port}
            onChange={(event) =>
              setPort(event.target.value.replace(/[^0-9]/g, "").slice(0, 5))
            }
            inputMode="numeric"
            className={cn(FIELD, "w-24 font-mono")}
          />
          <Button
            variant="outline"
            disabled={!portChanged || busy || port === ""}
            onClick={() => run("server_set_port", { port: Number(port) })}
          >
            Apply
          </Button>
        </div>
      </Setting>

      <Setting
        label="Token"
        detail="Every request carries it. Issuing a new one disconnects every agent until each is connected again."
      >
        <div className="flex items-center gap-2">
          <code
            className={cn(
              FIELD,
              "flex min-w-0 flex-1 items-center truncate font-mono text-fg-secondary",
            )}
          >
            {status?.token ?? "—"}
          </code>
          <Button
            variant="outline"
            size="icon"
            aria-label="Copy the token"
            disabled={!status}
            onClick={() => status && copy("token", status.token)}
          >
            {copied === "token" ? <Check /> : <Copy />}
          </Button>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => run("server_new_token")}
          >
            New
          </Button>
        </div>
      </Setting>

      <Setting
        label="Start at login"
        detail="An agent reaches Sync through this port, so an agent working while Sync is closed reaches nothing."
      >
        <Toggle
          isOn={atLogin ?? false}
          isAvailable={atLogin !== null}
          onChange={(wanted) => {
            void (wanted ? enable() : disable())
              .then(() => setAtLogin(wanted))
              .catch((error: unknown) => setFailure(messageOf(error)));
          }}
        />
      </Setting>

      {failure ? (
        <p className="max-w-[64ch] rounded-(--radius-control) border border-separator-strong bg-panel p-2.5 text-sm text-warning">
          {failure}
        </p>
      ) : null}
    </section>
  );
}

/** The same shape every settings row in this window has. */
function Setting({
  label,
  detail,
  children,
}: {
  label: string;
  detail: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="space-y-0.5">
        <h2 className="text-base font-medium text-fg">{label}</h2>
        <p className="max-w-[64ch] text-sm text-fg-tertiary">{detail}</p>
      </div>
      {children}
    </div>
  );
}

/**
 * Two states, said in words rather than drawn as a rocker.
 *
 * The window has no switch of its own, and a lone one built here would be the
 * only control in the application drawn that way. This is the segmented control
 * Appearance already uses, with two segments.
 */
function Toggle({
  isOn,
  isAvailable,
  onChange,
}: {
  isOn: boolean;
  isAvailable: boolean;
  onChange: (wanted: boolean) => void;
}) {
  if (!isAvailable) {
    return (
      <p className="text-sm text-fg-tertiary">
        This system does not offer login items to Sync.
      </p>
    );
  }
  return (
    <div role="radiogroup" aria-label="Start at login" className="flex gap-1">
      {[
        { label: "Off", wanted: false },
        { label: "On", wanted: true },
      ].map((option) => (
        <button
          key={option.label}
          type="button"
          role="radio"
          aria-checked={isOn === option.wanted}
          onClick={() => onChange(option.wanted)}
          className={cn(
            "flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2.5 text-sm transition-colors duration-(--motion-duration-fast) ease-shell",
            isOn === option.wanted
              ? "border-separator-strong bg-selected font-medium text-fg"
              : "text-fg-secondary hover:bg-hover hover:text-fg",
          )}
        >
          {isOn === option.wanted ? (
            <Check aria-hidden="true" className="size-3 shrink-0" />
          ) : null}
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** The height every control in this window shares. */
const FIELD =
  "h-(--control-height-lg) rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-sm text-fg";

function messageOf(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return typeof error === "string" ? error : "The server could not be reached.";
}
