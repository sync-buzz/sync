/**
 * The frontend's only route to project memory.
 *
 * Every function here is one `invoke` into the Rust command layer, which owns
 * the engine session. The frontend holds no connection, no revision authority
 * and no retry policy — those belong where the session lives.
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  ContentView,
  Dependents,
  DocumentPatch,
  EngineSummary,
  EntityInput,
  FetchOutcome,
  FieldDeclaration,
  Freshness,
  MemoryDocument,
  MemoryType,
  MemoryView,
  Listing,
  MemoryFailure,
  MemoryPresence,
  MemoryStatus,
  RelationshipDeclaration,
  ScanOutcome,
  SearchOutcome,
  SyncState,
  TransactionResult,
  TransportState,
  TypeRemoval,
  TypeStorage,
} from "./types";

/**
 * A memory failure, thrown with the engine's stable `kind` intact.
 *
 * Tauri rejects with whatever the command returned, which loses the type. This
 * restores it so callers can `catch (error) { if (isMemoryFailure(error) && …`
 * rather than matching on message text.
 */
export class MemoryError extends Error implements MemoryFailure {
  readonly kind: MemoryFailure["kind"];
  readonly data: unknown;

  constructor(failure: MemoryFailure) {
    super(failure.message);
    this.name = "MemoryError";
    this.kind = failure.kind;
    this.data = failure.data;
  }
}

/** Whether a caught value is a memory failure with a kind worth branching on. */
export function isMemoryFailure(error: unknown): error is MemoryError {
  return error instanceof MemoryError;
}

async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "kind" in error &&
      "message" in error
    ) {
      throw new MemoryError(error as MemoryFailure);
    }
    throw error;
  }
}

/**
 * Open a project's memory, starting the engine if this is the first use.
 *
 * Publishing the type corpus is part of opening: the engine validates writes
 * against it, so it has to be there before the first one.
 */
export function openMemory(project: string): Promise<EngineSummary> {
  return call<EngineSummary>("memory_open", { project });
}

/** Read the states the UI renders: lock, search mode, remote, revision. */
export function memoryStatus(project: string): Promise<MemoryStatus> {
  return call<MemoryStatus>("memory_status", { project });
}

/**
 * The types the project holds, in the order Sync publishes them.
 *
 * Asked of the store rather than taken from a constant here: the store is what
 * decides which types a project has, and a corpus written by another version of
 * Sync is exactly where the two lists differ.
 */
export function memoryTypes(project: string): Promise<MemoryType[]> {
  return call<MemoryType[]>("memory_types", { project });
}

/**
 * What a type is, as far as the window decides it.
 *
 * The identifier and the name are two answers, not one. `kind` is what the
 * engine stores on every record of the type; `title` is what a person reads,
 * and it can be several words. The window generates the first from the second
 * when a type is added and never again — a stored identifier is a fact, and one
 * re-derived on every save would move under the records carrying it.
 */
export interface TypeDefinition {
  kind: string;
  title: string;
  description: string;
  icon: string;
  /**
   * Which storage holds this type's documents, and what it needs to be told.
   * Answered when the type is created and never edited: where documents live is
   * not a setting whose change may quietly move data, and moving them is an
   * operation of its own with a plan and an acknowledgement.
   *
   * Omitted — or naming nothing — means the bodies live in the records, which
   * is what a definition saying nothing means to the engine.
   */
  storage?: TypeStorage;
}

/**
 * The four answers a definition is written from, without the storage section.
 *
 * Storage travels on its own route — a type whose records are repository files
 * is created by attaching the folder — so the two commands that write a
 * definition take exactly what they write.
 */
function definitionFields(type: TypeDefinition) {
  return {
    kind: type.kind,
    title: type.title,
    description: type.description,
    icon: type.icon,
  };
}

/**
 * Add a type to the project's corpus, and get the corpus back.
 *
 * The answer is the list as the store now holds it, so the window does not have
 * to guess what it just wrote.
 */
export function createMemoryType(
  project: string,
  type: TypeDefinition,
): Promise<MemoryType[]> {
  return call<MemoryType[]>("memory_type_create", {
    project,
    ...definitionFields(type),
  });
}

/**
 * Redefine a type the project holds, and get the corpus back.
 *
 * `kind` says which type and is the one answer that cannot change: it is the
 * identifier every record carries and the key the definition lives under, and
 * the store has no rename. Everything the definition holds beyond these answers
 * is preserved by the command rather than rebuilt from them.
 */
/**
 * One type an extension publishes, as the catalogue states it.
 *
 * The last three are what makes an extension's type more than a name. They are
 * sent as the extension wrote them and validated by the engine against its own
 * schema — a definition it refuses is an extension that must not count as
 * installed, which is a better answer than one this layer could give.
 */
export interface ExtensionTypeInput {
  readonly kind: string;
  readonly title: string;
  readonly description: string;
  readonly icon: string;
  /** The product fields records of this type carry. */
  readonly fields?: Readonly<Record<string, FieldDeclaration>>;
  /** The relations they may hold. A type declaring none cannot link at all. */
  readonly relationships?: Readonly<Record<string, RelationshipDeclaration>>;
  /** What an agent reads before writing one. */
  readonly guidance?: string;
}

/**
 * Publish the types an extension brings, as one transaction.
 *
 * Installing is all-or-nothing: a project holding some of an extension's types
 * would validate its records against a schema nobody chose. Publishing a set
 * that is already there writes nothing, so a project can declare what it uses
 * and have it published on every machine that opens it without a commit per
 * open.
 */
export function publishExtensionTypes(
  project: string,
  types: readonly ExtensionTypeInput[],
): Promise<MemoryType[]> {
  return call<MemoryType[]>("memory_extension_types_publish", {
    project,
    types,
  });
}

export function updateMemoryType(
  project: string,
  type: TypeDefinition,
): Promise<MemoryType[]> {
  // Storage is not sent. The stored definition keeps whatever it declares —
  // the command rewrites three members of it and preserves the rest — and an
  // edit that carried a storage section would be asking the engine to move
  // records, which is a different operation with a plan of its own.
  return call<MemoryType[]>("memory_type_update", {
    project,
    ...definitionFields(type),
  });
}

/**
 * Remove a type and every record written as it, in one transaction.
 *
 * The records go with it because a record whose kind has no definition is one
 * the engine's strict schema will not let anybody read or rewrite. The answer
 * carries how many there were, which is the number worth reporting: it is what
 * happened, rather than what the window counted before asking.
 */
export function deleteMemoryType(
  project: string,
  kind: string,
): Promise<TypeRemoval> {
  return call<TypeRemoval>("memory_type_delete", { project, kind });
}

/**
 * What attaching a folder produced: the corpus's types, and what the first scan
 * made of the files already in it.
 */
export interface FolderAttachment {
  readonly types: readonly MemoryType[];
  readonly scan: ScanOutcome;
}

/** What the window asks for when it attaches a folder of the repository. */
export interface FolderAttachmentInput {
  kind: string;
  title: string;
  description: string;
  icon: string;
  /**
   * A directory relative to the repository root — `docs`, `docs/guides`.
   *
   * Every file in it becomes a document of the type, images and PDFs included.
   * There is no mask: one hid a person's own files from them, and a folder
   * holding two types was never expressible anyway.
   */
  folder: string;
}

/**
 * Attach a folder of the repository as a type of documents.
 *
 * Nothing is written into the folder — not a marker, not an id in frontmatter —
 * so a colleague who has never heard of Memory sees a repository that has not
 * changed. That is the promise the whole arrangement rests on, which is why the
 * window states it before calling this rather than leaving it to be discovered.
 *
 * The answer carries the first scan, whose unmatched entries are the one part
 * of attaching that cannot be automated. `UnmatchedFiles` is where that is
 * explained, because that is where somebody is asked.
 */
export function attachFolder(
  project: string,
  attachment: FolderAttachmentInput,
): Promise<FolderAttachment> {
  return call<FolderAttachment>("memory_folder_attach", { project, attachment });
}

/**
 * One folder, and everything known about it from both sources at once.
 *
 * The two origins are separate answers because they mean different things.
 * `inStorage` without `inRecords` is an empty directory of the working tree —
 * somewhere a person can file into, and something they already see in Finder.
 * `inRecords` without `inStorage` is a folder whose documents this branch does
 * not have. A tree that showed one word for both would be answering a question
 * nobody asked.
 */
export interface MemoryFolder {
  /** Repository-relative. The root is `""`. */
  readonly path: string;
  readonly inRecords: boolean;
  readonly inStorage: boolean;
  /**
   * How many documents are filed directly in it — not what is in the folders
   * below, and not the type definitions, which are schema rather than something
   * the project knows.
   */
  readonly records: number;
  /**
   * The key of the record that *is* this folder, when one is.
   *
   * A tree needs it, or it draws that record twice: once as the folder and once
   * as its own child.
   */
  readonly describedBy: string | null;
}

/**
 * The project's folders, read live.
 *
 * `folder` absent asks about the whole project; `""` asks about the root, which
 * is a folder like any other — the two are different questions. `subtree` says
 * whether the region reaches below the folder it names.
 *
 * Never cached across a project: Git keeps no empty directories, so an empty
 * `docs/api/` is a fact about one working tree and absent from a fresh clone.
 */
export function memoryFolders(
  project: string,
  folder?: string,
  subtree = false,
  /**
   * One type's folders. Absent asks about the project.
   *
   * A tree drawn per type needs this. Folders are a namespace the whole project
   * shares — nothing stops a decision from being filed in `docs/guides` next to
   * the documents — so without it every type appears to have every folder, in
   * places its records are not.
   */
  kind?: string,
): Promise<MemoryFolder[]> {
  return call<MemoryFolder[]>("memory_folders", {
    project,
    folder,
    subtree,
    kind,
  });
}

/**
 * Make a folder that nothing is in yet, under the type named by `kind`.
 *
 * What a folder *is* differs by where that type keeps its documents, and the
 * engine decides it from the kind: a directory for documents that are files,
 * and the record that carries `isFolder` for documents that are records. The
 * window does not branch on it, which is the point — a second place deciding
 * this is a second place it can be decided differently.
 *
 * One difference reaches a person and nothing can hide it. Git keeps no empty
 * directories, so a folder made in an attached directory is a fact about this
 * working tree until something is filed in it, while one made in the records
 * travels immediately. Closing that would mean writing a marker into somebody's
 * repository, which is the one thing attaching a folder promises not to do.
 */
export function createMemoryFolder(
  project: string,
  folder: string,
  kind: string,
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_folder_create", {
    project,
    folder,
    kind,
  });
}

/**
 * Take a folder and everything filed under it, and say how many went.
 *
 * Everything, whatever its type. A folder exists while something is in it, so
 * sparing one type's records would empty the folder rather than delete it —
 * which is why the confirmation counts across types and says so.
 *
 * Files go with their records. Directories are removed only while they are
 * empty, so a file no scan has reached is left where somebody put it.
 */
export function deleteMemoryFolder(
  project: string,
  folder: string,
): Promise<number> {
  return call<number>("memory_folder_delete", { project, folder });
}

/**
 * How many records a folder holds, at any depth and whatever their type.
 *
 * Asked of the store rather than counted from the tree, which shows one type's
 * and only the level it is on. A confirmation may not name a number it guessed.
 */
export function memoryFolderToll(
  project: string,
  folder: string,
): Promise<number> {
  return call<number>("memory_folder_toll", { project, folder });
}

/**
 * Rename a folder, moving every record filed under it in one transaction.
 *
 * Where the documents are files the directory is renamed too, and the locators
 * follow it — the keys do not, so no link breaks. A type's own storage root is
 * refused: moving that is a change to the type, not a rename of a folder.
 */
export function renameMemoryFolder(
  project: string,
  from: string,
  to: string,
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_folder_rename", { project, from, to });
}

/**
 * File one record in another folder. `""` is the root.
 *
 * Whether a file moves with it is the engine's business and deliberately not
 * this window's: a record whose body is a repository file has a folder that
 * *is* that file's directory, and the engine moves both or neither. Sync never
 * writes into somebody's working tree itself.
 */
export function moveMemoryDocument(
  project: string,
  key: string,
  folder: string,
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_document_move", { project, key, folder });
}

/**
 * Reconcile every attached folder with the records, and report what moved.
 *
 * Worth calling when a project opens and when the window regains focus, which
 * is what the engine asks of a client that can see either: before every read is
 * too expensive, and only at open is too rare for somebody editing files in the
 * next window. A project with no attached folder scans nothing.
 */
export function scanFolders(project: string): Promise<ScanOutcome> {
  return call<ScanOutcome>("memory_scan", { project });
}

/**
 * Settle a file the scan could not attribute to a record.
 *
 * `adopt` names the record the file turned out to be — the record keeps its
 * key, so every link pointing at it survives the rename. Omitting it says the
 * file is a document in its own right.
 *
 * `contentHash` comes from the scan report rather than being computed here:
 * nothing between the window and the engine reads the working tree, and a
 * digest invented in the browser would be a claim about bytes it never saw.
 */
export function resolveUnmatched(
  project: string,
  file: { locator: string; contentHash: string; kind: string },
  adopt?: string,
): Promise<ScanOutcome> {
  return call<ScanOutcome>("memory_unmatched_resolve", {
    project,
    locator: file.locator,
    contentHash: file.contentHash,
    kind: file.kind,
    adopt,
  });
}

/**
 * How many records of one kind the project holds, archived ones included.
 *
 * Asked of the store rather than read off the navigator: the number on a row is
 * of the last read and excludes nothing this window hides, and a confirmation
 * that names a number has to name the one that is about to be destroyed.
 */
export async function countRecordsOfKind(
  project: string,
  kind: string,
): Promise<number> {
  const listing = await listRecords(project, {
    kind,
    limit: 1,
    metadata_only: true,
  });
  return listing.counts.total;
}

/** Which part of the corpus the column is showing. */
export interface MemorySelection {
  kind?: string;
  freshness?: Freshness[];
  /**
   * One folder. `""` is the root — the records filed nowhere.
   *
   * The engine files every record under the directory of its locator, so this
   * is also how a path is turned back into the record that holds it. Passed
   * through untouched: the command hands the selection to the engine as it
   * stands, so this is a name for a filter the engine already had rather than
   * a new capability.
   */
  folder?: string;
  /**
   * Whether `folder` above reaches below the folder it names.
   *
   * `exact` is the engine's default and what asking for one folder means
   * without saying so. `subtree` is what a tree wants when a branch is
   * collapsed and its whole contents should still be counted.
   */
  folderScope?: "exact" | "subtree";
  /**
   * Which of the type's own fields each row should carry.
   *
   * A row is a name and a state, and for years that was every question anybody
   * asked of a list. A column that groups its rows by a field asks a second
   * one, and it cannot answer it by opening every record — so it names the
   * fields it will draw and gets those, rather than the window deciding on its
   * behalf. Absent asks for none, which is what a list that draws none should
   * ask for: a type may declare a field of several lines, and a page of prose
   * per row is a cost nobody reading titles agreed to pay.
   *
   * Not a filter. It says what comes back about each record the selection
   * already chose, and never which records those are.
   */
  fields?: readonly string[];
  limit?: number;
  offset?: number;
}

/**
 * Counts over the whole corpus, and one page of the selection.
 *
 * `hidden` names the kinds this window is not showing. They are excluded from
 * the counts as well as from the page, so the numbers describe what is on the
 * screen rather than what the store happens to hold.
 */
export function loadRecords(
  project: string,
  selection: MemorySelection = {},
  hidden: readonly string[] = [],
): Promise<MemoryView> {
  return call<MemoryView>("memory_records", {
    project,
    selection,
    hidden,
  });
}

/**
 * One record, whole.
 *
 * `null` when the key does not exist at the current revision — a record deleted
 * while the window had it open is an answer, not a failure.
 */
export function memoryDocument(
  project: string,
  key: string,
): Promise<MemoryDocument | null> {
  return call<MemoryDocument | null>("memory_document", { project, key });
}

/**
 * Put a file into a type's storage, and get back the record that names it.
 *
 * The one route by which something that is not text reaches the working tree.
 * `content` is base64 because the protocol is JSON; the engine decodes it and
 * writes the file, which is why nothing in the window ever holds a path.
 *
 * The file lands in the root of the storage. Where a project keeps its pictures
 * is the project's arrangement, and inventing a folder for them would be this
 * application making that arrangement in somebody else's repository.
 */
export function createFileDocument(
  project: string,
  kind: string,
  name: string,
  content: string,
): Promise<MemoryDocument> {
  return call<MemoryDocument>("memory_file_create", {
    project,
    kind,
    name,
    content,
  });
}

/**
 * The content of one record, whatever shape it has.
 *
 * Asked for a single file by whatever is about to draw it, rather than carried
 * on every record a list reads: a folder holds pictures now that there is no
 * mask on it, and a PNG travelling with every row would be megabytes of base64
 * nobody asked for.
 */
export function documentContent(
  project: string,
  key: string,
): Promise<ContentView> {
  return call<ContentView>("memory_content", { project, key });
}

/**
 * Change what the patch names in one record, and get the record back as stored.
 *
 * Only what the patch names travels. The command reads what is there and hands
 * the rest of the record back to the store untouched — scope, tags, links,
 * freshness and the fields the type declares — because a record rebuilt from
 * what this window happens to know would drop everything it does not.
 *
 * `null` means the record left the store between the write and the read.
 */
export function updateMemoryDocument(
  project: string,
  key: string,
  edits: DocumentPatch,
): Promise<MemoryDocument | null> {
  return call<MemoryDocument | null>("memory_document_update", {
    project,
    key,
    edits,
  });
}

/**
 * Create an empty record of one of the project's types.
 *
 * The kind decides what the record must carry and the project's own definition
 * decides what that is, so nothing about its shape is this window's to choose:
 * what travels is the kind and a title somebody is about to type over. The key
 * is generated where the store can check it is free.
 */
export function createMemoryDocument(
  project: string,
  kind: string,
  title: string,
  /**
   * Where it goes. Absent files it where the type does by default — the root of
   * its storage, or no folder at all for a type whose documents are its
   * records — which is what "new record" means when nobody has picked a folder.
   */
  folder?: string,
): Promise<MemoryDocument> {
  return call<MemoryDocument>("memory_document_create", {
    project,
    kind,
    title,
    folder,
  });
}

/**
 * The document that *is* a folder: opened if it exists, written if it does not.
 *
 * How a folder gets a title and a text of its own. What comes back is an
 * ordinary record of an ordinary type, so what somebody writes in it is indexed
 * and found by search like any other document — nothing in the engine treats it
 * specially, which is exactly why it works.
 *
 * A folder that already has one answers with it rather than writing a second:
 * two records standing for one folder is a question with no answer, and the
 * engine refuses it for the same reason.
 */
export function describeMemoryFolder(
  project: string,
  folder: string,
  kind: string,
): Promise<MemoryDocument> {
  return call<MemoryDocument>("memory_folder_describe", {
    project,
    folder,
    kind,
  });
}

/**
 * Delete records by key, all of them or none.
 *
 * One transaction: a record deleted together with the ones that depend on it is
 * one decision, and half of it applied is a corpus in a state nobody chose.
 */
export function deleteMemoryDocuments(
  project: string,
  keys: readonly string[],
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_document_delete", {
    project,
    keys: [...keys],
  });
}

/**
 * What holds on to a record: the records that link to it, and the ones that
 * mention it in prose.
 *
 * Asked before a deletion is confirmed, because those are two different
 * consequences and only the person deciding can weigh them.
 */
export function documentDependents(
  project: string,
  key: string,
): Promise<Dependents> {
  return call<Dependents>("memory_document_dependents", { project, key });
}

/** Filters, sorting and paging over the corpus. */
export interface ListQuery {
  kind?: string;
  tags?: string[];
  archived?: boolean;
  freshness?: string[];
  sort?: "key" | "kind" | "title" | "freshness" | "archived";
  sort_order?: "asc" | "desc";
  /** Omits bodies — the shape a list view needs. */
  metadata_only?: boolean;
  limit?: number;
  offset?: number;
}

export function listRecords(project: string, query: ListQuery = {}): Promise<Listing> {
  return call<Listing>("memory_list", { project, query });
}

/** Search parameters. Filters match `listRecords`. */
export interface SearchQuery extends ListQuery {
  query: string;
  /**
   * Narrow to several types in one ask. Empty or absent is every type.
   *
   * A union with `kind` rather than a replacement: the engine takes both and
   * asks for both. The window uses this one, because what it filters by is a
   * set of checkboxes — and a set answered one kind at a time would be several
   * searches whose ranks cannot be compared with each other.
   */
  kinds?: string[];
}

export function search(project: string, query: SearchQuery): Promise<SearchOutcome> {
  return call<SearchOutcome>("memory_search", { project, query });
}

export function getRecord(project: string, key: string): Promise<unknown> {
  return call("memory_get", { project, key });
}

/**
 * Write entities in one transaction.
 *
 * No transaction id travels with this. The id must be unique per attempt — the
 * engine refuses a reused one, which is what makes a retry after a lost response
 * safe rather than a silent double write — and the only party that can be sure
 * it is naming its own attempt is the one doing the writing. That is the
 * sidecar, and it allocates one per call.
 */
export function saveEntities(
  project: string,
  entities: EntityInput[],
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_save", { project, entities });
}

/**
 * Delete records by key, in one transaction.
 *
 * The same path a document deletion takes, checks included: a type definition
 * goes with its type and the project's own record is what the project is opened
 * by, so neither is deleted through here.
 */
export function deleteEntities(
  project: string,
  keys: string[],
): Promise<TransactionResult> {
  return call<TransactionResult>("memory_delete", { project, keys });
}

/**
 * Whether the project's memory is in step with its remote.
 *
 * `askRemote` is what decides whether this waits on the network. The count of
 * unpublished records is computed locally, so the header has something true to
 * say before anybody has been asked anything.
 */
export function syncState(
  project: string,
  askRemote = false,
): Promise<SyncState> {
  return call<SyncState>("memory_sync_state", { project, askRemote });
}

/**
 * Whether this repository's memory is here, still on a remote, or nowhere.
 *
 * Asked before a project is described, never derived from an empty corpus: an
 * empty corpus is exactly what a fresh clone and a new project have in common.
 */
export function memoryPresence(project: string): Promise<MemoryPresence> {
  return call<MemoryPresence>("memory_presence", { project });
}

/** Configure the memory remote, which is separate from the code `origin`. */
export function setMemoryRemote(
  project: string,
  url: string,
  refspec?: string,
): Promise<TransportState> {
  return call<TransportState>("memory_remote_set", { project, url, refspec });
}

/** Forget the memory remote. Nothing local is touched. */
export function removeMemoryRemote(project: string): Promise<TransportState> {
  return call<TransportState>("memory_remote_remove", { project });
}

/** Fetch memory from its remote and merge it. */
export function fetchMemory(project: string): Promise<FetchOutcome> {
  return call<FetchOutcome>("memory_fetch", { project });
}

/**
 * Put memory back where it stood, undoing what has happened since.
 *
 * What a fetch is undone with: the revision to name is the
 * `localRevisionBefore` that fetch reported. Backwards along memory's own
 * history and nowhere else, so this cannot arrive at a state nobody wrote.
 *
 * Nothing is destroyed — the commits after it stay in the repository — and it
 * moves this clone alone. A revision already published is still on the remote,
 * and the next fetch brings it back.
 */
export function rewindMemory(
  project: string,
  revision: string,
  /**
   * Where memory should still stand. A tip that has moved past it means
   * somebody has written since, and the undo is refused rather than carrying
   * their record away with the merge.
   */
  expected: string,
): Promise<void> {
  return call<void>("memory_rewind", { project, revision, expected });
}

export function pushMemory(project: string, force = false): Promise<unknown> {
  return call("memory_push", { project, force });
}

/** Rebuild the search index, after corruption or a manual Git operation. */
export function reindex(project: string): Promise<unknown> {
  return call("memory_reindex", { project });
}

/**
 * Catch memory up with code history, rebuilding after it was rewritten.
 *
 * The engine does ordinary catch-up by itself, ahead of every write. This is
 * for the one state it will not settle alone: a rebase, a reset or a replaced
 * branch leaves reconciliation on a commit the current history does not descend
 * from, and until somebody says the new history is the real one every write is
 * refused with `diverged`.
 *
 * `fullRebuild` is that answer, and it is asked for rather than assumed because
 * it costs something: every record becomes `unverified`. Nothing written is
 * lost — what changes is how far a claim may be trusted before somebody checks
 * it against the code again.
 */
export function reconcileMemory(
  project: string,
  fullRebuild: boolean,
): Promise<unknown> {
  return call("memory_reconcile", { project, fullRebuild });
}
