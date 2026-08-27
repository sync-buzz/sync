//! Sync's domain, over the engine in this process.
//!
//! Moved here from the window's client, bodies intact: this is the code that
//! knows a decision from a document, what a type definition looks like and how
//! a conflict is replayed. It used to sit on the window's side of a process
//! boundary and reach the engine through a pipe; it now sits on the engine's
//! side and reaches it through a call. Nothing else about it changed, which is
//! why the move is a move and not a rewrite.
//!
//! The window keeps the vocabulary — the DTOs these methods return — and loses
//! the knowledge of how they are built.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use memory_hub_mcp::EmbeddingProvider;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

use crate::engine::Engine;
use sync_memory::mapping::{
    DOCUMENT_EXTENSION, Dependent, Dependents, Document, DocumentEdits, Entity, EntityKind,
    NEW_DOCUMENT_BODY, NOT_ON_BRANCH, PRESENT, REMOVED, RecordEntry, RecordType, RecordsCounts,
    RecordsPage, TYPE_KIND, TypeDeclaration, TypeStorage, already_published, bytes_hash,
    content_hash, corpus_matches, definition_of, delete, document_put, document_stem,
    is_definition_kind, is_fixed_record, is_own_kind, new_document_put, own_type_definitions,
    product_fields, reference_key, reference_put, relocate_put, sort_types, split_file_name,
    suggested_key, titled_put, type_definition, type_key, type_record,
};
use sync_memory::{
    ContentView, EntityInput, FolderEntry, Listing, MemoryPresence, ModelStatus, ProjectSettings,
    RecordView, ScanOutcome, SearchOutcome, SyncState, TransactionResult, TransportStatus,
};
use sync_memory::{MemoryError, Result};

/// A page big enough for every type a project has.
const TYPE_PAGE: usize = 200;

/// The engine's own ceiling, for the same reason it has one.
const MAX_KEY_ORDINAL: u32 = 1_000;

/// One project's memory and the domain over it.
pub struct Domain {
    engine: Engine,
    /// The last revision this session observed, and what `expected_revision`
    /// carries on the next write. The daemon holds it now: it is the writer,
    /// so it is the one that can be sure what it wrote last.
    revision: String,
    /// Whether the revision above was ever read successfully.
    ///
    /// Separate from "the string is empty" because an empty revision is the
    /// engine's business to define, not a sentinel this file may borrow.
    initialised: bool,
    /// How many transaction ids this session has handed out.
    transactions: u64,
}

impl Domain {
    /// Open the domain over the project rooted at `project`.
    ///
    /// Infallible, and that is the whole point. Reading a project's memory can
    /// refuse, and a constructor that propagated it would end the process
    /// before it had answered anything at all: the window would see a pipe that
    /// closed rather than the refusal it can act on.
    ///
    /// So the read is deferred to [`Self::ensure_initialised`], which the
    /// surface calls ahead of the operations that need a corpus and skips for
    /// the few that exist to make one readable.
    pub fn open(project: PathBuf, embeddings: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        Self {
            engine: Engine::open(project, embeddings),
            revision: String::new(),
            initialised: false,
            transactions: 0,
        }
    }

    /// Read the project's revision once, which is what gives a project that has
    /// never held memory its storage.
    ///
    /// A no-op after the first read that succeeds, and a fresh attempt after
    /// every one that did not — so a session that refused once can carry on
    /// once whatever refused it is fixed.
    ///
    /// # Errors
    ///
    /// Returns whatever reading the revision refused, `kind` intact.
    pub fn ensure_initialised(&mut self) -> Result<()> {
        if self.initialised {
            return Ok(());
        }
        self.refresh_revision().map(|_| ())
    }

    /// Read the project's revision once, for a caller that has already made
    /// sure there is memory to read.
    ///
    /// The agent surface's counterpart to [`Self::ensure_initialised`], and the
    /// difference is no longer here: reading the revision opens the records,
    /// and opening them is what creates them. So the agent surface asks
    /// [`Self::presence`] first and never reaches this for a repository that
    /// holds nothing — see `own::readable`.
    ///
    /// The window opens the storage because a person who opened a folder as a
    /// project has decided it keeps memory; the decision is the gesture. An
    /// agent that connected to a repository has decided nothing, and writing
    /// `refs/memory/*` into somebody's repository on the strength of a question
    /// being asked is not a decision Sync gets to make for them.
    ///
    /// # Errors
    ///
    /// Returns whatever reading the revision refused.
    pub fn ensure_revision(&mut self) -> Result<()> {
        if self.initialised {
            return Ok(());
        }
        self.revision = self.read_current_revision()?;
        self.initialised = true;
        Ok(())
    }

    /// Run one engine tool.
    ///
    /// The name the moved code already used, so the bodies did not have to
    /// change: what used to cross a pipe is now a call.
    fn call(&mut self, tool: &str, arguments: &Value) -> Result<Value> {
        self.engine.call(tool, arguments)
    }

    /// Run one engine tool and hand back the engine's own answer, untouched.
    ///
    /// The MCP surface publishes engine tools under the engine's own names and
    /// republishes their answers verbatim — see [`crate::engine::Engine::run`].
    pub fn engine_tool(&mut self, tool: &str, arguments: &Value) -> memory_hub_mcp::ToolCall {
        self.engine.run(tool, arguments)
    }

    /// What the engine says about the project it is serving.
    ///
    /// The window shows most of it — the backend, whether records live in Git —
    /// so it is read on connecting and re-read when something changes it.
    ///
    /// # Errors
    ///
    /// Returns whatever reading the project resource refused.
    pub fn describe(&mut self) -> Result<sync_memory::Handshake> {
        parse(self.engine.resource(sync_memory::PROJECT_RESOURCE)?)
    }

    /// The revision every read serves and every write compares against.
    fn read_current_revision(&mut self) -> Result<String> {
        self.engine.revision()
    }

    /// Re-read the revision and remember it.
    ///
    /// Public because the window asks for exactly this and nothing else when it
    /// wants to know whether the memory moved. Answering that from the field
    /// below would answer from what *this* session last wrote, which is the one
    /// thing the question is not about: a `git pull`, a second window and the
    /// engine's own CLI all move a revision without this process hearing of it.
    ///
    /// Reading the revision opens the records, so the first read of a project
    /// that has never held memory is what gives it storage. A read that
    /// succeeds is also an initialisation — the memory is demonstrably
    /// readable — so it counts as one.
    ///
    /// # Errors
    ///
    /// Returns whatever reading the revision refused.
    pub fn refresh_revision(&mut self) -> Result<String> {
        self.revision = self.read_current_revision()?;
        self.initialised = true;
        Ok(self.revision.clone())
    }

    // ── Reads ───────────────────────────────────────────────────────────────

    /// Fetch one record by key.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn get_record(&mut self, key: &str) -> Result<RecordView> {
        let revision = self.revision.clone();
        let value = self.call(
            "memory_get_record",
            &json!({"key": key, "revision": revision}),
        )?;
        parse(value)
    }

    /// List records with filters, sorting and paging.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn list_records(&mut self, query: &Value) -> Result<Listing> {
        let value = self.call("memory_list_records", query)?;
        parse(value)
    }

    /// Search, reporting honestly whether the answer is FTS-only.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn search(&mut self, query: &Value) -> Result<SearchOutcome> {
        let value = self.call("memory_search", query)?;
        parse(value)
    }

    /// Publish the definitions Sync needs for its own records, if the store
    /// does not already hold them.
    ///
    /// Returns whether anything was written. Only [`OWN_KINDS`] is touched —
    /// which is one kind, `project`, because the project's own record has to
    /// have a type the strict schema knows. What else a project can say is the
    /// project's decision, made in the window or by an agent, and opening a
    /// window is not the moment to make it for them.
    ///
    /// Two reasons this is a read before it is a write. A transaction id may not
    /// be reused, so publishing under a fixed one fails the second time a
    /// project is opened in the same session — which is what made switching
    /// between projects lose the type list. And every write is a commit:
    /// republishing an identical definition would put one on `refs/memory/*`
    /// each time a window opened, for nothing.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn publish_types(&mut self) -> Result<bool> {
        let stored = self.list_records(&json!({"kind": TYPE_KIND, "limit": TYPE_PAGE}))?;
        if corpus_matches(&stored.records) {
            return Ok(false);
        }
        let transaction = self.next_transaction_id("sync-types");
        self.apply(&transaction, &own_type_definitions())?;
        Ok(true)
    }

    /// A transaction id no other attempt will reuse.
    ///
    /// The engine refuses a reused id, which is what makes a retry after a lost
    /// response safe rather than a silent double write — so an id has to name
    /// *this attempt*, not the operation. The revision it was built against and
    /// a per-session counter are enough for that: a repeat of the same attempt
    /// is the one case where reusing an id is correct, and it is the case the
    /// engine's own replay handles.
    pub fn next_transaction_id(&mut self, prefix: &str) -> String {
        self.transactions += 1;
        format!("{prefix}-{}-{}", self.revision, self.transactions)
    }

    /// The types the project holds.
    ///
    /// Read as records rather than through `memory_list_types`: the engine
    /// parses a definition into its own shape and drops what it does not model,
    /// and the mark a type is drawn with is exactly that — Sync's, kept inside
    /// the definition, because a type created in the window is one no build
    /// knows about.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn list_types(&mut self) -> Result<Vec<RecordType>> {
        let listing = self.list_records(&json!({"kind": TYPE_KIND, "limit": TYPE_PAGE}))?;
        let mut types: Vec<RecordType> = listing
            .records
            .iter()
            .filter_map(RecordType::from_record)
            .collect();
        // One question the definitions cannot answer, asked once for the whole
        // corpus. Whether a type can be written is a fact about the storage it
        // names — a folder may be read-only or simply not there — and the
        // window has to know it *before* offering to create a document rather
        // than discovering it from a refusal after somebody typed one.
        let writable = self.writable_kinds()?;
        for type_ in &mut types {
            if let Some(answer) = writable.get(&type_.kind) {
                type_.writable = *answer;
            }
        }
        sort_types(&mut types);
        Ok(types)
    }

    /// Which kinds the engine says can be written, by kind name.
    ///
    /// A kind the answer does not mention keeps the assumption it was read
    /// with — writable, and refused at the write if it turns out not to be.
    /// That is the honest default: the alternative is a window grey-ing out the
    /// one command a type exists for because a newer engine renamed a field.
    fn writable_kinds(&mut self) -> Result<BTreeMap<String, bool>> {
        let answer = self.call("memory_list_types", &json!({}))?;
        Ok(answer
            .get("types")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|summary| {
                let kind = summary.get("kind_name").and_then(Value::as_str)?;
                let writable = summary.get("writable").and_then(Value::as_bool)?;
                Some((kind.to_owned(), writable))
            })
            .collect())
    }

    /// Add a type to the project's corpus.
    ///
    /// The engine validates every later write against it, so this is the one
    /// place a project decides what it is able to say. Nothing is published
    /// alongside it: a type is created because somebody asked for that type.
    ///
    /// `kind` is the identifier — what every record of the type carries and what
    /// an agent writes — and `title` is what a person reads. The caller derives
    /// the first from the second; this layer stores what it is given, because a
    /// kind may also arrive from an extension carrying its own prefix.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_record` when the
    /// definition does not satisfy the engine's own schema for one.
    pub fn create_type(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
    ) -> Result<TransactionResult> {
        // Sync maintains its own types and republishes them when a project
        // lacks one; letting a create overwrite one would put the definition
        // and the build at odds until the next open silently corrected it.
        if is_own_kind(kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is Sync's own type and is always present."),
                json!({"kind": kind}),
            ));
        }
        check_identifier(kind)?;
        let transaction = self.next_transaction_id("sync-type");
        self.apply(
            &transaction,
            &[type_definition(&TypeDeclaration::new(
                kind,
                title,
                description,
                icon,
            ))],
        )
    }

    /// Publish the types an extension brings, as one transaction.
    ///
    /// Installing an extension either happens or does not: a project left
    /// holding three of an extension's five types is a project whose records
    /// validate against a schema nobody chose. So this is one transaction, not
    /// a loop over [`Self::create_type`].
    ///
    /// **Only what differs is written.** Installing something already installed
    /// is a no-op rather than a commit on `refs/memory/*`, which matters
    /// because the same set is republished whenever a project that declares it
    /// is opened on a machine that has it. The answer says whether anything was
    /// actually written.
    ///
    /// Definitions arrive whole rather than as three strings, because an
    /// extension's type declares fields and relationships that a create built
    /// from a name could not carry.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_record` when a definition
    /// does not satisfy the engine's schema — an extension whose types the
    /// store refuses is one that must not count as installed.
    pub fn publish_extension_types(&mut self, definitions: &[Value]) -> Result<bool> {
        if definitions.is_empty() {
            return Ok(false);
        }

        let stored = self.list_records(&json!({"kind": TYPE_KIND, "limit": TYPE_PAGE}))?;
        let pending: Vec<Value> = definitions
            .iter()
            .filter(|definition| !already_published(&stored.records, definition))
            .cloned()
            .collect();
        if pending.is_empty() {
            return Ok(false);
        }

        let transaction = self.next_transaction_id("sync-extension-types");
        self.apply(&transaction, &pending)?;
        Ok(true)
    }

    /// Redefine a type the project holds.
    ///
    /// What is written is the stored definition with a new name, description and
    /// mark, not a definition built from those three: a type may declare fields,
    /// relationships or members a later engine added, and none of that is the
    /// window's to discard while changing a sentence.
    ///
    /// The `kind` is what cannot change. It is the identifier every record of
    /// the type carries and the key the definition lives under, so moving it
    /// would be a rewrite of every record rather than an edit of one — and the
    /// store has no rename. The name a person reads is not that identifier,
    /// which is exactly why it can be changed freely.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for one of Sync's own
    /// types or for a kind the project does not hold.
    pub fn update_type(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
    ) -> Result<TransactionResult> {
        // The same reason a create may not touch one: Sync republishes its own
        // definitions whenever a project lacks them, so an edit here would be
        // silently corrected by the next open.
        if is_own_kind(kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is Sync's own type and is not the project's to redefine."),
                json!({"kind": kind}),
            ));
        }
        let stored = self.get_record(&type_key(kind))?.record.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is not a type this project holds."),
                json!({"kind": kind}),
            )
        })?;

        let mut definition = definition_of(&stored);
        definition.insert("kind_name".to_owned(), json!(kind));
        definition.insert("title".to_owned(), json!(title));
        definition.insert("description".to_owned(), json!(description));
        definition.insert("icon".to_owned(), json!(icon));

        let transaction = self.next_transaction_id("sync-type");
        self.apply(
            &transaction,
            &[type_record(kind, &Value::Object(definition))],
        )
    }

    /// Remove a type and everything written as it. Answers with how many records
    /// went with it.
    ///
    /// The records are not collateral damage; they are the point. The engine
    /// runs a strict schema, so a record whose kind has no definition is one
    /// nothing can read, write or validate — leaving them behind would leave the
    /// project holding claims it can no longer open. That is why the interface
    /// asking for this has to say the number out loud first.
    ///
    /// **One engine operation, not a batch of deletions.** Deleting a record
    /// takes its document with it wherever the project keeps it, so a type over
    /// an attached folder removed one record at a time would take the team's
    /// documentation with it. Removing a type is a different thing and the
    /// engine says so: the definition goes, its records go, the declaration
    /// pointing at the folder goes, and every file stays where the people who
    /// wrote it put it. Sync no longer walks the corpus to build the list —
    /// which records a type has is the engine's question, and the answer is a
    /// count it reports.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for one of Sync's own
    /// types — deleting `project` would leave the record naming the project with
    /// a kind the strict schema rejects.
    pub fn delete_type(&mut self, kind: &str) -> Result<usize> {
        if is_own_kind(kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is Sync's own type and cannot be removed."),
                json!({"kind": kind}),
            ));
        }

        let transaction = self.next_transaction_id("sync-type");
        let answer = self.call(
            "memory_delete_type",
            &json!({"kind": kind, "transaction_id": transaction}),
        )?;
        if let Some(revision) = answer.get("revision").and_then(Value::as_str) {
            revision.clone_into(&mut self.revision);
        }
        Ok(
            usize::try_from(answer.get("removed").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(0),
        )
    }

    /// The shape of the project's knowledge, and one page of the selection.
    ///
    /// `hidden` names the kinds this window is not showing — a view preference,
    /// not a fact about the project. They come out of the counts as well as out
    /// of the page, because a navigator that lists nine types beside a total
    /// counting eleven is arithmetic nobody can follow.
    ///
    /// One read per excluded kind, plus one for everything and one for the page.
    /// The engine has no "every kind except these" filter, so each exclusion is
    /// counted on its own and subtracted; the counting reads are `limit: 1`
    /// metadata reads asked for their counts rather than their records, and all
    /// of them are local round trips to a process already running.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn records(&mut self, selection: &Value, hidden: &[String]) -> Result<RecordsPage> {
        // Only what the person chose to hide. The schema is left out by the
        // engine itself — type definitions are counted as `service` and in none
        // of the other numbers — so subtracting them here would take them off a
        // total that never had them, and a project with eleven types would
        // report eleven fewer claims than it holds.
        let excluded: BTreeSet<&str> = hidden.iter().map(String::as_str).collect();

        let counting = json!({"limit": 1, "metadata_only": true});
        let everything = self.list_records(&counting)?;
        let mut excluded_counts = Vec::with_capacity(excluded.len());
        for kind in &excluded {
            let mut query = counting.clone();
            query["kind"] = json!(kind);
            excluded_counts.push(self.list_records(&query)?.counts);
        }
        let listing = self.list_records(&engine_query(selection))?;

        // What the caller will draw beside each row, named by it. Absent asks
        // for none, which is what every caller wanted before a column needed to
        // group by a field, and is why a row does not carry them by default:
        // the envelope arrives whole either way, but forwarding all of it would
        // put a type's every `text` field into a list that draws none of them.
        let wanted: BTreeSet<&str> = selection
            .get("fields")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        Ok(RecordsPage {
            revision: listing.revision,
            counts: RecordsCounts::excluding(&everything.counts, &excluded_counts),
            // A page of the whole corpus carries every kind with it, including
            // the ones not being shown. A page of one kind cannot, so this
            // filter only ever removes something from the unfiltered view.
            records: listing
                .records
                .iter()
                .filter_map(|stored| {
                    let mut row = RecordEntry::from_record(stored)?;
                    if !wanted.is_empty() {
                        let envelope = stored.get("envelope").unwrap_or(stored);
                        row.fields = product_fields(envelope);
                        row.fields.retain(|name, _| wanted.contains(name.as_str()));
                    }
                    Some(row)
                })
                .filter(|record| !excluded.contains(record.kind.as_str()))
                .collect(),
            has_more: listing.has_more,
        })
    }

    /// One record, whole, as the document view shows it.
    ///
    /// `None` when the key does not exist at this revision — a record that was
    /// deleted while the window had it open is an answer, not a failure.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn document(&mut self, key: &str) -> Result<Option<Document>> {
        let view = self.get_record(key)?;
        let Some(mut document) = view.record.as_ref().and_then(Document::from_record) else {
            return Ok(None);
        };
        // A record whose content is a file carries none of it, so the record
        // alone would open as a blank document. The body is a second read
        // because it is a second place — and it is the one read that can come
        // back saying there is nothing there, or that what is there is not
        // text at all.
        if document.is_reference() {
            let resolved = self.read_content(key)?;
            document.content_missing = resolved.missing;
            document.content_binary = !resolved.missing && !resolved.is_text();
            // The file's own answer wins over the record's: the record says
            // what the name implied when it was written, and this says what was
            // actually read a moment ago.
            if resolved.media_type.is_some() {
                document.media_type.clone_from(&resolved.media_type);
            }
            // Empty for anything that is not text, and deliberately so: base64
            // rendered as prose is not the document, and an editor opened on it
            // would write that string over the file on the first save.
            resolved.text().clone_into(&mut document.content);
        }
        Ok(Some(document))
    }

    /// Find what links to or mentions a key.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn backlinks(&mut self, key: &str) -> Result<Value> {
        self.call("memory_backlinks", &json!({"key": key}))
    }

    // ── Attached folders ────────────────────────────────────────────────────

    /// Attach a folder of the repository as a type, and settle what is in it.
    ///
    /// Two steps, and each is a different kind of statement. The type names the
    /// folder — "this directory of the working tree is where my documents are"
    /// — and the scan turns the files already there into records. A type
    /// without a scan is a project claiming a corpus while the documents sit on
    /// disk beside it.
    ///
    /// One folder per type: there is no mask any more, so **every** file in the
    /// folder is a document of the type that names it — images and PDFs
    /// included — and two types over one folder would both claim every new file
    /// in it.
    ///
    /// Nothing is written into the folder — not a marker, not an id in
    /// frontmatter. That is the promise the whole arrangement rests on, and it
    /// is the engine's to keep; what this method owes the interface is not
    /// quietly doing something else on the way.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_record` for a kind the
    /// project may not define and for a folder the engine refuses.
    pub fn attach_folder(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
        folder: &str,
    ) -> Result<ScanOutcome> {
        if is_own_kind(kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is Sync's own type and is always present."),
                json!({"kind": kind}),
            ));
        }
        check_identifier(kind)?;

        let transaction = self.next_transaction_id("sync-attach");
        self.apply(
            &transaction,
            &[type_definition(
                &TypeDeclaration::new(kind, title, description, icon)
                    .in_storage(TypeStorage::attached(folder)),
            )],
        )?;
        self.scan()
    }

    // ── Folders ─────────────────────────────────────────────────────────────

    /// The project's folders, from the records and from the working tree at
    /// once.
    ///
    /// Both sources, because neither answers alone. Aggregating the folders of
    /// known records is the whole answer for records kept in Git metadata,
    /// where a folder cannot exist unnamed. It is not the answer for an
    /// attached directory, which is on disk whatever Memory thinks: `docs/api/`
    /// may be empty, or hold only documents this branch hides. A person sees it
    /// in their file tree either way, and a navigator drawn from records alone
    /// would not.
    ///
    /// `folder` selects a region and `subtree` says whether it reaches below —
    /// the same pair a listing takes, so a tree asks for one level the same way
    /// it asks for one page.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn folders(
        &mut self,
        folder: Option<&str>,
        subtree: bool,
        kind: Option<&str>,
    ) -> Result<Vec<FolderEntry>> {
        let mut arguments = json!({"folder_scope": if subtree { "subtree" } else { "exact" }});
        if let Some(folder) = folder {
            arguments["folder"] = json!(folder);
        }
        if let Some(kind) = kind {
            arguments["kind"] = json!(kind);
        }
        let value = self.call("memory_list_folders", &arguments)?;
        // Read member by member rather than deserialised. The engine spells
        // these `in_records`, `in_storage` and `described_by`, and the window's
        // DTO spells them the way every other DTO does — so one `derive` would
        // have to satisfy both, and what it does instead is match neither and
        // fill in `false`. A folder every record is in, reported as a folder no
        // record is in, is the kind of wrong that draws a plausible tree.
        let folders = value
            .get("folders")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                Some(FolderEntry {
                    path: entry.get("path")?.as_str()?.to_owned(),
                    in_records: entry
                        .get("in_records")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    in_storage: entry
                        .get("in_storage")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    records: entry
                        .get("records")
                        .and_then(Value::as_u64)
                        .and_then(|count| usize::try_from(count).ok())
                        .unwrap_or(0),
                    described_by: entry
                        .get("described_by")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
            })
            .collect();
        Ok(folders)
    }

    /// Make a folder that nothing is in yet.
    ///
    /// The type decides what that means and the engine decides from the type:
    /// a directory for documents that are files, the record that carries
    /// `is_folder` for documents that are records. This layer passes the kind
    /// and does not branch on it — a window that asked where a type keeps its
    /// documents before offering "New Folder" would be reimplementing that
    /// decision in a second place, where it can differ.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` for a folder
    /// outside the type's storage and for one that is already there.
    pub fn create_folder(&mut self, folder: &str, kind: &str) -> Result<TransactionResult> {
        let transaction = self.next_transaction_id("sync-folder");
        let value = self.call(
            "memory_create_folder",
            &json!({"folder": folder, "kind": kind, "transaction_id": transaction}),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// The document that *is* a folder, opened or written.
    ///
    /// A folder is a name until somebody gives it something to say, and this is
    /// how they say it. The record is an ordinary document of an ordinary type
    /// — it is listed, searched, counted and linked to like any other, and the
    /// index has never heard of the flag that makes it a folder's — so
    /// describing a folder costs nothing anybody has to learn.
    ///
    /// Idempotent on purpose: a folder that already has such a record answers
    /// with it rather than writing a second one. Two of them is not a conflict
    /// to resolve later but a question with no answer — which of the two is the
    /// folder — asked of every client that draws a tree, and the engine refuses
    /// the write for the same reason.
    ///
    /// The same record whichever storage the type uses, and no file anywhere.
    /// What a folder says is not one of its type's documents, so it is not
    /// written as one — a repository gains nothing from somebody typing a note
    /// in this window. A person who wants a `README.md` in that folder writes
    /// one as an ordinary document, which is a different thing and theirs to
    /// decide.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for a folder outside
    /// the type's storage.
    pub fn describe_folder(&mut self, folder: &str, kind: &str) -> Result<Document> {
        if let Some(key) = self.description_of(folder)? {
            return self.document(&key)?.ok_or_else(|| {
                MemoryError::domain(
                    "invalid_record",
                    format!("`{key}` stands for this folder and could not be read."),
                    json!({"key": key, "folder": folder}),
                )
            });
        }

        // Filed in the folder, marked as the folder, and carrying its own text
        // — which is the whole of it. No file appears anywhere: what a folder
        // says is not one of its type's documents, and writing one into
        // somebody's repository because they typed a note is the thing
        // attaching a folder promises not to do. Somebody who wants a README
        // writes one as an ordinary document, and nothing here stops them.
        let title = folder.rsplit_once('/').map_or(folder, |(_, name)| name);
        let key = self.free_key(kind, title)?;
        let transaction = self.next_transaction_id("sync-folder");
        let put = json!({"op": "put", "record": {
            "representation": "plaintext",
            "envelope": {
                "envelope_version": {"major": 1, "minor": 0},
                "key": key,
                "kind": kind,
                "title": title,
                "content": "",
                "content_hash": content_hash(""),
                "tags": [],
                "links": [],
                "source_paths": {"observed": [], "scope": []},
                "archive": {"archived": false},
                "freshness": {"state": "unverified"},
                "folder": folder,
                "is_folder": true,
            },
        }});
        self.apply(&transaction, &[put])?;
        self.document(&key)?.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{key}` was written and could not be read back."),
                json!({"key": key}),
            )
        })
    }

    /// The key of the record that is this folder, if one is.
    fn description_of(&mut self, folder: &str) -> Result<Option<String>> {
        Ok(self
            .folders(Some(folder), false, None)?
            .into_iter()
            .find(|entry| entry.path == folder)
            .and_then(|entry| entry.described_by))
    }

    /// Take a folder and everything filed under it, and say how many went.
    ///
    /// Everything, whatever its type: a folder exists while something is in it,
    /// so sparing one type's records would empty the folder rather than delete
    /// it. Files go with their records; directories are removed only while they
    /// are empty, so a file no scan has reached is left where somebody put it.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` for a type's
    /// own storage root — removing that is removing the type.
    pub fn delete_folder(&mut self, folder: &str) -> Result<usize> {
        let transaction = self.next_transaction_id("sync-folder");
        let answer = self.call(
            "memory_delete_folder",
            &json!({"folder": folder, "transaction_id": transaction}),
        )?;
        self.refresh_revision()?;
        Ok(
            usize::try_from(answer.get("removed").and_then(Value::as_u64).unwrap_or(0))
                .unwrap_or(0),
        )
    }

    /// How many records a folder holds, at any depth and whatever their type.
    ///
    /// What a confirmation needs before it names a number it is about to
    /// destroy — and it is asked of the store rather than counted from the tree,
    /// which shows one type's and only the level it is on.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn folder_toll(&mut self, folder: &str) -> Result<usize> {
        let listing = self.list_records(&json!({
            "folder": folder,
            "folder_scope": "subtree",
            "include_folders": true,
            // Everything, because everything is what goes. The default listing
            // hides a record whose document another branch has — right for a
            // list somebody is reading, wrong for a number promising what is
            // about to be destroyed. The deletion takes those too, and a
            // confirmation that undercounted them would be the one sentence
            // here that must never be a guess.
            "presence": "any",
            "metadata_only": true,
            "limit": 1,
        }))?;
        Ok(listing.counts.total)
    }

    /// Rename a folder, moving every record filed under it at once.
    ///
    /// One engine transaction rather than a record at a time: N writes leave
    /// the folder half-renamed the moment one of them fails, and the record
    /// that stands for the folder is among the ones that can be left behind.
    ///
    /// Where the documents are files the engine renames the directory too, and
    /// the locators follow it. Sync does not touch the working tree itself —
    /// it asks the one writer to.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` for a type's
    /// own storage root, which is a change to the type rather than a rename.
    pub fn rename_folder(&mut self, from: &str, to: &str) -> Result<TransactionResult> {
        let transaction = self.next_transaction_id("sync-folder");
        let value = self.call(
            "memory_rename_folder",
            &json!({"from": from, "to": to, "transaction_id": transaction}),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// File one record in another folder.
    ///
    /// `folder` is where it goes; the empty string is the root. What happens
    /// underneath is the engine's business and deliberately not this layer's: a
    /// record carrying its own body moves by metadata, and a record whose body
    /// is a repository file has that file moved with it. Sync does not touch
    /// the working tree — it asks the one writer to.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` when a
    /// file-backed document is asked to leave its type's storage or when a
    /// document of that name is already at the destination.
    pub fn move_document(&mut self, key: &str, folder: &str) -> Result<TransactionResult> {
        let transaction = self.next_transaction_id("sync-move");
        let value = self.call(
            "memory_move_document",
            &json!({"key": key, "folder": folder, "transaction_id": transaction}),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// Reconcile every attached folder with the records, and say what moved.
    ///
    /// Cheap enough to run when a project opens and when the window regains
    /// focus, which is what the engine asks of a client that can see either:
    /// before every read is too expensive, and only at open is too rare for
    /// somebody editing files in the next window.
    ///
    /// A project with no attached folder scans nothing and answers with an
    /// empty report rather than a failure.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn scan(&mut self) -> Result<ScanOutcome> {
        let transaction = self.next_transaction_id("sync-scan");
        let value = self.call("memory_scan", &json!({"transaction_id": transaction}))?;
        let outcome: ScanOutcome = parse(value)?;
        // A scan that only found a file it cannot match writes nothing, so the
        // revision does not move and the subscription stays silent. Re-reading
        // the revision here keeps the client honest for the case where it did.
        self.refresh_revision()?;
        self.name_new_documents(&outcome)?;
        Ok(outcome)
    }

    /// Give each record the scan just made the title its document states.
    ///
    /// The engine leaves a new record untitled, because it reads no file to
    /// decide what a file is — that rule is what keeps a scan from having to
    /// open every document in the tree, and it is why the key is derived from
    /// the file name and nothing else. A client showing those records to a
    /// person can afford one read each, and the difference is between a list of
    /// documents and a list of file stems.
    ///
    /// Only records the scan created, and only where the record has no title
    /// and the document states one. A title already there is somebody's, and a
    /// heading that changes later is a heading — the record's name stays what
    /// it was called.
    fn name_new_documents(&mut self, outcome: &ScanOutcome) -> Result<()> {
        let created: Vec<String> = outcome
            .changes
            .iter()
            .filter(|change| change.get("change").and_then(Value::as_str) == Some("new"))
            .filter_map(|change| change.get("key").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        if created.is_empty() {
            return Ok(());
        }

        let mut operations = Vec::new();
        for key in created {
            if let Some(operation) = self.naming_of(&key)? {
                operations.push(operation);
            }
        }
        if operations.is_empty() {
            return Ok(());
        }

        // One transaction: every one of these records is of an attached type,
        // so they share a backend, and naming a folder of documents is one act
        // rather than forty commits.
        let transaction = self.next_transaction_id("sync-titles");
        self.apply(&transaction, &operations)?;
        Ok(())
    }

    /// A key nothing is stored under yet, starting from the one the scan would
    /// have derived.
    ///
    /// The engine does this for the records it creates and this has to match
    /// it, because the two write into the same corpus: a key derived from a
    /// file name can collide with a record somebody wrote by hand, and a `put`
    /// under an occupied key is not a new record — it is that record replaced.
    /// Suffixes rather than a fresh identifier, so the key still says which
    /// document it belongs to.
    ///
    /// Bounded for the same reason the engine bounds it: the ordinal counts
    /// documents whose names slug alike, and a folder with a thousand of them
    /// has a naming problem this cannot solve.
    fn free_reference_key(&mut self, base: &str) -> Result<String> {
        if self.get_record(base)?.record.is_none() {
            return Ok(base.to_owned());
        }
        for ordinal in 2..=MAX_KEY_ORDINAL {
            let candidate = format!("{base}-{ordinal}");
            if self.get_record(&candidate)?.record.is_none() {
                return Ok(candidate);
            }
        }
        Err(MemoryError::domain(
            "invalid_record",
            format!("no free key could be derived from `{base}`."),
            json!({"key": base}),
        ))
    }

    /// The write that would give one record the title its document states, or
    /// `None` when there is nothing to say: the record is named already, the
    /// document states no heading, or its file is not here to read.
    fn naming_of(&mut self, key: &str) -> Result<Option<Value>> {
        let Some(stored) = self.get_record(key)?.record else {
            return Ok(None);
        };
        let resolved = self.read_content(key)?;
        // A missing file states nothing, and neither does an image: there is no
        // mask any more, so a folder holds diagrams and PDFs beside the prose,
        // and reading a heading off base64 would name a record after its own
        // encoding. Both keep the key the scan derived, which says what the
        // file is called — which is the best anything can do for a document
        // that cannot be read.
        if resolved.missing || !resolved.is_text() {
            return Ok(None);
        }
        Ok(titled_put(&stored, resolved.text()))
    }

    /// Read a record's body, following its locator when it has one.
    ///
    /// The one read that goes outside, and the only one that can answer that
    /// there is nothing there. Everything else — listing, search, counts —
    /// works from what Memory holds, so an unreachable folder can never make
    /// one of them quietly return less.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn read_content(&mut self, key: &str) -> Result<ContentView> {
        let value = self.call("memory_read_content", &json!({"key": key}))?;
        parse(value)
    }

    /// Write the body of a record whose content is a repository file.
    ///
    /// The engine writes the file first and the record second, so an
    /// interruption leaves a disagreement the next scan settles rather than a
    /// record pointing at content that was never written.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` for a record
    /// that keeps its content — that one is written as a record.
    pub fn write_content(&mut self, key: &str, content: &str) -> Result<TransactionResult> {
        self.write_content_as(key, content, "utf-8")
    }

    /// Write a body the engine should read as something other than text.
    ///
    /// `base64` is the one other spelling, and it is how a picture reaches the
    /// working tree: the bytes are read in the window, travel as text because
    /// the protocol is JSON, and are decoded by the engine before the file is
    /// written. Nothing here decodes anything — the window has no filesystem
    /// and the engine owns what a locator means.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn write_content_as(
        &mut self,
        key: &str,
        content: &str,
        encoding: &str,
    ) -> Result<TransactionResult> {
        let transaction = self.next_transaction_id("sync-content");
        let value = self.call(
            "memory_write_content",
            &json!({
                "key": key,
                "content": content,
                "encoding": encoding,
                "transaction_id": transaction,
            }),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// Settle a file the scan could not attribute to a record.
    ///
    /// This is the one step of attaching a folder that cannot be automated. A
    /// file renamed and edited in the same stroke matches no record by path and
    /// none by bytes, and neither does a genuinely new file — nothing about the
    /// file says which of the two it is. Guessing means either losing a
    /// document's history or merging two unrelated documents, so the engine
    /// ranks the records it could be and stops, and this writes the answer a
    /// person gave.
    ///
    /// `adopt` names the record the file turned out to be, keeping its key and
    /// therefore every link pointing at it. `None` says it is a document in its
    /// own right, and a record is written for it under a key derived the way
    /// the scan derives one.
    ///
    /// `content_hash` is the digest the scan reported for the file. It is
    /// carried rather than computed because this layer never reads the working
    /// tree: the engine owns the folder, and a digest invented here would be a
    /// claim about bytes nothing in this process has seen.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` when the record being
    /// adopted is not one this project holds or does not keep its content in a
    /// file.
    pub fn resolve_unmatched(
        &mut self,
        locator: &str,
        content_hash: &str,
        kind: &str,
        adopt: Option<&str>,
    ) -> Result<TransactionResult> {
        // The key the record ends up under: the adopted record's own, or one
        // derived from the file name that nothing is stored under yet.
        let key;
        let operation = if let Some(adopted) = adopt {
            key = adopted.to_owned();
            let stored = self.get_record(adopted)?.record.ok_or_else(|| {
                MemoryError::domain(
                    "invalid_record",
                    format!("`{adopted}` is not a record this project holds."),
                    json!({"key": adopted}),
                )
            })?;
            if stored
                .get("envelope")
                .unwrap_or(&stored)
                .get("content_ref")
                .is_none()
            {
                return Err(MemoryError::domain(
                    "invalid_record",
                    format!("`{adopted}` keeps its own content, so no file can be its document."),
                    json!({"key": adopted}),
                ));
            }
            relocate_put(&stored, locator, content_hash).ok_or_else(|| {
                MemoryError::domain(
                    "invalid_record",
                    format!("`{adopted}` is stored in a shape this build cannot rewrite."),
                    json!({"key": adopted}),
                )
            })?
        } else {
            // The folder only decides how much of the locator the key keeps:
            // `guides/api/auth.md` under `guides` is `api-auth`, and the same
            // file under an unknown folder is `guides-api-auth`. A type whose
            // folder nothing here can name is still adoptable — a key that says
            // a little more about where the file is beats refusing to file it.
            let folder = self
                .list_types()?
                .into_iter()
                .find(|type_| type_.kind == kind)
                .and_then(|type_| type_.storage.folder)
                .unwrap_or_default();
            key = self.free_reference_key(&reference_key(&folder, locator))?;
            reference_put(&key, kind, locator, content_hash)
        };

        let transaction = self.next_transaction_id("sync-adopt");
        let result = self.apply(&transaction, &[operation])?;

        // A record written for a file nobody had claimed is a new document, and
        // it gets the name its text states — the same courtesy the scan does
        // for the ones it created on its own. A record that was adopted keeps
        // the name it already had, which `naming_of` decides for itself.
        if let Some(naming) = self.naming_of(&key)? {
            let transaction = self.next_transaction_id("sync-title");
            return self.apply(&transaction, &[naming]);
        }
        Ok(result)
    }

    // ── Writes ──────────────────────────────────────────────────────────────

    /// Change what a patch names in one record, and leave the rest of it alone.
    ///
    /// This is the same shape as [`update_type`](Self::update_type) and for the
    /// same reason: what is written is the stored record with the patch applied,
    /// never a record rebuilt from it. A claim carries scope paths, tags, links,
    /// an archive flag and the product fields its type declares — and whatever a
    /// newer engine added — so anything the patch is silent about is read back
    /// rather than replaced.
    ///
    /// One transaction per save. The engine refuses a reused id, and
    /// [`apply`](Self::apply) already replays a stale-revision conflict once, so
    /// the ordinary cost of reading a record, waiting for a person to type, then
    /// writing it back is paid there rather than reported here.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for a key the project no
    /// longer holds, for a record with no envelope this build can rewrite, for a
    /// type definition — which is edited as a type, not as prose — and for a
    /// patch whose product fields would overwrite an envelope member.
    pub fn update_document(
        &mut self,
        key: &str,
        edits: &DocumentEdits,
    ) -> Result<TransactionResult> {
        let colliding = edits.colliding_fields();
        if !colliding.is_empty() {
            return Err(MemoryError::domain(
                "invalid_record",
                format!(
                    "`{}` names the envelope's own members, not product fields.",
                    colliding.join("`, `")
                ),
                json!({"key": key, "fields": colliding}),
            ));
        }

        let stored = self.get_record(key)?.record.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{key}` is not a record this project holds."),
                json!({"key": key}),
            )
        })?;

        let kind = RecordEntry::from_record(&stored)
            .map(|record| record.kind)
            .ok_or_else(|| {
                MemoryError::domain(
                    "invalid_record",
                    format!("`{key}` has no kind, so nothing can validate a write of it."),
                    json!({"key": key}),
                )
            })?;
        if is_definition_kind(&kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{key}` is a type definition, which is edited as a type."),
                json!({"key": key, "kind": kind}),
            ));
        }

        let operation = document_put(&stored, edits).ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{key}` is stored in a shape this build cannot rewrite."),
                json!({"key": key}),
            )
        })?;

        // A record whose content is a repository file is two writes, because it
        // is two places: the file, and the record beside it. The body goes
        // first — the engine writes the file before the record that points at
        // it, so an interruption leaves a disagreement the next scan settles
        // rather than a record claiming a text that was never written.
        //
        // A record that is missing its file is not written to at all. Saving a
        // draft over a document another branch holds would create the file
        // here and quietly fork it, which is not what somebody typing into an
        // open editor is asking for.
        let reference = stored
            .get("envelope")
            .unwrap_or(&stored)
            .get("content_ref")
            .is_some();
        if let Some(content) = edits.content.as_ref().filter(|_| reference) {
            let presence = RecordEntry::from_record(&stored)
                .map_or_else(|| PRESENT.to_owned(), |record| record.presence);
            if presence != PRESENT {
                return Err(MemoryError::domain(
                    "content_absent",
                    format!(
                        "`{key}` has no file here to write to: {}.",
                        absence_reason(&presence)
                    ),
                    json!({"key": key, "presence": presence}),
                ));
            }
            self.write_content(key, content)?;
        }

        let transaction = self.next_transaction_id("sync-doc");
        self.apply(&transaction, &[operation])
    }

    /// Create an empty record of a kind the project holds, and answer with it.
    ///
    /// The kind has to be one of the project's own types, so the definition is
    /// read first: it decides which fields the new record must carry, and a
    /// record missing a required one is a record the strict schema rejects. What
    /// this build contributes is a key and a title.
    ///
    /// The key is generated and then checked, because a key is identity: a
    /// collision would not be a new record but an overwrite of somebody else's.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for a kind the project
    /// does not hold, for one that is not created as a document — the schema and
    /// the project's own record — and when no free key could be found.
    pub fn create_document(
        &mut self,
        kind: &str,
        title: &str,
        in_folder: Option<&str>,
    ) -> Result<Document> {
        if is_fixed_record(kind) {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is not a kind records are created in."),
                json!({"kind": kind}),
            ));
        }

        let stored = self.get_record(&type_key(kind))?.record.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is not a type this project holds."),
                json!({"kind": kind}),
            )
        })?;
        let definition = Value::Object(definition_of(&stored));
        let storage = TypeStorage::of_definition(&definition);

        let key = if storage.is_attached() {
            self.create_document_file(kind, &document_stem(title), &storage, in_folder)?
        } else {
            let key = self.free_key(kind, title)?;
            let transaction = self.next_transaction_id("sync-doc");
            let mut put = new_document_put(kind, &key, title, &definition);
            // Filed as it is written rather than written and then moved. A
            // record that appeared at the root for a moment is a record every
            // other window saw there.
            if let Some(folder) = in_folder.filter(|folder| !folder.is_empty()) {
                put["record"]["envelope"]["folder"] = json!(folder);
            }
            self.apply(&transaction, &[put])?;
            key
        };

        self.document(&key)?.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{key}` was written and could not be read back."),
                json!({"key": key}),
            )
        })
    }

    /// Put a file into a type's storage, and answer with the record that names
    /// it.
    ///
    /// The one route by which something that is not text reaches the working
    /// tree. It is the same two writes as any other document — the record that
    /// names a file, then the file — with two differences: the name is given
    /// rather than derived from a title, because a picture arrives with one or
    /// with none at all, and the bytes are handed over as base64.
    ///
    /// The file goes in the **root of the storage**, never in a folder invented
    /// for it. Where a project keeps its pictures is the project's arrangement,
    /// and an application that quietly created `assets/` would be making that
    /// arrangement on the team's behalf, in their repository, in their diff.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for a kind whose
    /// documents are not files — there is no storage to put one in.
    pub fn create_file_document(
        &mut self,
        kind: &str,
        name: &str,
        content_base64: &str,
    ) -> Result<Document> {
        let stored = self.get_record(&type_key(kind))?.record.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{kind}` is not a type this project holds."),
                json!({"kind": kind}),
            )
        })?;
        let definition = Value::Object(definition_of(&stored));
        let storage = TypeStorage::of_definition(&definition);
        if !storage.is_attached() {
            return Err(MemoryError::domain(
                "invalid_record",
                format!(
                    "`{kind}` keeps its documents in records, so there is nowhere to put a file."
                ),
                json!({"kind": kind}),
            ));
        }

        self.scan()?;
        let folder = Self::folder_of(kind, &storage)?;
        let (stem, extension) = split_file_name(name);

        let taken = self.locators_of(kind)?;
        let mut locator = String::new();
        for ordinal in 1..=MAX_KEY_ORDINAL {
            let candidate_name = if ordinal == 1 {
                format!("{stem}{extension}")
            } else {
                format!("{stem}-{ordinal}{extension}")
            };
            let candidate = format!("{folder}/{candidate_name}");
            if !taken.contains(&candidate) {
                locator = candidate;
                break;
            }
        }
        if locator.is_empty() {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("no free file name could be derived in `{folder}`."),
                json!({"kind": kind, "folder": folder}),
            ));
        }

        let key = self.free_reference_key(&reference_key(&folder, &locator))?;
        let transaction = self.next_transaction_id("sync-file");
        // The digest is of the bytes the file is about to hold, so an
        // interruption between the record and the file leaves the two agreeing
        // rather than a disagreement the next scan has to settle.
        let bytes = BASE64.decode(content_base64.as_bytes()).map_err(|_| {
            MemoryError::domain(
                "invalid_record",
                "the file's bytes could not be read.".to_owned(),
                json!({"name": name}),
            )
        })?;
        self.apply(
            &transaction,
            &[reference_put(&key, kind, &locator, &bytes_hash(&bytes))],
        )?;
        self.write_content_as(&key, content_base64, "base64")?;

        self.document(&key)?.ok_or_else(|| {
            MemoryError::domain(
                "invalid_record",
                format!("`{key}` was written and could not be read back."),
                json!({"key": key}),
            )
        })
    }

    /// Write a new document into an attached folder, and answer with its key.
    ///
    /// A record of such a type points at its content rather than carrying it —
    /// the store refuses one that does not — so creating a document here is two
    /// writes: the record that names a file, then the file itself. That order
    /// is forced rather than chosen, because the engine writes a document
    /// through the record that points at it. Interrupted between them, the
    /// record is one whose file is missing, which is a state the window already
    /// draws and the next scan already explains.
    ///
    /// The file is named for the document, not for the record: `untitled.md`
    /// until somebody titles it, and never renamed afterwards. Renaming a file
    /// is something a person does in their editor, and doing it on their behalf
    /// because a title changed would move a file under a colleague's open
    /// branch.
    ///
    /// A scan comes first so the choice of name is made against what is
    /// actually in the folder: a file nobody has scanned yet has no record, and
    /// naming a new document over the top of it would write into somebody's
    /// text.
    fn create_document_file(
        &mut self,
        kind: &str,
        stem: &str,
        storage: &TypeStorage,
        in_folder: Option<&str>,
    ) -> Result<String> {
        self.scan()?;
        let root = Self::folder_of(kind, storage)?;
        // A directory the person picked, or the storage's own root when they
        // picked nothing. Checked rather than trusted: a document outside its
        // type's storage is not a document of that type, and the engine would
        // refuse the write afterwards with a message about a locator.
        let folder = match in_folder.filter(|folder| !folder.is_empty()) {
            None => root.clone(),
            Some(folder) if folder == root || folder.starts_with(&format!("{root}/")) => {
                folder.to_owned()
            }
            Some(folder) => {
                return Err(MemoryError::domain(
                    "invalid_record",
                    format!("`{folder}` is not inside `{root}`, where this type's documents live."),
                    json!({"kind": kind, "folder": folder}),
                ));
            }
        };
        let taken = self.locators_of(kind)?;
        let mut locator = String::new();
        for ordinal in 1..=MAX_KEY_ORDINAL {
            let name = if ordinal == 1 {
                format!("{stem}{DOCUMENT_EXTENSION}")
            } else {
                format!("{stem}-{ordinal}{DOCUMENT_EXTENSION}")
            };
            let candidate = format!("{folder}/{name}");
            if !taken.contains(&candidate) {
                locator = candidate;
                break;
            }
        }
        if locator.is_empty() {
            return Err(MemoryError::domain(
                "invalid_record",
                format!("no free file name could be derived in `{folder}`."),
                json!({"kind": kind, "folder": folder}),
            ));
        }

        // Derived against the storage root, never against the folder the
        // document is going into: that is what the scan does, and it is the
        // whole reason nested documents get `guides-api-auth` rather than three
        // records all wanting to be `auth`. Deriving it from the chosen folder
        // would make every folder's `intro.md` want the key `intro`.
        let key = self.free_reference_key(&reference_key(&root, &locator))?;
        let transaction = self.next_transaction_id("sync-doc");
        self.apply(
            &transaction,
            &[reference_put(
                &key,
                kind,
                &locator,
                &content_hash(NEW_DOCUMENT_BODY),
            )],
        )?;
        // What puts the file on disk. The engine creates the directories it
        // needs, so a folder that exists only in the type definition is made
        // here rather than by Sync reaching into the working tree. The digest
        // above is of the same body, so an interruption between the two leaves
        // a record that agrees with the file rather than one the next scan has
        // to correct.
        self.write_content(&key, NEW_DOCUMENT_BODY)?;
        Ok(key)
    }

    /// The directory an attached type's documents live in.
    ///
    /// Read from the definition, which is where the path is: a type says the
    /// folder outright, so nothing has to be resolved and nothing can point at
    /// a place the project no longer knows about. A type that says nothing is
    /// one whose documents are its records, and asking this about it is a
    /// caller that skipped [`TypeStorage::is_attached`].
    fn folder_of(kind: &str, storage: &TypeStorage) -> Result<String> {
        storage
            .folder
            .as_deref()
            .filter(|folder| !folder.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                MemoryError::domain(
                    "invalid_record",
                    format!("`{kind}` keeps its documents in its records, not in a folder."),
                    json!({"kind": kind}),
                )
            })
    }

    /// The files the records of one kind already point at.
    fn locators_of(&mut self, kind: &str) -> Result<BTreeSet<String>> {
        let mut locators = BTreeSet::new();
        let mut offset = 0;
        loop {
            let page = self.list_records(&json!({
                "kind": kind,
                "metadata_only": true,
                "presence": "any",
                "limit": TYPE_PAGE,
                "offset": offset,
            }))?;
            if page.records.is_empty() {
                break;
            }
            offset += page.records.len();
            locators.extend(
                page.records
                    .iter()
                    .filter_map(RecordEntry::from_record)
                    .filter_map(|record| record.locator),
            );
            if !page.has_more {
                break;
            }
        }
        Ok(locators)
    }

    /// A key of the corpus's usual shape that nothing is stored under yet.
    ///
    /// Eight attempts, each seeded differently. A ninth would be a store holding
    /// so many keys of one kind that the interface has a different problem, and
    /// answering with a failure is better than looping in a window.
    fn free_key(&mut self, kind: &str, title: &str) -> Result<String> {
        for attempt in 0..8 {
            let key = suggested_key(kind, &format!("{}:{attempt}:{title}", self.revision));
            if self.get_record(&key)?.record.is_none() {
                return Ok(key);
            }
        }
        Err(MemoryError::domain(
            "invalid_record",
            format!("no free key could be generated for a `{kind}`."),
            json!({"kind": kind}),
        ))
    }

    /// The delete operations for a set of keys, with the refusals they carry.
    ///
    /// Extracted because two callers need the same rule and neither may be the
    /// one that skips it: a record Sync will not delete is not one it deletes
    /// on an agent's word either. A key the store no longer holds is silently
    /// no operation — deleting what is already gone is what the caller asked
    /// for.
    ///
    /// # Errors
    ///
    /// Returns `invalid_record` for a key that is a type definition, which goes
    /// with its type, or the record that names the project.
    fn delete_operations(&mut self, keys: &[String]) -> Result<Vec<Value>> {
        let mut operations = Vec::new();
        for key in keys {
            let Some(stored) = self.get_record(key)?.record else {
                continue;
            };
            let kind = RecordEntry::from_record(&stored)
                .map(|record| record.kind)
                .unwrap_or_default();
            if is_fixed_record(&kind) {
                return Err(MemoryError::domain(
                    "invalid_record",
                    format!("`{key}` is not a record Sync deletes."),
                    json!({"key": key, "kind": kind}),
                ));
            }
            operations.push(delete(key));
        }
        Ok(operations)
    }

    /// Save and delete records in one transaction.
    ///
    /// The product-level write, and the one an agent is given. What it hides is
    /// what a caller cannot get right from outside: the transaction id, the
    /// revision the write expects, the envelope's own version and the digest of
    /// its content. An agent handing over raw envelopes would be computing a
    /// content hash to satisfy a format it has no stake in — and getting it
    /// wrong is a refusal it cannot read.
    ///
    /// Both halves in one transaction, because deleting a record together with
    /// the ones that referred to it is one decision, and half of it applied is
    /// a corpus in a state nobody chose.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, `invalid_record` for a key Sync will not
    /// delete, and a conflict the replay could not settle.
    pub fn apply_entities(
        &mut self,
        save: Vec<EntityInput>,
        remove: &[String],
    ) -> Result<TransactionResult> {
        if save.is_empty() && remove.is_empty() {
            return Err(MemoryError::domain(
                "invalid_argument",
                "nothing was named to save or delete.".to_owned(),
                Value::Null,
            ));
        }
        let mut operations = self.delete_operations(remove)?;
        operations.extend(save.into_iter().map(|input| Entity::from(input).to_put()));
        // Something was named and none of it is there: every key was already
        // gone, which is the state the caller asked for. Said as the no-op it
        // is rather than as "you named nothing", which would be untrue and
        // would send somebody looking for a mistake in their own arguments.
        if operations.is_empty() {
            return Ok(TransactionResult {
                revision: self.revision.clone(),
                changed_keys: Vec::new(),
            });
        }
        let transaction = self.next_transaction_id("agent");
        self.apply(&transaction, &operations)
    }

    /// Delete records by key, in one transaction.
    ///
    /// All of them or none: a caller deleting a record together with the ones
    /// that depend on it is describing one decision, and half of it applied is a
    /// corpus in a state nobody chose.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` when any of the keys is
    /// one the window may not delete — a type definition, which goes with its
    /// type, or the record that names the project.
    pub fn delete_documents(&mut self, keys: &[String]) -> Result<TransactionResult> {
        if keys.is_empty() {
            return Err(MemoryError::domain(
                "invalid_record",
                "nothing was named to delete.".to_owned(),
                Value::Null,
            ));
        }

        // One transaction, whatever the keys are. A record lives in the one
        // storage this project keeps records in — a type naming a storage puts
        // its *documents* elsewhere, never its envelopes — so deleting a
        // decision together with the document it is about is one write, and
        // atomic in the way the person who asked for it assumed.
        let operations = self.delete_operations(keys)?;
        // Every key was a record the store no longer holds. Nothing to write,
        // and the revision is the one already in hand.
        if operations.is_empty() {
            return Ok(TransactionResult {
                revision: self.revision.clone(),
                changed_keys: Vec::new(),
            });
        }

        let transaction = self.next_transaction_id("sync-doc");
        self.apply(&transaction, &operations)
    }

    /// What holds on to a record, split by how it holds on.
    ///
    /// The distinction is the whole point. A record in `links` is a structural
    /// dependency: delete the target and the link points at nothing. A record
    /// that names the key in its prose is a sentence about it, and deleting the
    /// sentence's author because it mentioned something is deleting the reasoning
    /// along with the conclusion.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn dependents(&mut self, key: &str) -> Result<Dependents> {
        let answer = self.backlinks(key)?;
        let mut links = Vec::new();
        let mut mentions = Vec::new();

        for entry in answer
            .get("backlinks")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(source) = entry.get("source_id").and_then(Value::as_str) else {
                continue;
            };
            let dependent = Dependent {
                key: source.to_owned(),
                kind: entry
                    .get("source_kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                title: entry
                    .get("source_title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                relation: entry
                    .get("relation")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            };
            // The engine spells this `explicit_link` or `body_mention`. The
            // underscores are dropped before comparing because the spelling is
            // the engine's to change and the distinction is not; anything a newer
            // engine reports that is not a link is treated as a mention, which is
            // the cautious half — a mention is never deleted on a record's
            // behalf.
            if entry
                .get("mention_type")
                .and_then(Value::as_str)
                .is_some_and(|mention| {
                    mention
                        .replace(['_', '-'], "")
                        .eq_ignore_ascii_case("explicitlink")
                })
            {
                links.push(dependent);
            } else {
                mentions.push(dependent);
            }
        }

        Ok(Dependents { links, mentions })
    }

    /// Apply a batch of operations atomically.
    ///
    /// On a same-key conflict this refreshes the revision and replays once: a
    /// stale `expected_revision` is the ordinary cost of a UI that reads, waits
    /// for a person, then writes. A second conflict is a real one — someone
    /// else is editing the same record — and is surfaced with both revisions so
    /// the UI can say who and what.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn apply(
        &mut self,
        transaction_id: &str,
        operations: &[Value],
    ) -> Result<TransactionResult> {
        match self.apply_once(transaction_id, operations) {
            Err(error) if error.is_retryable_conflict() => {
                self.refresh_revision()?;
                // A fresh id: the engine refuses a reused one, and this is a
                // new attempt rather than a repeat of the same write.
                let retry_id = format!("{transaction_id}-retry");
                self.apply_once(&retry_id, operations)
            }
            other => other,
        }
    }

    fn apply_once(
        &mut self,
        transaction_id: &str,
        operations: &[Value],
    ) -> Result<TransactionResult> {
        let value = self.call(
            "memory_apply_transaction",
            &json!({
                "transaction_id": transaction_id,
                "expected_revision": self.revision,
                "operations": operations,
            }),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// Import a bundle in one transaction.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn import(&mut self, transaction_id: &str, bundle: &Value) -> Result<TransactionResult> {
        let value = self.call(
            "memory_import",
            &json!({
                "transaction_id": transaction_id,
                "expected_revision": self.revision,
                "bundle": bundle,
            }),
        )?;
        let result: TransactionResult = parse(value)?;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }

    /// Export the current revision as a bundle.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn export(&mut self) -> Result<Value> {
        let revision = self.revision.clone();
        self.call("memory_export", &json!({"revision": revision}))
    }

    // ── Status ──────────────────────────────────────────────────────────────

    /// Whether search runs hybrid or FTS-only, and which model is active.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn model_status(&mut self) -> Result<ModelStatus> {
        let value = self.call("memory_model_status", &json!({}))?;
        parse(value)
    }

    /// Whether every record satisfies the type published for its kind.
    ///
    /// The engine runs a strict schema, so this is a gate rather than a report:
    /// an incompatible record means Sync wrote something its own type
    /// definition forbids.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn schema_status(&mut self) -> Result<Value> {
        self.call("memory_schema_status", &json!({}))
    }

    /// Remote configuration for memory, which is separate from code `origin`.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn transport_status(&mut self) -> Result<TransportStatus> {
        let value = self.call("memory_transport_status", &json!({}))?;
        parse(value)
    }

    /// Whether the project's memory is in step with its remote.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn sync_state(&mut self, ask_remote: bool) -> Result<SyncState> {
        let value = self.call("memory_sync_state", &json!({"ask_remote": ask_remote}))?;
        parse(value)
    }

    /// Put memory back where it stood, undoing what has happened since.
    ///
    /// Every record may have changed, so the revision this window holds is
    /// re-read rather than reasoned about: whoever is showing a list has to ask
    /// for it again, and the revision moving is what tells them to.
    ///
    /// # Errors
    ///
    /// Returns the engine failure: `invalid_argument` for a revision this
    /// memory never passed through, and `conflict` when something has been
    /// written since.
    pub fn rewind(&mut self, revision: &str, expected: &str) -> Result<()> {
        self.call(
            "memory_rewind",
            &json!({"revision": revision, "expected_revision": expected}),
        )?;
        self.refresh_revision()?;
        Ok(())
    }

    /// Whether this repository's memory is here, still on a remote, or
    /// nowhere yet.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn presence(&mut self) -> Result<MemoryPresence> {
        let value = self.call("memory_presence", &json!({}))?;
        parse(value)
    }

    /// Configure the memory remote, which is separate from the code `origin`.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including a URL Git would read as an option
    /// or as a remote helper.
    pub fn set_remote(&mut self, url: &str, refspec: Option<&str>) -> Result<TransportStatus> {
        let mut arguments = json!({"url": url});
        if let Some(refspec) = refspec {
            arguments["refspec"] = json!(refspec);
        }
        let value = self.call("memory_remote_set", &arguments)?;
        parse(value)
    }

    /// Forget the memory remote.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn remove_remote(&mut self) -> Result<TransportStatus> {
        let value = self.call("memory_remote_remove", &json!({}))?;
        parse(value)
    }

    // ── Transport ───────────────────────────────────────────────────────────

    /// Fetch memory from the remote and merge it.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn fetch(&mut self) -> Result<Value> {
        let result = self.call("memory_fetch", &json!({}))?;
        self.refresh_revision()?;
        Ok(result)
    }

    /// Push memory to the remote.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `push_blocked` from the
    /// stale-record policy.
    pub fn push(&mut self, force: bool) -> Result<Value> {
        self.call("memory_push", &json!({"force": force}))
    }

    /// Rebuild the search index.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn reindex(&mut self) -> Result<Value> {
        self.call("memory_reindex", &json!({}))
    }

    /// Catch memory up with code history.
    ///
    /// The engine reconciles before every mutation on its own, so this exists
    /// for the one case that reconciliation refuses to settle by itself: code
    /// history that was rewritten — a rebase, a reset, a branch replaced —
    /// leaves the cursor on a commit HEAD no longer descends from, and every
    /// write is refused with `diverged` until somebody says what to do about
    /// it. `full_rebuild` is that answer, and it is deliberately the caller's
    /// to give: it marks every record unverified, because a claim checked
    /// against a history that no longer exists has not been checked.
    ///
    /// Nothing is lost by it. What a record says is untouched; what changes is
    /// how far it may be trusted before somebody looks again.
    ///
    /// The revision is re-read afterwards because a rebuild moves it, and the
    /// write this is clearing the way for carries the one we hold.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `diverged` when the history moved
    /// and `full_rebuild` was not asked for.
    pub fn reconcile(&mut self, full_rebuild: bool) -> Result<Value> {
        let divergence = if full_rebuild {
            "full_rebuild"
        } else {
            "report"
        };
        let report = self.call("memory_reconcile", &json!({"divergence": divergence}))?;
        self.refresh_revision()?;
        Ok(report)
    }

    /// Create or update entities in one transaction.
    ///
    /// The envelopes are built here rather than by the caller, which is the
    /// whole of why this operation exists: an envelope is the store's shape,
    /// and a window that assembles one has to be kept in step with a format it
    /// does not own.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including a conflict the replay could not
    /// settle.
    pub fn save_entities(&mut self, entities: Vec<EntityInput>) -> Result<TransactionResult> {
        let operations: Vec<Value> = entities
            .into_iter()
            .map(|input| Entity::from(input).to_put())
            .collect();
        let transaction = self.next_transaction_id("sync-entities");
        self.apply(&transaction, &operations)
    }

    // ── The project's own record ────────────────────────────────────────────

    /// Read the project's own record of what it is called.
    ///
    /// This is what decides whether the opening flow asks anything at all: a
    /// repository whose memory already carries a project record has been opened
    /// before, and re-asking would be the application forgetting rather than
    /// the person changing their mind.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn project_settings(&mut self) -> Result<Option<ProjectSettings>> {
        let Some(key) = self.project_key()? else {
            return Ok(None);
        };
        // Read by key now that there is one. A listing carries what a listing
        // is for — the envelope's own columns — and the project's language and
        // extensions are product fields, which only the record itself has.
        Ok(self
            .get_record(&key)?
            .record
            .as_ref()
            .and_then(|record| settings_from_record(record, &key)))
    }

    /// The project's own record, as the engine holds it, or `None` when this
    /// repository has never been described.
    ///
    /// Found by kind rather than by key. The key is the project's identifier,
    /// and the identifier is exactly what a read of this record is for — asking
    /// for it by key would mean already knowing the answer.
    ///
    /// Two of them is not a project with a choice to make: the record is what
    /// names the project, and a repository that answers twice would open under
    /// whichever name the listing happened to put first.
    fn project_key(&mut self) -> Result<Option<String>> {
        let listing = self.list_records(&json!({
            "kind": EntityKind::Project.as_str(),
            "limit": 2,
        }))?;
        if listing.records.len() > 1 {
            return Err(MemoryError::domain(
                "several_project_records",
                "This repository holds more than one project record, so there is no \
                 single answer to what it is called."
                    .to_owned(),
                json!({"count": listing.total}),
            ));
        }
        Ok(listing
            .records
            .first()
            .and_then(|record| record.get("envelope").unwrap_or(record).get("key"))
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    /// Write the project's record, creating the project's memory if this is its
    /// first write.
    ///
    /// One type definition travels in the same transaction: `project`. The
    /// engine runs a strict schema and rejects a record whose kind it has no
    /// definition for, so on a repository that has never held memory that
    /// definition has to land no later than the record it describes — and one
    /// transaction is what makes this all-or-nothing rather than a half-created
    /// project.
    ///
    /// That is also why this is one operation rather than the two the move was
    /// sketched as. Seeding the types and writing the record are not two things
    /// a caller may order; they are one write that must not be interrupted.
    ///
    /// Nothing else is published. A new project knows what it is called and
    /// nothing about what it may say; the types it works in are created in the
    /// window or by an agent, when there is something to say in them.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    pub fn update_project(&mut self, settings: &ProjectSettings) -> Result<TransactionResult> {
        let mut operations = own_type_definitions();
        operations.push(project_record(settings).to_put());
        // A fresh id per attempt: the engine refuses a reused one, so a fixed
        // id would fail the second time a project is described in the same
        // session — which is what a person does when they open a folder, go
        // back, and open it again.
        let transaction = self.next_transaction_id("sync-project");
        self.apply(&transaction, &operations)
    }

    // ── Plumbing ────────────────────────────────────────────────────────────
}

/// A listing selection in the engine's own spelling.
///
/// The window's selection is handed to the engine as it stands, which works
/// because every member it can carry — `kind`, `freshness`, `folder`, `limit`,
/// `offset` — is spelled the same on both sides. `folderScope` is the one that
/// is not, and a key the engine does not recognise is *ignored* rather than
/// refused: the filter would quietly stop applying and a tree would show a
/// folder's whole subtree as though it were its contents.
///
/// So the translation is here, in the layer that knows what the engine reads,
/// rather than in the window spelling a `snake_case` member among `camelCase`
/// ones that somebody would eventually tidy up.
fn engine_query(selection: &Value) -> Value {
    let mut query = selection.clone();
    if let Some(object) = query.as_object_mut() {
        // Which fields the caller will draw is decided here, over the answer.
        // Sent on, it would be a member the engine does not model in a query it
        // validates — and a listing refused because a column named a field is a
        // failure with no useful sentence in it.
        object.remove("fields");
        if let Some(scope) = object.remove("folderScope") {
            object.insert("folder_scope".to_owned(), scope);
        }
    }
    query
}

/// Whether a string can be a kind at all.
///
/// The window derives an identifier from a name and generates one where a name
/// cannot be reduced, so nothing legitimate arrives here empty or with a space
/// in it. That is exactly why it is checked: a caller that gets this wrong would
/// otherwise write `__type__/` — a definition addressed by a key with nothing in
/// it, describing every record of a kind nobody can name.
///
/// The kind the corpus itself is written in is refused for the same reason: a
/// type called `__type__` would be a definition of the thing definitions are
/// stored as, and every read of the corpus would list it beside the types it
/// describes.
///
/// Nothing else about the shape is ruled on. An extension prefixes the kinds it
/// brings, and inventing a namespace grammar here would settle that question
/// years before the first extension exists.
fn check_identifier(kind: &str) -> Result<()> {
    if kind.is_empty() || kind.chars().any(char::is_whitespace) {
        return Err(MemoryError::domain(
            "invalid_record",
            "A type's identifier cannot be empty or contain spaces.".to_owned(),
            json!({"kind": kind}),
        ));
    }
    if kind == TYPE_KIND {
        return Err(MemoryError::domain(
            "invalid_record",
            format!(
                "`{TYPE_KIND}` is how the corpus stores every definition, so it cannot be a type of its own."
            ),
            json!({"kind": kind}),
        ));
    }
    Ok(())
}

/// Why a record's document is not here, in a sentence rather than a word.
///
/// The two absences are not degrees of the same thing. One says another branch
/// has this document and this one does not, which is routine; the other says
/// somebody deleted it here, which is a decision worth naming.
fn absence_reason(presence: &str) -> &'static str {
    match presence {
        NOT_ON_BRANCH => "the checked-out branch does not have it",
        REMOVED => "it was deleted on this branch and the deletion is not committed",
        _ => "its file could not be found",
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| MemoryError::Protocol(format!("unreadable engine response: {error}")))
}

/// The project's own record, as an envelope.
///
/// `language` is a product field rather than the envelope's own `language`
/// column, because the column is per record and this is a statement about the
/// project — every record in it inherits the answer, so the project's own
/// language needs a product field of its own.
fn project_record(settings: &ProjectSettings) -> Entity {
    let mut extensions = serde_json::Map::new();
    extensions.insert("language".to_owned(), json!(settings.language));
    // Named `installed` rather than `extensions`: the envelope's own
    // `extensions` map is where every product field of every kind lives, and a
    // field called `extensions` inside it would read as though the map were
    // about this one thing.
    extensions.insert("installed".to_owned(), json!(settings.installed));

    Entity {
        key: settings.identifier.clone(),
        kind: EntityKind::Project.as_str().to_owned(),
        title: settings.name.clone(),
        content: settings.description.clone(),
        tags: Vec::new(),
        links: Vec::new(),
        paths_observed: Vec::new(),
        scope_paths: Vec::new(),
        extensions,
        // The record that names the project is the project, not something filed
        // in it.
        folder: None,
        is_folder: false,
    }
}

/// Read the record back.
///
/// The engine answers with the stored record, which wraps the envelope Sync
/// wrote rather than being it — the representation travels beside it, because
/// an encrypted project's record is not readable prose.
///
/// A record missing the fields Sync wrote is treated as no record at all: the
/// flow then asks, which is recoverable, where trusting half of it would open a
/// project called nothing.
fn settings_from_record(record: &Value, key: &str) -> Option<ProjectSettings> {
    let record = record.get("envelope").unwrap_or(record);
    let name = record.get("title")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(ProjectSettings {
        name: name.to_owned(),
        // The key, with nothing between it and the answer. A record addressed
        // by anything else is a record this build did not write, and saying so
        // where it is read would be guessing at what it meant.
        identifier: key.to_owned(),
        description: record
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        language: record
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        // A record written before the field existed has no `installed`, and a
        // project that declares nothing is a project composed of nothing. Both
        // are the empty list, and neither is an error: the window opens on the
        // catalogue and says so.
        installed: record
            .get("installed")
            .and_then(|value| {
                serde_json::from_value::<Vec<sync_memory::InstalledExtension>>(value.clone()).ok()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_selection_the_engine_already_understands_is_untouched() {
        let selection = json!({"kind": "doc", "folder": "docs/guides", "limit": 50});
        assert_eq!(engine_query(&selection), selection);
    }

    #[test]
    fn the_one_member_spelled_differently_is_translated() {
        let query = engine_query(&json!({"folder": "docs", "folderScope": "subtree"}));

        assert_eq!(query["folder_scope"], json!("subtree"));
        assert!(
            query.get("folderScope").is_none(),
            "the window's spelling does not travel: the engine ignores what it does not \
             recognise, and a filter that quietly stops applying shows a subtree as a folder"
        );
        assert_eq!(query["folder"], json!("docs"), "the rest is left alone");
    }

    #[test]
    fn the_fields_a_column_will_draw_do_not_travel_to_the_engine() {
        let query = engine_query(&json!({
            "kind": "tasks.task",
            "fields": ["status", "priority"],
        }));

        assert!(
            query.get("fields").is_none(),
            "which fields come back is a question about the answer, not a filter on what is              selected: sent on, it is a member of a query the engine validates"
        );
        assert_eq!(query["kind"], json!("tasks.task"), "the rest is left alone");
    }
}
