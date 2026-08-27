"use client";

import { useEffect, useState } from "react";

import type { CatalogueEntry } from "@/lib/extension-host/catalogue";
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
 * Removing an extension, and what stays behind.
 *
 * Nothing is destroyed here, and that is the whole reason the sheet exists.
 * Turning an extension off is not a statement that its data is expendable —
 * somebody switching one off to try another still expects their decisions to be
 * there afterwards — so the types and every record written under them stay
 * exactly where they are.
 *
 * What changes is that nothing shows them: the section goes with the
 * extension. That is worth a sentence and a number, because a removal whose
 * consequence is invisible reads as free, and this one is not. The count is
 * asked of the store at the moment of asking rather than remembered, so it
 * describes the corpus as it is now.
 */
export function ExtensionRemovalSheet({
  open,
  onOpenChange,
  extension,
  countRecords,
  onRemove,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The extension about to be removed, or `null` when the sheet is closed. */
  extension: CatalogueEntry | null;
  countRecords: (id: string) => Promise<number>;
  onRemove: (id: string) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="extension-removal-lead">
        <SheetHeader>
          <SheetTitle>Remove extension</SheetTitle>
        </SheetHeader>
        {open && extension ? (
          <Confirmation
            extension={extension}
            countRecords={countRecords}
            onRemove={onRemove}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/** How many records would be left without a screen: counting, a count, or no answer. */
type Toll =
  | { state: "counting" }
  | { state: "counted"; records: number }
  | { state: "unknown" };

function Confirmation({
  extension,
  countRecords,
  onRemove,
  onDone,
}: {
  extension: CatalogueEntry;
  countRecords: (id: string) => Promise<number>;
  onRemove: (id: string) => Promise<void>;
  onDone: () => void;
}) {
  const [toll, setToll] = useState<Toll>({ state: "counting" });
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    countRecords(extension.id).then(
      (records) => {
        if (current) setToll({ state: "counted", records });
      },
      () => {
        // The store not answering is not a reason to refuse the removal — it
        // destroys nothing. It is a reason not to promise a number.
        if (current) setToll({ state: "unknown" });
      },
    );
    return () => {
      current = false;
    };
  }, [countRecords, extension.id]);

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
        <SheetDescription id="extension-removal-lead">
          {extension.name} stops being part of this project: its section leaves
          the sidebar, and the project no longer declares it.
        </SheetDescription>

        <p className="text-sm leading-5 text-fg-secondary">
          {toll.state === "counting"
            ? "Counting what it holds…"
            : toll.state === "unknown"
              ? "Nothing it wrote is deleted. Its types and every record under them stay in the project, and installing it again shows them exactly as they were."
              : toll.records === 0
                ? "It has written nothing yet, so there is nothing to leave behind."
                : `Nothing is deleted: ${toll.records} ${
                    toll.records === 1 ? "record stays" : "records stay"
                  } in the project with nothing to show them. Installing ${
                    extension.name
                  } again shows them exactly as they were.`}
        </p>

        {error === null ? null : (
          <p className="text-sm leading-5 text-danger">{error}</p>
        )}
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        <Button
          onClick={() => {
            setError(null);
            setIsBusy(true);
            onRemove(extension.id)
              .then(onDone)
              .catch((refused: unknown) => {
                setError(
                  refused instanceof Error
                    ? refused.message
                    : "The project's memory did not answer.",
                );
              })
              .finally(() => setIsBusy(false));
          }}
          disabled={isBusy}
          className="min-w-28"
        >
          {isBusy ? "Removing…" : "Remove"}
        </Button>
      </SheetFooter>
    </>
  );
}
