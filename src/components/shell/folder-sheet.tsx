"use client";

import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";

/**
 * The one question a folder asks, making it or renaming it: what is it called.
 *
 * A sheet rather than a row that appears already named. Finder makes an
 * "untitled folder" and puts the name in edit mode, which works because the
 * name is editable in place a moment later; here it is not yet, and a folder
 * called "untitled folder" that cannot be renamed is worse than being asked.
 *
 * Where it goes is not asked, because it has already been answered: the command
 * came from a row, and that row is the parent. A sheet re-asking would be the
 * window forgetting what somebody just pointed at.
 */
export function FolderSheet({
  open,
  onOpenChange,
  parent,
  renaming,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /**
   * Where the new folder goes, repository-relative. The empty string is the
   * project's root, which is where a type keeping its documents in its own
   * records starts.
   */
  parent: string;
  /**
   * The folder's current name, when this is a rename rather than a new folder.
   *
   * The same sheet either way, because it is the same question. What differs is
   * what the field starts with and what the button says — and a second sheet
   * asking the same thing would be a second place to keep the rules in.
   */
  renaming?: string;
  /** Answers when the folder has been written, or throws what refused it. */
  onSubmit: (name: string) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="folder-sheet-lead">
        <SheetHeader>
          <SheetTitle>{renaming ? "Rename folder" : "New folder"}</SheetTitle>
        </SheetHeader>
        {/* Mounted only while it is open, so each visit starts empty rather
            than from the name the last one was given. */}
        {open ? (
          <FolderForm
            parent={parent}
            renaming={renaming}
            onSubmit={onSubmit}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function FolderForm({
  parent,
  renaming,
  onSubmit,
  onDone,
}: {
  parent: string;
  renaming?: string;
  onSubmit: (name: string) => Promise<void>;
  onDone: () => void;
}) {
  const [name, setName] = useState(renaming ?? "");
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const trimmed = name.trim();
  // A name and not a path. Somebody typing `guides/api` means two folders, and
  // making one called that would put a slash in a segment — which the engine
  // refuses, later, in words about a locator.
  const isPath = trimmed.includes("/");
  const canSubmit =
    trimmed !== "" && !isPath && !isBusy && trimmed !== renaming;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    try {
      await onSubmit(trimmed);
      onDone();
    } catch (refused) {
      setFailure(refused instanceof Error ? refused.message : String(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-3">
        <SheetDescription id="folder-sheet-lead">
          {parent === ""
            ? "At the top of this type."
            : `Inside ${parent}.`}
          {renaming
            ? " Everything filed under it moves with it, and no link breaks."
            : null}
        </SheetDescription>

        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="folder-name"
            className="text-sm font-medium text-fg-secondary"
          >
            Name
          </label>
          <input
            id="folder-name"
            autoFocus
            value={name}
            onChange={(event) => {
              setName(event.target.value);
              setFailure(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submit();
            }}
            maxLength={60}
            placeholder="guides"
            onFocus={(event) => event.currentTarget.select()}
            className="h-(--control-height-lg) w-full rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-base text-fg placeholder:text-fg-tertiary"
          />
          {isPath ? (
            <p className="text-xs text-fg-tertiary">
              One folder at a time — a name rather than a path.
            </p>
          ) : null}
          {failure ? <p className="text-xs text-warning">{failure}</p> : null}
        </div>
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          onClick={() => void submit()}
          disabled={!canSubmit}
          className="min-w-28"
        >
          {renaming
            ? isBusy
              ? "Renaming…"
              : "Rename folder"
            : isBusy
              ? "Making…"
              : "Make folder"}
        </Button>
      </SheetFooter>
    </>
  );
}
