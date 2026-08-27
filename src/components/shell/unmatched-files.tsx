"use client";

import { useState } from "react";
import { FileQuestion } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { MemoryType, ScanChange } from "@/lib/memory/types";
import { typeOfLocator } from "@/lib/memory/types";

/**
 * The files a scan could not attribute to a record, and the question they ask.
 *
 * Everything else an attached folder does is settled without anybody: a file
 * edited in place, moved, gone, or back where a record said it was. This is the
 * fifth outcome, and it is not a matter of the engine trying harder. A file
 * renamed and edited in the same stroke matches no record by path and none by
 * bytes — and neither does a file somebody just wrote. Nothing about the file
 * says which it is.
 *
 * Deciding silently means one of two bad outcomes: a document loses its history
 * and every link pointing at it, or two unrelated documents are merged into
 * one. So the engine ranks the records it could be, the way Git scores renames,
 * and stops. This is where a person answers.
 *
 * It lives above "Needs attention" for the same reason that view exists: it is
 * the one thing in the window that is waiting on somebody rather than on the
 * code.
 */
export function UnmatchedFiles({
  files,
  types,
  onResolve,
}: {
  files: readonly ScanChange[];
  /** The project's types, to say which folder — and so which type — a file is in. */
  types: readonly MemoryType[];
  onResolve: (file: ScanChange, kind: string, adopt?: string) => Promise<void>;
}) {
  if (files.length === 0) return null;

  return (
    <section className="border-b border-separator px-3 py-3">
      <h3 className="flex items-center gap-2 text-sm font-semibold text-fg">
        <FileQuestion className="size-4 shrink-0 text-fg-tertiary" />
        {files.length === 1
          ? "One file is waiting on you"
          : `${files.length} files are waiting on you`}
      </h3>
      <p className="mt-1 text-xs leading-5 text-fg-tertiary">
        Each of these could be a document that was renamed and edited at the
        same time, or could be new. The file itself does not say which, and
        guessing would either lose a document&rsquo;s links or merge two
        unrelated documents.
      </p>

      <ul className="mt-3 space-y-2">
        {files.map((file) => (
          <UnmatchedFile
            key={file.locator}
            file={file}
            types={types}
            onResolve={onResolve}
          />
        ))}
      </ul>
    </section>
  );
}

function UnmatchedFile({
  file,
  types,
  onResolve,
}: {
  file: ScanChange;
  types: readonly MemoryType[];
  onResolve: (file: ScanChange, kind: string, adopt?: string) => Promise<void>;
}) {
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locator = file.locator ?? "";
  const type = typeOfLocator(types, locator);

  async function answer(adopt?: string) {
    if (type === null || isBusy) return;
    setIsBusy(true);
    setError(null);
    try {
      await onResolve(file, type.kind, adopt);
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "That answer could not be written.",
      );
      setIsBusy(false);
    }
  }

  return (
    <li className="rounded-(--radius-control) border border-separator bg-panel p-3">
      <p className="truncate font-mono text-xs text-fg" title={locator}>
        {locator}
      </p>

      {type === null ? (
        // Nothing to offer, and the reason is worth saying: the type whose
        // folder held this file is gone, so there is no kind to write it as.
        <p className="mt-2 text-xs text-fg-tertiary">
          No attached type covers this folder any more, so there is nothing this
          file could be written as.
        </p>
      ) : (
        <>
          <ul className="mt-2 space-y-1">
            {(file.candidates ?? []).map((candidate) => (
              <li
                key={candidate.key}
                className="flex items-center justify-between gap-2"
              >
                <span className="min-w-0 flex-1 truncate text-xs text-fg-secondary">
                  <span className="font-mono">{candidate.locator}</span>
                  {/* The score, plainly. Git renames on the same measure, and
                      somebody choosing between two candidates is choosing on
                      exactly this. */}
                  <span className="ml-2 text-fg-tertiary">
                    {Math.round(candidate.similarity * 100)}% alike
                  </span>
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={isBusy}
                  onClick={() => void answer(candidate.key)}
                >
                  Same document
                </Button>
              </li>
            ))}
          </ul>

          <div className="mt-2 flex items-center justify-between gap-2">
            <p className="min-w-0 flex-1 text-xs text-fg-tertiary">
              Adopting keeps that record&rsquo;s key, so every link pointing at
              it survives the rename.
            </p>
            <Button
              variant="ghost"
              size="sm"
              disabled={isBusy}
              onClick={() => void answer()}
            >
              New document
            </Button>
          </div>
        </>
      )}

      {error === null ? null : (
        <p className="mt-2 text-xs text-destructive">{error}</p>
      )}
    </li>
  );
}
