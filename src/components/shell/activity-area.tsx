"use client";

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { KindGlyph, KindMark } from "@/components/shell/entity-marks";
import type { Opener, Opening } from "@/components/shell/opening";
import {
  FooterAction,
  PanelFooter,
  PanelHeader,
  PanelPlaceholder,
  PanelSurface,
} from "@/components/shell/panel";
import { TypeFilter } from "@/components/shell/type-filter";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Check, CheckCheck, Inbox } from "lucide-react";
import { useAppMenu } from "@/lib/app-menu";
import { elapsed } from "@/lib/elapsed";
import type { AreaModule, AreaProviderProps } from "@/lib/extension-host/activate";
import { memoryTypes } from "@/lib/memory/client";
import type { Activity, ActivityEntry } from "@/lib/memory/use-activity";
import type { MemoryType } from "@/lib/memory/types";
import { typeName } from "@/lib/memory/use-corpus";
import { showNativeContextMenu } from "@/lib/native-menu";
import type { ProjectViewState } from "@/lib/project/use-project-view";
import { cn } from "@/lib/utils";

/**
 * Activity, as an area of the window.
 *
 * The second screen the shell owns, and the one that is a **view of the corpus
 * rather than a part of it**: it names no type, no extension and no section.
 * What it lists is a key, a kind and a title; where each one opens is asked of
 * `opening.ts`, which is the same lookup the search palette makes. That is what
 * keeps the rule the rest of the window keeps — the shell is empty of subject
 * matter — while still showing everything at once.
 *
 * **The arrangement is the one macOS uses for a history.** The navigator is a
 * source list of what there is *of* — every change, then one row per kind that
 * has any — and the workspace is the changes themselves. Mail, Reminders and
 * Notes all read this way, and the reason is not the resemblance: the column
 * that answers *where am I* has to hold something a person can come back to,
 * and a single change is not that. It is gone the moment it is read.
 *
 * An earlier pass had it the other way round — the changes in the navigator and
 * one of them drawn in the workspace — and the workspace ended up a page of
 * three facts already printed in the row beside it, with the whole width spent
 * on a button.
 *
 * **It does not edit anything.** The rows describe the event: which record, of
 * what kind, whose hand and when. The record itself is drawn by whichever
 * section opens that kind, because the record inspector belongs to that section
 * — `docs/extensions.md` §9a — and a second one drawn here would be this screen
 * deciding how somebody else's type is shown.
 *
 * Two commands, and they are not each other. *Open* takes a person to the
 * record, in its own section. *Mark as seen* says they have looked. Opening one
 * does not put it away: going to look at something and being done with it are
 * two decisions, and a list that made the first into the second would empty
 * itself under somebody who is working through it. Publishing memory to a
 * remote is a third thing and is nowhere near this screen: a change pushed
 * unread would otherwise count as read.
 */

interface ActivityShell {
  readonly activity: Activity;
  /**
   * Which kinds this person does not want reported, on this machine.
   *
   * The same state the figure on the sidebar is counted through, held by the
   * window rather than read again here: two readers of one preference is how
   * the count and the list come to disagree.
   */
  readonly watched: ProjectViewState;
  /**
   * What this window can do with a record of that kind, and what to say when it
   * can do nothing.
   *
   * Asked before a row is drawn rather than discovered from pressing it: a row
   * that explains itself after the fact is one that should not have looked
   * pressable. The lookup is the window's because it needs what is unpacked,
   * what the project declares and what actually mounted — three answers this
   * screen has no business holding.
   */
  readonly opening: Opener;
  /**
   * Take somebody to a record, in whichever section opens its kind.
   *
   * Held by the window for the same reason the lookup above is.
   */
  readonly open: (key: string, kind: string) => void;
}

const ShellContext = createContext<ActivityShell | null>(null);

/**
 * What the window knows about the activity, given to the area and to the row in
 * the sidebar both.
 *
 * Mounted above the areas rather than inside this one, because the count on the
 * sidebar has to be true for somebody who has never opened this screen — the
 * same reason a declared badge is answered without the section running.
 */
export function ActivityProvider({
  value,
  children,
}: {
  value: ActivityShell;
  children: ReactNode;
}) {
  return <ShellContext.Provider value={value}>{children}</ShellContext.Provider>;
}

function useShell(): ActivityShell {
  const shell = useContext(ShellContext);
  if (shell === null) {
    throw new Error("Activity is drawn outside the window that holds it");
  }
  return shell;
}

/**
 * One kind that has changed, and how many of it has.
 *
 * The count is exactly what the rows under it will be, so the figure beside a
 * kind and the number of rows a person then sees are one answer rather than two
 * that have to agree.
 */
interface ChangedKind {
  readonly kind: string;
  readonly count: number;
}

/** Which filter is chosen, and what the navigator has to draw it with. */
const SelectionContext = createContext<{
  /** The chosen kind, or `null` for every change at once. */
  readonly kind: string | null;
  readonly choose: (kind: string | null) => void;
  readonly types: readonly MemoryType[];
  readonly kinds: readonly ChangedKind[];
  /** The changes the choice leaves, which is what both columns are about. */
  readonly shown: readonly ActivityEntry[];
} | null>(null);

function useSelection() {
  const selection = useContext(SelectionContext);
  if (selection === null) {
    throw new Error("An activity column is drawn outside its area");
  }
  return selection;
}

/**
 * What this area holds: which kind is chosen, and the types it draws them with.
 *
 * The types are read when the area mounts rather than held for the life of the
 * window, which is where the search palette reads them too — and for the same
 * reason: a type published while nobody was looking at this screen would
 * otherwise be missing from the one control that can switch it back on.
 */
function ActivityAreaProvider({ project, active, children }: AreaProviderProps) {
  const { activity } = useShell();
  const [chosen, choose] = useState<string | null>(null);
  const [types, setTypes] = useState<readonly MemoryType[]>(NO_TYPES);

  // Nothing here writes a record or names a type. Saying so is what takes the
  // previous area's commands away: an area that stayed silent would leave `⌘N`
  // offering to write a record while the window is showing a history.
  useAppMenu(
    {
      selected: null,
      createRecord: null,
      createType: null,
      table: null,
    },
    active,
  );

  useEffect(() => {
    let current = true;
    void memoryTypes(project.path).then(
      (published) => {
        if (current) setTypes(published);
      },
      // The filter is the only thing that wants them, and a filter with nothing
      // to offer draws nothing. Not worth a message over a screen that
      // otherwise works.
      () => undefined,
    );
    return () => {
      current = false;
    };
  }, [project.path]);

  const kinds = useMemo(() => counted(activity.entries), [activity.entries]);

  const value = useMemo(() => {
    // A kind that no longer has a change is a row that is no longer there, and
    // the choice falls back to everything rather than to an empty column. Read
    // from the list in hand instead of corrected in an effect: an effect would
    // draw the empty column once before putting it right.
    const kind =
      chosen !== null && kinds.some((row) => row.kind === chosen) ? chosen : null;
    return {
      kind,
      choose,
      types,
      kinds,
      shown:
        kind === null
          ? activity.entries
          : activity.entries.filter((entry) => entry.kind === kind),
    };
  }, [activity.entries, chosen, kinds, types]);

  return (
    <SelectionContext.Provider value={value}>{children}</SelectionContext.Provider>
  );
}

const NO_TYPES: readonly MemoryType[] = [];

/**
 * The kinds that have changed, newest first, with how many changes each has.
 *
 * The order is the order the changes arrived in rather than an alphabet: what a
 * person is looking for is what has just happened, and a list sorted by name
 * puts that anywhere.
 */
function counted(entries: readonly ActivityEntry[]): readonly ChangedKind[] {
  const kinds = new Map<string, number>();
  for (const entry of entries) {
    kinds.set(entry.kind, (kinds.get(entry.kind) ?? 0) + 1);
  }
  return [...kinds].map(([kind, count]) => ({ kind, count }));
}

/**
 * What there is to look at, by kind.
 *
 * A source list rather than the changes themselves: this column answers *where
 * am I*, and it has to hold something that is still there tomorrow. `All
 * changes` is first and ungrouped for the reason `All Inboxes` is in Mail — it
 * is not one of the things the rows under it count, it is every one of them.
 */
function ActivityNavigator() {
  const { activity, watched } = useShell();
  const { kind, choose, types, kinds } = useSelection();

  return (
    <PanelSurface className="bg-panel">
      <PanelHeader title="Activity" />
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 px-2 pt-2 pb-3">
          <div className="flex flex-col gap-0.5">
            <FilterRow
              label="All changes"
              icon={Inbox}
              count={activity.entries.length}
              isActive={kind === null}
              onSelect={() => choose(null)}
            />
          </div>

          {kinds.length === 0 ? null : (
            <div className="flex flex-col gap-0.5">
              {kinds.map((row) => (
                <FilterRow
                  key={row.kind}
                  label={typeName(types, row.kind)}
                  icon={types.find((type) => type.kind === row.kind)?.icon}
                  count={row.count}
                  isActive={kind === row.kind}
                  onSelect={() => choose(row.kind)}
                />
              ))}
            </div>
          )}

          {activity.isLoading || activity.entries.length > 0 ? null : (
            // One line, not the sentence the workspace carries. Two columns
            // saying the same thing is the window saying one thing twice and
            // neither of them saying what its own column is for — and the two
            // absences are still two sentences, because one is what the project
            // did and the other is what this window could not find out.
            <p className="px-2 text-xs text-fg-tertiary">
              {activity.failed
                ? "Nothing here says what has changed."
                : "Kinds appear here as they change."}
            </p>
          )}
        </div>
      </ScrollArea>

      {/* The band macOS keeps for what acts on a list — Mail, Reminders,
          Music, Xcode's navigator — carrying the view preference on its
          trailing edge. It decides what is reported at all, which is a
          different question from which kind is being looked at above: one is
          held for ever and travels to the sidebar's figure, the other is where
          somebody is standing this minute. What acts on the changes acts on
          them where they are, in the column beside this one. */}
      <PanelFooter>
        <div className="ml-auto">
          <TypeFilter types={types} view={watched} verb="reported" align="end" />
        </div>
      </PanelFooter>
    </PanelSurface>
  );
}

/**
 * One row of the navigator: a kind, and how many of it changed.
 *
 * The figure sits at the trailing edge because that is where a source list on
 * this system keeps one, and it carries no colour: a count is information, and
 * this window keeps colour for status and for destruction. The row says the
 * whole of itself to assistive technology rather than leaving a number to be
 * read as a second word.
 */
function FilterRow({
  label,
  icon,
  count,
  isActive,
  onSelect,
}: {
  label: string;
  /** A mark's name, from a published type, or a component for the row above. */
  icon: string | null | undefined | typeof Inbox;
  count: number;
  isActive: boolean;
  onSelect: () => void;
}) {
  const Glyph = typeof icon === "function" ? icon : null;

  return (
    <button
      type="button"
      data-active={isActive}
      aria-current={isActive ? "true" : undefined}
      aria-label={`${label}, ${count} ${count === 1 ? "change" : "changes"}`}
      onClick={onSelect}
      className="group flex h-(--control-height-lg) w-full items-center gap-2.5 rounded-(--radius-control) px-2 text-left text-base text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg data-[active=true]:bg-accent-fill data-[active=true]:font-medium data-[active=true]:text-accent-on"
    >
      {Glyph === null ? (
        <KindGlyph
          icon={icon as string | null | undefined}
          className="size-4 shrink-0 opacity-80 group-data-[active=true]:opacity-100"
        />
      ) : (
        <Glyph
          aria-hidden="true"
          // Full strength on the filled row. White on the accent is already at
          // the ceiling this window's contrast rule allows, so four fifths of it
          // lands under the floor — the tier a glyph carries on a selected row
          // is its size, which costs no contrast.
          className="size-4 shrink-0 opacity-80 group-data-[active=true]:opacity-100"
        />
      )}
      <span className="truncate">{label}</span>
      <span
        aria-hidden="true"
        className="ml-auto shrink-0 text-xs font-normal text-fg-tertiary group-data-[active=true]:text-accent-on"
      >
        {count}
      </span>
    </button>
  );
}

/**
 * The changes themselves, newest first, one row per record.
 *
 * A record written three times is one row saying so, not three rows: the person
 * has one thing to go and look at, and a list that counted the writes would be
 * counting how an agent works rather than what it did.
 */
function ActivityWorkspace() {
  const { activity } = useShell();
  const { kind, types, shown } = useSelection();
  const everything = kind === null;

  return (
    <PanelSurface className="bg-workspace">
      {/* The header names what is being shown of the column beside it, which is
          the filter that column chose. It carries no control: the one command a
          header may carry is the one that writes into the thing it names, and
          nothing here writes a record. */}
      <PanelHeader title={everything ? "All changes" : typeName(types, kind)} />

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-0.5 p-2">
          {shown.length > 0 ? null : (
            <div className="px-2 pt-1">
              {activity.failed ? (
                <PanelPlaceholder
                  headline="The history could not be read."
                  detail="Nothing here says whether anything changed. Coming back to the window asks again."
                />
              ) : activity.isLoading ? null : (
                <PanelPlaceholder
                  headline="No changes since you last looked."
                  detail="What an agent writes, and what arrives from a colleague, appears here."
                />
              )}
            </div>
          )}

          {shown.map((entry) => (
            <ChangeRow key={entry.key} entry={entry} types={types} />
          ))}

          {activity.hasMore && everything ? (
            // Said rather than left to be inferred from a list that stops. What
            // is missing is older than the last row, and a column that quietly
            // truncated would read as "this is all of it".
            <p className="px-2 pt-2 text-xs text-fg-tertiary">
              Older changes are not shown.
            </p>
          ) : null}
        </div>
      </ScrollArea>

      {/* The bar belongs to the list above it, by the same rule the navigator's
          does: what acts on a list sits under it, in the band that does not
          scroll away. It says how far it reaches, because those are two
          different acts — one moves the reading mark across the whole history,
          the other puts away the rows on screen and leaves the mark alone. */}
      <PanelFooter>
        <FooterAction
          icon={CheckCheck}
          label={everything ? "Mark all as seen" : "Mark these as seen"}
          disabled={shown.length === 0}
          onSelect={() => {
            if (everything) activity.markSeen();
            else activity.dismiss(shown.map((entry) => entry.key));
          }}
        />
      </PanelFooter>
    </PanelSurface>
  );
}

/**
 * One change: what happened to a record, and the two things to do about it.
 *
 * The row is the command. A change is a thing to go and look at, so pressing it
 * goes and looks — the same gesture a search result answers to, and deliberately
 * the same lookup behind it.
 *
 * A change nothing can open is **not** pressable, and says why where it stands.
 * The palette can afford to open an explanation instead, because it has a whole
 * surface to put one on; a row in a list has the row, so the reason goes in it
 * and the row stops pretending. It is still a row, and it can still be put away.
 */
function ChangeRow({
  entry,
  types,
}: {
  entry: ActivityEntry;
  types: readonly MemoryType[];
}) {
  const { activity, opening, open } = useShell();
  const where = opening(entry.kind);
  // A deleted record has nothing left to open whatever owns its kind, and that
  // is the first question rather than the second: an extension that would have
  // shown it cannot show something that is gone.
  const openable = entry.change !== "deleted" && where.outcome === "area";
  const name = named(entry);
  const line = said(entry);
  const when = elapsed(entry.at);

  const content = (
    <>
      <KindMark icon={types.find((type) => type.kind === entry.kind)?.icon} />
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className="truncate text-sm text-fg">{name}</span>
          <span className="ml-auto shrink-0 text-xs text-fg-tertiary">
            {when}
          </span>
        </span>
        <span className="flex items-baseline gap-2">
          <span className="truncate text-xs text-fg-tertiary">{line}</span>
          {openable ? null : (
            <span className="ml-auto shrink-0 text-xs text-fg-tertiary">
              {refusal(entry, where)}
            </span>
          )}
        </span>
      </span>
    </>
  );

  return (
    <div
      className="group flex items-center gap-1 rounded-(--radius-control) pr-1 transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover"
      onContextMenu={(event) => {
        // The system's own menu, as every other row in this window answers a
        // secondary click with. Both commands are offered whether or not they
        // can be performed, so the menu is the same shape over every row and
        // the one that is unavailable says so by being dim rather than by being
        // missing.
        showNativeContextMenu(event, [
          {
            label: "Open",
            enabled: openable,
            onSelect: () => open(entry.key, entry.kind),
          },
          "separator",
          {
            label: "Mark as Seen",
            onSelect: () => activity.dismiss([entry.key]),
          },
        ]);
      }}
    >
      {openable ? (
        <button
          type="button"
          // The whole of the row, said once. A screen reader would otherwise
          // read four fragments in a row and leave which record they are about
          // to the listener.
          aria-label={`${name}. ${line}. ${when}`}
          onClick={() => open(entry.key, entry.kind)}
          className="flex min-w-0 flex-1 items-center gap-2.5 rounded-(--radius-control) px-2 py-1.5 text-left"
        >
          {content}
        </button>
      ) : (
        <div className="flex min-w-0 flex-1 items-center gap-2.5 px-2 py-1.5">
          {content}
        </div>
      )}

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Mark ${name} as seen`}
            onClick={() => activity.dismiss([entry.key])}
            // Shown when the row is pointed at, the way a row's own commands
            // are on this system — and never *only* then. A control that exists
            // solely under a pointer is one nobody reaches by keyboard and one
            // that does not exist at all on a touch screen, so it comes back
            // for focus and stands permanently where there is no fine pointer.
            className={cn(
              "shrink-0 text-fg-tertiary opacity-0 transition-opacity duration-(--motion-duration-fast) hover:text-fg",
              "group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100",
            )}
          >
            <Check />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Mark as seen</TooltipContent>
      </Tooltip>
    </div>
  );
}

/**
 * What to call the record.
 *
 * The title where the history carried one, and the key where it did not. A
 * record can be written without a title, and a row reading "Untitled" would
 * name nothing a person could recognise — the key at least is what they would
 * search for.
 */
function named(entry: ActivityEntry): string {
  return entry.title ?? entry.key;
}

/** What happened to it, in a line: what was done, by whom, and how often. */
function said(entry: ActivityEntry): string {
  const parts = [`${verb(entry.change)} by ${hand(entry.source)}`];
  if (entry.writes > 1) parts.push(`${entry.writes} writes`);
  return parts.join(" · ");
}

/**
 * Why this row does not open, in the words the palette uses for the same three
 * states.
 *
 * One vocabulary for one fact: a person who has read *No screen* under a search
 * result should not have to learn a second sentence for it here.
 */
function refusal(entry: ActivityEntry, where: Opening): string {
  if (entry.change === "deleted") return "Deleted";
  if (where.outcome === "install") return `Needs ${where.extension.name}`;
  return "No screen";
}

function verb(change: ActivityEntry["change"]): string {
  if (change === "added") return "Added";
  if (change === "deleted") return "Deleted";
  if (change === "modified") return "Changed";
  // A word from an engine newer than this build. Shown as it came rather than
  // translated into one of the three above, which would be this window
  // guessing what somebody else's vocabulary means.
  return change;
}

/**
 * Whose hand it was, in the words a person uses.
 *
 * `unknown` is a write path this build has never heard of, and it says so
 * rather than claiming a hand: attributing it to an agent or to a person would
 * both be inventions, and it is still a change worth looking at.
 */
function hand(source: ActivityEntry["source"]): string {
  if (source === "agent") return "an agent";
  if (source === "window") return "you";
  if (source === "housekeeping") return "Sync";
  return "something else";
}

export const ACTIVITY_AREA_MODULE: AreaModule = {
  Provider: ActivityAreaProvider,
  Navigator: ActivityNavigator,
  Workspace: ActivityWorkspace,
};
