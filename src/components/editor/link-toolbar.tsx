"use client";

/**
 * Making a link, changing one, and taking one off.
 *
 * All three are the link plugin's own machinery rather than ours beside it.
 * That matters more than it sounds: the plugin already knows when the caret is
 * inside a link, when a selection could become one, where the panel belongs on
 * screen, what `⌘K` means and how Escape closes it. A second implementation of
 * any of that would be a second answer to a question the editor has already
 * answered, and the two would disagree the first time something moved.
 *
 * What is ours is the one thing the plugin cannot know: that most links in this
 * application point at another record, and that finding it is a search rather
 * than typing an address. So the field is the project's own search — the same
 * `useSearch` the palette in the title bar uses — and what it submits is still
 * the plugin's `submitFloatingLink`.
 *
 * The field takes one of two questions and answers one at a time. Words are a
 * search; an address — a scheme, `sync://`, or a path — is an address, and is
 * offered as the row it is rather than as a query, because search has no
 * relevance floor and a url torn into words matches a hundred records that
 * share one of them. Every one of those is the wrong answer to the question
 * that was asked.
 *
 * How the url is *spelled* is not offered as a choice, because it is not one: a
 * record whose body is a file is linked by its path, the way GitHub reads one,
 * and a record with no file is named with the scheme. That is decided from the
 * record after it is picked, so nobody has to know which storage the thing they
 * are linking to happens to use.
 *
 * Two panels, and which one appears is the plugin's decision:
 *
 * - **Insert**, over a selection that is not already a link. Reached from the
 *   toolbar over the selection, or with `⌘K`.
 * - **Edit**, whenever the caret is resting inside a link. This is the one that
 *   was missing: a link could be made and never unmade, and where it pointed
 *   was something you could only find out by following it.
 */

import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import {
  flip,
  getRangeBoundingClientRect,
  offset,
  shift,
} from "@platejs/floating";
import {
  LinkPlugin,
  submitFloatingLink,
  useFloatingLinkEdit,
  useFloatingLinkEditState,
  useFloatingLinkInsert,
  useFloatingLinkInsertState,
} from "@platejs/link/react";
import { ArrowUpRight, Link2, Link2Off, Pencil } from "lucide-react";
import { KEYS, type TLinkElement } from "platejs";
import {
  useEditorPlugin,
  useEditorRef,
  useEditorSelector,
  usePluginOption,
} from "platejs/react";

import { KindMark } from "@/components/shell/entity-marks";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { useSearch } from "@/lib/memory/use-search";
import {
  isWebUrl,
  recordTarget,
  useLinkOrigin,
  useRecordLinks,
} from "@/lib/record-link";
import { cn } from "@/lib/utils";

/** How many records are offered at once. A list to pick from, not a report. */
const SHOWN = 6;

/** A row of the panel, whether it offers a record or the address itself. */
const ROW =
  "flex w-full items-center gap-2.5 rounded-(--radius-control) px-2 py-1.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover data-[active=true]:bg-selected";

const PLACEMENT = {
  middleware: [
    offset(8),
    flip({ fallbackPlacements: ["top-start"], padding: 8 }),
    shift({ padding: 8 }),
  ],
  placement: "bottom-start" as const,
};

/**
 * Where the panel points, and why it is not the plugin's own answer.
 *
 * The insert panel is anchored by default to the *browser's* selection, which
 * is right until the moment a field appears and takes the caret — and this
 * panel's whole content is a field, focused as it opens. The browser's
 * selection is then the one inside the input, so the panel would be measured
 * against itself.
 *
 * The editor's own selection does not move when focus does. It is the same
 * range the link will be made over, which is the thing the panel is about.
 */
function usePlacement() {
  const editor = useEditorRef();

  return useMemo(
    () => ({
      ...PLACEMENT,
      getBoundingClientRect: () =>
        editor.selection
          ? getRangeBoundingClientRect(editor, editor.selection)
          : new DOMRect(),
    }),
    [editor],
  );
}

export function LinkToolbar() {
  return (
    <>
      <InsertPanel />
      <EditPanel />
    </>
  );
}

/** A link being made, over words that are not one yet. */
function InsertPanel() {
  const state = useFloatingLinkInsertState({ floatingOptions: usePlacement() });
  const { hidden, props, ref } = useFloatingLinkInsert(state);

  if (hidden) return null;

  return (
    <div ref={ref} {...props} className={PANEL}>
      <LinkField placeholder="Search records, or paste an address" />
    </div>
  );
}

/**
 * A link the caret is resting in.
 *
 * It says where the link goes before it offers to do anything about it, because
 * that is the question somebody who put the caret there is asking. `Open`
 * exists so following a link is not only a modifier-click somebody has to know
 * about; the other two are the plugin's own, wired to its transforms.
 */
function EditPanel() {
  const state = useFloatingLinkEditState({ floatingOptions: PLACEMENT });
  const { editButtonProps, props, ref, unlinkButtonProps } =
    useFloatingLinkEdit(state);
  const mode = usePluginOption(LinkPlugin, "mode");
  /**
   * Where this link goes, read from the link the caret is in.
   *
   * Not from the plugin's `url` option, which is what the first version did and
   * why this panel came up empty: that option is the *field's* value, filled in
   * only once somebody asks to edit. Merely resting the caret in a link never
   * touches it, so the panel described the link by showing nothing about it.
   */
  const url = useEditorSelector((editor) => {
    const entry = editor.api.above<TLinkElement>({
      match: { type: editor.getType(KEYS.link) },
    });
    return entry?.[0].url ?? "";
  }, []);

  const links = useRecordLinks();
  const base = useLinkOrigin()?.locator ?? null;
  const target = links?.targetOf(url, base) ?? null;

  if (state.readOnly || !state.isOpen || mode !== "edit") return null;

  if (state.isEditing) {
    return (
      <div ref={ref} {...props} className={PANEL}>
        <LinkField placeholder="Search records, or paste an address" />
      </div>
    );
  }

  return (
    <div ref={ref} {...props} className={cn(PANEL, "flex items-center gap-0.5")}>
      {url === "" ? null : (
        <>
          <span className="min-w-0 max-w-56 truncate px-2 text-xs text-fg-tertiary">
            {url}
          </span>
          <Separator orientation="vertical" className="mx-0.5 h-5" />
        </>
      )}
      {target === null || links === null ? null : (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Open"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => links.follow(target)}
          className="text-fg-secondary"
        >
          <ArrowUpRight />
        </Button>
      )}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Edit link"
        onMouseDown={(event) => event.preventDefault()}
        {...editButtonProps}
        className="text-fg-secondary"
      >
        <Pencil />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Remove link"
        {...unlinkButtonProps}
        className="text-fg-secondary"
      >
        <Link2Off />
      </Button>
    </div>
  );
}

/**
 * Whether this is an address rather than something somebody is searching for.
 *
 * Deliberately stricter than `isProjectPath`, which answers yes to any word at
 * all — that is the right answer when *reading* a body somebody wrote, and the
 * wrong one here, where a bare word is far more likely to be a search than a
 * file called that.
 */
function looksLikeAddress(text: string): boolean {
  const value = text.trim();
  return isWebUrl(value) || recordTarget(value) !== null || value.includes("/");
}

const PANEL =
  "z-50 rounded-(--radius-control) border border-separator-strong bg-raised p-1 shadow-(--shadow-content)";

/**
 * One field for both panels, and for both kinds of link.
 *
 * The plugin's `url` option *is* the query. Keeping a second copy of what has
 * been typed would be two answers to what is in one field, and the plugin's own
 * submit reads its copy — so the search reads the same one rather than a
 * shadow of it.
 *
 * What is typed is offered as an address and searched for at the same time,
 * without asking which was meant. Somebody pasting a URL sees no records for
 * it, so an empty list is the same signal as the address being an address.
 *
 * **It opens already searching for the words that were selected.** Those words
 * are almost always the name of the thing being linked to — that is why they
 * were the ones somebody selected — so asking them to type it again would be
 * asking for something already on the screen. The plugin has them: it puts the
 * selection in its `text` option when the panel is triggered.
 */
function LinkField({ placeholder }: { placeholder: string }) {
  const { editor, getOptions, setOption } = useEditorPlugin(LinkPlugin);
  const url = usePluginOption(LinkPlugin, "url") ?? "";
  const links = useRecordLinks();
  const origin = useLinkOrigin();
  const base = origin?.locator ?? null;
  /**
   * Which row the keyboard is on, and which question it was on.
   *
   * The question travels with the index rather than the index being reset when
   * the answer changes: a list that has just been replaced has no row four, and
   * remembering which question the cursor belonged to answers that in a render
   * instead of in an effect that fires after the wrong row is drawn.
   */
  const [cursorAt, setCursorAt] = useState({ stamp: "", index: 0 });

  /**
   * Search for what was selected, once, as the panel opens.
   *
   * On mount rather than on a change, because this component *is* mounted when
   * the panel opens and unmounted when it closes. Only into an empty field: the
   * edit panel fills the url with the link's own address before this renders,
   * and overwriting that would answer "change this link" by forgetting it.
   */
  useEffect(() => {
    const options = getOptions();
    if (options.url === "" && options.text !== "") {
      setOption("url", options.text);
    }
    // Once, as the panel opens. The field is the person's from then on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * An address in the field is an address, and the corpus is not asked about it.
   *
   * Not a refinement of the ranking but a different question: search has no
   * relevance floor, so a pasted url is torn into ordinary words — `https`,
   * `docs`, the host — and comes back with a hundred records that share one of
   * them. Every one of those is a wrong answer to "link to this address", and
   * offering them cost the right one: the list was never empty, so the row the
   * keyboard was on was always a record, and Return put a stranger's record
   * where somebody meant to put a url. There was no way to insert one at all.
   */
  const address = looksLikeAddress(url);
  const answer = useSearch(
    links?.project ?? "",
    url,
    [],
    links !== null && !address,
  );
  // The record being written is not one of its own answers. Filtered before the
  // list is cut to length, or excluding it would cost the row at the bottom.
  // Empty for an address, and emptied here rather than at every place that
  // reads it: a paused search still holds the answer to the previous question.
  const hits = address
    ? []
    : answer.hits.filter((hit) => hit.id !== origin?.key).slice(0, SHOWN);
  const stamp = url.trim();
  const cursor = cursorAt.stamp === stamp ? cursorAt.index : 0;
  const at = Math.min(cursor, Math.max(hits.length - 1, 0));
  const moveTo = (index: number) => setCursorAt({ stamp, index });

  /**
   * Put the record in, through the plugin's own submit.
   *
   * The url is written into the option the submit reads, which is the same
   * thing typing an address does — so a record and an address take one path
   * into the document and there is one place a link is made.
   */
  const pick = (key: string, kind: string, title: string) => {
    if (links === null) return;
    void links
      .hrefTo(key, kind, base)
      .then((href) => {
        setOption("url", href);
        // The words that were selected are the link's text. With nothing
        // selected there are none, and the record's own title is the honest
        // thing to write rather than the address of it.
        if (getOptions().text === "") setOption("text", title);
        submitFloatingLink(editor);
      })
      .catch(() => undefined);
  };

  /**
   * Put the address in, which is the plugin's own submit and nothing else.
   *
   * Nothing is read and nothing is resolved: an address is already the url, and
   * where it goes is the system's question rather than this window's. The text
   * is left alone — with a selection those are the words the link is made over,
   * and with none the plugin writes the url itself, which is what a url with no
   * name should read as.
   */
  const putAddress = () => submitFloatingLink(editor);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" && hits.length > 0) {
      event.preventDefault();
      moveTo(Math.min(at + 1, hits.length - 1));
      return;
    }
    if (event.key === "ArrowUp" && hits.length > 0) {
      event.preventDefault();
      moveTo(Math.max(at - 1, 0));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      // The row the panel is showing, and the two cases cannot both be on
      // screen: an address is offered alone, and records are offered when there
      // is no address. So Return is the row somebody is looking at either way.
      if (address) {
        putAddress();
        return;
      }
      const hit = hits[at];
      if (hit) {
        pick(hit.id, hit.kind ?? "", hit.title ?? hit.id);
        return;
      }
      // Nothing else is a link. What is in the field is the words somebody
      // selected, and making a link out of those would point at a document
      // named after them — a dead link created by pressing Return on a search
      // that found nothing.
    }
  };

  return (
    <div className="w-80">
      <input
        autoFocus
        value={url}
        spellCheck={false}
        aria-label="Link to a record, or an address"
        placeholder={placeholder}
        onChange={(event) => setOption("url", event.target.value)}
        // The selected words are a starting point, not something to type past:
        // whoever wants a different search should get it by typing, not by
        // clearing the field first.
        onFocus={(event) => event.target.select()}
        onKeyDown={onKeyDown}
        className="h-(--control-height-lg) w-full bg-transparent px-2 text-sm text-fg outline-none placeholder:text-fg-tertiary"
      />

      {address ? (
        <div className="mt-1 border-t border-separator pt-1">
          <button
            type="button"
            data-active="true"
            // Before the click, or the panel closes on its way out.
            onMouseDown={(event) => event.preventDefault()}
            onClick={putAddress}
            className={ROW}
          >
            <span
              aria-hidden="true"
              className="flex size-6 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary"
            >
              <Link2 className="size-3.5" />
            </span>
            <span className="min-w-0 flex-1 truncate text-sm text-fg">
              {stamp}
            </span>
          </button>
        </div>
      ) : null}

      {hits.length === 0 && stamp !== "" && !answer.isSearching && !address ? (
        <p className="border-t border-separator px-2 pt-1.5 pb-1 text-xs text-fg-tertiary">
          No records match. Paste an address to link outside the project.
        </p>
      ) : null}

      {hits.length === 0 ? null : (
        <div className="mt-1 flex flex-col gap-0.5 border-t border-separator pt-1">
          {hits.map((hit, index) => (
            <button
              key={hit.id}
              type="button"
              data-active={index === at}
              onMouseMove={() => moveTo(index)}
              // Before the click, or the panel closes on its way out.
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => pick(hit.id, hit.kind ?? "", hit.title ?? hit.id)}
              className={ROW}
            >
              <KindMark icon={links?.iconOf(hit.kind ?? "") ?? null} />
              <span className="min-w-0 flex-1 truncate text-sm text-fg">
                {hit.title ?? hit.id}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
