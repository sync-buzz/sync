/**
 * Which of the person's own servers a package may call through the flagship,
 * and how that is agreed and withdrawn.
 *
 * Beside the door rather than under the settings window, because the two
 * screens that touch it are not the same screen and neither owns it: an
 * agreement is *given* on the package's own page, where a person is being told
 * what it brings and what it reaches, and *withdrawn* in Settings, where they
 * go looking for everything they have ever agreed to.
 *
 * The unit is one package and one server — every tool that server publishes.
 * Sync speaks no MCP and the flagship's configuration holds a server's key and
 * how to reach it, so there is no list of tools to put on a card: the two
 * parties who know the names are the person, from memory, and the agent, after
 * a turn has been spent.
 *
 * Nothing is cached beyond a render. The file behind this is the installation's
 * and a second window may write it, so each screen asks when it opens and after
 * every change it makes — the same way the flagship choice beside it does.
 */

import { command } from "@/lib/command";

/** One package, and every server it was agreed it may call. */
export interface ConsentedExtension {
  /** The id every agreement is stored against. */
  readonly id: string;
  /**
   * What a person reads: the package's own name, or the id where this machine
   * no longer serves it.
   */
  readonly name: string;
  /**
   * Whether this machine still serves that id.
   *
   * An agreement outlives the package it was given to, so this can be `false`
   * and the row is still shown: an agreement nobody can find is one nobody can
   * withdraw, and reinstalling the same id would find it standing.
   */
  readonly installed: boolean;
  /** The servers, keyed as the flagship's own configuration keys them. */
  readonly servers: readonly string[];
}

/** Everything this installation has agreed a package may call. */
export function loadToolConsent(): Promise<readonly ConsentedExtension[]> {
  return command<readonly ConsentedExtension[]>("tool_consent_status", {});
}

/**
 * Agree that one package may call one server.
 *
 * **What the package's page calls, and the only way an agreement is made.** One
 * row, one server: it joins whatever that package was already agreed to reach
 * rather than replacing it, because each row is withdrawn on its own and a
 * person pressing the second one is not asking to undo the first.
 *
 * Answers with every agreement this installation holds, so the screen that
 * pressed redraws from what was written rather than from what it hoped was.
 *
 * The package need not be installed yet — an agreement is about what a person
 * was shown rather than about what is on the disk.
 *
 * Rejects with a named refusal rather than a sentence: `nothing_named` where
 * neither a package nor a server was stated, which is a row that could never
 * match anything.
 */
export function grantToolConsent(
  extension: string,
  server: string,
): Promise<readonly ConsentedExtension[]> {
  return command<readonly ConsentedExtension[]>("tool_consent_grant", {
    extension,
    server,
  });
}

/**
 * Take one agreement back, and read what should now be shown.
 *
 * The next call to that server is refused by name. A turn already running is
 * left alone: it was allowed when it started, and stopping it here would leave
 * a panel with half an answer and no account of why.
 */
export function revokeToolConsent(
  extension: string,
  server: string,
): Promise<readonly ConsentedExtension[]> {
  return command<readonly ConsentedExtension[]>("tool_consent_revoke", {
    extension,
    server,
  });
}
