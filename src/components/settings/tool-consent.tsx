"use client";

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { messageOf } from "@/components/settings/shared";
import {
  loadToolConsent,
  revokeToolConsent,
  type ConsentedExtension,
} from "@/lib/flagship/consent";

/**
 * Every server a package was agreed to be able to reach, and one way to take
 * any of it back.
 *
 * A list of rows with a button each, which is what macOS uses for permissions
 * somebody granted one at a time and revokes one at a time. Not switches: a
 * switch says the state can be set both ways from here, and this one cannot —
 * an agreement is *given* where a person is being told what a package will
 * reach, which is that package's own page, and this screen is where they come
 * to find what they have agreed to since.
 *
 * The row is a server, because a server is what somebody agreed to. It carries
 * no list of tools under it, and inventing one would be this screen claiming to
 * know something the application cannot ask anybody: Sync speaks no MCP, and
 * the file the servers are read from holds a key and a way to reach it.
 *
 * A package this machine no longer serves keeps its rows and says so rather
 * than disappearing. An agreement outlives the package it was given to, so
 * hiding it would leave somebody unable to withdraw something that would still
 * be standing the day they installed that id again.
 */
export function ToolConsent() {
  const [listed, setListed] = useState<readonly ConsentedExtension[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void loadToolConsent()
      .then((answer) => {
        if (live) setListed(answer);
      })
      .catch((error: unknown) => {
        if (live) setFailure(messageOf(error, "The agreement could not be changed."));
      });
    return () => {
      live = false;
    };
  }, []);

  const withdraw = useCallback((extension: string, server: string) => {
    setBusy(`${extension} ${server}`);
    setFailure(null);
    void revokeToolConsent(extension, server)
      .then(setListed)
      .catch((error: unknown) =>
        setFailure(messageOf(error, "The agreement could not be changed.")),
      )
      .finally(() => setBusy(null));
  }, []);

  // Nothing agreed to and nothing gone wrong is the state every machine starts
  // in, and the sentence says where an agreement comes from rather than
  // reporting an empty list back to the person looking at one.
  if (listed !== null && listed.length === 0) {
    return (
      <p className="max-w-[64ch] text-xs text-fg-tertiary">
        Nothing has been agreed to. A package asks on its own page, under Marketplace, and until
        somebody has agreed there it can call nothing.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {(listed ?? []).map((held) => (
        <div key={held.id} className="flex flex-col gap-1">
          <h3 className="flex items-baseline gap-2 text-sm font-medium text-fg-secondary">
            {held.name}
            {held.installed ? null : (
              <span className="text-xs font-normal text-fg-tertiary">No longer installed</span>
            )}
          </h3>
          <ul className="flex flex-col gap-px">
            {held.servers.map((server) => (
              <li
                key={server}
                className="flex items-center gap-3 rounded-(--radius-control) px-2 py-2"
              >
                <div className="min-w-0 flex-1">
                  {/* The key the person's own agent configuration holds, which
                      is what somebody matches against when they go looking for
                      it there. */}
                  <p className="truncate text-base text-fg">
                    <span className="font-mono">{server}</span>
                  </p>
                  <p className="truncate text-xs text-fg-tertiary">Every tool it publishes</p>
                </div>

                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => withdraw(held.id, server)}
                >
                  Withdraw
                </Button>
              </li>
            ))}
          </ul>
        </div>
      ))}

      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </div>
  );
}
