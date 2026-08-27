"use client";

import { useState, type KeyboardEvent } from "react";
import { Database, FolderOpen, type LucideIcon } from "lucide-react";

import { DEFAULT_ICON, KIND_ICON } from "@/components/shell/entity-marks";
import { ErrorNote } from "@/components/shell/project-setup";
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
import type { TypeDefinition } from "@/lib/memory/client";
import { isAttachedType, type MemoryType } from "@/lib/memory/types";
import {
  generatedIdentifier,
  identifierFrom,
} from "@/lib/memory/type-identifier";
import { chooseProjectFolder } from "@/lib/project/client";

/**
 * Naming a type the project can then speak in, and saying where its records
 * live.
 *
 * A type is the one thing in this window that changes what the project is
 * *able* to say: the engine validates every record against the corpus, so a
 * kind that has no definition is a kind that cannot be written. That is why
 * this is a sheet rather than an inline row — it configures the project, and
 * the shell has one kind of modal for exactly that.
 *
 * Two questions, so two panes. **What the type is** — its name, what it is for,
 * the mark it is drawn with. **Where its records live** — which storage engine
 * holds them, and whatever that engine needs to be told. They are separated
 * because they are answered by different people at different times, and because
 * one form holding both is taller than a small window.
 *
 * Storage is chosen when the type is created and not afterwards. That is the
 * engine's rule and it is the right one: where records live is not a setting
 * whose edit can be allowed to move data behind somebody's back. Moving them is
 * an operation with a plan and an acknowledgement — `memory_migrate_storage` —
 * and it is not this form.
 *
 * The identifier is a fourth thing, and it is not a question. It is made from
 * the name when the type is added — lower case, one word — and then it stops
 * moving: every record of the type carries it, the definition's key is built
 * from it, and an agent writes it. It is shown, so it is never a secret, and it
 * is not editable, because the store has no rename.
 *
 * A name the kind alphabet cannot carry is given a generated identifier rather
 * than a refusal — see `lib/memory/type-identifier`. The window asks for a name
 * in the project's own language; what the store needs to key on is the window's
 * problem to solve, not the person's.
 */
export function TypeSheet({
  open,
  onOpenChange,
  editing,
  onSubmit,
  existing,
  projectPath,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The type being redefined, or `null` while one is being named. */
  editing: MemoryType | null;
  onSubmit: (type: TypeDefinition) => Promise<void>;
  /** What the project already holds, so an identifier cannot collide with one. */
  existing: readonly MemoryType[];
  /** Where the repository is, so a folder is chosen from inside it. */
  projectPath: string;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="type-sheet-lead">
        <SheetHeader>
          <SheetTitle>{editing ? "Edit type" : "New type"}</SheetTitle>
        </SheetHeader>
        {/* Mounted only while it is open, so each visit starts from the type it
            was opened for rather than from the last one it was given. */}
        {open ? (
          <TypeForm
            editing={editing}
            onSubmit={onSubmit}
            onDone={() => onOpenChange(false)}
            existing={existing}
            projectPath={projectPath}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/**
 * One storage engine, as the sheet offers it.
 *
 * A list rather than a switch, because the list is going to grow: the engine
 * says so in as many words — a project without Git, an external source — and a
 * form built around two cases would have to be rewritten for the third. A new
 * engine is a row here and a settings block beside it.
 */
interface StoragePlace {
  readonly place: string;
  readonly label: string;
  /** One line, on the card. What this engine *is*, not how to configure it. */
  readonly summary: string;
  readonly icon: LucideIcon;
}

/** A type whose documents are its own records. */
const STORAGE_RECORDS = "records";

/**
 * A type whose documents are files of the working tree.
 *
 * Named apart from `records` on purpose: this pane's own vocabulary, chosen so
 * the value the directory field is checked against can never be confused with
 * the place a type keeps its documents when it keeps them in its records.
 */
const STORAGE_FOLDER = "repo_folder";

const STORAGE_PLACES: readonly StoragePlace[] = [
  {
    place: STORAGE_RECORDS,
    label: "Memory",
    summary:
      "Kept by Sync in the repository's memory refs. Nothing appears in the working tree.",
    icon: Database,
  },
  {
    place: STORAGE_FOLDER,
    label: "Repository folder",
    summary:
      "Ordinary files the team edits and reviews. Sync writes nothing into them.",
    icon: FolderOpen,
  },
];

/** Which pane of the sheet is showing. */
type Pane = "type" | "storage";

function TypeForm({
  editing,
  onSubmit,
  onDone,
  existing,
  projectPath,
}: {
  editing: MemoryType | null;
  onSubmit: (type: TypeDefinition) => Promise<void>;
  onDone: () => void;
  existing: readonly MemoryType[];
  projectPath: string;
}) {
  // What the form opened with, and what "changed" is measured against. A type
  // whose definition names no mark opens on the one it is *drawn* with, so the
  // comparison has to be against that rather than against the absence — or the
  // save button would offer to write a change nobody made and nobody can see.
  const opened = {
    title: editing?.title ?? "",
    description: editing?.description ?? "",
    icon: editing?.icon ?? DEFAULT_ICON,
  };
  const [pane, setPane] = useState<Pane>("type");
  const [name, setName] = useState(opened.title);
  const [description, setDescription] = useState(opened.description);
  const [icon, setIcon] = useState(opened.icon);
  const [place, setPlace] = useState(
    editing && isAttachedType(editing) ? STORAGE_FOLDER : STORAGE_RECORDS,
  );
  const [folder, setFolder] = useState(editing?.storage.folder ?? "");
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The identifier for a name the kind alphabet cannot carry. Decided once, on
  // the way in, and kept: a random word recomputed as somebody types would
  // change under the line that is showing it, and what that line promises is
  // the identifier this type is about to be stored under.
  const [generated] = useState(() =>
    generatedIdentifier(existing.map((type) => type.kind)),
  );

  // An identifier is derived once, when the type is added. After that it is a
  // fact about every record of the type, and the form reports it rather than
  // recomputing it from a name that is now free to change.
  const derived = identifierFrom(name);
  const kind = editing ? editing.kind : derived || generated;
  const collision = existing.find((type) => type.kind === kind);
  const taken = !editing && collision !== undefined;
  const title = name.trim();
  const trimmedFolder = folder.trim().replace(/^\/+|\/+$/g, "");
  const needsFolder = place === STORAGE_FOLDER;
  // Nothing to write is nothing to write. Every save is a commit on the
  // project's memory refs, and one that changes no byte of the definition is a
  // commit saying so. Storage is not compared: it cannot be edited.
  const changed =
    title !== opened.title ||
    description.trim() !== opened.description ||
    icon !== opened.icon;
  // A name is all that is asked for, except where the storage engine needs
  // somewhere to put things. Whatever the name is written in, there is an
  // identifier for it — derived where the alphabet allows and generated where
  // it does not — so a person writing in their own language is never told the
  // window cannot store what they typed.
  const canSubmit =
    !isBusy &&
    title.length > 0 &&
    (needsFolder ? trimmedFolder.length > 0 : true) &&
    (editing ? changed : !taken);

  async function chooseDirectory() {
    setError(null);
    const chosen = await chooseProjectFolder(projectPath);
    if (chosen === null) {
      setError(
        "That folder is outside this repository. A type's folder is a path inside the project, because it travels to everyone who has the repository.",
      );
      return;
    }
    // The repository root is not a documentation folder. Attaching it would
    // make every file in the project — at any depth, of any kind — a document
    // of this type, which is not a folder somebody chose.
    if (chosen === "") {
      setError(
        "The repository root cannot be a type's folder: every file in the project would become a document of this type. Choose a directory inside it.",
      );
      return;
    }
    setFolder(chosen);
    // The folder's own name is the obvious name for the type, and it is only a
    // suggestion: whatever is typed afterwards wins.
    if (name.trim() === "") {
      const leaf = chosen.split("/").filter(Boolean).at(-1) ?? "";
      setName(leaf.charAt(0).toUpperCase() + leaf.slice(1));
    }
  }

  async function submit() {
    if (!canSubmit) return;
    setIsBusy(true);
    setError(null);
    try {
      await onSubmit({
        kind,
        title,
        description: description.trim(),
        icon,
        // A folder is all there is to say, and all the type records: the
        // definition carries the path itself.
        storage: needsFolder ? { folder: trimmedFolder } : {},
      });
      onDone();
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "The type could not be written.",
      );
      // Cleared here and not in a `finally`: a write that succeeded closes the
      // sheet, and clearing the flag on the way out flips the button's label
      // back to "Create type" while the sheet is still animating shut — a
      // change of label is a change of width, and the button beside it moves.
      setIsBusy(false);
    }
  }

  return (
    <>
      <PaneSwitch pane={pane} onSelect={setPane} />

      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-3.5 p-4">
          {pane === "type" ? (
            <div
              role="tabpanel"
              id="type-sheet-pane-type"
              aria-labelledby="type-sheet-tab-type"
              className="space-y-3.5"
            >
              <SheetDescription id="type-sheet-lead">
                {editing
                  ? "A type is what the project is able to say. Changing it changes how every record of this kind is named and drawn — the records themselves are untouched."
                  : "A type is what the project is able to say. The engine validates every record against the corpus, so this is what lets records of this kind be written at all — by you or by an agent."}
              </SheetDescription>

              <div className="space-y-1.5">
                <label
                  htmlFor="type-name"
                  className="text-sm font-medium text-fg-secondary"
                >
                  Name
                </label>
                <input
                  id="type-name"
                  autoFocus
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void submit();
                  }}
                  maxLength={60}
                  placeholder="Open question"
                  className="h-(--control-height-lg) w-full rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-base text-fg placeholder:text-fg-tertiary"
                />
                <Identifier
                  kind={title ? kind : ""}
                  editing={editing !== null}
                  own={collision?.own === true}
                  taken={taken}
                  generated={!editing && title.length > 0 && derived === ""}
                />
              </div>

              <div className="space-y-1.5">
                <label
                  htmlFor="type-description"
                  className="flex items-baseline gap-1.5 text-sm font-medium text-fg-secondary"
                >
                  Description
                  <span className="text-xs font-normal text-fg-tertiary">
                    Optional
                  </span>
                </label>
                <textarea
                  id="type-description"
                  value={description}
                  onChange={(event) => setDescription(event.target.value)}
                  rows={2}
                  maxLength={200}
                  placeholder="Something nobody has settled yet."
                  className="w-full resize-none rounded-(--radius-control) border border-separator-strong bg-workspace px-2 py-1.5 text-base leading-5 text-fg placeholder:text-fg-tertiary"
                />
                <p className="text-xs text-fg-tertiary">
                  Published with the type, so an agent reading the schema learns
                  what the kind is for.
                </p>
              </div>

              <fieldset className="space-y-1.5">
                <legend className="text-sm font-medium text-fg-secondary">
                  Mark
                </legend>
                <div className="flex flex-wrap gap-1">
                  {Object.entries(KIND_ICON).map(([markName, Icon]) => (
                    <button
                      key={markName}
                      type="button"
                      aria-label={markName}
                      aria-pressed={icon === markName}
                      onClick={() => setIcon(markName)}
                      className={cn(
                        "flex size-8 items-center justify-center rounded-(--radius-control) text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover",
                        icon === markName && "bg-selected text-fg",
                      )}
                    >
                      <Icon className="size-4" aria-hidden="true" />
                    </button>
                  ))}
                </div>
              </fieldset>
            </div>
          ) : (
            <div
              role="tabpanel"
              id="type-sheet-pane-storage"
              aria-labelledby="type-sheet-tab-storage"
              className="space-y-3.5"
            >
              <StoragePane
                place={place}
                onPlace={setPlace}
                folder={folder}
                onFolder={setFolder}
                onChooseFolder={() => void chooseDirectory()}
                locked={editing !== null}
              />
            </div>
          )}

          <ErrorNote message={error} />
        </div>
      </ScrollArea>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        {/* Wide enough for the longest thing it says. The label changes while
            the write is in flight, and a button that resizes under the pointer
            drags its neighbour sideways with it. */}
        <Button
          onClick={() => void submit()}
          disabled={!canSubmit}
          className="min-w-28"
        >
          {editing
            ? isBusy
              ? "Saving…"
              : "Save changes"
            : isBusy
              ? "Creating…"
              : "Create type"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * The two panes, as one control.
 *
 * A segmented control rather than a wizard's steps: neither pane is a
 * precondition of the other, both can be answered in either order, and the
 * whole of what is being decided is visible from either side. Steps would say
 * otherwise.
 */
function PaneSwitch({
  pane,
  onSelect,
}: {
  pane: Pane;
  onSelect: (pane: Pane) => void;
}) {
  const panes: readonly { id: Pane; label: string }[] = [
    { id: "type", label: "Type" },
    { id: "storage", label: "Storage" },
  ];

  // One tab stop, and the arrows move within it — what a tab strip does
  // everywhere else on this system, and the reason a keyboard can reach the
  // second pane at all.
  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const current = panes.findIndex((entry) => entry.id === pane);
    const next =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? Math.min(current + 1, panes.length - 1)
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? Math.max(current - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? panes.length - 1
              : current;
    if (next === current) return;
    event.preventDefault();
    onSelect(panes[next].id);
  }

  return (
    <div className="shrink-0 border-b border-separator px-4 py-2.5">
      <div
        role="tablist"
        aria-label="What this sheet is asking about"
        onKeyDown={handleKeyDown}
        className="flex gap-0.5 rounded-(--radius-control) bg-panel p-0.5"
      >
        {panes.map((entry) => (
          <button
            key={entry.id}
            type="button"
            role="tab"
            id={`type-sheet-tab-${entry.id}`}
            aria-selected={pane === entry.id}
            aria-controls={`type-sheet-pane-${entry.id}`}
            tabIndex={pane === entry.id ? 0 : -1}
            onClick={() => onSelect(entry.id)}
            className={cn(
              "h-(--control-height) min-w-0 flex-1 rounded-(--radius-control) text-sm text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:text-fg",
              // Selection is a surface shift and a weight change, and nothing
              // else: no shadow, no fill of its own.
              pane === entry.id && "bg-raised font-medium text-fg",
            )}
          >
            {entry.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * Where this type's records live, and what that engine needs to know.
 *
 * The engines are cards rather than a pop-up because they are not
 * interchangeable values of one field: each is a different bargain about
 * visibility, ownership and review, and the sentence on the card is the part
 * somebody is actually choosing between. A pop-up would hide it behind the one
 * word that fits on a menu row.
 */
function StoragePane({
  place,
  onPlace,
  folder,
  onFolder,
  onChooseFolder,
  locked,
}: {
  place: string;
  onPlace: (place: string) => void;
  folder: string;
  onFolder: (folder: string) => void;
  onChooseFolder: () => void;
  /**
   * True while an existing type is being edited. Storage is then a fact rather
   * than a question: the engine refuses to edit it out from under records, and
   * moving them is an operation with a plan and an acknowledgement.
   */
  locked: boolean;
}) {
  const selected =
    STORAGE_PLACES.find((entry) => entry.place === place) ?? STORAGE_PLACES[0];

  return (
    <>
      <SheetDescription id="type-sheet-lead">
        {locked
          ? "Where this type's records live, as the definition states it. It is not edited here: moving records between engines is an operation of its own, with a plan you accept before anything is written."
          : "Where this type's records live. Chosen once, when the type is created — a setting whose edit moved data would be data loss wearing the clothes of a preference."}
      </SheetDescription>

      <div
        role="radiogroup"
        aria-label="Storage engine"
        onKeyDown={(event) => {
          if (locked) return;
          const current = STORAGE_PLACES.findIndex(
            (entry) => entry.place === place,
          );
          const next =
            event.key === "ArrowRight" || event.key === "ArrowDown"
              ? (current + 1) % STORAGE_PLACES.length
              : event.key === "ArrowLeft" || event.key === "ArrowUp"
                ? (current - 1 + STORAGE_PLACES.length) % STORAGE_PLACES.length
                : current;
          if (next === current) return;
          event.preventDefault();
          onPlace(STORAGE_PLACES[next].place);
        }}
        className="grid grid-cols-2 gap-2"
      >
        {STORAGE_PLACES.map((entry) => (
          <StorageCard
            key={entry.place}
            entry={entry}
            selected={entry.place === place}
            disabled={locked}
            onSelect={() => onPlace(entry.place)}
          />
        ))}
      </div>

      {place === STORAGE_FOLDER ? (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <label
              htmlFor="type-folder"
              className="text-sm font-medium text-fg-secondary"
            >
              Folder
            </label>
            <div className="flex gap-2">
              <input
                id="type-folder"
                value={folder}
                disabled={locked}
                onChange={(event) => onFolder(event.target.value)}
                placeholder="docs"
                className="h-(--control-height-lg) min-w-0 flex-1 rounded-(--radius-control) border border-separator-strong bg-workspace px-2 font-mono text-sm text-fg placeholder:text-fg-tertiary disabled:text-fg-secondary"
              />
              {locked ? null : (
                <Button variant="outline" onClick={onChooseFolder}>
                  Choose
                </Button>
              )}
            </div>
            <p className="text-xs text-fg-tertiary">
              A path inside the repository. Every file below it is a document of
              this type — diagrams and PDFs as well as prose — so one folder
              belongs to one type.
            </p>
          </div>

          {/* Said before it runs, not after. Somebody attaching a folder in
              order to keep their use of Sync to themselves is making a decision
              a later push would undo. */}
          <p className="rounded-(--radius-control) border border-separator bg-panel p-2.5 text-xs leading-5 text-fg-tertiary">
            The files stay untouched — Sync keeps a record beside each one and
            writes no marker into it. Those records live in{" "}
            <span className="font-mono">refs/memory/*</span>, which an ordinary
            clone does not copy and a branch merge does not touch. Pushing
            memory puts them on the remote, where{" "}
            <span className="font-mono">git ls-remote</span> shows them: staying
            unnoticed and syncing between your own machines are not both
            available, and without a push the records live on this machine with
            no backup.
          </p>
        </div>
      ) : (
        <p className="rounded-(--radius-control) border border-separator bg-panel p-2.5 text-xs leading-5 text-fg-tertiary">
          {selected.summary} Nothing else to configure: Sync is the only writer,
          so there is no folder to name and no file anybody else has open.
        </p>
      )}
    </>
  );
}

function StorageCard({
  entry,
  selected,
  disabled,
  onSelect,
}: {
  entry: StoragePlace;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  const Icon = entry.icon;

  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      // One tab stop for the group, as a radio group has: the arrows move the
      // choice within it.
      tabIndex={selected ? 0 : -1}
      disabled={disabled && !selected}
      onClick={onSelect}
      className={cn(
        "flex min-w-0 flex-col gap-1 rounded-(--radius-control) border p-2.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell",
        selected
          ? "border-separator-strong bg-selected"
          : "border-separator hover:bg-hover",
        // The disabled look the shell already has, rather than an opacity of
        // this component's own.
        disabled && !selected && "opacity-50",
      )}
    >
      <span className="flex items-center gap-2">
        <Icon
          className="size-4 shrink-0 text-fg-secondary"
          aria-hidden="true"
        />
        <span className="truncate text-sm font-medium text-fg">
          {entry.label}
        </span>
      </span>
      <span className="text-xs leading-4 text-fg-tertiary">
        {entry.summary}
      </span>
    </button>
  );
}

/**
 * The identifier under the name: what the store will call this kind, and what
 * every record of it carries.
 *
 * Shown rather than hidden, because it is the word an agent writes and the one
 * that appears in the corpus — a person configuring a type should not have to
 * discover it later. Never editable: the store has no rename, so a field
 * offering one would be a rewrite of every record disguised as a text box.
 */
function Identifier({
  kind,
  editing,
  own,
  taken,
  generated,
}: {
  kind: string;
  editing: boolean;
  /** The colliding type is Sync's own, which is a different answer from taken. */
  own: boolean;
  taken: boolean;
  /** The name is written in a script the kind alphabet cannot carry. */
  generated: boolean;
}) {
  if (!kind) {
    return (
      <p className="text-xs text-fg-tertiary">
        The identifier is made from the name: lower case, one word, spaces
        become underscores.
      </p>
    );
  }

  return (
    <p className="text-xs text-fg-tertiary">
      Stored as <span className="font-mono text-fg-secondary">{kind}</span>
      {own
        ? " — Sync's own type, always present."
        : taken
          ? " — the project already holds this type."
          : editing
            ? " — the identifier every record of this type carries. It does not change when the name does."
            : generated
              ? // Said plainly, and said here: an identifier nobody chose is
                // going to turn up in the corpus and in an agent's
                // instructions, and finding it there without ever having been
                // told where it came from is worse than reading one line now.
                " — generated, because this name is written in characters an identifier cannot use. The name is stored as you typed it."
              : " — every record of this type will carry it, and it cannot be changed afterwards."}
    </p>
  );
}
