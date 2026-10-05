"use client";

import { useCallback, useEffect, useState } from "react";

import { Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import { grantToolConsent, loadToolConsent, type ConsentedExtension } from "@/lib/flagship/consent";
import { useFlagshipServers, type Reach } from "@/lib/flagship/servers";
import { said } from "@/lib/refusal";
import { openSettings } from "@/lib/settings/window";

/**
 * Which of the person's own servers this package may call, agreed one row at a
 * time.
 *
 * **Here rather than in Settings, because this is where the question can be
 * answered.** An agreement is about one package, and a person deciding whether
 * a package may spend turns of their agent on their own server is deciding it
 * against what that package is — which is this page and nowhere else. Settings
 * holds the other half: everything ever agreed to, and one way to take any of
 * it back. That asymmetry is deliberate and it is the same one macOS keeps —
 * granted where the thing asking is in front of you, listed and revoked in one
 * place afterwards.
 *
 * **Not switches.** A switch says the state can be set both ways from here, and
 * a row that could be switched off here would be a second place the agreement
 * is managed from — with nothing to say which of the two is the list. The row
 * gives, and says where it is taken back.
 *
 * **One agreement is a whole server.** Sync speaks no MCP and the flagship's
 * configuration holds a server's key and how to reach it, so there is no list
 * of tools to agree to one at a time; a row implying otherwise would promise a
 * narrowness nobody is keeping. The sentence says so before anything is
 * pressed.
 *
 * Drawn only for a package whose manifest asked to ask — the page decides that,
 * because the page holds the manifest. A section drawn for a package that can
 * call nothing would name a state instead of showing one.
 */
export function ServersItMayCall({ id }: { id: string }) {
  const flagship = useFlagshipServers();
  const [agreed, setAgreed] = useState<readonly string[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    void loadToolConsent()
      .then((held) => {
        if (current) setAgreed(serversOf(held, id));
      })
      .catch((refused: unknown) => {
        if (current) setFailure(said(refused));
      });
    return () => {
      current = false;
    };
  }, [id]);

  const agree = useCallback(
    (server: string) => {
      setBusy(server);
      setFailure(null);
      // Redrawn from what came back rather than from what was pressed: the
      // command answers with every agreement this installation holds, so a
      // second window that agreed to something a moment ago is on this screen
      // too.
      void grantToolConsent(id, server)
        .then((held) => setAgreed(serversOf(held, id)))
        .catch((refused: unknown) => setFailure(said(refused)))
        .finally(() => setBusy(null));
    },
    [id],
  );

  return (
    <div className="flex flex-col gap-2">
      <p className="max-w-[68ch] text-sm leading-5 text-fg-secondary">
        It asks through the agent Sync works through, so every call costs a turn of yours. Agreeing
        covers that server whole — every tool it publishes, including the ones that change things —
        because Sync speaks none of these protocols and cannot ask a server what it holds. Withdraw
        an agreement in Settings, under Agents.
      </p>

      {flagship.refusal !== null ? (
        <Unavailable kind={flagship.refusal.kind} message={flagship.refusal.message} />
      ) : flagship.isLoading ? null : flagship.servers.length === 0 ? (
        <p className="max-w-[68ch] text-sm leading-5 text-fg-tertiary">
          That agent&apos;s own configuration names no servers, so there is nothing here to agree to
          yet. Servers are added there rather than here — Sync writes into that file only to connect
          itself.
        </p>
      ) : (
        <ul className="divide-y divide-separator overflow-hidden rounded-(--radius-surface) border border-separator">
          {flagship.servers.map((server) => {
            const isAgreed = agreed?.includes(server.name) ?? false;
            return (
              <li key={server.name} className="flex items-center gap-3 px-3 py-2">
                <div className="min-w-0 flex-1">
                  {/* The key that person's own file spells, which is what they
                      match this row against when they go looking for it
                      there. */}
                  <p className="truncate font-mono text-sm text-fg">{server.name}</p>
                  <p className="truncate text-xs text-fg-tertiary">{reached(server.reach)}</p>
                </div>

                {isAgreed ? (
                  <span className="flex shrink-0 items-center gap-1 text-xs text-fg-tertiary">
                    <Check aria-hidden="true" className="size-3 shrink-0" />
                    Agreed
                  </span>
                ) : (
                  <Button
                    variant="outline"
                    size="sm"
                    // Until the agreements have been read, whether this row is
                    // already agreed to is unknown — and a control that turned
                    // out to be a second yes to the same thing is worse than a
                    // control that waited a moment.
                    disabled={agreed === null || busy !== null}
                    onClick={() => agree(server.name)}
                  >
                    Agree
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </div>
  );
}

/**
 * Why there is no list, and what to do about it where there is something.
 *
 * Nobody having chosen an agent is not a failure and is not worded as one: it
 * is the ordinary state of a machine somebody has not finished setting up, and
 * what it needs is the way to the choice rather than a sentence about a file.
 * Every other refusal is the refuser's own words — the path of the file that
 * could not be read is in them, and that is the part a person acts on.
 */
function Unavailable({ kind, message }: { kind: string | null; message: string }) {
  if (kind === "no_flagship") {
    return (
      <div className="flex flex-col items-start gap-2">
        <p className="max-w-[68ch] text-sm leading-5 text-fg-tertiary">
          Sync works through one agent of yours, and none has been chosen on this Mac. Its servers
          are the ones a package can be agreed to call.
        </p>
        <Button size="sm" variant="secondary" onClick={() => void openSettings("agents/provider")}>
          Choose an agent
        </Button>
      </div>
    );
  }

  return <p className="max-w-[68ch] text-sm leading-5 text-fg-tertiary">{message}</p>;
}

/** How this server is reached, in one line a person recognises it by. */
function reached(reach: Reach): string {
  switch (reach.kind) {
    case "http":
      return reach.url;
    case "process":
      return reach.command;
    default:
      // Carried rather than dropped, for the reason Rust carries it: a server
      // left off this list reads as a server the person does not have.
      return "Its entry says neither an address nor a program.";
  }
}

/** Every server one package was agreed to reach, out of the whole list. */
function serversOf(held: readonly ConsentedExtension[], id: string): readonly string[] {
  return held.find((one) => one.id === id)?.servers ?? [];
}

/** The server name that means *any server*, kept with the Rust that reads it. */
const WILDCARD_SERVER = "*";

/**
 * One agreement for every MCP server the project declares, for a package that
 * reaches them through `sync.call` rather than through the flagship.
 *
 * The per-server section above is for `tools.call` — a package spending turns
 * of the person's agent on the agent's own servers, agreed one server at a
 * time. A package that calls a project-scoped MCP server from a handler
 * (`sync.call`, gated by `handler.call`) reaches servers the person added to
 * the project themselves, and the set is decided in the project rather than in
 * the package: a workflow node names `playwright.navigate` today and
 * `stripe.get_account` tomorrow. Agreeing one server at a time is a friction
 * the person asked to lift, so the row here is the wildcard — `*` — and it is
 * the width it says it is: every MCP server the project holds, now and later.
 *
 * Drawn for `handler.call`, because that is the capability `sync.call` is
 * gated by, and `sync.call` is the door that reaches a project-scoped MCP
 * server from a handler.
 */
export function McpServersItMayCall({ id }: { id: string }) {
  const [agreed, setAgreed] = useState<readonly string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    void loadToolConsent()
      .then((held) => {
        if (current) setAgreed(serversOf(held, id));
      })
      .catch((refused: unknown) => {
        if (current) setFailure(said(refused));
      });
    return () => {
      current = false;
    };
  }, [id]);

  const agree = useCallback(() => {
    setBusy(true);
    setFailure(null);
    void grantToolConsent(id, WILDCARD_SERVER)
      .then((held) => setAgreed(serversOf(held, id)))
      .catch((refused: unknown) => setFailure(said(refused)))
      .finally(() => setBusy(false));
  }, [id]);

  const isAgreed = agreed?.includes(WILDCARD_SERVER) ?? false;

  return (
    <div className="flex flex-col gap-2">
      <p className="max-w-[68ch] text-sm leading-5 text-fg-secondary">
        A handler of this package calls MCP servers the project declares — a workflow node naming
        <span className="font-mono text-xs"> playwright.navigate</span>, for instance. Agreeing here
        lets it call <strong>any</strong> MCP server in this project, now and later, including ones
        that change things. Withdraw it in Settings, under Agents.
      </p>

      {isAgreed ? (
        <span className="flex items-center gap-1 text-xs text-fg-tertiary">
          <Check aria-hidden="true" className="size-3 shrink-0" />
          May call any MCP server in this project
        </span>
      ) : (
        <Button variant="outline" size="sm" disabled={agreed === null || busy} onClick={agree}>
          Allow all MCP servers
        </Button>
      )}

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </div>
  );
}
