"use client";

import { Check, ChevronRight, FilePlus2, FolderPlus, Store } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";

import { KindGlyph, KindMark } from "@/components/shell/entity-marks";
import {
  PackageTags,
  describePackage,
  useActivationOutcomes,
  useAddPackage,
  useForgetPackage,
} from "@/components/shell/extension-packages";
import {
  PanelBody,
  PanelFooter,
  PanelHeader,
  PanelSurface,
} from "@/components/shell/panel";
import { Button } from "@/components/ui/button";
import type { Manifest } from "@/lib/extension-host/client";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useCompositionContext } from "@/lib/composition";
import type {
  ListedExtension,
  RegistryArtefact,
} from "@/lib/extension-host/client";
import {
  changelogOf,
  useLedger,
  type AvailableUpdate,
} from "@/lib/extension-host/updates";
import type { CatalogueEntry } from "@/lib/extension-host/catalogue";
import type { Marketplace } from "@/lib/extension-host/marketplace";
import type { Outcome } from "@/components/shell/extension-packages";
import { cn } from "@/lib/utils";

/**
 * The Extensions area, in the three columns every area is read in.
 *
 * Every word on these columns comes out of a package. The shell used to hold a
 * constant with a name, a summary, a description and a list of what each
 * extension added to the window — prose about three extensions, written in the
 * application that shipped them, which is exactly the arrangement that only
 * works for extensions we wrote. A package describes itself now, and what the
 * catalogue does is arrange those descriptions.
 *
 * The navigator lists the marketplace and then the sections this project
 * actually has. The workspace is either the marketplace — every extension there
 * is, as cards — or one extension: what it is, what it brings, the types it
 * would publish, what it tells an agent. The inspector is the package: where it
 * came from, whether it is signed, which Sync it needs, and the two different
 * things removing it can mean.
 *
 * **A row and a card are two different claims**, which is why the marketplace
 * is a destination rather than a group in the list. A row in the navigator says
 * *this is a part of this window*; a card says *this is something a project
 * could install*. Listing an uninstalled package as a row said the first about
 * something for which only the second was true, and it read — correctly — as
 * the window claiming a section it did not have.
 *
 * Nothing here simulates an extension running. What a person is deciding is
 * whether a project should be able to do something, and the answer to that is a
 * description of what would be installed — stated in full, so that installing
 * is never a surprise, and marked unavailable wherever it is.
 */

export function ExtensionNavigator({
  installed,
  selectedId,
  onSelect,
}: {
  installed: readonly CatalogueEntry[];
  /** The extension being shown, or `null` for the marketplace. */
  selectedId: string | null;
  onSelect: (id: string | null) => void;
}) {
  const adding = useAddPackage();

  return (
    <PanelSurface className="bg-panel">
      <PanelHeader title="Extensions" />
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 px-2 pt-2 pb-3">
          {/* First and ungrouped, because it is not one of the things the
              groups below are counting. It is where every one of them came
              from, and on a project that has installed nothing it is the only
              row there is — which is the right first screen rather than an
              empty column apologising for itself. */}
          <div className="flex flex-col gap-0.5">
            <Row
              label="Marketplace"
              icon={Store}
              isActive={selectedId === null}
              onSelect={() => onSelect(null)}
            />
          </div>

          {adding.failure === null ? null : (
            <p className="px-2 font-mono text-xs leading-4 text-danger">
              {adding.failure}
            </p>
          )}

          {/* A group with nothing in it is not drawn: an empty heading names a
              state instead of showing one. */}
          {installed.length === 0 ? null : (
            <section className="flex flex-col gap-0.5">
              <h3 className="px-2 pb-0.5 text-xs font-semibold text-fg-tertiary">
                Installed
              </h3>

              {installed.map((entry) => (
                <Row
                  key={entry.id}
                  label={entry.name}
                  icon={entry.packaged?.manifest.icon}
                  trailing={entry.version}
                  isActive={entry.id === selectedId}
                  // Still a row and still opens: what is unavailable is the
                  // extension's sections, not the page about it, and the page
                  // is where the reason is written out. Dropping it from the
                  // list would take away the one place it can be read.
                  dimmed={entry.unavailable !== null}
                  onSelect={() => onSelect(entry.id)}
                />
              ))}
            </section>
          )}
        </div>
      </ScrollArea>

      {/* The band macOS keeps for what acts on a list — Mail, Reminders,
          Music, Xcode's navigator — with the actions on the leading edge. Both
          of these add to the list above, which is what puts them here rather
          than in a header or in the panel beside this one.

          They were under the selected package's facts in the inspector until
          2026-08-24, which made the only door in the application open from a
          room nobody could reach: a machine with nothing unpacked selects no
          card, the inspector draws nothing, and there was no button anywhere.
          That is the state everybody starts in. */}
      <PanelFooter>
        <FooterAction
          icon={FilePlus2}
          label="Add from a package file"
          disabled={adding.busy}
          onSelect={() => void adding.add("file")}
        />
        <FooterAction
          icon={FolderPlus}
          label="Add from a folder"
          disabled={adding.busy}
          onSelect={() => void adding.add("folder")}
        />
      </PanelFooter>
    </PanelSurface>
  );
}

/**
 * One command in the bottom bar, in the weight that band is drawn at.
 *
 * Tertiary until it is pointed at, like every other control in a bottom bar and
 * like the pinned row in the sidebar beside it: the bar is furniture, and a
 * control at full weight in it reads as the loudest thing in a column whose
 * subject is the list above. The name is a tooltip rather than a `title`,
 * because `title` is the system's tooltip and arrives late enough that people
 * stop waiting for it.
 */
function FooterAction({
  icon: Icon,
  label,
  disabled,
  onSelect,
}: {
  icon: LucideIcon;
  label: string;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={label}
          disabled={disabled}
          onClick={onSelect}
          className="text-fg-tertiary hover:text-fg"
        >
          <Icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * One row of the navigator, for the marketplace and for a section alike.
 *
 * The mark is the one the manifest asked for, resolved against the shell's own
 * table — an extension names an icon rather than drawing one, so a name this
 * build cannot draw is shown neutrally instead of guessed at. The marketplace
 * passes a component instead, because it is the shell's own row and there is no
 * manifest behind it to ask.
 */
function Row({
  label,
  icon,
  trailing,
  isActive,
  dimmed,
  onSelect,
}: {
  label: string;
  /** A mark's name, from a manifest, or a component for the shell's own row. */
  icon: string | null | undefined | LucideIcon;
  trailing?: string;
  isActive: boolean;
  /**
   * The package is in this project and does nothing on this machine.
   *
   * A tier and not a state: the row behaves exactly as the others do, because
   * what it opens — the page describing the package — works everywhere. What
   * the weight says is that the sections it brings are not here.
   */
  dimmed?: boolean;
  onSelect: () => void;
}) {
  const Glyph = typeof icon === "function" ? icon : null;

  return (
    <button
      type="button"
      data-active={isActive}
      aria-current={isActive ? "true" : undefined}
      onClick={onSelect}
      className={cn(
        "flex h-(--control-height-lg) w-full items-center gap-2.5 rounded-(--radius-control) px-2 text-left text-base text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg data-[active=true]:bg-selected data-[active=true]:font-medium data-[active=true]:text-fg",
        dimmed && "text-fg-tertiary",
      )}
    >
      {Glyph === null ? (
        <KindGlyph
          icon={icon as string | null | undefined}
          className="size-4 shrink-0 opacity-80"
        />
      ) : (
        <Glyph aria-hidden="true" className="size-4 shrink-0 opacity-80" />
      )}
      <span className="truncate">{label}</span>
      {trailing === undefined ? null : (
        <span className="ml-auto shrink-0 font-mono text-xs font-normal text-fg-tertiary">
          {trailing}
        </span>
      )}
    </button>
  );
}

/**
 * Every extension there is, as cards.
 *
 * This is the screen the area opens on, and the one that makes `Extensions` an
 * area at all rather than a list: what a person arrives to decide is *what
 * could this project do*, and the answer to that is a set of things to compare,
 * not a column of names to click one at a time.
 *
 * **Every entry is here and every card says where it stands.** The four states
 * are all reachable in ordinary use and none of them is hidden: in this
 * project; unpacked and never asked for; asked for and absent; and unpacked and
 * refused by this build. The last one is the reason nothing is filtered out —
 * a package on the disk that will not run is still taking up room, and this is
 * now the only place it can be seen and removed.
 *
 * A card is not a decision. Choosing one opens the extension's own page, which
 * is where installing happens, because what a project is agreeing to — the
 * types it would publish, the sections it would gain, what it tells an agent —
 * does not fit on a card and must not be agreed to without it.
 */
export function ExtensionMarketplace({
  entries,
  marketplace,
  updates,
  onOpen,
}: {
  entries: readonly CatalogueEntry[];
  marketplace: Marketplace;
  /** What the registry has that is newer than what this project runs, by id. */
  updates: ReadonlyMap<string, AvailableUpdate>;
  onOpen: (id: string) => void;
}) {
  const packaged = useMemo(
    () =>
      entries
        .map((entry) => entry.packaged)
        .filter((one): one is NonNullable<typeof one> => one !== null),
    [entries],
  );
  const outcomes = useActivationOutcomes(packaged);

  // What somebody typed, and it narrows this page rather than asking anybody
  // anything. The whole registry is one file already read, so search is a
  // filter over what is on screen — no request, no debounce, no spinner, and
  // no state where the field has been typed into and the list has not caught
  // up.
  const [asked, setAsked] = useState("");
  const shown = useMemo(() => matching(entries, asked), [entries, asked]);

  return (
    <section className="flex h-full min-w-0 flex-col bg-workspace">
      {/* The header names what the column is showing and carries the one
          control that acts on it. A search field belongs here rather than in
          the title bar for the reason the window's own search does not: this
          one searches the marketplace, and a field in the band above every
          column would claim to search the project. */}
      <div className="flex h-(--panel-header-height) shrink-0 items-center gap-3 border-b border-separator px-3">
        <h2 className="min-w-0 shrink-0 truncate text-sm font-semibold text-fg">
          Marketplace
        </h2>
        <input
          type="search"
          value={asked}
          onChange={(event) => setAsked(event.target.value)}
          placeholder="Search extensions"
          aria-label="Search extensions"
          className="h-(--control-height-sm) min-w-0 flex-1 rounded-(--radius-control) border border-separator bg-raised px-2 text-sm text-fg placeholder:text-fg-tertiary"
        />
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-4 p-4">
          <MarketplaceSource marketplace={marketplace} />

          {entries.length === 0 ? (
            <p className="max-w-[68ch] text-sm leading-5 text-fg-tertiary">
              Nothing is unpacked, this project declares nothing, and the
              registry lists nothing. That is a real project and not a broken
              one: a window with no sections is what Sync is before anybody has
              said what it should hold.
            </p>
          ) : shown.length === 0 ? (
            // The one absence worth a sentence of its own, because it is the
            // one a person caused: they typed something, and it matched none of
            // a list they can see the size of.
            <p className="max-w-[68ch] text-sm leading-5 text-fg-tertiary">
              Nothing here matches “{asked}”. {entries.length} extensions are
              listed.
            </p>
          ) : (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(19rem,1fr))] gap-2">
              {shown.map((entry) => (
                <MarketplaceCard
                  key={entry.id}
                  entry={entry}
                  update={updates.get(entry.id) ?? null}
                  outcome={
                    entry.packaged === null
                      ? null
                      : (outcomes[entry.id] ?? null)
                  }
                  onOpen={() => onOpen(entry.id)}
                />
              ))}
            </ul>
          )}
        </div>
      </ScrollArea>
    </section>
  );
}

/**
 * Which entries a typed word keeps, over what a person can see of them.
 *
 * The name, the summary and the kinds it publishes — the three things on a
 * card, plus the one that is the reason somebody is looking: an extension is
 * found by what it *holds*, so typing "decision" finds whatever publishes one.
 * The id is in it too, because an id is what an agent is told and what a
 * project's record names.
 *
 * Every word has to appear somewhere, so a second word narrows rather than
 * widens. Nothing is ranked: this is a filter over a list of a dozen, and a
 * relevance order over a dozen cards would be a claim nobody can check.
 */
function matching(
  entries: readonly CatalogueEntry[],
  asked: string,
): readonly CatalogueEntry[] {
  const words = asked.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return entries;

  return entries.filter((entry) => {
    const haystack = [
      entry.id,
      entry.name,
      entry.packaged?.manifest.summary ?? entry.listed?.summary ?? "",
      ...(entry.packaged?.types.map((type) => type.kind) ??
        entry.listed?.publishes ??
        []),
    ]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}

/**
 * Where this list came from, said only when it is not the obvious answer.
 *
 * Silence is the state for a registry that answered: what is on screen is what
 * exists, and a line saying so would be the page congratulating itself. What is
 * worth saying is the two states that are not that — a list that is a day old
 * because there was no network, and no list at all — and in both the control to
 * ask again is beside the sentence, because that is the one thing a person can
 * do about either.
 */
function MarketplaceSource({ marketplace }: { marketplace: Marketplace }) {
  const { cached, failure, isLoading, reload } = marketplace;

  if (isLoading) {
    return (
      <p className="text-sm leading-5 text-fg-tertiary">
        Reading what extensions there are…
      </p>
    );
  }

  if (failure !== null) {
    return (
      <div className="flex max-w-[68ch] flex-col items-start gap-2">
        <p className="text-sm leading-5 text-fg-secondary">
          The registry could not be reached and nothing was cached, so this is
          only what is already on this machine.
        </p>
        {/* On its own line rather than inside the sentence: what comes back is
            the network's own words, and they begin lower case and end where
            they end. Splicing them into a sentence produces a full stop
            followed by a small letter, every time. */}
        <p className="text-sm leading-5 text-fg-tertiary">{failure}</p>
        <Button size="sm" variant="secondary" onClick={reload}>
          Try again
        </Button>
      </div>
    );
  }

  if (cached) {
    return (
      <div className="flex max-w-[68ch] flex-col items-start gap-2">
        <p className="text-sm leading-5 text-fg-secondary">
          These are the extensions there were when this machine last reached the
          registry.
        </p>
        <Button size="sm" variant="secondary" onClick={reload}>
          Check again
        </Button>
      </div>
    );
  }

  return (
    <p className="max-w-[68ch] text-sm leading-5 text-fg-secondary">
      Everything this project can do arrives as a package. One gets here from
      the registry, or from a file or a folder — both are at the foot of the
      list beside this.
    </p>
  );
}

function MarketplaceCard({
  entry,
  update,
  outcome,
  onOpen,
}: {
  entry: CatalogueEntry;
  update: AvailableUpdate | null;
  outcome: Outcome;
  onOpen: () => void;
}) {
  const { packaged } = entry;

  return (
    <li className="min-w-0">
      <button
        type="button"
        onClick={onOpen}
        className={cn(
          "flex h-full w-full flex-col gap-2 rounded-(--radius-surface) border border-separator bg-panel/60 p-3 text-left transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-panel",
          // Held back rather than crossed out. The card still opens, because
          // the page behind it is where the reason is written and where the
          // package can still be installed for the machines that do run it.
          entry.unavailable === null ? null : "opacity-60",
        )}
      >
        <div className="flex items-start gap-2">
          <span
            aria-hidden="true"
            className="flex size-7 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary"
          >
            <KindGlyph icon={packaged?.manifest.icon ?? entry.listed?.icon} className="size-4" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="flex items-baseline justify-between gap-2">
              <span className="truncate text-base font-medium text-fg">
                {entry.name}
              </span>
              <span className="shrink-0 font-mono text-xs text-fg-tertiary">
                {entry.version}
              </span>
            </p>
            <Standing entry={entry} update={update} />
          </div>
        </div>

        {/* A package describes itself better than an index entry does — it is
            the manifest rather than a summary of one — so where both exist the
            package's words are the ones shown. The sentence at the end is for
            the one entry neither answered for: a dependency this project
            declared and nothing on this machine or in the registry has. */}
        <p className="text-xs leading-4 text-fg-tertiary">
          {packaged?.manifest.summary ||
            entry.listed?.summary ||
            "This project depends on it, and nothing answers to the name."}
        </p>

        {packaged === null ? null : <PackageTags extension={packaged} />}

        {/* Three different things it could say, and only ever one of them. The
            first is true of the package whether or not anybody ran it; the
            second is true of this machine and says nothing about the package,
            which is why it is not in the danger tier; the third is what
            happened when somebody ran it. The second comes before the third
            because on a phone the activation was refused for exactly this
            reason, and printing both would be the same sentence twice, once in
            red. */}
        {entry.unrunnable !== null ? (
          <p className="text-xs leading-4 text-danger">{entry.unrunnable}</p>
        ) : entry.unavailable !== null ? (
          <p className="text-xs leading-4 text-fg-secondary">
            {entry.unavailable}
          </p>
        ) : outcome !== null && outcome !== "" ? (
          <p className="font-mono text-xs leading-4 text-danger">{outcome}</p>
        ) : packaged !== null && outcome === "" ? (
          <p className="text-xs leading-4 text-fg-tertiary">
            {describePackage(packaged)}
          </p>
        ) : null}
      </button>
    </li>
  );
}

/** What the page needs to draw and move one extension's clock. */
export interface ClockControl {
  readonly isOn: boolean;
  /** True while the switch is being written, so it cannot be asked twice. */
  readonly isBusy: boolean;
  readonly onChange: (on: boolean) => void;
}

export function ExtensionPage({
  entry,
  update,
  onInstall,
  onChange,
  onRemove,
  isBusy,
  failure,
  onDismissFailure,
  clock,
}: {
  entry: CatalogueEntry;
  /** A version the registry lists that this project is not on, or `null`. */
  update: AvailableUpdate | null;
  onInstall: () => void;
  /** Move to another published version. The same command in both directions. */
  onChange: (artefact: RegistryArtefact) => void;
  onRemove: () => void;
  /** True while the store is being written, so the command cannot be asked twice. */
  isBusy: boolean;
  /**
   * Why the last install or removal did not happen, in the store's own words.
   *
   * Shown here because here is where it was asked for. Without it the button
   * stays exactly as it was, nothing appears, and the person is left clicking —
   * which is the failure this shell is least allowed to have, and which it had
   * again until somebody installed an extension and watched nothing happen.
   */
  failure: string | null;
  onDismissFailure: () => void;
  /**
   * This extension's clock in this project, or `null` where there is none to
   * switch — a package with no schedule, or one this project has not installed.
   *
   * A card for something nobody has installed still says that it runs on a
   * clock, because that is what a person is deciding about; what it does not
   * carry is a control over a schedule that is not running anywhere.
   */
  clock: ClockControl | null;
}) {
  const { packaged } = entry;
  // Installable when something can answer for it — a package on this machine,
  // or an entry the registry can be asked for — and when this build would run
  // what came back. The refusal is computed from the `syncApi` range and the
  // capabilities, both of which the index carries, so a card says *needs a
  // newer Sync* about a package nobody has downloaded rather than spending
  // somebody's network to tell them no.
  const canInstall =
    (packaged !== null || entry.listed !== null) && entry.unrunnable === null;
  // What it would publish, and the whole of it or nothing. A package on this
  // machine carries its type definitions; the index carries only their names,
  // and a name is not a definition — so a registry entry says which kinds are
  // coming and the definitions arrive with the package.
  const types = packaged?.types ?? [];
  // What it does with no screen, and which of those rows the switch belongs
  // to. Read here rather than twice in the markup: `occasionsOf` composes a
  // sentence, and composing it once to count it and again to draw it is how the
  // two come to disagree.
  const occasions =
    packaged === null ? [] : occasionsOf(packaged.manifest, clock?.isOn ?? true);
  // What it does outside this window, and the hosts it named. Both come off the
  // package rather than off the index here, so the sentence and the list are
  // the same package's own words.
  const reach =
    packaged === null ? null : reachOf(packaged.manifest.capabilities);
  const firstClockKey =
    occasions.find((occasion) => occasion.isClock)?.key ?? null;

  return (
    <section className="flex h-full min-w-0 flex-col bg-workspace">
      <div className="flex h-(--panel-header-height) shrink-0 items-center justify-between gap-3 border-b border-separator px-3">
        <h2 className="min-w-0 truncate text-sm font-semibold text-fg">
          {entry.name}
        </h2>
        {/* The one command a panel header may carry: the one that writes into
            the thing the header names. Installing an extension is a change to
            what this project is, which is exactly what the page is about. */}
        <div className="flex shrink-0 items-center gap-3">
          <Standing entry={entry} update={update} />
          <Button
            variant={entry.declared ? "outline" : "default"}
            size="sm"
            onClick={entry.declared ? onRemove : onInstall}
            disabled={isBusy || (!entry.declared && !canInstall)}
          >
            {isBusy
              ? entry.declared
                ? "Removing…"
                : "Installing…"
              : entry.declared
                ? "Remove"
                : "Install"}
          </Button>
        </div>
      </div>

      {/* A strip under the header rather than a sheet: it reports something
          that did not happen, which is news and not a question, and it is
          dismissed by hand so that it cannot vanish before it was read. */}
      {failure === null ? null : (
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-separator bg-panel px-3 py-2">
          <p className="min-w-0 font-mono text-xs leading-4 text-danger">
            {failure}
          </p>
          <Button
            variant="ghost"
            size="sm"
            className="shrink-0"
            onClick={onDismissFailure}
          >
            Dismiss
          </Button>
        </div>
      )}

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex max-w-[76ch] flex-col gap-5 p-4">
          {/* The name again, two centimetres under the header that already
              carries it — and worth it: the page scrolls, and somebody who has
              read to the bottom has lost the header. The version belongs here
              rather than in the sentence, because it is a fact about the
              package and not part of what the thing is. */}
          <div className="flex items-start gap-3">
            <span
              aria-hidden="true"
              className="flex size-8 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary"
            >
              <KindGlyph
                icon={packaged?.manifest.icon ?? entry.listed?.icon}
                className="size-4"
              />
            </span>
            <div className="min-w-0 flex-1">
              <p className="flex items-baseline justify-between gap-3">
                <span className="truncate text-base font-medium text-fg">
                  {entry.name}
                </span>
                <span className="shrink-0 font-mono text-xs text-fg-tertiary">
                  {entry.version}
                </span>
              </p>
              <p className="text-sm leading-5 text-fg-secondary">
                {packaged?.manifest.summary ||
                  entry.listed?.summary ||
                  "This project depends on it, and nothing answers to the name."}
              </p>
            </div>
          </div>

          {/* Above everything the page says about the extension, because it is
              the one thing on it that is about right now. What it *is* has not
              changed since the last time somebody read this page; what is
              available has. */}
          {update === null ? null : (
            <UpdateNotice
              update={update}
              isBusy={isBusy}
              onChange={() => onChange(update.artefact)}
            />
          )}

          {entry.unrunnable === null ? null : (
            <p className="rounded-(--radius-surface) border border-separator bg-panel/60 px-3 py-2 text-sm leading-5 text-danger">
              {entry.unrunnable}
            </p>
          )}

          {/* Said in the same place and in a quieter tier, because it is not a
              fault and nothing here is to be fixed. Every control on the page
              stays as it was: installing this from a phone is a decision about
              a repository, and the computer that opens it next honours it. */}
          {entry.unavailable === null ? null : (
            <p className="rounded-(--radius-surface) border border-separator bg-panel/60 px-3 py-2 text-sm leading-5 text-fg-secondary">
              {entry.unavailable}
            </p>
          )}

          {packaged === null && entry.listed !== null ? (
            // Listed and not here: the ordinary state of everything nobody has
            // installed. What the index carries is said, and what only the
            // package can say is named as coming with it rather than guessed at
            // — a page that summarised a prompt it had not read would be a
            // second thing to keep true.
            <ListedElsewhere listed={entry.listed} />
          ) : packaged === null ? (
            <p className="text-sm leading-5 text-fg-secondary">
              The project&apos;s own record names <code>{entry.id}</code> at
              version {entry.version}. Until a package of that name is unpacked
              here, the sections and the vocabulary it brings are simply absent —
              nothing has been lost, and nothing about the project has been
              changed to hide it.
            </p>
          ) : (
            <>
              {packaged.manifest.description === "" ? null : (
                <p className="text-sm leading-5 text-fg-secondary">
                  {packaged.manifest.description}
                </p>
              )}

              <Section title="What it adds to this window">
                {packaged.manifest.areas.length === 0 ? (
                  <p className="text-sm text-fg-tertiary">
                    No section. It works through what it publishes and what it
                    tells an agent — which is why it can have nothing to look at
                    and still be worth installing.
                  </p>
                ) : (
                  <ul className="flex flex-col gap-1.5">
                    {packaged.manifest.areas.map((area) => (
                      <li
                        key={area.id}
                        className="flex gap-2 text-sm text-fg-secondary"
                      >
                        <span aria-hidden="true" className="text-fg-tertiary">
                          —
                        </span>
                        <span className="min-w-0">
                          <span className="text-fg">{area.label}</span>
                          {area.description === ""
                            ? null
                            : ` — ${area.description}`}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </Section>

              {/* Only for a package that reaches outside, by the same rule the
                  section below follows: a section drawn empty names a state
                  instead of showing one. A package that dials nowhere is the
                  ordinary case and says so by not being here. */}
              {reach === null ? null : (
                <Section title="What it reaches outside this window">
                  <p className="text-sm leading-5 text-fg-secondary">{reach}</p>
                  {/* Every host it named, and the whole list: this is what a
                      request is checked against, so a page that showed the
                      first few would be describing a narrower permission than
                      the one being agreed to. A manifest asking for the
                      capability and naming nowhere is refused when it is read,
                      so there is no empty case to draw. */}
                  <ul className="flex flex-col gap-1">
                    {packaged.manifest.net.hosts.map((host) => (
                      <li
                        key={host}
                        className="font-mono text-xs text-fg-secondary"
                      >
                        {host}
                      </li>
                    ))}
                  </ul>
                  {/* Whose secret goes where, said before anybody installs. A
                      package that sends one of this person's tokens somewhere
                      is the thing they are agreeing to, and the row names the
                      entry rather than showing a value — the window never has
                      one, and the package that declared this does not hold one
                      either. */}
                  {packaged.manifest.net.secrets.map((sending) => (
                    <p
                      key={`${sending.host}/${sending.header}`}
                      className="text-sm leading-5 text-fg-secondary"
                    >
                      It sends the secret{" "}
                      <span className="font-mono text-xs">
                        {sending.secret}
                      </span>{" "}
                      to{" "}
                      <span className="font-mono text-xs">{sending.host}</span>,
                      and never reads it itself.
                    </p>
                  ))}
                </Section>
              )}

              {/* Only for a package that has handlers. A section drawn empty
                  would name a state instead of showing one, which is the rule
                  the navigator's groups already follow. */}
              {occasions.length === 0 ? null : (
                <Section title="What it does with no screen">
                  <ul className="flex flex-col gap-1.5">
                    {occasions.map((occasion) => (
                      <li
                        key={occasion.key}
                        className={cn(
                          "flex items-start gap-2 text-sm",
                          occasion.isClock && clock !== null && !clock.isOn
                            ? "text-fg-tertiary"
                            : "text-fg-secondary",
                        )}
                      >
                        <span aria-hidden="true" className="text-fg-tertiary">
                          —
                        </span>
                        <span className="min-w-0 flex-1">{occasion.said}</span>
                        {/* The control sits beside the claim it governs, and on
                            the first of the clock's rows when a package has
                            several: it is one switch for one extension in one
                            project, and a second copy of it further down the
                            list would read as a second question. */}
                        {occasion.key === firstClockKey && clock !== null ? (
                          <ClockSwitch
                            isOn={clock.isOn}
                            isBusy={clock.isBusy}
                            onChange={clock.onChange}
                          />
                        ) : null}
                      </li>
                    ))}
                  </ul>
                </Section>
              )}

              {/* Everything below is what the project would be agreeing to, in
                  the detail somebody may want and most people will not. Folded,
                  because a page that opens on schemas answers a question almost
                  nobody asked — and reachable, because the person who does ask
                  it is deciding what their repository will contain. The counts
                  stay visible while folded: how many types is scale, not
                  detail. */}
              <div className="divide-y divide-separator border-t border-separator">
                <Disclosure
                  title="Types it publishes"
                  note={types.length === 0 ? "none" : String(types.length)}
                >
                  {types.length === 0 ? (
                    <p className="text-sm text-fg-tertiary">
                      It stores nothing of its own in the project&apos;s memory.
                    </p>
                  ) : (
                    // One group with hairlines between its rows, the way macOS
                    // lists settings — not a card per row. A row is not a card.
                    <ul className="divide-y divide-separator overflow-hidden rounded-(--radius-surface) border border-separator">
                      {types.map((type) => (
                        <li key={type.kind} className="flex gap-2.5 px-3 py-2">
                          <KindMark icon={type.icon} className="mt-0.5" />
                          <div className="min-w-0 flex-1">
                            <p className="flex items-baseline gap-2">
                              <span className="text-base text-fg">
                                {type.title}
                              </span>
                              <span className="truncate font-mono text-xs text-fg-tertiary">
                                {type.kind}
                              </span>
                            </p>
                            <p className="text-xs leading-4 text-fg-tertiary">
                              {type.description}
                            </p>
                          </div>
                        </li>
                      ))}
                    </ul>
                  )}
                </Disclosure>

                <Disclosure
                  title="What it tells an agent"
                  note={packaged.prompt === null ? "nothing" : undefined}
                >
                  {packaged.prompt === null ? (
                    <p className="text-sm text-fg-tertiary">
                      Nothing. An agent connected to this project reads its
                      types like any other, and is told nothing further about
                      how to use them.
                    </p>
                  ) : (
                    <div className="flex flex-col gap-1.5">
                      <p className="text-xs text-fg-tertiary">
                        Written into the project on install, because the agent
                        reads it through a server that has never seen this
                        catalogue. This is the text itself:
                      </p>
                      <pre className="max-h-80 overflow-auto rounded-(--radius-surface) border border-separator bg-panel/60 px-3 py-2 font-mono text-xs leading-4 whitespace-pre-wrap text-fg-secondary">
                        {packaged.prompt}
                      </pre>
                    </div>
                  )}
                </Disclosure>
              </div>
            </>
          )}

          {/* The same command as the one in the header, and deliberately so.
              The page is long enough that a person reaches the end of it having
              read what they needed to decide, and sending them back to the top
              to act on the decision is asking them to remember where the button
              was. It is the same state and the same handler, so the two cannot
              disagree. */}
          <div>
            <Button
              variant={entry.declared ? "outline" : "default"}
              onClick={entry.declared ? onRemove : onInstall}
              disabled={isBusy || (!entry.declared && !canInstall)}
            >
              {isBusy
                ? entry.declared
                  ? "Removing…"
                  : "Installing…"
                : entry.declared
                  ? `Remove ${entry.name}`
                  : `Install ${entry.name}`}
            </Button>
          </div>
        </div>
      </ScrollArea>
    </section>
  );
}

/**
 * Where this entry stands, in as few words as keep the states apart.
 *
 * Four of them, and each is reachable in ordinary use: the project uses it; the
 * machine holds it and the project has not asked for it; the project asked for
 * something this machine does not have; and the machine holds something this
 * build will not run.
 */
/**
 * An extension the registry lists and this machine has not fetched.
 *
 * Everything here comes out of the index, and the index carries what a *card*
 * needs. What it does not carry is what an author wrote at length — the
 * description, the type definitions, the prompt — so those are named as
 * arriving with the package rather than approximated. Installing is what fetches
 * them, and the page fills in the moment it has.
 */
function ListedElsewhere({ listed }: { listed: ListedExtension }) {
  // Composed once and drawn once. A sentence built twice in one component is
  // two sentences that agree until somebody edits one of them.
  const reach = reachOf(listed.capabilities);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm leading-5 text-fg-secondary">
        In the registry, and not on this machine. Installing fetches it, checks
        the bytes against what the registry named, and publishes the types it
        brings into this project&apos;s memory.
      </p>

      <Section title="Sections it adds">
        {listed.areas.length === 0 ? (
          <p className="text-sm leading-5 text-fg-tertiary">
            It draws nothing. An extension is not necessarily a screen — one that
            publishes a vocabulary and a prompt reaches a project without a line
            of it being run.
          </p>
        ) : (
          <ul className="flex flex-wrap gap-1.5">
            {listed.areas.map((area) => (
              <li
                key={area.id}
                className="rounded-(--radius-control) border border-separator px-2 py-0.5 text-xs text-fg-secondary"
              >
                {area.label}
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Types it would publish">
        {listed.publishes.length === 0 ? (
          <p className="text-sm leading-5 text-fg-tertiary">
            It brings no vocabulary of its own.
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {listed.publishes.map((kind) => (
              <li key={kind} className="font-mono text-xs text-fg-secondary">
                {kind}
              </li>
            ))}
          </ul>
        )}
      </Section>

      {reach === null ? null : (
        <Section title="What it reaches outside this window">
          <p className="text-sm leading-5 text-fg-secondary">{reach}</p>
          <p className="text-sm leading-5 text-fg-tertiary">
            Which hosts, exactly, is a sentence in the package rather than in the
            registry&apos;s index — it is shown here whole once the package is
            unpacked, and it is what every request is checked against.
          </p>
        </Section>
      )}

      <Section title="What it tells an agent">
        <p className="text-sm leading-5 text-fg-tertiary">
          {listed.prompt
            ? "It carries instructions for a connected agent. They arrive with the package and are shown here whole once it is installed — the registry's index lists what exists, not what each package says."
            : "Nothing. A connected agent is told about this project's memory and about nothing this extension adds."}
        </p>
      </Section>
    </div>
  );
}

/**
 * A version the registry has and this project is not on.
 *
 * **Nothing here happens on its own, and that is the whole shape of it.**
 * Applying an update publishes type definitions into the project's memory,
 * which is a write to the repository — doing that while somebody is not looking
 * is not an update, it is a commit they did not make. So this says what is
 * there, shows what the author said changed, and waits.
 *
 * The changelog comes from the extension's own ledger, fetched when this page
 * is opened rather than carried in the index every window reads: one file per
 * extension, asked for about the one extension somebody is looking at. Nothing
 * is drawn in its place while it arrives and nothing is drawn if it never does
 * — the version and the button are the part that matters, and a page that
 * blocked on a changelog would make a network failure look like a broken
 * update.
 */
function UpdateNotice({
  update,
  isBusy,
  onChange,
}: {
  update: AvailableUpdate;
  isBusy: boolean;
  onChange: () => void;
}) {
  const { ledger } = useLedger(update.id);
  const changelog = changelogOf(ledger, update.to);

  return (
    <section className="flex flex-col gap-2 rounded-(--radius-surface) border border-separator bg-panel/60 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="min-w-0 text-sm font-semibold text-fg">
          Version {update.to} is available
        </h3>
        <span className="shrink-0 font-mono text-xs text-fg-tertiary">
          {update.from} → {update.to}
        </span>
      </div>

      {/* Either the button or the reason there is none, never both and never
          neither. A build below the range the new version states would download
          an artefact and then refuse it, so what is offered instead is the
          sentence — said here, once, rather than as a notification about an
          application somebody may not want to update. */}
      {update.refusal === null ? (
        <div className="flex items-center gap-3">
          <Button size="sm" disabled={isBusy} onClick={onChange}>
            {isBusy ? "Updating…" : "Update"}
          </Button>
          <p className="text-xs leading-4 text-fg-tertiary">
            It publishes this version&apos;s type definitions and writes the
            version into the project, which is a commit.
          </p>
        </div>
      ) : (
        <p className="text-xs leading-4 text-fg-tertiary">{update.refusal}</p>
      )}

      {changelog === null ? null : (
        <div className="flex flex-col gap-1.5">
          <p className="text-xs text-fg-tertiary">What changed:</p>
          {/* The author's own text, whole. A summary of it would be a second
              thing to keep true, and this is the one place a person can read
              what they are about to agree to. */}
          <pre className="max-h-64 overflow-auto rounded-(--radius-surface) border border-separator bg-workspace px-3 py-2 font-mono text-xs leading-4 whitespace-pre-wrap text-fg-secondary">
            {changelog}
          </pre>
        </div>
      )}
    </section>
  );
}

function Standing({
  entry,
  update = null,
}: {
  entry: CatalogueEntry;
  /**
   * A newer version the registry lists, or `null`.
   *
   * Said here rather than in a mark of its own, because it *is* where this
   * entry stands: an extension with something newer waiting is in a different
   * state from one that is current, and this line is what the two columns and
   * the card all read to find that out. The version is named — a person
   * deciding whether to move wants to know what to.
   */
  update?: AvailableUpdate | null;
}) {
  // Before the states below, because it is the one that is news. An extension
  // is installed for months and has something newer for a day, so the day is
  // what the line says while it lasts — and it goes back to saying "Installed"
  // the moment somebody has moved.
  if (update !== null) {
    return update.refusal === null ? (
      <span className="text-xs text-fg-tertiary">
        Version {update.to} is available
      </span>
    ) : (
      // Named once, on the card, rather than as a notification about an
      // application somebody may not want to update. The button is not drawn
      // at all: it would download an artefact this build would then refuse.
      <span className="text-xs text-fg-tertiary">
        Version {update.to} needs a newer Sync
      </span>
    );
  }
  if (entry.packaged === null) {
    // Two different absences, and only one of them is a problem. A project that
    // asked for something nothing answers to is missing a dependency; an entry
    // the registry lists and this machine has not fetched is the ordinary state
    // of everything nobody has installed yet.
    if (entry.listed === null) {
      return (
        <span className="text-xs text-fg-tertiary">Declared, not available</span>
      );
    }
    return entry.unrunnable === null ? (
      <span className="text-xs text-fg-tertiary">In the registry</span>
    ) : (
      <span className="text-xs text-danger">Will not run here</span>
    );
  }
  if (entry.unrunnable !== null) {
    return <span className="text-xs text-danger">Will not run here</span>;
  }
  return (
    <span className="flex items-center gap-1 text-xs text-fg-tertiary">
      {entry.declared ? (
        <>
          <Check aria-hidden="true" className="size-3 shrink-0" />
          Installed
        </>
      ) : (
        "Not installed"
      )}
    </span>
  );
}

export function ExtensionInspector({
  entry,
  onRemove,
}: {
  entry: CatalogueEntry;
  /** Asks about taking it out of the project — the same ask the page makes. */
  onRemove: () => void;
}) {
  const { packaged } = entry;

  return (
    <PanelSurface className="bg-panel">
      <PanelHeader title="Package" />
      <PanelBody className="space-y-4">
        <dl className="space-y-2">
          <Fact label="Identifier" value={entry.id} mono />
          <Fact label="Version" value={entry.version} mono />
          <Fact
            label="Declared by"
            value={entry.declared ? "This project" : "Nothing here"}
          />
          {packaged === null ? (
            <Fact label="Unpacked" value="No" />
          ) : (
            <>
              <Fact label="Source" value={packaged.pointer.source} />
              <Fact
                label="Signature"
                value={
                  packaged.pointer.signature === "valid"
                    ? "Verified"
                    : packaged.pointer.signature === "invalid"
                      ? "Does not match"
                      : "None"
                }
              />
              <Fact
                label="Integrity"
                value={packaged.pointer.integrity ?? "—"}
                mono={packaged.pointer.integrity !== null}
              />
              <Fact
                label="Needs Sync"
                value={packaged.manifest.engines.syncApi}
                mono
              />
              <Fact
                label="Needs"
                value={
                  packaged.manifest.capabilities.length === 0
                    ? "Nothing"
                    : packaged.manifest.capabilities.join(", ")
                }
              />
              {packaged.manifest.author === null ? null : (
                <Fact label="Author" value={packaged.manifest.author.name} />
              )}
              {packaged.manifest.license === null ? null : (
                <Fact label="Licence" value={packaged.manifest.license} />
              )}
            </>
          )}
        </dl>

        <p className="text-xs leading-4 text-fg-tertiary">
          A project names the extensions it depends on by id and version, and
          records the digest each resolved to — so the same repository opened
          elsewhere resolves the same bytes rather than the same number.
        </p>

        {packaged === null ? null : (
          <PackageRemovals
            id={entry.id}
            declared={entry.declared}
            onRemove={onRemove}
          />
        )}
      </PanelBody>
    </PanelSurface>
  );
}

/**
 * The two things removing an extension can mean, said as two commands.
 *
 * They are not degrees of the same action and are never collapsed into one.
 * Taking it out of the project is a write to the repository that travels to
 * everybody who has it, and leaves the package on the disk. Taking it off the
 * machine leaves every project's declaration exactly as it was — the id simply
 * stops resolving here, and re-installing costs nothing because the artefact
 * was never touched.
 *
 * Here rather than on the card because this column is the package, and because
 * a destructive-sounding command belongs where somebody went to look at what
 * they have rather than where they went to see what they could get.
 *
 * Leaving the project goes through the same confirmation the page's own command
 * does — it is the sheet that says how many records will be left with nothing
 * to show them, and a second button wearing the same words and skipping it
 * would make the count a property of which one was pressed.
 */
function PackageRemovals({
  id,
  declared,
  onRemove,
}: {
  id: string;
  /** Whether the open project's record names it. */
  declared: boolean;
  onRemove: () => void;
}) {
  const composition = useCompositionContext();
  const forgetting = useForgetPackage();

  return (
    <div className="space-y-1.5 border-t border-separator pt-3">
      {forgetting.failure === null ? null : (
        <p className="font-mono text-xs leading-4 text-danger">
          {forgetting.failure}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-1">
        {declared ? (
          <Button
            size="xs"
            variant="ghost"
            disabled={composition.isBusy}
            onClick={onRemove}
          >
            Remove from this project
          </Button>
        ) : null}
        <Button
          size="xs"
          variant="ghost"
          disabled={forgetting.busy}
          onClick={() => void forgetting.forget(id)}
        >
          Remove from this machine
        </Button>
      </div>
    </div>
  );
}

/**
 * A section that is folded until somebody wants it.
 *
 * The triangle, the label and a hairline — the disclosure macOS has had since
 * before the web had one, and the reason this is not an accordion: an accordion
 * is a card that opens, and a column here is not made of cards.
 *
 * Folded by default and open per session. Which sections somebody unfolded
 * while comparing two extensions is not worth a preference, and restoring it
 * next week would be the window remembering something nobody asked it to.
 */
function Disclosure({
  title,
  note,
  children,
}: {
  title: string;
  /** Shown beside the title while folded — scale, not detail. */
  note?: string;
  children: React.ReactNode;
}) {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <section className="py-3">
      <button
        type="button"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
        className="group flex w-full items-center gap-1.5 text-left"
      >
        <ChevronRight
          aria-hidden="true"
          className={cn(
            "size-3.5 shrink-0 text-fg-tertiary transition-transform duration-(--motion-duration-fast) ease-shell motion-reduce:transition-none",
            isOpen && "rotate-90",
          )}
        />
        <span className="text-xs font-semibold text-fg-tertiary group-hover:text-fg-secondary">
          {title}
        </span>
        {note === undefined ? null : (
          <span className="ml-auto shrink-0 text-xs text-fg-tertiary">
            {note}
          </span>
        )}
      </button>

      {isOpen ? <div className="pt-3 pl-5">{children}</div> : null}
    </section>
  );
}

/**
 * What a package says it does outside this window, or `null` for nothing.
 *
 * Read off the capabilities rather than off the host list, because the two
 * answer different questions and only this one is on the registry's index: a
 * card for something nobody has downloaded still has to say that installing it
 * lets a package file in somebody's tracker. Where it reaches is drawn beside
 * this wherever it is known, and it is only ever known from the package itself.
 */
function reachOf(capabilities: readonly string[]): string | null {
  if (!capabilities.includes("net")) return null;
  // Two sentences rather than one with a clause, because they are what a person
  // is deciding between: something that watches, and something that acts.
  return capabilities.includes("net.write")
    ? "It reaches outside this window, and may change things where it reaches rather than only read them."
    : "It reads from outside this window and changes nothing there.";
}

/** One reason a package runs with no screen, as the page says it. */
interface Occasion {
  readonly key: string;
  readonly said: string;
  /**
   * Whether this row is the clock's.
   *
   * The switch governs the clock and not the other occasions — an install
   * handler runs because somebody clicked, and there is nothing to turn off
   * about that — so the row that carries the control is the row that says so.
   */
  readonly isClock: boolean;
}

/**
 * When a package runs code with nobody watching, said in words.
 *
 * The handler's own name is deliberately absent. It is the package's internal
 * name for one of its own functions, read only beside the package that declared
 * it, and it answers none of the question somebody deciding whether to install
 * is actually asking — the same reason a type's identifier is not in the
 * navigator's tooltip.
 *
 * **A clock's row is written by two authors, and each says only what it knows.**
 * The package supplies what the handler does, because nothing else here can
 * know it; the host supplies how often, from `every`, because a sentence about
 * frequency that the package wrote could disagree with the frequency the clock
 * actually uses. `every` is shown as the author spelled it — `6h` — rather than
 * spelled out in words, which is a thing this can grow later without anything
 * downstream changing.
 *
 * One row per occasion, and the list grows as occasions do: a request from
 * another extension is the next one. A package with none of them gets no
 * section.
 */
function occasionsOf(manifest: Manifest, isClockOn: boolean): Occasion[] {
  const occasions: Occasion[] = [];
  if (manifest.lifecycle?.installed) {
    occasions.push({
      key: "installed",
      said: "Runs once when it is added to a project.",
      isClock: false,
    });
  }
  for (const scheduled of manifest.schedule) {
    occasions.push({
      key: `schedule/${scheduled.handler}`,
      // Off is not a quieter way of saying the same thing: the section is
      // titled with what the package *does*, and a package whose clock somebody
      // stopped is not doing this. The row says so and goes quiet.
      said: isClockOn
        ? `${scheduled.description} — every ${scheduled.every}, with or without a window open.`
        : `${scheduled.description} — every ${scheduled.every}. Off for this project.`,
      isClock: true,
    });
  }
  return occasions;
}

/**
 * The clock, stopped or started, for this extension in this project.
 *
 * **A switch, not a gate.** Installing an extension that declares a schedule
 * was the consent — the page said, before the install, that it runs while
 * nobody is there — so this is how somebody takes that back without removing
 * the package, and asking again would be the application forgetting rather than
 * being careful.
 *
 * Two segments rather than a rocker, for the reason the settings window already
 * gives: this window has no switch of its own, and a lone one built here would
 * be the only control in the application drawn that way.
 */
function ClockSwitch({
  isOn,
  isBusy,
  onChange,
}: {
  isOn: boolean;
  isBusy: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Run on a clock in this project"
      className="flex shrink-0 gap-1"
    >
      {[
        { label: "Off", wanted: false },
        { label: "On", wanted: true },
      ].map((option) => (
        <button
          key={option.label}
          type="button"
          role="radio"
          aria-checked={isOn === option.wanted}
          disabled={isBusy}
          onClick={() => onChange(option.wanted)}
          className={cn(
            "flex h-(--control-height-sm) items-center gap-1 rounded-(--radius-control) border border-transparent px-2 text-xs transition-colors duration-(--motion-duration-fast) ease-shell disabled:opacity-50",
            isOn === option.wanted
              ? "border-separator-strong bg-selected font-medium text-fg"
              : "text-fg-secondary hover:bg-hover hover:text-fg",
          )}
        >
          {isOn === option.wanted ? (
            <Check aria-hidden="true" className="size-3 shrink-0" />
          ) : null}
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-2">
      <h3 className="text-xs font-semibold text-fg-tertiary">{title}</h3>
      {children}
    </section>
  );
}

function Fact({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="shrink-0 text-xs text-fg-tertiary">{label}</dt>
      <dd
        className={
          mono
            ? "min-w-0 truncate font-mono text-xs text-fg-secondary"
            : "min-w-0 truncate text-sm text-fg-secondary"
        }
      >
        {value}
      </dd>
    </div>
  );
}
