"use client";

import { GitCommitHorizontal } from "lucide-react";
import { PanelHeader, PanelSurface } from "@/components/shell/panel";
import { FRESHNESS_STATES, StateMark } from "@/components/shell/entity-marks";
import { RecordMetadata } from "@/components/shell/record-metadata";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { OpenDocument } from "@/lib/memory/use-document";
import { ATTENTION_STATES, type Corpus } from "@/lib/memory/use-corpus";

/**
 * The context panel.
 *
 * This is where this application differs from the tools it competes with at a
 * glance. Their right-hand panel searches files; ours answers a different
 * question — what does the project currently claim about itself, and how much
 * of that is still true after the code moved.
 *
 * It answers that question about whatever the window is pointed at. With a
 * record open, that is the record: everything *about* it lives here so that the
 * centre can be nothing but the text. With no record open, it is the corpus.
 */
export function ContextInspector({
  corpus,
  open,
  projectPath,
}: {
  corpus: Corpus;
  /** The record the workspace has open, if it has one. */
  open: OpenDocument | null;
  /** Where the project is, so the panel's open panel opens inside it. */
  projectPath: string;
}) {
  return (
    <PanelSurface className="bg-panel">
      <PanelHeader title={open?.document ? "Record" : "Context"} />
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-5 p-3">
          {open?.document && open.draft ? (
            <RecordMetadata
              document={open.document}
              draft={open.draft}
              type={corpus.types.find(
                (type) => type.kind === open.document?.kind,
              )}
              types={corpus.types}
              projectPath={projectPath}
              onEdit={open.edit}
              onWrite={open.write}
            />
          ) : (
            <CorpusFacts corpus={corpus} />
          )}
        </div>
      </ScrollArea>
    </PanelSurface>
  );
}

/** What is true about the whole of what the project knows. */
function CorpusFacts({ corpus }: { corpus: Corpus }) {
  const { counts } = corpus;
  const attention = ATTENTION_STATES.reduce(
    (total, state) => total + (counts.byFreshness[state] ?? 0),
    0,
  );

  return (
    <>
      <section className="space-y-2">
        <div className="flex items-center gap-1.5 text-xs text-fg-tertiary">
          <GitCommitHorizontal className="size-3.5 shrink-0" />
          {/* The memory's own revision, not the code branch: the corpus is
              committed to refs of its own and moves independently of the branch
              that happens to be checked out. */}
          <span className="truncate font-mono">
            {corpus.revision ? corpus.revision.slice(0, 7) : "no revision"}
          </span>
        </div>
        <dl className="space-y-1.5">
          {states(corpus).map((state) => (
            <div key={state} className="flex items-center gap-2">
              <dt className="min-w-0 flex-1">
                <StateMark freshness={state} />
              </dt>
              <dd className="shrink-0 font-mono text-xs text-fg-secondary tabular-nums">
                {counts.byFreshness[state] ?? 0}
              </dd>
            </div>
          ))}
        </dl>
        <p className="text-xs text-fg-tertiary">{summary(corpus, attention)}</p>
      </section>

      <section className="space-y-2">
        <h3 className="text-xs font-semibold text-fg-tertiary">Types</h3>
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 text-xs text-fg-secondary">
            In this project&apos;s corpus
          </span>
          <span className="shrink-0 font-mono text-xs text-fg-secondary tabular-nums">
            {corpus.types.length}
          </span>
        </div>
        <p className="text-xs text-fg-tertiary">
          What this project is able to say. The engine validates every write
          against these, and an agent reads the same definitions the window
          lists.
        </p>
      </section>
    </>
  );
}

/**
 * The states worth a row.
 *
 * The four this build draws are always listed, including the empty ones —
 * "invalid 0" is the good news, and a summary that hides it cannot be read as
 * an answer. A state a newer engine reports is added rather than dropped.
 */
function states(corpus: Corpus): string[] {
  const known: readonly string[] = FRESHNESS_STATES;
  const reported = Object.keys(corpus.counts.byFreshness).filter(
    (state) => !known.includes(state),
  );
  return [...FRESHNESS_STATES, ...reported];
}

function summary(corpus: Corpus, attention: number): string {
  if (corpus.error) return corpus.error;
  // These counts cover the types this window lists, so with types hidden a
  // total of zero says nothing about the project — only about the filter.
  if (corpus.hidden.length > 0) {
    return `Counted over the types this window lists; ${corpus.hidden.length} of them are hidden.`;
  }
  if (corpus.counts.total === 0) {
    return "The project has not stated anything yet, so there is nothing to have gone stale.";
  }
  if (attention === 0) {
    return "Nothing has stopped matching the code. Freshness is derived from each record's scope as the code moves under it.";
  }
  return `${attention} ${attention === 1 ? "claim" : "claims"} stopped matching the code. Freshness is derived from each record's scope as the code moves under it.`;
}
