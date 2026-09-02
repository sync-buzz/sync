"use client";

import { useEffect, useState } from "react";

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
import { said } from "@/lib/refusal";

/**
 * Deleting a folder, and everything filed under it.
 *
 * **Everything, whatever its type**, and the sheet says so rather than leaving
 * it to be discovered. A folder exists while something is in it, so a deletion
 * that spared another type's records would empty the folder and leave it
 * standing — which is not what anybody asking for this meant. Nothing filed
 * there is collateral: it is what the folder *is*.
 *
 * The number is asked of the store when the sheet opens rather than read off
 * the row behind it. That row counts one type's records at one level; this
 * counts every type at every depth, and a sentence naming a count is promising
 * the one about to be destroyed.
 *
 * What it does not promise is the directory. A folder holding a file no scan
 * has reached keeps that file and therefore keeps itself — so the sheet says
 * the records go, and leaves the working tree to speak for itself afterwards.
 */
export function FolderRemovalSheet({
  open,
  onOpenChange,
  folder,
  countRecords,
  onDelete,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The folder about to go, or `null` when the sheet is closed. */
  folder: string | null;
  countRecords: (folder: string) => Promise<number>;
  onDelete: (folder: string) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="folder-removal-lead">
        <SheetHeader>
          <SheetTitle>Delete folder</SheetTitle>
        </SheetHeader>
        {/* Mounted only while it is open, so the count is read for the folder
            this was opened on rather than left over from the last one. */}
        {open && folder !== null ? (
          <FolderRemoval
            folder={folder}
            countRecords={countRecords}
            onDelete={onDelete}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function FolderRemoval({
  folder,
  countRecords,
  onDelete,
  onDone,
}: {
  folder: string;
  countRecords: (folder: string) => Promise<number>;
  onDelete: (folder: string) => Promise<void>;
  onDone: () => void;
}) {
  const [toll, setToll] = useState<number | null>(null);
  const [counted, setCounted] = useState(false);
  const [isBusy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    void countRecords(folder).then(
      (count) => {
        if (!current) return;
        setToll(count);
        setCounted(true);
      },
      // A count that could not be read is said as that, not as zero. "Nothing
      // will be lost" is the one sentence this sheet must never guess at.
      () => {
        if (!current) return;
        setToll(null);
        setCounted(true);
      },
    );
    return () => {
      current = false;
    };
  }, [folder, countRecords]);

  const remove = async () => {
    setBusy(true);
    try {
      await onDelete(folder);
      onDone();
    } catch (refused) {
      setFailure(said(refused));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-3">
        <SheetDescription id="folder-removal-lead">
          {tollSentence(counted, toll)}
        </SheetDescription>

        <p className="font-mono text-sm text-fg-secondary">{folder}</p>

        {counted && toll !== null && toll > 0 ? (
          <p className="text-xs text-fg-tertiary">
            Every type filed there is counted, not only the one this folder is
            listed under — a folder exists while anything is in it.
          </p>
        ) : null}

        {failure ? <ErrorNote message={failure} /> : null}
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          variant="destructive"
          onClick={() => void remove()}
          disabled={!counted || isBusy}
          className="min-w-28"
        >
          {isBusy ? "Deleting…" : "Delete folder"}
        </Button>
      </SheetFooter>
    </>
  );
}

/**
 * What is about to be destroyed, in one sentence and never a guess.
 *
 * Four states, because "we could not ask" is not "nothing will be lost" and an
 * empty folder is not one with something in it.
 */
function tollSentence(counted: boolean, toll: number | null): string {
  if (!counted) return "Counting what is filed in this folder…";
  if (toll === null) {
    return "Everything filed in this folder goes with it, at any depth and whatever its type. The project could not be asked how many records that is.";
  }
  if (toll === 0) {
    return "Nothing is filed in this folder, so the folder is all that goes.";
  }
  if (toll === 1) {
    return "The one record filed in this folder goes with it. A record whose content is a file takes that file too.";
  }
  return `All ${toll} records filed in this folder go with it, at any depth and whatever their type. A record whose content is a file takes that file too.`;
}
