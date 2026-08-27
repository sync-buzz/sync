//! The tools that are Sync's rather than the engine's.
//!
//! Each earns its place by answering something the engine cannot be asked. The
//! engine knows records; it does not know what this project *is*, what it is
//! composed of, or how Sync expects to be worked with. Those are product
//! questions, and a client that had to assemble the answers out of
//! `memory_list_records` calls would be reimplementing Sync badly.
//!
//! [`SPEAK`] is the odd one and is here for the same reason turned around: it
//! answers nothing at all. It is the machine's speakers, and no engine has
//! any. It is also the only tool that is published *conditionally* — see
//! [`tools`].
//!
//! They are named `sync_*` so the two halves of the surface never collide: the
//! engine's tools keep the engine's own names, and adding one upstream can
//! therefore never shadow one of these.

use std::fmt::Write as _;

use memory_hub_mcp::ToolCall;
use rmcp::model::Tool;
use serde_json::{Value, json};
use sync_memory::{InstalledExtension, MemoryError, MemoryPresence, Result};

use crate::contributed::Registry;
use crate::domain::Domain;

/// Which projects this machine answers for.
pub const PROJECTS: &str = "sync_projects";
/// What this project is, what it is composed of, and what to read next.
pub const PROJECT: &str = "sync_project";
/// How Sync expects to be worked with, by topic.
pub const INSTRUCTIONS: &str = "sync_instructions";
/// The one write.
pub const APPLY: &str = "sync_apply";
/// The one that makes a sound.
pub const SPEAK: &str = "sync_speak";

/// Whether a name is one of ours.
#[must_use]
pub fn is_ours(name: &str) -> bool {
    matches!(name, PROJECTS | PROJECT | INSTRUCTIONS | APPLY | SPEAK)
}

/// Ours, described for a model.
///
/// The descriptions are written here rather than derived from anything, because
/// unlike the engine's tools these have no upstream to inherit a voice from.
///
/// # Why one of them is conditional
///
/// `may_speak` is what the person set on Sync's Voice page, and a `false` there
/// leaves [`SPEAK`] out of the catalogue entirely rather than publishing a tool
/// that refuses. A tool a model can see is a tool it will spend a turn calling,
/// and finding out afterwards that speaking was switched off costs that turn
/// for nothing. The refusal in [`speak`] is still written, for the model that
/// calls a name it remembers from a catalogue read before the switch moved.
#[must_use]
pub fn tools(may_speak: bool) -> Vec<Tool> {
    let mut tools = vec![
        tool(
            PROJECTS,
            "The projects this machine answers for, with the key each one is \
             called by. Every other tool takes one of these keys as its \
             `project` argument, so this is where a session starts: nothing \
             else can be called without a key, and a key cannot be guessed \
             from a directory name.",
            &json!({"type": "object", "properties": {}, "additionalProperties": false}),
        ),
        tool(
            PROJECT,
            "What one project is: its name, what it is about, the language it \
             writes in, the kinds it holds and the extensions it is composed of. \
             Read this before working in a project — the kinds it names are the \
             vocabulary every other tool's `kind` argument is drawn from, and \
             they differ from project to project.",
            &json!({"type": "object", "properties": {}, "additionalProperties": false}),
        ),
        tool(
            INSTRUCTIONS,
            "How Sync expects this project to be worked with. Called with no \
             arguments it lists the topics and when each one applies; called \
             with `topic` it returns that one. Read the topic before your first \
             action in its area, once per session.",
            &json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "One of the topics `sync_instructions` lists. Omit to get the list."
                    }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            APPLY,
            "Create, update or delete records in one transaction, all of them or \
             none. Pass records as Sync states them — the transaction id, the \
             revision the write expects, the envelope version and the digest of \
             the content are this server's to supply, and none of them is worth \
             your turn. A write that loses a race against another writer is \
             replayed once against the revision that won rather than handed \
             back for you to retry; a conflict that survives the replay is \
             reported as one. Keys are permanent: writing to a key that exists \
             replaces that record, and there is no rename.",
            &json!({
                "type": "object",
                "properties": {
                    "save": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string", "description": "Permanent. What every link refers to."},
                                "kind": {"type": "string", "description": "One of the kinds `sync_project` names."},
                                "title": {"type": "string"},
                                "content": {"type": "string", "description": "Markdown."},
                                "tags": {"type": "array", "items": {"type": "string"}},
                                "links": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {"key": {"type": "string"}, "relation": {"type": "string"}},
                                        "required": ["key", "relation"]
                                    }
                                },
                                "paths_observed": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "Files this record was written against — the evidence."
                                },
                                "scope_paths": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "Files the claim covers. The engine marks the record stale when they change."
                                },
                                "fields": {
                                    "type": "object",
                                    "description": "Product fields for the kind, validated against its type definition."
                                }
                            },
                            "required": ["key", "kind", "title", "content"]
                        }
                    },
                    "delete": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Keys to remove. A type definition and the project's own record are refused."
                    }
                },
                "additionalProperties": false
            }),
        ),
    ];

    if may_speak {
        tools.push(speaking());
    }

    tools
}

/// The tool that makes a sound, described for a model.
///
/// Its own function because the catalogue above is already at the length a
/// reader can hold, and because what this one says to a model is the whole of
/// how well it behaves: everything about *when not to speak* is here.
fn speaking() -> Tool {
    tool(
        SPEAK,
        "Say something out loud, through the speakers of the machine Sync is \
             running on. For telling somebody something while they are not looking at a \
             screen — a long job that finished, a watch that found what it was watching \
             for. One or two spoken sentences: this is a room, not a transcript, and \
             there is no way to scroll back. Say nothing when there is nothing worth \
             interrupting somebody for. The voice, the language and the speed are the \
             person's own settings and are not yours to pass.",
        &json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "What to say, as it should sound. Written for an ear: no Markdown, no code, no file paths."
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Stop whatever is being said and say this instead. Default false, which waits its turn."
                }
            },
            "required": ["text"],
            "additionalProperties": false
        }),
    )
}

/// Say something, if the person left that switched on.
///
/// It is the one tool here that does not take a project, and the one that
/// touches no memory: speakers belong to a machine. Which is also why it is
/// answered before a call is resolved to a project rather than inside a domain.
///
/// # Errors
///
/// When speaking is switched off for agents, when there is nothing to say, or
/// when the engine refused — a chosen voice removed in System Settings is the
/// case that actually happens.
pub fn speak(configuration: Option<&std::path::Path>, arguments: &Value) -> Result<Value> {
    let Some(directory) = configuration else {
        return Err(MemoryError::domain(
            "voice_unavailable",
            "this server does not know where Sync keeps its settings, so it cannot tell whether              speaking is allowed"
                .to_owned(),
            Value::Null,
        ));
    };

    // Read per call rather than held: the switch is a file the window rewrites,
    // and an agent connected yesterday must not go on speaking because it was
    // allowed to when it connected.
    let preference = sync_voice::preference(directory);
    if !preference.agents {
        return Err(MemoryError::domain(
            "voice_refused",
            "speaking is switched off for agents on this machine. Sync's settings, under Voice,              is where somebody turns it on — asking again will not change it."
                .to_owned(),
            Value::Null,
        ));
    }

    let text = arguments
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let interrupt = arguments
        .get("interrupt")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    sync_voice::say(&preference, text, interrupt)
        .map_err(|error| MemoryError::domain("voice_failed", error.to_string(), Value::Null))?;
    Ok(json!({"said": text}))
}

/// Run one of ours, answering in the shape the engine's tools answer in.
///
/// A [`ToolCall`] rather than a `Result`, so the surface has one way of turning
/// an answer into MCP regardless of which half of it answered.
pub fn call(
    domain: &mut Domain,
    contributed: &Registry,
    name: &str,
    arguments: &Value,
) -> ToolCall {
    let answer = match name {
        PROJECT => project(domain),
        INSTRUCTIONS => instructions(domain, contributed, arguments),
        APPLY => apply(domain, arguments),
        _ => Err(MemoryError::domain(
            "tool_not_found",
            format!("no tool named `{name}`"),
            Value::Null,
        )),
    };
    crate::engine::as_tool_call(answer)
}

/// Whether the project's memory can be read, having read its revision if so.
///
/// Asked of the repository, not of the store, and that is the whole of it:
/// opening a store creates one. The records live under `refs/memory/*`, and a
/// read that opened them would give somebody's repository memory on the
/// strength of a question being asked. `memory_presence` counts what is
/// actually there and writes nothing.
///
/// These two tools are how an agent orients itself, and a repository with no
/// Sync memory is exactly when orientation matters — refusing the tool that
/// would explain what that means leaves a client with an error and no next
/// step.
///
/// Creating the memory here is not the alternative. Writing `refs/memory/*`
/// into somebody's repository is a decision a person makes by opening the
/// project in Sync, not one an agent makes by connecting to it. Every other
/// refusal — `locked` above all — is passed on, because those are answers a
/// client must see rather than states to paper over.
fn readable(domain: &mut Domain) -> Result<bool> {
    if !matches!(domain.presence()?, MemoryPresence::Present { .. }) {
        return Ok(false);
    }
    domain.ensure_revision()?;
    Ok(true)
}

/// The answer for a repository Sync has never been opened on.
fn without_memory() -> Value {
    json!({
        "memory": "none",
        "next": "This repository has no Sync memory yet. Open it as a project in Sync, \
                 which is what creates one — an agent does not write memory into a \
                 repository nobody has chosen to keep memory in.",
    })
}

/// `sync_project`.
fn project(domain: &mut Domain) -> Result<Value> {
    if !readable(domain)? {
        return Ok(without_memory());
    }
    let settings = domain.project_settings()?;
    let types = domain.list_types()?;
    Ok(json!({
        "name": settings.as_ref().map(|s| s.name.clone()),
        "about": settings.as_ref().map(|s| s.description.clone()),
        "language": settings.as_ref().map(|s| s.language.clone()),
        "installed": settings.as_ref().map_or_else(Vec::new, |settings| {
            settings
                .installed
                .iter()
                .map(|extension| json!({"id": extension.id, "version": extension.version}))
                .collect::<Vec<Value>>()
        }),
        "kinds": types
            .iter()
            .map(|entry| json!({
                "kind": entry.kind,
                "title": entry.title,
                "about": entry.description,
            }))
            .collect::<Vec<Value>>(),
        "revision": domain.refresh_revision()?,
        "next": format!("Call `{INSTRUCTIONS}` for how this project expects to be worked with."),
    }))
}

/// `sync_instructions`.
fn instructions(domain: &mut Domain, contributed: &Registry, arguments: &Value) -> Result<Value> {
    // The built-in topics are about Sync and answer on any repository at all.
    // Only the per-extension ones need the project, so only they go missing
    // when there is no project to ask.
    let installed = if readable(domain)? {
        domain
            .project_settings()?
            .map_or_else(Vec::new, |settings| settings.installed)
    } else {
        Vec::new()
    };

    let Some(topic) = arguments.get("topic").and_then(Value::as_str) else {
        let mut topics: Vec<Value> = TOPICS
            .iter()
            .map(|(name, when, _)| json!({"topic": name, "when": when}))
            .collect();
        // One per extension the project declares. The bodies arrive with the
        // manifests, which is a later stage; until then the topic is listed and
        // says so, because a topic that is silently absent looks like an
        // extension that has nothing to say about itself.
        topics.extend(installed.iter().map(|extension| {
            json!({
                "topic": format!("extension:{}", extension.id),
                "when": format!("Before working in the kinds `{}` publishes.", extension.id),
            })
        }));
        // Deliberately not the bodies. A project may declare several
        // extensions, each with a document's worth of prose, and the list is
        // read to find out what there is to read.
        return Ok(json!({"topics": topics}));
    };

    if let Some((_, _, body)) = TOPICS.iter().find(|(name, _, _)| *name == topic) {
        return Ok(json!({"topic": topic, "body": body}));
    }
    if let Some(id) = topic.strip_prefix("extension:") {
        return if let Some(extension) = installed.iter().find(|extension| extension.id == id) {
            Ok(json!({"topic": topic, "body": extension_body(extension, contributed)}))
        } else {
            Err(MemoryError::domain(
                "invalid_argument",
                format!("this project has no extension named `{id}`."),
                json!({"extension": id}),
            ))
        };
    }
    Err(MemoryError::domain(
        "invalid_argument",
        format!("no topic named `{topic}`. Call `{INSTRUCTIONS}` with no arguments for the list."),
        json!({"topic": topic}),
    ))
}

/// `sync_apply`.
fn apply(domain: &mut Domain, arguments: &Value) -> Result<Value> {
    let save: Vec<sync_memory::EntityInput> = match arguments.get("save") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            MemoryError::domain(
                "invalid_argument",
                format!("unreadable record in `save`: {error}"),
                Value::Null,
            )
        })?,
    };
    let remove: Vec<String> = arguments
        .get("delete")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(|key| key.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if !readable(domain)? {
        return Err(MemoryError::domain(
            "not_initialised",
            "this repository has no Sync memory to write into. Open it as a project in \
             Sync, which is what creates one."
                .to_owned(),
            Value::Null,
        ));
    }
    let result = domain.apply_entities(save, &remove)?;
    serde_json::to_value(result).map_err(|error| {
        MemoryError::domain(
            "internal",
            format!("the answer could not be encoded: {error}"),
            Value::Null,
        )
    })
}

/// What one extension says about itself.
///
/// Its own prompt where it has one — the text it published into the project, in
/// its own words, because what its vocabulary is for is not something this
/// server could summarise — followed by the tools it contributes, which is the
/// one thing it cannot know about itself: the same extension contributes
/// nothing on a build that does not carry its runtime.
///
/// An extension with neither is not an error and does not pretend to be
/// interesting. It says what is true — that it published types and describes
/// itself through them — because a topic that came back empty would read as an
/// extension that failed rather than as one with nothing to add.
fn extension_body(extension: &InstalledExtension, contributed: &Registry) -> String {
    let mut body = extension.prompt.clone().unwrap_or_else(|| {
        format!(
            "`{}` publishes types and says nothing further about them. Read the kinds it \
             brought with `sync_project` and work in them the way `records` describes.",
            extension.id
        )
    });
    let tools = contributed.contributed_by(&extension.id);
    if !tools.is_empty() {
        let id = &extension.id;
        // Into the string rather than through a second one. Infallible for a
        // `String`, so the result is the trait's shape and not an outcome.
        let _ = write!(
            body,
            "\n\n## Its own tools\n\n`{id}` contributes: {}. Each is called under its full \
             name, `{id}.<tool>`.",
            tools.join(", ")
        );
    }
    body
}

/// The topics, their when-hints, and their bodies.
///
/// Prose rather than generated text, and short on purpose: every word here is
/// paid for out of an agent's context on the turn it reads them.
const TOPICS: &[(&str, &str, &str)] = &[
    (
        "records",
        "Before writing anything.",
        "A record is an envelope: a key, a kind, a title, Markdown content, tags, typed links, \
         the paths it was written against and the paths its claim covers. Product fields for the \
         kind live under `extensions`, and the engine validates them against the type definition \
         the project published — a record whose kind has no definition is refused, so publish the \
         type before the first record of it.\n\n\
         Keys are permanent. There is no rename: a key is what every link and every reader refers \
         to, so choose one that will still be true. Write through `sync_apply`, never by \
         constructing a transaction id of your own.",
    ),
    (
        "freshness",
        "Before trusting what a record says.",
        "Every record carries a freshness the engine derives rather than anybody states: it \
         reconciles code history against the record's scope paths. `valid` and `unverified` are \
         usable. `stale` and `invalid` mean the code moved under the claim — they are a flag, \
         never a fact. Verify against the code, then revalidate, edit, or archive.\n\n\
         Live code is the final arbiter. A record that disagrees with what the code does is a \
         record to fix, not evidence.",
    ),
    (
        "search",
        "Before concluding a project holds nothing on a topic.",
        "Search answers *something* for any input at all: full-text runs first, and a vector \
         channel picks up where the words thinned. In a small corpus one record is always the \
         nearest, and how near depends on how broadly it is written rather than on your \
         question — measured on this codebase, nonsense peaks around 0.46 and a correct \
         cross-language question around 0.49, so no similarity threshold separates the relevant \
         from the irrelevant.\n\n\
         Each hit says how it was found. Read `matched`: `words` and `both` are matches, \
         `meaning` alone is the nearest thing rather than an answer.",
    ),
    (
        "storage",
        "Before writing a record whose body is a file.",
        "A type either keeps its documents in the corpus or names a folder of the repository. \
         For the second, the record points at a file rather than carrying it, and the file is \
         the document — editable in any editor, reviewable in any diff. Read one with \
         `memory_read_content`; the answer says how to read itself, and a body that is not text \
         comes back base64.\n\n\
         Do not invent a folder for a file. Where a project keeps its documents is the \
         project's arrangement, written into the type by whoever attached the folder.",
    ),
];

/// One tool, described.
fn tool(name: &'static str, description: &'static str, schema: &Value) -> Tool {
    Tool::new(
        name,
        description,
        // Every schema here is an object literal written a few lines above, so
        // the shape is known rather than hoped for.
        std::sync::Arc::new(
            schema
                .as_object()
                .cloned()
                .unwrap_or_else(serde_json::Map::new),
        ),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn named(tools: &[Tool]) -> Vec<String> {
        tools.iter().map(|tool| tool.name.to_string()).collect()
    }

    /// A tool a model can see is a tool it will spend a turn calling. Switched
    /// off, it is not in the catalogue at all rather than in it and refusing.
    #[test]
    fn speaking_is_absent_from_the_catalogue_until_it_is_allowed() {
        assert!(!named(&tools(false)).iter().any(|name| name == SPEAK));
        assert!(named(&tools(true)).iter().any(|name| name == SPEAK));
    }

    /// Everything else is published either way: the switch is about speaking,
    /// not about the rest of the surface.
    #[test]
    fn the_switch_moves_nothing_but_the_one_tool() {
        let without = named(&tools(false));
        let with = named(&tools(true));
        assert_eq!(with.len(), without.len() + 1);
        for name in &without {
            assert!(
                with.contains(name),
                "`{name}` went missing with the switch on"
            );
        }
    }

    /// The refusal exists for the model that calls a name it remembers from a
    /// catalogue read before the switch moved, and it says where the switch is
    /// so that trying again is not the next thing it does.
    #[test]
    fn speaking_while_switched_off_is_refused_in_words_that_help() {
        let directory = tempfile::tempdir().expect("a directory");
        // No `voice.json` at all: the default, which is agents silent.
        let refused = speak(Some(directory.path()), &json!({"text": "hello"}))
            .expect_err("agents may not speak by default");
        let said = refused.to_string();
        assert!(
            said.contains("switched off"),
            "the refusal says what state it is in: {said}"
        );
        assert!(
            said.contains("Voice"),
            "the refusal names where the switch is: {said}"
        );
    }

    /// A server with no registry has no configuration directory, so it cannot
    /// know whether speaking was allowed — and says that rather than guessing
    /// either way.
    #[test]
    fn a_server_that_knows_no_settings_directory_says_so() {
        let refused = speak(None, &json!({"text": "hello"})).expect_err("nothing to read");
        assert!(
            refused.to_string().contains("settings"),
            "it says what it does not know: {refused}"
        );
    }
}
