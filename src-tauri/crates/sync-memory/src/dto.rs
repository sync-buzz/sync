//! The engine's wire shapes, as Rust types.
//!
//! These deliberately mirror what `memory-hub-mcp` documents rather than
//! importing its crates: the engine is a separate process on its own release
//! cadence, and duplicating a handful of field names is the price of not
//! linking it. Unknown fields are ignored, so a newer engine that adds one does
//! not break this build.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::mapping::RecordType;
use serde_json::Value;

/// A major/minor pair from the handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

/// What the engine reports about itself and the project at `initialize`.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Handshake {
    pub memory_interface_version: Version,
    pub store_version: Version,
    pub envelope_version: Value,
    pub index_version: Version,
    /// `None` when no embedding model is installed — search is FTS-only, which
    /// is a normal state rather than a failure.
    #[serde(default)]
    pub model_fingerprint: Option<String>,
    pub installation_id: String,
    pub project_id: String,
    pub project_path: String,
    #[serde(default)]
    pub git_dir: Option<String>,
    /// Which storage holds this project's records: `refs` for the Git objects
    /// Sync initialises, `folder` for a project some other client set up as
    /// files.
    ///
    /// Absent for a project that has not been initialised at all, which is the
    /// state `memory_init` exists to fix. What reads it hides the affordances
    /// that are Git's alone — diff, fetch and push —
    /// rather than offering them and explaining the refusal afterwards.
    #[serde(default)]
    pub backend: Option<String>,
    /// How far memory trails code history, as of session start.
    #[serde(default)]
    pub reconciliation: Value,
}

impl Handshake {
    /// Whether this project's records are Git objects.
    ///
    /// The question behind diff, fetch and push: those
    /// are Git's, and a project keeping its records as files answers
    /// `unsupported` to every one of them. Asked so the window can leave them
    /// out rather than offer them and explain the refusal afterwards.
    #[must_use]
    pub fn records_are_git(&self) -> bool {
        self.backend.as_deref() == Some("refs")
    }
}

/// The result of a transaction or import.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionResult {
    pub revision: String,
    #[serde(default)]
    pub changed_keys: Vec<String>,
}

/// One record as of a revision. `record` is `None` when the key does not exist.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordView {
    pub revision: String,
    #[serde(default)]
    pub record: Option<Value>,
}

/// A page of records plus counts over the whole filtered corpus.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub revision: String,
    #[serde(default)]
    pub records: Vec<Value>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
    #[serde(default)]
    pub counts: Counts,
}

/// Counts over everything a listing's filters selected, not over its page.
///
/// That distinction is the whole reason to ask for them: one listing with
/// `limit: 1` answers "how much of each kind does the project hold" without
/// reading the corpus.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Counts {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub by_kind: BTreeMap<String, usize>,
    #[serde(default)]
    pub by_freshness: BTreeMap<String, usize>,
    #[serde(default)]
    pub archived: usize,
    #[serde(default)]
    pub live: usize,
    /// Memory's own machinery — type definitions — counted apart and in none of
    /// the numbers above. Schema is not an answer to a question about the
    /// subject matter, so it is never part of "how much does this project
    /// know".
    #[serde(default)]
    pub service: usize,
}

/// A search result, including how it was answered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchOutcome {
    #[serde(default)]
    pub hits: Vec<Value>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
    /// `fts` or `hybrid`.
    pub mode: String,
    /// `true` when `total` is a floor: the engine stopped counting at its cap.
    ///
    /// Defaulted rather than required, because an engine older than the count
    /// answers without it and its `total` is not capped — it is simply the
    /// page it read.
    #[serde(default)]
    pub total_capped: bool,
    /// `true` only when no embedding model is available at all. Not an error:
    /// FTS-only is a working state, and the UI is expected to say so plainly.
    pub degraded: bool,
    pub revision: String,
}

/// Whether search can use vectors, and which model would be used.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub dimensions: Option<u32>,
    pub runtime: String,
    pub runtime_state: String,
    pub vector_search: bool,
    pub fts_only: bool,
    pub mode: String,
}

/// The memory remote, which is separate from the code `origin`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportStatus {
    pub remote_configured: bool,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub refspec: Option<String>,
    /// The code `origin`, which is not where memory is published and is only
    /// carried here because it is the one address a window can suggest when
    /// offering to configure a memory remote.
    #[serde(default)]
    pub code_origin_url: Option<String>,
}

/// What a fetch did.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchOutcome {
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub fast_forward: bool,
    /// Records where both sides had moved the same thing. This side's version
    /// was kept there; the other is still a commit in the history.
    #[serde(default)]
    pub overlaps: Vec<Overlap>,
    /// Where memory stood before this fetch.
    ///
    /// What an undo needs, and the reason it is carried rather than derived: a
    /// merge is an ordinary commit on top of what was here, so going back is
    /// naming the revision that was here — and nothing else in the window knows
    /// it once the fetch has landed.
    #[serde(default)]
    pub local_revision_before: Option<String>,
    /// Where the fetch left memory.
    ///
    /// What an undo is checked against: the engine refuses if the tip has moved
    /// past it, because then something was written after the merge and going
    /// back would take that with it.
    #[serde(default)]
    pub local_revision_after: Option<String>,
}

/// One record a fetch merged over, and what of the other version it cost.
///
/// Named rather than counted. "A record was merged over" is not something
/// anybody can act on; knowing it was the title says where to look, and knowing
/// it was the body says to read it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overlap {
    pub key: String,
    /// The same lines of the body were rewritten on both sides.
    #[serde(default)]
    pub body: bool,
    /// Members of the record both sides moved: `title`, `folder`, a product
    /// field's own name.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// What the remote had to say, when it was asked at all.
///
/// Four states rather than a flag: "not asked" and "could not be asked" are not
/// "nothing is waiting", and a header that collapsed them would tell somebody
/// their memory is published when nobody could reach the remote.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCheck {
    NotAsked,
    Waiting,
    UpToDate,
    Unreachable,
}

/// Whether the project's memory is in step with its remote.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub remote_configured: bool,
    /// **Records**, not commits. Every save is a commit, so a count of commits
    /// would say `12` for twelve edits of one record — true of the history and
    /// not of anything a person would recognise as theirs.
    pub unpublished: usize,
    pub remote: RemoteCheck,
}

/// Whether this repository's memory is here, still on a remote, or nowhere.
///
/// `git clone` copies no `refs/memory/*`, so a fresh clone of a project with
/// years of memory and a project that never had any are the same picture from
/// inside. The engine asks the remote; this is the answer, and the flow that
/// opens a project branches on it before it offers to describe anything.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemoryPresence {
    /// Memory is here.
    Present { records: usize },
    /// Memory is on `url` and has not been fetched. `configured` is false when
    /// the address is the code `origin` rather than a memory remote, which is
    /// the state every fresh clone is in and the one that needs a remote
    /// configured before anything can be fetched.
    NotFetched { url: String, configured: bool },
    /// There is none, here or there. A project starts here.
    Absent { url: Option<String> },
    /// Nobody could say — which is not the same answer as "there is none".
    Unreachable { url: String, reason: String },
}

/// What a record's body turned out to be, and whether there was one.
///
/// A record that keeps its content always answers with it. One whose content
/// is a repository file answers with the file — or says the file is not here,
/// which is a normal state on a branch that does not have the document rather
/// than a failure to report.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentView {
    /// `record` when the body is the record's own, `file` when it was resolved
    /// through a locator.
    #[serde(default)]
    pub source: String,
    /// `null` on the wire when there was nothing to read, which is why this is
    /// an `Option` rather than an empty string: an empty document is something
    /// somebody wrote.
    #[serde(default)]
    pub content: Option<String>,
    /// True when the locator resolved to nothing.
    #[serde(default)]
    pub missing: bool,
    /// Why it is not here, in the engine's own words — `not_on_branch` or
    /// `removed`. The two mean opposite things to a person: one is another
    /// branch's document, the other is a deletion somebody made here.
    #[serde(default)]
    pub reason: Option<String>,
    /// Where the bytes were read from, when they came from outside.
    #[serde(default)]
    pub path: Option<String>,
    /// The digest of what was actually read.
    #[serde(default, rename = "content_hash")]
    pub content_hash: Option<String>,
    /// True when the file has changed since Memory last looked, so whatever
    /// the record claims about it was checked against another text.
    #[serde(default)]
    pub changed: bool,
    /// How to read `content`: `utf-8` for text, `base64` for bytes, `none` for
    /// a body that was not fetched at all and is named by `url`.
    ///
    /// Absent for a record that carries its own body, which is always text.
    /// Branching on this is not optional: a folder holds whatever is in it now
    /// that there is no mask, and a client ignoring the encoding renders a PNG
    /// as a page of base64.
    #[serde(default)]
    pub encoding: Option<String>,
    /// How many bytes there were, when what was read was bytes.
    #[serde(default)]
    pub bytes: Option<usize>,
    /// Where the content is, for a record that points at something nothing
    /// fetched.
    #[serde(default)]
    pub url: Option<String>,
    /// What the document is, from its file name: `text/markdown`, `image/png`.
    #[serde(default)]
    pub media_type: Option<String>,
}

/// The encoding of a body that is text, and of every record that holds its own.
const UTF8: &str = "utf-8";

impl ContentView {
    /// Whether what came back is text this window can show and edit.
    ///
    /// A record carrying its own body says nothing about encoding and is always
    /// text; a file says which it is. Anything else — bytes, a link, a spelling
    /// this build has not heard of — is not text, and treating an unknown as
    /// prose is the one failure this method exists to prevent.
    #[must_use]
    pub fn is_text(&self) -> bool {
        match self.encoding.as_deref() {
            None => true,
            Some(encoding) => encoding.eq_ignore_ascii_case(UTF8),
        }
    }

    /// The body as something to show, which is the empty string when there was
    /// nothing to read — or when what was read is not text, because base64
    /// shown as prose is not the document and inviting somebody to edit it
    /// would write that string over the file.
    ///
    /// Whether the empty string means "empty", "missing" or "not text" is
    /// [`missing`](Self::missing)'s and [`is_text`](Self::is_text)'s answer,
    /// and callers are expected to ask.
    #[must_use]
    pub fn text(&self) -> &str {
        if self.is_text() {
            self.content.as_deref().unwrap_or_default()
        } else {
            ""
        }
    }
}

/// One folder, and everything known about it from both sources at once.
///
/// The two origins are separate answers because they mean different things. A
/// folder known from the records is one documents are filed in; a folder known
/// from storage is a directory of the working tree, which exists without
/// anybody's permission. Storage without records is an empty directory somebody
/// can file into. Records without storage is a folder whose documents this
/// branch does not have.
///
/// **Read live and never stored.** Git keeps no empty directories, so an empty
/// `docs/api/` is a fact about one working tree and is simply absent from a
/// fresh clone. Remembering the list would raise, on one machine, a folder that
/// does not exist on another.
/// This is the window's vocabulary, not the engine's. The engine spells these
/// `in_records`, `in_storage` and `described_by`; the sidecar's domain reads
/// that shape and builds this one, so nothing here carries two spellings of a
/// member. No `#[serde(default)]` for the same reason a [`Document`] has none:
/// the window and the sidecar are built together, and a member that arrives
/// missing is a bundle assembled wrong, which should say so rather than read as
/// `false`.
///
/// [`Document`]: crate::mapping::Document
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    /// Repository-relative. The root is `""`.
    pub path: String,
    /// At least one record is filed here.
    pub in_records: bool,
    /// The working tree has this directory, whatever is in it.
    pub in_storage: bool,
    /// How many documents are filed directly in it — not counting what is in
    /// the folders below, and not counting type definitions, which are schema
    /// rather than something the project knows.
    pub records: usize,
    /// The key of the record that *is* this folder, when one is.
    ///
    /// Whatever draws a tree needs it, or it shows that record twice — once as
    /// the folder and once as its own child.
    pub described_by: Option<String>,
}

/// What removing a type took with it.
///
/// The count is the answer to the question the confirmation asked, reported
/// back from the write rather than from the count the window showed before it:
/// the two differ if anything was written in between, and the one that happened
/// is the true one.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeRemoval {
    /// The corpus as it now stands.
    pub types: Vec<RecordType>,
    /// How many records of the type were deleted with its definition.
    pub removed: usize,
}

/// What attaching a folder produced: the corpus's types, and what the first
/// scan made of the files already in it.
///
/// Both halves matter to the window. The type is what the navigator lists; the
/// scan is what turned the documents on disk into records, and its unmatched
/// entries are the one part of attaching that a person has to answer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderAttachment {
    pub types: Vec<RecordType>,
    pub scan: ScanOutcome,
}

/// What reconciling the attached folders with the records did.
///
/// Four of the five outcomes are unambiguous and are applied without asking:
/// an edit in place, a move, a disappearance, a return. The fifth — a file
/// matching no record — is never guessed, because a rename with an edit and a
/// new file look identical from outside. Those arrive as `unmatched` changes
/// carrying the records they could be, and wait for a person.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    #[serde(default)]
    pub revision: Option<String>,
    /// How many files the scan looked at.
    #[serde(default)]
    pub scanned: usize,
    /// How many conclusions it wrote. Zero with changes present is the case
    /// worth knowing about: everything it found needs a person.
    #[serde(default)]
    pub applied: usize,
    /// One entry per conclusion, each naming what happened: `edited`, `moved`,
    /// `missing`, `returned`, `new`, `unmatched`.
    #[serde(default)]
    pub changes: Vec<Value>,
}

impl ScanOutcome {
    /// The files the scan could not attribute to a record.
    ///
    /// A rename with an edit and a genuinely new file are indistinguishable
    /// from outside, so the engine ranks the records it could be and stops.
    /// This is the queue a person answers.
    #[must_use]
    pub fn unmatched(&self) -> Vec<&Value> {
        self.changes
            .iter()
            .filter(|change| change.get("change").and_then(Value::as_str) == Some("unmatched"))
            .collect()
    }
}

/// What a project says about itself.
///
/// The one record of kind `project` in the project's own memory, read back as
/// the fields the opening flow asks for. It lives here rather than in the
/// window because both ends of the host channel speak it: the window collects
/// it from a person, and the daemon is what turns it into an envelope.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    pub name: String,
    /// What this project is called by anyone who refers to it — an agent
    /// naming which project a call is about, a document mentioning a
    /// neighbour, a person in a message.
    ///
    /// Derived from the name when the project is created and fixed from then
    /// on. It is the **key** of the record rather than a field beside it: the
    /// record travels with the repository, so two people who opened it hold the
    /// same identifier, and a key cannot drift from the field that mirrors it
    /// because there is no such field. Renaming the project does not move it —
    /// by then it has been written down elsewhere.
    ///
    /// A record written before the identifier was the key is addressed by the
    /// word every project used to share; `settings_from_record` derives one
    /// from the name instead, and the record moves at the next update.
    #[serde(default)]
    pub identifier: String,
    /// Optional, and empty far more often than not.
    #[serde(default)]
    pub description: String,
    /// The language the project writes its knowledge in.
    pub language: String,
    /// The extensions this project is composed of.
    ///
    /// A project declares what it depends on; the artefacts are the machine's
    /// business. Two checkouts of the same repository are therefore the same
    /// project rather than two folders that have to be set up separately, and
    /// an installation missing one of them can say so instead of silently
    /// showing less.
    ///
    /// Empty is a real answer: a project composed of nothing opens on the
    /// catalogue.
    #[serde(default)]
    pub installed: Vec<InstalledExtension>,
}

/// One extension a project depends on, by identifier and version.
///
/// The version is what was installed rather than what is available: an
/// extension that has moved on is something the window can notice and say,
/// which it could not do if the record only named the id.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub id: String,
    pub version: String,
    /// What this extension tells an agent, in full.
    ///
    /// Written into the project rather than held by the build that brought the
    /// extension, and that is what makes it reach an agent at all: the MCP
    /// server is a process of its own with no view of the catalogue, so a
    /// prompt that lived in the window would be a prompt only the window could
    /// read. Here it travels with the repository, and a colleague who clones
    /// the project gets the same agent.
    ///
    /// The copy goes out of date when the build moves on, which is why the
    /// window rewrites it whenever it opens a project and finds the two
    /// disagree — the same way it republishes the types.
    ///
    /// Absent for an extension that has nothing to say, which is most of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The sha256 of the artefact this version was resolved to.
    ///
    /// What turns the declaration into a lockfile: a release re-tagged under a
    /// version somebody already has is detected rather than trusted, and a
    /// colleague who clones the repository resolves the same bytes rather than
    /// the same number. `None` for a package with no fixed content to hash,
    /// which is a folder somebody is writing in — that state is honest and is
    /// said out loud rather than filled in with something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    /// Where the package came from: `registry`, `file`, `folder` or
    /// `seeded` — the last written only by builds that shipped archives in the bundle.
    ///
    /// Kept because it is the difference between a dependency and a working
    /// tree. A project declaring one that came from a folder was composed
    /// against code somebody was in the middle of writing, and anybody opening
    /// it elsewhere deserves to know that before wondering why the section is
    /// missing. The path is deliberately not here — it is one machine's, and in
    /// a shared record it is noise at best.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What this extension offers an agent to call, as it declared them.
    ///
    /// Here for the reason [`InstalledExtension::prompt`] is, and it is the
    /// same reason twice: the MCP server is a process with no view of the
    /// catalogue, so a declaration that stayed in the window would be one only
    /// the window could read. The manifest is on this machine; the project
    /// travels, and what an agent may call has to travel with it.
    ///
    /// The package's own name for the function behind a tool is deliberately
    /// not here. It is how the package finds its own code, it changes when the
    /// author renames something, and nothing outside that package can do
    /// anything with it — what travels is what an agent is told.
    ///
    /// Rewritten when the build and the record disagree, the way the prompt is.
    /// Empty for the extensions that offer none, which is most of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDeclaration>,
}

/// One tool an extension offers an agent, as the project records it.
///
/// Three members, and each is read by something that cannot ask for it twice:
/// the name is what a call carries, the description is the whole of what the
/// decision to call it is made on, and the schema is what the arguments are
/// checked against.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    /// What an agent calls it, without the extension's id in front of it.
    pub name: String,
    pub description: String,
    /// The shape of what it takes, as JSON Schema, carried whole.
    ///
    /// Never interpreted on the way through: what an argument means is the
    /// package's business, and a layer between that read one would be a second
    /// opinion about somebody else's schema. Absent is a tool that takes
    /// nothing, which is an ordinary tool.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub input: serde_json::Value,
}

/// An entity on its way from the interface into memory.
///
/// The shape the frontend sends and the host channel forwards, unchanged. It is
/// not [`crate::Entity`]: `fields` is what a product field is called on the way
/// in, and the envelope calls the same thing `extensions`. Turning one into the
/// other is the daemon's job — the window carries this across and never builds
/// an envelope of its own.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityInput {
    pub key: String,
    /// What the record is, as the project spells it — not only the kinds Sync
    /// ships. See [`crate::Entity::kind`].
    pub kind: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<LinkInput>,
    #[serde(default)]
    pub paths_observed: Vec<String>,
    #[serde(default)]
    pub scope_paths: Vec<String>,
    /// Product fields for this kind, validated by the engine against its
    /// published type definition.
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
    /// Where the record is filed, absent for the root.
    ///
    /// For a record whose body is its own. A record whose content is a
    /// repository file has a folder that is its file's directory, and it is
    /// moved by moving the file — `documents.move`, never this.
    #[serde(default)]
    pub folder: Option<String>,
    /// Whether this record *is* the folder it is filed in.
    #[serde(default)]
    pub is_folder: bool,
    /// Whether the record is put away, or `None` to leave it as it is.
    ///
    /// A write states the whole record, so a flag left out of one would be a
    /// flag cleared by it: a record archived last week would come back into
    /// every listing because somebody corrected a sentence in it. `None` is
    /// what keeps that from happening — the writer says nothing about the
    /// archive and nothing about it changes.
    #[serde(default)]
    pub archived: Option<bool>,
    /// Whether the writer checked this claim against the code it covers.
    ///
    /// The one way a record becomes `fresh`. Everything else about freshness is
    /// derived — the engine marks a record stale when the code under its scope
    /// moves, and marks it unverified when its text changes — but nothing
    /// derives *somebody read the code and the claim still holds*, because that
    /// is a judgement rather than a diff. So it is stated, by whoever made it,
    /// on the write that carries it.
    #[serde(default)]
    pub verified: bool,
}

/// One typed link, as the interface states it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkInput {
    pub key: String,
    pub relation: String,
}

impl From<EntityInput> for crate::mapping::Entity {
    fn from(input: EntityInput) -> Self {
        Self {
            key: input.key,
            kind: input.kind,
            title: input.title,
            content: input.content,
            tags: input.tags,
            links: input
                .links
                .into_iter()
                .map(|link| crate::mapping::Link {
                    key: link.key,
                    relation: link.relation,
                })
                .collect(),
            paths_observed: input.paths_observed,
            scope_paths: input.scope_paths,
            extensions: input.fields,
            folder: input.folder,
            is_folder: input.is_folder,
            // Absent means "leave it alone", and an entity carries a flag
            // rather than a question — so what fills it in is whoever knows
            // what the record already says. See `Entity::archived`.
            archived: input.archived.unwrap_or(false),
            verified: input.verified,
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot set itself up has failed, and panicking is the
    // shortest true way to say so.
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;

    /// **What a declaration is worth is what survives the round trip.** The
    /// end-to-end version of this runs a real engine and is skipped on a
    /// machine that has not built one; this is the half that can be checked in
    /// milliseconds, and it is the half that catches the ordinary mistake — a
    /// member renamed on one side of the boundary and read by its old name on
    /// the other, which crosses as nothing and reports nothing.
    #[test]
    fn a_tool_declaration_survives_the_round_trip_under_the_names_it_crosses_by() {
        let written = InstalledExtension {
            id: "acme.tracker".to_owned(),
            version: "1.2.0".to_owned(),
            prompt: None,
            integrity: None,
            source: None,
            tools: vec![ToolDeclaration {
                name: "search_tickets".to_owned(),
                description: "Finds tickets by their words".to_owned(),
                input: json!({"type": "object", "properties": {"words": {"type": "string"}}}),
            }],
        };

        let crossing = serde_json::to_value(&written).expect("it serialises");
        assert_eq!(
            crossing["tools"][0]["name"], "search_tickets",
            "the names on the wire are the ones the other side reads: {crossing}"
        );
        assert_eq!(
            crossing["tools"][0]["description"],
            "Finds tickets by their words"
        );
        assert_eq!(
            crossing["tools"][0]["input"]["properties"]["words"]["type"], "string",
            "the schema crosses whole, to the depth the package wrote it: {crossing}"
        );

        let read: InstalledExtension = serde_json::from_value(crossing).expect("it reads back");
        assert_eq!(read.tools, written.tools);
    }

    /// An extension that offers none writes none — not an empty list, which a
    /// reader would have to know to treat as an absence.
    #[test]
    fn an_extension_that_offers_no_tools_writes_nothing_about_them() {
        let bare = InstalledExtension {
            id: "records".to_owned(),
            version: "1.0.1".to_owned(),
            prompt: None,
            integrity: None,
            source: None,
            tools: Vec::new(),
        };

        let crossing = serde_json::to_value(&bare).expect("it serialises");

        assert!(
            crossing.get("tools").is_none(),
            "nothing to say is said by saying nothing: {crossing}"
        );
    }

    /// And a record written before any of this existed still reads.
    #[test]
    fn a_declaration_from_before_tools_existed_reads_as_offering_none() {
        let older = json!({"id": "records", "version": "1.0.0"});

        let read: InstalledExtension = serde_json::from_value(older).expect("it reads back");

        assert!(read.tools.is_empty());
    }
}
