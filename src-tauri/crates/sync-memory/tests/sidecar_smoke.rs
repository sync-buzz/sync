#![allow(clippy::expect_used, clippy::unwrap_used)]
// Two of these walk a whole path a person takes, end to end, and are longer
// than the lint allows. Cutting them into halves that only ever run together
// would hide the path, which is the thing being tested.
#![allow(clippy::too_many_lines)]

//! End-to-end against the real engine.
//!
//! Everything else in this crate tests the client against a scripted transport,
//! which proves the framing but not the contract. These tests drive an actual
//! `sync-mcp` process: create, read, search, checkpoint, history, and a
//! kill/restart cycle.
//!
//! The binary is taken from `SYNC_MCP_BINARY`, falling back to the
//! sibling checkout's release build. Without one the tests report why and pass
//! — a developer without the engine built should not see a red suite — while CI
//! sets the variable, so a regression there is a build failure.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;
use sync_memory::{
    DocumentEdits, ENTITY_KINDS, Entity, EntityKind, LaunchConfig, Link, MemoryClient, OWN_KINDS,
    Operations as _, TYPE_KIND, type_definitions, type_record,
};

fn engine_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SYNC_MCP_BINARY") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    // The sidecar is this workspace's own crate now, so it is looked for
    // where this workspace builds. `binaries/` first, because that is the copy
    // the bundle would ship and therefore the one worth testing; a plain
    // release build second, for the loop where somebody is changing it.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let staged = std::fs::read_dir(root.join("binaries"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sync-mcp-"))
        });
    staged
        .into_iter()
        .chain([
            root.join("target/release/sync-mcp"),
            root.join("target/debug/sync-mcp"),
        ])
        .find(|path| path.is_file())
}

/// A git repository with a memory client attached to it.
struct Fixture {
    _project_dir: tempfile::TempDir,
    _logs: tempfile::TempDir,
    project: PathBuf,
    client: MemoryClient,
    log_file: PathBuf,
}

fn fixture(binary: &Path) -> Fixture {
    let project = tempfile::tempdir().expect("temp project");
    let logs = tempfile::tempdir().expect("temp logs");
    let status = Command::new("git")
        .arg("init")
        .arg(project.path())
        .output()
        .expect("git init");
    assert!(status.status.success(), "the fixture needs a repository");

    let log_file = logs.path().join("memory.log");
    let client = MemoryClient::connect(LaunchConfig {
        project: project.path().to_path_buf(),
        bundled: binary.to_path_buf(),
        log_file: log_file.clone(),
        // The binary under test is passed as the bundled one: these tests are
        // about the copy a bundle would ship, and there is nothing else here to
        // prefer over it.
        override_binary: None,
        host_socket: None,
    })
    .expect("the engine starts and handshakes");

    Fixture {
        project: project.path().to_path_buf(),
        _project_dir: project,
        _logs: logs,
        client,
        log_file,
    }
}

fn spec(key: &str, title: &str, content: &str) -> Entity {
    let mut extensions = serde_json::Map::new();
    extensions.insert("status".to_owned(), json!("todo"));
    Entity {
        key: key.to_owned(),
        kind: EntityKind::Spec.as_str().to_owned(),
        title: title.to_owned(),
        content: content.to_owned(),
        tags: vec!["memory".to_owned()],
        links: Vec::new(),
        paths_observed: Vec::new(),
        scope_paths: vec!["src/".to_owned()],
        extensions,
        folder: None,
        is_folder: false,
        archived: false,
        verified: false,
    }
}

#[test]
fn create_read_search_checkpoint_and_history_through_the_real_engine() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);

    // The strict schema rejects a kind with no `__type__` record, so the type
    // corpus is published before anything else — exactly as Sync will on first
    // open.
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");

    let entity = spec(
        "s-sidecar",
        "Drive memory-hub as a sidecar",
        "Sync bundles the engine and speaks MCP to it.",
    );
    let write = fixture
        .client
        .apply("seed", &[entity.to_put()])
        .expect("the record is written");
    assert_eq!(write.changed_keys, vec!["s-sidecar".to_owned()]);
    assert_eq!(
        fixture.client.revision(),
        write.revision,
        "the client tracks the revision it just produced"
    );

    let view = fixture
        .client
        .get_record("s-sidecar")
        .expect("the record reads back");
    let record = view.record.expect("the record exists at this revision");
    assert_eq!(record["envelope"]["title"], "Drive memory-hub as a sidecar");
    assert_eq!(
        record["envelope"]["status"], "todo",
        "product fields survive the round trip, flattened as the envelope stores them"
    );

    let search = fixture
        .client
        .search(&json!({"query": "sidecar", "limit": 10}))
        .expect("search answers");
    assert!(
        search.hits.iter().any(|hit| hit["id"] == "s-sidecar"),
        "the written record is findable"
    );
    assert!(
        search.mode == "fts" || search.mode == "hybrid",
        "search reports how it answered: {}",
        search.mode
    );

    let listing = fixture
        .client
        .list_records(&json!({"kind": "spec"}))
        .expect("listing answers");
    assert_eq!(
        listing.total, 1,
        "one spec, and the type records are hidden"
    );

    assert!(
        fixture.log_file.is_file(),
        "the engine's stderr goes to a log file, never to the UI"
    );
}

#[test]
fn editing_a_record_rewrites_its_prose_and_keeps_everything_else() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    fixture
        .client
        .apply(
            "seed",
            &[spec("s-editor", "Edit records in place", "The old body.").to_put()],
        )
        .expect("the record is written");

    fixture
        .client
        .update_document(
            "s-editor",
            &DocumentEdits {
                title: Some("Edit records in place, in the workspace".to_owned()),
                content: Some("# New\n".to_owned()),
                ..DocumentEdits::default()
            },
        )
        .expect("the edit is written");

    let document = fixture
        .client
        .document("s-editor")
        .expect("the record reads back")
        .expect("it still exists");
    assert_eq!(document.title, "Edit records in place, in the workspace");
    assert_eq!(document.content, "# New\n");
    assert_eq!(
        document.scope,
        vec!["src/".to_owned()],
        "scope drives freshness and is not the editor's to drop"
    );
    assert_eq!(document.tags, vec!["memory".to_owned()]);
    assert_eq!(
        document
            .fields
            .get("status")
            .and_then(|value| value.as_str()),
        Some("todo"),
        "the product fields the type declares survive an edit of the prose"
    );

    // A type definition is a record like any other and would take a body
    // happily. It is refused here rather than by the engine, because a corpus
    // whose definitions have been overwritten with prose is one nothing can
    // parse — and the refusal names the kind rather than reporting a schema
    // failure after the fact.
    // A patch that names one member moves that member and nothing else — which
    // is what lets the panel beside the editor write a tag without writing back
    // a body somebody is still typing.
    fixture
        .client
        .update_document(
            "s-editor",
            &DocumentEdits {
                tags: Some(vec!["editor".to_owned()]),
                archived: Some(true),
                fields: Some(
                    [("status".to_owned(), serde_json::json!("in_progress"))]
                        .into_iter()
                        .collect(),
                ),
                ..DocumentEdits::default()
            },
        )
        .expect("the metadata edit is written");

    let patched = fixture
        .client
        .document("s-editor")
        .expect("the record reads back")
        .expect("it still exists");
    assert_eq!(
        patched.content, "# New\n",
        "the body nobody edited is intact"
    );
    assert_eq!(patched.tags, vec!["editor".to_owned()]);
    assert!(patched.archived, "archiving is a field, not a deletion");
    assert_eq!(
        patched
            .fields
            .get("status")
            .and_then(|value| value.as_str()),
        Some("in_progress")
    );

    let refused = fixture
        .client
        .update_document(
            &format!("{TYPE_KIND}/spec"),
            &DocumentEdits {
                title: Some("Renamed".to_owned()),
                ..DocumentEdits::default()
            },
        )
        .expect_err("the schema is not edited as a document");
    assert!(
        refused.to_string().contains("type definition"),
        "the refusal says why: {refused}"
    );

    let definition = fixture
        .client
        .list_types()
        .expect("the corpus still reads")
        .into_iter()
        .find(|kind| kind.kind == "spec")
        .expect("the spec type is still a type");
    assert_eq!(
        definition.title, "Spec",
        "the refused write changed nothing"
    );
}

#[test]
fn a_killed_engine_is_replaced_without_losing_the_session() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    fixture
        .client
        .apply(
            "seed",
            &[spec("s-one", "First", "before the crash").to_put()],
        )
        .expect("a record exists before the crash");

    // Kill this session's engine the way a crash would, with no chance for it
    // to shut down cleanly. By pid, not by name: other tests run beside this
    // one with engines of their own — and `engine_pid` answers `None` for the
    // resident process precisely so that a test cannot kill the memory of every
    // project on the machine by reaching for a pid that is not its to take.
    let pid = fixture
        .client
        .engine_pid()
        .expect("this fixture started its own engine");
    kill_engine(pid);

    // The next call must recover on its own: restart, re-initialize,
    // re-subscribe, re-read the revision. No user action, no lost write.
    let view = fixture
        .client
        .get_record("s-one")
        .expect("the client reconnects and answers");
    assert!(
        view.record.is_some(),
        "the record written before the crash is still there"
    );

    let write = fixture
        .client
        .apply(
            "after-restart",
            &[spec("s-two", "Second", "after the crash").to_put()],
        )
        .expect("writing works after the restart");
    assert_eq!(write.changed_keys, vec!["s-two".to_owned()]);
}

#[test]
fn every_written_record_satisfies_its_published_type() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    fixture
        .client
        .apply("seed", &[spec("s-one", "First", "a body").to_put()])
        .expect("the record is written");

    let status = fixture
        .client
        .schema_status()
        .expect("the engine reports schema health");

    assert_eq!(
        status["incompatibleCount"], 0,
        "every record Sync writes satisfies the type it published: {status}"
    );
    assert_eq!(
        status["schemaActive"], true,
        "publishing the type corpus is what turns validation on"
    );
}

#[test]
fn a_same_key_race_surfaces_as_a_conflict_rather_than_an_overwrite() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    fixture
        .client
        .apply("seed", &[spec("s-one", "First", "original").to_put()])
        .expect("the record exists");

    // A second session on the same project, holding the revision as it was
    // before the first one writes again — an editor left open in another
    // window.
    let mut other = MemoryClient::connect(LaunchConfig {
        project: fixture.project.clone(),
        bundled: binary.clone(),
        log_file: fixture.log_file.clone(),
        override_binary: None,
        host_socket: None,
    })
    .expect("a second session connects");

    fixture
        .client
        .apply(
            "first-edit",
            &[spec("s-one", "First", "edited here").to_put()],
        )
        .expect("the first writer wins");

    // The second writer replays once against a fresh revision; a second
    // collision on the same key is a real conflict and must be reported, not
    // resolved by overwriting.
    let outcome = other.apply(
        "second-edit",
        &[spec("s-one", "First", "edited elsewhere").to_put()],
    );

    match outcome {
        Ok(result) => {
            // The replay succeeded, which is the intended behaviour when the
            // race has already settled. What must not happen is a silent write
            // against the stale revision.
            assert_eq!(result.changed_keys, vec!["s-one".to_owned()]);
            let view = fixture
                .client
                .get_record("s-one")
                .expect("the record still reads");
            assert!(view.record.is_some());
        }
        Err(error) => {
            assert!(
                error.is_retryable_conflict(),
                "a same-key race is reported as a conflict: {error}"
            );
        }
    }
}

#[test]
fn the_records_view_lists_the_published_types_and_counts_no_schema_as_a_record() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    fixture
        .client
        .apply("seed", &[spec("s-one", "First", "a body").to_put()])
        .expect("the record is written");

    let types = fixture
        .client
        .list_types()
        .expect("the store lists its types");

    assert_eq!(
        types.len(),
        ENTITY_KINDS.len(),
        "this fixture published every kind Sync knows how to describe"
    );
    assert_eq!(
        types[0].kind, "project",
        "the list leads with the kinds Sync describes, not with the alphabetical answer"
    );
    assert!(
        types.iter().all(|entry| entry.icon.is_some()),
        "the mark is read back out of each definition, which is where it was written"
    );

    let view = fixture
        .client
        .records(&json!({"limit": 200}), &[])
        .expect("the records view answers");

    assert_eq!(
        view.counts.total, 1,
        "eleven type definitions are schema, not claims: {:?}",
        view.counts
    );
    assert_eq!(view.counts.by_kind.get("spec"), Some(&1));
    assert!(
        !view.counts.by_kind.contains_key(TYPE_KIND),
        "the schema is not one of the project's types to browse"
    );
    assert_eq!(view.records.len(), 1, "the page carries no schema records");
    assert_eq!(view.records[0].key, "s-one");
    assert_eq!(view.records[0].scope, vec!["src/".to_owned()]);
    assert!(
        !view.records[0].freshness.is_empty(),
        "every row states how far it can be trusted"
    );
}

#[test]
fn opening_a_project_twice_publishes_its_types_once() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);

    let published = fixture
        .client
        .publish_types()
        .expect("a project with no corpus gets the one definition Sync needs");
    let after_first = fixture.client.revision().to_owned();

    // The second open of the same project in the same session. Under a fixed
    // transaction id this failed with `transaction_reused`, and the window lost
    // its list of types.
    let republished = fixture
        .client
        .publish_types()
        .expect("opening the project again does not fail");

    assert!(published, "the corpus was missing and had to be written");
    assert!(!republished, "an unchanged corpus is not written again");
    assert_eq!(
        fixture.client.revision(),
        after_first,
        "no commit lands on refs/memory for a corpus that already matched"
    );
    let types = fixture
        .client
        .list_types()
        .expect("the store lists its types");
    assert_eq!(
        types.len(),
        OWN_KINDS.len(),
        "a new project is given one type — its own — and decides the rest for itself: {types:?}"
    );
    assert_eq!(types[0].kind, "project");
    assert_eq!(
        types[0].icon.as_deref(),
        Some("folder-git-2"),
        "the type Sync publishes arrives with the mark it is drawn with"
    );
}

#[test]
fn a_project_can_add_a_type_of_its_own_and_write_records_of_it() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .publish_types()
        .expect("the project's own type lands");

    fixture
        .client
        .create_type(
            "hypothesis",
            "Hypothesis",
            "Something to test",
            "flask-conical",
        )
        .expect("a project can name a type this build has never heard of");

    let types = fixture.client.list_types().expect("the store lists them");
    let added = types
        .iter()
        .find(|entry| entry.kind == "hypothesis")
        .expect("the new type is in the corpus");
    assert_eq!(added.description, "Something to test");
    assert_eq!(
        added.icon.as_deref(),
        Some("flask-conical"),
        "the mark survives the round trip through the record's own content"
    );

    // The point of a type is that the engine then accepts records of it.
    let record = json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": {
            "envelope_version": {"major": 1, "minor": 0},
            "key": "h-one",
            "kind": "hypothesis",
            "title": "The index rebuilds on unlock",
            "content": "Worth testing.",
            "content_hash": sync_memory::content_hash("Worth testing."),
            "tags": [],
            "links": [],
            "source_paths": {"observed": [], "scope": []},
            "archive": {"archived": false},
            "freshness": {"state": "unverified"},
        }
    }});
    let write = fixture
        .client
        .apply("seed-hypothesis", &[record])
        .expect("the strict schema accepts a record of a type the project defined");
    assert_eq!(write.changed_keys, vec!["h-one".to_owned()]);

    // Sync's own type is not one of the project's to redefine: it is
    // republished whenever a project lacks it, so a project-side edit would be
    // silently corrected on the next open.
    let refused =
        fixture
            .client
            .create_type("project", "Project", "Something else entirely", "bug");
    assert!(
        refused.is_err(),
        "`project` is always present and is not the project's to change"
    );
}

#[test]
fn redefining_a_type_keeps_its_records_and_removing_it_takes_them() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .publish_types()
        .expect("the project's own type lands");
    fixture
        .client
        .create_type(
            "hypothesis",
            "Hypothesis",
            "Something to test",
            "flask-conical",
        )
        .expect("the project names a type of its own");
    fixture
        .client
        .apply(
            "seed-hypotheses",
            &[
                hypothesis("h-one", "The index rebuilds on unlock"),
                hypothesis("h-two", "The engine restarts under load"),
            ],
        )
        .expect("two records of it are written");

    fixture
        .client
        .update_type(
            "hypothesis",
            "Working hypothesis",
            "Something the project believes",
            "lightbulb",
        )
        .expect("a type the project named is a type it can redefine");

    let types = fixture.client.list_types().expect("the store lists them");
    let edited = types
        .iter()
        .find(|entry| entry.kind == "hypothesis")
        .expect("the type is still in the corpus");
    assert_eq!(
        edited.title, "Working hypothesis",
        "the name a person reads is free to change"
    );
    assert_eq!(
        edited.kind, "hypothesis",
        "the identifier records carry is not"
    );
    assert_eq!(edited.description, "Something the project believes");
    assert_eq!(edited.icon.as_deref(), Some("lightbulb"));

    // An edit is an edit of the definition and of nothing else: the records
    // written as this type are still readable, and still of it.
    let listing = fixture
        .client
        .list_records(&json!({"kind": "hypothesis", "limit": 10}))
        .expect("the records are listed");
    assert_eq!(listing.records.len(), 2, "a redefinition keeps its records");

    // Removing the type takes them, because a record whose kind has no
    // definition is one the strict schema will not let anybody read or rewrite.
    let removed = fixture
        .client
        .delete_type("hypothesis")
        .expect("a type the project named is a type it can remove");
    assert_eq!(removed, 2, "the count reports what actually went");

    let types = fixture.client.list_types().expect("the store lists them");
    assert!(
        !types.iter().any(|entry| entry.kind == "hypothesis"),
        "the definition is gone"
    );
    let listing = fixture
        .client
        .list_records(&json!({"kind": "hypothesis", "limit": 10}))
        .expect("the store answers for a kind it no longer holds");
    assert!(listing.records.is_empty(), "and so is everything of it");

    // Sync's own type is not the project's to redefine or to remove: deleting
    // `project` would leave the record naming the project with a kind the
    // strict schema rejects.
    assert!(
        fixture
            .client
            .update_type("project", "Project", "Anything", "bug")
            .is_err(),
        "`project` is always present and is not the project's to change"
    );
    assert!(
        fixture.client.delete_type("project").is_err(),
        "and it is not the project's to remove"
    );
    assert!(
        fixture
            .client
            .update_type("nothing_named_this", "Nothing", "", "bug")
            .is_err(),
        "a type the project does not hold cannot be redefined into existence"
    );
}

#[test]
fn removing_a_type_takes_its_archived_records_and_the_count_says_so() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .publish_types()
        .expect("the project's own type lands");
    fixture
        .client
        .create_type("hypothesis", "Hypothesis", "Something to test", "shapes")
        .expect("the project names a type of its own");

    let mut archived = hypothesis("h-old", "Settled long ago");
    archived["record"]["envelope"]["archive"]["archived"] = json!(true);
    fixture
        .client
        .apply("seed-hypotheses", &[hypothesis("h-one", "Live"), archived])
        .expect("one live record and one archived one are written");

    // An archived record is still a record of the type, so it is still one the
    // strict schema would strand. The confirmation counts the way the window
    // counts — one metadata read asked for its counts — and that number has to
    // be the number that actually goes.
    let counted = fixture
        .client
        .list_records(&json!({"kind": "hypothesis", "limit": 1, "metadata_only": true}))
        .expect("the store counts them")
        .counts
        .total;
    assert_eq!(
        counted, 2,
        "the count a person is asked to confirm includes archived records"
    );

    let removed = fixture
        .client
        .delete_type("hypothesis")
        .expect("the type goes");
    assert_eq!(removed, counted, "and the removal takes exactly those");

    let left = fixture
        .client
        .list_records(&json!({"kind": "hypothesis", "limit": 10}))
        .expect("the store answers for a kind it no longer holds");
    assert!(
        left.records.is_empty(),
        "a record of a type that no longer exists is one nothing could ever read again"
    );
}

#[test]
fn a_type_named_in_another_script_is_stored_under_a_generated_identifier() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .publish_types()
        .expect("the project's own type lands");

    // What the window sends for a name no kind alphabet can carry: an
    // identifier it generated, and the name exactly as it was typed.
    fixture
        .client
        .create_type(
            "type_k3n8q2",
            "Открытый вопрос",
            "Что-то, что ещё не решено",
            "circle-help",
        )
        .expect("a project writes its knowledge in its own language");

    let types = fixture.client.list_types().expect("the store lists them");
    let generated = types
        .iter()
        .find(|entry| entry.kind == "type_k3n8q2")
        .expect("the generated identifier is what the corpus keys on");
    assert_eq!(
        generated.title, "Открытый вопрос",
        "the name is stored as it was typed, whatever alphabet it is in"
    );

    assert!(
        fixture
            .client
            .create_type("", "Nameless", "", "bug")
            .is_err(),
        "a definition addressed by an empty key would describe a kind nobody can name"
    );
    assert!(
        fixture
            .client
            .create_type(TYPE_KIND, "Type", "", "bug")
            .is_err(),
        "the kind definitions are stored as is not a kind a project can define"
    );
}

/// A record of the type the tests above name, as a transaction operation.
fn hypothesis(key: &str, title: &str) -> serde_json::Value {
    let content = "Worth testing.";
    json!({"op": "put", "record": {
        "representation": "plaintext",
        "envelope": {
            "envelope_version": {"major": 1, "minor": 0},
            "key": key,
            "kind": "hypothesis",
            "title": title,
            "content": content,
            "content_hash": sync_memory::content_hash(content),
            "tags": [],
            "links": [],
            "source_paths": {"observed": [], "scope": []},
            "archive": {"archived": false},
            "freshness": {"state": "unverified"},
        }
    }})
}

/// Kill one engine process, and wait for it to actually be gone.
fn kill_engine(pid: u32) {
    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
    // `kill` returns as soon as the signal is delivered; the pipe closes a
    // moment later. Without this the next call can win the race and succeed
    // against a process that is already dying, which would test nothing.
    for _ in 0..50 {
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .is_ok_and(|output| output.status.success());
        if !alive {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_record_is_created_from_its_type_and_deleted_with_what_depends_on_it() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");

    let created = fixture
        .client
        .create_document("spec", "A spec created in the window", None)
        .expect("the record is created");
    assert!(
        created.key.starts_with("spec-"),
        "the key says which kind it is: {}",
        created.key
    );
    assert_eq!(created.title, "A spec created in the window");
    assert_eq!(created.content, "", "a new record is empty prose");
    assert_eq!(
        created
            .fields
            .get("status")
            .and_then(|value| value.as_str()),
        Some("backlog"),
        "the required field its type declares is filled from the definition"
    );

    // A type that declares the relation. The engine validates every link
    // against the declarations and rejects a relation the type does not name, so
    // a link cannot be written until one exists — which is why the window offers
    // the declared relations and nothing else.
    fixture
        .client
        .apply(
            "relations",
            &[type_record(
                "spec",
                &json!({
                    "kind_name": "spec",
                    "title": "Spec",
                    "description": "A unit of work.",
                    "icon": "rule",
                    "fields": {"status": {"type": "enum", "values": ["backlog", "todo"], "required": true, "default": "backlog"}},
                    "relationships": {"depends_on": {"target": "any"}},
                }),
            )],
        )
        .expect("the definition declaring the relation lands");

    // Two records that hold on to it in the two different ways.
    let mut dependent = spec("s-dependent", "Depends on it", "Body");
    dependent.links = vec![Link {
        key: created.key.clone(),
        relation: "depends_on".to_owned(),
    }];
    let mentioner = spec(
        "s-mentions",
        "Mentions it",
        &format!("As decided in {}, this is settled.", created.key),
    );
    fixture
        .client
        .apply("holders", &[dependent.to_put(), mentioner.to_put()])
        .expect("both are written");

    let holding = fixture
        .client
        .dependents(&created.key)
        .expect("the store answers what holds on to it");
    assert_eq!(
        holding.links.len(),
        1,
        "an explicit link is a structural dependency: {holding:?}"
    );
    assert_eq!(holding.links[0].key, "s-dependent");
    assert_eq!(holding.links[0].relation.as_deref(), Some("depends_on"));
    assert!(
        holding
            .mentions
            .iter()
            .any(|entry| entry.key == "s-mentions"),
        "and a body that names the key is a mention, not a dependency: {holding:?}"
    );

    // Deleting the record together with the one that links to it: one
    // transaction, and the mention is left alone.
    fixture
        .client
        .delete_documents(&[created.key.clone(), "s-dependent".to_owned()])
        .expect("both go");
    assert!(
        fixture
            .client
            .document(&created.key)
            .expect("the read succeeds")
            .is_none()
    );
    assert!(
        fixture
            .client
            .document("s-dependent")
            .expect("the read succeeds")
            .is_none()
    );
    assert!(
        fixture
            .client
            .document("s-mentions")
            .expect("the read succeeds")
            .is_some(),
        "a record that only mentioned the key keeps its own reasoning"
    );
}

#[test]
fn a_record_is_created_the_way_the_window_creates_one() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);

    // A definition of the shape a corpus written by the engine's own tooling
    // actually has: no title, no icon, an enumeration with no stated default,
    // and no relations. This is what `memory-hub`'s `sync` server publishes into
    // this repository, and a window that only works against Sync's own richer
    // definitions works nowhere real.
    fixture
        .client
        .apply(
            "types",
            &[type_record(
                "constraint",
                &json!({
                    "kind_name": "constraint",
                    "description": "A rule the project must hold to.",
                    "fields": {
                        "validation_state": {
                            "required": true,
                            "type": "enum",
                            "values": ["valid", "unverified", "stale", "invalid"],
                        }
                    },
                    "relationships": {},
                }),
            )],
        )
        .expect("the definition lands");

    // Empty, because the window creates a record and then names it by typing.
    let created = fixture
        .client
        .create_document("constraint", "", None)
        .expect("an empty record of a project's own type is created");

    assert_eq!(created.title, "");
    assert_eq!(created.content, "");
    assert_eq!(
        created
            .fields
            .get("validation_state")
            .and_then(|v| v.as_str()),
        Some("valid"),
        "a required enumeration with no stated default opens at its first value"
    );
    assert!(
        fixture
            .client
            .document(&created.key)
            .expect("the read succeeds")
            .is_some(),
        "and it is in the store rather than only in the answer"
    );
}

/// A project answers to a key in the script its name is written in.
///
/// The identifier is derived from the name and keeps its letters, so a project
/// called `Мой Проект` is addressed by `МОЙ-ПРОЕКТ`. Whether a key like that
/// survives being written and read back is the store's answer to give, not
/// something the window may assume — and the alternative, restricting
/// identifiers to ASCII, would make a Russian project ask for one by hand.
#[test]
fn a_project_is_addressed_by_a_key_in_its_own_script() {
    const PROJECT_IDENTIFIER: &str = "МОЙ-ПРОЕКТ";

    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");

    let mut extensions = serde_json::Map::new();
    extensions.insert("language".to_owned(), serde_json::json!("ru"));
    fixture
        .client
        .apply(
            "project",
            &[Entity {
                key: PROJECT_IDENTIFIER.to_owned(),
                kind: EntityKind::Project.as_str().to_owned(),
                title: "Мой Проект".to_owned(),
                content: String::new(),
                tags: Vec::new(),
                links: Vec::new(),
                paths_observed: Vec::new(),
                scope_paths: Vec::new(),
                extensions,
                folder: None,
                is_folder: false,
                archived: false,
                verified: false,
            }
            .to_put()],
        )
        .expect("the project record is written");

    let project = fixture
        .client
        .document(PROJECT_IDENTIFIER)
        .expect("it reads back")
        .expect("it exists");
    assert_eq!(project.title, "Мой Проект");
}

#[test]
fn the_project_record_is_edited_but_never_created_or_deleted() {
    // The key of a project's record is the project's own identifier, derived
    // from its name — `Sync` here, as the record below says.
    const PROJECT_IDENTIFIER: &str = "SYNC";

    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");

    let mut extensions = serde_json::Map::new();
    extensions.insert("language".to_owned(), serde_json::json!("en"));
    fixture
        .client
        .apply(
            "project",
            &[Entity {
                key: PROJECT_IDENTIFIER.to_owned(),
                kind: EntityKind::Project.as_str().to_owned(),
                title: "Sync".to_owned(),
                content: "The project as it was described.".to_owned(),
                tags: Vec::new(),
                links: Vec::new(),
                paths_observed: Vec::new(),
                scope_paths: Vec::new(),
                extensions,
                folder: None,
                is_folder: false,
                archived: false,
                verified: false,
            }
            .to_put()],
        )
        .expect("the project record is written");

    // Its title is the project's name, its body is the description, and
    // `language` is the language it writes its knowledge in. All three are the
    // project's own data, and all three are edited here.
    fixture
        .client
        .update_document(
            PROJECT_IDENTIFIER,
            &DocumentEdits {
                title: Some("Sync, renamed".to_owned()),
                content: Some("A newer description.".to_owned()),
                fields: Some(
                    [("language".to_owned(), serde_json::json!("ru"))]
                        .into_iter()
                        .collect(),
                ),
                ..DocumentEdits::default()
            },
        )
        .expect("the project record is a document");

    let project = fixture
        .client
        .document(PROJECT_IDENTIFIER)
        .expect("it reads back")
        .expect("it exists");
    assert_eq!(project.title, "Sync, renamed");
    assert_eq!(project.content, "A newer description.");
    assert_eq!(
        project.fields.get("language").and_then(|v| v.as_str()),
        Some("ru")
    );

    let no_second = fixture
        .client
        .create_document("project", "A second project", None)
        .expect_err("there is one project record");
    assert!(no_second.to_string().contains("project"), "{no_second}");

    let no_delete = fixture
        .client
        .delete_documents(&[PROJECT_IDENTIFIER.to_owned()])
        .expect_err("and it cannot be deleted");
    assert!(no_delete.to_string().contains("delete"), "{no_delete}");
    assert!(
        fixture
            .client
            .document(PROJECT_IDENTIFIER)
            .expect("it reads back")
            .is_some(),
        "the project is still openable"
    );
}

/// Write a file into the project the way a person would: with an editor, and
/// with no idea that anything is watching.
fn write_document(project: &Path, path: &str, body: &str) {
    let file = project.join(path);
    std::fs::create_dir_all(file.parent().expect("a document has a directory"))
        .expect("the directory is created");
    std::fs::write(file, body).expect("the document is written");
}

#[test]
fn attaching_a_folder_turns_its_documents_into_records_and_writes_nothing_into_it() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    write_document(&fixture.project, "docs/api/auth.md", "# Auth\n\nTokens.\n");
    // A document of this type as much as the prose is. There is no mask any
    // more: a folder holds what it holds, and a diagram beside the pages it
    // illustrates is one of the folder's documents rather than a file Memory
    // pretends not to see.
    write_document(&fixture.project, "docs/logo.svg", "<svg/>\n");
    let before = std::fs::read_to_string(fixture.project.join("docs/setup.md"))
        .expect("the document is there to begin with");

    let scan = fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached and scanned");

    assert_eq!(
        scan.changes
            .iter()
            .filter(|change| change["change"] == "new")
            .count(),
        3,
        "every file in the folder became a record, the diagram included: {:?}",
        scan.changes
    );
    assert_eq!(
        std::fs::read_to_string(fixture.project.join("docs/setup.md")).expect("still there"),
        before,
        "attaching writes nothing into the folder — that is the whole promise"
    );

    let view = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the records view answers");
    let setup = view
        .records
        .iter()
        .find(|record| record.locator.as_deref() == Some("docs/setup.md"))
        .expect("the record points at the file it was made from");
    assert_eq!(setup.presence, "present");

    // The body is not in the record: it is in the file, and reading it is the
    // one operation that goes outside.
    let document = fixture
        .client
        .document(&setup.key)
        .expect("the document reads")
        .expect("it is still there");
    assert_eq!(document.content, "# Setup\n\nClone it.\n");
    assert!(!document.content_missing);
}

#[test]
fn editing_an_attached_document_writes_the_file_a_colleague_will_review() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");
    let key = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .first()
        .expect("one record")
        .key
        .clone();

    fixture
        .client
        .update_document(
            &key,
            &DocumentEdits {
                content: Some("# Setup\n\nClone it, then run it.\n".to_owned()),
                title: Some("Setting up".to_owned()),
                ..DocumentEdits::default()
            },
        )
        .expect("the edit lands");

    assert_eq!(
        std::fs::read_to_string(fixture.project.join("docs/setup.md")).expect("the file is there"),
        "# Setup\n\nClone it, then run it.\n",
        "the body went to the file, which is where the team reads it"
    );
    let document = fixture
        .client
        .document(&key)
        .expect("the document reads")
        .expect("it is there");
    assert_eq!(document.title, "Setting up");
    assert_eq!(document.content, "# Setup\n\nClone it, then run it.\n");
}

#[test]
fn a_document_that_leaves_the_working_tree_is_missing_rather_than_gone() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");
    let key = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .first()
        .expect("one record")
        .key
        .clone();

    std::fs::remove_file(fixture.project.join("docs/setup.md")).expect("somebody deletes it");
    fixture.client.scan().expect("the scan notices");

    let document = fixture
        .client
        .document(&key)
        .expect("the record is still readable")
        .expect("and it is still there");
    assert!(
        document.content_missing,
        "the body could not be read, which is not the same as an empty document"
    );
    assert_ne!(
        document.presence, "present",
        "the record says why its document is not here"
    );

    let refused = fixture.client.update_document(
        &key,
        &DocumentEdits {
            content: Some("something typed over a document that is not here".to_owned()),
            ..DocumentEdits::default()
        },
    );
    assert!(
        refused.is_err(),
        "writing would create the file here and fork a document that exists elsewhere"
    );
}

#[test]
fn a_rename_with_an_edit_waits_for_a_person_and_keeps_the_record_its_key() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");
    let key = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .first()
        .expect("one record")
        .key
        .clone();

    // Renamed and edited in one stroke: no path matches and no digest matches,
    // which is exactly what a genuinely new file looks like.
    std::fs::remove_file(fixture.project.join("docs/setup.md")).expect("the old name goes");
    write_document(
        &fixture.project,
        "docs/setting-up.md",
        "# Setup\n\nClone it, then run it.\n",
    );

    let scan = fixture.client.scan().expect("the scan runs");
    let file = scan
        .changes
        .iter()
        .find(|change| change["change"] == "unmatched")
        .expect("the stray file is carried out");
    assert_eq!(file["locator"], "docs/setting-up.md");
    assert_eq!(
        file["candidates"][0]["key"], key,
        "the record it could be is ranked first: {file:?}"
    );

    let locator = file["locator"].as_str().expect("a locator").to_owned();
    let hash = file["contentHash"].as_str().expect("a digest").to_owned();
    fixture
        .client
        .resolve_unmatched(&locator, &hash, "guide", Some(&key))
        .expect("a person says it is the same document");

    let document = fixture
        .client
        .document(&key)
        .expect("the record reads")
        .expect("under the key it always had");
    assert_eq!(
        document.locator.as_deref(),
        Some("docs/setting-up.md"),
        "the locator followed the file and the key did not move, so no link broke"
    );
    assert_eq!(document.content, "# Setup\n\nClone it, then run it.\n");
}

#[test]
fn a_document_is_named_by_its_own_heading_and_keeps_that_name() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(
        &fixture.project,
        "docs/setup.md",
        "# Setting up a development machine\n\nClone it.\n",
    );
    // Front matter is metadata, not prose: the heading under it still names the
    // document.
    write_document(
        &fixture.project,
        "docs/api.md",
        "---\ntitle: ignored\n---\n\nHTTP API\n========\n\nTokens.\n",
    );
    // No heading, so nothing to take. The record is named by its key rather
    // than by a sentence out of the middle of somebody's paragraph.
    write_document(&fixture.project, "docs/notes.md", "Just a paragraph.\n");

    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    let titles: std::collections::BTreeMap<String, String> = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .iter()
        .map(|record| {
            (
                record.locator.clone().unwrap_or_default(),
                record.title.clone(),
            )
        })
        .collect();

    assert_eq!(
        titles.get("docs/setup.md").map(String::as_str),
        Some("Setting up a development machine")
    );
    assert_eq!(
        titles.get("docs/api.md").map(String::as_str),
        Some("HTTP API")
    );
    assert_eq!(
        titles.get("docs/notes.md").map(String::as_str),
        Some(""),
        "a document that names itself nothing is not given a name out of its prose"
    );

    // Renaming the record is a person's decision. A later scan re-reads the
    // file and must not put the heading back over the top of it.
    let key = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .iter()
        .find(|record| record.locator.as_deref() == Some("docs/setup.md"))
        .expect("the record is there")
        .key
        .clone();
    fixture
        .client
        .update_document(
            &key,
            &DocumentEdits {
                title: Some("How we set up".to_owned()),
                ..DocumentEdits::default()
            },
        )
        .expect("the rename lands");
    fixture.client.scan().expect("a later scan runs");

    assert_eq!(
        fixture
            .client
            .document(&key)
            .expect("the record reads")
            .expect("it is there")
            .title,
        "How we set up",
        "the name a person gave it is not overwritten by the document's heading"
    );
}

#[test]
fn deleting_a_record_takes_the_document_it_points_at() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    write_document(&fixture.project, "docs/api.md", "# API\n\nTokens.\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    let key = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the view answers")
        .records
        .iter()
        .find(|record| record.locator.as_deref() == Some("docs/setup.md"))
        .expect("the record is there")
        .key
        .clone();

    fixture
        .client
        .delete_documents(std::slice::from_ref(&key))
        .expect("the record goes");

    assert!(
        !fixture.project.join("docs/setup.md").exists(),
        "a record owns its document, so deleting one takes the other"
    );
    assert!(
        fixture.project.join("docs/api.md").is_file(),
        "and takes nothing else"
    );

    // The point of taking it: a file left behind is a deletion the next scan
    // undoes, handing the document back as a record with a new key.
    fixture.client.scan().expect("a later scan runs");
    assert!(
        fixture
            .client
            .records(&json!({"kind": "guide", "limit": 50}), &[])
            .expect("the view answers")
            .records
            .iter()
            .all(|record| record.locator.as_deref() != Some("docs/setup.md")),
        "the deletion stays done"
    );
}

#[test]
fn removing_an_attached_type_takes_its_records_and_leaves_the_files() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    write_document(&fixture.project, "docs/api.md", "# API\n\nTokens.\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    // The definition lives in refs and the records are of a kind stored in a
    // repository folder, so this cannot be one transaction — and a person
    // removing a type should never have to know that.
    let removed = fixture
        .client
        .delete_type("guide")
        .expect("the type and its records go");

    assert_eq!(removed, 2);
    assert!(
        fixture
            .client
            .list_types()
            .expect("the corpus lists its types")
            .iter()
            .all(|entry| entry.kind != "guide"),
        "the definition went with the records"
    );
    assert!(
        fixture.project.join("docs/setup.md").is_file()
            && fixture.project.join("docs/api.md").is_file(),
        "the documents are the team's; Memory never wrote them and does not delete them"
    );
}

#[test]
fn adopting_a_stray_file_as_new_never_lands_on_a_record_somebody_else_holds() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("type definitions land");
    write_document(&fixture.project, "docs/setup.md", "# Setup\n\nClone it.\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    // A record somebody wrote by hand, under the key a document would derive.
    // Keys are one namespace across the corpus, so this is an ordinary
    // collision rather than a contrived one.
    fixture
        .client
        .apply(
            "decision",
            &[spec("setup-guide", "How we set machines up", "The decision.").to_put()],
        )
        .expect("the record is written");

    // The document goes, and one arrives whose name is close enough to be the
    // same document renamed — which is the state nothing can decide alone.
    std::fs::remove_file(fixture.project.join("docs/setup.md")).expect("it goes");
    write_document(
        &fixture.project,
        "docs/setup-guide.md",
        "# Setup\n\nClone it, then run it.\n",
    );
    let scan = fixture.client.scan().expect("the scan runs");
    let stray = scan
        .changes
        .iter()
        .find(|change| change["change"] == "unmatched")
        .expect("a file that could be the one that moved");

    fixture
        .client
        .resolve_unmatched(
            stray["locator"].as_str().expect("a locator"),
            stray["contentHash"].as_str().expect("a digest"),
            "guide",
            None,
        )
        .expect("a person says it is a document of its own");

    let held = fixture
        .client
        .document("setup-guide")
        .expect("the hand-written record reads")
        .expect("and it is still there");
    assert_eq!(
        held.title, "How we set machines up",
        "a derived key must not take over a record somebody else wrote"
    );
    assert!(
        held.locator.is_none(),
        "and it must not have become a pointer at somebody's file"
    );
}

#[test]
fn a_project_that_has_never_kept_memory_is_initialised_in_git_when_it_opens() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    // The fixture is a repository nobody has run anything in, so connecting is
    // the whole test: the engine refuses to guess where a project keeps its
    // memory, Sync answers Git's own metadata when it opens, and opening that
    // storage is what creates it.
    let fixture = fixture(&binary);

    assert!(
        fixture.project.join(".git/refs/memory/main").exists()
            || std::fs::read_to_string(fixture.project.join(".git/packed-refs"))
                .is_ok_and(|packed| packed.contains("refs/memory/main")),
        "memory that travels with the repository and puts nothing in the \
         working tree"
    );
    assert!(
        !fixture.project.join(".memory").exists(),
        "and nothing beside it: there is no configuration file any more"
    );

    let handshake = &fixture.client.info().handshake;
    assert!(
        handshake.records_are_git(),
        "and the engine agrees about what it is serving: {:?}",
        handshake.backend
    );
    // Opening is not the same as writing: a project opened for the first time
    // still publishes Sync's own types, and reads answer normally.
    assert!(
        !fixture.client.revision().is_empty(),
        "a revision is readable the moment the project is opened"
    );
}

#[test]
fn attaching_a_folder_puts_the_folder_in_the_type_that_names_it() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n");
    write_document(&fixture.project, "notes/monday.md", "# Monday\n");

    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");
    fixture
        .client
        .attach_folder("note", "Note", "Working notes", "book", "notes")
        .expect("the second folder is attached");

    let types = fixture.client.list_types().expect("the corpus reads");
    let folder_of = |kind: &str| {
        types
            .iter()
            .find(|type_| type_.kind == kind)
            .unwrap_or_else(|| panic!("`{kind}` is in the corpus"))
            .storage
            .folder
            .clone()
    };
    assert_eq!(
        folder_of("guide").as_deref(),
        Some("docs"),
        "the type carries the path itself, so nothing has to be looked up"
    );
    assert_eq!(
        folder_of("note").as_deref(),
        Some("notes"),
        "and two folders are two types, each naming its own"
    );

    let guide = types
        .iter()
        .find(|type_| type_.kind == "guide")
        .expect("the type is in the corpus");
    assert!(
        guide.writable,
        "a folder that is there and writable is a type documents can be created in"
    );
}

#[test]
fn a_document_created_in_an_attached_folder_is_a_file_a_colleague_can_open() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    write_document(&fixture.project, "docs/setup.md", "# Setup\n");
    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    let created = fixture
        .client
        .create_document("guide", "", None)
        .expect("a document of an attached type is created");
    let locator = created
        .locator
        .clone()
        .expect("it is a record pointing at a file");
    assert_eq!(
        locator, "docs/untitled.md",
        "named for the document rather than for the record, and Markdown because \
         that is what this window writes"
    );
    assert!(
        fixture.project.join(&locator).is_file(),
        "the file is on disk, where an editor and a pull request can see it"
    );

    // And the second one does not land on the first.
    let second = fixture
        .client
        .create_document("guide", "", None)
        .expect("a second document is created");
    assert_eq!(second.locator.as_deref(), Some("docs/untitled-2.md"));
}

#[test]
fn a_document_that_is_not_text_is_described_rather_than_opened() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    // A PNG's first bytes, which are not valid UTF-8 — the whole point. Before
    // interface 2 a folder's mask kept this out; now the folder holds what it
    // holds, and the client is the one that must not show it as prose.
    let diagram = fixture.project.join("docs/diagram.png");
    std::fs::create_dir_all(diagram.parent().expect("a directory")).expect("the directory");
    std::fs::write(
        &diagram,
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff],
    )
    .expect("the picture is written");

    fixture
        .client
        .attach_folder("guide", "Guide", "Team documentation", "book", "docs")
        .expect("the folder is attached");

    let view = fixture
        .client
        .records(&json!({"kind": "guide", "limit": 50}), &[])
        .expect("the records view answers");
    let record = view
        .records
        .iter()
        .find(|record| record.locator.as_deref() == Some("docs/diagram.png"))
        .expect("the picture is a document of the folder like anything else");
    assert_eq!(record.media_type.as_deref(), Some("image/png"));

    let document = fixture
        .client
        .document(&record.key)
        .expect("the document reads")
        .expect("and it is there");
    assert!(
        document.content_binary,
        "the window is told what it is holding rather than left to render base64"
    );
    assert!(
        document.content.is_empty(),
        "and it is not handed bytes it cannot edit: {:?}",
        document.content
    );
    assert!(
        !document.content_missing,
        "the file is here — not being text is a different thing from not being there"
    );
    assert_eq!(
        document.title, "",
        "nothing invents a title out of an image's bytes; the key says what the file is called"
    );
}

#[test]
fn reopening_a_project_reads_the_memory_it_has_rather_than_starting_one() {
    let Some(binary) = engine_binary() else {
        eprintln!("skipping: no sync-mcp binary (set SYNC_MCP_BINARY)");
        return;
    };
    let mut fixture = fixture(&binary);
    fixture
        .client
        .apply("types", &type_definitions(ENTITY_KINDS))
        .expect("the corpus is published");
    fixture
        .client
        .apply(
            "before",
            &[spec("s-old", "Written before this window closed", "Body.").to_put()],
        )
        .expect("a record is written");

    // Nothing on disk says where the records are: Sync tells the engine when it
    // opens, and opening the storage is what would create one. So this is the
    // case that matters — opening a second time must find what the first left,
    // not start over beside it.
    drop(fixture.client);

    let mut reopened = MemoryClient::connect(LaunchConfig {
        project: fixture.project.clone(),
        bundled: binary.clone(),
        log_file: fixture.log_file.clone(),
        override_binary: None,
        host_socket: None,
    })
    .expect("opening it again reads the memory rather than refusing to");

    let held = reopened
        .document("s-old")
        .expect("the record reads")
        .expect("and it is still there");
    assert_eq!(
        held.title, "Written before this window closed",
        "opening the storage finds it; it does not make a new one"
    );
    assert!(
        reopened.info().handshake.records_are_git(),
        "and they are where they always were"
    );
}
