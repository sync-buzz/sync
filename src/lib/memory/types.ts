/**
 * The memory contract, as the frontend sees it.
 *
 * These mirror the Rust command layer in `src-tauri/src/memory.rs`, which in
 * turn mirrors the engine. Nothing here talks to the engine directly: the
 * frontend has no filesystem access and no process control, and it should stay
 * that way.
 */

/**
 * The key of the one record that names the project.
 *
 * Fixed rather than generated because there is exactly one of it, which is what
 * lets Sync ask "has this repository been opened before?" with a single read.
 * Mirrors `PROJECT_KEY` in `sync-memory`; the window uses it to say which record
 * is the project's own, never to decide what may be written.
 */
export const PROJECT_KEY = "project";

/** Every entity kind Sync stores. The engine rejects anything else. */
export type EntityKind =
  | "goal"
  | "milestone"
  | "spec"
  | "decision"
  | "constraint"
  | "observation"
  | "question"
  | "artifact"
  | "doc"
  | "comment";

/**
 * How far a record can still be trusted, in the engine's own words.
 *
 * This is not a field Sync writes: memory-hub derives it by reconciling code
 * history against each record's scope paths, which is why it is the state the
 * interface shows. `stale` and `invalid` mean the code moved under the claim —
 * the one thing in the window worth interrupting for.
 *
 * Treated as open: a newer engine may report a state this build has no mark
 * for, and an unknown word is shown as it arrived rather than mapped onto a
 * state it may not mean.
 */
export type Freshness = "fresh" | "unverified" | "stale" | "invalid" | (string & {});

/**
 * A type the project holds, as the column that lists them shows it.
 *
 * All of it comes from the project's own corpus, the mark included: a type
 * created in the window is one no build has heard of. Where a definition names
 * no mark and the kind is one Sync knows how to describe, Sync's own is used.
 */
export interface MemoryType {
  /**
   * The identifier: what every record of this type carries, what an agent
   * writes, and what its definition's key is built from. Generated from the
   * name when a person adds a type, prefixed when an extension brings one, and
   * never re-derived afterwards — it is stored.
   */
  readonly kind: string;
  /**
   * What the type is called where a person reads it. More than one word is
   * normal, and changing it touches no record.
   */
  readonly title: string;
  readonly description: string;
  /**
   * A Lucide icon name, or `null` when neither the definition nor this build
   * names one — in which case the type is drawn with a neutral mark.
   */
  readonly icon: string | null;
  readonly fieldCount: number;
  /**
   * Where this type's records keep their content.
   *
   * Not a storage detail the interface can leave unsaid: it decides whether a
   * document has a file behind it, whether its folder is a name or a directory
   * somebody can rename in Finder, and whether the body being edited is one a
   * colleague may have open in their own editor at the same time.
   */
  readonly storage: TypeStorage;
  /**
   * Whether a document of this type can be written at all, as the engine
   * answers it.
   *
   * Asked before offering to create rather than discovered from a failure: a
   * type keeping its content in its records is always writable, and one
   * pointing at a folder is only as writable as that folder — which may be
   * read-only, or may not be checked out here at all.
   */
  readonly writable: boolean;
  /**
   * The fields this type declares, exactly as the store spells them.
   *
   * Carried verbatim rather than parsed into a shape of this build's: the schema
   * is published at runtime, so what edits a field generates its control from
   * what the declaration says — a `type`, whether it is `required`, its `values`
   * when it enumerates them, its `default` when it states one — and falls back to
   * showing the value as text when it cannot recognise the shape.
   */
  readonly fields: Readonly<Record<string, FieldDeclaration>>;
  /**
   * The relations this type declares, name to `{ target, description }`.
   *
   * Not decoration: the engine validates every link against these and rejects a
   * relation a type does not declare, so this is the list of links a record of
   * this type is allowed to hold. A type that declares none cannot link at all,
   * and the panel says so rather than offering a field the store would refuse.
   */
  readonly relationships: Readonly<Record<string, RelationshipDeclaration>>;
  /**
   * What an agent is told before it writes a record of this type, or `null`
   * where the definition says nothing.
   *
   * Published with the type rather than held by the build that brought it, so
   * it travels with the repository and any client of the engine can read it.
   * Nothing in this window shows it: it is written for whoever writes records
   * without a screen in front of them.
   */
  readonly guidance: string | null;
  /**
   * True for a type Sync publishes and maintains. It is always in the corpus —
   * the project's own record has that kind — so nothing may offer to remove it.
   */
  readonly own: boolean;
}

/**
 * One field, as its type declares it.
 *
 * Every member is optional because a definition is the project's and may say as
 * little as it likes; a declaration this build cannot read is a field shown as
 * text rather than a field it refuses to draw.
 */
export interface FieldDeclaration {
  /**
   * `string`, `text`, `enum`, `number`, `integer`, `boolean`, `array`,
   * `object`.
   *
   * `text` and `string` are both strings to the store and different things to
   * a person: `text` is the engine's word for prose, so it is offered as the
   * several lines it says it is.
   */
  readonly type?: string;
  readonly required?: boolean;
  /** The values an enumeration allows. */
  readonly values?: readonly string[];
  /**
   * What an `array` holds, one declaration for every entry. It is what decides
   * whether a list can be offered as a control at all: a list of strings is a
   * token field and a list over an enumeration is a set of checkboxes, while a
   * list of objects is shown as stored.
   */
  readonly items?: FieldDeclaration;
  /** What a control offers when somebody fills the field in. */
  readonly default?: unknown;
  readonly description?: string;
}

/**
 * Where a type's documents live: a folder of the repository, or nothing at all.
 *
 * A type naming no folder keeps its bodies in its records — nothing else writes
 * them, and a record's body is part of the record. A type naming one points at
 * a directory of the working tree: the files are the team's, Git versions them,
 * a pull request shows them in its diff, and Memory writes nothing into them.
 * What it keeps beside each file is a record holding the key, the locator, the
 * digest, the tags and the links.
 *
 * The path is in the type definition itself, so a type that is attached is
 * always locatable — including one somebody else's build attached.
 */
export interface TypeStorage {
  /** The directory, relative to the repository root. Absent for a type whose bodies are records. */
  readonly folder?: string | null;
}

/**
 * Whether this type's documents are files somebody else edits.
 *
 * One question asked in one place, because the answer changes what the window
 * may offer: a folder that is a directory is renamed in Finder, and a body that
 * is a file may be open in another editor while Sync shows it.
 */
export function isAttachedType(type: MemoryType): boolean {
  return typeof type.storage.folder === "string" && type.storage.folder.length > 0;
}

/**
 * Why a record's document is not here, in the fewest words that keep the two
 * absences apart.
 *
 * `not_on_branch` is routine — the corpus holds every branch's documents and
 * this checkout has only some of them — while `removed` is somebody deleting
 * the file on the branch that owns it, which is the one absence a person is
 * asked about. Collapsing them into "missing" would ask nobody anything.
 *
 * One answer in one place, because a row, an open document and a menu that all
 * describe the same state in different words describe three states.
 */
export function absenceLabel(presence: Presence): string {
  if (presence === "not_on_branch") return "not on this branch";
  if (presence === "removed") return "file deleted here";
  return "file missing";
}

/**
 * Which type's folder a file belongs to, by where the file is.
 *
 * The deepest declared folder wins, because `docs` and `docs/adr` can both be
 * attached and a file under the second belongs to the second. Membership is a
 * question the engine has already answered for any file it reported; this only
 * says which type the answer was about.
 *
 * `null` when no attached type could hold it, so a file this window cannot
 * place is one it says nothing about rather than one it files under a guess.
 */
export function typeOfLocator(
  types: readonly MemoryType[],
  locator: string,
): MemoryType | null {
  let best: MemoryType | null = null;
  for (const type of types) {
    const folder = type.storage.folder;
    if (!isAttachedType(type) || !folder) continue;
    if (locator !== folder && !locator.startsWith(`${folder}/`)) continue;
    if (best === null || folder.length > (best.storage.folder?.length ?? 0)) {
      best = type;
    }
  }
  return best;
}

/**
 * Whether a record's content is here, and if not, why not.
 *
 * Memory does not branch and code does, so the corpus holds the union of every
 * branch's documents and the checked-out branch decides which of them are real
 * right now. `not_on_branch` is routine — another branch has it — while
 * `removed` is somebody deleting the file on the branch that owns it, which is
 * the one absence worth asking a person about.
 *
 * Open, like every other word the engine publishes.
 */
export type Presence = "present" | "not_on_branch" | "removed" | (string & {});

/** One relation a type declares. `target` is a kind name, or `any`. */
export interface RelationshipDeclaration {
  readonly target?: string;
  readonly description?: string;
}

/**
 * What an edit of one record changes. Every member is optional and means
 * "replace this"; anything absent is left exactly as the store holds it, which
 * is what lets the panel write a tag while somebody is still typing a paragraph.
 *
 * A field set to `null` is one the record stops carrying — how an optional field
 * is cleared, which is not the same as leaving it alone.
 */
export interface DocumentPatch {
  readonly title?: string;
  readonly content?: string;
  readonly tags?: readonly string[];
  readonly links?: readonly EntityLink[];
  readonly scope?: readonly string[];
  readonly observed?: readonly string[];
  readonly archived?: boolean;
  readonly fields?: Readonly<Record<string, unknown>>;
}

/** One record that holds on to another. */
export interface Dependent {
  readonly key: string;
  readonly kind: string;
  readonly title: string;
  /** The relation the link declares, when it is a link rather than a mention. */
  readonly relation: string | null;
}

/**
 * What holds on to a record, split by how it holds on.
 *
 * `links` name it structurally — delete it and the link points at nothing.
 * `mentions` talk about it in prose, and deleting one of those because it named
 * a record would delete the reasoning along with the conclusion.
 */
export interface Dependents {
  readonly links: readonly Dependent[];
  readonly mentions: readonly Dependent[];
}

/**
 * What removing a type took with it.
 *
 * The count is reported by the write rather than taken from what the window
 * showed before it: an agent may have written a record of the type while the
 * confirmation was on screen, and the number that happened is the true one.
 */
export interface TypeRemoval {
  /** The corpus as it now stands. */
  readonly types: readonly MemoryType[];
  /** How many records of the type were deleted with its definition. */
  readonly removed: number;
}

/** One record, as a row. The body is not carried: a row is not a document. */
export interface MemoryRecord {
  readonly key: string;
  readonly kind: string;
  readonly title: string;
  /**
   * The fields this row was asked for, in the store's own words. Untyped for
   * the same reason [`MemoryDocument.fields`] is: the schema is published at
   * runtime, and a build that typed them here would be a second copy of it
   * going out of date on its own.
   *
   * Absent unless the selection named some — see `MemorySelection.fields` —
   * and absent rather than empty, because a row nobody asked fields of should
   * not carry a member saying so. Optional for a second reason too: a caller
   * may build a `MemoryRecord` of its own for a row that is not in the corpus,
   * and a required member would make every one of those a compile error over
   * a fact it has no answer to.
   *
   * A name that was asked for and is missing means the record does not carry
   * it, which for an optional field is the ordinary answer rather than a fault.
   */
  readonly fields?: Readonly<Record<string, unknown>>;
  readonly freshness: Freshness;
  /** The paths the claim's scope covers. Empty is a real answer. */
  readonly scope: readonly string[];
  readonly archived: boolean;
  readonly tags: readonly string[];
  /** The file holding this record's content, when it lives outside. */
  readonly locator: string | null;
  readonly presence: Presence;
  /**
   * Where this record is filed — a path of segments, `null` for the root.
   *
   * A name and never a location: in the records themselves the tree stays flat
   * and the folder is metadata somebody sets. For a record whose content is a
   * repository file it is the directory that file is in, and the two may not
   * disagree — which is why moving such a record is an engine operation rather
   * than a field this window writes.
   */
  readonly folder: string | null;
  /**
   * Whether this record *is* the folder it is filed in.
   *
   * A folder is a name until somebody gives it a title and a text of its own,
   * and that is this flag. It matters to whatever draws a tree — which would
   * otherwise show the record twice, once as the folder and once as its own
   * child — and to nothing else.
   */
  readonly isFolder: boolean;
}

/**
 * One record, whole.
 *
 * The body is Markdown exactly as the store holds it: what renders it is the
 * window's decision, and what edits it will be the editor's.
 */
export interface MemoryDocument {
  readonly key: string;
  readonly kind: string;
  readonly title: string;
  readonly content: string;
  readonly freshness: Freshness;
  /** Paths the claim's scope covers — what turns it stale when code moves. */
  readonly scope: readonly string[];
  /** Paths it was written against. */
  readonly observed: readonly string[];
  readonly tags: readonly string[];
  readonly links: readonly EntityLink[];
  readonly archived: boolean;
  /**
   * The fields this record's type declares, in the store's own words. Untyped
   * on purpose: the schema is published at runtime, and a build that typed them
   * here would be a second copy of it going out of date on its own.
   */
  readonly fields: Readonly<Record<string, unknown>>;
  /** The file this record's content lives in, when it lives outside. */
  readonly locator: string | null;
  /**
   * What the document is, from its file name: `text/markdown`, `image/png`.
   * `null` when nobody said.
   */
  readonly mediaType: string | null;
  /**
   * Whether the body is not text — an image, a PDF, anything an attached folder
   * holds now that there is no mask keeping it out.
   *
   * The bytes never travel: a window that cannot edit them has no use for them,
   * and base64 rendered as prose is worse than saying plainly what the document
   * is. What reads this says so, and leaves the file alone.
   */
  readonly contentBinary: boolean;
  readonly presence: Presence;
  /**
   * Whether the body could not be read because the file is not here.
   *
   * Distinct from an empty document, and the distinction is the point: an empty
   * file is something somebody wrote, and a missing one is a document this
   * branch does not have. Showing the second as the first would invite somebody
   * to fix it by typing into it, which would fork a document that exists
   * elsewhere.
   */
  readonly contentMissing: boolean;
  /** Where this record is filed, `null` for the root. See {@link MemoryRecord.folder}. */
  readonly folder: string | null;
  /**
   * Whether this record is the folder it is filed in. See
   * {@link MemoryRecord.isFolder}.
   */
  readonly isFolder: boolean;
}

/**
 * What reconciling the attached folders with the records did.
 *
 * Four outcomes are unambiguous and are applied without asking — an edit in
 * place, a move, a disappearance, a return. The fifth is not: a file matching
 * no record may be new or may be a rename with an edit, and nothing about the
 * file says which. Those arrive as `unmatched` changes carrying the records
 * they could be, and wait for a person.
 */
export interface ScanOutcome {
  readonly revision: string | null;
  /** How many files the scan looked at. */
  readonly scanned: number;
  /**
   * How many conclusions it wrote. Zero alongside changes is the case worth
   * knowing about: everything it found needs somebody to answer it.
   */
  readonly applied: number;
  readonly changes: readonly ScanChange[];
}

/** One conclusion of a scan, or one question it could not answer. */
export interface ScanChange {
  readonly change:
    | "edited"
    | "moved"
    | "missing"
    | "returned"
    | "new"
    | "unmatched"
    | (string & {});
  readonly key?: string;
  readonly locator?: string;
  readonly from?: string;
  readonly to?: string;
  readonly presence?: Presence;
  /**
   * The digest of what is on disk. Carried back verbatim when a person settles
   * an unmatched file: the window never reads the working tree, so this is the
   * only honest statement it can make about those bytes.
   */
  readonly contentHash?: string;
  /** For `unmatched`: the records this file could be, best first. */
  readonly candidates?: readonly RenameCandidate[];
}

/** One record an unmatched file could be, and how alike the two names are. */
export interface RenameCandidate {
  readonly key: string;
  readonly locator: string;
  /** Between 0 and 1. Git scores renames the same way, and for the same reason. */
  readonly similarity: number;
}

/** How much the project holds, by type and by trust. */
export interface MemoryCounts {
  readonly total: number;
  readonly byKind: Readonly<Record<string, number>>;
  readonly byFreshness: Readonly<Record<string, number>>;
}

/**
 * What a column listing the project's own types is given.
 *
 * The counts describe the whole corpus and the records describe the current
 * selection, because the navigator lists every type while the workspace shows
 * one of them. Schema records are excluded from both.
 */
export interface MemoryView {
  readonly revision: string;
  readonly counts: MemoryCounts;
  readonly records: readonly MemoryRecord[];
  /** True when the selection holds more than this page. */
  readonly hasMore: boolean;
}

/** A typed relation to another entity, by key. */
export interface EntityLink {
  key: string;
  relation: string;
}

/** An entity on its way into memory. */
export interface EntityInput {
  key: string;
  /**
   * What the record is, as the project spells it.
   *
   * `EntityKind` widened to a string on purpose: those eleven are the kinds
   * Sync ships definitions for, and the corpus belongs to the project. A type
   * created in the type sheet or published by an extension is a kind this build
   * has never heard of, and a record of it is an ordinary record. Whether the
   * kind exists is the engine's to say.
   */
  kind: EntityKind | (string & {});
  title: string;
  /** Markdown body. */
  content: string;
  tags?: string[];
  links?: EntityLink[];
  /** Files this entity was written against. */
  paths_observed?: string[];
  /**
   * Files this entity's scope covers. The engine marks a record stale when
   * code under these paths changes, so this is what keeps freshness honest.
   */
  scope_paths?: string[];
  /** Product fields for the kind, validated against its type definition. */
  fields?: Record<string, unknown>;
}

/** Which engine is serving a project, and how. */
export interface EngineSummary {
  binary: string;
  /**
   * `override` when somebody named a sidecar, `bundled` when it is ours, and
   * `channel` where the engine is not on this machine at all — which is a
   * phone, and is why `binary` can be empty.
   */
  source: "override" | "bundled" | "channel";
  version: string;
  projectId: string;
  revision: string;
  /**
   * Which storage holds this project's records: `refs` for the Git objects
   * Sync initialises, `folder` for a project some other client set up as
   * files. `null` for an engine that did not say.
   */
  recordsBackend: string | null;
  /**
   * Whether the records are Git objects, and so whether checkpoints, history,
   * diff, fetch and push mean anything here. A project keeping its records as
   * files answers `unsupported` to every one of them, and those affordances are
   * left out rather than offered and explained afterwards.
   */
  recordsAreGit: boolean;
  /** `null` means no embedding model: search is FTS-only, not broken. */
  modelFingerprint: string | null;
}

/** Whether search can use vectors. */
export interface ModelState {
  modelId: string | null;
  dimensions: number | null;
  runtime: string;
  runtimeState: string;
  vectorSearch: boolean;
  ftsOnly: boolean;
  mode: "fts" | "hybrid";
}

/** The memory remote, which is separate from the code `origin`. */
export interface TransportState {
  remoteConfigured: boolean;
  remoteUrl: string | null;
  refspec: string | null;
  /**
   * The code `origin`. Memory is never published here — it travels with the
   * answer because it is the one address a window can sensibly suggest when it
   * offers to configure a memory remote, and asking for it separately would be
   * a second round trip for half a question.
   */
  codeOriginUrl: string | null;
}

/** What a fetch did. */
export interface FetchOutcome {
  merged: boolean;
  fastForward: boolean;
  /**
   * Records where both sides had moved the same thing. This side's version was
   * kept there, and the other is still a commit in the history — but somebody
   * has to be told whose sentence is not in front of them.
   */
  overlaps: Overlap[];
  /**
   * Where memory stood before this fetch, or `null`.
   *
   * What an undo needs. A merge is an ordinary commit on top of what was here,
   * so going back is naming the revision that was here — and once the fetch has
   * landed nothing else in the window knows it.
   */
  localRevisionBefore: string | null;
  /**
   * Where the fetch left memory. What an undo is checked against: the engine
   * refuses if the tip has moved past it, because then something was written
   * after the merge and going back would take it too.
   */
  localRevisionAfter: string | null;
}

/** One record a fetch merged over, and what of the other version it cost. */
export interface Overlap {
  readonly key: string;
  /** The same lines of the body were rewritten on both sides. */
  readonly body: boolean;
  /**
   * Members of the record both sides moved: `title`, `folder`, a product
   * field's own name. Empty when only the body collided.
   */
  readonly fields: readonly string[];
}

/**
 * What the remote had to say, when it was asked at all.
 *
 * Four states rather than a flag. `not_asked` and `unreachable` are not
 * `up_to_date`: a header that collapsed them would tell somebody their memory
 * is published when nobody had reached the remote to find out.
 */
export type RemoteCheck =
  | "not_asked"
  | "waiting"
  | "up_to_date"
  | "unreachable";

/** Whether the project's memory is in step with its remote. */
export interface SyncState {
  remoteConfigured: boolean;
  /**
   * **Records**, not commits. Every save is a commit, so a count of commits
   * would say `12` for twelve edits of one record — true of the history, and
   * not of anything a person would recognise as theirs.
   */
  unpublished: number;
  remote: RemoteCheck;
}

/**
 * Whether this repository's memory is here, still on a remote, or nowhere.
 *
 * `git clone` copies no `refs/memory/*`, so a fresh clone of a project with
 * years of memory and a project that never had any are the same picture from
 * inside — an empty corpus. Only the remote can tell them apart, and the flow
 * that opens a project asks before it offers to describe one: describing a
 * clone writes a `project` record that will never merge with the one already
 * on the remote.
 */
export type MemoryPresence =
  | { state: "present"; records: number }
  | { state: "not_fetched"; url: string; configured: boolean }
  | { state: "absent"; url: string | null }
  | { state: "unreachable"; url: string; reason: string };

/** Everything the UI needs to render memory's current state. */
export interface MemoryStatus {
  revision: string;
  /** The engine process is gone; the next call reconnects transparently. */
  reconnecting: boolean;
  model: ModelState;
  transport: TransportState;
}

/**
 * How a hit was found.
 *
 * The engine says so rather than the window inferring it from a missing score.
 * It matters because the two are not the same claim: `words` is the record
 * containing what was asked, `meaning` is the record being the nearest thing to
 * it — and in a corpus of a dozen records something is always nearest,
 * whatever was typed.
 */
export type MatchedBy = "words" | "meaning" | "both";

/** One search hit. */
export interface SearchHit {
  id: string;
  kind: string | null;
  title: string | null;
  /**
   * The window of the body around what matched — never the body.
   *
   * A hit says which record to read and why it ranked; the text itself is read
   * by key, once, for the one record somebody opens. `null` when the row had
   * no body to cut a window out of: a binary document, or one this branch does
   * not have.
   */
  excerpt: string | null;
  /** Where the window starts, in characters from the beginning of the body. */
  excerpt_at: number | null;
  /** How long the whole body is, in characters. */
  content_chars: number;
  archived: boolean;
  freshness: string | null;
  tags: string[];
  fts_score: number | null;
  vector_score: number | null;
  combined_rank: number;
  /**
   * Which channel found it. Absent from an engine older than this field, which
   * is read as `words` — the reading that shows a hit rather than hiding it.
   */
  matched?: MatchedBy;
}

/** A search result, including how it was answered. */
export interface SearchOutcome {
  hits: SearchHit[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
  /** `fts` when BM25 answered alone, `hybrid` when vectors contributed. */
  mode: "fts" | "hybrid";
  /**
   * True when `total` is a floor rather than a count: the engine stopped
   * counting at a thousand. What is shown then is "1000+", because a number
   * that no longer moves when the corpus grows is not a number.
   */
  total_capped: boolean;
  /**
   * True only when no embedding model is installed at all. A normal state to
   * be stated plainly — not an error to apologise for.
   */
  degraded: boolean;
  revision: string;
}

/**
 * The content of one record, as the engine reports it — including the bodies
 * that are not text.
 *
 * `encoding` is the field that matters and the one a caller may not skip:
 * `utf-8` is text, `base64` is bytes, and `none` is a body the engine did not
 * fetch and named with a `url` instead. A caller that ignores it draws a
 * picture as a page of base64.
 */
export interface ContentView {
  /** `record` when the body is the record's own, `file` when read through a locator. */
  readonly source: string;
  /** `null` when there was nothing to read — which is not the same as empty. */
  readonly content: string | null;
  readonly missing: boolean;
  /** Why it is not here, in the engine's own words. */
  readonly reason: string | null;
  /** The file the bytes were read from. */
  readonly path: string | null;
  readonly changed: boolean;
  readonly encoding: string | null;
  readonly bytes: number | null;
  readonly url: string | null;
  readonly mediaType: string | null;
}

/** A page of records plus counts over everything the filters selected. */
export interface Listing {
  revision: string;
  records: unknown[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
  counts: {
    total: number;
    by_kind: Record<string, number>;
    by_freshness: Record<string, number>;
    archived: number;
    live: number;
  };
}

/** What a write produced. */
export interface TransactionResult {
  revision: string;
  changed_keys: string[];
}

/**
 * A failure the UI can act on.
 *
 * `kind` is stable vocabulary, not prose: switch on it. The messages are for
 * people and may change.
 */
export interface MemoryFailure {
  kind: MemoryErrorKind;
  message: string;
  data: unknown;
}

/**
 * The failure kinds worth branching on.
 *
 * A newer engine may report a kind this build does not know, so treat the
 * string as open: match the ones you handle, fall through to a generic message
 * for the rest.
 */
export type MemoryErrorKind =
  /** A same-key write raced another. Refresh and show both sides. */
  | "conflict"
  /** The record did not satisfy its type definition. Point at the field. */
  | "invalid_record"
  /** The search index is unusable. Offer a reindex. */
  | "index"
  /** Push blocked by the stale-record policy. */
  | "push_blocked"
  /** No memory remote configured yet. */
  | "no_remote_configured"
  /** Code history moved; reconciliation needs an explicit full rebuild. */
  | "diverged"
  /**
   * The sidecar could not be started, stayed dead, or does not answer what
   * this window calls — which is what a bundle assembled from mismatched
   * halves looks like. The engine is linked into the sidecar rather than
   * installed beside it, so there is no separate engine version to disagree
   * about; the method list is the whole of the check.
   */
  | "sidecar"
  /** Anything else, including kinds newer engines introduce. */
  | (string & {});
