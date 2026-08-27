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
import type {
  Dependent,
  Dependents,
  MemoryRecord,
  MemoryType,
} from "@/lib/memory/types";
import { typeName } from "@/lib/memory/use-corpus";

/**
 * Deleting a record, and deciding what goes with it.
 *
 * A record nothing holds on to is a plain confirmation. One that other records
 * hold on to is a decision, and the sheet's whole job is to make it with the
 * store's own answer in view rather than with a warning about dependencies in
 * general.
 *
 * The two ways a record is held are not the same and are never treated as one:
 *
 * - **A link is structural.** Delete the target and the link points at nothing:
 *   the memory still works, and the part of it that explained why stops
 *   resolving. So the sheet offers to take those with it, one level — the records
 *   that link to this one, not everything that links to those. A whole branch
 *   deleted from one confirmation is the kind of thing nobody can undo and few
 *   would have chosen.
 * - **A mention is prose.** A record that names this one in its body is a
 *   sentence about it. Deleting the sentence's author because it mentioned
 *   something would delete the reasoning along with the conclusion, so mentions
 *   are counted, listed and never deleted here.
 *
 * There is no undo. That is why the sheet names what will go and shows it as the
 * rows it will disappear from.
 */
export function RecordRemovalSheet({
  open,
  onOpenChange,
  record,
  types,
  dependentsOf,
  onDelete,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The record about to go, or `null` when the sheet is closed. */
  record: MemoryRecord | null;
  types: readonly MemoryType[];
  dependentsOf: (key: string) => Promise<Dependents>;
  onDelete: (keys: readonly string[]) => Promise<void>;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="record-removal-lead">
        <SheetHeader>
          <SheetTitle>Delete record</SheetTitle>
        </SheetHeader>
        {open && record ? (
          <Confirmation
            record={record}
            types={types}
            dependentsOf={dependentsOf}
            onDelete={onDelete}
            onDone={() => onOpenChange(false)}
          />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

/** What holds on to the record: still being asked, the answer, or a refusal. */
type Held =
  | { state: "asking" }
  | { state: "answered"; dependents: Dependents }
  | { state: "unknown" };

function Confirmation({
  record,
  types,
  dependentsOf,
  onDelete,
  onDone,
}: {
  record: MemoryRecord;
  types: readonly MemoryType[];
  dependentsOf: (key: string) => Promise<Dependents>;
  onDelete: (keys: readonly string[]) => Promise<void>;
  onDone: () => void;
}) {
  const [held, setHeld] = useState<Held>({ state: "asking" });
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    dependentsOf(record.key).then(
      (dependents) => {
        if (current) setHeld({ state: "answered", dependents });
      },
      // A store that cannot be asked can still be written to. Refusing to go on
      // would strand the person on the one screen that cannot answer for itself,
      // so the sheet says the answer is unknown and offers only the narrow
      // deletion — the one whose consequences do not depend on the answer.
      () => {
        if (current) setHeld({ state: "unknown" });
      },
    );
    return () => {
      current = false;
    };
  }, [dependentsOf, record.key]);

  const links =
    held.state === "answered" ? held.dependents.links : ([] as Dependent[]);
  const mentions =
    held.state === "answered" ? held.dependents.mentions : ([] as Dependent[]);

  async function remove(keys: readonly string[]) {
    if (isBusy) return;
    setIsBusy(true);
    setError(null);
    try {
      await onDelete(keys);
      onDone();
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "The record could not be deleted.",
      );
      setIsBusy(false);
    }
  }

  const icon = types.find((type) => type.kind === record.kind)?.icon;

  return (
    <>
      <div className="space-y-4 p-4">
        {/* The record as the workspace draws it, so what is about to go is
            recognised as the row it was chosen from. */}
        <div className="flex items-center gap-2.5">
          <KindMark icon={icon} />
          <span className="min-w-0">
            <span className="block truncate text-base text-fg">
              {record.title.trim() || record.key}
            </span>
            <span className="block truncate font-mono text-xs text-fg-tertiary">
              {record.key} · {typeName(types, record.kind)}
            </span>
          </span>
        </div>

        <SheetDescription id="record-removal-lead">
          {sentence(held, links.length, mentions.length)}
        </SheetDescription>

        {links.length > 0 ? (
          <Holders
            title={links.length === 1 ? "Links to it" : `${links.length} link to it`}
            holders={links}
            types={types}
            detail="Deleting only this record leaves these links pointing at nothing. Deleting them with it leaves nothing dangling and takes their own claims too."
          />
        ) : null}

        {mentions.length > 0 ? (
          <Holders
            title={
              mentions.length === 1
                ? "Mentions it in prose"
                : `${mentions.length} mention it in prose`
            }
            holders={mentions}
            types={types}
            detail="These are never deleted here. A record that named this one is a sentence about it, and the sentence is somebody's reasoning — it will name a record that no longer exists, and that is the honest state to leave it in."
          />
        ) : null}

        {/* Said before it happens, not discovered from a diff. A record whose
            body is a file owns that file: the engine takes it with the record,
            because leaving it would be a deletion the next scan undoes — the
            document would come back as a new record, with a new key and none of
            the links this one had. There is nothing to decide here, so there is
            nothing to tick; what there is, is a file to name. */}
        {record.locator === null ? null : (
          <p className="rounded-(--radius-control) border border-separator bg-panel p-2.5 text-xs leading-5 text-fg-secondary">
            The document goes with it:{" "}
            <span className="font-mono">{record.locator}</span> is removed from
            the working tree, and the deletion is yours to commit like any other
            change to the repository.
          </p>
        )}

        <p className="text-xs text-fg-tertiary">
          Archiving is the reversible half of this: an archived record leaves the
          lists and keeps every link. Deleting is a transaction on the project&rsquo;s
          memory and nothing in the window brings it back.
        </p>

        <ErrorNote message={error} />
      </div>

      <SheetFooter>
        <div className="min-w-0 flex-1" />
        <Button variant="outline" onClick={onDone} disabled={isBusy}>
          Cancel
        </Button>
        {links.length > 0 ? (
          <Button
            variant="destructive"
            className="min-w-28"
            onClick={() =>
              void remove([record.key, ...links.map((link) => link.key)])
            }
            disabled={isBusy}
          >
            {isBusy ? "Deleting…" : `Delete all ${links.length + 1}`}
          </Button>
        ) : null}
        {/* Wide enough for the longest label either of these shows: a button
            that resizes when its label changes drags its neighbour with it. */}
        <Button
          variant={links.length > 0 ? "outline" : "destructive"}
          className="min-w-36"
          onClick={() => void remove([record.key])}
          disabled={isBusy || held.state === "asking"}
        >
          {isBusy ? "Deleting…" : "Delete this record"}
        </Button>
      </SheetFooter>
    </>
  );
}

function Holders({
  title,
  holders,
  types,
  detail,
}: {
  title: string;
  holders: readonly Dependent[];
  types: readonly MemoryType[];
  detail: string;
}) {
  return (
    <section className="space-y-2">
      <h3 className="text-xs font-semibold text-fg-tertiary">{title}</h3>
      <ul className="space-y-1">
        {holders.map((holder) => (
          <li key={holder.key} className="min-w-0 text-xs">
            <span className="text-fg-secondary">
              {holder.title.trim() || holder.key}
            </span>{" "}
            <span className="font-mono text-fg-tertiary">
              {holder.key}
              {holder.relation ? ` · ${holder.relation}` : ""}
              {holder.kind ? ` · ${typeName(types, holder.kind)}` : ""}
            </span>
          </li>
        ))}
      </ul>
      <p className="text-xs text-fg-tertiary">{detail}</p>
    </section>
  );
}

/** What is about to happen, in the one sentence a person has to read. */
function sentence(held: Held, links: number, mentions: number): string {
  if (held.state === "asking") {
    return "Asking the project what holds on to this record…";
  }
  if (held.state === "unknown") {
    return "The project could not be asked what holds on to this record, so only this one can be deleted from here.";
  }
  if (links === 0 && mentions === 0) {
    return "Nothing links to this record and nothing mentions it, so it goes on its own.";
  }
  if (links === 0) {
    return "Nothing links to this record, so it goes on its own — and what mentions it keeps saying so.";
  }
  return "Something else depends on this record, so there are two ways to delete it.";
}
