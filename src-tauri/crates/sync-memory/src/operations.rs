//! Every operation of the host channel, spelled once.
//!
//! Two clients ask the same questions of two different things. The window's
//! [`MemoryClient`](crate::MemoryClient) has a sidecar on the other end and
//! restarts it when it dies; the phone has a computer on the other end of a
//! network and names its project in every call. Everything above that — which
//! operation a question is, what its parameters are called, what shape comes
//! back — is identical, and was identical in two files until this one existed.
//!
//! So it is one trait with one required method. A client says how a call is
//! carried; the channel's vocabulary is written here and nowhere else, which is
//! what makes a renamed parameter a compiler error in both clients rather than
//! a call that returns nothing in the one nobody rebuilt.
//!
//! **What is not here is as deliberate as what is.** The handshake, the project
//! a connection is about, and the revision this session last saw are not
//! operations — they are how a particular client is arranged, and two clients
//! arrange them differently. A trait method for one of those would be this file
//! claiming something true of only one caller.

use serde_json::{Value, json};

use crate::dto::{
    ContentView, EntityInput, FetchOutcome, FolderEntry, Handshake, Listing, MemoryPresence,
    ModelStatus, ProjectSettings, RecordView, ScanOutcome, SearchOutcome, SyncState,
    TransactionResult, TransportStatus,
};
use crate::error::{MemoryError, Result};
use crate::mapping::{Dependents, Document, DocumentEdits, RecordType, RecordsPage};

/// What running an operation does to the project.
///
/// One question and it has one caller: whether a call whose answer never came
/// back can be made again. A client that has just dialled its computer a second
/// time knows the connection is new and knows nothing about what happened on
/// the old one — so *asking again* is free for one of these and a second record
/// for the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Answers a question and leaves the project as it was.
    Reads,
    /// Writes: a record, a file in the working tree, the index, the remote.
    Writes,
}

/// What `method` does, or `None` where this build has never heard of it.
///
/// `None` rather than a guess, and a caller reading it as *writes* rather than
/// as *reads*: an operation added to the channel and forgotten here would
/// otherwise become the one thing this exists to prevent, quietly, on whichever
/// client was not rebuilt. The engine's own test names every operation it
/// registers against this list, so the forgetting is a red build there rather
/// than a duplicate record on somebody's phone.
#[must_use]
pub fn effect(method: &str) -> Option<Effect> {
    Some(match method {
        // The first two are answered by the door rather than by an operation:
        // they are what a connection asks before it has named a project, and
        // the first thing a client that lost its network asks again.
        crate::METHODS
        | crate::PROJECTS
        | "documents.get"
        | "documents.read"
        | "engine.model_status"
        | "engine.presence"
        | "engine.sync_state"
        | "engine.transport_status"
        | "folders.list"
        | "folders.toll"
        | "project.describe"
        | "project.export"
        | "project.revision"
        | "project.schema_status"
        | "project.settings"
        | "records.backlinks"
        | "records.dependents"
        | "records.get"
        | "records.list"
        | "records.load"
        | "records.search"
        | "types.list"
        // The reads a machine answers about its own artefacts and its registry.
        // A fetch of the index touches the network and writes a cache file, and
        // it is still a read: asking twice answers the same question twice.
        | crate::EXTENSION_LIST
        | crate::EXTENSION_FILE
        | crate::REGISTRY_INDEX
        | crate::REGISTRY_CACHED
        | crate::REGISTRY_LEDGER
        | crate::SCHEDULE_OFF => Effect::Reads,
        "documents.create"
        | "documents.create_file"
        | "documents.move"
        | "documents.resolve_unmatched"
        // A scan reconciles an attached folder with the records, and writes
        // whatever it settles.
        | "documents.scan"
        | "documents.update"
        | "documents.write"
        | "engine.fetch"
        | "engine.push"
        | "engine.remote_remove"
        | "engine.remote_set"
        | "engine.rewind"
        | "folders.create"
        | "folders.delete"
        // The folder's own record is written where it does not have one.
        | "folders.describe"
        | "folders.rename"
        | "project.import"
        | "project.reconcile"
        | "project.reindex"
        | "project.update"
        | "records.apply"
        | "records.delete"
        | "records.save"
        | "types.attach_folder"
        | "types.create"
        | "types.delete"
        | "types.publish"
        | "types.publish_extension"
        | "types.update"
        // Not operations of the surface either: what a machine does with the
        // artefacts on its own disk. They are named here because a client that
        // lost its network has to decide about them too, and the answers differ
        // — asking what is installed again is free, and installing again is a
        // download.
        | crate::EXTENSION_FETCH
        | crate::EXTENSION_INSTALL
        | crate::EXTENSION_FORGET
        | crate::EXTENSION_REPOINT
        // A handler runs somebody else's code, and what it does is its own
        // business. Nothing here may decide that asking again is free.
        | crate::EXTENSION_OCCASION
        | crate::SCHEDULE_REMEMBER
        | crate::SCHEDULE_SWITCH => Effect::Writes,
        _ => return None,
    })
}

/// The channel's operations, over whatever carries them.
pub trait Operations {
    /// Ask for one operation and hand back what it answered.
    ///
    /// The one thing a client has to supply, and the one thing the two clients
    /// do differently: the window's restarts a dead sidecar and replays, the
    /// phone's puts the project's key in the call and dials again if the
    /// network went away.
    ///
    /// # Errors
    ///
    /// Returns whatever the operation refused, and the transport's own failure
    /// where the question never arrived.
    fn request(&mut self, method: &str, params: &Value) -> Result<Value>;

    /// What the store says about itself and about this project.
    ///
    /// The versions it runs, which storage holds the records, whether there is
    /// a model. A client asks it once on connecting and a phone asks it when a
    /// project is opened, which are the same moment seen from two places.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn describe(&mut self) -> Result<Handshake> {
        parse(self.request("project.describe", &json!({}))?)
    }

    /// The revision the project stands at, read from the store.
    ///
    /// Read rather than recalled: the question is about everything else that
    /// may have moved it — a `git pull`, a second window, the engine's own
    /// CLI — so a client answering from what it last wrote would answer a
    /// different question.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn read_revision(&mut self) -> Result<String> {
        parse(self.request("project.revision", &json!({}))?)
    }

    // ── Reads ───────────────────────────────────────────────────────────────

    /// Fetch one record by key.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn get_record(&mut self, key: &str) -> Result<RecordView> {
        let answer = self.request("records.get", &json!({"key": key}))?;
        parse(answer)
    }

    /// List records with filters, sorting and paging.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn list_records(&mut self, query: &Value) -> Result<Listing> {
        let answer = self.request("records.list", &query.clone())?;
        parse(answer)
    }

    /// Search, reporting honestly whether the answer is FTS-only.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn search(&mut self, query: &Value) -> Result<SearchOutcome> {
        let answer = self.request("records.search", &query.clone())?;
        parse(answer)
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
    fn publish_types(&mut self) -> Result<bool> {
        let answer = self.request("types.publish", &json!({}))?;
        parse(answer)
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
    fn list_types(&mut self) -> Result<Vec<RecordType>> {
        let answer = self.request("types.list", &json!({}))?;
        parse(answer)
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
    fn create_type(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
    ) -> Result<TransactionResult> {
        let answer = self.request(
            "types.create",
            &json!({"kind": kind, "title": title, "description": description, "icon": icon}),
        )?;
        parse(answer)
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
    fn publish_extension_types(&mut self, types: &Value) -> Result<bool> {
        let answer = self.request("types.publish_extension", &json!({"types": types}))?;
        parse(answer)
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
    fn update_type(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
    ) -> Result<TransactionResult> {
        let answer = self.request(
            "types.update",
            &json!({"kind": kind, "title": title, "description": description, "icon": icon}),
        )?;
        parse(answer)
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
    /// One transaction: the records and the definition together. Every envelope
    /// lives in the storage that holds records — a type naming a storage puts
    /// its *documents* elsewhere, never its envelopes — so there is no pair to
    /// order and no window between them. The state that must never exist,
    /// records of a kind nothing can define, is one this can no longer pass
    /// through.
    ///
    /// The documents of an attached folder are not deleted. Memory removes the
    /// records that point at the files and leaves the files where the team put
    /// them — it never wrote them, and deleting somebody's documentation
    /// because a type was removed from an application is not a decision this
    /// window gets to make. Whoever asked is told so before they confirm.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, and `invalid_record` for one of Sync's own
    /// types — deleting `project` would leave the record naming the project with
    /// a kind the strict schema rejects.
    fn delete_type(&mut self, kind: &str) -> Result<usize> {
        let answer = self.request("types.delete", &json!({"kind": kind}))?;
        parse(answer)
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
    fn records(&mut self, selection: &Value, hidden: &[String]) -> Result<RecordsPage> {
        let answer = self.request(
            "records.load",
            &json!({"selection": selection, "hidden": hidden}),
        )?;
        parse(answer)
    }

    /// One record, whole, as the document view shows it.
    ///
    /// `None` when the key does not exist at this revision — a record that was
    /// deleted while the window had it open is an answer, not a failure.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn document(&mut self, key: &str) -> Result<Option<Document>> {
        let answer = self.request("documents.get", &json!({"key": key}))?;
        parse(answer)
    }

    /// Find what links to or mentions a key.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn backlinks(&mut self, key: &str) -> Result<Value> {
        let answer = self.request("records.backlinks", &json!({"key": key}))?;
        Ok(answer)
    }

    // ── Folders ─────────────────────────────────────────────────────────────

    /// The project's folders, from the records and from the working tree at
    /// once.
    ///
    /// `folder` absent asks about the whole project. `Some("")` asks about the
    /// root, which is a folder like any other — the two are different
    /// questions. `subtree` decides whether the region reaches below the folder
    /// it names.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn folders(
        &mut self,
        folder: Option<&str>,
        subtree: bool,
        kind: Option<&str>,
    ) -> Result<Vec<FolderEntry>> {
        let mut params = json!({"subtree": subtree});
        if let Some(folder) = folder {
            params["folder"] = json!(folder);
        }
        if let Some(kind) = kind {
            params["kind"] = json!(kind);
        }
        let answer = self.request("folders.list", &params)?;
        parse(answer)
    }

    /// Make a folder that nothing is in yet, under the type named by `kind`.
    ///
    /// What a folder is differs by where the type keeps its documents, and the
    /// engine decides that from the kind. One difference reaches a person and
    /// cannot be hidden: Git keeps no empty directories, so a folder made in an
    /// attached directory is a fact about this working tree until something is
    /// filed in it, while one made in the records travels at once.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` for a folder
    /// outside the type's storage and for one that already exists.
    fn create_folder(&mut self, folder: &str, kind: &str) -> Result<TransactionResult> {
        let answer = self.request("folders.create", &json!({"folder": folder, "kind": kind}))?;
        parse(answer)
    }

    /// Take a folder and everything filed under it, and say how many went.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn delete_folder(&mut self, folder: &str) -> Result<usize> {
        let answer = self.request("folders.delete", &json!({"folder": folder}))?;
        parse(answer)
    }

    /// How many records a folder holds, at any depth and whatever their type.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn folder_toll(&mut self, folder: &str) -> Result<usize> {
        let answer = self.request("folders.toll", &json!({"folder": folder}))?;
        parse(answer)
    }

    /// Rename a folder, moving every record filed under it in one transaction.
    ///
    /// # Errors
    ///
    /// Where the documents are files the directory is renamed too. A type's own
    /// storage root is not: moving that is a change to the type.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<TransactionResult> {
        let answer = self.request("folders.rename", &json!({"from": from, "to": to}))?;
        parse(answer)
    }

    /// File one record in another folder. `""` is the root.
    ///
    /// Whether a file moves with it is the engine's business: a record whose
    /// body is a repository file has a folder that *is* that file's directory,
    /// and the engine moves both. This window never writes into somebody's
    /// working tree itself.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_argument` when a
    /// file-backed document is asked to leave its type's storage, or when a
    /// document of that name is already at the destination.
    fn move_document(&mut self, key: &str, folder: &str) -> Result<TransactionResult> {
        let answer = self.request("documents.move", &json!({"key": key, "folder": folder}))?;
        parse(answer)
    }

    // ── Attached folders ────────────────────────────────────────────────────

    /// Attach a folder of the repository as a type, and settle what is in it.
    ///
    /// Three steps now, and each is a different kind of statement. The project
    /// declares a storage — "this directory of the working tree is somewhere I
    /// keep documents" — the type names it, and the scan turns the files
    /// already there into records. A declaration on its own is a folder nothing
    /// reads; a definition on its own would name a storage that does not exist
    /// and be refused; a type without a scan is a project claiming a corpus
    /// while the documents sit on disk beside it.
    ///
    /// One storage per type, because that is now what a storage is: there is no
    /// mask any more, so **every** file in the folder is a document of the type
    /// that names it — images and PDFs included — and two types over one folder
    /// would both claim every new file in it.
    ///
    /// Nothing is written into the folder — not a marker, not an id in
    /// frontmatter. That is the promise the whole arrangement rests on, and it
    /// is the engine's to keep; what this method owes the interface is not
    /// quietly doing something else on the way.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_record` for a kind the
    /// project may not define and for a folder the engine refuses to declare.
    fn attach_folder(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
        folder: &str,
    ) -> Result<ScanOutcome> {
        let answer = self.request("types.attach_folder", &json!({"kind": kind, "title": title, "description": description, "icon": icon, "folder": folder}))?;
        parse(answer)
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
    fn scan(&mut self) -> Result<ScanOutcome> {
        let answer = self.request("documents.scan", &json!({}))?;
        parse(answer)
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
    fn read_content(&mut self, key: &str) -> Result<ContentView> {
        let answer = self.request("documents.read", &json!({"key": key}))?;
        parse(answer)
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
    fn write_content(&mut self, key: &str, content: &str) -> Result<TransactionResult> {
        let answer = self.request("documents.write", &json!({"key": key, "content": content}))?;
        parse(answer)
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
    fn write_content_as(
        &mut self,
        key: &str,
        content: &str,
        encoding: &str,
    ) -> Result<TransactionResult> {
        let answer = self.request(
            "documents.write",
            &json!({"key": key, "content": content, "encoding": encoding}),
        )?;
        parse(answer)
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
    fn resolve_unmatched(
        &mut self,
        locator: &str,
        content_hash: &str,
        kind: &str,
        adopt: Option<&str>,
    ) -> Result<TransactionResult> {
        let answer = self.request(
            "documents.resolve_unmatched",
            &json!({"locator": locator, "contentHash": content_hash, "kind": kind, "adopt": adopt}),
        )?;
        parse(answer)
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
    fn update_document(&mut self, key: &str, edits: &DocumentEdits) -> Result<TransactionResult> {
        let answer = self.request("documents.update", &json!({"key": key, "edits": edits}))?;
        parse(answer)
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
    fn create_document(
        &mut self,
        kind: &str,
        title: &str,
        folder: Option<&str>,
    ) -> Result<Document> {
        let mut params = json!({"kind": kind, "title": title});
        if let Some(folder) = folder {
            params["folder"] = json!(folder);
        }
        let answer = self.request("documents.create", &params)?;
        parse(answer)
    }

    /// The document that *is* a folder, opened or written.
    ///
    /// A folder that already has one answers with it: two records standing for
    /// one folder is a question with no answer, and the engine refuses the
    /// second write for the same reason.
    ///
    /// What comes back is an ordinary document — searched, listed and linked to
    /// like any other, because that is all it is. Nothing had to learn about
    /// folders for its text to be indexed.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `invalid_record` for a folder
    /// outside the type's storage.
    fn describe_folder(&mut self, folder: &str, kind: &str) -> Result<Document> {
        let answer = self.request("folders.describe", &json!({"folder": folder, "kind": kind}))?;
        parse(answer)
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
    fn create_file_document(
        &mut self,
        kind: &str,
        name: &str,
        content_base64: &str,
    ) -> Result<Document> {
        let answer = self.request(
            "documents.create_file",
            &json!({"kind": kind, "name": name, "contentBase64": content_base64}),
        )?;
        parse(answer)
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
    fn delete_documents(&mut self, keys: &[String]) -> Result<TransactionResult> {
        let answer = self.request("records.delete", &json!({"keys": keys}))?;
        parse(answer)
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
    fn dependents(&mut self, key: &str) -> Result<Dependents> {
        let answer = self.request("records.dependents", &json!({"key": key}))?;
        parse(answer)
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
    fn apply(&mut self, transaction_id: &str, operations: &[Value]) -> Result<TransactionResult> {
        let answer = self.request(
            "records.apply",
            &json!({"occasion": transaction_id, "operations": operations}),
        )?;
        parse(answer)
    }

    /// Import a bundle in one transaction.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn import(&mut self, _transaction_id: &str, bundle: &Value) -> Result<TransactionResult> {
        let answer = self.request("project.import", &json!({"bundle": bundle}))?;
        parse(answer)
    }

    /// Export the current revision as a bundle.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn export(&mut self) -> Result<Value> {
        let answer = self.request("project.export", &json!({}))?;
        Ok(answer)
    }

    /// Create or update entities in one transaction.
    ///
    /// The transaction id is the daemon's to allocate, like every other write
    /// on this channel: an id names one attempt, and only the writer can be
    /// sure it is naming its own.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn save_entities(&mut self, entities: &[EntityInput]) -> Result<TransactionResult> {
        parse(self.request("records.save", &json!({"entities": entities}))?)
    }

    // ── The project's own record ────────────────────────────────────────────

    /// What this project says it is called, or `None` if it has never said.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn project_settings(&mut self) -> Result<Option<ProjectSettings>> {
        parse(self.request("project.settings", &json!({}))?)
    }

    /// Write what the project says it is, seeding Sync's own type definitions
    /// in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn update_project(&mut self, settings: &ProjectSettings) -> Result<TransactionResult> {
        parse(self.request("project.update", &json!({"settings": settings}))?)
    }

    // ── Status ──────────────────────────────────────────────────────────────

    /// Whether search runs hybrid or FTS-only, and which model is active.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn model_status(&mut self) -> Result<ModelStatus> {
        let answer = self.request("engine.model_status", &json!({}))?;
        parse(answer)
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
    fn schema_status(&mut self) -> Result<Value> {
        let answer = self.request("project.schema_status", &json!({}))?;
        Ok(answer)
    }

    /// Remote configuration for memory, which is separate from code `origin`.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn transport_status(&mut self) -> Result<TransportStatus> {
        let answer = self.request("engine.transport_status", &json!({}))?;
        parse(answer)
    }

    /// Whether this repository's memory is here, still on a remote, or nowhere.
    ///
    /// Asked before a project is described, and deliberately not derived from
    /// an empty corpus: an empty corpus is what a fresh clone and a new project
    /// have in common, and only the remote can tell them apart.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn presence(&mut self) -> Result<MemoryPresence> {
        let answer = self.request("engine.presence", &json!({}))?;
        parse(answer)
    }

    /// Whether the project's memory is in step with its remote.
    ///
    /// `ask_remote` decides whether the network is touched. The count of
    /// unpublished records never needs it, so a window opening offline still
    /// has something true to show.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn sync_state(&mut self, ask_remote: bool) -> Result<SyncState> {
        let answer = self.request("engine.sync_state", &json!({"askRemote": ask_remote}))?;
        parse(answer)
    }

    /// Configure the memory remote, which is separate from the code `origin`.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including a URL Git would read as an option
    /// or as a remote helper rather than as a location.
    fn set_remote(&mut self, url: &str, refspec: Option<&str>) -> Result<TransportStatus> {
        let mut arguments = json!({"url": url});
        if let Some(refspec) = refspec {
            arguments["refspec"] = json!(refspec);
        }
        let answer = self.request("engine.remote_set", &arguments)?;
        parse(answer)
    }

    /// Forget the memory remote. Nothing local is touched: memory stays where
    /// it is and stops being published, which is the state it starts in.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn remove_remote(&mut self) -> Result<TransportStatus> {
        let answer = self.request("engine.remote_remove", &json!({}))?;
        parse(answer)
    }

    // ── Transport ───────────────────────────────────────────────────────────

    /// Fetch memory from the remote and merge it.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn fetch(&mut self) -> Result<FetchOutcome> {
        let answer = self.request("engine.fetch", &json!({}))?;
        parse(answer)
    }

    /// Put memory back where it stood, undoing what has happened since.
    ///
    /// What a fetch is undone with: the revision to name is the
    /// `localRevisionBefore` that fetch reported. Backwards along memory's own
    /// history and nowhere else, so this cannot arrive at a state the project
    /// was never in.
    ///
    /// # Errors
    ///
    /// Returns the engine failure: `invalid_argument` for a revision this
    /// memory never passed through, and `conflict` when something has been
    /// written since — the undo would carry that away with the merge.
    fn rewind(&mut self, revision: &str, expected: &str) -> Result<()> {
        self.request(
            "engine.rewind",
            &json!({"revision": revision, "expected": expected}),
        )?;
        Ok(())
    }

    /// Push memory to the remote.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `push_blocked` from the
    /// stale-record policy.
    fn push(&mut self, force: bool) -> Result<Value> {
        let answer = self.request("engine.push", &json!({"force": force}))?;
        Ok(answer)
    }

    /// Rebuild the search index.
    ///
    /// # Errors
    ///
    /// Returns the engine failure.
    fn reindex(&mut self) -> Result<Value> {
        let answer = self.request("project.reindex", &json!({}))?;
        Ok(answer)
    }

    /// Catch memory up with code history, rebuilding when it was rewritten.
    ///
    /// Ordinary catch-up is the engine's own business — it reconciles ahead of
    /// every write without being asked. This is the one case it will not settle
    /// alone: a rebase, a reset or a replaced branch leaves the cursor on a
    /// commit the current history does not descend from, and from then on every
    /// write is refused with `diverged`. Somebody has to say that the new
    /// history is the real one, and `full_rebuild` is them saying it.
    ///
    /// What it costs is freshness, not text: every record becomes `unverified`,
    /// which is the honest state for a claim last checked against a history
    /// that no longer exists.
    ///
    /// # Errors
    ///
    /// Returns the engine failure, including `diverged` when the history moved
    /// and `full_rebuild` was not asked for.
    fn reconcile(&mut self, full_rebuild: bool) -> Result<Value> {
        let answer = self.request("project.reconcile", &json!({"full_rebuild": full_rebuild}))?;
        Ok(answer)
    }
}

/// Read an answer into the shape its operation promises.
///
/// A body that does not fit is a protocol failure rather than a domain one: the
/// engine answered, and what it answered is not what this build knows how to
/// read.
pub(crate) fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| MemoryError::Protocol(format!("unreadable engine response: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{Effect, effect};

    /// Every call a door carries to the application says what it does.
    ///
    /// The two lists are read by different people at different times — one
    /// decides where a call goes, the other decides whether a client that lost
    /// its network may make it again — and a name in the first that is missing
    /// from the second is a call quietly treated as a write. That is the safe
    /// direction, which is exactly why nothing would ever report it: installing
    /// a package would simply stop replaying, for ever, and nobody would know
    /// it had been decided.
    #[test]
    fn every_carried_call_says_whether_it_writes() {
        for method in [
            crate::EXTENSION_FETCH,
            crate::EXTENSION_LIST,
            crate::EXTENSION_FILE,
            crate::EXTENSION_INSTALL,
            crate::EXTENSION_FORGET,
            crate::EXTENSION_REPOINT,
            crate::EXTENSION_OCCASION,
            crate::REGISTRY_INDEX,
            crate::REGISTRY_CACHED,
            crate::REGISTRY_LEDGER,
            crate::SCHEDULE_REMEMBER,
            crate::SCHEDULE_OFF,
            crate::SCHEDULE_SWITCH,
        ] {
            assert!(
                crate::carried(method),
                "`{method}` is listed here and is not carried"
            );
            assert!(
                effect(method).is_some(),
                "`{method}` is carried and does not say whether it writes"
            );
        }
    }

    /// Reading a package's files is a read, and installing one is not.
    ///
    /// Stated rather than left to the list above, because this is the pair the
    /// distinction exists for: a phone whose network came back re-reads a
    /// stylesheet without asking anybody, and does not download an artefact a
    /// second time.
    #[test]
    fn reading_an_artefact_replays_and_installing_one_does_not() {
        assert_eq!(effect(crate::EXTENSION_FILE), Some(Effect::Reads));
        assert_eq!(effect(crate::EXTENSION_INSTALL), Some(Effect::Writes));
    }
}
