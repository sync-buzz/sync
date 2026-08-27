"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { Dialog } from "radix-ui";
import { Search } from "lucide-react";

import { KindMark } from "@/components/shell/entity-marks";
import type { Opener, Opening } from "@/components/shell/opening";
import { TypeFilter } from "@/components/shell/type-filter";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { AreaIntent } from "@/lib/area-intent";
import { loadRecords, memoryTypes } from "@/lib/memory/client";
import type { MemoryType, SearchHit } from "@/lib/memory/types";
import { typeName } from "@/lib/memory/use-corpus";
import { useSearch } from "@/lib/memory/use-search";
import type { OpenProject } from "@/lib/project/types";
import { useProjectView } from "@/lib/project/use-project-view";
import { cn } from "@/lib/utils";

/**
 * Searching the project, as one surface over every area.
 *
 * It is a palette rather than a column because what it searches is not a
 * section: a question is asked of the whole corpus, and the answer is records
 * of every type — including types belonging to areas that are installed but
 * were never opened. A column would have made search a section of the project,
 * which would be the one section a project cannot choose not to have.
 *
 * **It opens nothing itself.** A result is a record of some kind, the kind
 * decides which area owns it, and that area is what opens it — the palette
 * hands over an intent and closes. That is the whole of the arrangement: this
 * file has no idea what Records does with a key, and an area from an
 * extension this build has never seen would be handed exactly the same object.
 *
 * When nothing owns the kind, that is said rather than worked around. A record
 * whose extension is not installed is not a broken row and not a dead click; it
 * is an answerable state, and the answer is one section away.
 */

export function SearchPalette({
  project,
  opener,
  open,
  onOpenChange,
  onShow,
}: {
  project: OpenProject;
  /** Which section opens a kind, bound to what this window is running. */
  opener: Opener;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Hand a result to the area that owns it, and select that area. */
  onShow: (areaKey: string, intent: AreaIntent) => void;
}) {
  const [query, setQuery] = useState("");
  const [types, setTypes] = useState<readonly MemoryType[]>([]);
  // How many records each type holds, so the filter here carries the same
  // numbers the navigator's does. A filter that shows counts in one place and
  // not in the other is two controls again, however alike they look.
  const [counts, setCounts] = useState<Readonly<Record<string, number>>>({});
  /**
   * Which row the keyboard is on, and which question it was on.
   *
   * The question is carried with the index rather than the index being reset
   * when the answer changes: a list that has just been replaced has no row four,
   * and remembering which question the cursor belonged to answers that in a
   * render instead of in an effect that fires after the wrong row is drawn.
   */
  const [cursorAt, setCursorAt] = useState<{
    readonly stamp: string;
    readonly index: number;
  }>({ stamp: "", index: 0 });
  // The result somebody asked for that nothing can open, and why. It replaces
  // the list rather than annotating it: the question has stopped being "which
  // of these" and become "what do I do about this one".
  const [blocked, setBlocked] = useState<
    { readonly hit: SearchHit; readonly opening: Opening } | null
  >(null);

  const field = useRef<HTMLInputElement>(null);

  /**
   * Which types are searched, as the project view holds it.
   *
   * The same preference the navigator's filter writes, and deliberately not a
   * second one: a type somebody took out of this project's window is not one
   * they want back in an answer, and two stored answers to "which types" would
   * be two things to keep in step. It outlives the palette because it outlives
   * the window — it is written where the recent projects are.
   */
  const view = useProjectView(project.path);
  const searched = types
    .filter((type) => !view.isHidden(type.kind))
    .map((type) => type.kind);
  // Nothing is narrowed until something is hidden, and an empty set is the
  // whole corpus rather than nothing — so the two states are told apart here
  // rather than by the length of a list.
  const narrowed = view.hidden.length > 0;
  const everythingHidden = narrowed && searched.length === 0;
  const kinds = narrowed ? searched : [];

  const answer = useSearch(project.path, query, kinds, open && !everythingHidden);
  const stamp = `${query.trim()} ${kinds.join(",")}`;

  // The project's types, for what a group is called and what the filter offers.
  // Read when the palette opens rather than held for the life of the window: a
  // type added while it was closed would otherwise be missing from both.
  useEffect(() => {
    if (!open) return;
    let current = true;
    void (async () => {
      try {
        // One page of nothing: the counts describe the whole corpus and the
        // records are not wanted here, so the cheapest read that carries them.
        const [published, view] = await Promise.all([
          memoryTypes(project.path),
          loadRecords(project.path, { limit: 1 }),
        ]);
        if (!current) return;
        setTypes(published);
        setCounts(view.counts.byKind);
      } catch {
        // The names are decoration here: a group falls back to the kind as the
        // store spells it, and the filter to the types last read. Neither is
        // worth an error message over a search that otherwise works.
      }
    })();
    return () => {
      current = false;
    };
  }, [open, project.path]);

  // Opening on the last question, with it selected: the ordinary next search is
  // a different one, and the ordinary next keystroke should replace it.
  useEffect(() => {
    if (!open) return;
    // After the dialog has taken focus, not before.
    const frame = requestAnimationFrame(() => field.current?.select());
    return () => cancelAnimationFrame(frame);
  }, [open]);

  const sections = useMemo(() => sectionsOf(answer.hits), [answer.hits]);
  const walked = useMemo(
    () =>
      sections.flatMap((section) =>
        section.groups.flatMap((group) => group.rows.map((row) => row.hit)),
      ),
    [sections],
  );

  const cursor =
    cursorAt.stamp === stamp
      ? Math.min(cursorAt.index, Math.max(walked.length - 1, 0))
      : 0;

  /**
   * Closing settles the palette back to what it opens as.
   *
   * Done on the way out rather than on the way in: the palette is opened from
   * two places and closed from four, and what a person sees when it appears
   * should not depend on which of them last touched it.
   */
  const setOpen = (next: boolean) => {
    if (!next) {
      setBlocked(null);
      setCursorAt({ stamp: "", index: 0 });
    }
    onOpenChange(next);
  };

  const activate = (hit: SearchHit) => {
    const opening = opener(hit.kind ?? "");
    if (opening.outcome !== "area") {
      setBlocked({ hit, opening });
      return;
    }
    onShow(opening.areaKey, {
      show: "record",
      key: hit.id,
      kind: hit.kind ?? "",
    });
    setOpen(false);
  };

  const showExtension = (id: string) => {
    onShow("extensions", { show: "extension", id });
    setOpen(false);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (walked.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setCursorAt({ stamp, index: Math.min(cursor + 1, walked.length - 1) });
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setCursorAt({ stamp, index: Math.max(cursor - 1, 0) });
    } else if (event.key === "Enter") {
      event.preventDefault();
      const hit = walked[cursor];
      if (hit) activate(hit);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-(--window-inset) z-40 rounded-(--radius-window) bg-scrim duration-(--motion-duration) data-closed:animate-out data-closed:fade-out-0 data-open:animate-in data-open:fade-in-0" />
        <Dialog.Content
          // Under the title bar and centred on the window, like every other
          // modal here: the palette belongs to this project's window, not to
          // the screen it happens to be on.
          className={cn(
            "fixed top-[calc(var(--window-inset)+var(--header-height)+12px)] left-1/2 z-50 -translate-x-1/2",
            "w-[min(640px,calc(100vw-var(--window-inset)*2-64px))] max-h-[calc(100vh-var(--window-inset)*2-var(--header-height)-64px)]",
            "flex flex-col overflow-hidden rounded-(--radius-surface) bg-raised text-fg shadow-(--shadow-content)",
            "duration-(--motion-duration) data-closed:animate-out data-closed:fade-out-0 data-closed:slide-out-to-top-4 data-open:animate-in data-open:fade-in-0 data-open:slide-in-from-top-4",
          )}
          aria-describedby={undefined}
          // The filter's menu is a portal of its own, so a click on one of its
          // rows lands outside this content and would dismiss the whole
          // palette. Ticking three types is one decision, and the menu is
          // built to stay open through it — the surface under it has to stay
          // open too.
          onInteractOutside={(event) => {
            const target = event.target as HTMLElement | null;
            if (target?.closest('[data-slot="dropdown-menu-content"]')) {
              event.preventDefault();
            }
          }}
        >
          <Dialog.Title className="sr-only">Search this project</Dialog.Title>

          <div className="flex h-(--panel-header-height) shrink-0 items-center gap-2 border-b border-separator pr-1.5 pl-3">
            <Search className="size-3.5 shrink-0 text-fg-tertiary" />
            <input
              ref={field}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Search this project"
              aria-label="Search this project"
              autoComplete="off"
              spellCheck={false}
              className="min-w-0 flex-1 bg-transparent text-sm text-fg outline-none placeholder:text-fg-tertiary"
            />
            <TypeFilter
              types={types}
              counts={counts}
              view={view}
              verb="searched"
              align="end"
            />
          </div>

          <ScrollArea className="min-h-0 flex-1">
            {blocked ? (
              <Blocked
                hit={blocked.hit}
                opening={blocked.opening}
                types={types}
                onShowExtension={showExtension}
                onBack={() => setBlocked(null)}
              />
            ) : (
              <Results
                query={query}
                everythingHidden={everythingHidden}
                answer={answer}
                sections={sections}
                walked={walked}
                cursor={cursor}
                types={types}
                opener={opener}
                onHover={(index) => setCursorAt({ stamp, index })}
                onActivate={activate}
              />
            )}
          </ScrollArea>

          <Footnote
            answer={answer}
            query={query}
            quiet={blocked !== null || everythingHidden}
          />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** One hit, and where it sits in the list the keyboard walks. */
interface Row {
  readonly hit: SearchHit;
  readonly index: number;
}

/** The hits of one kind, in the order the store ranked them. */
interface Group {
  readonly kind: string;
  readonly rows: readonly Row[];
}

/**
 * The answer, split by what the store actually did.
 *
 * Two claims are being made and they are not the same one. A record containing
 * the words asked for is an answer; a record that is merely the nearest thing
 * in the corpus is a suggestion — and the engine returns one of those for
 * anything at all, including a keyboard mash, because in a corpus of a dozen
 * records something is always nearest.
 *
 * Mixed into one list, the second kind reads as the first, which is how a
 * search for `qqqqqqqqq` comes back looking like it found something. Kept
 * apart and named, it stops making a claim it cannot support, and the useful
 * half of it survives: a synonym the corpus does not spell is exactly what
 * this channel is for.
 */
interface Section {
  readonly id: "words" | "meaning";
  readonly heading: string | null;
  readonly groups: readonly Group[];
}

/**
 * The answer, cut into sections and then into types.
 *
 * Groups appear in the order their best hit did, so the most relevant type
 * leads and the store's ranking is not quietly re-sorted into an alphabet
 * nobody asked for. The index on each row is its place in the list *as drawn* —
 * the arrow keys cross headings and section boundaries without noticing them,
 * and nothing may number a row before its position on screen is settled.
 */
function sectionsOf(hits: readonly SearchHit[]): readonly Section[] {
  const byWords = hits.filter((hit) => hit.matched !== "meaning");
  const byMeaning = hits.filter((hit) => hit.matched === "meaning");

  const sections: Section[] = [];
  let next = 0;

  const build = (
    id: Section["id"],
    heading: string | null,
    subset: readonly SearchHit[],
  ) => {
    if (subset.length === 0) return;
    const order: string[] = [];
    const byKind = new Map<string, SearchHit[]>();
    for (const hit of subset) {
      const kind = hit.kind ?? "";
      const held = byKind.get(kind);
      if (held === undefined) {
        byKind.set(kind, [hit]);
        order.push(kind);
      } else {
        held.push(hit);
      }
    }
    // Numbered after grouping, never before. The store ranks hits without
    // regard to type, so a rank order of A B A is drawn as A A B once the
    // groups are formed — and a row numbered by its rank would then sit second
    // on screen while the arrow keys think it is third. The index is a place in
    // the list as drawn, so it is assigned by walking the list as drawn.
    const groups: Group[] = order.map((kind) => ({
      kind,
      rows: (byKind.get(kind) ?? []).map((hit) => {
        const row = { hit, index: next };
        next += 1;
        return row;
      }),
    }));
    sections.push({ id, heading, groups });
  };

  build("words", null, byWords);
  build(
    "meaning",
    byWords.length === 0
      ? "No words matched. Nearest by meaning:"
      : "Also near in meaning",
    byMeaning,
  );

  return sections;
}

function Results({
  query,
  everythingHidden,
  answer,
  sections,
  walked,
  cursor,
  types,
  opener,
  onHover,
  onActivate,
}: {
  query: string;
  /** Every type is hidden, so there is nothing this search could look in. */
  everythingHidden: boolean;
  answer: ReturnType<typeof useSearch>;
  sections: readonly Section[];
  walked: readonly SearchHit[];
  cursor: number;
  types: readonly MemoryType[];
  opener: Opener;
  onHover: (index: number) => void;
  onActivate: (hit: SearchHit) => void;
}) {
  if (everythingHidden) {
    return (
      <Quiet
        headline="Every type is hidden."
        detail="Search looks in the types this window lists, and none of them is listed. Tick one in the filter beside the field."
      />
    );
  }
  if (answer.error !== null) {
    return <Quiet headline="The store did not answer." detail={answer.error} />;
  }
  if (query.trim() === "") {
    return (
      <Quiet
        headline="Search this project"
        detail="Every record the project holds, of every type — including the ones written by agents."
      />
    );
  }
  if (answer.hits.length === 0) {
    return answer.isSearching ? (
      <Quiet headline="Searching…" />
    ) : (
      <Quiet
        headline="Nothing matched."
        detail="No record answers that question at this revision."
      />
    );
  }

  return (
    <div className="p-1.5">
      {sections.map((section) => (
        <div key={section.id}>
          {section.heading === null ? null : (
            // The one line in the palette that is about the search rather than
            // about a record: it says what the rows under it are, so that a
            // suggestion is never read as a match.
            <p className="border-t border-separator px-2 pt-2.5 pb-1 text-xs text-fg-secondary first:border-t-0">
              {section.heading}
            </p>
          )}
          {section.groups.map((group) => (
            <section key={group.kind || "untyped"} className="pb-1 last:pb-0">
              <h3 className="px-2 pt-2 pb-1 text-xs font-medium text-fg-tertiary">
                {group.kind === "" ? "Untyped" : typeName(types, group.kind)}
              </h3>
              {group.rows.map(({ hit, index }) => (
                <Hit
                  key={hit.id}
                  hit={hit}
                  icon={types.find((type) => type.kind === hit.kind)?.icon}
                  opening={opener(hit.kind ?? "")}
                  isActive={index === cursor && index < walked.length}
                  onHover={() => onHover(index)}
                  onActivate={() => onActivate(hit)}
                />
              ))}
            </section>
          ))}
        </div>
      ))}
    </div>
  );
}

/**
 * One hit.
 *
 * A row whose kind nothing installed can open says so where it is, quietly and
 * before it is clicked. It is still a row and still activates: what it opens is
 * the explanation, which is the only honest thing behind it.
 */
function Hit({
  hit,
  icon,
  opening,
  isActive,
  onHover,
  onActivate,
}: {
  hit: SearchHit;
  icon: string | null | undefined;
  opening: Opening;
  isActive: boolean;
  onHover: () => void;
  onActivate: () => void;
}) {
  const snippet = oneLine(hit.excerpt);

  return (
    <button
      type="button"
      data-active={isActive}
      onMouseMove={onHover}
      onClick={onActivate}
      className="flex w-full items-center gap-2.5 rounded-(--radius-control) px-2 py-1.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover data-[active=true]:bg-selected"
    >
      <KindMark icon={icon} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm text-fg">
            {hit.title ?? hit.id}
          </span>
          {hit.archived ? (
            <span className="shrink-0 text-xs text-fg-tertiary">Archived</span>
          ) : null}
        </span>
        {snippet === null ? null : (
          <span className="block truncate text-xs text-fg-tertiary">
            {snippet}
          </span>
        )}
      </span>
      {opening.outcome === "area" ? null : (
        <span className="shrink-0 text-xs text-fg-tertiary">
          {opening.outcome === "install"
            ? `Needs ${opening.extension.name}`
            : "No screen"}
        </span>
      )}
    </button>
  );
}

/**
 * What a result nothing opens says for itself.
 *
 * Not an error and not a warning: the record is there, it is intact, and the
 * only thing missing is a screen for its type. So this names the type, names
 * what publishes it, and leads to the one place a person can do something about
 * it. Installing is not offered here — what an extension does is described in
 * the catalogue, and installing one is a decision made there rather than in
 * passing from a search result.
 */
function Blocked({
  hit,
  opening,
  types,
  onShowExtension,
  onBack,
}: {
  hit: SearchHit;
  opening: Opening;
  types: readonly MemoryType[];
  onShowExtension: (id: string) => void;
  onBack: () => void;
}) {
  const kind = hit.kind ?? "";
  const named = kind === "" ? "no type" : typeName(types, kind);
  const extension = opening.outcome === "area" ? null : opening.extension;

  return (
    <div className="flex flex-col gap-3 p-4">
      <p className="text-sm text-fg">
        Nothing in this project opens{" "}
        <span className="font-medium">{hit.title ?? hit.id}</span>.
      </p>
      <p className="text-xs text-fg-secondary">
        {opening.outcome === "install" && extension
          ? `It is a ${named} record, and records of that type are shown by the ${extension.name} extension. This project has not installed it.`
          : extension
            ? `It is a ${named} record. ${extension.name} publishes that type, and this build of Sync carries no screen for it.`
            : `It is a ${named} record, and nothing this build knows about can show it. A project written by a newer version of Sync looks exactly like this.`}
      </p>
      <div className="flex items-center gap-2">
        {extension ? (
          <Button size="sm" onClick={() => onShowExtension(extension.id)}>
            Show in Extensions
          </Button>
        ) : null}
        <Button variant="outline" size="sm" onClick={onBack}>
          Back to results
        </Button>
      </div>
    </div>
  );
}

/**
 * How the store answered, said in the one place it does not interrupt.
 *
 * An installation with no embedding model searches words alone. That is a
 * normal state of a normal machine — not a degradation to apologise for and not
 * a thing to hide — so it is stated here, where somebody wondering why a
 * synonym did not match will look.
 */
function Footnote({
  answer,
  query,
  quiet,
}: {
  answer: ReturnType<typeof useSearch>;
  query: string;
  /** The palette is showing something other than an answer to a search. */
  quiet: boolean;
}) {
  if (quiet || query.trim() === "" || answer.error !== null) return null;

  // `total` is how many records match, not how many were read, so this says
  // the size of the finding rather than the size of the page. A capped count
  // is shown as a floor: the store stopped counting, and a bare "1000" would
  // read as a corpus that stopped growing.
  const total = answer.totalCapped ? `${answer.total}+` : `${answer.total}`;
  const found =
    answer.total === 0
      ? null
      : answer.hasMore
        ? `${answer.hits.length} of ${total}`
        : `${total} ${answer.total === 1 ? "result" : "results"}`;

  return (
    <div className="flex h-(--panel-header-height) shrink-0 items-center justify-between gap-3 border-t border-separator px-3 text-xs text-fg-tertiary">
      <span className="truncate">
        {answer.degraded
          ? "Words only: this installation has no embedding model."
          : answer.mode === "hybrid"
            ? "Words and meaning."
            : "Words."}
      </span>
      {found === null ? null : <span className="shrink-0">{found}</span>}
    </div>
  );
}

/** What the palette says while it has nothing to list. */
function Quiet({ headline, detail }: { headline: string; detail?: string }) {
  return (
    <div className="flex flex-col gap-1 px-4 py-6">
      <p className="text-sm text-fg-secondary">{headline}</p>
      {detail === undefined ? null : (
        <p className="text-xs text-fg-tertiary">{detail}</p>
      )}
    </div>
  );
}

/**
 * One line of the body, for a row that is two lines tall.
 *
 * The store cuts the window — around the words that matched, or from the top
 * when nothing matched by words and only meaning did — so a row shows the
 * place the record answered from rather than however it happens to begin. Two
 * records that open with the same preamble are told apart here, which is what
 * a row this size is for. Trimmed again because a window is sized for a
 * context, and a row is narrower than that.
 */
function oneLine(excerpt: string | null): string | null {
  if (excerpt === null) return null;
  const flattened = excerpt.replace(/\s+/g, " ").trim();
  if (flattened === "") return null;
  return flattened.length > 160 ? `${flattened.slice(0, 160)}…` : flattened;
}
