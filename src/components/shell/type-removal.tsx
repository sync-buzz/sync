"use client";

import { useEffect, useState } from "react";

import { KindMark } from "@/components/shell/entity-marks";
import { ErrorNote } from "@/components/shell/project-setup";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { isAttachedType, type MemoryType } from "@/lib/memory/types";

/**
 * Removing a type, and everything the project wrote as it.
 *
 * The records are not collateral damage — they are the substance of the
 * decision. The engine runs a strict schema, so a record whose kind has no
 * definition is one nothing can read, write or validate; leaving them behind
 * would leave the project holding claims it can no longer open. There is no
 * version of this that removes only the definition, which is why the sheet
 * states the number instead of hiding it behind a phrase like "and its data".
 *
 * **A type over a folder is detached rather than deleted, and the sheet says
 * so in the title, the sentence and the button.** Its records describe files
 * the repository had before Sync was asked about them: removing the type takes
 * what Sync knew and leaves what the team wrote. One word for two operations
 * would be this window promising, in the same sentence, both to delete
 * everything of a type and to leave most of it alone.
 *
 * The number is asked of the store when the sheet opens rather than read off
 * the row behind it. The row shows what the last read found and leaves out
 * whatever this window hides; a sentence that names a count is promising the
 * one that is about to be destroyed.
 */
export function TypeRemovalSheet({
  open,
  onOpenChange,
  type,
  countRecords,
  onDelete,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The type about to go, or `null` when the sheet is closed. */
  type: MemoryType | null;
  countRecords: (kind: string) => Promise<number>;
  onDelete: (kind: string) => Promise<number>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="type-removal-lead">
        <SheetHeader>
          {/* Titled for the operation, not for the menu item that opened it.
              A sheet headed "Delete type" over a folder of somebody's
              documentation is the one heading that could make a person cancel
              the thing they wanted. */}
          <SheetTitle>
            {type && isAttachedType(type) ? "Detach folder" : "Delete type"}
          </SheetTitle>
        </SheetHeader>
        {open && type ? (
          <Confirmation
            type={type}
            countRecords={countRecords}
            onDelete={onDelete}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/**
 * How many records would go with the type: still being counted, the count, or
 * the store's refusal to say.
 */
type Toll =
  | { state: "counting" }
  | { state: "counted"; records: number }
  | { state: "unknown" };

function Confirmation({
  type,
  countRecords,
  onDelete,
  onDone,
}: {
  type: MemoryType;
  countRecords: (kind: string) => Promise<number>;
  onDelete: (kind: string) => Promise<number>;
  onDone: () => void;
}) {
  const [toll, setToll] = useState<Toll>({ state: "counting" });
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const attached = isAttachedType(type);

  useEffect(() => {
    let current = true;
    countRecords(type.kind).then(
      (records) => {
        if (current) setToll({ state: "counted", records });
      },
      // A store that cannot be counted can still be written to, and refusing to
      // go on would strand the person on the one screen that cannot answer for
      // itself. The warning below says the number is unknown instead.
      () => {
        if (current) setToll({ state: "unknown" });
      },
    );
    return () => {
      current = false;
    };
  }, [countRecords, type.kind]);

  async function remove() {
    if (isBusy) return;
    setIsBusy(true);
    setError(null);
    try {
      await onDelete(type.kind);
      onDone();
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "The type could not be removed.",
      );
      setIsBusy(false);
    }
  }

  return (
    <>
      <div className="space-y-4 p-4">
        {/* The type as the navigator draws it, so what is about to go is
            recognised as the row it was chosen from. */}
        <div className="flex items-center gap-2.5">
          <KindMark icon={type.icon} />
          <span className="min-w-0">
            <span className="block truncate text-base text-fg">
              {type.title}
            </span>
            {/* The identifier, because that is what the records about to go
                carry and what an agent would have written. */}
            <span className="block truncate font-mono text-xs text-fg-tertiary">
              {type.kind}
            </span>
          </span>
        </div>

        <SheetDescription id="type-removal-lead">
          {sentence(toll, attached)}
        </SheetDescription>

        <p className="text-xs text-fg-tertiary">
          The engine validates every record against its type, so records of a
          type that no longer exists could not be read or rewritten — they go
          with it. Nothing in the window brings them back.
        </p>

        {/* The one thing somebody detaching a folder would otherwise have to
            guess at, and the worst possible thing to guess wrong. It names the
            engine operation because that is what makes the promise keepable:
            deleting these records one at a time would take every file with
            them, and detaching is a different call. */}
        {attached ? (
          <p className="rounded-(--radius-control) border border-separator bg-panel p-2.5 text-xs leading-5 text-fg-secondary">
            The documents themselves stay where they are. This type&rsquo;s
            content lives{" "}
            {type.storage.folder ? (
              <>
                in <span className="font-mono">{type.storage.folder}</span>
              </>
            ) : (
              "in a folder of the repository"
            )}{" "}
            — ordinary repository files Sync never wrote — so what goes is the
            records that point at them and the attachment itself. Every file is
            left untouched, and the folder can be attached again.
          </p>
        ) : null}

        <ErrorNote message={error} />
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          variant="destructive"
          className="min-w-28"
          onClick={() => void remove()}
          // Not while the toll is unknown for want of asking: the button is
          // enabled once the store has answered, one way or the other.
          disabled={isBusy || toll.state === "counting"}
        >
          {isBusy
            ? attached
              ? "Detaching…"
              : "Deleting…"
            : attached
              ? "Detach folder"
              : "Delete type"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * What is about to happen, in the one sentence a person has to read.
 *
 * Two vocabularies, because two things happen. What a type over a folder loses
 * is what Sync knew about the documents — the count is of records, and the
 * sentence says as much, so nobody reads a number of files into it.
 */
function sentence(toll: Toll, attached: boolean): string {
  if (toll.state === "counting") {
    return attached
      ? "Counting what Sync would stop knowing about the folder…"
      : "Counting what would be deleted with it…";
  }
  if (toll.state === "unknown") {
    return attached
      ? "Every record Sync keeps for this folder goes with the type, and the documents stay. The project could not be asked how many records there are."
      : "Every record written as this type will be deleted with it. The project could not be asked how many there are.";
  }
  if (toll.records === 0) {
    return attached
      ? "Sync holds no records for this folder yet, so the type and the attachment are all that go."
      : "Nothing has been written as this type yet, so the definition is all that will be deleted.";
  }
  if (attached) {
    return toll.records === 1
      ? "The one record Sync keeps for this folder goes with the type. The document it describes stays."
      : `All ${toll.records} records Sync keeps for this folder go with the type. The documents they describe stay.`;
  }
  return toll.records === 1
    ? "The one record written as this type will be deleted with it."
    : `All ${toll.records} records written as this type will be deleted with it.`;
}
