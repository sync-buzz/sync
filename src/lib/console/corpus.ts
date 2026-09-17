"use client";

import { useEffect, useState } from "react";

import { loadRecords, memoryFolders, memoryTypes, search } from "@/lib/memory/client";
import type { Outcome } from "@/lib/console/types";

/**
 * The project's memory, as a console addresses it.
 *
 * **Addressed by kind, and written as an address.** A kind, then whatever
 * folders it is filed under, then the key — which is what an agent's own tools
 * already take. What a person builds by walking this tree is the thing that
 * goes on the wire: nothing is translated on the way out, so nothing can be
 * translated wrongly.
 *
 * **Identifiers all the way, and never titles.** A title here is a whole
 * sentence, with spaces and back quotes in it; a path made of them cannot be
 * picked out of a line that is itself words, and the first attempt at it
 * unrolled a ten-word heading into the input as a ghost. The title is shown
 * *beside* what is offered, which is where it helps and where it costs nothing.
 *
 * **`/` is not this.** The slash belongs to the file system, where it has meant
 * a path for forty years — the two spaces are addressed by two marks.
 */

/** What the console can point at inside memory. */
export interface Corpus {
  /** The kinds this project holds. */
  readonly types: readonly { readonly kind: string; readonly title: string }[];
  /** Every folder, by its path, with how many documents sit directly in it. */
  readonly folders: readonly {
    readonly path: string;
    readonly records: number;
  }[];
  readonly records: readonly {
    readonly kind: string;
    readonly key: string;
    readonly folder: string;
    readonly title: string;
  }[];
}

/**
 * How many records are read for completion.
 *
 * A page rather than the corpus: this is a list somebody types against, and a
 * project with ten thousand records would spend the memory of all of them to
 * answer a prefix. What is missed is still reachable by writing the address
 * out — an identifier somebody knows is an identifier they can type.
 */
const PAGE = 500;

/** The mark that opens an address into memory. */
export const SYNC = "sync://";

const NOTHING: Corpus = { types: [], folders: [], records: [] };

export function useCorpus(project: string): Corpus {
  const [corpus, setCorpus] = useState<Corpus>(NOTHING);

  useEffect(() => {
    let reading = true;
    void Promise.all([
      memoryTypes(project),
      memoryFolders(project),
      loadRecords(project, { limit: PAGE }),
    ])
      .then(([types, folders, view]) => {
        if (!reading) return;
        setCorpus({
          // Read from the project rather than derived from the records: a kind
          // with nothing filed under it is still a kind, and a console that
          // only knew the kinds it had seen records of would hide exactly the
          // one somebody is about to write the first record in.
          types: types.map((type) => ({ kind: type.kind, title: type.title })),
          folders: folders
            .filter((folder) => folder.path.trim().length > 0)
            .map((folder) => ({ path: folder.path, records: folder.records })),
          records: view.records.map((record) => ({
            kind: record.kind,
            key: record.key,
            folder: record.folder?.trim() ?? "",
            title: record.title,
          })),
        });
      })
      .catch(() => {
        // The corpus is an offer, not an answer. A console that refused a line
        // because it could not read the memory would be worse than one that
        // offers no addresses.
      });
    return () => {
      reading = false;
    };
  }, [project]);

  return corpus;
}

/** How many documents a folder holds, in the words a list shows. */
function held(count: number): string {
  return count === 1 ? "1 document" : `${count} documents`;
}

/**
 * What stands directly inside the address being typed, best first.
 *
 * A step at a time, the way a shell completes a directory: `sync://` offers the
 * kinds, a kind offers its folders and the documents at its root, a folder
 * offers what is in it. A flat list of every address in the project is a list
 * nobody can walk.
 *
 * Anything that continues — a kind, a folder — comes back with a trailing
 * slash, and that is the whole of how the walk goes on: taking one leaves the
 * line ready for the next step rather than finishing it.
 */
export function inside(
  word: string,
  corpus: Corpus,
): readonly { readonly token: string; readonly hint: string }[] {
  if (!word.startsWith(SYNC)) return [];
  const said = word.slice(SYNC.length);
  const cut = said.lastIndexOf("/");
  // Everything up to the last slash is where we are; what follows it is what
  // is being narrowed down.
  const here = cut === -1 ? "" : said.slice(0, cut);
  const wanted = (cut === -1 ? said : said.slice(cut + 1)).toLowerCase();

  // Nothing chosen yet: the kinds, which is what an address begins with.
  if (here.length === 0) {
    return corpus.types
      .filter((type) => type.kind.toLowerCase().startsWith(wanted))
      .sort((left, right) => left.kind.localeCompare(right.kind))
      .map((type) => ({ token: `${SYNC}${type.kind}/`, hint: type.title }));
  }

  const slash = here.indexOf("/");
  const kind = slash === -1 ? here : here.slice(0, slash);
  const folder = slash === -1 ? "" : here.slice(slash + 1);
  const under = folder.length === 0 ? "" : `${folder}/`;

  // The folders directly below this one, by their own next segment. Folders
  // are a namespace the whole project shares, so they are offered under every
  // kind and filtered by what is actually filed there.
  const names = new Set(
    corpus.folders
      .filter((one) => one.path.startsWith(under))
      .map((one) => one.path.slice(under.length).split("/")[0] ?? "")
      .filter((name) => name.length > 0 && name.toLowerCase().startsWith(wanted)),
  );

  const folders = [...names]
    .map((name) => {
      const path = `${under}${name}`;
      const count = corpus.records.filter(
        (record) => record.kind === kind && record.folder === path,
      ).length;
      return { path, count };
    })
    // A folder with nothing of this kind in it is not this kind's folder. The
    // tree is per kind, and offering every folder under every kind is what
    // makes a namespace look like a hierarchy it is not.
    .filter((one) => one.count > 0)
    .sort((left, right) => left.path.localeCompare(right.path))
    .map((one) => ({
      token: `${SYNC}${kind}/${one.path}/`,
      hint: held(one.count),
    }));

  const documents = corpus.records
    .filter(
      (record) =>
        record.kind === kind &&
        record.folder === folder &&
        record.key.toLowerCase().startsWith(wanted),
    )
    .sort((left, right) => left.key.localeCompare(right.key))
    .map((record) => ({
      token: `${SYNC}${record.kind}/${
        record.folder.length === 0 ? "" : `${record.folder}/`
      }${record.key}`,
      hint: record.title,
    }));

  // Folders first: a step deeper is what somebody is doing while they are
  // still typing slashes, and a document is where they stop.
  return [...folders, ...documents];
}

/** Whether a line is one address and nothing else, which is what opens it. */
export function onlyAddress(line: string): string | null {
  const typed = line.trim();
  if (!typed.startsWith(SYNC) || /\s/u.test(typed)) return null;
  // A trailing slash is a folder or a kind: a place rather than a document, and
  // there is nothing to open.
  return typed.endsWith("/") ? null : typed;
}

/** How many hits one line of searching prints. */
const HITS = 8;

/** Plain text in a block, which is what these verbs answer with. */
function printed(text: string): Outcome {
  return { state: "done", result: { view: "text", text } };
}

/**
 * The verbs about the project's memory, carried out.
 *
 * They belong to the shell for the reason the verbs about agents do: memory is
 * the core — `sync-memory` is the only door to the engine — and not a package.
 *
 * **Not one of them names a kind.** `types` reads what the project holds and
 * `find` searches whatever is there; a verb that knew `tasks.task` would be the
 * shell learning a subject, which is the one thing it does not do. What they
 * print is addresses, so anything found is a line away from being opened or
 * handed to an agent.
 *
 * Answers `null` for a verb that is not one of these, which is how the caller
 * knows to offer it elsewhere.
 */
export async function corpusVerb(
  verb: string,
  args: readonly string[],
  project: string,
  corpus: Corpus,
): Promise<Outcome | null> {
  switch (verb) {
    case "types": {
      if (corpus.types.length === 0) return printed("this project holds no types");
      return printed(
        corpus.types
          .map((type) => `${SYNC}${type.kind}/  ${type.title}`)
          .join("\n"),
      );
    }

    case "folders": {
      if (corpus.folders.length === 0) return printed("no folders");
      return printed(
        corpus.folders
          .map((folder) => `${folder.path}  ${held(folder.records)}`)
          .join("\n"),
      );
    }

    case "find": {
      const asked = args.join(" ").trim();
      if (asked.length === 0) {
        return {
          state: "failed",
          result: { view: "text", text: "console: say what to look for" },
        };
      }
      const found = await search(project, { query: asked, limit: HITS });
      if (found.hits.length === 0) return printed(`nothing matches ${asked}`);
      const more =
        found.total > found.hits.length
          ? `\n… ${found.total - found.hits.length} more`
          : "";
      return printed(
        found.hits
          .map((hit) => {
            // The address first, because it is the part that does something:
            // put it on a line of its own and the record opens.
            const at = `${SYNC}${hit.kind ?? ""}/${hit.id}`;
            return `${at}  ${hit.title ?? ""}`;
          })
          .join("\n") + more,
      );
    }

    default:
      return null;
  }
}

/**
 * The kind and the key an address names, or nothing when it names neither.
 *
 * Here rather than wherever an address is acted on, because this module owns
 * what an address is made of: a second place that split one would part company
 * with this one the first time the shape gains a segment.
 */
export function addressParts(
  address: string,
): { readonly kind: string; readonly key: string } | null {
  if (!address.startsWith(SYNC)) return null;
  const said = address.slice(SYNC.length).split("/");
  const kind = said[0] ?? "";
  const key = said.at(-1) ?? "";
  // A trailing slash leaves an empty last segment, and a bare kind leaves the
  // kind as its own key: both are places rather than documents.
  if (kind.length === 0 || key.length === 0 || key === kind) return null;
  return { kind, key };
}
