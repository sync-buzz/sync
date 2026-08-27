"use client";

import { useState } from "react";

import {
  ErrorNote,
  Field,
  FIELD_CONTROL,
  StepBody,
} from "@/components/shell/project-setup";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import type { Overlap } from "@/lib/memory/types";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import { cn } from "@/lib/utils";

/**
 * Where a project's memory is published, and the two commands that move it.
 *
 * A sheet rather than a window: it configures the project this window has open,
 * which is what a sheet is for. Settings would have been the wrong home twice
 * over — a memory remote is neither the installation's nor the project's, but
 * this clone's, and the engine keeps it in the repository's own Git config.
 *
 * The state, the address and the commands are on one surface because deciding
 * to publish is one decision made with all three in view: what would go, where
 * it would go, and whether anything is coming the other way.
 */
export function SyncSheet({
  open,
  onOpenChange,
  sync,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sync: SyncStatus;
}) {
  const configured = sync.transport?.remoteConfigured ?? false;

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent aria-describedby="sync-lead">
        <SheetHeader>
          <SheetTitle>Project memory</SheetTitle>
        </SheetHeader>

        <StepBody>
          <SheetDescription id="sync-lead">{lead(sync)}</SheetDescription>
          {/* Keyed on what the store answered, so the field is *remounted*
              with the right address rather than assigned it inside an effect.
              A person's half-typed edit survives every re-render and is
              discarded only when the stored answer itself changes, which is
              the moment it stopped being about the same thing. */}
          <RemoteField
            key={
              sync.transport
                ? (sync.transport.remoteUrl ??
                  sync.transport.codeOriginUrl ??
                  "none")
                : "unread"
            }
            sync={sync}
          />
          <Overlaps
            overlaps={sync.overlaps}
            onSeen={sync.acknowledgeOverlaps}
          />
          <ErrorNote message={sync.error} />
        </StepBody>

        <SheetFooter>
          {/* Leading, away from the two that move memory: it undoes rather than
              exchanges, and a person reaching for Fetch must not find it. It is
              here only while it would do something — a fetch that merged
              nothing has nothing to undo, and one written on top of since is
              refused by the engine rather than offered here. */}
          {sync.undoable ? (
            <Button
              variant="outline"
              onClick={sync.undoFetch}
              disabled={sync.busy !== null}
            >
              Undo fetch
            </Button>
          ) : null}
          <div className="min-w-0 flex-1" />
          {/* Fetch first and Publish last: the destructive-adjacent one is the
              one that leaves this machine, and it sits where a sheet's
              confirming action sits. */}
          <Button
            variant="outline"
            onClick={sync.fetchNow}
            disabled={!configured || sync.busy !== null}
            className="min-w-28"
          >
            {sync.busy === "fetching" ? "Fetching…" : "Fetch"}
          </Button>
          <Button
            onClick={sync.publishNow}
            disabled={!configured || sync.busy !== null}
            className="min-w-28"
          >
            {sync.busy === "publishing" ? "Publishing…" : "Publish"}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

/**
 * The address memory is published to, which is not the code remote.
 *
 * A clone knows one address — its code `origin` — so that is what the field is
 * filled with when nothing is configured. It is a suggestion and says so:
 * memory has a remote of its own precisely so it does not have to follow the
 * code's, and a private repository for memory beside a public one for code is
 * one edit away.
 */
function RemoteField({ sync }: { sync: SyncStatus }) {
  const transport = sync.transport;
  const [url, setUrl] = useState(
    transport?.remoteUrl ?? transport?.codeOriginUrl ?? "",
  );
  const [saving, setSaving] = useState(false);

  const stored = transport?.remoteUrl ?? null;
  const changed = url.trim().length > 0 && url.trim() !== stored;

  return (
    <Field
      label="Memory remote"
      htmlFor="memory-remote"
      hint={
        transport?.remoteConfigured
          ? "Memory is published here and nowhere else. An ordinary git push never sends it."
          : "Suggested from this repository’s code origin. Memory keeps a remote of its own, so it can go somewhere the code does not."
      }
    >
      <div className="flex items-center gap-2">
        <input
          id="memory-remote"
          value={url}
          spellCheck={false}
          onChange={(event) => setUrl(event.target.value)}
          placeholder="git@example.com:team/project.git"
          className={cn(FIELD_CONTROL, "font-mono text-sm")}
        />
        <Button
          variant="outline"
          disabled={!changed || saving}
          className="min-w-20"
          onClick={() => {
            setSaving(true);
            void sync
              .setRemote(url.trim())
              .catch(() => undefined)
              .finally(() => setSaving(false));
          }}
        >
          {saving ? "Saving…" : "Save"}
        </Button>
      </div>
    </Field>
  );
}

/**
 * What the last fetch merged over.
 *
 * A record both sides had edited in the same places keeps this side's text, and
 * that is a decision made on somebody's behalf while they were not looking —
 * so it is reported here rather than absorbed. Nothing is gone: both versions
 * are commits, and the one that lost is still reachable in the history.
 */
function Overlaps({
  overlaps,
  onSeen,
}: {
  overlaps: readonly Overlap[];
  onSeen: () => void;
}) {
  if (overlaps.length === 0) return null;

  return (
    <div className="space-y-1.5">
      <p className="text-sm text-fg">
        {overlaps.length === 1
          ? "One record was changed in the same place on both sides."
          : `${overlaps.length} records were changed in the same place on both sides.`}{" "}
        Your version was kept there. The other is still in this repository’s
        history.
      </p>
      <ul className="space-y-0.5">
        {overlaps.map((overlap) => (
          <li key={overlap.key} className="flex min-w-0 items-baseline gap-2">
            <span className="truncate font-mono text-xs text-fg-tertiary">
              {overlap.key}
            </span>
            {/* What it cost, beside what it happened to. A key alone tells
                somebody a record was merged over and leaves them to open it
                and guess; naming the part says whether to read the text again
                or just look at the title. */}
            <span className="shrink-0 text-xs text-fg-tertiary">
              {whatLost(overlap)}
            </span>
          </li>
        ))}
      </ul>
      <Button variant="outline" size="sm" onClick={onSeen}>
        Got it
      </Button>
    </div>
  );
}

/**
 * Which parts of a record the other version lost, in a person's words.
 *
 * The engine names members as it spells them — `is_folder`, `media_type`, a
 * product field's own identifier — and a panel that printed those would be
 * quoting the store's schema at somebody reading about their colleague's work.
 * A member with no name here keeps its own, which is right for a product field:
 * that name is the project's, and the project chose it.
 */
function whatLost(overlap: Overlap): string {
  const parts = [
    ...(overlap.body ? ["the text"] : []),
    ...overlap.fields.map((field) => MEMBER_NAMES[field] ?? field),
  ];
  if (parts.length === 0) return "";
  if (parts.length === 1) return parts[0];
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

const MEMBER_NAMES: Readonly<Record<string, string>> = {
  title: "the title",
  folder: "the folder",
  is_folder: "what it is",
  tags: "the tags",
  links: "the links",
  archive: "whether it is archived",
  freshness: "its freshness",
  media_type: "the media type",
  content_ref: "where its file is",
  kind: "its type",
};

/** What is true, said in full — the header has room for a word, this has room for a sentence. */
function lead(sync: SyncStatus): string {
  const state = sync.state;
  if (!state) return "Reading what this project has published.";
  if (!state.remoteConfigured) {
    return "This project’s memory has never left this repository. Name a remote below to publish it.";
  }

  const mine =
    state.unpublished === 1
      ? "One record here is not on the remote."
      : state.unpublished > 1
        ? `${state.unpublished} records here are not on the remote.`
        : "Everything here is published.";

  if (state.remote === "unreachable") {
    return `${mine} The remote could not be reached, so what it holds is unknown.`;
  }
  if (state.remote === "waiting") {
    return `${mine} The remote holds something this repository does not.`;
  }
  if (state.remote === "not_asked") return mine;
  return `${mine} Nothing is waiting on the remote.`;
}
