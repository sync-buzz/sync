"use client";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Check, ChevronDown, Folder } from "lucide-react";

import { KindGlyph } from "@/components/shell/entity-marks";
import type { InstalledExtension } from "@/lib/extension-host/client";
import { usePackagesState } from "@/lib/extension-host/packages";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import {
  chooseFolder,
  initializeRepository,
  loadProjectSettings,
  loadRecentProjects,
  probeFolder,
  recordRecentProject,
  forgetProject,
  registerProject,
  saveProjectSettings,
  suggestProjectIdentifier,
} from "@/lib/project/client";
import { fetchMemory, memoryPresence, setMemoryRemote } from "@/lib/memory/client";
import type { MemoryPresence } from "@/lib/memory/types";
import {
  DEFAULT_LANGUAGE_ID,
  PROJECT_LANGUAGES,
  asLanguageId,
  type FolderProbe,
  type OpenProject,
  type ProjectLanguageId,
  type RecentProject,
  type RegisteredProject,
} from "@/lib/project/types";

/**
 * Opening a folder as a project.
 *
 * The flow asks as little as it can get away with, in the order the questions
 * can be answered at all:
 *
 * 1. **Can this folder hold a project?** Sync keeps knowledge in Git, so a
 *    folder outside version control is offered a repository and, if that is
 *    declined, is not opened.
 * 2. **Has it been opened before?** A repository whose memory already carries a
 *    project record answers for itself, and the flow ends here without showing
 *    a single field. Re-asking would be the application forgetting.
 * 3. **Is its memory somewhere else?** A repository with no memory of its own
 *    is asked of its remote before it is asked of a person. `git clone` copies
 *    no `refs/memory/*`, so a clone of a project with years of memory looks
 *    exactly like a project that never had any — and describing the first of
 *    them writes a `project` record that can never merge with the one already
 *    on the remote. That is not a lost keystroke; it is a clone that can never
 *    be reunited with its own memory.
 * 4. **What is it?** Name, an optional description, and the language it writes
 *    its knowledge in — asked once, for a project that has never existed.
 * 5. **What can it do?** The extension marketplace.
 *
 * The answers are written to the project's own memory. Opening a repository
 * that has never held any declares where that memory goes — `refs`, the Git
 * objects that travel with the repository — and the first transaction is what
 * makes `refs/memory/*` exist. Nothing about a project is kept on this machine
 * except the list of which ones were opened recently, which is the one fact
 * that genuinely belongs to the installation rather than to any project.
 *
 * The controller is separate from the sheet so the shell can start the flow
 * from three places — the empty window, its recent list, and the project
 * switcher — without any of them owning its state.
 */

type Stage =
  | "closed"
  | "repository"
  | "memory"
  | "details"
  | "extensions"
  | "collision";

/** A project being described. It is an `OpenProject` that is not open yet. */
type Draft = OpenProject;

/**
 * The flow, as one object.
 *
 * The shell uses `begin`, `open`, `recent`, `isBusy` and `error` — enough to
 * offer the action, list what was opened before, and report a failure where the
 * action was taken. The rest is the sheet's, and both read it from here rather
 * than from each other.
 */
export interface ProjectSetup {
  readonly stage: Stage;
  /** True while the system is being asked something that takes time. */
  readonly isBusy: boolean;
  /** The last failure, in words, or `null`. Shown wherever the flow is. */
  readonly error: string | null;
  /** The projects this installation has opened, most recent first. */
  readonly recent: readonly RecentProject[];
  /** Ask for a folder and start the flow. */
  readonly begin: () => void;
  /** Start the flow on a folder already known, from the recent list. */
  readonly open: (path: string) => void;
  readonly cancel: () => void;
  readonly probe: FolderProbe | null;
  readonly draft: Draft | null;
  /**
   * Why the project's memory could not be read, when it could not be. The flow
   * asks anyway — there is nothing else it can do — but it says so, because a
   * project that already exists must not be quietly described a second time.
   */
  readonly memoryError: string | null;
  /** True once saving has failed, which turns the last button into a choice. */
  readonly saveFailed: boolean;
  /**
   * Where this project's memory is, when it is not here. Only ever set to the
   * one state worth stopping for: memory that exists on a remote and has not
   * arrived.
   */
  readonly presence: MemoryPresence | null;
  /** True once fetching has failed, which turns the last button into a choice. */
  readonly fetchFailed: boolean;
  readonly initialize: () => void;
  /** Bring the memory that is on the remote here, and open what arrives. */
  readonly fetchExisting: () => void;
  readonly submitDetails: (draft: Draft) => void;
  readonly backToDetails: () => void;
  /**
   * Add or remove one extension from what the project will be composed of.
   *
   * Nothing is chosen for a person here. A tick they did not put there is a
   * decision made on the fourth screen of a product they have not used yet;
   * the catalogue marks what we would pick and leaves the picking to them.
   */
  readonly toggleExtension: (id: string) => void;
  /**
   * The packages this machine can offer, which is the whole of what a new
   * project may be composed of.
   *
   * There is no registry yet, so a fresh machine has nothing here and the step
   * says so. That is the honest state rather than a defect: everything a
   * project can do arrives as a package, and until one is unpacked there is
   * nothing to choose between.
   */
  readonly available: readonly InstalledExtension[];
  readonly finish: () => void;
  /**
   * The project that could not be registered, and the one holding the name it
   * wanted. Present only at the `collision` stage.
   */
  readonly collision: Collision | null;
  /** Answer for the project under `identifier` on this machine, and open it. */
  readonly settleCollision: (identifier: string) => void;
  /** Take a project out of the menu, and stop answering for it. */
  readonly forget: (path: string) => void;
}

/** A project whose identifier already belongs to another project here. */
export interface Collision {
  readonly project: OpenProject;
  readonly takenBy: RegisteredProject;
}

export function useProjectSetup({
  onOpened,
}: {
  onOpened: (project: OpenProject) => void;
}): ProjectSetup {
  const [stage, setStage] = useState<Stage>("closed");
  const [probe, setProbe] = useState<FolderProbe | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  // Read here rather than in the step, because the toggle needs it too: what a
  // project declares is `{id, version, integrity, source}`, and every one of
  // those comes off the package rather than out of this build.
  const packages = usePackagesState();
  const [memoryError, setMemoryError] = useState<string | null>(null);
  const [saveFailed, setSaveFailed] = useState(false);
  const [presence, setPresence] = useState<MemoryPresence | null>(null);
  const [fetchFailed, setFetchFailed] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recent, setRecent] = useState<readonly RecentProject[]>([]);

  // Outside Tauri there is no command layer to ask, and an empty list is the
  // truthful answer rather than an error worth showing anyone.
  useEffect(() => {
    void loadRecentProjects().then(setRecent, () => undefined);
  }, []);

  const cancel = useCallback(() => {
    setStage("closed");
    setProbe(null);
    setDraft(null);
    setMemoryError(null);
    setSaveFailed(false);
    setPresence(null);
    setFetchFailed(false);
    setError(null);
  }, []);

  const [collision, setCollision] = useState<Collision | null>(null);

  /**
   * Open a project, once this installation answers for it.
   *
   * Registering comes first because it can stop: two repositories can derive
   * the same identifier, and only a person can say which of them answers to it
   * here. A registry that could not be *written* is a different matter — the
   * window works either way, and the next open writes it — so that failure is
   * swallowed the way the recent list's is.
   */
  const openProject = useCallback(
    async (project: OpenProject, answersAs?: string) => {
      const identifier = answersAs ?? project.identifier;
      const registration = await registerProject({
        path: project.path,
        name: project.name,
        identifier,
      }).catch(() => null);
      if (registration?.takenBy) {
        setCollision({ project, takenBy: registration.takenBy });
        setStage("collision");
        return;
      }
      // A recent list that could not be written costs nobody anything: the
      // project is open either way, and the menu is one item shorter than it
      // could have been.
      await recordRecentProject({ path: project.path, name: project.name }).then(
        setRecent,
        () => undefined,
      );
      setCollision(null);
      onOpened(project);
      cancel();
    },
    [cancel, onOpened],
  );

  const forget = useCallback((path: string) => {
    // A list that could not be rewritten leaves the project where it was, which
    // the person can see for themselves — there is nothing here to explain that
    // the unchanged list does not already say.
    void forgetProject(path).then(setRecent, () => undefined);
  }, []);

  const settleCollision = useCallback(
    (identifier: string) => {
      if (!collision) return;
      void openProject(collision.project, identifier);
    },
    [collision, openProject],
  );

  /**
   * Everything that happens once a folder is named, whether it was chosen from
   * the open panel, taken from the recent list, or just made a repository.
   *
   * Opening a folder inside a repository opens the repository: the project is
   * the work tree, and its name comes from there rather than from whichever
   * subdirectory was pointed at.
   */
  const continueWith = useCallback(
    async (path: string) => {
      const chosen = await probeFolder(path);
      setProbe(chosen);

      if (chosen.repositoryRoot === null) {
        setStage("repository");
        return;
      }

      const root =
        chosen.repositoryRoot === chosen.path
          ? chosen
          : await probeFolder(chosen.repositoryRoot);

      const known = await loadProjectSettings(root.path);
      if (known.settings) {
        await openProject({
          path: root.path,
          name: known.settings.name,
          identifier: known.settings.identifier,
          description: known.settings.description,
          language: asLanguageId(known.settings.language),
          installed: known.settings.installed,
        });
        return;
      }

      setMemoryError(known.memoryError);
      setDraft({
        path: root.path,
        name: root.name,
        // Derived once the form is on screen, from whatever the name ends up
        // being. Empty here rather than derived twice.
        identifier: "",
        description: "",
        language: DEFAULT_LANGUAGE_ID,
        installed: [],
      });

      // Before describing a project, find out whether it already exists
      // somewhere. Only one answer is worth stopping for: memory that is on a
      // remote and has not arrived. "There is none anywhere" is how every
      // project starts, and "nobody could say" must not hold up a person who
      // is offline — both go straight on to the questions, which is what they
      // did before this step existed.
      const found = await memoryPresence(root.path).catch(() => null);
      if (found?.state === "not_fetched") {
        setPresence(found);
        setStage("memory");
        return;
      }

      setStage("details");
    },
    [openProject],
  );

  /** Run one step of the flow, with its waiting and its failure handled once. */
  const attempt = useCallback((step: () => Promise<void>) => {
    setError(null);
    setIsBusy(true);
    void (async () => {
      try {
        await step();
      } catch (failure) {
        setError(messageOf(failure));
      } finally {
        setIsBusy(false);
      }
    })();
  }, []);

  const begin = useCallback(() => {
    attempt(async () => {
      const chosen = await chooseFolder();
      // Dismissing the open panel is an outcome, not a failure.
      if (chosen === null) return;
      await continueWith(chosen);
    });
  }, [attempt, continueWith]);

  const open = useCallback(
    (path: string) => attempt(() => continueWith(path)),
    [attempt, continueWith],
  );

  const initialize = useCallback(() => {
    if (!probe) return;
    attempt(async () => {
      const repository = await initializeRepository(probe.path);
      await continueWith(repository.path);
    });
  }, [attempt, continueWith, probe]);

  /**
   * Bring the memory that is on the remote here.
   *
   * A clone knows the address as its code `origin` and not as a memory remote,
   * so the first thing this does is make it one — memory has a remote of its
   * own precisely so it does not have to follow the code's, and a fetch has
   * nowhere to fetch from until one is configured.
   *
   * What arrives usually answers the rest of the flow: memory carrying a
   * project record is a project that has already been described, so it opens
   * rather than asking again. Memory written by an agent and never opened in
   * Sync has records and no project record, and that falls through to the
   * questions — which is the truth about it.
   *
   * A failure follows the rule the last screen already follows: it reports
   * itself and stops, and pressing again describes the project as a new one.
   * Somebody without access to the remote is not trapped, and they are not
   * quietly diverged from it either — they are told, and then they decide.
   */
  const fetchExisting = useCallback(() => {
    if (!draft || presence?.state !== "not_fetched") return;
    const project = draft;

    if (fetchFailed) {
      setStage("details");
      return;
    }

    setError(null);
    setIsBusy(true);
    void (async () => {
      try {
        if (!presence.configured) {
          await setMemoryRemote(project.path, presence.url);
        }
        await fetchMemory(project.path);
        const known = await loadProjectSettings(project.path);
        if (known.settings) {
          await openProject({
            path: project.path,
            name: known.settings.name,
            identifier: known.settings.identifier,
            description: known.settings.description,
            language: asLanguageId(known.settings.language),
            installed: known.settings.installed,
          });
          return;
        }
        setMemoryError(known.memoryError);
        setStage("details");
      } catch (failure) {
        setFetchFailed(true);
        setError(messageOf(failure));
      } finally {
        setIsBusy(false);
      }
    })();
  }, [draft, fetchFailed, openProject, presence]);

  const submitDetails = useCallback((next: Draft) => {
    setDraft(next);
    setStage("extensions");
  }, []);

  const backToDetails = useCallback(() => setStage("details"), []);

  /**
   * Write what was asked, then open.
   *
   * A project whose settings cannot be stored will be asked about again next
   * time, which is worth saying before it happens rather than after. So the
   * first failure reports itself and stops; pressing the button a second time
   * opens the project anyway, which is a decision rather than a silent
   * fallback.
   */
  const toggleExtension = useCallback((id: string) => {
    setDraft((current) => {
      if (!current) return current;
      const chosen = current.installed.some((entry) => entry.id === id);
      if (chosen) {
        return {
          ...current,
          installed: current.installed.filter((entry) => entry.id !== id),
        };
      }
      // Read off the package at the moment of choosing: what gets declared is
      // what this machine actually holds, down to the digest — not a version
      // number this build happens to know.
      const packaged = packages.byId(id);
      if (packaged === null) return current;
      return {
        ...current,
        installed: [
          ...current.installed,
          {
            id,
            version: packaged.manifest.version,
            prompt: packaged.prompt ?? undefined,
            integrity: packaged.pointer.integrity ?? undefined,
            source: packaged.pointer.source,
          },
        ],
      };
    });
  }, [packages]);

  const finish = useCallback(() => {
    if (!draft) return;
    const project = draft;

    if (saveFailed) {
      attempt(() => openProject(project));
      return;
    }

    setError(null);
    setIsBusy(true);
    void (async () => {
      try {
        await saveProjectSettings(project.path, {
          name: project.name,
          identifier: project.identifier,
          description: project.description,
          language: project.language,
          installed: project.installed,
        });
        await openProject(project);
      } catch (failure) {
        setSaveFailed(true);
        setError(messageOf(failure));
      } finally {
        setIsBusy(false);
      }
    })();
  }, [attempt, draft, openProject, saveFailed]);

  return {
    stage,
    isBusy,
    error,
    recent,
    begin,
    open,
    cancel,
    probe,
    draft,
    memoryError,
    saveFailed,
    presence,
    fetchFailed,
    initialize,
    fetchExisting,
    submitDetails,
    backToDetails,
    toggleExtension,
    available: packages.all,
    finish,
    collision,
    settleCollision,
    forget,
  };
}

function messageOf(failure: unknown): string {
  return failure instanceof Error
    ? failure.message
    : "The folder could not be opened.";
}

export function ProjectSetupSheet({ setup }: { setup: ProjectSetup }) {
  return (
    <Sheet
      open={setup.stage !== "closed"}
      onOpenChange={(open) => {
        if (!open) setup.cancel();
      }}
    >
      <SheetContent aria-describedby="project-setup-lead">
        <SheetHeader>
          <SheetTitle>Open a project</SheetTitle>
        </SheetHeader>

        {setup.stage === "collision" && setup.collision ? (
          <CollisionStep setup={setup} collision={setup.collision} />
        ) : null}

        {setup.stage === "repository" && setup.probe ? (
          <RepositoryStep setup={setup} probe={setup.probe} />
        ) : null}
        {setup.stage === "memory" &&
        setup.presence?.state === "not_fetched" ? (
          <MemoryStep setup={setup} presence={setup.presence} />
        ) : null}
        {setup.stage === "details" && setup.draft ? (
          <DetailsStep setup={setup} draft={setup.draft} />
        ) : null}
        {setup.stage === "extensions" && setup.draft ? (
          <ExtensionsStep setup={setup} draft={setup.draft} />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/**
 * The memory this project already has, somewhere else.
 *
 * This is the one moment where memory living outside the working tree stops
 * being a design property and becomes a defect: the claim that memory travels
 * with the repository reads as false exactly when somebody first checks it,
 * because `git clone` copies no `refs/memory/*` and no option makes it.
 *
 * The screen states what is true and offers the one thing worth doing about
 * it. Declining is Cancel, as it is on the repository screen, and for the same
 * reason: the alternative to fetching is describing this project a second
 * time, and that is not a smaller version of opening it — it is a clone that
 * can never be reunited with its own memory. Once fetching has actually been
 * tried and refused, it becomes a choice the person is entitled to make, which
 * is why the button changes rather than sitting there repeating itself.
 */
function MemoryStep({
  setup,
  presence,
}: {
  setup: ProjectSetup;
  presence: Extract<MemoryPresence, { state: "not_fetched" }>;
}) {
  return (
    <>
      <StepBody>
        <div className="space-y-2">
          <h3 className="text-lg font-medium text-fg">
            This project already has memory, and it is not here yet.
          </h3>
          <SheetDescription id="project-setup-lead">
            A clone copies branches and tags. It does not copy the refs Sync
            keeps a project&rsquo;s knowledge in, so what this project has
            decided, constrained and specified is still on its remote.
          </SheetDescription>
          <p className="break-all font-mono text-xs text-fg-tertiary">
            {presence.url}
          </p>
          <p className="text-xs text-fg-tertiary">
            {presence.configured
              ? "Fetching merges what the remote holds into this repository. Nothing in the working tree is touched."
              : "Fetching sets this address as the project\u2019s memory remote and merges what it holds. Nothing in the working tree is touched, and nothing is published."}
          </p>
        </div>

        {setup.fetchFailed ? (
          <p className="text-xs text-fg-tertiary">
            The memory could not be fetched. Describing this project instead
            makes a second, separate memory for it: what is on the remote stays
            there, and the two cannot be merged afterwards.
          </p>
        ) : null}

        <ErrorNote message={setup.error} />
      </StepBody>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={setup.cancel}>
          Cancel
        </Button>
        {/* Wide enough for the longest label it shows, so the row does not
            resize under the pointer while the network is being waited on. */}
        <Button
          onClick={setup.fetchExisting}
          disabled={setup.isBusy}
          className="min-w-52"
        >
          {setup.isBusy
            ? "Fetching\u2026"
            : setup.fetchFailed
              ? "Describe as a New Project"
              : "Fetch Memory"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * The precondition, stated before it is asked about.
 *
 * Declining is a real answer and it ends the flow, so the reason has to be on
 * this screen rather than in a second one that appears after the refusal. A
 * person who reads this and still says no has decided, and being told again
 * would be an argument, not an interface.
 */
function RepositoryStep({
  setup,
  probe,
}: {
  setup: ProjectSetup;
  probe: FolderProbe;
}) {
  return (
    <>
      <StepBody>
        <FolderLine path={probe.path} />

        <div className="space-y-2">
          <h3 className="text-lg font-medium text-fg">
            This folder is not a Git repository.
          </h3>
          <SheetDescription id="project-setup-lead">
            Sync keeps what a project knows in its own repository, so decisions,
            constraints and specifications are versioned with the code and
            travel with it. Without a repository there is nowhere to put any of
            that, and the folder cannot be opened as a project.
          </SheetDescription>
          <p className="text-xs text-fg-tertiary">
            Initializing runs <span className="font-mono">git init</span> in the
            folder. Nothing is committed and nothing already there is changed.
          </p>
        </div>

        <ErrorNote message={setup.error} />
      </StepBody>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={setup.cancel}>
          Cancel
        </Button>
        {/* Wide enough for the longest label it shows. The label changes while
            git runs, and a button that resizes drags its neighbour sideways
            with it — under the pointer, and leaving the webview repainting a
            strip of the row that has already moved. */}
        <Button
          onClick={setup.initialize}
          disabled={setup.isBusy}
          className="min-w-44"
        >
          {setup.isBusy ? "Initializing…" : "Initialize Repository"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * Two projects, one name, one machine.
 *
 * Both identifiers are right where they came from — derived from names their
 * own people chose — and neither record is edited here. What is decided is
 * narrower and local: which of the two an agent on *this* machine means when it
 * types that word. The other project keeps it, this one answers to something
 * else, and everything either repository has ever written goes on saying what
 * it said.
 */
function CollisionStep({
  setup,
  collision,
}: {
  setup: ProjectSetup;
  collision: Collision;
}) {
  const [identifier, setIdentifier] = useState(collision.project.identifier);
  const derivation = useRef(0);

  const derive = useCallback((from: string) => {
    const asked = ++derivation.current;
    void suggestProjectIdentifier(from).then((derived) => {
      if (derivation.current === asked) setIdentifier(derived);
    });
  }, []);

  const isTaken = identifier === collision.takenBy.identifier;

  return (
    <>
      <StepBody>
        <FolderLine path={collision.project.path} />
        <SheetDescription id="project-setup-lead">
          {collision.project.name} answers to{" "}
          <strong className="font-medium text-fg-primary">
            {collision.project.identifier}
          </strong>
          , and so does another project on this machine. Give this one a
          different name to be called by here.
        </SheetDescription>

        <div className="space-y-1 rounded-(--radius-control) border border-separator-strong bg-panel p-2.5">
          <p className="text-xs font-medium text-fg-primary">
            {collision.takenBy.identifier} is already taken
          </p>
          <p className="text-xs text-fg-secondary">
            {collision.takenBy.name} — {collision.takenBy.path}
          </p>
        </div>

        <div className="space-y-3.5">
          <Field
            label="Called here"
            htmlFor="project-local-identifier"
            hint="Only this machine changes. The project keeps the name it was created with, so every document naming it still means the same project."
          >
            <input
              id="project-local-identifier"
              autoFocus
              value={identifier}
              onChange={(event) => derive(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && identifier && !isTaken) {
                  setup.settleCollision(identifier);
                }
              }}
              spellCheck={false}
              className={FIELD_CONTROL}
            />
          </Field>

          {identifier && !isTaken ? (
            <p className="text-xs text-fg-tertiary">
              Agents on this machine will mean {collision.project.name} when
              they say {identifier}.
            </p>
          ) : null}
        </div>

        <ErrorNote message={setup.error} />
      </StepBody>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={setup.cancel}>
          Cancel
        </Button>
        <Button
          onClick={() => setup.settleCollision(identifier)}
          disabled={!identifier || isTaken}
        >
          Register and open
        </Button>
      </SheetFooter>
    </>
  );
}

/** Name, description and language: what the project is, in its own words. */
function DetailsStep({
  setup,
  draft,
}: {
  setup: ProjectSetup;
  draft: Draft;
}) {
  const [name, setName] = useState(draft.name);
  const [description, setDescription] = useState(draft.description);
  const [language, setLanguage] = useState<ProjectLanguageId>(draft.language);
  const [identifier, setIdentifier] = useState(draft.identifier);
  // Whether the person has taken the field over. Until they do it follows the
  // name; once they have, the name stops writing over what they typed.
  const [identifierIsTheirs, setIdentifierIsTheirs] = useState(
    draft.identifier !== "",
  );
  // Which derivation is the current one. The rule is asked for over IPC, so two
  // keystrokes are two answers in flight, and the one that comes back last is
  // not necessarily the one that was asked last.
  const derivation = useRef(0);

  const derive = useCallback((from: string) => {
    const asked = ++derivation.current;
    void suggestProjectIdentifier(from).then((derived) => {
      if (derivation.current === asked) setIdentifier(derived);
    });
  }, []);

  // The name arrives already filled in from the folder, so the identifier it
  // implies is derived when the step appears rather than on the first
  // keystroke — a field that is blank until touched reads as one that is
  // optional.
  useEffect(() => {
    if (!identifierIsTheirs) derive(draft.name);
    // Once, for the name this step opened with.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const trimmedName = name.trim();

  function submit() {
    if (!trimmedName || !identifier) return;
    setup.submitDetails({
      path: draft.path,
      name: trimmedName,
      identifier,
      description: description.trim(),
      language,
      installed: draft.installed,
    });
  }

  return (
    <>
      <StepBody>
        <FolderLine path={draft.path} />
        <SheetDescription id="project-setup-lead">
          The project is new. Its name, description and language are stored in
          the repository at that path, alongside everything else it comes to
          know, so it is only asked once.
        </SheetDescription>

        {setup.memoryError ? (
          <div className="space-y-1 rounded-(--radius-control) border border-separator-strong bg-panel p-2.5">
            <p className="text-xs font-medium text-warning">
              Project memory could not be read
            </p>
            {/* The engine's own words, then what they mean here. A project
                that already exists must not be quietly described again. */}
            <p className="text-xs text-fg-secondary">{setup.memoryError}</p>
            <p className="text-xs text-fg-tertiary">
              If this repository was already a Sync project, continuing
              describes it a second time.
            </p>
          </div>
        ) : null}

        <div className="space-y-3.5">
          <Field label="Name" htmlFor="project-name">
            <input
              id="project-name"
              autoFocus
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                if (!identifierIsTheirs) derive(event.target.value);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") submit();
              }}
              maxLength={64}
              className={FIELD_CONTROL}
            />
          </Field>

          {/* Written once. Every mention of this project anywhere — an
              agent's call, a document naming a neighbour — is this word, and
              nothing later can move it, so it is shown while it can still be
              changed rather than after. Typed characters go through the same
              rule that derives it, so a field that accepts what was typed has
              already accepted what will be stored. */}
          <Field label="Identifier" htmlFor="project-identifier">
            <input
              id="project-identifier"
              value={identifier}
              onChange={(event) => {
                setIdentifierIsTheirs(true);
                derive(event.target.value);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") submit();
              }}
              spellCheck={false}
              className={FIELD_CONTROL}
            />
          </Field>

          <Field
            label="Description"
            htmlFor="project-description"
            optional
            hint="One line about what this project is. It is shown wherever the project is listed."
          >
            <textarea
              id="project-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              rows={2}
              maxLength={280}
              className={cn(FIELD_CONTROL, "h-auto resize-none py-1.5 leading-5")}
            />
          </Field>

          <Field
            label="Language"
            htmlFor="project-language"
            hint="The language this project writes its knowledge in. It belongs to the project, not to the person reading it."
          >
            <div className="relative">
              <select
                id="project-language"
                value={language}
                onChange={(event) =>
                  setLanguage(event.target.value as ProjectLanguageId)
                }
                className={cn(FIELD_CONTROL, "cursor-default appearance-none pr-8")}
              >
                {PROJECT_LANGUAGES.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
              <ChevronDown
                aria-hidden="true"
                className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-fg-tertiary"
              />
            </div>
          </Field>
        </div>

        <ErrorNote message={setup.error} />
      </StepBody>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={setup.cancel}>
          Cancel
        </Button>
        <Button onClick={submit} disabled={!trimmedName}>
          Continue
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * The marketplace, at the size it currently is.
 *
 * Only what can be installed is listed, which now means only what is unpacked
 * on this machine. A card for something a person cannot choose is a way of
 * saying no to a question they asked in good faith, and this is the screen
 * where they are asking it.
 *
 * A machine with nothing unpacked shows nothing to pick, and says so in a
 * sentence rather than with an empty grid. That is where everybody starts until
 * the registry exists, and a project composed of nothing is a real project —
 * it opens on the catalogue, which is where the packages are added.
 */
function ExtensionsStep({
  setup,
  draft,
}: {
  setup: ProjectSetup;
  draft: Draft;
}) {
  return (
    <>
      <StepBody>
        <SheetDescription id="project-setup-lead">
          Extensions decide what Sync can do in {draft.name}. Nothing is
          chosen for you — a project with none opens on this list, and anything
          here can be added later.
        </SheetDescription>

        {setup.available.length === 0 ? (
          <p className="text-sm leading-5 text-fg-tertiary">
            Nothing is unpacked yet, so there is nothing to choose
            between. Open the project and add a package from a file or a folder
            in Extensions — the project is composed there just as well as here.
          </p>
        ) : (
          <ul className="grid grid-cols-2 gap-2">
            {setup.available.map((packaged) => (
              <ExtensionCard
                key={packaged.manifest.id}
                packaged={packaged}
                chosen={draft.installed.some(
                  (entry) => entry.id === packaged.manifest.id,
                )}
                onToggle={() => setup.toggleExtension(packaged.manifest.id)}
              />
            ))}
          </ul>
        )}

        <ErrorNote message={setup.error} />
      </StepBody>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button
          variant="outline"
          onClick={setup.backToDetails}
          disabled={setup.isBusy}
        >
          Back
        </Button>
        {/* Three labels, one width. Wide enough for "Open Anyway", which is the
            longest of them, so pressing the button never moves "Back". */}
        <Button
          onClick={setup.finish}
          disabled={setup.isBusy}
          className="min-w-32"
        >
          {setup.isBusy
            ? "Opening…"
            : setup.saveFailed
              ? "Open Anyway"
              : "Open Project"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * One extension, as a choice rather than as a report.
 *
 * The whole card is the control: a tick box beside a card that is also
 * clickable gives two targets for one decision, and on a grid of cards the
 * second one is the one people miss.
 *
 * Nothing is chosen in advance, and nothing is recommended any more. The shell
 * used to mark three cards `Recommended`, which it could do because it had
 * written all three; it cannot recommend a package it has never seen, and a
 * mark that only ever appeared on our own extensions would be the catalogue
 * saying which authors it trusts.
 */
function ExtensionCard({
  packaged,
  chosen,
  onToggle,
}: {
  packaged: InstalledExtension;
  chosen: boolean;
  onToggle: () => void;
}) {
  const development = packaged.pointer.source === "folder";

  return (
    <li className="min-w-0">
      <button
        type="button"
        aria-pressed={chosen}
        onClick={onToggle}
        data-chosen={chosen}
        className="flex h-full w-full flex-col gap-2 rounded-(--radius-surface) border border-separator bg-panel/60 p-3 text-left transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-panel data-[chosen=false]:opacity-70 data-[chosen=true]:border-separator-strong data-[chosen=true]:bg-panel data-[chosen=true]:opacity-100"
      >
        <div className="flex items-start gap-2">
          <span
            aria-hidden="true"
            className="flex size-7 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary"
          >
            <KindGlyph icon={packaged.manifest.icon} className="size-4" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="truncate text-base font-medium text-fg">
              {packaged.manifest.name}
            </p>
            <p className="flex items-center gap-1 text-xs text-fg-tertiary">
              {chosen ? (
                <>
                  <Check aria-hidden="true" className="size-3 shrink-0" />
                  Chosen
                </>
              ) : development ? (
                "Development"
              ) : (
                "Not chosen"
              )}
            </p>
          </div>
        </div>
        <p className="text-xs leading-4 text-fg-tertiary">
          {packaged.manifest.summary}
        </p>
      </button>
    </li>
  );
}

/**
 * Every text control in the sheet is the same control. macOS text fields are a
 * hairline and a field colour, and the one focus indicator in `globals.css`
 * takes care of the rest.
 */
export const FIELD_CONTROL =
  "h-(--control-height-lg) w-full rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-base text-fg placeholder:text-fg-tertiary";

export function Field({
  label,
  htmlFor,
  hint,
  optional,
  children,
}: {
  label: string;
  htmlFor: string;
  hint?: string;
  optional?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <label
        htmlFor={htmlFor}
        className="flex items-baseline gap-1.5 text-sm font-medium text-fg-secondary"
      >
        {label}
        {optional ? (
          <span className="text-xs font-normal text-fg-tertiary">Optional</span>
        ) : null}
      </label>
      {children}
      {hint ? <p className="text-xs text-fg-tertiary">{hint}</p> : null}
    </div>
  );
}

export function StepBody({ children }: { children: ReactNode }) {
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="space-y-4 p-4">{children}</div>
    </ScrollArea>
  );
}

/** The folder under discussion, in the one place every step shows it. */
function FolderLine({ path }: { path: string }) {
  return (
    <p className="flex items-center gap-1.5 text-xs text-fg-tertiary">
      <Folder aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="truncate font-mono" title={path}>
        {path}
      </span>
    </p>
  );
}

export function ErrorNote({ message }: { message: string | null }) {
  if (!message) return null;

  return (
    <p role="alert" className="text-xs font-medium text-danger">
      {message}
    </p>
  );
}
