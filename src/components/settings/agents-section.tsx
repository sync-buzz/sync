"use client";

import { useCallback, useEffect, useState } from "react";

import { ModelChoice } from "@/components/settings/model-choice";
import { AGENT_GROUPS } from "@/components/settings/sections";
import { ToolConsent } from "@/components/settings/tool-consent";
import { Button } from "@/components/ui/button";
import {
  connectAgent,
  disconnectAgent,
  loadAgents,
  type AgentGroup,
  type AgentRow,
} from "@/lib/settings/agents";

/**
 * Three decisions about agents, and the first and last point in opposite
 * directions.
 *
 * The first is the provider Sync works through by default — the console, and
 * any work the window starts on its own, run through it. The last is every
 * agent that reaches Sync — a line Sync writes into their configuration. They
 * are one section rather than two because they are asked about the same
 * programs by the same person in the same sitting, and apart neither would say
 * what the other decided.
 *
 * Between them is which of that provider's servers a package may spend its
 * turns on. It belongs beside the choice rather than beside the packages: a
 * package is installed into a project and this is not about a project at all —
 * it is one person's account, one machine, and the provider named a block
 * above.
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
 *
 * Each of the three is a screenful, so each is a row in the column beside this
 * one and this file draws exactly one of them at a time. `Agents` itself is not
 * a screen: choosing it in that column chooses the first part. What that gives
 * up is the three seen side by side, which is a real loss and was weighed —
 * they are one sitting's worth of the same question. What it buys is a column
 * where a row is a place and every row means the same thing, and a page three
 * times as long as any of the reasons to be on it was not paying for the
 * reading anybody actually did.
 */
export function AgentsSection({ only }: { only: string }) {
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

  // One part of the section and never all three. The window's own header has
  // already said which part this is and what it is for, so nothing here
  // repeats it: a name printed again three inches below the first reads as two
  // things rather than as one said twice.
  return (
    <section className="flex flex-col gap-5">
      {only === PROVIDER.id ? (
        <ModelChoice />
      ) : only === SERVERS.id ? (
        <>
          <ToolConsent />

          {/* Beside the servers a package may call, because that is the part
              of this window a person reaches while thinking about packages —
              and the answer to where the packages themselves are chosen has to
              be somewhere they are already looking. */}
          <p className="text-xs text-fg-tertiary">
            Extensions are not here: one installs its types, its scripts and its screen into a
            project, so it is chosen from the project&apos;s own window — at the foot of the
            sidebar.
          </p>
        </>
      ) : (
        <div className="flex flex-col gap-3">
          {runs(rows).map(([group, members]) => (
            <div key={group} className="flex flex-col gap-1">
              <h3 className="text-sm font-medium text-fg-secondary">{HEADING[group]}</h3>
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
        </div>
      )}
    </section>
  );
}

// The two parts this file asks about by name. What is connected to Sync is the
// third and is not named here: it is what is left, and a constant that only
// ever appears in the branch nothing tests would be a name kept in step with
// the list for no reader's benefit.
const [PROVIDER, SERVERS] = AGENT_GROUPS;

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
