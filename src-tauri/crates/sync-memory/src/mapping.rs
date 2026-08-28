//! Sync's entities as generic memory envelopes.
//!
//! memory-hub stores one shape: a key, a kind, Markdown content, a title, tags,
//! typed links, source paths, and a free `extensions` object. Sync's entity
//! kinds are that shape with product fields under `extensions` — nothing here
//! invents a parallel storage model, and nothing product-specific leaks into
//! the engine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::dto::Counts;

/// The kind under which type definitions are published. The engine validates
/// writes against these and serves them at `memory://schema`.
pub const TYPE_KIND: &str = "__type__";

/// How long an identifier is allowed to be.
///
/// It is written in prose — in documents, in messages, in prompts — so it has
/// to stay quotable. A name long enough to overflow this is a name the person
/// will shorten anyway.
pub const IDENTIFIER_LIMIT: usize = 32;

/// The identifier a project of this name gets.
///
/// Derived rather than assigned, and derived from the name alone: the same
/// repository opened by two people has to answer to the same word, or a
/// sentence naming a project stops meaning the same thing on the other side of
/// a `git pull`. Nothing about the machine takes part.
///
/// Upper-cased, runs of anything that is not a letter or a digit collapsed to a
/// single `-`, trimmed at both ends. Letters keep their own script: a project
/// called `Мой Проект` answers to `МОЙ-ПРОЕКТ` rather than to a transliteration
/// nobody would guess.
///
/// Returns an empty string when the name holds nothing to build one from —
/// which the caller has to treat as "ask", not as a default.
#[must_use]
pub fn identifier_from_name(name: &str) -> String {
    let mut identifier = String::with_capacity(name.len());
    let mut pending_separator = false;
    for character in name.chars() {
        if character.is_alphanumeric() {
            if pending_separator && !identifier.is_empty() {
                identifier.push('-');
            }
            pending_separator = false;
            identifier.extend(character.to_uppercase());
            if identifier.chars().count() >= IDENTIFIER_LIMIT {
                break;
            }
        } else {
            pending_separator = true;
        }
    }
    identifier
}

/// Whether `identifier` is one this build would have produced.
///
/// The gate for an identifier typed by hand at creation. It is deliberately the
/// same alphabet as [`identifier_from_name`] rather than a looser one: an
/// identifier that could be entered but never derived would be a second class
/// of name, and the day something re-derives one it would silently change.
#[must_use]
pub fn is_identifier(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.chars().count() <= IDENTIFIER_LIMIT
        && identifier == identifier_from_name(identifier)
}

/// The kinds Sync has definitions of its own for.
///
/// This is no longer what a project holds — a project's types are the
/// project's, created in the window or by an agent — but Sync still knows how
/// to describe these, which is what the corpus migration maps the old `.sync/`
/// store onto and what a starter set would be drawn from.
///
/// The engine runs a strict schema: a record whose kind has no `__type__`
/// definition is rejected at write time.
pub const ENTITY_KINDS: &[EntityKind] = &[
    EntityKind::Project,
    EntityKind::Goal,
    EntityKind::Milestone,
    EntityKind::Spec,
    EntityKind::Decision,
    EntityKind::Constraint,
    EntityKind::Observation,
    EntityKind::Question,
    EntityKind::Artifact,
    EntityKind::Doc,
    EntityKind::Comment,
];

/// One of Sync's entity kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Project,
    Goal,
    Milestone,
    Spec,
    Decision,
    Constraint,
    Observation,
    Question,
    Artifact,
    Doc,
    Comment,
}

impl EntityKind {
    /// The string the engine stores as `kind`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Goal => "goal",
            Self::Milestone => "milestone",
            Self::Spec => "spec",
            Self::Decision => "decision",
            Self::Constraint => "constraint",
            Self::Observation => "observation",
            Self::Question => "question",
            Self::Artifact => "artifact",
            Self::Doc => "doc",
            Self::Comment => "comment",
        }
    }

    /// What the kind is called where a person reads it.
    ///
    /// Separate from [`as_str`](Self::as_str) because the two answer different
    /// questions: the identifier is what every record carries and what an agent
    /// writes, and the name is what the window says. They coincide for the kinds
    /// published here only because these were named in one word.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Goal => "Goal",
            Self::Milestone => "Milestone",
            Self::Spec => "Spec",
            Self::Decision => "Decision",
            Self::Constraint => "Constraint",
            Self::Observation => "Observation",
            Self::Question => "Question",
            Self::Artifact => "Artifact",
            Self::Doc => "Doc",
            Self::Comment => "Comment",
        }
    }

    /// A one-line description, published with the type so an agent reading
    /// `memory://schema` learns what the kind is for.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Project => {
                "The project itself: what it is called, what it is, and the language it writes in."
            }
            Self::Goal => "An outcome the project is working towards.",
            Self::Milestone => "A group of specs that together reach part of a goal.",
            Self::Spec => "A unit of work, with acceptance criteria and a status.",
            Self::Decision => "A chosen path among alternatives, with the reason for it.",
            Self::Constraint => "A rule the project must hold to.",
            Self::Observation => "Something found to be true about the system.",
            Self::Question => "An open question, and its answer once it has one.",
            Self::Artifact => "A pointer to something produced outside the memory.",
            Self::Doc => "Long-form prose: architecture, guides, references.",
            Self::Comment => "A remark attached to another entity.",
        }
    }

    /// The mark this kind is drawn with, as a Lucide icon name.
    ///
    /// A type's own definition is what decides its mark — see
    /// [`type_definition`] — and this is where the definitions Sync writes get
    /// theirs. It is also the fallback for a definition that names none: a
    /// corpus written before marks travelled with types still reads as the
    /// kinds people recognise on sight, rather than as a column of neutral
    /// glyphs.
    ///
    /// The name travels, never the drawing: the store is not where a picture
    /// belongs, and the engine has no business validating one.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Project => "folder-git-2",
            Self::Goal => "target",
            Self::Milestone => "flag",
            Self::Spec => "ruler",
            Self::Decision => "signpost",
            Self::Constraint => "lock",
            Self::Observation => "eye",
            Self::Question => "circle-help",
            Self::Artifact => "package",
            Self::Doc => "file-text",
            Self::Comment => "message-square",
        }
    }

    /// The kind this build publishes under that name, if it publishes one.
    ///
    /// A store can hold a kind this build knows nothing about — an older or
    /// newer Sync wrote the corpus — and that is a `None`, not a failure.
    #[must_use]
    pub fn from_kind_name(name: &str) -> Option<Self> {
        ENTITY_KINDS
            .iter()
            .copied()
            .find(|kind| kind.as_str() == name)
    }

    /// Where this kind sits in the published order.
    ///
    /// [`ENTITY_KINDS`] is not alphabetical: it runs from what the project is
    /// working towards, through what it claims, to what it attaches. That order
    /// is the product's own grouping, so it is the order the interface lists
    /// them in.
    #[must_use]
    pub fn position(self) -> usize {
        ENTITY_KINDS
            .iter()
            .position(|kind| *kind == self)
            .unwrap_or(ENTITY_KINDS.len())
    }

    /// Product fields this kind carries, as the engine's schema declares them.
    ///
    /// Only fields worth validating are listed. Anything free-form stays out of
    /// the definition, so the product can grow a field without a schema change
    /// — the strict schema rejects unknown *kinds*, not unknown fields.
    #[must_use]
    pub fn extension_fields(self) -> Map<String, Value> {
        let mut fields = Map::new();
        let mut declare = |name: &str, definition: Value| {
            fields.insert(name.to_owned(), definition);
        };
        match self {
            Self::Project => {
                declare("language", optional_string());
            }
            Self::Spec => {
                declare("status", enumerated(SPEC_STATUSES, true, "backlog"));
                declare("priority", enumerated(PRIORITIES, false, "medium"));
                declare("milestone", optional_string());
                declare(
                    "checklist",
                    json!({"type": "array", "items": {"type": "object"}, "required": false}),
                );
            }
            Self::Goal | Self::Milestone => {
                declare("status", enumerated(SPEC_STATUSES, true, "backlog"));
                declare("horizon", enumerated(HORIZONS, false, "next"));
            }
            Self::Question => {
                declare("status", enumerated(QUESTION_STATUSES, true, "open"));
                // Prose, and the schema has a word for prose. `string` and
                // `text` are the same JSON string to the engine and two
                // different things to a person: an answer to an open question
                // is a paragraph, and a field declared as one line is offered
                // as one line.
                declare("answer", json!({"type": "text", "required": false}));
            }
            Self::Decision | Self::Constraint | Self::Observation | Self::Doc => {
                declare(
                    "validation_state",
                    enumerated(VALIDATION_STATES, true, "unverified"),
                );
            }
            Self::Artifact | Self::Comment => {}
        }
        fields
    }
}

/// Spec, goal and milestone lifecycle.
const SPEC_STATUSES: &[&str] = &[
    "backlog",
    "todo",
    "in_progress",
    "in_review",
    "done",
    "canceled",
];
const QUESTION_STATUSES: &[&str] = &["open", "answered"];
const PRIORITIES: &[&str] = &["low", "medium", "high"];
const HORIZONS: &[&str] = &["now", "next", "later", "someday", "backlog"];
/// How far a claim can be trusted. `stale` and `invalid` are flags, never
/// facts: they mean "verify this against the code before using it".
const VALIDATION_STATES: &[&str] = &["valid", "unverified", "stale", "invalid"];

/// One enumerated field.
///
/// `default` is what a record created in the window starts the field at. It is
/// stated rather than left to be guessed because the first value of a list is
/// not always the one a new record means: the first validation state is `valid`,
/// and a claim nobody has checked is `unverified`.
fn enumerated(values: &[&str], required: bool, default: &str) -> Value {
    json!({
        "type": "enum",
        "values": values,
        "required": required,
        "default": default,
    })
}

/// `required` is stated even though `false` is the engine's default: the schema
/// comes back from `memory://schema` normalised, and a definition that omits a
/// default never compares equal to the one the store holds — which would
/// republish the whole corpus on every open.
fn optional_string() -> Value {
    json!({"type": "string", "required": false})
}

/// A Sync entity on its way into memory.
#[derive(Clone, Debug)]
pub struct Entity {
    /// Stable identity, unique across the project — this becomes the record key
    /// and is what links point at.
    pub key: String,
    /// What the record is, as the project spells it.
    ///
    /// A string rather than [`EntityKind`], because the corpus belongs to the
    /// project. `EntityKind` is only the set Sync ships definitions for; a type
    /// created in the window or published by an extension is a kind this build
    /// has never heard of, and a record of it is an ordinary record. Whether
    /// the kind exists is the engine's to say — its strict schema refuses a
    /// record whose kind has no definition, which is a better answer than one
    /// this enum could give.
    pub kind: String,
    pub title: String,
    /// Markdown body.
    pub content: String,
    pub tags: Vec<String>,
    /// Typed relations to other entities, by key.
    pub links: Vec<Link>,
    /// Files this entity was written against (evidence).
    pub paths_observed: Vec<String>,
    /// Files this entity's scope covers. memory-hub's code-history
    /// reconciliation uses these to mark records stale when the code under them
    /// changes, which is why Sync maps its existing path tracking onto them
    /// rather than keeping a parallel mechanism.
    pub scope_paths: Vec<String>,
    /// Product fields, validated against the kind's `__type__` definition.
    pub extensions: Map<String, Value>,
    /// Where this record is filed, `None` for the root.
    ///
    /// Only ever set for a record whose body is its own. The folder of a record
    /// whose content is a repository file is the directory that file is in, and
    /// writing it here would be this layer stating where somebody's file is
    /// instead of reading it — such a record is moved with `documents.move`,
    /// which moves the file and lets the folder follow.
    pub folder: Option<String>,
    /// Whether this record *is* the folder it is filed in.
    pub is_folder: bool,
}

/// A typed relation between two entities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub key: String,
    pub relation: String,
}

impl Entity {
    /// Render as the envelope the engine stores.
    ///
    /// Product fields sit at the top level rather than under a nested object:
    /// the envelope's `extensions` are flattened on the wire, so that is where
    /// the engine's schema validator looks for them.
    #[must_use]
    pub fn to_envelope(&self) -> Value {
        let mut envelope = json!({
            "envelope_version": {"major": 1, "minor": 0},
            "key": self.key,
            "kind": self.kind,
            "title": self.title,
            "content": self.content,
            "content_hash": content_hash(&self.content),
            "tags": self.tags,
            "links": self.links,
            "source_paths": {
                "observed": self.paths_observed,
                "scope": self.scope_paths,
            },
            "archive": {"archived": false},
            "freshness": {"state": "unverified"},
        });
        // Written only when they say something. An envelope carrying
        // `"folder": null` and `"is_folder": false` on every record would be
        // stating the absence of a hierarchy in every record of a project that
        // has none.
        if let Some(folder) = &self.folder {
            envelope["folder"] = json!(folder);
        }
        if self.is_folder {
            envelope["is_folder"] = json!(true);
        }
        if let Some(object) = envelope.as_object_mut() {
            for (name, value) in &self.extensions {
                object.insert(name.clone(), value.clone());
            }
        }
        envelope
    }

    /// Render as a transaction operation.
    #[must_use]
    pub fn to_put(&self) -> Value {
        json!({"op": "put", "record": {
            "representation": "plaintext",
            "envelope": self.to_envelope(),
        }})
    }
}

/// A delete operation for one key.
#[must_use]
pub fn delete(key: &str) -> Value {
    json!({"op": "delete", "key": key})
}

/// The kinds Sync publishes into every project, and the only ones it does.
///
/// Exactly one: the project's own record has a kind, and the strict schema
/// rejects a record whose kind has no definition — so without this a project
/// could not be created at all. Everything else a project knows is typed by the
/// project, not by this build.
pub const OWN_KINDS: &[EntityKind] = &[EntityKind::Project];

/// Whether a kind is one of Sync's own.
///
/// Sync's own types are republished whenever a project lacks them, so they
/// cannot be missing; and nothing in the interface may offer to delete one,
/// because deleting `project` would leave the record that names the project
/// with a kind the strict schema rejects — a project that cannot be opened.
#[must_use]
pub fn is_own_kind(kind: &str) -> bool {
    OWN_KINDS.iter().any(|own| own.as_str() == kind)
}

/// The definitions Sync publishes for itself.
#[must_use]
pub fn own_type_definitions() -> Vec<Value> {
    OWN_KINDS
        .iter()
        .map(|kind| {
            type_definition(
                &TypeDeclaration::new(kind.as_str(), kind.title(), kind.description(), kind.icon())
                    .with_fields(kind.extension_fields()),
            )
        })
        .collect()
}

/// The `__type__` records for a set of kinds Sync knows how to describe.
///
/// Used by the corpus migration and by the tests; a running project's types
/// come from the project.
#[must_use]
pub fn type_definitions(kinds: &[EntityKind]) -> Vec<Value> {
    kinds
        .iter()
        .map(|kind| {
            type_definition(
                &TypeDeclaration::new(kind.as_str(), kind.title(), kind.description(), kind.icon())
                    .with_fields(kind.extension_fields()),
            )
        })
        .collect()
}

/// Where a type's documents live: a folder of the repository, or nothing at all.
///
/// A definition that names no folder keeps its bodies in its records, which is
/// what every type was before storage was a choice. A definition that names one
/// points at a directory of the working tree: the files are the team's, Git
/// versions them, a pull request shows them in its diff, and Memory writes
/// nothing into them.
///
/// The path is in the definition, so a client that has to build a locator — the
/// file a new document is written to — reads it from the type it already has.
/// One folder per type: a folder holds one type's documents, because a new file
/// in it cannot belong to two types at once.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeStorage {
    /// The directory, relative to the repository root. `None` for a type whose
    /// bodies are part of its records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The definition member the engine reads the folder from.
pub const STORAGE_FIELD: &str = "storage";

impl TypeStorage {
    /// A folder of the repository this type keeps its documents in.
    #[must_use]
    pub fn attached(folder: &str) -> Self {
        Self {
            folder: Some(folder.to_owned()),
        }
    }

    /// What a definition declares, which is nothing for a type whose documents
    /// are its records.
    #[must_use]
    pub fn of_definition(definition: &Value) -> Self {
        Self {
            folder: definition
                .get(STORAGE_FIELD)
                .and_then(Value::as_str)
                .filter(|folder| !folder.is_empty())
                .map(str::to_owned),
        }
    }

    /// The type whose documents are its records.
    #[must_use]
    pub const fn with_records() -> Self {
        Self { folder: None }
    }

    /// Whether this type's documents live somewhere other than its records —
    /// which, for every folder Sync attaches, means files somebody else edits.
    #[must_use]
    pub const fn is_attached(&self) -> bool {
        self.folder.is_some()
    }
}

/// One type definition, as the transaction operation that publishes it.
///
/// The name and the icon travel inside the definition rather than beside it in
/// this build. A type created in the window is a type this build has never
/// heard of, so the only place they can live is with the type — and the engine
/// keeps `content` verbatim, which is what makes that possible.
/// `memory://schema` parses the definition into its own shape and drops what it
/// does not model, so both are read back from the record rather than from the
/// schema.
#[must_use]
pub fn type_definition(declaration: &TypeDeclaration) -> Value {
    let mut definition = json!({
        "kind_name": declaration.kind_name,
        "title": declaration.title,
        "description": declaration.description,
        "icon": declaration.icon,
        "fields": declaration.fields,
        "relationships": declaration.relationships,
    });
    // Both of these are written only when they say something, and for the same
    // reason: a definition that names no folder already means "the bodies are
    // in the records", and one that says nothing to an agent is not the same as
    // one that says nothing *in particular*. A member restating an absence is
    // one more thing a later engine has to keep agreeing with — and, here, one
    // more difference that would republish every definition already stored.
    if let Some(guidance) = &declaration.guidance {
        definition[GUIDANCE_FIELD] = json!(guidance);
    }
    if let Some(folder) = declaration
        .storage
        .as_ref()
        .and_then(|storage| storage.folder.as_ref())
    {
        definition[STORAGE_FIELD] = json!(folder);
    }
    type_record(&declaration.kind_name, &definition)
}

/// The definition member an agent is told about the type from.
///
/// The engine's own, not ours: `memory-hub` parses it off the definition and
/// puts it in front of any client that asks for the schema. Sync writes it so
/// that what a type expects of whoever writes it travels with the type, into
/// the repository, rather than living only in the build that published it.
pub const GUIDANCE_FIELD: &str = "guidance";

/// What a type declares, as whoever publishes it states it.
///
/// A value rather than a longer argument list. Four of these every type states
/// and four only some do, and eight positional arguments at a call site is a
/// place to put a description where an icon goes — the compiler would take it,
/// because they are all strings.
///
/// The three optional halves are what an extension brings and what a type made
/// in the window does not have: the fields its records carry, the relations
/// they may hold, and what an agent is told before it writes one.
#[derive(Clone, Debug, Default)]
pub struct TypeDeclaration {
    /// The identifier every record of the type carries.
    pub kind_name: String,
    /// What a person reads. Several words is normal.
    pub title: String,
    pub description: String,
    /// A Lucide icon name.
    pub icon: String,
    /// What an agent is told before it writes a record of this type.
    pub guidance: Option<String>,
    /// Product fields, as the engine's schema declares them.
    pub fields: Map<String, Value>,
    /// The relations a record of this type may hold: name to
    /// `{target, description}`, where `target` is a kind or `any`. The engine
    /// refuses a link this does not declare, so a type declaring none cannot
    /// link at all.
    pub relationships: Map<String, Value>,
    /// Where the type's documents live. `None` keeps them in the records.
    pub storage: Option<TypeStorage>,
}

impl TypeDeclaration {
    /// The four answers every type gives.
    #[must_use]
    pub fn new(kind_name: &str, title: &str, description: &str, icon: &str) -> Self {
        Self {
            kind_name: kind_name.to_owned(),
            title: title.to_owned(),
            description: description.to_owned(),
            icon: icon.to_owned(),
            ..Self::default()
        }
    }

    /// The product fields records of this type carry.
    #[must_use]
    pub fn with_fields(mut self, fields: Map<String, Value>) -> Self {
        self.fields = fields;
        self
    }

    /// The relations records of this type may hold.
    #[must_use]
    pub fn with_relationships(mut self, relationships: Map<String, Value>) -> Self {
        self.relationships = relationships;
        self
    }

    /// What an agent is told before writing one. Blank is the same as nothing:
    /// a member carrying an empty string would republish every definition that
    /// was written without one.
    #[must_use]
    pub fn with_guidance(mut self, guidance: Option<&str>) -> Self {
        self.guidance = guidance
            .map(str::trim)
            .filter(|guidance| !guidance.is_empty())
            .map(str::to_owned);
        self
    }

    /// A folder of the repository this type keeps its documents in.
    #[must_use]
    pub fn in_storage(mut self, storage: TypeStorage) -> Self {
        self.storage = Some(storage);
        self
    }
}

/// The operation that publishes one definition, whatever the definition says.
///
/// Split from [`type_definition`] because a redefinition is not a fresh
/// definition: the stored one may declare fields, relationships or members a
/// later engine added, and rewriting a type from four arguments would drop
/// everything the four do not name. The caller reads what is there, changes
/// what it means to change, and hands the whole of it back.
#[must_use]
pub fn type_record(kind_name: &str, definition: &Value) -> Value {
    let content =
        serde_json::to_string_pretty(definition).unwrap_or_else(|_| definition.to_string());
    json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": {
            "envelope_version": {"major": 1, "minor": 0},
            "key": type_key(kind_name),
            "kind": TYPE_KIND,
            "title": format!("Type: {kind_name}"),
            "content": content,
            "content_hash": content_hash(&content),
            "tags": ["schema"],
            "links": [],
            "source_paths": {"observed": [], "scope": []},
            "archive": {"archived": false},
            "freshness": {"state": "unverified"},
        }
    }})
}

/// What a document Sync creates in a folder is called.
///
/// A choice rather than a rule now. There is no mask: every file in an attached
/// folder is a document of its type, images and PDFs included, so nothing about
/// the folder says what a *new* document should be. Markdown is what the window
/// writes and what its editor edits, and a person wanting something else makes
/// the file in their own editor — the next scan adopts it.
pub const DOCUMENT_EXTENSION: &str = ".md";

/// What a document Sync creates in a folder holds before anybody types in it.
///
/// A newline rather than nothing, and not by preference: `memory_write_content`
/// refuses an empty body, so a document created empty would be a record
/// pointing at a file that was never written — which the window would then
/// show as a document this branch does not have. One blank line is the smallest
/// truthful thing to put in a file somebody is about to write in.
pub const NEW_DOCUMENT_BODY: &str = "\n";

/// A file name split into the part that may be renumbered and the part that
/// must not be.
///
/// The extension is kept whole rather than slugged into the stem, because it is
/// what the engine reads a document's media type from: `diagram.png` renumbered
/// is `diagram-2.png` and never `diagram-png-2`. The stem is slugged the way a
/// title is, because a name arriving from somebody's desktop may hold spaces,
/// capitals and whatever else their filesystem allowed — so `My Diagram.png`
/// lands as `my-diagram.png`, and only the last dot separates the two halves,
/// which makes `archive.tar.gz` an `archive-tar` and a `.gz`.
#[must_use]
pub fn split_file_name(name: &str) -> (String, String) {
    let trimmed = name.trim();
    let (stem, extension) = match trimmed.rsplit_once('.') {
        // A leading dot is a hidden file rather than an extension: `.gitignore`
        // is one name, not a stem and a suffix, so the whole of it is slugged
        // as the stem and there is no extension to keep off the end.
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, file_extension(ext)),
        _ => (trimmed, String::new()),
    };
    // `document_stem` answers `untitled` for a name it can make nothing of, so
    // there is no empty case left to guard here — a second fallback would be a
    // different word for the same state and only one of them could be the one
    // that happens.
    (document_stem(stem), extension)
}

/// The suffix a file keeps, reduced to what a file name may carry.
///
/// Both halves of the name are spliced into a locator, so both are reduced —
/// this one was not, and a `/` reaching it would have filed the document in a
/// directory nobody named. There is no `..` to worry about, because only the
/// text after the last dot arrives here and it therefore holds no dot at all;
/// what is left to refuse is separators, spaces and a suffix long enough to be
/// a path in disguise.
///
/// Nothing legitimate is lost. An extension is letters and digits — `png`,
/// `mp4`, `gz` — and a name whose tail is anything else keeps its stem and
/// loses a suffix that was never a media type.
fn file_extension(extension: &str) -> String {
    let kept: String = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .take(16)
        .collect();
    if kept.is_empty() {
        String::new()
    } else {
        format!(".{kept}")
    }
}

/// The file name a new document starts life under.
///
/// The title where there is one, and `untitled` where there is not — which is
/// the ordinary case, because a record is created and then named. The file is
/// not renamed when the title changes: renaming a file is something a person
/// does in their editor, and doing it for them would move a document under a
/// colleague's open branch.
#[must_use]
pub fn document_stem(title: &str) -> String {
    let slug: String = title
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "untitled".to_owned()
    } else {
        slug
    }
}

/// The key a new record of an attached folder is filed under.
///
/// The same slug the engine derives when a scan finds a file no record could
/// be: the folder's own prefix off the front, the extension off the back, and
/// everything an identifier cannot carry reduced to a hyphen. Kept in step with
/// `key_for` in `memory-hub-service` on purpose — a person adopting a stray
/// file should get the key the scan would have given it, not one that says a
/// person was involved.
#[must_use]
pub fn reference_key(folder: &str, locator: &str) -> String {
    let below = locator
        .strip_prefix(&format!("{folder}/"))
        .unwrap_or(locator);
    let stem = below.rsplit_once('.').map_or(below, |(stem, _)| stem);
    let slug: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        locator.replace('/', "-")
    } else {
        slug
    }
}

/// The directory a locator is in, or `None` for a file at the repository root.
///
/// The folder of a record whose content is a file is not a field somebody sets:
/// it is where the file is. One fact, one place.
#[must_use]
pub fn folder_of(locator: &str) -> Option<String> {
    locator
        .rsplit_once('/')
        .map(|(folder, _)| folder.to_owned())
        .filter(|folder| !folder.is_empty())
}

/// A record that points at a file rather than holding its text.
///
/// Written when a person says a stray file is a document in its own right. The
/// digest is the one the scan reported for that file, not one computed here:
/// this layer never reads the working tree, and a digest it invented would be a
/// claim about bytes it has not seen.
#[must_use]
pub fn reference_put(key: &str, kind: &str, locator: &str, content_hash: &str) -> Value {
    let mut envelope = json!({
        "envelope_version": {"major": 1, "minor": 0},
        "key": key,
        "kind": kind,
        "content": "",
        "content_hash": content_hash,
        "content_ref": {"path": locator},
        "tags": [],
        "links": [],
        "source_paths": {"observed": [], "scope": []},
        "archive": {"archived": false},
        "freshness": {"state": "unverified"},
    });
    if let Some(folder) = folder_of(locator) {
        envelope["folder"] = json!(folder);
    }
    json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": envelope,
    }})
}

/// The title a Markdown document states for itself, if it states one.
///
/// A document that opens with a heading has already been given a name by
/// whoever wrote it, and taking it is the difference between a list of
/// documents and a list of file stems. `setup.md` is a path; "Setting up a
/// development machine" is what the document is called.
///
/// Only the opening heading counts, and only when it is genuinely the opening:
/// a heading further down names a section, not the document. Front matter is
/// stepped over because it is metadata rather than prose, and a file that
/// begins with a paragraph has no title to take — which is an answer, not a
/// failure, and leaves the record named by its key.
///
/// Both Markdown spellings are read: `# Title`, and a line underlined with
/// `===`. Nothing else is guessed at.
#[must_use]
pub fn heading_of(content: &str) -> Option<String> {
    let body = content.strip_prefix("---\n").map_or(content, |rest| {
        rest.split_once("\n---\n")
            .map_or(rest, |(_front_matter, body)| body)
    });

    let mut lines = body.lines().skip_while(|line| line.trim().is_empty());
    let first = lines.next()?.trim();

    if let Some(heading) = first.strip_prefix("# ") {
        return non_empty(heading.trim().trim_end_matches('#').trim());
    }
    // Setext: the text is the line above, and the rule under it is what makes
    // it a heading. A rule with nothing over it is a horizontal rule.
    let underlined = lines
        .next()
        .is_some_and(|line| !line.trim().is_empty() && line.trim().chars().all(|mark| mark == '='));
    if underlined {
        return non_empty(first);
    }
    None
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

/// Give a record the title its document states, keeping everything else.
///
/// `None` when there is nothing to do: the record already carries a title, or
/// the document states none. Both are ordinary, and neither is worth a write —
/// every write here is a commit on the project's memory.
#[must_use]
pub fn titled_put(stored: &Value, content: &str) -> Option<Value> {
    let envelope = stored.get("envelope").unwrap_or(stored).as_object()?;
    let untitled = envelope
        .get("title")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    if !untitled {
        return None;
    }
    let heading = heading_of(content)?;

    let mut envelope = envelope.clone();
    envelope.insert("title".to_owned(), json!(heading));
    Some(json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": envelope,
    }}))
}

/// Point a record at another file, keeping everything else it holds.
///
/// This is what "yes, that stray file is this document, renamed" writes. The
/// key does not move, so every link pointing at the record still resolves —
/// which is the whole reason the answer is worth asking a person for rather
/// than letting the file become a second record nobody linked to.
///
/// Freshness drops to `unverified` for the same reason an edit in place does:
/// whatever the record claims was checked against a text that has changed.
///
/// `None` when the record has no envelope this build can rewrite.
#[must_use]
pub fn relocate_put(stored: &Value, locator: &str, content_hash: &str) -> Option<Value> {
    let mut envelope = stored
        .get("envelope")
        .unwrap_or(stored)
        .as_object()?
        .clone();

    envelope.insert("content_ref".to_owned(), json!({"path": locator}));
    envelope.insert("content_hash".to_owned(), json!(content_hash));
    match folder_of(locator) {
        Some(folder) => envelope.insert("folder".to_owned(), json!(folder)),
        None => envelope.remove("folder"),
    };
    let mut freshness = member(&envelope, "freshness");
    freshness.insert("state".to_owned(), json!(DEFAULT_FRESHNESS));
    envelope.insert("freshness".to_owned(), Value::Object(freshness));

    Some(json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": envelope,
    }}))
}

/// The operation that rewrites one stored record's title and body.
///
/// The envelope handed back is the stored one with three members replaced, never
/// an envelope rebuilt from the arguments. A record carries scope paths, tags,
/// links, an archive flag, a freshness state and whatever product fields its
/// type declares — none of that is the editor's to discard while somebody edits
/// a sentence, and a rebuild would drop everything a newer engine added.
///
/// Freshness is left exactly as it was found for the same reason it is shown
/// rather than computed: the engine derives it by reconciling code history
/// against the record's scope, so it is the engine's answer and not this layer's
/// to revise because the prose moved.
///
/// `None` when the record has no envelope to change, which is a record this
/// build cannot safely rewrite.
#[must_use]
pub fn document_put(stored: &Value, edits: &DocumentEdits) -> Option<Value> {
    let mut envelope = stored
        .get("envelope")
        .unwrap_or(stored)
        .as_object()?
        .clone();

    if let Some(title) = &edits.title {
        envelope.insert("title".to_owned(), json!(title));
    }
    // A record whose content is a repository file keeps none of it: the bytes
    // belong to the folder, and the digest is what the engine writes after it
    // has written the file. Putting either here would be a second version of
    // the truth, so the body of such a record travels as `memory_write_content`
    // and this operation carries everything else.
    let holds_its_content = !envelope.contains_key("content_ref");
    if let Some(content) = edits.content.as_ref().filter(|_| holds_its_content) {
        envelope.insert("content".to_owned(), json!(content));
        // The engine re-derives this digest and rejects a wrong one, so it is
        // replaced in the same breath as the content it describes.
        envelope.insert("content_hash".to_owned(), json!(content_hash(content)));
    }
    if let Some(tags) = &edits.tags {
        envelope.insert("tags".to_owned(), json!(tags));
    }
    if let Some(links) = &edits.links {
        envelope.insert("links".to_owned(), json!(links));
    }
    // Two lists under one member, so the one that did not change is read back
    // rather than replaced with an empty list.
    if edits.scope.is_some() || edits.observed.is_some() {
        let mut paths = member(&envelope, "source_paths");
        if let Some(scope) = &edits.scope {
            paths.insert("scope".to_owned(), json!(scope));
        }
        if let Some(observed) = &edits.observed {
            paths.insert("observed".to_owned(), json!(observed));
        }
        envelope.insert("source_paths".to_owned(), Value::Object(paths));
    }
    if let Some(archived) = edits.archived {
        let mut archive = member(&envelope, "archive");
        archive.insert("archived".to_owned(), json!(archived));
        envelope.insert("archive".to_owned(), Value::Object(archive));
    }
    // Removed rather than written false: the engine says so only when it is
    // true, and a record carrying `"is_folder": false` would be stating that it
    // is not a folder, which every record that has never been one does not.
    if let Some(is_folder) = edits.is_folder {
        if is_folder {
            envelope.insert("is_folder".to_owned(), json!(true));
        } else {
            envelope.remove("is_folder");
        }
    }
    // Product fields sit at the top level of the envelope, which is where the
    // engine's schema validator looks for them. A field explicitly set to null
    // is one the record no longer carries — that is how an optional field is
    // cleared, and it is not the same as leaving it alone.
    if let Some(fields) = &edits.fields {
        for (name, value) in fields {
            if is_envelope_member(name) {
                continue;
            }
            if value.is_null() {
                envelope.remove(name);
            } else {
                envelope.insert(name.clone(), value.clone());
            }
        }
    }

    Some(json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": envelope,
    }}))
}

/// One envelope member as an object to change, empty when it is missing or is
/// something else.
fn member(envelope: &Map<String, Value>, name: &str) -> Map<String, Value> {
    envelope
        .get(name)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// What an edit of one record may change.
///
/// Every member is optional and means "replace this"; `None` means "leave it
/// alone". That is what makes one command serve a body being typed, a tag being
/// added and a record being archived without any of them writing back a stale
/// copy of the others — the window sends what changed, and nothing else moves.
///
/// What is deliberately absent is the record's identity and the engine's own
/// answer: `key` and `kind` are what every link and every agent refer to and the
/// store has no rename, and `freshness` is derived from code history rather than
/// stated by anybody.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DocumentEdits {
    pub title: Option<String>,
    /// The Markdown body.
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub links: Option<Vec<Link>>,
    /// Paths whose changes make the claim stale.
    pub scope: Option<Vec<String>>,
    /// Paths the claim was written against.
    pub observed: Option<Vec<String>>,
    pub archived: Option<bool>,
    /// Product fields the record's type declares. A `null` value removes one.
    pub fields: Option<Map<String, Value>>,
    /// Whether this record *is* the folder it is filed in.
    ///
    /// Here rather than among the product fields because it is the envelope's,
    /// and editable rather than fixed at birth because describing a folder is
    /// something somebody decides later — usually about a document that was
    /// already there. Setting it on a folder that already has such a record is
    /// refused by the engine, naming the one that is there.
    pub is_folder: Option<bool>,
}

impl DocumentEdits {
    /// Whether this patch would change anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.content.is_none()
            && self.tags.is_none()
            && self.links.is_none()
            && self.scope.is_none()
            && self.observed.is_none()
            && self.archived.is_none()
            && self.is_folder.is_none()
            && self.fields.as_ref().is_none_or(Map::is_empty)
    }

    /// The field names in this patch that are envelope members rather than
    /// product fields.
    ///
    /// A type is free to declare a field called anything, and a definition that
    /// calls one `title` describes a record the engine could not store — the
    /// envelope's own member would be overwritten by it. Named here so the
    /// refusal can say which one rather than dropping it quietly.
    #[must_use]
    pub fn colliding_fields(&self) -> Vec<String> {
        self.fields
            .iter()
            .flat_map(|fields| fields.keys())
            .filter(|name| is_envelope_member(name))
            .cloned()
            .collect()
    }
}

/// Everything a record carries that its type declared, and nothing the envelope
/// owns.
///
/// The envelope flattens product fields onto the top level on the wire, which
/// is where the engine's validator looks for them, so reading them back is that
/// rule in reverse: take out what the envelope owns and what is left is the
/// type's. Which of them a caller wants is the caller's question and is asked
/// after this, not here — a listing names the few it will draw, and a document
/// keeps all of them.
#[must_use]
pub fn product_fields(envelope: &Value) -> Map<String, Value> {
    let mut fields = Map::new();
    if let Some(object) = envelope.as_object() {
        for (name, value) in object {
            if !is_envelope_member(name) {
                fields.insert(name.clone(), value.clone());
            }
        }
    }
    fields
}

/// Whether a name is one of the envelope's own members rather than a product
/// field.
#[must_use]
pub fn is_envelope_member(name: &str) -> bool {
    ENVELOPE_FIELDS.contains(&name)
}

/// The names in a type's field declarations that the envelope has already
/// taken.
///
/// A type may call a field anything, and one called `folder` or `title`
/// describes a record the store cannot hold — the envelope's own member of that
/// name is what would be written instead. Nothing downstream can recover from
/// it: a new record of the type is refused for missing a required field the
/// window is not allowed to send, and a write naming the field is refused for
/// naming an envelope member. Both refusals are about a decision made when the
/// type was published, which is where this belongs and why it is separate from
/// [`DocumentEdits::colliding_fields`] — the same rule, asked of a definition
/// rather than of a patch.
#[must_use]
pub fn colliding_declarations(fields: &Map<String, Value>) -> Vec<String> {
    fields
        .keys()
        .filter(|name| is_envelope_member(name))
        .cloned()
        .collect()
}

/// Whether a record is a type definition rather than a document.
///
/// A `__type__` record is edited through the type sheet and deleted with its
/// records; rewriting its body as prose would leave the project holding a corpus
/// nothing can parse.
///
/// Sync's own kinds are **not** in here. The definition of `project` is Sync's
/// and is republished whenever a project lacks it, but the record that names the
/// project is the project's own data: its title is the project's name, its body
/// is the description, and its `language` field is the language the project
/// writes in. Those are edited, not protected.
#[must_use]
pub fn is_definition_kind(kind: &str) -> bool {
    kind == TYPE_KIND
}

/// Whether a record is one the window may not create or delete.
///
/// Both are true of exactly the records that have to exist and have to be
/// unique: a type definition, which is created and removed as a type, and the
/// project's own record, of which there is one and without which the project
/// could not be opened.
#[must_use]
pub fn is_fixed_record(kind: &str) -> bool {
    // The key used to be part of the test, when a project's record was
    // addressed by a word every project shared. It is addressed by the
    // project's own identifier now, so the kind answers the whole question:
    // this is the record of a `project`, of which there is one.
    is_definition_kind(kind) || is_own_kind(kind)
}

/// A new record of a kind the project holds, with the fields its type declares.
///
/// The definition decides the fields, not this build: a required one the store
/// would reject is filled from what the definition says — its `default`, else
/// the first value of an enumeration, else the empty value of its type — and an
/// optional one is left out entirely. A record created here therefore satisfies
/// the strict schema without the window knowing what any of the fields mean.
#[must_use]
pub fn new_document_put(kind: &str, key: &str, title: &str, definition: &Value) -> Value {
    let mut envelope = Map::new();
    envelope.insert(
        "envelope_version".to_owned(),
        json!({"major": 1, "minor": 0}),
    );
    envelope.insert("key".to_owned(), json!(key));
    envelope.insert("kind".to_owned(), json!(kind));
    envelope.insert("title".to_owned(), json!(title));
    envelope.insert("content".to_owned(), json!(""));
    envelope.insert("content_hash".to_owned(), json!(content_hash("")));
    envelope.insert("tags".to_owned(), json!([]));
    envelope.insert("links".to_owned(), json!([]));
    envelope.insert(
        "source_paths".to_owned(),
        json!({"observed": [], "scope": []}),
    );
    envelope.insert("archive".to_owned(), json!({"archived": false}));

    if let Some(fields) = definition.get("fields").and_then(Value::as_object) {
        for (name, declaration) in fields {
            if is_envelope_member(name) {
                continue;
            }
            if let Some(value) = opening_value(declaration) {
                envelope.insert(name.clone(), value);
            }
        }
    }

    json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": envelope,
    }})
}

/// What a required field starts at, or `None` for a field the record may omit.
///
/// An optional field is left out even when the definition states a default: the
/// default is what a control offers when somebody chooses to fill the field in,
/// and a record carrying a value nobody chose is a record making a claim on their
/// behalf. A required field has no such choice — the store would reject the
/// record without it.
fn opening_value(declaration: &Value) -> Option<Value> {
    if !declaration
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if let Some(default) = declaration.get("default") {
        return Some(default.clone());
    }
    // A required enumeration with no stated default opens at its first value.
    // Which value that is belongs to the definition: the window has no way to
    // know which of `todo` and `done` a project means by "new", and inventing an
    // answer per field name would be a second copy of the corpus.
    if let Some(first) = declaration
        .get("values")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return Some(first.clone());
    }
    Some(match declaration.get("type").and_then(Value::as_str) {
        Some("number" | "integer") => json!(0),
        Some("boolean") => json!(false),
        Some("array") => json!([]),
        Some("object") => json!({}),
        _ => json!(""),
    })
}

/// A key for a new record: the kind it is, then six hex of a digest.
///
/// `decision-` and `observation-`, each with six hex after it. The kind is
/// spelled out rather than abbreviated to its first letter, because that letter
/// answered the question only by accident: `decision` and `doc` share one, and
/// every kind an extension publishes shares whatever letter the extension's
/// name starts with — two kinds from one package would both be `p-`, which
/// names the package in a place nobody is asking about it.
///
/// What names the record is the last segment of the kind, for the same reason:
/// the namespace in front of it is identical on every record that extension
/// writes and so says nothing about which of them this is. A key is read far
/// more often than it is typed — in a link, in a listing, in a sentence an
/// agent writes — and a key that says what it is saves the read that answers it.
///
/// Derived rather than random because nothing in this crate carries a random
/// number generator and a caller that has to retry needs the second attempt to
/// differ from the first. `seed` is what makes it differ.
///
/// Keys are permanent and there is no rename, so this decides what *new*
/// records are called and nothing else: a corpus written before it keeps the
/// keys it has, and both shapes go on being ordinary keys.
#[must_use]
pub fn suggested_key(kind: &str, seed: &str) -> String {
    let digest = content_hash(&format!("{kind}/{seed}"));
    let hex: String = digest.chars().skip("sha256:".len()).take(6).collect();
    format!("{}-{hex}", key_stem(kind))
}

/// The part of a kind that names a record, spelled the way a key may be.
///
/// Its own slug rather than [`document_stem`]'s: that one names a file in
/// somebody's repository and answers to what a filesystem accepts, and this one
/// names a record in a url. They agree today and are free to stop.
fn key_stem(kind: &str) -> String {
    let named = kind.rsplit('.').next().unwrap_or(kind);
    let slug: String = named
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    // A kind with nothing an identifier can carry. The engine would take it and
    // the window will never make one, so this is a key rather than a refusal —
    // failing to name a record is not a reason to fail to create it.
    if slug.is_empty() {
        "record".to_owned()
    } else {
        slug
    }
}

/// Where a type's definition lives. One place says it, so a read, a write and a
/// delete cannot disagree about the key.
#[must_use]
pub fn type_key(kind_name: &str) -> String {
    format!("{TYPE_KIND}/{kind_name}")
}

/// The definition inside a `__type__` record, as an object to be changed.
///
/// A definition that cannot be parsed comes back empty rather than as an error:
/// the record is still a type the project holds, and rewriting it from what the
/// interface knows is better than refusing to touch it.
#[must_use]
pub fn definition_of(record: &Value) -> Map<String, Value> {
    record
        .get("envelope")
        .unwrap_or(record)
        .get("content")
        .and_then(Value::as_str)
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .and_then(|definition| match definition {
            Value::Object(members) => Some(members),
            _ => None,
        })
        .unwrap_or_default()
}

/// One record that holds on to another.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependent {
    pub key: String,
    pub kind: String,
    pub title: String,
    /// The relation the link declares, when it is a link rather than a mention.
    pub relation: Option<String>,
}

/// What holds on to a record, split by how it holds on.
///
/// The split is what a confirmation needs in order to say something true. A
/// record in `links` names this one structurally: delete the target and the link
/// points at nothing. A record in `mentions` talks about it in prose, and
/// deleting that record because it named another would delete the reasoning
/// along with the conclusion.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Dependents {
    pub links: Vec<Dependent>,
    pub mentions: Vec<Dependent>,
}

/// A type the project holds, as the interface lists it.
///
/// Read from the project's own `__type__` records rather than from this build:
/// the corpus belongs to the project, and a type created in the window is one
/// this build has never heard of. Its name, its description and the mark it is
/// drawn with all come from the record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordType {
    /// The identifier: what every record of this type carries, what an agent
    /// writes, and what the definition's key is built from. It is generated
    /// from the name when a person adds a type, and carries a prefix when an
    /// extension brings one — either way it is stored, never re-derived.
    pub kind: String,
    /// What the type is called where a person reads it. More than one word is
    /// normal, and it can be changed without touching a single record.
    pub title: String,
    pub description: String,
    /// A Lucide icon name, or `None` for a definition that names none. The
    /// interface draws those with a neutral mark rather than borrowing another
    /// type's.
    pub icon: Option<String>,
    /// How many product fields the definition declares.
    pub field_count: usize,
    /// The fields the definition declares, exactly as the store spells them:
    /// name to declaration, where a declaration says a `type`, whether it is
    /// `required`, its `values` when it enumerates them, and its `default` when
    /// it states one.
    ///
    /// Carried verbatim rather than parsed into a shape of this build's: the
    /// schema is published at runtime, so a build that modelled it would be a
    /// second copy going out of date the day a type changes. What reads this
    /// generates a control from what the declaration says and shows the rest as
    /// text.
    pub fields: Map<String, Value>,
    /// The relations the definition declares: name to `{target, description}`,
    /// where `target` is a kind name or `any`.
    ///
    /// The engine validates every link against these and rejects a relation a
    /// type does not declare, so this is not decoration — it is the list of links
    /// a record of this type is allowed to hold, and a type that declares none
    /// cannot link at all.
    pub relationships: Map<String, Value>,
    /// What an agent is told before it writes a record of this type, or `None`
    /// for a definition that says nothing.
    ///
    /// Read here rather than only written because a type's guidance is part of
    /// what the project holds: it was published with the type, it travels with
    /// the repository, and something that could publish it but never read it
    /// back could not say whether it landed.
    pub guidance: Option<String>,
    /// Where this type's records keep their content: with the records
    /// themselves, or in a storage the project declared over a folder of the
    /// repository the team edits directly.
    ///
    /// Not a detail of storage the interface can leave unsaid. It decides
    /// whether a document has a file behind it, whether its folder is a name
    /// or a directory, and whether the body being edited is one somebody else
    /// may be editing at the same time in their IDE.
    pub storage: TypeStorage,
    /// Whether a document of this type can be written at all, as the engine
    /// answers it.
    ///
    /// Asked before offering to create rather than discovered from a failure: a
    /// type keeping its content in its records is always writable, and one
    /// pointing at a folder is only as writable as that folder — which may be
    /// read-only, or may not be there.
    pub writable: bool,
    /// True for a type Sync publishes and maintains. It is always in the
    /// corpus, and nothing may offer to remove it.
    pub own: bool,
}

impl RecordType {
    /// Read a `__type__` record.
    ///
    /// The definition is JSON inside the record's `content`, which is where the
    /// engine keeps it verbatim. A definition that cannot be parsed is still a
    /// type the project holds, so the name is taken from the key and the rest
    /// is left empty rather than the row being dropped.
    #[must_use]
    pub fn from_record(record: &Value) -> Option<Self> {
        let envelope = record.get("envelope").unwrap_or(record);
        let key = envelope.get("key").and_then(Value::as_str)?;
        let named = key.strip_prefix(&format!("{TYPE_KIND}/")).unwrap_or(key);
        let definition: Value = envelope
            .get("content")
            .and_then(Value::as_str)
            .and_then(|content| serde_json::from_str(content).ok())
            .unwrap_or(Value::Null);
        let text = |field: &str| {
            definition
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let kind_name = text("kind_name").unwrap_or_else(|| named.to_owned());
        Some(Self {
            icon: text("icon")
                .filter(|icon| !icon.is_empty())
                // A corpus written before marks travelled with types names none,
                // and so does one written by hand. Where the definition is
                // silent and the name is one Sync knows how to describe, Sync's
                // own mark is used: the alternative is a window full of neutral
                // glyphs for the kinds people recognise on sight. The
                // definition always wins where it speaks.
                .or_else(|| {
                    EntityKind::from_kind_name(&kind_name).map(|known| known.icon().to_owned())
                }),
            title: text("title")
                .filter(|title| !title.trim().is_empty())
                // The same fallback the mark takes, for the same reason. A
                // corpus written before types carried a name of their own — or
                // one written by hand, or by an agent — has only the identifier
                // to go on: Sync's own name where it knows the kind, and
                // otherwise the identifier made readable. Never a blank row.
                .or_else(|| {
                    EntityKind::from_kind_name(&kind_name).map(|known| known.title().to_owned())
                })
                .unwrap_or_else(|| readable(&kind_name)),
            description: text("description").unwrap_or_default(),
            kind: kind_name.clone(),
            field_count: definition
                .get("fields")
                .and_then(Value::as_object)
                .map_or(0, serde_json::Map::len),
            fields: definition
                .get("fields")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            relationships: definition
                .get("relationships")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            guidance: text(GUIDANCE_FIELD).filter(|guidance| !guidance.trim().is_empty()),
            storage: TypeStorage::of_definition(&definition),
            // What the definition alone cannot answer: whether the storage it
            // names can be written is a question about the storage, not about
            // the type, and only the engine can be asked. Read from a record,
            // a type is assumed writable until `memory_list_types` says
            // otherwise — see `MemoryClient::list_types`.
            writable: true,
            own: is_own_kind(&kind_name),
        })
    }
}

/// An identifier made readable, for a type that never said what it is called.
///
/// Underscores are word breaks and an extension's prefix is dropped: a kind
/// arriving as `review.open_question` is a type called "Open question" until its
/// definition says otherwise. This is a last resort, not a naming scheme — a
/// type that carries a name is shown by it, whatever it looks like.
fn readable(kind: &str) -> String {
    let bare = kind.rsplit(['.', ':', '/']).next().unwrap_or(kind);
    let spaced = bare.replace('_', " ");
    let mut characters = spaced.chars();
    characters.next().map_or_else(
        || kind.to_owned(),
        |first| first.to_uppercase().collect::<String>() + characters.as_str(),
    )
}

/// The project's types, in the order the navigator lists them.
///
/// Kinds Sync knows how to describe come first, in the order it describes them;
/// the project's own follow, by name. Alphabetical throughout would bury the
/// project's own record among types somebody added this morning, and creation
/// order is not a thing the store keeps.
///
/// The tie-break is the name rather than the identifier, because the name is
/// what the list is read down. Two types whose names match sort by identifier,
/// which is the only thing left that cannot be equal.
pub fn sort_types(types: &mut [RecordType]) {
    types.sort_by(|left, right| {
        let position = |entry: &RecordType| {
            EntityKind::from_kind_name(&entry.kind).map_or(ENTITY_KINDS.len(), EntityKind::position)
        };
        position(left)
            .cmp(&position(right))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.kind.cmp(&right.kind))
    });
}

/// One record, as the interface lists it.
///
/// A list row does not carry the record's body: the store holds Markdown, and a
/// column that renders a paragraph clipped to one line is showing neither the
/// claim nor its absence. What it does carry is what a row is scanned for — the
/// title, the type, how far the claim can still be trusted, and the part of the
/// repository it is about.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordEntry {
    pub key: String,
    pub kind: String,
    pub title: String,
    /// The product fields this row was asked for, in the store's own words.
    ///
    /// A row carries fields because a column that groups by one cannot open
    /// every record to find it — but it carries the ones the caller named and
    /// no others, because a type may declare a `text` field and a list has no
    /// use for a page of prose per row.
    ///
    /// Left off the wire entirely when it is empty, which is the ordinary case:
    /// a row nobody asked fields of should not carry a member saying so, and
    /// the shape a caller reads then matches the shape a caller may build.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
    /// `fresh`, `unverified`, `stale` or `invalid`, in the engine's own words.
    /// `stale` and `invalid` mean the code moved under the claim.
    pub freshness: String,
    /// The paths this record's scope covers. Empty is a real answer: a claim
    /// about no particular file is what an unscoped decision looks like.
    pub scope: Vec<String>,
    pub archived: bool,
    pub tags: Vec<String>,
    /// The repository-relative path of the file holding this record's content,
    /// for a record whose content is not its own.
    ///
    /// `None` is the ordinary case: the record carries its body.
    pub locator: Option<String>,
    /// What the document is, as the engine read it off the file name — never
    /// off the bytes, because reading every document on every scan would cost
    /// what a scan exists to avoid.
    ///
    /// `None` means nobody said. There is no mask any more, so an attached
    /// folder holds whatever is in it: this is what tells a diagram apart from
    /// a page of prose before anything tries to show it as text.
    pub media_type: Option<String>,
    /// Whether the content is here, and if not, why not: `present`,
    /// `not_on_branch` or `removed`.
    ///
    /// Memory does not branch and code does, so the corpus holds every
    /// branch's documents and the checked-out branch decides which are real
    /// right now. `not_on_branch` is routine and `removed` is the one case
    /// worth asking a person about — which is why they are two words and not
    /// one flag.
    pub presence: String,
    /// Where this record is filed — a path of segments, `None` for the root.
    ///
    /// A name and never a location: in `refs` the tree stays flat and the
    /// folder is metadata somebody sets. For a record whose content is a
    /// repository file it is the directory that file is in, and the two may not
    /// disagree — which is why moving such a record is the engine's operation
    /// and never a field this layer writes on its own.
    pub folder: Option<String>,
    /// Whether this record *is* the folder it is filed in.
    ///
    /// A folder is a name until somebody gives it a title and a text of its
    /// own, and that is this flag. It matters to whatever draws a tree and to
    /// nothing else: a client unaware of it shows the record as an ordinary
    /// document of its folder, which it also is. The engine says so only when
    /// it is true.
    pub is_folder: bool,
}

/// What a record without a stated freshness is treated as.
///
/// Not a fact and not a failure — the same thing a record gets when it is
/// written and nothing has verified it since.
const DEFAULT_FRESHNESS: &str = "unverified";

/// The presence of a record whose content is its own: it is here, always.
pub const PRESENT: &str = "present";

/// The checked-out commit does not have this document at all — another branch
/// does. Hidden by default, and never a reason to delete anything.
pub const NOT_ON_BRANCH: &str = "not_on_branch";

/// The branch that owns the document has it and the working tree does not:
/// somebody deleted the file. The one absence a person is asked about.
pub const REMOVED: &str = "removed";

impl RecordEntry {
    /// Read a listed record, whichever shape it arrived in.
    ///
    /// The engine flattens `freshness` and `archived` for a metadata-only
    /// listing and nests them under `freshness.state` and `archive.archived`
    /// for a full one, and a record read by key arrives wrapped in an envelope.
    /// All three are the same record, so all three are accepted here rather
    /// than at three call sites.
    ///
    /// Returns `None` for anything without a key and a kind: that is not a
    /// record, and inventing values for it would put a row in the interface
    /// that the store cannot be asked about.
    #[must_use]
    pub fn from_record(record: &Value) -> Option<Self> {
        let record = record.get("envelope").unwrap_or(record);
        let text = |field: &str| record.get(field).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            key: text("key")?,
            kind: text("kind")?,
            title: text("title").unwrap_or_default(),
            // Empty here, always. Which fields a row carries is the caller's
            // question and is answered where it was asked — see
            // [`product_fields`]. Filling them in unasked would build a map per
            // row for the listings that draw none, which is most of them.
            fields: Map::new(),
            freshness: text("freshness")
                .or_else(|| {
                    record
                        .get("freshness")
                        .and_then(|freshness| freshness.get("state"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| DEFAULT_FRESHNESS.to_owned()),
            scope: strings(record.pointer("/source_paths/scope")),
            archived: record
                .pointer("/archive/archived")
                .or_else(|| record.get("archived"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            tags: strings(record.get("tags")),
            locator: record
                .pointer("/content_ref/path")
                .and_then(Value::as_str)
                .map(str::to_owned),
            media_type: text("media_type"),
            // Two shapes, one fact. A listing flattens the locator's presence
            // onto the record; a record read by key carries the whole envelope,
            // where it sits inside `content_ref` and is omitted entirely while
            // the document is here. Both are accepted, so no caller has to know
            // which read it is holding.
            presence: text("presence")
                .or_else(|| {
                    record
                        .pointer("/content_ref/presence")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| PRESENT.to_owned()),
            folder: text("folder"),
            // Said only when true, so its absence is the answer rather than a
            // shape this build has not heard of.
            is_folder: record
                .get("is_folder")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Whether this record is one of the schema definitions rather than
    /// knowledge the project stated.
    #[must_use]
    pub fn is_schema(&self) -> bool {
        self.kind == TYPE_KIND
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The envelope's own fields.
///
/// Everything else a record carries at its top level is a product field: the
/// envelope flattens `extensions` on the wire, which is where the engine's
/// validator looks for them. Reading them back is the same rule in reverse —
/// take out what the envelope owns, and what is left is what the type declared.
const ENVELOPE_FIELDS: &[&str] = &[
    "envelope_version",
    "key",
    "kind",
    "title",
    "content",
    "content_hash",
    "tags",
    "links",
    "source_paths",
    "archive",
    "freshness",
    // The members a record gained when content stopped having to live inside
    // it. They are the envelope's, not a type's, so a document view that
    // listed them as product fields would offer to edit the engine's own
    // bookkeeping — and a folder is moved by moving a file, never by typing a
    // path into a panel.
    "content_ref",
    "folder",
    "is_folder",
    "profile",
];

/// One record, whole: what a document view shows and what its metadata panel
/// describes.
///
/// The flags are four separate facts the engine states separately — archived,
/// the body is not text, the body is not here, this record is its folder — and
/// none of them implies another. Gathering them into a state enum would be
/// inventing combinations the store never reports; gathering them into a
/// sub-struct would be a shape with no name anybody uses.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub key: String,
    pub kind: String,
    pub title: String,
    /// The Markdown body, exactly as stored. Rendering is the window's job and
    /// editing will be the editor's; neither belongs here.
    pub content: String,
    pub freshness: String,
    /// Paths the claim's scope covers — what turns it stale when code moves.
    pub scope: Vec<String>,
    /// Paths it was written against.
    pub observed: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<Link>,
    pub archived: bool,
    /// The fields this record's type declares, in the store's own words. Kept
    /// as JSON rather than typed per kind: the schema is published at runtime,
    /// so a build that types them here would be a second copy of it.
    pub fields: Map<String, Value>,
    /// The file this record's content lives in, when it lives outside.
    pub locator: Option<String>,
    /// What the document is, from its file name: `text/markdown`, `image/png`.
    pub media_type: Option<String>,
    /// True when the body is not text — an image, a PDF, anything a folder
    /// holds now that there is no mask to keep it out.
    ///
    /// The bytes are deliberately not carried: a window that cannot edit them
    /// has no use for them, and a base64 string rendered as prose is worse than
    /// saying plainly what the document is. What reads this says so and leaves
    /// the file alone.
    pub content_binary: bool,
    /// `present`, `not_on_branch` or `removed`.
    pub presence: String,
    /// Whether the body could not be read because the file is not here.
    ///
    /// Distinct from an empty document, and the distinction is the point: an
    /// empty file is something somebody wrote, and a missing one is a document
    /// this branch does not have. Showing the second as the first would invite
    /// somebody to "fix" it by typing into it, which would write a file over
    /// the top of a document that exists elsewhere.
    pub content_missing: bool,
    /// Where this record is filed, `None` for the root. See
    /// [`RecordEntry::folder`].
    pub folder: Option<String>,
    /// Whether this record is the folder it is filed in. See
    /// [`RecordEntry::is_folder`].
    pub is_folder: bool,
}

impl Document {
    /// Read a stored record, envelope or bare.
    ///
    /// Returns `None` for anything without a key and a kind — the same rule as
    /// [`RecordEntry::from_record`], for the same reason.
    #[must_use]
    pub fn from_record(record: &Value) -> Option<Self> {
        let row = RecordEntry::from_record(record)?;
        let envelope = record.get("envelope").unwrap_or(record);
        // A document keeps every one of them: it is the record itself, and the
        // panel beside it draws whatever the type declared.
        let fields = product_fields(envelope);
        Some(Self {
            key: row.key,
            kind: row.kind,
            title: row.title,
            content: envelope
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            freshness: row.freshness,
            scope: row.scope,
            observed: strings(envelope.pointer("/source_paths/observed")),
            tags: row.tags,
            links: envelope
                .get("links")
                .and_then(Value::as_array)
                .map(|links| {
                    links
                        .iter()
                        .filter_map(|link| serde_json::from_value(link.clone()).ok())
                        .collect()
                })
                .unwrap_or_default(),
            archived: row.archived,
            fields,
            locator: row.locator,
            media_type: row.media_type,
            // Both settled by whoever reads the body: this shape only knows
            // what the record says, and what a file holds is a question about
            // the working tree.
            content_binary: false,
            presence: row.presence,
            content_missing: false,
            folder: row.folder,
            is_folder: row.is_folder,
        })
    }

    /// Whether this record's body lives in a file rather than in the record.
    #[must_use]
    pub const fn is_reference(&self) -> bool {
        self.locator.is_some()
    }
}

/// How much knowledge the project holds, by type and by trust.
///
/// These are counts over the whole corpus rather than over any page of it,
/// because the navigator lists every type while the workspace is showing one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordsCounts {
    pub total: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub by_freshness: BTreeMap<String, usize>,
}

impl RecordsCounts {
    /// Everything the project holds, minus the kinds the window is not showing.
    ///
    /// The engine counts `__type__` records like any other, so a project with
    /// eleven published definitions and nothing else would report eleven
    /// unverified claims it never made; a hidden kind would go on being counted
    /// under a type that is no longer in the list. There is no "every kind
    /// except these" filter, so each exclusion is counted on its own and
    /// subtracted here.
    #[must_use]
    pub fn excluding(everything: &Counts, excluded: &[Counts]) -> Self {
        let mut counts = Self {
            total: everything.total,
            by_kind: everything.by_kind.clone(),
            by_freshness: everything.by_freshness.clone(),
        };
        for removed in excluded {
            counts.total = counts.total.saturating_sub(removed.total);
            for kind in removed.by_kind.keys() {
                counts.by_kind.remove(kind);
            }
            for (state, removed_count) in &removed.by_freshness {
                let remaining = counts
                    .by_freshness
                    .get(state)
                    .copied()
                    .unwrap_or_default()
                    .saturating_sub(*removed_count);
                if remaining == 0 {
                    counts.by_freshness.remove(state);
                } else {
                    counts.by_freshness.insert(state.clone(), remaining);
                }
            }
        }
        counts
    }
}

/// What the Records column needs to draw itself once.
///
/// The counts describe the whole corpus and the records describe the selection,
/// which is the asymmetry the column is built on: the navigator lists every type
/// with how much of it there is, while the workspace shows one of them.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordsPage {
    pub revision: String,
    pub counts: RecordsCounts,
    pub records: Vec<RecordEntry>,
    /// True when the selection holds more than this page. The interface says so
    /// rather than presenting a truncated list as the whole of it.
    pub has_more: bool,
}

/// Whether the store holds Sync's own definitions exactly as this build writes
/// them.
///
/// Compared against the `__type__` records rather than against
/// `memory://schema`: the engine parses a definition into its own shape and
/// drops what it does not model — the mark among it — so a definition that has
/// lost its icon would look identical to one that never had it, and the corpus
/// would never heal.
///
/// The comparison is the content digest of the whole definition. Anything that
/// differs — a reworded description, a renamed field, a mark added by a later
/// version of Sync — is a difference, and a field-by-field check would have to
/// be extended every time the shape grows.
///
/// Every other kind in the store is the project's own and is not looked at:
/// deciding to change or drop one is not a side effect of opening a window.
#[must_use]
pub fn corpus_matches(records: &[Value]) -> bool {
    OWN_KINDS.iter().all(|kind| {
        let expected = type_definition(
            &TypeDeclaration::new(kind.as_str(), kind.title(), kind.description(), kind.icon())
                .with_fields(kind.extension_fields()),
        );
        let expected = &expected["record"]["envelope"];
        records.iter().any(|record| {
            let stored = record.get("envelope").unwrap_or(record);
            stored["key"] == expected["key"] && stored["content_hash"] == expected["content_hash"]
        })
    })
}

/// Whether the store already holds this definition, exactly as written.
///
/// The same comparison [`corpus_matches`] makes, for one definition that came
/// from outside rather than for Sync's own set: a key and the digest of its
/// content. A definition whose text changed has a different digest and is
/// republished; one that did not is left alone, because every write is a commit
/// and republishing an identical definition would put one on `refs/memory/*`
/// each time a project that declares it is opened.
#[must_use]
pub fn already_published(records: &[Value], definition: &Value) -> bool {
    let expected = &definition["record"]["envelope"];
    records.iter().any(|record| {
        let stored = record.get("envelope").unwrap_or(record);
        stored["key"] == expected["key"] && stored["content_hash"] == expected["content_hash"]
    })
}

/// The digest the envelope contract requires: `sha256:` and 64 lowercase hex
/// digits over the content. The engine re-derives it on write, so a wrong one
/// is rejected rather than stored.
#[must_use]
pub fn content_hash(content: &str) -> String {
    bytes_hash(content.as_bytes())
}

/// The same digest over content that is not text.
///
/// A document of an attached folder is whatever is in the folder — a picture,
/// a PDF — so the digest has to be taken over bytes. Text is bytes too, which
/// is why [`content_hash`] is this function with one step in front of it
/// rather than a second implementation of the same hash.
#[must_use]
pub fn bytes_hash(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {

    #![allow(clippy::unwrap_used)]

    #[test]
    fn an_identifier_is_the_name_in_one_word() {
        assert_eq!(identifier_from_name("Sync"), "SYNC");
        assert_eq!(identifier_from_name("Play Demo"), "PLAY-DEMO");
        assert_eq!(identifier_from_name("  a  b  "), "A-B");
        assert_eq!(identifier_from_name("v2.0 / rewrite"), "V2-0-REWRITE");
    }

    #[test]
    fn letters_keep_their_own_script() {
        assert_eq!(identifier_from_name("Мой Проект"), "МОЙ-ПРОЕКТ");
    }

    #[test]
    fn a_name_with_nothing_to_build_on_derives_nothing() {
        assert_eq!(identifier_from_name("   "), "");
        assert_eq!(identifier_from_name("--"), "");
        assert!(!is_identifier(""));
    }

    #[test]
    fn an_identifier_is_one_this_build_would_have_derived() {
        assert!(is_identifier("SYNC"));
        assert!(is_identifier("PLAY-DEMO"));
        // Lower case, a trailing separator, a space: all of them derive to
        // something else, and something else is not this identifier.
        assert!(!is_identifier("sync"));
        assert!(!is_identifier("PLAY-"));
        assert!(!is_identifier("PLAY DEMO"));
    }

    #[test]
    fn an_identifier_stops_at_the_limit_it_states() {
        let derived = identifier_from_name(&"a".repeat(IDENTIFIER_LIMIT + 10));
        assert_eq!(derived.chars().count(), IDENTIFIER_LIMIT);
        assert!(is_identifier(&derived));
    }

    use super::*;

    #[test]
    fn what_the_envelope_owns_is_never_a_product_field() {
        let fields = product_fields(&json!({
            "key": "task-3ad25f",
            "kind": "tasks.task",
            "title": "Fix the login redirect loop",
            "content": "## What to do\n",
            "tags": ["login"],
            "archive": {"archived": false},
            "freshness": {"state": "unverified"},
            "status": "in_review",
            "priority": "high",
        }));

        // By what is in it, never by the order: `sync-mcp` turns on serde_json's
        // `preserve_order` and Cargo unifies features across a workspace build,
        // so these keys come back sorted when this crate is tested alone and in
        // insertion order when it is tested beside that one. A test that passed
        // on its own and failed in the suite is worse than no test.
        let mut names: Vec<&String> = fields.keys().collect();
        names.sort();
        assert_eq!(names, ["priority", "status"]);
        // The two that would be worst to hand back as the type's own: a record
        // whose `title` looked like a product field is one a panel would offer
        // to edit twice, in two places, writing to one of them.
        assert!(!fields.contains_key("title"));
        assert!(!fields.contains_key("tags"));
    }

    #[test]
    fn a_row_carries_no_fields_until_somebody_asks_for_them() {
        let row = RecordEntry::from_record(&json!({
            "key": "task-3ad25f",
            "kind": "tasks.task",
            "title": "Fix the login redirect loop",
            "status": "in_review",
        }))
        .unwrap();

        assert_eq!(row.kind, "tasks.task");
        assert!(
            row.fields.is_empty(),
            "a listing of two hundred rows must not build two hundred maps for the columns \
             that draw none of them"
        );
    }

    #[test]
    fn a_file_keeps_its_extension_and_is_slugged_around_it() {
        assert_eq!(
            split_file_name("My Diagram.PNG"),
            ("my-diagram".to_owned(), ".png".to_owned())
        );
        // Only the last dot separates the halves.
        assert_eq!(
            split_file_name("archive.tar.gz"),
            ("archive-tar".to_owned(), ".gz".to_owned())
        );
        // A leading dot is a name, not a suffix.
        assert_eq!(
            split_file_name(".gitignore"),
            ("gitignore".to_owned(), String::new())
        );
        // A name with nothing in it to slug falls to the same word an untitled
        // document does, which is the one `document_stem` already answers with.
        assert_eq!(
            split_file_name("   "),
            ("untitled".to_owned(), String::new())
        );
    }

    #[test]
    fn nothing_in_a_file_name_can_choose_a_directory() {
        // Both halves are spliced into a locator. The stem was already reduced;
        // the extension is the half that used to travel whole, and a separator
        // in it would have filed the document somewhere nobody named.
        let (stem, extension) = split_file_name("shot.png/../../etc/passwd");
        assert!(
            !stem.contains('/') && !extension.contains('/'),
            "no separator survives: {stem}{extension}"
        );
        assert!(
            !stem.contains("..") && !extension.contains(".."),
            "and nothing climbs: {stem}{extension}"
        );

        // A tail that is not a media type is dropped rather than kept as one.
        assert_eq!(
            split_file_name("notes./etc/passwd"),
            ("notes".to_owned(), ".etcpasswd".to_owned())
        );
        assert_eq!(
            split_file_name("picture.\\..\\windows"),
            ("picture".to_owned(), ".windows".to_owned())
        );
    }

    #[test]
    fn an_entity_becomes_an_envelope_with_its_product_fields_under_extensions() {
        let mut extensions = Map::new();
        extensions.insert("status".to_owned(), json!("in_progress"));
        let entity = Entity {
            key: "s-sidecar".to_owned(),
            kind: EntityKind::Spec.as_str().to_owned(),
            title: "Drive memory-hub as a sidecar".to_owned(),
            content: "# Body".to_owned(),
            tags: vec!["memory".to_owned()],
            links: vec![Link {
                key: "d-be7536".to_owned(),
                relation: "decided_by".to_owned(),
            }],
            paths_observed: vec!["src-tauri/src/lib.rs".to_owned()],
            scope_paths: vec!["src-tauri/".to_owned()],
            extensions,
            folder: None,
            is_folder: false,
        };

        let envelope = entity.to_envelope();

        assert_eq!(envelope["kind"], "spec");
        assert_eq!(
            envelope["status"], "in_progress",
            "product fields are flattened, which is where the schema looks"
        );
        assert_eq!(
            envelope["content_hash"],
            content_hash("# Body"),
            "the engine re-derives this digest and rejects a wrong one"
        );
        assert_eq!(
            envelope["source_paths"]["scope"][0], "src-tauri/",
            "scope paths drive code-history reconciliation"
        );
        assert_eq!(envelope["links"][0]["relation"], "decided_by");
    }

    #[test]
    fn rewriting_a_document_keeps_everything_it_was_not_asked_to_change() {
        let stored = json!({
            "representation": "plaintext",
            "envelope": {
                "envelope_version": {"major": 1, "minor": 0},
                "key": "d-a396db",
                "kind": "decision",
                "title": "Old title",
                "content": "Old body",
                "content_hash": content_hash("Old body"),
                "tags": ["shell"],
                "links": [{"key": "s-editor", "relation": "implements"}],
                "source_paths": {"observed": ["src/app"], "scope": ["src/components"]},
                "archive": {"archived": false},
                "freshness": {"state": "stale"},
                "status": "accepted",
                "something_a_newer_engine_added": 7,
            }
        });

        let edits = DocumentEdits {
            title: Some("New title".to_owned()),
            content: Some("New body".to_owned()),
            ..DocumentEdits::default()
        };
        let operation = document_put(&stored, &edits).unwrap();
        let envelope = &operation["record"]["envelope"];

        assert_eq!(envelope["title"], "New title");
        assert_eq!(envelope["content"], "New body");
        assert_eq!(
            envelope["content_hash"],
            content_hash("New body"),
            "the engine re-derives this digest and rejects a wrong one"
        );
        assert_eq!(
            envelope["source_paths"]["scope"][0], "src/components",
            "scope drives freshness, and an edit of the prose is not a change of scope"
        );
        assert_eq!(envelope["tags"][0], "shell");
        assert_eq!(envelope["links"][0]["relation"], "implements");
        assert_eq!(
            envelope["freshness"]["state"], "stale",
            "freshness is the engine's answer, not this layer's to revise"
        );
        assert_eq!(
            envelope["status"], "accepted",
            "product fields belong to the record's type, not to the editor"
        );
        assert_eq!(
            envelope["something_a_newer_engine_added"], 7,
            "what this build does not model, it hands back untouched"
        );
    }

    #[test]
    fn a_patch_moves_only_what_it_names() {
        let stored = json!({"envelope": {
            "key": "d-a396db",
            "kind": "decision",
            "title": "Kept",
            "content": "Kept too",
            "content_hash": content_hash("Kept too"),
            "tags": ["one"],
            "source_paths": {"observed": ["src/app"], "scope": ["src/components"]},
            "archive": {"archived": false},
            "validation_state": "unverified",
        }});

        let edits = DocumentEdits {
            tags: Some(vec!["two".to_owned(), "three".to_owned()]),
            scope: Some(vec!["src-tauri/".to_owned()]),
            archived: Some(true),
            fields: Some(
                [("validation_state".to_owned(), json!("valid"))]
                    .into_iter()
                    .collect(),
            ),
            ..DocumentEdits::default()
        };
        let envelope = document_put(&stored, &edits).unwrap()["record"]["envelope"].clone();

        assert_eq!(
            envelope["title"], "Kept",
            "a patch that says nothing about the title moves none of it"
        );
        assert_eq!(envelope["content"], "Kept too");
        assert_eq!(envelope["content_hash"], content_hash("Kept too"));
        assert_eq!(envelope["tags"], json!(["two", "three"]));
        assert_eq!(envelope["source_paths"]["scope"], json!(["src-tauri/"]));
        assert_eq!(
            envelope["source_paths"]["observed"],
            json!(["src/app"]),
            "two lists under one member: the one nobody edited is read back"
        );
        assert_eq!(envelope["archive"]["archived"], true);
        assert_eq!(envelope["validation_state"], "valid");
    }

    #[test]
    fn a_field_set_to_null_is_a_field_the_record_stops_carrying() {
        let stored = json!({"envelope": {
            "key": "s-one",
            "kind": "spec",
            "title": "A spec",
            "content": "",
            "milestone": "m-first",
        }});

        let edits = DocumentEdits {
            fields: Some(
                [("milestone".to_owned(), Value::Null)]
                    .into_iter()
                    .collect(),
            ),
            ..DocumentEdits::default()
        };
        let envelope = document_put(&stored, &edits).unwrap()["record"]["envelope"].clone();

        assert!(
            envelope.get("milestone").is_none(),
            "clearing an optional field removes it rather than storing a null"
        );
    }

    #[test]
    fn a_field_named_like_an_envelope_member_is_named_rather_than_applied() {
        let edits = DocumentEdits {
            fields: Some(
                [
                    ("title".to_owned(), json!("Hijacked")),
                    ("status".to_owned(), json!("todo")),
                ]
                .into_iter()
                .collect(),
            ),
            ..DocumentEdits::default()
        };

        assert_eq!(edits.colliding_fields(), vec!["title".to_owned()]);

        let stored = json!({"envelope": {"key": "s-one", "kind": "spec", "title": "Kept"}});
        let envelope = document_put(&stored, &edits).unwrap()["record"]["envelope"].clone();
        assert_eq!(
            envelope["title"], "Kept",
            "and it does not reach the envelope even if the caller ignores that"
        );
        assert_eq!(envelope["status"], "todo");
    }

    #[test]
    fn a_type_may_not_declare_a_field_the_envelope_already_owns() {
        // The fault this exists to catch: Chat published a `chat.conversation`
        // whose fields included `folder`, and keeping a conversation was
        // refused every time — the new record for missing a required field the
        // window strips, and the write after it for naming an envelope member.
        // Nothing said so, because nothing looked at the declaration.
        let declared = json!({
            "agent": {"type": "string", "required": true},
            "folder": {"type": "string", "required": true},
            "tokens": {"type": "integer"},
        });
        assert_eq!(
            colliding_declarations(declared.as_object().unwrap()),
            vec!["folder".to_owned()]
        );

        let sound = json!({
            "agent": {"type": "string", "required": true},
            "workdir": {"type": "string", "required": true},
        });
        assert!(
            colliding_declarations(sound.as_object().unwrap()).is_empty(),
            "the same fact under a name of its own is an ordinary product field"
        );
    }

    #[test]
    fn the_schema_is_not_a_document_and_the_project_record_is() {
        assert!(is_definition_kind(TYPE_KIND));
        assert!(
            !is_definition_kind("project"),
            "the definition of `project` is Sync's; the record that names the project is the project's"
        );
        assert!(!is_definition_kind("decision"));

        assert!(is_fixed_record(TYPE_KIND));
        assert!(is_fixed_record("project"));
        assert!(!is_fixed_record("decision"));
        assert!(
            !is_fixed_record("type_k3n8q2"),
            "a type the project invented is the project's to write and to remove"
        );
    }

    #[test]
    fn a_new_record_carries_the_required_fields_its_type_declares() {
        let definition = json!({
            "kind_name": "decision",
            "fields": {
                "validation_state": enumerated(VALIDATION_STATES, true, "unverified"),
                "note": optional_string(),
                "rank": {"type": "number", "required": true},
                "flavour": {"type": "enum", "values": ["salt", "sugar"], "required": true},
            },
        });

        let operation = new_document_put("decision", "d-1f2e3d", "A new decision", &definition);
        let envelope = &operation["record"]["envelope"];

        assert_eq!(envelope["key"], "d-1f2e3d");
        assert_eq!(envelope["kind"], "decision");
        assert_eq!(envelope["title"], "A new decision");
        assert_eq!(envelope["content"], "");
        assert_eq!(envelope["content_hash"], content_hash(""));
        assert_eq!(
            envelope["validation_state"], "unverified",
            "the declaration's own default, not the first value of the list"
        );
        assert_eq!(
            envelope["flavour"], "salt",
            "a required enumeration with no default opens at its first value"
        );
        assert_eq!(envelope["rank"], 0, "and a required number at zero");
        assert!(
            envelope.get("note").is_none(),
            "an optional field is left out: a value nobody chose is a claim nobody made"
        );
        assert_eq!(envelope["archive"]["archived"], false);
    }

    #[test]
    fn a_generated_key_says_what_kind_of_record_it_names() {
        let first = suggested_key("decision", "one");
        let second = suggested_key("decision", "two");

        assert!(first.starts_with("decision-"), "the kind, spelled: {first}");
        assert_eq!(
            first.len(),
            "decision-".len() + 6,
            "and six hex after it: {first}"
        );
        assert!(
            first
                .chars()
                .skip("decision-".len())
                .all(|c| c.is_ascii_hexdigit()),
            "{first}"
        );
        assert_ne!(first, second, "a retry has to differ from the attempt");
        assert_eq!(
            first,
            suggested_key("decision", "one"),
            "and the same seed has to give the same key, so a replay is one record"
        );
    }

    #[test]
    fn a_prefixed_kind_is_named_by_the_kind_and_not_by_whoever_published_it() {
        let decision = suggested_key("project-memory.decision", "one");
        let question = suggested_key("project-memory.question", "one");

        assert!(decision.starts_with("decision-"), "{decision}");
        assert!(question.starts_with("question-"), "{question}");
        assert_eq!(
            decision.len(),
            "decision-".len() + 6,
            "the namespace is not carried into the key: {decision}"
        );
    }

    #[test]
    fn a_kind_the_project_invented_is_named_by_itself() {
        // Whatever a person types becomes a kind, and a key is made of it the
        // same way — reduced to what an identifier can carry rather than
        // refused.
        assert!(suggested_key("type_k3n8q2", "one").starts_with("type-k3n8q2-"));
        assert!(suggested_key("Hypothesis", "one").starts_with("hypothesis-"));
        assert!(suggested_key("...", "one").starts_with("record-"));
    }

    #[test]
    fn every_entity_kind_has_a_published_type_definition() {
        let definitions = type_definitions(ENTITY_KINDS);

        assert_eq!(
            definitions.len(),
            ENTITY_KINDS.len(),
            "a kind without a definition is rejected by the strict schema"
        );
        for (definition, kind) in definitions.iter().zip(ENTITY_KINDS) {
            let envelope = &definition["record"]["envelope"];
            assert_eq!(envelope["kind"], TYPE_KIND);
            assert_eq!(envelope["key"], format!("__type__/{}", kind.as_str()));
        }
    }

    #[test]
    fn the_project_record_is_a_kind_the_engine_knows() {
        let published: Vec<String> = type_definitions(ENTITY_KINDS)
            .iter()
            .map(|definition| definition["record"]["envelope"]["key"].to_string())
            .collect();

        assert!(
            published.contains(&format!("\"__type__/{}\"", EntityKind::Project.as_str())),
            "without a published definition the strict schema rejects the record \
             that tells Sync a repository has been opened as a project before"
        );
    }

    #[test]
    fn every_published_kind_carries_a_mark_of_its_own() {
        let icons: Vec<&str> = ENTITY_KINDS.iter().map(|kind| kind.icon()).collect();

        assert!(
            icons.iter().all(|icon| !icon.is_empty()),
            "a kind Sync publishes and cannot draw is a type the product declared and then failed to recognise"
        );
        let mut unique = icons.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            icons.len(),
            "two kinds sharing a mark makes the mark meaningless"
        );
    }

    #[test]
    fn types_are_listed_with_the_kinds_sync_knows_first() {
        let mut types = vec![
            type_row("hypothesis", "invented here", "shapes"),
            type_row("artifact", "", "package"),
            type_row("project", "", "folder-git-2"),
        ];

        sort_types(&mut types);

        assert_eq!(
            types
                .iter()
                .map(|entry| entry.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["project", "artifact", "hypothesis"],
            "the kinds Sync describes lead, in its own order; the project's own follow by name"
        );
    }

    #[test]
    fn a_type_carries_the_mark_its_own_definition_names() {
        let listed = type_row("hypothesis", "Something to test", "flask-conical");

        assert_eq!(listed.kind, "hypothesis");
        assert_eq!(listed.description, "Something to test");
        assert_eq!(
            listed.icon.as_deref(),
            Some("flask-conical"),
            "no build knows a type invented in the window, so its mark travels with it"
        );
    }

    #[test]
    fn a_type_over_a_folder_names_the_folder_itself() {
        let record = type_definition(
            &TypeDeclaration::new("guide", "Guide", "Team documentation", "book")
                .in_storage(TypeStorage::attached("docs/guides")),
        );
        let definition: Value =
            serde_json::from_str(record["record"]["envelope"]["content"].as_str().unwrap())
                .unwrap();

        assert_eq!(
            definition[STORAGE_FIELD], "docs/guides",
            "the path, so nothing has to be looked up to write a document"
        );

        let listed = RecordType::from_record(&record["record"]["envelope"]).unwrap();
        assert!(listed.storage.is_attached());
        assert_eq!(listed.storage.folder.as_deref(), Some("docs/guides"));
    }

    #[test]
    fn a_type_that_says_nothing_about_storage_keeps_its_documents_in_its_records() {
        let record = type_definition(&TypeDeclaration::new(
            "hypothesis",
            "Hypothesis",
            "Something to test",
            "flask-conical",
        ));
        let definition: Value =
            serde_json::from_str(record["record"]["envelope"]["content"].as_str().unwrap())
                .unwrap();

        assert!(
            definition.get(STORAGE_FIELD).is_none(),
            "silence already means `with the records`, and a member restating it \
             is one more thing a later engine has to keep agreeing with"
        );
        assert!(!TypeStorage::of_definition(&definition).is_attached());
    }

    #[test]
    fn a_type_is_named_by_its_definition_and_identified_by_its_kind() {
        let record = type_definition(&TypeDeclaration::new(
            "open_question",
            "Open question",
            "Something nobody has settled",
            "circle-help",
        ));
        let listed = RecordType::from_record(&record["record"]["envelope"]).unwrap();

        assert_eq!(
            listed.title, "Open question",
            "the name is what a person reads, and more than one word is normal"
        );
        assert_eq!(
            listed.kind, "open_question",
            "the identifier is what every record of the type carries"
        );
    }

    #[test]
    fn a_definition_with_no_name_falls_back_to_the_one_sync_knows_then_to_the_kind() {
        let unnamed = |kind: &str| {
            let definition = json!({"kind_name": kind, "description": "", "fields": {}});
            let content = serde_json::to_string_pretty(&definition).unwrap();
            RecordType::from_record(&json!({
                "key": format!("{TYPE_KIND}/{kind}"),
                "kind": TYPE_KIND,
                "content": content,
            }))
            .unwrap()
            .title
        };

        assert_eq!(
            unnamed("decision"),
            "Decision",
            "a corpus written before types carried a name still reads as the kinds Sync describes"
        );
        assert_eq!(
            unnamed("open_question"),
            "Open question",
            "an identifier is made readable rather than shown raw"
        );
        assert_eq!(
            unnamed("review.open_question"),
            "Open question",
            "an extension prefixes its kinds to stay out of the project's way; that is not part of the name"
        );
    }

    #[test]
    fn a_definition_with_no_mark_falls_back_to_the_one_sync_knows() {
        let silent = |kind: &str| {
            let definition = json!({"kind_name": kind, "description": "", "fields": {}});
            let content = serde_json::to_string_pretty(&definition).unwrap();
            RecordType::from_record(&json!({
                "key": format!("{TYPE_KIND}/{kind}"),
                "kind": TYPE_KIND,
                "content": content,
            }))
            .unwrap()
        };

        assert_eq!(
            silent("decision").icon.as_deref(),
            Some("signpost"),
            "a corpus written before marks travelled with types still reads as the kinds people recognise"
        );
        assert!(
            silent("hypothesis").icon.is_none(),
            "a kind Sync cannot describe gets the neutral mark rather than a borrowed one"
        );
        assert_eq!(
            type_row("decision", "", "flask-conical").icon.as_deref(),
            Some("flask-conical"),
            "the definition wins wherever it speaks"
        );
    }

    #[test]
    fn syncs_own_type_is_marked_as_its_own_and_the_project_s_are_not() {
        assert!(
            type_row("project", "", "folder-git-2").own,
            "the project's own record has this kind, so nothing may offer to remove it"
        );
        assert!(!type_row("hypothesis", "", "flask-conical").own);
    }

    #[test]
    fn a_definition_that_cannot_be_read_is_still_a_type_the_project_holds() {
        let record = json!({
            "key": "__type__/hypothesis",
            "kind": TYPE_KIND,
            "content": "not json",
        });

        let listed = RecordType::from_record(&record).unwrap();

        assert_eq!(
            listed.kind, "hypothesis",
            "the key names it even when the definition does not"
        );
        assert!(listed.icon.is_none());
    }

    #[test]
    fn a_metadata_row_and_a_full_record_read_as_the_same_row() {
        let metadata = json!({
            "key": "d-7e7e2d",
            "kind": "decision",
            "title": "Git as the knowledge store is the moat",
            "freshness": "stale",
            "archived": false,
        });
        let full = json!({
            "key": "d-7e7e2d",
            "kind": "decision",
            "title": "Git as the knowledge store is the moat",
            "freshness": {"state": "stale"},
            "archive": {"archived": false},
        });

        let from_metadata = RecordEntry::from_record(&metadata).unwrap();
        let from_full = RecordEntry::from_record(&full).unwrap();

        assert_eq!(from_metadata.freshness, "stale");
        assert_eq!(
            from_metadata.freshness, from_full.freshness,
            "the engine flattens these for a metadata listing and nests them for a full one"
        );
        assert!(!from_full.archived);
    }

    #[test]
    fn a_record_read_by_key_is_unwrapped_from_its_envelope() {
        let stored = json!({
            "representation": "plaintext",
            "envelope": {
                "key": "c-788561",
                "kind": "constraint",
                "title": "The window must hold its structure at 1024 x 700",
                "source_paths": {"scope": ["src/", "src-tauri/"]},
            },
        });

        let record = RecordEntry::from_record(&stored).unwrap();

        assert_eq!(record.key, "c-788561");
        assert_eq!(
            record.scope,
            vec!["src/".to_owned(), "src-tauri/".to_owned()]
        );
        assert_eq!(
            record.freshness, DEFAULT_FRESHNESS,
            "a record that states no freshness is unverified, which is neither a fact nor a failure"
        );
    }

    #[test]
    fn a_row_carries_the_folder_it_is_filed_in() {
        // Both shapes again: the engine writes `folder` at the top level of a
        // metadata row and of a full envelope alike.
        let listed = RecordEntry::from_record(&json!({
            "key": "guides-intro",
            "kind": "doc",
            "folder": "docs/guides",
        }))
        .unwrap();
        assert_eq!(listed.folder.as_deref(), Some("docs/guides"));
        assert!(!listed.is_folder);

        let stored = RecordEntry::from_record(&json!({
            "envelope": {"key": "api-guides", "kind": "doc", "folder": "docs/guides/api"},
        }))
        .unwrap();
        assert_eq!(stored.folder.as_deref(), Some("docs/guides/api"));
    }

    #[test]
    fn a_record_filed_nowhere_is_in_no_folder() {
        let row = RecordEntry::from_record(&json!({"key": "d-1", "kind": "decision"})).unwrap();
        assert_eq!(
            row.folder, None,
            "the root is the absence of a folder, not a folder named by the empty string"
        );
    }

    #[test]
    fn the_record_that_is_a_folder_says_so() {
        let row = RecordEntry::from_record(&json!({
            "key": "api-guides",
            "kind": "doc",
            "folder": "docs/guides/api",
            "is_folder": true,
        }))
        .unwrap();

        assert!(row.is_folder);
        assert_eq!(
            row.folder.as_deref(),
            Some("docs/guides/api"),
            "the folder it stands for is the one it is filed in, never a path of its own"
        );
    }

    #[test]
    fn a_document_is_filed_where_its_row_says() {
        let document = Document::from_record(&json!({
            "envelope": {
                "key": "api-guides",
                "kind": "doc",
                "title": "API guides",
                "content": "How authentication works here.",
                "folder": "docs/guides/api",
                "is_folder": true,
            },
        }))
        .unwrap();

        assert_eq!(document.folder.as_deref(), Some("docs/guides/api"));
        assert!(document.is_folder);
        assert!(
            !document.fields.contains_key("folder") && !document.fields.contains_key("is_folder"),
            "both belong to the envelope, and a metadata panel offering to type a path into \
             one would be offering to move a file by editing a field"
        );
    }

    #[test]
    fn something_without_a_key_is_not_a_row() {
        assert!(
            RecordEntry::from_record(&json!({"kind": "decision"})).is_none(),
            "a row the store cannot be asked about has no place in the interface"
        );
    }

    #[test]
    fn the_schema_is_left_out_by_the_engine_and_not_subtracted_twice() {
        // What the engine reports: type definitions counted as `service` and in
        // none of the other numbers. Subtracting them again here would take
        // eleven claims off a project that never counted them.
        let everything = Counts {
            total: 2,
            by_kind: BTreeMap::from([("decision".to_owned(), 1), ("spec".to_owned(), 1)]),
            by_freshness: BTreeMap::from([("unverified".to_owned(), 1), ("stale".to_owned(), 1)]),
            archived: 0,
            live: 2,
            service: 11,
        };

        let counts = RecordsCounts::excluding(&everything, &[]);

        assert_eq!(counts.total, 2);
        assert!(!counts.by_kind.contains_key(TYPE_KIND));
        assert_eq!(counts.by_freshness.get("unverified"), Some(&1));
        assert_eq!(counts.by_freshness.get("stale"), Some(&1));
    }

    #[test]
    fn a_hidden_kind_leaves_the_counts_as_well_as_the_list() {
        let everything = Counts {
            total: 6,
            by_kind: BTreeMap::from([
                (TYPE_KIND.to_owned(), 11),
                ("decision".to_owned(), 2),
                ("comment".to_owned(), 3),
            ]),
            by_freshness: BTreeMap::from([("fresh".to_owned(), 2), ("stale".to_owned(), 4)]),
            archived: 0,
            live: 6,
            service: 11,
        };
        let comments = Counts {
            total: 3,
            by_kind: BTreeMap::from([("comment".to_owned(), 3)]),
            by_freshness: BTreeMap::from([("stale".to_owned(), 3)]),
            archived: 0,
            live: 3,
            service: 0,
        };

        let counts = RecordsCounts::excluding(&everything, &[comments]);

        assert_eq!(counts.total, 3);
        assert!(!counts.by_kind.contains_key("comment"));
        assert_eq!(
            counts.by_freshness.get("stale"),
            Some(&1),
            "a navigator listing nine types beside a total counting eleven is arithmetic nobody can follow"
        );
        assert_eq!(counts.by_freshness.get("fresh"), Some(&2));
    }

    #[test]
    fn a_corpus_that_already_matches_is_not_republished() {
        assert!(
            corpus_matches(&stored_corpus()),
            "republishing an identical definition puts a commit on refs/memory for nothing"
        );
    }

    #[test]
    fn a_definition_that_lost_its_mark_is_republished() {
        // What every corpus written before marks travelled with types looks
        // like: the same description and fields, no icon.
        let stale = json!({
            "kind_name": EntityKind::Project.as_str(),
            "description": EntityKind::Project.description(),
            "fields": EntityKind::Project.extension_fields(),
            "relationships": {},
        });
        let content = serde_json::to_string_pretty(&stale).unwrap();
        let records = vec![json!({
            "key": format!("{TYPE_KIND}/project"),
            "kind": TYPE_KIND,
            "content": content.clone(),
            "content_hash": content_hash(&content),
        })];

        assert!(
            !corpus_matches(&records),
            "a comparison that cannot see the mark leaves the corpus without one for ever"
        );
    }

    #[test]
    fn a_type_the_project_added_is_not_something_to_republish_over() {
        let mut records = stored_corpus();
        records.push(
            type_definition(&TypeDeclaration::new(
                "hypothesis",
                "Hypothesis",
                "the project's own",
                "flask-conical",
            ))["record"]["envelope"]
                .clone(),
        );

        assert!(
            corpus_matches(&records),
            "what a project can say is the project's decision, not something opening a window revises"
        );
    }

    #[test]
    fn a_project_without_syncs_own_type_gets_it() {
        assert!(
            !corpus_matches(&[]),
            "without the `project` definition the strict schema rejects the record that names the project"
        );
    }

    /// The `__type__` records of a project Sync has opened.
    fn stored_corpus() -> Vec<Value> {
        own_type_definitions()
            .iter()
            .map(|operation| operation["record"]["envelope"].clone())
            .collect()
    }

    /// A type as the store holds it, read back the way the window reads it.
    /// Named the way a type with nothing else to go on would be, so the tests
    /// that are about marks and ordering are not also about naming.
    fn type_row(kind: &str, description: &str, icon: &str) -> RecordType {
        let record = type_definition(&TypeDeclaration::new(
            kind,
            &readable(kind),
            description,
            icon,
        ));
        RecordType::from_record(&record["record"]["envelope"]).unwrap()
    }

    #[test]
    fn a_delete_addresses_a_record_by_its_semantic_key() {
        let operation = delete("s-sidecar");

        assert_eq!(operation["op"], "delete");
        assert_eq!(
            operation["key"], "s-sidecar",
            "encrypted projects only accept semantic keys, so plaintext ones use them too"
        );
    }
}
