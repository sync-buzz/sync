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
use sync_memory::{InstalledExtension, MemoryError, MemoryPresence, RecordType, Result};

use crate::domain::Domain;

/// Which projects this machine answers for.
pub const PROJECTS: &str = "sync_projects";
/// What this project is, what it is composed of, and what to read next.
pub const PROJECT: &str = "sync_project";
/// How Sync expects to be worked with, by topic.
pub const INSTRUCTIONS: &str = "sync_instructions";
/// The one write.
pub const APPLY: &str = "sync_apply";
/// The one door onto what the project's extensions offer.
pub const CALL: &str = "sync_call";
/// The one that makes a sound.
pub const SPEAK: &str = "sync_speak";

/// The topic describing the kinds a project made for itself.
///
/// Not in [`TOPICS`] because its body is the project rather than prose: what it
/// says differs per repository, and on a project whose kinds all came from
/// packages there is nothing for it to say and it is not offered.
const PROJECT_KINDS: &str = "kinds";

/// Whether a name is one of ours.
#[must_use]
pub fn is_ours(name: &str) -> bool {
    matches!(
        name,
        PROJECTS | PROJECT | INSTRUCTIONS | APPLY | CALL | SPEAK
    )
}

/// What separates an extension's id from the tool it offers.
///
/// A dot, and the reason is collision rather than taste: two extensions may
/// both offer a `search`, and neither shadows the other because each name
/// carries whose it is.
pub const SEPARATOR: char = '.';

/// The name one extension's tool is called by.
#[must_use]
pub fn full_name(extension: &str, tool: &str) -> String {
    format!("{extension}{SEPARATOR}{tool}")
}

/// The extension a full name belongs to, and the bare tool after it.
///
/// Split at the **last** dot, and what makes that unambiguous is the tool's
/// name rather than the id: `is_tool_name` refuses a dot when the manifest is
/// read, so whatever stands before the last one is the whole of the id. Doing
/// it from the other end would depend on the id instead, and the id in a
/// project's record was written by the window rather than checked here.
#[must_use]
pub fn extension_of(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once(SEPARATOR)
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
        writing(),
        calling(),
    ];

    if may_speak {
        tools.push(speaking());
    }

    tools
}

/// The one door onto extensions, described for a model.
///
/// **One tool rather than one per extension**, which is a decision about the
/// agent's context rather than about tidiness: every tool in a catalogue is
/// paid for in tokens by every agent on every turn, including the ones that
/// will never call it. A project with four extensions offering three tools each
/// would put twelve descriptions and twelve schemas in front of an agent that
/// asked about none of them.
///
/// So the catalogue carries one name, and what is behind it is read on demand:
/// `sync_project` names the tools each extension offers, and
/// `sync_instructions` with `extension:<id>` describes them and states their
/// schemas — to the one agent that asked.
///
/// The cost, named: a client cannot check the arguments before sending them,
/// because the schema is not in the catalogue entry. This server holds it
/// instead, checks against it, and its refusal says what exists and which topic
/// describes it — which is more than a client's own check could say.
fn calling() -> Tool {
    tool(
        CALL,
        "Call a tool one of this project's extensions offers. `sync_project` \
         names them; `sync_instructions` with the topic `extension:<id>` \
         describes each one and states the arguments it takes. Read that topic \
         before the first call to an extension — the arguments are checked \
         against the schema its author wrote, and this catalogue does not carry \
         it.\n\n\
         What a tool does is the extension's, not Sync's: it runs in the \
         application, under the permissions the person agreed to when they \
         installed the package, and it may reach the network or a stored \
         credential if they agreed to that. It answers with whatever the \
         extension answered.",
        &json!({
            "type": "object",
            "properties": {
                "tool": {
                    "type": "string",
                    "description": "The tool's full name, `<extension id>.<tool>` — as `sync_project` lists it."
                },
                "arguments": {
                    "type": "object",
                    "description": "What the tool takes, as the topic `extension:<id>` states it. Omit for a tool that takes nothing.",
                    "additionalProperties": true
                }
            },
            "required": ["tool"],
            "additionalProperties": false
        }),
    )
}

/// The one write, described for a model.
///
/// Its own function for the reason [`speaking`] is: the catalogue above is
/// already at the length a reader can hold, and this one carries the whole
/// shape of a record as well as what the answer says back.
fn writing() -> Tool {
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
         replaces that record, and there is no rename — so state a record \
         whole, because whatever a write leaves out is gone from it.\n\n\
         Two members are read the other way round, and both are how a record's \
         standing is set rather than its text. `archived` left out leaves the \
         record filed exactly as it was, so correcting a sentence cannot empty \
         somebody's archive; say `true` to put one away, `false` to bring it \
         back. `verified` left out means `unverified` — saying nothing is not \
         the same as saying you checked.\n\n\
         Name a record, never its key: a reference is \
         `[the record's title](sync://<kind>/<key>)`, and every hit hands you \
         both. Double brackets are refused and a bare key in a code span comes \
         back with the link to write instead — the topic `records` says why.",
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
                            "content": {
                                "type": "string",
                                "description": "Markdown. A reference to another record is its name carrying its address — `[the record's title](sync://<kind>/<key>)`; a file of the repository is linked as GitHub links one — `[Setup](./setup.md)`. Double brackets are refused."
                            },
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
                            },
                            "folder": {
                                "type": "string",
                                "description": "Where the record is filed, as `memory_list_folders` names it. Omit for the root. A record whose body is a repository file is filed where its file is, and that one is moved by moving the file."
                            },
                            "is_folder": {
                                "type": "boolean",
                                "description": "This record *is* the folder it is filed in: what belongs there, written where whoever files something will read it."
                            },
                            "archived": {
                                "type": "boolean",
                                "description": "Put the record away, or bring it back. Left out, it stays as it is — a write that says nothing about the archive must not empty it."
                            },
                            "verified": {
                                "type": "boolean",
                                "description": "You read the code under `scope_paths` and the claim still holds. The one way a record becomes `fresh`; left out, the write leaves it `unverified`. Refused where there are no scope paths, because nothing would ever flag it again."
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
    )
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
                    "description": "What to say, as it should sound. Written for an ear: no Markdown, no code, no file paths, and no record keys — say what a thing is called."
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
pub fn call(domain: &mut Domain, name: &str, arguments: &Value) -> ToolCall {
    let answer = match name {
        PROJECT => project(domain),
        INSTRUCTIONS => instructions(domain, arguments),
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
/// What a `sync_call` turned out to be about, once the project's record was
/// read and the arguments were checked.
///
/// Three owned strings and a value rather than borrows of the call, because
/// what happens next happens **after** the project's memory has been given
/// back: the tool runs in the application and may take twenty seconds doing it,
/// and holding a project's engine open for the length of somebody's API call
/// would stop that project for everybody else.
pub struct Intent {
    pub extension: String,
    pub tool: String,
    pub arguments: Value,
}

/// Read a `sync_call` against the project's own record, or refuse it in words.
///
/// **Everything checkable is checked here, before anything leaves this
/// process**: that the name is well formed, that the project declares that
/// extension, that the extension offers that tool, and that the arguments match
/// the schema its author wrote. What is left for the application is the part
/// only it can answer — whether the package is on this machine and what its
/// handler does.
///
/// The refusals name what exists and where to read about it, because that is
/// the whole compensation for a catalogue that carries one name instead of
/// twelve: a client cannot check a schema it was not given, so this one has to
/// answer better than a client's own check would have.
///
/// # Errors
///
/// `invalid_argument` for every one of them. They are all the same class of
/// mistake — a call written from a guess instead of from the topic — and
/// splitting them by kind would invite a client to branch on which of its
/// guesses was wrong.
pub fn intended(domain: &mut Domain, project: &str, arguments: &Value) -> Result<Intent> {
    let Some(name) = arguments.get("tool").and_then(Value::as_str) else {
        return Err(refused(
            format!(
                "`{CALL}` needs `tool`: the full name of the tool to call, `<extension id>.<tool>`, as `{PROJECT}` lists it."
            ),
            json!({}),
        ));
    };
    let Some((extension, tool)) = extension_of(name) else {
        return Err(refused(
            format!(
                "`{name}` is not a tool's full name. An extension's tool is called `<extension id>.<tool>` — `{PROJECT}` lists what this project offers."
            ),
            json!({"tool": name}),
        ));
    };

    let installed = if readable(domain)? {
        domain
            .project_settings()?
            .map_or_else(Vec::new, |settings| settings.installed)
    } else {
        Vec::new()
    };
    let Some(declared) = installed.iter().find(|entry| entry.id == extension) else {
        return Err(refused(
            format!(
                "`{project}` has no extension named `{extension}`, so nothing here answers to `{name}`. `{PROJECT}` lists what this project is composed of."
            ),
            json!({"project": project, "extension": extension}),
        ));
    };
    let Some(offered) = declared.tools.iter().find(|declared| declared.name == tool) else {
        let offers = if declared.tools.is_empty() {
            "it offers none at all".to_owned()
        } else {
            format!(
                "it offers {}",
                declared
                    .tools
                    .iter()
                    .map(|declared| full_name(extension, &declared.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return Err(refused(
            format!(
                "`{extension}` has no tool called `{tool}`: {offers}. Read `{INSTRUCTIONS}` with the topic `extension:{extension}` for what each one takes."
            ),
            json!({"project": project, "extension": extension, "tool": tool}),
        ));
    };

    let given = arguments
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    checked(&offered.input, &given, extension, tool)?;

    Ok(Intent {
        extension: extension.to_owned(),
        tool: tool.to_owned(),
        arguments: given,
    })
}

/// Hold the arguments to the schema the package published, and say what failed.
///
/// **The package's schema, unread by us.** It is carried whole from the
/// manifest to the project's record to here, so what an argument means stays
/// the author's business — this only answers whether what arrived is what they
/// said they take.
///
/// A schema that is not itself valid is the author's mistake and is named as
/// one, rather than being treated as "takes anything": a package whose schema
/// never compiled would otherwise be a package whose arguments were never
/// checked, silently, for as long as it is installed.
fn checked(schema: &Value, arguments: &Value, extension: &str, tool: &str) -> Result<()> {
    if schema.is_null() {
        return Ok(());
    }
    let validator = jsonschema::validator_for(schema).map_err(|error| {
        refused(
            format!(
                "`{extension}` declares `{tool}` with a schema that cannot be read, so nothing can be checked against it: {error}. This is the package's to fix."
            ),
            json!({"extension": extension, "tool": tool}),
        )
    })?;
    let wrong: Vec<String> = validator
        .iter_errors(arguments)
        .map(|error| format!("{} at `{}`", error, error.instance_path()))
        .collect();
    if wrong.is_empty() {
        return Ok(());
    }
    Err(refused(
        format!(
            "these arguments are not what `{}` takes: {}. `{INSTRUCTIONS}` with the topic `extension:{extension}` states the whole schema.",
            full_name(extension, tool),
            wrong.join("; ")
        ),
        json!({"extension": extension, "tool": tool, "wrong": wrong}),
    ))
}

fn refused(message: String, data: Value) -> MemoryError {
    MemoryError::domain("invalid_argument", message, data)
}

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
                .map(|extension| {
                    let mut said = json!({
                        "id": extension.id,
                        "version": extension.version,
                        "instructions": format!("extension:{}", extension.id),
                    });
                    // Which of the kinds above this package brought. Without
                    // it, a project is a list of kinds and a list of packages
                    // with nothing joining them, and the agent that wants to
                    // write one of those kinds has no way to know whose
                    // instructions describe it — the join is a prefix, and
                    // saying it costs a word where guessing it costs a turn.
                    let brought: Vec<Value> = types
                        .iter()
                        .filter(|entry| brought_by(entry, &extension.id))
                        .map(|entry| Value::String(entry.kind.clone()))
                        .collect();
                    if !brought.is_empty() {
                        said["kinds"] = Value::Array(brought);
                    }
                    // The names and not the descriptions, and not the schemas.
                    // This is the first thing an agent reads about a project
                    // and every word of it is paid for out of its context on
                    // that turn, so what belongs here is *that there is
                    // something to call* — the rest is a topic away, read by
                    // the agent that decided it cares.
                    if !extension.tools.is_empty() {
                        said["tools"] = extension
                            .tools
                            .iter()
                            .map(|tool| Value::String(tool.name.clone()))
                            .collect();
                    }
                    said
                })
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
        // Two sentences rather than one, because they answer different
        // questions and an agent that read only the first would take the
        // project's own habits for the whole of what it was told. What a
        // package expects of a record of its kinds is the package's to say,
        // and it says it in a topic nobody reads by accident.
        "next": format!(
            "Call `{INSTRUCTIONS}` for how this project expects to be worked with. Before \
             writing a record of a kind a package brought, or calling one of its tools, read \
             that package's own topic — the `instructions` beside it above."
        ),
    }))
}

/// `sync_instructions`.
fn instructions(domain: &mut Domain, arguments: &Value) -> Result<Value> {
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
    let published = if readable(domain)? {
        domain.list_types()?
    } else {
        Vec::new()
    };

    let Some(topic) = arguments.get("topic").and_then(Value::as_str) else {
        return Ok(json!({"topics": listed(&installed, &published)}));
    };

    if let Some((_, _, body)) = TOPICS.iter().find(|(name, _, _)| *name == topic) {
        return Ok(json!({"topic": topic, "body": body}));
    }
    if let Some(id) = topic.strip_prefix("extension:") {
        return if let Some(extension) = installed.iter().find(|extension| extension.id == id) {
            let brought: Vec<&RecordType> = published
                .iter()
                .filter(|entry| brought_by(entry, id))
                .collect();
            Ok(json!({"topic": topic, "body": extension_body(extension, &brought)}))
        } else {
            Err(MemoryError::domain(
                "invalid_argument",
                format!("this project has no extension named `{id}`."),
                json!({"extension": id}),
            ))
        };
    }
    // The kinds nobody's package brought: types somebody made in the window,
    // for this project and no other. They have fields and guidance exactly as a
    // package's do, and no topic of their own to be described in — so they are
    // described here, in the one place a writer is already looking.
    if topic == PROJECT_KINDS {
        let own: Vec<&RecordType> = published
            .iter()
            .filter(|entry| !from_a_package(entry, &installed))
            .collect();
        return Ok(json!({
            "topic": topic,
            "body": format!(
                "Kinds this project made for itself, rather than taking from a package.{}",
                kinds_section(&own)
            ),
        }));
    }
    Err(MemoryError::domain(
        "invalid_argument",
        format!("no topic named `{topic}`. Call `{INSTRUCTIONS}` with no arguments for the list."),
        json!({"topic": topic}),
    ))
}

/// Whether a kind came from the package with this id.
///
/// The join is the prefix and nothing else: a package's kinds are its id, a
/// dot, and a word it chose. Written once because three places ask it and a
/// fourth spelling of `starts_with` is how one of them ends up subtly
/// different — `tasks` matching `tasksmith.thing`, for instance.
fn brought_by(kind: &RecordType, id: &str) -> bool {
    kind.kind
        .strip_prefix(id)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Whether any installed package brought this kind.
fn from_a_package(kind: &RecordType, installed: &[InstalledExtension]) -> bool {
    installed
        .iter()
        .any(|extension| brought_by(kind, &extension.id))
}

/// What there is to read, and when each one is worth reading.
///
/// Deliberately not the bodies. A project may declare several packages, each
/// with a document's worth of prose, and this list is read to find out what
/// there is to read.
///
/// Each hint is written in the words of what the topic covers rather than in
/// the name of what published it. An id is not something anybody asks for by:
/// somebody asks for a Task, and the topic describing one has to be findable
/// from that word. The titles are the project's own, read off its types, so
/// this stays a list of what is here rather than of what Sync knows about.
fn listed(installed: &[InstalledExtension], published: &[RecordType]) -> Vec<Value> {
    let mut topics: Vec<Value> = TOPICS
        .iter()
        .map(|(name, when, _)| json!({"topic": name, "when": when}))
        .collect();
    // Offered only where there is something to describe. A project whose every
    // kind came from a package would otherwise be handed a topic that answers
    // with a heading and nothing under it.
    if published
        .iter()
        .any(|entry| !from_a_package(entry, installed))
    {
        topics.push(json!({
            "topic": PROJECT_KINDS,
            "when": "Before writing a record of a kind no package brought.",
        }));
    }
    topics.extend(installed.iter().map(|extension| {
        let brought: Vec<&str> = published
            .iter()
            .filter(|entry| brought_by(entry, &extension.id))
            .map(|entry| entry.title.as_str())
            .collect();
        let when = match (brought.is_empty(), extension.tools.is_empty()) {
            (true, true) => format!("Before working in the kinds `{}` publishes.", extension.id),
            (true, false) => format!("Before calling a tool of `{}`.", extension.id),
            (false, true) => format!("Before writing one of: {}.", brought.join(", ")),
            (false, false) => format!(
                "Before writing one of: {}, or calling a tool of `{}`.",
                brought.join(", "),
                extension.id
            ),
        };
        json!({"topic": format!("extension:{}", extension.id), "when": when})
    }));
    topics
}

/// The records a write names by their key rather than by their name.
///
/// Read off what is being written. Every part of that is deliberate:
///
/// - **Reported, not refused.** A key in a code span is ambiguous — `d-one` is
///   a key in one body and a command in the next — and refusing on a guess
///   would throw away somebody's transaction to make a point about style. What
///   comes back is the key, where it was written and the exact string to write
///   instead: a loop the writer can close on its next turn, which a line in a
///   prompt is not. The unambiguous spelling is refused instead, in
///   [`refuse_wikilinks`], and the difference between the two is the whole
///   subject of `link.rs`'s opening.
/// - **Only through this door.** The window writes through its own commands and
///   a person typing in the editor is not being marked. This is the agent's
///   door, and the check is on what an agent writes.
///
/// A key of a record being written in this same transaction is named from the
/// transaction rather than from the store, so a record naming its sibling is
/// answered rather than missed. A read that refuses leaves that key unreported:
/// this is advice about a write that already landed, and a store that cannot be
/// read has something worse to say than which links to tidy.
fn bare_keys(domain: &mut Domain, written: &[sync_memory::EntityInput]) -> Vec<Value> {
    let mut reports = Vec::new();
    for entity in written {
        let found = crate::link::bare(&entity.content, &entity.key, |key| {
            resolved(domain, written, key)
        });
        reports.extend(found.into_iter().map(|bare| {
            json!({
                "key": bare.key,
                "written_in": bare.written_in,
                "write_instead": bare.instead,
            })
        }));
    }
    reports
}

/// The one spelling a write is refused for: a record in double brackets.
///
/// Refused rather than reported, and the reason is that nothing has to be
/// guessed. `[[d-one]]` means nothing in Markdown, nothing to the window and
/// nothing to any other reader of this corpus, so there is no reading of it
/// that was worth writing — which is what makes it safe to answer with a
/// refusal instead of advice. The transaction is untouched: nothing was
/// written, the whole of what is wrong is in the message, and writing again
/// with the names in it costs the writer one turn.
///
/// Every text a record carries is read, not only its body. A wikilink in a
/// title or in a product field is the same dead end, and the check that looked
/// only at `content` would let it through in the two places a reader meets
/// first.
///
/// The link to write instead is offered where the store answers to the key.
/// Where nothing does, the write is still refused — the spelling is what is
/// wrong, and a link to nowhere is not what was missing.
fn refuse_wikilinks(domain: &mut Domain, written: &[sync_memory::EntityInput]) -> Result<()> {
    let mut found = Vec::new();
    let mut said = Vec::new();

    for entity in written {
        for (place, text) in texts(entity) {
            for name in crate::link::wikilinks(text) {
                let instead = resolved(domain, written, name)
                    .map(|record| crate::link::markdown(&record.kind, name, &record.title));
                said.push(match &instead {
                    Some(link) => format!("`{}` {place}: [[{name}]] → {link}", entity.key),
                    None => format!(
                        "`{}` {place}: [[{name}]] → nothing answers to that key",
                        entity.key
                    ),
                });
                found.push(json!({
                    "written_in": entity.key,
                    "where": place,
                    "wrote": name,
                    "write_instead": instead,
                }));
            }
        }
    }

    if found.is_empty() {
        return Ok(());
    }
    Err(MemoryError::domain(
        "invalid_argument",
        format!(
            "double brackets are not a link: they carry no kind, so nothing can follow one, \
             and the window draws it as text rather than as a way through. Name the record \
             instead — `[the record's title](sync://<kind>/<key>)` — and send the write again. \
             Nothing was written. {}",
            said.join("; ")
        ),
        json!({"wikilinks": found}),
    ))
}

/// The one claim a record cannot make: checked, against nothing.
///
/// `verified` says the writer read the code the record covers and found the
/// claim standing. Where the record names no scope paths there is no such code:
/// nothing was read, and — worse — nothing will ever take the flag off again,
/// because the engine marks a record stale by reconciling code history against
/// exactly those paths. A record left permanently marked as checked is the one
/// failure this whole scale exists to prevent, so the write is refused and the
/// message says what to add.
///
/// Refused rather than quietly downgraded to `unverified`: a writer who claimed
/// to have checked something has read code, and silently storing that as *not
/// checked* throws the reading away without telling anybody it happened.
fn refuse_unscoped_verification(written: &[sync_memory::EntityInput]) -> Result<()> {
    let unscoped: Vec<&str> = written
        .iter()
        .filter(|entity| entity.verified && entity.scope_paths.is_empty())
        .map(|entity| entity.key.as_str())
        .collect();
    if unscoped.is_empty() {
        return Ok(());
    }
    Err(MemoryError::domain(
        "invalid_argument",
        format!(
            "`verified` says you read the code this record covers, and it names none: {}. \
             Freshness is derived by reconciling code history against `scope_paths`, so a \
             record with none would stay marked as checked for ever. Add the paths the claim \
             covers and send the write again, or leave `verified` out. Nothing was written.",
            unscoped.join(", ")
        ),
        json!({"verified_without_scope": unscoped}),
    ))
}

/// Every text of a record a reader will meet, with where it was written.
///
/// The body, the name, and whatever the kind's own fields hold — a question's
/// answer and its options are prose as much as a body is. Strings are reached
/// through arrays and objects alike, because a field's shape is the type's to
/// decide and this has no business knowing it.
fn texts(entity: &sync_memory::EntityInput) -> Vec<(String, &str)> {
    let mut texts = vec![
        ("content".to_owned(), entity.content.as_str()),
        ("title".to_owned(), entity.title.as_str()),
    ];
    for (name, value) in &entity.fields {
        prose_in(value, &format!("fields.{name}"), &mut texts);
    }
    texts
}

/// The strings inside one field value, however deeply the type nests them.
fn prose_in<'a>(value: &'a Value, place: &str, texts: &mut Vec<(String, &'a str)>) {
    match value {
        Value::String(text) => texts.push((place.to_owned(), text.as_str())),
        Value::Array(values) => {
            for value in values {
                prose_in(value, place, texts);
            }
        }
        Value::Object(members) => {
            for (name, value) in members {
                prose_in(value, &format!("{place}.{name}"), texts);
            }
        }
        _ => {}
    }
}

/// What one key names, from this transaction first and the store after it.
///
/// The transaction comes first for the reason it does in [`bare_keys`]: a
/// record naming its sibling is naming something that is not in the store yet
/// and will be a moment from now.
fn resolved(
    domain: &mut Domain,
    written: &[sync_memory::EntityInput],
    key: &str,
) -> Option<crate::link::Record> {
    written
        .iter()
        .find(|sibling| sibling.key == key)
        .map(|sibling| crate::link::Record {
            kind: sibling.kind.clone(),
            title: sibling.title.clone(),
        })
        .or_else(|| named(domain, key))
}

/// What one key names, as far as the store will say.
fn named(domain: &mut Domain, key: &str) -> Option<crate::link::Record> {
    let stored = domain.get_record(key).ok()?.record?;
    // The engine answers a record either whole or wrapped in its envelope, and
    // both spellings carry the same two members.
    let envelope = stored.get("envelope").unwrap_or(&stored);
    Some(crate::link::Record {
        kind: envelope.get("kind")?.as_str()?.to_owned(),
        title: envelope.get("title")?.as_str()?.to_owned(),
    })
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
    // Both readings happen before the write, because the write consumes what it
    // is given. The refusal answers first: there is no point reporting prose in
    // a transaction that is not going to land.
    refuse_wikilinks(domain, &save)?;
    refuse_unscoped_verification(&save)?;
    // Attached only after a write that succeeded, because a transaction that
    // failed is not the moment to discuss somebody's style.
    let bare = bare_keys(domain, &save);
    let result = domain.apply_entities(save, &remove)?;
    let mut answer = serde_json::to_value(result).map_err(|error| {
        MemoryError::domain(
            "internal",
            format!("the answer could not be encoded: {error}"),
            Value::Null,
        )
    })?;
    if let (Some(answer), false) = (answer.as_object_mut(), bare.is_empty()) {
        answer.insert("bare_keys".to_owned(), Value::Array(bare));
    }
    Ok(answer)
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
fn extension_body(extension: &InstalledExtension, brought: &[&RecordType]) -> String {
    let mut body = extension.prompt.clone().unwrap_or_else(|| {
        format!(
            "`{}` says nothing about itself beyond what its types declare, which is below.",
            extension.id
        )
    });
    body.push_str(&kinds_section(brought));
    if !extension.tools.is_empty() {
        let id = &extension.id;
        // Read off the project's own record rather than off anything this
        // process was handed at startup. The declarations travel with the
        // repository, so a colleague who cloned it is told the same thing —
        // and this server has no view of the catalogue the packages are in.
        //
        // Into the string rather than through a second one. Infallible for a
        // `String`, so the result is the trait's shape and not an outcome.
        let _ = write!(
            body,
            "\n\n## Its own tools\n\n`{id}` offers {}, each called under its full name, \
             `{id}.<tool>`.",
            if extension.tools.len() == 1 {
                "one tool".to_string()
            } else {
                format!("{} tools", extension.tools.len())
            }
        );
        for tool in &extension.tools {
            // The schema whole, as the package wrote it. An agent calling
            // without it is guessing, and a summary of a schema is a schema
            // that disagrees with the one the arguments are checked against.
            let takes = if tool.input.is_null() {
                "nothing".to_string()
            } else {
                serde_json::to_string(&tool.input).unwrap_or_else(|_| "nothing".to_string())
            };
            let _ = write!(
                body,
                "\n\n### `{id}.{}`\n\n{}\n\nTakes: `{takes}`",
                tool.name, tool.description
            );
        }
    }
    body
}

/// The kinds themselves, as their definitions state them.
///
/// A definition holds everything somebody needs to write a record of its type —
/// the fields, what each one takes, which are required, the relations it may
/// carry — and one more thing: the `guidance` its author wrote for exactly this
/// moment. None of it is on the catalogue: `memory_list_types` answers with how
/// *many* fields a type has, which tells a writer nothing it can act on, and a
/// writer who cannot see a field name finds it out from a refusal.
///
/// So it is here, in the topic that is read before writing one, and stated
/// verbatim. A summary of a schema is a schema that disagrees with the one the
/// engine validates against.
fn kinds_section(kinds: &[&RecordType]) -> String {
    if kinds.is_empty() {
        return String::new();
    }
    let mut section = String::from("\n\n## The kinds, as their definitions state them");
    for kind in kinds {
        let _ = write!(
            section,
            "\n\n### `{}` — {}\n\n{}",
            kind.kind, kind.title, kind.description
        );
        if let Some(guidance) = &kind.guidance {
            let _ = write!(section, "\n\n{guidance}");
        }
        if kind.fields.is_empty() {
            section.push_str("\n\nFields: none. Everything it says is its title and its body.");
        } else {
            let _ = write!(
                section,
                "\n\nFields: `{}`",
                Value::Object(kind.fields.clone())
            );
        }
        if !kind.relationships.is_empty() {
            let _ = write!(
                section,
                "\nRelations: `{}`",
                Value::Object(kind.relationships.clone())
            );
        }
    }
    section
}

/// Every name in a text that reads as a tool of this surface, and is not one.
///
/// Written for the tests below and for the one in `server.rs`, because both are
/// holding the same rule against two halves of the same prose. What it looks
/// for is deliberately narrow: a word in backticks beginning `memory_` or
/// `sync_` is this surface's own shape, so a text that spells one nothing
/// answers to is a text telling an agent to call something that does not exist.
///
/// Names carrying a dot are not checked, and the reason is that they are
/// ambiguous by design — `fields.archived` and `acme.tracker.search` are the
/// same shape, one of them a member and the other a call, and a check that
/// guessed would fail on the prose it was meant to protect. That class of
/// mistake is caught where the names are: in a package's own repository,
/// against the surface it was written for.
#[cfg(test)]
pub(crate) fn names_that_are_not_tools(text: &str) -> Vec<String> {
    let catalogue: Vec<&str> = crate::published::PUBLISHED
        .iter()
        .copied()
        .chain([PROJECTS, PROJECT, INSTRUCTIONS, APPLY, CALL, SPEAK])
        .collect();
    let mut unknown = Vec::new();
    // Odd runs of a split on the fence are what stood between two of them.
    for spelled in text.split('`').skip(1).step_by(2) {
        let named = spelled.trim();
        if !(named.starts_with("memory_") || named.starts_with("sync_")) {
            continue;
        }
        if catalogue.contains(&named) || unknown.iter().any(|seen| seen == named) {
            continue;
        }
        unknown.push(named.to_owned());
    }
    unknown
}

/// Everything this server says to an agent in its own words.
///
/// The tool descriptions, their schemas, and every topic — the whole of what an
/// agent is told by Sync rather than by a package or by the engine.
#[cfg(test)]
pub(crate) fn everything_said() -> Vec<String> {
    let mut said: Vec<String> = TOPICS
        .iter()
        .flat_map(|(name, when, body)| [(*name).to_owned(), (*when).to_owned(), (*body).to_owned()])
        .collect();
    for tool in tools(true) {
        said.push(tool.description.unwrap_or_default().to_string());
        said.push(Value::Object((*tool.input_schema).clone()).to_string());
    }
    said
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
         constructing a transaction id of your own.\n\n\
         Two members say where a record sits rather than what it holds. `folder` files it — \
         `memory_list_folders` names the folders a project already keeps, and where a folder \
         has a record describing it, that description is what belongs there, read before \
         filing rather than after. `archived` puts it away, and a closed thing is archived in \
         the same write that closes it. A write replaces everything else it does not repeat, \
         and leaves the archive exactly as it found it.\n\n\
         **Name another record, never its key.** In prose, a reference is a readable name \
         carrying an address: `[the record's title](sync://<kind>/<key>)`. A bare key is \
         unreadable and cannot be opened, and double brackets are not a link — they carry no \
         kind, so nothing can route on one. You already hold what a link needs: every hit and \
         every listing carries the title and the kind beside the key. A record whose body is a \
         file of the repository is linked the way GitHub links one instead — `[Setup](./setup.md)` \
         — so that the link works where the file is read as well as here.\n\n\
         The write door holds this rather than trusting it: `sync_apply` **refuses** a write \
         spelling a record in double brackets, in a title or a field as much as in a body, and \
         says what to write instead. A key left bare in a code span is ambiguous — it may be a \
         command — so that one is reported back with its link rather than refused.\n\n\
         That is the prose link, and it is not the typed one. `links` carries \
         `{key, relation}`, which is what the engine validates against the type and what the \
         window draws as a relation. Write the typed link where the type declares the relation, \
         and the prose link wherever a reader would otherwise have to go looking.",
    ),
    (
        "freshness",
        "Before relying on what a record says, and after checking one against the code.",
        "Freshness is the engine reconciling code history against a record's `scope_paths`. \
         Four states, and only one of them is anybody's to state:\n\n\
         - `fresh` — somebody read that code and wrote the record back with `verified: true`. \
         It holds until a commit touches those paths again.\n\
         - `unverified` — nobody has said either way. Every record starts here and every edit \
         returns it here: a text that changed is a claim nobody has checked since.\n\
         - `stale`, `invalid` — the code moved under the claim. **A flag, never a fact.** Do \
         not quote the body as true.\n\n\
         A flagged record is settled one of three ways, said in one line before you act on it: \
         the claim still holds, so write it back with `verified: true`; it needs editing, so \
         edit it in that same write; it is obsolete, so set `archived: true`. Leaving it \
         flagged is the one option that costs the next session the same reading again.\n\n\
         A record with no scope paths is never flagged, because there is nothing to reconcile \
         it against — and for the same reason it cannot be verified: the write door refuses \
         the claim rather than storing one that would stand for ever. A claim about code \
         belongs to the paths it covers; give it them.\n\n\
         Live code is the final arbiter. A record that disagrees with what the code does is a \
         record to fix, and fixing it is part of shipping the change.",
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
         `meaning` alone is the nearest thing rather than an answer.\n\n\
         A hit carries the title and the kind beside the key. Those are what a link is made of, \
         and quoting the key on its own throws away the only part of a hit a reader can use.",
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

    /// A name an agent reads in backticks is a name it will call.
    ///
    /// Both halves of this were live at once: a topic here named five calls
    /// that have never existed on this surface, and the agent following it
    /// spent a turn each finding that out. Prose is the one part of a server
    /// that the compiler cannot hold, so this holds the part of it that is
    /// checkable — a tool named must be a tool published.
    #[test]
    fn every_tool_this_server_names_is_one_it_publishes() {
        for said in everything_said() {
            let unknown = names_that_are_not_tools(&said);
            assert!(
                unknown.is_empty(),
                "these are named as tools and are not published: {unknown:?} — in: {said}"
            );
        }
    }

    /// One word for one state, and it is the engine's word.
    ///
    /// Freshness was written here as `valid` and everywhere else — the engine,
    /// the window, the filters an agent passes — as `fresh`. Nothing failed:
    /// the agent simply filtered on a state no record has ever been in, and got
    /// an empty answer that looked like an empty corpus.
    #[test]
    fn freshness_is_spelled_the_way_the_engine_spells_it() {
        const STATES: &[&str] = &["fresh", "unverified", "stale", "invalid"];
        for said in everything_said() {
            for spelled in said.split('`').skip(1).step_by(2) {
                let named = spelled.trim();
                assert_ne!(
                    named, "valid",
                    "the engine has no such state; the four are {STATES:?} — in: {said}"
                );
            }
        }
    }

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
