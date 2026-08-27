/**
 * The agents a project's work is done with, and how one is connected.
 *
 * Connecting writes **one server entry** into the agent's own configuration
 * file, pointing it at this project's Sync. What that file is called, where it
 * lives and what shape the entry takes is decided in Rust — `src-tauri/src/connect.rs`
 * — and not here, on purpose. A window that could write any file on request is
 * a window whose safety is a property of its frontend code; this one can ask
 * for *this server, in that file, for this project*, and nothing else is
 * reachable from the interface.
 *
 * So the catalogue is not duplicated in this file. Two copies of "where does
 * Claude Code keep its servers" drift, and the copy that matters is the one
 * doing the writing.
 */

import { invoke } from "@tauri-apps/api/core";

/** How a row reads. */
export type AgentState =
  /** No entry under Sync's name. */
  | "not_connected"
  /** An entry Sync wrote, pointing at this machine's server. */
  | "connected"
  /** An entry under Sync's name that Sync did not write. Said, never replaced. */
  | "foreign"
  /** The file is there and cannot be read as what it is meant to be. */
  | "unreadable";

/**
 * Which heading a row is listed under.
 *
 * Decided in Rust with the rest of the catalogue, for the same reason no file
 * path is named in this module: a second list of which client goes where
 * drifts from the one that does the writing.
 */
export type AgentGroup = "command_line" | "desktop" | "editor";

export interface AgentRow {
  readonly id: string;
  readonly name: string;
  readonly group: AgentGroup;
  /** The file, as a person would recognise it. */
  readonly configuration: string;
  readonly scope: "installation";
  readonly state: AgentState;
  /** Why, when the state alone does not say it. */
  readonly detail: string | null;
}

/** What one connect or disconnect changed. */
export interface ConnectionReport {
  /** The file that was written, in full: somebody about to open it needs all of it. */
  readonly file: string;
  /** What the server is called in it. */
  readonly server: string;
  /** One sentence, shown as it arrives. */
  readonly changed: string;
  readonly state: AgentState;
}

/** Every client Sync knows, and whether this machine is connected to it. */
export function loadAgents(): Promise<AgentRow[]> {
  return invoke<AgentRow[]>("agents_list", {});
}

/** Write Sync into one client's configuration. */
export function connectAgent(agent: string): Promise<ConnectionReport> {
  return invoke<ConnectionReport>("agent_connect", { agent });
}

/** Take Sync back out of it. */
export function disconnectAgent(agent: string): Promise<ConnectionReport> {
  return invoke<ConnectionReport>("agent_disconnect", { agent });
}
