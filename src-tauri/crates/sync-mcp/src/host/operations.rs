//! Every product operation the window can call.
//!
//! Each is an adapter and nothing more: read what the call carries, hand it to
//! [`Domain`], answer with what came back. That is deliberate — the moment an
//! operation starts deciding something, the domain has a second home.
//!
//! The macro exists so that stays true. Adding one is a name and a line, so
//! there is no temptation to fold two things into an existing operation
//! because writing a new one looked expensive.

use serde_json::{Value, json};
use sync_memory::mapping::{TypeDeclaration, colliding_declarations, type_definition};
use sync_memory::{DocumentEdits, GUIDANCE_FIELD, MemoryError, Result};

use super::Operation;
use crate::domain::Domain;

/// Define one operation: a name on the wire and a line that runs it.
///
/// The `without_memory` form marks one that must answer on a project whose
/// corpus cannot be read yet — see [`Operation::needs_memory`]. It is a
/// separate spelling rather than an argument every operation carries so that
/// the ordinary case stays a name and a line, and the exception has to be
/// written out.
macro_rules! operation {
    ($type:ident, $name:literal, |$domain:ident, $params:ident| $body:expr) => {
        operation!(@define $type, $name, true, |$domain, $params| $body);
    };
    (without_memory $type:ident, $name:literal, |$domain:ident, $params:ident| $body:expr) => {
        operation!(@define $type, $name, false, |$domain, $params| $body);
    };
    (@define $type:ident, $name:literal, $needs:literal, |$domain:ident, $params:ident| $body:expr) => {
        struct $type;
        impl Operation for $type {
            fn name(&self) -> &'static str {
                $name
            }
            fn needs_memory(&self) -> bool {
                $needs
            }
            #[allow(unused_variables)]
            fn run(&self, $domain: &mut Domain, $params: &Value) -> Result<Value> {
                $body
            }
        }
    };
}

// --- types ----------------------------------------------------------------

operation!(TypesList, "types.list", |d, p| encode(d.list_types()?));
operation!(TypesCreate, "types.create", |d, p| encode(d.create_type(
    text(p, "kind")?,
    text(p, "title")?,
    text(p, "description")?,
    text(p, "icon")?
)?));
operation!(TypesUpdate, "types.update", |d, p| encode(d.update_type(
    text(p, "kind")?,
    text(p, "title")?,
    text(p, "description")?,
    text(p, "icon")?
)?));
operation!(TypesDelete, "types.delete", |d, p| encode(
    d.delete_type(text(p, "kind")?)?
));
operation!(TypesPublish, "types.publish", |d, p| encode(
    d.publish_types()?
));
// Takes the types as a catalogue states them, not as envelopes. The catalogue
// is the window's and the schema is the engine's; what a `__type__` record
// looks like in between is neither's, and used to be assembled in the window
// purely because that is where the list came from.
operation!(TypesPublishExtension, "types.publish_extension", |d, p| {
    let definitions: Vec<Value> = values(p, "types")
        .iter()
        .map(|entry| {
            let fields = declared(entry, "fields");
            // The one thing about a declaration this layer does check, because
            // the engine cannot: a product field named after an envelope member
            // describes a record the store has no room for. Nothing refuses it
            // at publication, and the damage shows up much later and twice
            // over — a new record is rejected for missing a required field the
            // window strips, and a write naming the field is rejected outright.
            // An extension whose type is unwritable must not count as
            // installed, so it is refused here, by name.
            let colliding = colliding_declarations(&fields);
            if !colliding.is_empty() {
                return Err(MemoryError::domain(
                    "invalid_record",
                    format!(
                        "`{}` names the envelope's own members, not product fields.",
                        colliding.join("`, `")
                    ),
                    json!({"kind": text(entry, "kind")?, "fields": colliding}),
                ));
            }
            Ok(type_definition(
                &TypeDeclaration::new(
                    text(entry, "kind")?,
                    text(entry, "title")?,
                    text(entry, "description")?,
                    text(entry, "icon")?,
                )
                // Carried whole rather than read member by member. What a
                // field or a relation may say is the engine's schema, which
                // moves on its own; a copy of it here would be a second
                // vocabulary to keep in step, and the engine refuses what it
                // does not recognise anyway.
                .with_fields(fields)
                .with_relationships(declared(entry, "relationships"))
                .with_guidance(entry.get(GUIDANCE_FIELD).and_then(Value::as_str)),
            ))
        })
        .collect::<Result<Vec<Value>>>()?;
    encode(d.publish_extension_types(&definitions)?)
});
operation!(TypesAttachFolder, "types.attach_folder", |d, p| encode(
    d.attach_folder(
        text(p, "kind")?,
        text(p, "title")?,
        text(p, "description")?,
        text(p, "icon")?,
        text(p, "folder")?
    )?
));

// --- records --------------------------------------------------------------

operation!(RecordsGet, "records.get", |d, p| encode(
    d.get_record(text(p, "key")?)?
));
operation!(RecordsList, "records.list", |d, p| encode(
    d.list_records(p)?
));
operation!(RecordsSearch, "records.search", |d, p| encode(d.search(p)?));
operation!(RecordsApply, "records.apply", |d, p| {
    // The id is allocated here, not by the caller. A transaction id names one
    // attempt, and the writer is the only one that can be sure it is naming
    // its own.
    let id = d.next_transaction_id(maybe(p, "occasion").unwrap_or("window"));
    encode(d.apply(&id, &values(p, "operations"))?)
});
operation!(RecordsSave, "records.save", |d, p| {
    let entities: Vec<sync_memory::EntityInput> = serde_json::from_value(
        p.get("entities").cloned().unwrap_or(Value::Null),
    )
    .map_err(|error| {
        MemoryError::domain(
            "invalid_argument",
            format!("unreadable entities: {error}"),
            Value::Null,
        )
    })?;
    encode(d.save_entities(entities)?)
});
operation!(RecordsDelete, "records.delete", |d, p| encode(
    d.delete_documents(&strings(p, "keys"))?
));

// --- documents ------------------------------------------------------------

operation!(DocumentsGet, "documents.get", |d, p| encode(
    d.document(text(p, "key")?)?
));
operation!(DocumentsCreate, "documents.create", |d, p| encode(
    d.create_document(
        text(p, "kind")?,
        text(p, "title")?,
        p.get("folder").and_then(Value::as_str)
    )?
));
operation!(DocumentsCreateFile, "documents.create_file", |d, p| encode(
    d.create_file_document(
        text(p, "kind")?,
        text(p, "name")?,
        text(p, "contentBase64")?
    )?
));
operation!(DocumentsUpdate, "documents.update", |d, p| {
    let edits: DocumentEdits = serde_json::from_value(
        p.get("edits").cloned().unwrap_or(Value::Null),
    )
    .map_err(|error| {
        MemoryError::domain(
            "invalid_argument",
            format!("unreadable edits: {error}"),
            Value::Null,
        )
    })?;
    encode(d.update_document(text(p, "key")?, &edits)?)
});
operation!(DocumentsRead, "documents.read", |d, p| encode(
    d.read_content(text(p, "key")?)?
));
operation!(DocumentsWrite, "documents.write", |d, p| encode(
    d.write_content_as(
        text(p, "key")?,
        text(p, "content")?,
        maybe(p, "encoding").unwrap_or("utf-8")
    )?
));
operation!(DocumentsScan, "documents.scan", |d, p| encode(d.scan()?));
operation!(DocumentsResolve, "documents.resolve_unmatched", |d, p| {
    encode(d.resolve_unmatched(
        text(p, "locator")?,
        text(p, "contentHash")?,
        text(p, "kind")?,
        maybe(p, "adopt"),
    )?)
});

// --- records ------------------------------------------------------------

operation!(RecordsLoad, "records.load", |d, p| encode(d.records(
    p.get("selection").unwrap_or(&Value::Null),
    &strings(p, "hidden")
)?));
operation!(RecordsBacklinks, "records.backlinks", |d, p| encode(
    d.backlinks(text(p, "key")?)?
));

// --- folders ----------------------------------------------------------------

// `folder` absent asks about the whole project, which is not the same as asking
// about the root: the root is `""` and is a folder like any other.
operation!(FoldersList, "folders.list", |d, p| encode(d.folders(
    p.get("folder").and_then(Value::as_str),
    flag(p, "subtree"),
    p.get("kind").and_then(Value::as_str)
)?));
operation!(FoldersCreate, "folders.create", |d, p| encode(
    d.create_folder(text(p, "folder")?, text(p, "kind")?)?
));
operation!(FoldersDescribe, "folders.describe", |d, p| encode(
    d.describe_folder(text(p, "folder")?, text(p, "kind")?)?
));
operation!(FoldersDelete, "folders.delete", |d, p| encode(
    d.delete_folder(text(p, "folder")?)?
));
operation!(FoldersToll, "folders.toll", |d, p| encode(
    d.folder_toll(text(p, "folder")?)?
));
operation!(FoldersRename, "folders.rename", |d, p| encode(
    d.rename_folder(text(p, "from")?, text(p, "to")?)?
));
// Not `text`: the empty string is the root, which is somewhere a record can be
// moved, and a helper that reads a required string would refuse it.
operation!(DocumentsMove, "documents.move", |d, p| encode(
    d.move_document(
        text(p, "key")?,
        p.get("folder").and_then(Value::as_str).ok_or_else(|| {
            MemoryError::domain(
                "invalid_argument",
                "`folder` is required and must be a string",
                json!({"field": "folder"}),
            )
        })?
    )?
));
operation!(RecordsDependents, "records.dependents", |d, p| encode(
    d.dependents(text(p, "key")?)?
));

// --- the project ----------------------------------------------------------

// `without_memory`: what the window asks first, and the answer that tells it
// the project is encrypted at all. A handshake that required the corpus would
// require unlocking in order to discover that unlocking is what is needed.
operation!(without_memory ProjectDescribe, "project.describe", |d, p| encode(
    d.describe()?
));
// Read, not recalled. The field this used to answer from is what *this* session
// last wrote, and the question is about everything else: a `git pull`, a second
// window, the engine's own CLI. `without_memory` because this *is* the read
// that initialises — gating it on one would be gating it on itself.
operation!(without_memory ProjectRevision, "project.revision", |d, p| encode(
    d.refresh_revision()?
));
// The project's own record. The window collects these fields from a person and
// reads them back to decide whether to ask at all; what they look like as an
// envelope is not its business and is no longer in its code.
operation!(ProjectSettingsRead, "project.settings", |d, p| encode(
    d.project_settings()?
));
operation!(ProjectUpdate, "project.update", |d, p| {
    let settings: sync_memory::ProjectSettings = serde_json::from_value(
        p.get("settings").cloned().unwrap_or(Value::Null),
    )
    .map_err(|error| {
        MemoryError::domain(
            "invalid_argument",
            format!("unreadable project settings: {error}"),
            Value::Null,
        )
    })?;
    encode(d.update_project(&settings)?)
});
operation!(ProjectImport, "project.import", |d, p| {
    let id = d.next_transaction_id("import");
    encode(d.import(&id, p.get("bundle").unwrap_or(&Value::Null))?)
});
operation!(ProjectExport, "project.export", |d, p| d.export());
operation!(ProjectReindex, "project.reindex", |d, p| d.reindex());
// Answers on a project whose corpus cannot be read, because a corpus behind a
// rewritten history is exactly what this is for: requiring one first would
// refuse the operation precisely when it is needed.
operation!(without_memory ProjectReconcile, "project.reconcile", |d, p| d
    .reconcile(flag(p, "full_rebuild")));
operation!(ProjectSchemaStatus, "project.schema_status", |d, p| d
    .schema_status());

// --- the engine underneath ------------------------------------------------
//
// All `without_memory`. These describe the engine and the state of the store,
// not the corpus inside it, so requiring a readable corpus would refuse the
// questions a window asks precisely when it cannot read one.

operation!(without_memory EngineModel, "engine.model_status", |d, p| encode(
    d.model_status()?
));
operation!(without_memory EngineTransport, "engine.transport_status", |d, p| encode(
    d.transport_status()?
));
operation!(without_memory EngineSyncState, "engine.sync_state", |d, p| encode(
    d.sync_state(flag(p, "askRemote"))?
));
operation!(without_memory EngineRewind, "engine.rewind", |d, p| encode(
    d.rewind(text(p, "revision")?, text(p, "expected")?)?
));
operation!(without_memory EnginePresence, "engine.presence", |d, p| encode(
    d.presence()?
));
operation!(without_memory EngineRemoteSet, "engine.remote_set", |d, p| encode(
    d.set_remote(text(p, "url")?, maybe(p, "refspec"))?
));
operation!(without_memory EngineRemoteRemove, "engine.remote_remove", |d, p| encode(
    d.remove_remote()?
));
// A fetch keeps the default: `needs_memory` here means "open the storage first
// if nobody has", which is the order a fresh clone needs anyway — there has to
// be a store before there is anywhere to merge into.
operation!(EngineFetch, "engine.fetch", |d, p| d.fetch());
operation!(EnginePush, "engine.push", |d, p| d.push(flag(p, "force")));

/// Everything this surface answers.
pub fn operations() -> Vec<Box<dyn Operation>> {
    vec![
        Box::new(TypesList),
        Box::new(TypesCreate),
        Box::new(TypesUpdate),
        Box::new(TypesDelete),
        Box::new(TypesPublish),
        Box::new(TypesPublishExtension),
        Box::new(TypesAttachFolder),
        Box::new(RecordsGet),
        Box::new(RecordsList),
        Box::new(RecordsSearch),
        Box::new(RecordsApply),
        Box::new(RecordsSave),
        Box::new(RecordsDelete),
        Box::new(DocumentsGet),
        Box::new(DocumentsCreate),
        Box::new(DocumentsCreateFile),
        Box::new(DocumentsUpdate),
        Box::new(DocumentsRead),
        Box::new(DocumentsWrite),
        Box::new(DocumentsScan),
        Box::new(DocumentsResolve),
        Box::new(RecordsLoad),
        Box::new(RecordsBacklinks),
        Box::new(RecordsDependents),
        Box::new(FoldersList),
        Box::new(FoldersCreate),
        Box::new(FoldersDescribe),
        Box::new(FoldersDelete),
        Box::new(FoldersToll),
        Box::new(FoldersRename),
        Box::new(DocumentsMove),
        Box::new(ProjectDescribe),
        Box::new(ProjectSettingsRead),
        Box::new(ProjectUpdate),
        Box::new(ProjectRevision),
        Box::new(ProjectImport),
        Box::new(ProjectExport),
        Box::new(ProjectReindex),
        Box::new(ProjectReconcile),
        Box::new(ProjectSchemaStatus),
        Box::new(EngineModel),
        Box::new(EngineTransport),
        Box::new(EngineSyncState),
        Box::new(EngineRewind),
        Box::new(EnginePresence),
        Box::new(EngineRemoteSet),
        Box::new(EngineRemoteRemove),
        Box::new(EngineFetch),
        Box::new(EnginePush),
    ]
}

// --- reading what a call carries -------------------------------------------

/// A required string. Missing is the caller's mistake, and is said as one.
fn text<'a>(params: &'a Value, field: &str) -> Result<&'a str> {
    params.get(field).and_then(Value::as_str).ok_or_else(|| {
        MemoryError::domain(
            "invalid_argument",
            format!("`{field}` is required and must be a string"),
            json!({"field": field}),
        )
    })
}

/// An optional string.
fn maybe<'a>(params: &'a Value, field: &str) -> Option<&'a str> {
    params.get(field).and_then(Value::as_str)
}

/// An optional flag, absent meaning false.
fn flag(params: &Value, field: &str) -> bool {
    params.get(field).and_then(Value::as_bool).unwrap_or(false)
}

/// A list of strings, absent meaning empty.
fn strings(params: &Value, field: &str) -> Vec<String> {
    params
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A list of anything, absent meaning empty.
fn values(params: &Value, field: &str) -> Vec<Value> {
    params
        .get(field)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// A section of a type's definition, as the caller wrote it.
///
/// Not validated here, and that is the point: what a field or a relation may
/// say is the engine's schema, and it refuses a definition it cannot read. A
/// check in this layer would be a second opinion that goes out of date on an
/// engine release, and the failure it would produce — a section silently
/// dropped — is worse than the one it would prevent.
fn declared(params: &Value, field: &str) -> serde_json::Map<String, Value> {
    params
        .get(field)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Answer with a view of our own.
///
/// A failure here is a bug in this process, not a refusal from the engine, and
/// is marked as such so the two cannot be confused in a log.
fn encode<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| {
        MemoryError::domain(
            "internal",
            format!("the answer could not be encoded: {error}"),
            Value::Null,
        )
    })
}
