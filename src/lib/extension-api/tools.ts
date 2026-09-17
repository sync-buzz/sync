"use client";

import { command } from "@/lib/command";

import type { ExtensionTools, ToolAsk } from "@/lib/extension-api/contract";

/**
 * Asking one of the person's own tools something, on a package's behalf.
 *
 * Here rather than beside the rest of the surface's functions for the reason
 * `net` and `vault` are: **every other call an extension makes is about the
 * project, and this one is about the extension.** Whether a package may ask at
 * all is read off the manifest on this machine, so the call has to arrive
 * attributed rather than carrying an id somebody wrote into an argument.
 *
 * So this is not exported. The host builds one per package while it is
 * activating it, with the id closed over, and hands it to the module as
 * `host.tools`. A package holds what it was given; there is no function on the
 * surface it could call instead.
 *
 * The check is not here and deliberately: this builds a request and says what
 * came back. Rust resolves the package, reads its manifest off the artefact,
 * and refuses a package that did not ask for this. A check in the window would
 * be a check inside the thing being checked.
 */
export function toolsFor(id: string): ExtensionTools {
  return {
    // The ask crosses whole rather than as members this rebuilds on the way.
    // What an ask is, is stated once — in `ToolAsk`, and in the type Rust reads
    // it back into — so a member added to one is a member the other refuses by
    // name rather than one that quietly never arrives.
    call: (project: string, ask: ToolAsk) =>
      command<unknown>("extension_tool_call", { id, project, ask }),
  };
}
