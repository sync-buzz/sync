"use client";

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  connectAgent,
  disconnectAgent,
  loadAgents,
  type AgentGroup,
  type AgentRow,
} from "@/lib/settings/agents";

/**
 * Connecting an agent to Sync.
 *
 * Each row is read from the agent's own configuration file rather than from
 * anything Sync remembers, and that is the whole design: a connection is a line
 * in somebody else's file, and a person may edit or delete it without telling
 * this window. A row that reported from a record of our own would say
 * "Connected" about a file that no longer says so.
 *
 * Which is also why an entry under Sync's name that Sync did not write is
 * reported rather than replaced. The name in that file is theirs until they say
 * otherwise, and a Connect button that quietly took it over would disconnect an
 * agent from whatever it was pointed at.
 *
 * The section is about this machine, and no project is named in it. One server
 * serves every project a person has opened, so an entry is written once per
 * client and says the same thing whatever was open at the time — which is also
 * why nothing here is written into a repository: a file in a checkout would be
 * a commit announcing to a team that somebody is trying this.
 */
export function AgentsSection() {
  const [rows, setRows] = useState<readonly AgentRow[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  // A counter rather than a boolean: a refresh started by a Connect that
  // finished after the effect re-ran would otherwise write its rows over newer
  // ones. Bumping it is what makes the last read win.
  const [reading, setReading] = useState(0);
  const refresh = useCallback(() => setReading((count) => count + 1), []);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const loaded = await loadAgents();
        if (live) setRows(loaded);
      } catch (error: unknown) {
        if (live) setFailure(explain(error));
      }
    })();
    return () => {
      live = false;
    };
  }, [reading]);

  const act = useCallback(
    (agent: string, connect: boolean) => {
      setBusy(agent);
      setFailure(null);
      setSaid(null);
      const run = connect ? connectAgent(agent) : disconnectAgent(agent);
      void run
        .then((report) => {
          // What changed, in the words the command chose. Restating it here
          // would be this component deciding what happened to a file it did not
          // write.
          setSaid(report.changed);
          refresh();
        })
        .catch((error: unknown) => {
          setFailure(explain(error));
        })
        .finally(() => setBusy(null));
    },
    [refresh],
  );

  return (
    <section className="flex flex-col gap-3">
      <p className="text-sm text-fg-secondary">
        Sync serves every project from one address. Connecting
        writes a single server entry into the agent&apos;s own configuration —
        outside any repository, so nothing about it reaches your team — and
        disconnecting takes exactly that entry back out. Which project a call is
        about is the agent&apos;s to say on each call.
      </p>
      {runs(rows).map(([group, members]) => (
        <div key={group} className="flex flex-col gap-1">
          <h2 className="text-base font-medium text-fg">{HEADING[group]}</h2>
          <ul className="flex flex-col gap-px">
            {members.map((agent) => (
              <li
                key={agent.id}
                className="flex items-center gap-3 rounded-(--radius-control) px-2 py-2"
              >
                <div className="min-w-0 flex-1">
                  <p className="truncate text-base text-fg">{agent.name}</p>
                  <p className="truncate text-xs text-fg-tertiary">
                    <span className="font-mono">{agent.configuration}</span>
                  </p>
                  {agent.detail !== null && (
                    <p className="text-xs text-fg-tertiary">{agent.detail}</p>
                  )}
                </div>

                <span
                  className={
                    agent.state === "connected"
                      ? "shrink-0 text-xs text-fg-secondary"
                      : "shrink-0 text-xs text-fg-tertiary"
                  }
                >
                  {LABEL[agent.state]}
                </span>

                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => act(agent.id, agent.state !== "connected")}
                >
                  {agent.state === "connected" ? "Disconnect" : "Connect"}
                </Button>
              </li>
            ))}
          </ul>
        </div>
      ))}

      {said !== null && <p className="text-xs text-fg-secondary">{said}</p>}
      {failure !== null && <p className="text-xs text-danger">{failure}</p>}
    </section>
  );
}

/**
 * The rows in runs, one run per heading.
 *
 * The order is the one `agents_list` answered in rather than a sort of our own.
 * Which client belongs under which heading is decided in `connect.rs` with the
 * rest of the catalogue, and re-deciding it here would be a second list to keep
 * in step with that one — the kind that drifts quietly, because both look right
 * on their own.
 */
function runs(rows: readonly AgentRow[]): readonly [AgentGroup, AgentRow[]][] {
  const grouped: [AgentGroup, AgentRow[]][] = [];
  for (const row of rows) {
    const open = grouped.at(-1);
    if (open?.[0] === row.group) open[1].push(row);
    else grouped.push([row.group, [row]]);
  }
  return grouped;
}

const HEADING: Record<AgentGroup, string> = {
  command_line: "Command-line agents",
  desktop: "Desktop apps",
  editor: "Code editors",
};

const LABEL: Record<AgentRow["state"], string> = {
  connected: "Connected",
  not_connected: "Not connected",
  foreign: "Name taken",
  unreadable: "Unreadable",
};

/**
 * A refusal in the words it arrived in.
 *
 * The commands answer with a `kind` and a message written for a person — the
 * file that could not be written, the name already spoken for — and a sentence
 * of our own would drop the path, which is the part somebody acts on.
 */
function explain(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  if (error instanceof Error) return error.message;
  return "The agent's configuration could not be changed.";
}
