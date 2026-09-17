/**
 * The data door: one tool of one of the flagship's servers, asked and answered.
 *
 * The window states three things — which MCP server, which tool, and the
 * arguments that tool takes — and gets back the JSON the tool itself returned.
 * Nothing about how it was fetched crosses this line, and that is the point of
 * the shape rather than an omission: what carries the ask today is a turn of
 * the chosen agent, it costs tokens and a wait every time, and the day it is
 * carried some other way this file does not change and neither does anything
 * that imports it.
 *
 * **The answer is the tool's own, never the agent's account of it.** A turn in
 * which the agent talked about the tool instead of calling it rejects by name;
 * it does not resolve with prose. That distinction is the whole reason the door
 * exists — a panel filled from a model's summary looks like a working panel,
 * which makes it worse than an empty one.
 *
 * It is also the tool that was asked for and no other. A turn that called
 * something Sync cannot read back as the named tool rejects, even where that
 * something answered: an answer credited to a tool by elimination is a guess,
 * and a panel cannot tell one of those from data.
 *
 * It is not a conversation. Nothing about this call joins the list a person
 * reads their agent's work in, and nothing of it is left to resume.
 */

import { command } from "@/lib/command";

/** What the window asks a tool for. */
export interface ToolAsk {
  /** The MCP server, keyed as the flagship's own configuration keys it. */
  readonly server: string;
  /**
   * The tool, as that server publishes it.
   *
   * Not the spelling any particular agent uses: one tool of one server is
   * written four different ways across the agents measured, and rendering it
   * for the chosen one happens in Rust, where the choice is known.
   */
  readonly tool: string;
  /** Passed through untouched. What a tool takes is the tool's business. */
  readonly arguments?: unknown;
}

/**
 * Ask one tool, and answer with what it returned.
 *
 * Rejects with a named refusal — `no_flagship`, `unknown_server`,
 * `tool_not_called`, `tool_permission_needed` and the rest — rather than with
 * a sentence, so a screen can tell *nobody has chosen an agent yet* from *the
 * agent would not run it* and say something different about each.
 *
 * @param project The project the turn runs in. An agent resolves its own
 *   configuration against it, and a tool that reads a repository reads this one.
 */
export function callFlagshipTool(
  project: string,
  ask: ToolAsk,
): Promise<unknown> {
  return command<unknown>("flagship_call", { project, ask });
}
