"use client";

import { useEffect, useMemo } from "react";

import { ArrowLeft, Ellipsis } from "lucide-react";

import { DocumentEditor } from "@/components/editor/document-editor";
import { KindMark } from "@/components/shell/entity-marks";
import { Markdown } from "@/components/shell/markdown";
import { PanelPlaceholder } from "@/components/shell/panel";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { editorHoldsRecord, forgetEditability } from "@/lib/editor/probe";
import type { Presence } from "@/lib/memory/types";
import type { OpenDocument, SaveState } from "@/lib/memory/use-document";
import { LinkOriginProvider } from "@/lib/record-link";
import { cn } from "@/lib/utils";

/**
 * One record, open.
 *
 * The body is the whole surface: a claim is prose, and prose is what the widest
 * column in the window is for. Everything that is *about* the record — its type,
 * how far it can be trusted, what it is scoped to, what it links, the fields its
 * type declares — is in the context panel beside it, because it describes the
 * thing rather than being it, and it is edited there for the same reason.
 *
 * It opens editable, with no mode to enter first: a person reads a claim, sees
 * the part that is wrong, and puts the caret there. What the header band carries
 * is the one thing that cannot be inferred from the text — whether what is on
 * screen is what the store holds — and the two commands that act on the record as
 * a whole.
 *
 * The project's own record is a document like any other. Its title is the
 * project's name, its body is the description and its `language` field is the
 * language the project writes in; all three are the project's data. What is not a
 * document is a type *definition*, and those are never listed as a record.
 *
 * One record is read instead of edited: one whose Markdown would not survive the
 * round trip through blocks. That is checked by round-tripping it, not guessed
 * at, and the reason is on the page.
 */
export function DocumentView({
  open,
  icon,
  note,
  onBack,
  backLabel,
  fixed,
  onArchive,
  onDelete,
  justCreated,
}: {
  open: OpenDocument;
  /** The mark for this record's type, from the published corpus. */
  icon: string | null | undefined;
  /** What is worth saying about this record before its text, if anything. */
  note?: string;
  onBack: () => void;
  /** Where returning goes — the list this record was opened from. */
  backLabel: string;
  /**
   * True for a record the window neither creates nor removes — the one that
   * names the project. Both commands are still drawn, and refused with the
   * reason: a menu missing an item explains nothing.
   */
  fixed?: boolean;
  onArchive: () => void;
  /** Ask to delete this record. The confirmation belongs to whoever owns the
   *  list, because a deletion changes it. */
  onDelete: () => void;
  /** True when this record was created a moment ago and still has no name. */
  justCreated?: boolean;
}) {
  const { document, draft, save } = open;

  /**
   * Whether this record can be edited, decided once when it is opened.
   *
   * Deliberately keyed on the record rather than on the store's copy of it. A
   * save answers with the body as stored, and asking again on every answer made
   * this a question the person could fail *while typing*: write something the
   * round trip would not survive, wait for the save, and the editor was
   * replaced mid-sentence by the reading view — with the caret gone and no way
   * back into the text.
   *
   * The verdict is about the body that was opened, which is the one thing this
   * is protecting: what is typed after it cannot be lost by an editor that is
   * already holding it, only by being written, and the store is what decides
   * that.
   */
  const key = document?.key ?? null;
  // Held steady, because every link in the body reads it: rebuilt on each
  // render, it would redraw all of them on every keystroke.
  const origin = useMemo(
    () =>
      key === null
        ? null
        : {
            key,
            kind: document?.kind ?? "",
            title: document?.title ?? "",
            locator: document?.locator ?? null,
          },
    [key, document?.kind, document?.title, document?.locator],
  );
  const holds =
    key === null ? null : editorHoldsRecord(key, document?.content ?? "");

  // Closing the record forgets the verdict, so opening it again asks about
  // whatever it holds by then rather than about what it held this time.
  useEffect(() => {
    if (key === null) return;
    return () => forgetEditability(key);
  }, [key]);
  // A record whose document is not in this working tree is read, never typed
  // into. Sync writes the file before the record, so saving a draft here would
  // create the file on this branch and quietly fork a document that exists on
  // another one — which is not what somebody who opened it to read it asked
  // for. The record itself is intact: its title, its tags and every link
  // pointing at it are in `refs` and unaffected by which branch is out.
  const absent = document !== null && document.contentMissing;
  // A document that is not text is read about rather than read. There is no
  // mask on an attached folder any more, so a diagram or a PDF sits beside the
  // prose and opens here like anything else — and the one thing the window must
  // not do is show it as an empty page somebody can type into, because the
  // first save would write that text over the file.
  const binary = document !== null && document.contentBinary;
  const editable =
    document !== null && holds?.editable === true && !absent && !binary;
  const refusal = absent
    ? absenceReason(document.presence, document.locator)
    : binary
      ? binaryReason(document.mediaType, document.locator)
      : holds?.editable === false
        ? holds.reason
        : null;

  return (
    <section className="flex h-full min-w-0 flex-col bg-workspace">
      <div className="flex h-(--panel-header-height) shrink-0 items-center gap-2 border-b border-separator px-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          className="shrink-0 gap-1.5 text-fg-secondary"
        >
          <ArrowLeft />
          <span>{backLabel}</span>
        </Button>
        <div className="min-w-0 flex-1" />
        <SaveMark save={save} />
        {document ? (
          <span className="shrink-0 truncate font-mono text-xs text-fg-tertiary">
            {document.key}
          </span>
        ) : null}
        {document && draft ? (
          <RecordActions
            archived={draft.archived}
            fixed={fixed === true}
            onArchive={onArchive}
            onDelete={onDelete}
          />
        ) : null}
      </div>

      {save.status === "failed" ? (
        <div className="flex shrink-0 items-center gap-3 border-b border-separator bg-panel px-3 py-2">
          <p className="min-w-0 flex-1 text-xs text-fg-secondary">
            <span className="font-medium text-danger">Not saved.</span>{" "}
            {save.kind === "diverged"
              ? "This project's code history was rewritten — a rebase, a reset, or a branch replaced — so what every record claims about the code has to be checked again before memory takes a write. Rechecking marks every record unverified; nothing written is lost by it."
              : save.message}{" "}
            What you wrote is still here, and stays here while the record is
            open.
          </p>
          {/* A refusal that will repeat itself is not answered by repeating the
              write. Rewritten history refuses identically until somebody says
              the new history is the real one, so that is the button — and it
              writes what was waiting once it has. */}
          <Button
            variant="outline"
            size="sm"
            onClick={
              save.kind === "diverged"
                ? () => void open.reconcile()
                : open.write
            }
            className="shrink-0"
          >
            {save.kind === "diverged" ? "Recheck and save" : "Try again"}
          </Button>
        </div>
      ) : null}

      {/* Which record this body is, for the links inside it: what a relative
          path is relative to, and which record the field must not offer as a
          target. A record with no file carries a `null` locator, so relative
          links in one are drawn as the text they are rather than resolved
          against a location it does not have. */}
      <LinkOriginProvider value={origin}>
        <ScrollArea className="min-h-0 flex-1">
          {editable && draft !== null && document !== null ? (
            <DocumentEditor
              key={document.key}
              opening={{ title: draft.title, content: draft.content }}
              icon={icon}
              note={note}
              autoFocusTitle={justCreated}
              onTitle={(title) => open.edit({ title })}
              onBody={open.editBody}
            />
          ) : (
            <div className="prose-surface mx-auto px-8 py-8">
              {document ? (
                <article className="space-y-6">
                  <header className="flex items-start gap-3">
                    <KindMark icon={icon} className="mt-1.5" />
                    <h1 className="min-w-0 flex-1 text-[1.85em] leading-tight font-semibold text-balance text-fg">
                      {document.title.trim() || document.key}
                    </h1>
                  </header>

                  {refusal ? (
                    <p className="rounded-(--radius-control) bg-panel px-3 py-2 text-xs text-fg-tertiary">
                      {refusal}
                    </p>
                  ) : null}

                  {document.content.trim() ? (
                    <Markdown>{document.content}</Markdown>
                  ) : absent ? (
                    // Not "no body". An empty document is something somebody
                    // wrote; a missing one is a document this checkout does not
                    // have, and telling the two apart is the whole reason the
                    // engine reports presence rather than an empty string.
                    <PanelPlaceholder
                      headline="The document is not in this working tree."
                      detail="The record, its links and everything said about it are unaffected."
                    />
                  ) : (
                    <PanelPlaceholder
                      headline="This record has no body."
                      detail="Its title and its fields are the whole of what it says."
                    />
                  )}
                </article>
              ) : (
                <PanelPlaceholder {...emptyState(open)} />
              )}
            </div>
          )}
        </ScrollArea>
      </LinkOriginProvider>
    </section>
  );
}

/**
 * The two commands that act on the record rather than on its text.
 *
 * A menu of the shell's own, because it is a control in a band the shell drew —
 * the same reason the navigator's bottom bar has one. The same two commands are
 * under the secondary button on the row this record was opened from, where the
 * system draws the menu itself; neither is the only way to reach them.
 *
 * Archiving leads because it is the reversible one, and because it is what most
 * of "this is no longer relevant" actually means.
 */
function RecordActions({
  archived,
  fixed,
  onArchive,
  onDelete,
}: {
  archived: boolean;
  fixed: boolean;
  onArchive: () => void;
  onDelete: () => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Actions for this record"
          className="shrink-0 text-fg-tertiary hover:text-fg aria-expanded:bg-selected aria-expanded:text-fg"
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <DropdownMenuItem disabled={fixed} onSelect={onArchive}>
          {archived ? "Bring back from the archive" : "Archive"}
        </DropdownMenuItem>
        <DropdownMenuLabel className="font-normal text-wrap text-fg-tertiary">
          An archived record keeps every link to it and leaves the lists.
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          variant="destructive"
          disabled={fixed}
          onSelect={onDelete}
        >
          Delete
        </DropdownMenuItem>
        {/* A disabled pair with no reason beside it is a window refusing without
            saying why — the same rule the type menu follows. */}
        {fixed ? (
          <DropdownMenuLabel className="font-normal text-wrap text-fg-tertiary">
            The record that names the project. There is one of it, and the
            project cannot be opened without it.
          </DropdownMenuLabel>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Whether what is on screen is what the store holds.
 *
 * It says nothing at all about a record nobody has typed into: the resting state
 * of an editor is not news. `Saved` is said once, after a write, because a write
 * here is a commit on the project's memory and the person who made it is owed the
 * confirmation. A failure is the only one of these that gets a colour, and it
 * gets the strip under the header as well, because it is the only one that needs
 * a decision.
 */
function SaveMark({ save }: { save: SaveState }) {
  if (save.status === "clean") return null;

  const label =
    save.status === "edited"
      ? "Edited"
      : save.status === "saving"
        ? "Saving…"
        : save.status === "saved"
          ? "Saved"
          : "Not saved";

  return (
    <span
      // A save that has not landed is a claim about the store, so it is polite
      // rather than urgent: the strip under the header is what interrupts.
      aria-live="polite"
      className={cn(
        "shrink-0 text-xs",
        save.status === "failed" ? "font-medium text-danger" : "text-fg-tertiary",
      )}
    >
      {label}
    </span>
  );
}

/**
 * Why this record's document cannot be read here, and what to do about it.
 *
 * Two absences, and they call for opposite things. A document another branch
 * holds comes back by switching to that branch; one deleted here is a decision
 * somebody made on this branch, and restoring it is a Git operation rather than
 * anything this window should offer to do behind their back.
 */
function absenceReason(presence: Presence, locator: string | null): string {
  const file = locator ?? "The file";
  if (presence === "removed") {
    return `${file} was deleted on this branch. The record is kept — deleted, on another branch and not pulled look identical from here, and two of the three are routine.`;
  }
  return `${file} is not on the checked-out branch. Memory does not branch and code does, so the corpus holds every branch's documents and this checkout has only some of them.`;
}

/**
 * Why a document is shown rather than opened, when what it holds is not text.
 *
 * Names the file and what it is, because those are the two things somebody
 * needs in order to open it in the application that can: Sync knows where the
 * document is and refuses to pretend it can edit it.
 */
function binaryReason(mediaType: string | null, locator: string | null): string {
  const file = locator ?? "This document";
  const what = mediaType === null ? "not text" : `a ${mediaType} file`;
  return `${file} is ${what}. Sync keeps the record beside it — its title, tags and links — and leaves the file to the application that edits it.`;
}

function emptyState(open: OpenDocument): {
  headline: string;
  detail?: string;
} {
  if (open.error) {
    return { headline: "This record could not be read.", detail: open.error };
  }
  if (open.isLoading) return { headline: "Reading…" };
  return {
    headline: "This record is no longer in the store.",
    detail: "It was deleted, or the memory moved to a revision without it.",
  };
}
