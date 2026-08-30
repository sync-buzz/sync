"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

import { forgetAdapters, prepareAdapters } from "@/lib/agent-sessions/client";
import {
  callExtensionHandler,
  installFromRegistry,
  rememberDeclaration,
  repointExtension,
  type InstalledExtension,
  type Pointer,
  type RegistryArtefact,
} from "@/lib/extension-host/client";
import type { Packages } from "@/lib/extension-host/packages";
import {
  countRecordsOfKind,
  publishExtensionTypes,
  type ExtensionTypeInput,
} from "@/lib/memory/client";
import { explain } from "@/lib/memory/use-corpus";
import { saveProjectSettings } from "@/lib/project/client";
import type { OpenProject, ToolDeclaration } from "@/lib/project/types";

/**
 * What a project is composed of, and how that changes.
 *
 * A project declares the extensions it uses in its own record, so the
 * declaration travels with the repository and the same folder opened elsewhere
 * is the same project. What satisfies the declaration is the machine's
 * business: a package unpacked under that id, whose own files say what it
 * publishes and what it tells an agent.
 *
 * What a declaration resolves to is asked of the packages on this machine, and
 * of nothing else. There is no second answer to fall back on: this build
 * compiles no extension, so an id nothing on the disk answers to is an id this
 * machine cannot satisfy, and saying so is the honest reply.
 *
 * Installing is two writes in one order that matters: the types first, then the
 * declaration. A project that declares an extension whose types were never
 * published would validate records against a schema it does not have; the other
 * way round, a failure leaves types nobody declared, which the next install
 * quietly reuses. Failing safe means failing towards the harmless one.
 *
 * Removing writes only the declaration. **Types and records are left exactly
 * where they are** — turning an extension off is not a statement that its data
 * is expendable, and the count of what stays is said out loud rather than
 * discovered later.
 */
export interface Composition {
  /** The ids this project declares, in the order it declares them. */
  readonly installed: readonly string[];
  readonly isInstalled: (id: string) => boolean;
  /**
   * Whether this machine holds a package that answers to the id.
   *
   * Asked before offering to install rather than discovered from a click that
   * does nothing. A card describing an extension nobody has unpacked is a real
   * thing to show — that is what a catalogue is — but the button on it has to
   * say which of the two states it is in.
   */
  readonly canInstall: (id: string) => boolean;
  /** True while the store is being written, so a command cannot be asked twice. */
  readonly isBusy: boolean;
  /** Why the last change did not happen, in words, or `null`. */
  readonly failure: string | null;
  readonly dismissFailure: () => void;
  /**
   * Declare an extension, fetching it first when this machine has not got it.
   *
   * The artefact comes from the registry's index and is given by the catalogue.
   * Without one this can only declare what is already unpacked, which is what
   * installing from a file or a folder leaves behind.
   */
  readonly install: (id: string, from?: RegistryArtefact) => Promise<void>;
  /**
   * Move a declared extension to another version the registry published.
   *
   * The same operation in both directions — an update is the larger number and
   * a downgrade the smaller one — and neither happens on its own: what it
   * writes is type definitions and a version into the repository, which is a
   * commit, and a commit nobody made is not an update.
   */
  readonly change: (id: string, to: RegistryArtefact) => Promise<void>;
  readonly remove: (id: string) => Promise<void>;
  /**
   * How many records would be left without a screen if this extension were
   * removed, asked of the store at the moment of asking rather than remembered.
   */
  readonly countRecords: (id: string) => Promise<number>;
}

/**
 * What a declared id resolves to on this machine.
 *
 * One lookup rather than four, because every caller below wants a different
 * part of the same answer and four lookups is four places to forget the
 * fallback. `version` is `null` when nothing on this machine answers to the id,
 * which is the state a project has when it was composed elsewhere.
 */
interface Resolved {
  readonly version: string | null;
  /** The artefact's sha256, and `undefined` for a folder being written in. */
  readonly integrity: string | undefined;
  /** `registry`, `file`, `folder`, or `seeded` for one the build shipped. */
  readonly source: string | undefined;
  readonly types: readonly ExtensionTypeInput[];
  readonly prompt: string | undefined;
  /**
   * What it offers an agent, as the record carries them.
   *
   * The manifest's `handler` is dropped here rather than in the record: it is
   * the package's own name for one of its functions, it changes when the author
   * renames something, and nothing outside the package can act on it. What
   * travels is what an agent is told.
   */
  readonly tools: readonly ToolDeclaration[];
  /** Packages it needs from npm before it works well. */
  readonly npm: readonly string[];
}

const NOTHING: Resolved = {
  version: null,
  integrity: undefined,
  source: undefined,
  types: [],
  prompt: undefined,
  tools: [],
  npm: [],
};

function resolve(id: string, packages: Packages): Resolved {
  const packaged = packages.byId(id);
  return packaged === null ? NOTHING : resolvedOf(packaged);
}

/**
 * The same answer, from a package in hand rather than from the id.
 *
 * Needed because installing from the registry answers with the package it just
 * unpacked, and the list of what is on this machine has not caught up yet: a
 * reload is asked for, but it lands in a later render, and the declaration
 * being written now must describe what was actually installed rather than what
 * this closure last saw.
 */
function resolvedOf(packaged: InstalledExtension): Resolved {
  return {
    version: packaged.manifest.version,
    integrity: packaged.pointer.integrity ?? undefined,
    source: packaged.pointer.source,
    // Straight through, with no translation on the way: the package's files are
    // already written in the shape the engine's command asks for.
    types: packaged.types,
    prompt: packaged.prompt ?? undefined,
    tools: packaged.manifest.tools.map((tool) => ({
      name: tool.name,
      description: tool.description,
      input: tool.input,
    })),
    npm: packaged.manifest.dependencies.npm,
  };
}

/**
 * Whether two sets of declarations say the same thing.
 *
 * Compared member by member rather than by serialising both, because `input` is
 * somebody else's JSON: two schemas that differ only in the order their keys
 * were written are the same schema, and rewriting the record over that would be
 * a write on every open for ever. Order within the list does matter — it is the
 * order an agent is told about them in, and it is the author's.
 */
function sameTools(
  mine: readonly ToolDeclaration[],
  stored: readonly ToolDeclaration[] | undefined,
): boolean {
  const theirs = stored ?? [];
  return (
    mine.length === theirs.length &&
    mine.every(
      (tool, at) =>
        tool.name === theirs[at].name &&
        tool.description === theirs[at].description &&
        sameShape(tool.input, theirs[at].input),
    )
  );
}

/** Deep equality over what a schema is made of, and nothing more. */
function sameShape(mine: unknown, theirs: unknown): boolean {
  if (mine === theirs) return true;
  if (mine === null || theirs === null) return false;
  if (typeof mine !== "object" || typeof theirs !== "object") return false;
  if (Array.isArray(mine) !== Array.isArray(theirs)) return false;
  if (Array.isArray(mine) && Array.isArray(theirs)) {
    return (
      mine.length === theirs.length &&
      mine.every((one, at) => sameShape(one, theirs[at]))
    );
  }
  const ours = Object.keys(mine as object);
  const others = Object.keys(theirs as object);
  return (
    ours.length === others.length &&
    ours.every((key) =>
      sameShape(
        (mine as Record<string, unknown>)[key],
        (theirs as Record<string, unknown>)[key],
      ),
    )
  );
}

/**
 * Downloads what an extension needs, and never fails the install for it.
 *
 * Installing is a statement about the project, written into its record; whether
 * a package could be fetched is a fact about this machine and this minute. A
 * project that declared a package while the machine was offline has still
 * declared it, and refusing the declaration would make the record depend on the
 * network.
 * The cost of the download not having happened is one slow first conversation —
 * the session layer checks before it raises an agent — which is exactly what
 * every conversation cost before any of this existed.
 *
 * What the manifest declares is *that* something is needed from npm. What is
 * fetched is the adapter set this build knows how to fetch, and deliberately
 * not the names in the list: installing an arbitrary package into the window on
 * an extension's say-so is a much larger permission than the one being asked
 * for here, and nothing needs it yet.
 */
async function acquireDependencies(npm: readonly string[]): Promise<void> {
  if (npm.length === 0) return;
  try {
    await prepareAdapters();
  } catch {
    // Deliberately silent here. It is reported where it is actionable — in the
    // agent picker, which says whether an adapter is ready — rather than as a
    // failure of an install that did in fact happen.
  }
}

/**
 * Deletes what an extension downloaded.
 *
 * Safe despite the mismatch it looks like: what was installed belongs to the
 * machine and what was removed was one project's declaration, so another
 * project may still want this. It is still deleted, because the alternative is
 * bookkeeping across every registered project to answer a question about a
 * directory — and because being wrong is cheap: the next launch that needs it
 * fetches it again.
 */
async function releaseDependencies(npm: readonly string[]): Promise<void> {
  if (npm.length === 0) return;
  try {
    await forgetAdapters();
  } catch {
    // A directory that could not be deleted is not a reason to keep an
    // extension the person has removed.
  }
}

/**
 * Puts the pointer back after a change that could not be finished.
 *
 * Answers with `null` when the machine is where it was, and with a sentence
 * when it is not. That second case is small and must not be silent: the
 * artefact was swapped, the project's record was not, and the two now disagree
 * about which version this extension is. Nothing here can repair it — what it
 * can do is say so beside the reason the change failed, so that the state a
 * person is left in is one they were told about.
 */
async function stepBack(previous: Pointer): Promise<string | null> {
  try {
    await repointExtension(previous);
    return null;
  } catch {
    return `This machine is still serving ${previous.version}, which is no longer what the project declares.`;
  }
}

export function useComposition(
  project: OpenProject,
  /** What this machine has unpacked, which is what a declaration resolves to. */
  packages: Packages,
  onChanged: (project: OpenProject) => void,
): Composition {
  const [isBusy, setIsBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  // Held rather than rebuilt, because it is a dependency and not only a value.
  // The list leaves this hook and is read into `useMemo` deps elsewhere — the
  // opener the window binds is one — so a fresh array on every render is a
  // fresh answer on every render, and anything downstream that re-reads when
  // its opener changes would re-read for ever. It was, before badges asked.
  const installed = useMemo(
    () => project.installed.map((entry) => entry.id),
    [project.installed],
  );
  // Encoded as well, because a project object replaced with one declaring the
  // same extensions is a new array and the same answer: the effect below must
  // run when what the project declares changes, not when the object holding it
  // is swapped.
  const declared = installed.join();

  /**
   * What the project declares is published on whatever machine opens it.
   *
   * The declaration travels with the repository; the types do not have to. A
   * colleague who clones the project and has the same build gets its schema by
   * opening it, rather than by being told to install something. Republishing
   * what is already there writes nothing, so this costs one read per open.
   *
   * A failure here is left to the surface that notices it. There is nothing a
   * person can do about it at the moment a window opens, and a project whose
   * types are missing says so where records fail to be written.
   */
  useEffect(() => {
    void (async () => {
      for (const id of declared === "" ? [] : declared.split(",")) {
        const { types } = resolve(id, packages);
        if (types.length === 0) continue;
        try {
          await publishExtensionTypes(project.path, types);
        } catch {
          // Deliberately quiet: see above.
        }
      }
    })();
  }, [declared, packages, project.path]);

  /**
   * What this project declares is remembered where the clock can read it.
   *
   * The clock runs in the process that survives every window being closed, and
   * what a project declares is inside that project's repository — so without
   * this, finding out whether anything is scheduled would mean opening every
   * repository on the machine. Writing the ids when a window opens the project
   * costs one call and makes that unnecessary.
   *
   * It runs on every open rather than only when the declaration changes: this
   * is the moment the installation learns the project exists at all, and an
   * effect that fired only on a change would leave a project that has always
   * declared the same thing out of the file for ever.
   *
   * A failure is left where the others in this hook are — there is nothing a
   * person can do about it while a window is opening, and the cost is that
   * handlers do not run in the background until the next open, which is not
   * news to interrupt somebody with.
   */
  useEffect(() => {
    void rememberDeclaration(
      project.path,
      declared === "" ? [] : declared.split(","),
    ).catch(() => {
      // Deliberately quiet: see above.
    });
  }, [declared, project.path]);

  /**
   * What an agent is told travels with the project, so this build's has to
   * reach it.
   *
   * A prompt and a set of tool declarations are written into the project
   * because the agent reads both through a server that cannot see the
   * catalogue. Those copies are the build's, not the project's decision, so
   * when the two disagree — the extension shipped a better prompt, renamed a
   * tool, or this repository was written by an older build — the build wins and
   * the record is rewritten. Nothing is written when they agree, which is every
   * open but the first after an update.
   *
   * Both in one pass rather than two effects: they come off one manifest and go
   * into one record, and two writers would mean two saves on the open after an
   * update that changed both.
   *
   * The version is left exactly as it was. What a project declares is the
   * version somebody installed, and raising it here would be this window
   * upgrading a dependency while nobody was looking.
   */
  useEffect(() => {
    const refreshed = project.installed.map((entry) => {
      const { version, prompt, tools } = resolve(entry.id, packages);
      // Nothing on this machine answers to the id, so there is no text to
      // compare against and the stored one is left exactly as it is. Erasing it
      // would mean opening a project on a machine missing one of its extensions
      // silently took that extension's instructions away from every agent.
      if (version === null) return entry;
      if (prompt === entry.prompt && sameTools(tools, entry.tools)) return entry;
      return { ...entry, prompt, tools };
    });
    if (refreshed.every((entry, at) => entry === project.installed[at])) return;

    void (async () => {
      const next = { ...project, installed: refreshed };
      try {
        await saveProjectSettings(next.path, {
          name: next.name,
          identifier: next.identifier,
          description: next.description,
          language: next.language,
          installed: next.installed,
        });
        onChanged(next);
      } catch {
        // As above: nothing a person can do about it while a window opens, and
        // an agent reading a prompt one version old is not a failure to report
        // in front of somebody who did not ask for one.
      }
    })();
  }, [packages, project, onChanged]);

  const write = useCallback(
    async (next: OpenProject, publish: readonly string[]) => {
      setIsBusy(true);
      try {
        // Types before the declaration: see the note on this interface. A
        // project may declare several extensions, and each publishes its own
        // set — one transaction per extension, because installing two things
        // is two decisions even when they are made in one sitting.
        for (const id of publish) {
          const { types } = resolve(id, packages);
          if (types.length === 0) continue;
          await publishExtensionTypes(project.path, types);
        }

        // Then the package's own handler, between the two writes and for the
        // same reason they are in that order. Its types exist by now, so it may
        // write records of them; the declaration does not, so a handler that
        // refuses leaves a project that never took the extension on. Anything
        // it throws lands in the catch below and is reported where the person
        // clicked, in the package's own words.
        for (const id of publish) {
          await callExtensionHandler(project.path, id, "installed", {
            project: { path: next.path, name: next.name },
            version: resolve(id, packages).version,
          });
        }

        await saveProjectSettings(next.path, {
          name: next.name,
          identifier: next.identifier,
          description: next.description,
          language: next.language,
          installed: next.installed,
        });
        setFailure(null);
        onChanged(next);
      } catch (refused) {
        setFailure(explain(refused));
      } finally {
        setIsBusy(false);
      }
    },
    [onChanged, packages, project.path],
  );

  const install = useCallback(
    async (
      id: string,
      /**
       * Where to fetch it from, for an extension this machine does not have.
       *
       * Given by the catalogue for an entry the registry lists and nothing here
       * answers to. Fetching is the first half of installing rather than a step
       * of its own: there is nothing a person could do between the two, and a
       * downloaded package nobody declared is litter with no owner.
       */
      from?: RegistryArtefact,
    ) => {
      if (project.installed.some((entry) => entry.id === id)) return;

      let resolved = resolve(id, packages);
      if (resolved.version === null && from !== undefined) {
        setIsBusy(true);
        try {
          // Answered with the package, and read from that rather than from the
          // list of what is unpacked: the reload below lands in a later render,
          // and what is declared has to describe what was actually installed.
          resolved = resolvedOf(await installFromRegistry(from));
        } catch (refused) {
          setFailure(explain(refused));
          setIsBusy(false);
          return;
        }
        setIsBusy(false);
        void packages.reload();
      }

      const { version, integrity, source, prompt, tools, npm } = resolved;
      // Nothing on this machine answers to the id, and nothing was offered to
      // fetch. A project cannot declare a version nobody can name, and
      // inventing one would write a lockfile entry for an artefact that does
      // not exist.
      if (version === null) return;
      await write(
        {
          ...project,
          installed: [
            ...project.installed,
            { id, version, prompt, integrity, source, tools },
          ],
        },
        [id],
      );
      await acquireDependencies(npm);
    },
    [packages, project, write],
  );

  /**
   * Moves a declared extension to another version the registry published.
   *
   * The same operation upwards and downwards: what makes it an update or a
   * downgrade is which number was chosen, and there is nothing else different
   * about it. A project pinned to an older version because the newer one needs
   * a Sync this machine does not have is the case that earns the second
   * direction, and it is a real one.
   *
   * **The install is not the whole of it, and that is why the rollback is
   * here.** Unpacking and moving the pointer happen in Rust, where a failure
   * leaves an unreferenced directory and a pointer that never moved. What
   * follows — publishing the new type definitions and writing the new version
   * into the project's record — happens up here, after the pointer has already
   * moved, and a failure in either would leave this machine serving one version
   * while the project declares another. So the pointer goes back to exactly
   * what it was: the previous artefact was never modified, so putting it back
   * costs a few bytes rather than another download.
   *
   * The handler is deliberately not called. `lifecycle.installed` is the moment
   * a project took an extension on, and it took this one on before; a second
   * call would be the package told it was installed twice.
   */
  const change = useCallback(
    async (id: string, to: RegistryArtefact) => {
      const packaged = packages.byId(id);
      const declared = project.installed.find((entry) => entry.id === id);
      // Nothing to move: an id this machine does not hold, or one this project
      // never declared. Both are states the button is not drawn in, and neither
      // is worth a sentence in front of somebody who did not ask a question.
      if (packaged === null || declared === undefined) return;
      const previous = packaged.pointer;

      setIsBusy(true);
      try {
        const resolved = resolvedOf(await installFromRegistry(to));
        const { version, integrity, source, prompt, tools, types, npm } = resolved;
        if (version === null) return;

        const next = {
          ...project,
          installed: project.installed.map((entry) =>
            entry.id === id
              ? { id, version, prompt, integrity, source, tools }
              : entry,
          ),
        };

        try {
          // One transaction for the whole vocabulary, and before the record is
          // written: a project declaring a version whose types were never
          // published would validate records against a schema it has not got.
          if (types.length > 0) {
            await publishExtensionTypes(project.path, types);
          }
          await saveProjectSettings(next.path, {
            name: next.name,
            identifier: next.identifier,
            description: next.description,
            language: next.language,
            installed: next.installed,
          });
        } catch (refused) {
          const stranded = await stepBack(previous);
          throw stranded === null
            ? refused
            : new Error(`${explain(refused)} ${stranded}`);
        }

        setFailure(null);
        onChanged(next);
        await acquireDependencies(npm);
      } catch (refused) {
        setFailure(explain(refused));
      } finally {
        setIsBusy(false);
        void packages.reload();
      }
    },
    [onChanged, packages, project],
  );

  const remove = useCallback(
    async (id: string) => {
      const { npm } = resolve(id, packages);
      await write(
        {
          ...project,
          installed: project.installed.filter((entry) => entry.id !== id),
        },
        [],
      );
      await releaseDependencies(npm);
    },
    [packages, project, write],
  );

  const countRecords = useCallback(
    async (id: string) => {
      const counts = await Promise.all(
        resolve(id, packages).types.map((type) =>
          countRecordsOfKind(project.path, type.kind),
        ),
      );
      return counts.reduce((total, count) => total + count, 0);
    },
    [packages, project.path],
  );

  return {
    installed,
    isInstalled: (id: string) => installed.includes(id),
    canInstall: (id: string) => packages.byId(id) !== null,
    isBusy,
    failure,
    dismissFailure: () => setFailure(null),
    install,
    change,
    remove,
    countRecords,
  };
}

const Context = createContext<Composition | null>(null);

export const CompositionProvider = Context.Provider;

/**
 * What the project is composed of, for the one area that changes it.
 *
 * Only the catalogue uses this. An extension does not install other extensions:
 * what a project is made of is a decision a person makes, and putting it behind
 * an interface any area could reach would make it one an extension could make.
 */
export function useCompositionContext(): Composition {
  const value = useContext(Context);
  if (value === null) {
    throw new Error("The catalogue was rendered outside a project window.");
  }
  return value;
}
