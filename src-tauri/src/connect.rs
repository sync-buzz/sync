//! Connecting an agent to this project's Sync.
//!
//! An agent reaches Sync by having one server written into its own
//! configuration file. Which file, and what shape the entry takes, is different
//! for every client — and all of it is decided here, in Rust, because the
//! alternative is a filesystem capability handed to the webview.
//!
//! That is the whole reason this module exists rather than a few lines of
//! TypeScript. A window that could write any file on request is a window whose
//! safety is a property of its frontend code; a window that can ask for *this
//! server, in that file, for this project* has a policy instead. What arrives
//! from the interface is an agent's id and a project path, and nothing else is
//! reachable from there.
//!
//! Writing is done by [`document`], which splices rather than reformats: the
//! rest of somebody's configuration is not ours to tidy.

mod document;

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Runtime};

use crate::project::ProjectError;

/// Where a client keeps the servers it connects to.
///
/// Every one of them is the person's own, and none is inside a repository. Sync
/// serves every project from one address, so a per-project entry would be the
/// same bytes copied into each checkout — and a file in the repository is a
/// commit that announces to a team that somebody is trying this.
#[derive(Clone, Copy)]
enum Location {
    /// In the person's home directory, once for this machine.
    Home(&'static str),
    /// Where the platform keeps application data — `~/Library/Application
    /// Support` on macOS, the usual config directory elsewhere.
    Data(&'static str),
}

/// The shape of the entry a client expects.
///
/// Four, because four is how many ways these clients spell the same three
/// facts — an address, a header, and that this is HTTP. None of them is
/// wrong; they were written by different people at different times, and a
/// connection is a line in *their* file.
#[derive(Clone, Copy)]
enum Wire {
    /// `{"<holder>": {"<name>": {"type": "http", "url": …, "headers": {…}}}}`.
    ///
    /// `typed` for the clients that want `"type"` alongside — VS Code asks for
    /// it, Claude Code accepts it, and the others neither need nor mind it.
    Json { holder: &'static str, typed: bool },
    /// `[<table>.<name>]` with `url` and an `http_headers` table — Codex CLI.
    CodexToml { table: &'static str },
    /// `[<table>.<name>]` with `url`, `enabled`, and a nested `headers` table —
    /// Grok CLI.
    GrokToml { table: &'static str },
    /// A command to run, for the one client that speaks no HTTP.
    ///
    /// It still reaches every project: the process is started with the same
    /// registry the HTTP server serves, so the only thing it does differently
    /// is talk over a pipe.
    Stdio { holder: &'static str },
}

/// Which heading a client is listed under.
///
/// Three, because where a client keeps its file is not the only thing that
/// differs about them. A person who came here to connect Zed was reading past
/// four terminals to find it, and the flat column that made them do it was
/// telling them these seven things are alike in a way they are not.
///
/// Nothing but the interface reads this. Connecting is the same work under
/// every heading, and a group that changed what got written would be a second
/// place deciding what a connection is.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// Started from a terminal.
    CommandLine,
    /// An application with a window of its own.
    Desktop,
    /// An editor the work is written in.
    Editor,
}

/// One client Sync knows how to connect to.
struct Client {
    id: &'static str,
    name: &'static str,
    group: Group,
    location: Location,
    wire: Wire,
}

/// The clients, and the whole of what Sync will write.
///
/// Deliberately short: these are the clients this product's users have, not a
/// catalogue of everything that speaks the protocol. Adding one is a row here —
/// and a row here is the only way to add one, which is the point.
const CLIENTS: &[Client] = &[
    Client {
        id: "claude-code",
        name: "Claude Code",
        group: Group::CommandLine,
        location: Location::Home(".claude.json"),
        wire: Wire::Json {
            holder: "mcpServers",
            typed: true,
        },
    },
    Client {
        id: "codex-cli",
        name: "Codex CLI",
        group: Group::CommandLine,
        location: Location::Home(".codex/config.toml"),
        wire: Wire::CodexToml {
            table: "mcp_servers",
        },
    },
    Client {
        id: "grok-cli",
        name: "Grok CLI",
        group: Group::CommandLine,
        location: Location::Home(".grok/config.toml"),
        wire: Wire::GrokToml {
            table: "mcp_servers",
        },
    },
    Client {
        id: "claude-desktop",
        name: "Claude Desktop",
        group: Group::Desktop,
        location: Location::Data("Claude/claude_desktop_config.json"),
        // The one that speaks no HTTP. Its entry runs the server over a pipe,
        // against the same registry — every project, one process, just not a
        // shared one.
        wire: Wire::Stdio {
            holder: "mcpServers",
        },
    },
    Client {
        id: "cursor",
        name: "Cursor",
        group: Group::Editor,
        location: Location::Home(".cursor/mcp.json"),
        wire: Wire::Json {
            holder: "mcpServers",
            typed: false,
        },
    },
    Client {
        id: "vscode",
        name: "Visual Studio Code",
        group: Group::Editor,
        location: Location::Data("Code/User/mcp.json"),
        wire: Wire::Json {
            holder: "servers",
            typed: true,
        },
    },
    Client {
        id: "zed",
        name: "Zed",
        group: Group::Editor,
        // Not `Data`: Zed keeps its settings in `~/.config` on macOS too,
        // rather than in Application Support with the others.
        location: Location::Home(".config/zed/settings.json"),
        // `context_servers` is what Zed calls the holder, and a remote one is
        // a `url` and a `headers` table with no `type` beside them.
        wire: Wire::Json {
            holder: "context_servers",
            typed: false,
        },
    },
];

impl Client {
    /// Every client is connected once for this machine now, so the word is the
    /// same for all of them — kept because the interface says it out loud, and
    /// a row that stopped saying where its file lives would be a row that looks
    /// like it writes into the project.
    fn scope(&self) -> &'static str {
        "installation"
    }

    /// What the file is called where a person would recognise it.
    fn shown(&self) -> String {
        match self.location {
            Location::Home(path) => format!("~/{path}"),
            Location::Data(path) => path.to_owned(),
        }
    }

    fn file<R: Runtime>(&self, app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
        match self.location {
            Location::Home(path) => {
                app.path()
                    .home_dir()
                    .map(|home| home.join(path))
                    .map_err(|error| {
                        ProjectError::new(
                            "configuration_failed",
                            format!("could not find your home directory: {error}"),
                        )
                    })
            }
            Location::Data(path) => {
                app.path()
                    .data_dir()
                    .map(|data| data.join(path))
                    .map_err(|error| {
                        ProjectError::new(
                            "configuration_failed",
                            format!("could not find the application data directory: {error}"),
                        )
                    })
            }
        }
    }

    /// What the server is called inside that file.
    ///
    /// `sync`, everywhere. It used to be `sync-<folder>` in the files that
    /// serve a whole machine, because an entry named a project and a second
    /// project connecting would have silently disconnected the first. One
    /// server for every project is what retires that: there is one entry to
    /// write, and it says the same thing whichever project a person was looking
    /// at when they wrote it.
    const fn server_name(&self) -> &'static str {
        "sync"
    }
}

/// Where Sync's own sidecar is, which is what an agent will run.
///
/// The same resolution the window uses for its own session, for the same
/// reason: a connection that named a binary the window is not running would
/// point the agent at a different build of the engine.
fn sidecar<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
    if let Some(named) = std::env::var_os(crate::memory::BINARY_OVERRIDE) {
        return Ok(PathBuf::from(named));
    }
    std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
        .map(|directory| directory.join(crate::memory::BUNDLED_BINARY))
        .ok_or_else(|| {
            let _ = app;
            ProjectError::new(
                "configuration_failed",
                "could not locate Sync's own memory server".to_owned(),
            )
        })
}

/// How a row reads.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No entry under Sync's name.
    NotConnected,
    /// An entry Sync wrote, for this project.
    Connected,
    /// An entry under Sync's name that Sync did not write — it names another
    /// command, or another project. Said rather than replaced.
    Foreign,
    /// The file is there and cannot be read as what it is meant to be.
    Unreadable,
}

/// One row of the Agents section.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    /// Which heading it is listed under.
    pub group: Group,
    /// The file, as a person would recognise it.
    pub configuration: String,
    /// `project` or `installation`.
    pub scope: String,
    pub state: State,
    /// Why, when the state alone does not say it.
    pub detail: Option<String>,
}

/// What one call changed, in words the interface can show unchanged.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReport {
    /// The file that was written, in full, because a person about to look at it
    /// needs the whole path rather than the pretty one.
    pub file: String,
    /// What the server is called in it.
    pub server: String,
    /// One sentence: what changed, or that nothing did.
    pub changed: String,
    pub state: State,
}

/// Every client, and whether this project is connected to it.
///
/// # Errors
///
/// Returns [`ProjectError`] only when the machine cannot say where a
/// configuration file would live. A file that is missing, or unreadable, or
/// holds somebody else's entry is a *row*, not a failure: the section's job is
/// to describe what is there.
#[tauri::command(async)]
pub fn agents_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<AgentRow>, ProjectError> {
    let reached = reachable(&app);
    CLIENTS
        .iter()
        .map(|client| {
            let file = client.file(&app)?;
            let (state, detail) = inspect(client, &file, reached.as_ref());
            Ok(AgentRow {
                id: client.id.to_owned(),
                name: client.name.to_owned(),
                group: client.group,
                configuration: client.shown(),
                scope: client.scope().to_owned(),
                state,
                detail,
            })
        })
        .collect()
}

/// Write Sync into one client's configuration.
///
/// # Errors
///
/// Returns [`ProjectError`] when the file cannot be read or written, or when
/// something else already holds Sync's name in it.
#[tauri::command(async)]
pub fn agent_connect<R: Runtime>(
    app: AppHandle<R>,
    agent: String,
) -> Result<ConnectionReport, ProjectError> {
    let client = client(&agent)?;
    let file = client.file(&app)?;
    let server = client.server_name();
    let reached = reachable(&app).ok_or_else(|| {
        ProjectError::new(
            "server_unavailable",
            "Sync's own server has no address to give out yet.".to_owned(),
        )
    })?;

    // One read, and what is judged is what is written back. Reading the file
    // again to inspect it would be deciding against one copy and splicing into
    // another, with somebody's editor free to save in between.
    let text = read(&file)?;
    if let (State::Foreign, Some(detail)) = describe(client, &text, &file, Some(&reached)) {
        return Err(ProjectError::new("occupied", detail));
    }

    let (written, change) = splice(client, &text, &reached)?;

    if change != document::Change::Unchanged {
        write(&file, &written)?;
    }
    Ok(ConnectionReport {
        file: file.to_string_lossy().into_owned(),
        changed: sentence(change, server, &file),
        server: server.to_owned(),
        state: State::Connected,
    })
}

/// Take Sync back out of one client's configuration.
///
/// # Errors
///
/// Returns [`ProjectError`] when the file cannot be read or written.
#[tauri::command(async)]
pub fn agent_disconnect<R: Runtime>(
    app: AppHandle<R>,
    agent: String,
) -> Result<ConnectionReport, ProjectError> {
    let client = client(&agent)?;
    let file = client.file(&app)?;
    let server = client.server_name();

    let text = read(&file)?;
    let (written, change) = match client.wire {
        Wire::Json { holder, .. } | Wire::Stdio { holder } => {
            document::json_take(&text, holder, server)
        }
        Wire::CodexToml { table } | Wire::GrokToml { table } => {
            document::toml_take(&text, table, server)
        }
    }
    .map_err(|trouble| ProjectError::new("configuration_failed", trouble.to_string()))?;

    if change != document::Change::Unchanged {
        write(&file, &written)?;
    }
    Ok(ConnectionReport {
        file: file.to_string_lossy().into_owned(),
        changed: sentence(change, server, &file),
        server: server.to_owned(),
        state: State::NotConnected,
    })
}

/// Write this machine's server into `text`, in the shape `client` expects.
///
/// Separated from the command around it so it can be checked without a file:
/// what these seven clients disagree about is the shape of one entry, and that
/// disagreement is exactly the thing worth a test.
fn splice(
    client: &Client,
    text: &str,
    reached: &Reachable,
) -> Result<(String, document::Change), ProjectError> {
    let server = client.server_name();
    match client.wire {
        Wire::Json { holder, typed } => {
            let mut entry = serde_json::Map::new();
            if typed {
                entry.insert("type".to_owned(), json!("http"));
            }
            entry.insert("url".to_owned(), json!(reached.url));
            entry.insert(
                "headers".to_owned(),
                json!({"Authorization": reached.authorization()}),
            );
            let rendered = rendered(&entry)?;
            document::json_put(text, holder, server, &rendered)
        }
        Wire::Stdio { holder } => {
            let mut entry = serde_json::Map::new();
            entry.insert("command".to_owned(), json!(reached.binary));
            entry.insert(
                "args".to_owned(),
                json!(["--registry", reached.registry.as_str()]),
            );
            let rendered = rendered(&entry)?;
            document::json_put(text, holder, server, &rendered)
        }
        Wire::CodexToml { table } => {
            let mut entry = toml_edit::Table::new();
            entry.insert("url", toml_edit::value(reached.url.clone()));
            let mut headers = toml_edit::Table::new();
            headers.insert("Authorization", toml_edit::value(reached.authorization()));
            entry.insert("http_headers", toml_edit::Item::Table(headers));
            document::toml_put(text, table, server, entry)
        }
        Wire::GrokToml { table } => {
            let mut entry = toml_edit::Table::new();
            entry.insert("url", toml_edit::value(reached.url.clone()));
            entry.insert("enabled", toml_edit::value(true));
            let mut headers = toml_edit::Table::new();
            headers.insert("Authorization", toml_edit::value(reached.authorization()));
            entry.insert("headers", toml_edit::Item::Table(headers));
            document::toml_put(text, table, server, entry)
        }
    }
    .map_err(|trouble| ProjectError::new("configuration_failed", trouble.to_string()))
}

fn rendered(entry: &serde_json::Map<String, Value>) -> Result<String, ProjectError> {
    serde_json::to_string_pretty(&Value::Object(entry.clone())).map_err(|error| {
        ProjectError::new("configuration_failed", format!("unwritable entry: {error}"))
    })
}

/// Where this installation's server is, and what opens it.
///
/// Read fresh rather than remembered: the port and the token are the person's
/// to change from the Server section, and a connection written from a
/// remembered copy would point an agent at an address that has moved.
struct Reachable {
    url: String,
    token: String,
    /// For the one client that speaks no HTTP.
    binary: String,
    registry: String,
}

impl Reachable {
    fn authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

fn reachable<R: Runtime>(app: &AppHandle<R>) -> Option<Reachable> {
    let settings = crate::server::ServerSettings::load(app);
    Some(Reachable {
        url: settings.url(),
        token: settings.token,
        binary: sidecar(app).ok()?.to_string_lossy().into_owned(),
        registry: crate::project::configuration_file(app, "registered-projects.json")
            .ok()?
            .to_string_lossy()
            .into_owned(),
    })
}

fn client(id: &str) -> Result<&'static Client, ProjectError> {
    CLIENTS
        .iter()
        .find(|client| client.id == id)
        .ok_or_else(|| {
            ProjectError::new(
                "unknown_agent",
                format!("`{id}` is not a client Sync knows."),
            )
        })
}

/// What one file says, and why.
fn inspect(client: &Client, file: &Path, reached: Option<&Reachable>) -> (State, Option<String>) {
    let Ok(text) = read(file) else {
        return (
            State::Unreadable,
            Some(format!("{} could not be read.", file.display())),
        );
    };
    describe(client, &text, file, reached)
}

/// The same judgement, made against text the caller already holds.
///
/// Split out for the one caller that is about to write: it has read the file,
/// and deciding against a second read would be judging one copy and splicing
/// into another.
fn describe(
    client: &Client,
    text: &str,
    file: &Path,
    reached: Option<&Reachable>,
) -> (State, Option<String>) {
    let server = client.server_name();
    let held = match client.wire {
        Wire::Json { holder, .. } | Wire::Stdio { holder } => {
            document::json_read(text, holder, server)
        }
        Wire::CodexToml { table } | Wire::GrokToml { table } => {
            document::toml_read(text, table, server)
        }
    };
    let entry = match held {
        Ok(Some(entry)) => entry,
        Ok(None) => return (State::NotConnected, None),
        Err(trouble) => return (State::Unreadable, Some(trouble.to_string())),
    };

    // Ours by what it points at, not by a mark left in the file. A marker would
    // be a field in somebody else's schema, and an entry pointing at this
    // machine's own server is ours whether or not it carries one.
    let Some(reached) = reached else {
        return (State::Connected, None);
    };
    let points_here = match client.wire {
        Wire::Stdio { .. } => entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command == reached.binary),
        _ => entry
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| url == reached.url),
    };
    if !points_here {
        // An entry under Sync's name that Sync did not write — or wrote before
        // the address moved. The two are told apart by the token: an entry
        // carrying this machine's own is ours, pointing somewhere stale.
        let ours =
            header_of(&entry).is_some_and(|authorization| authorization == reached.authorization());
        return if ours {
            (
                State::Connected,
                Some(format!(
                    "`{server}` names an address Sync no longer listens on. Connect again to point it here."
                )),
            )
        } else {
            (
                State::Foreign,
                Some(format!(
                    "`{server}` in {} was written by something else — it does not point at this machine's Sync.",
                    file.display()
                )),
            )
        };
    }
    if header_of(&entry).is_some_and(|authorization| authorization == reached.authorization()) {
        return (State::Connected, None);
    }
    (
        State::Connected,
        Some(format!(
            "`{server}` carries an older token. Connect again to give it the current one."
        )),
    )
}

/// The `Authorization` a client's entry carries, wherever that client keeps it.
fn header_of(entry: &Value) -> Option<&str> {
    ["headers", "http_headers"]
        .iter()
        .find_map(|where_it_lives| entry.get(where_it_lives))
        .and_then(|headers| headers.get("Authorization"))
        .and_then(Value::as_str)
}

fn sentence(change: document::Change, server: &str, file: &Path) -> String {
    let file = file.display();
    match change {
        document::Change::Added => format!("Added `{server}` to {file}."),
        document::Change::Updated => format!("Updated `{server}` in {file}."),
        document::Change::Unchanged => format!("`{server}` in {file} already says this."),
        document::Change::Removed => format!("Removed `{server}` from {file}."),
        document::Change::Absent => format!("{file} held no `{server}` to remove."),
    }
}

/// The file's text, or empty when there is no file.
fn read(file: &Path) -> Result<String, ProjectError> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(ProjectError::new(
            "configuration_failed",
            format!("could not read {}: {error}", file.display()),
        )),
    }
}

fn write(file: &Path, text: &str) -> Result<(), ProjectError> {
    if let Some(directory) = file.parent() {
        std::fs::create_dir_all(directory).map_err(|error| {
            ProjectError::new(
                "configuration_failed",
                format!("could not make {}: {error}", directory.display()),
            )
        })?;
    }
    std::fs::write(file, text).map_err(|error| {
        ProjectError::new(
            "configuration_failed",
            format!("could not write {}: {error}", file.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{CLIENTS, Reachable, State, Wire, client, describe, splice};
    use std::path::Path;

    fn reachable() -> Reachable {
        Reachable {
            url: "http://127.0.0.1:41847/mcp".to_owned(),
            token: "a-token".to_owned(),
            binary: "/Applications/Sync.app/Contents/MacOS/sync-mcp".to_owned(),
            registry:
                "/Users/somebody/Library/Application Support/buzz.sync/registered-projects.json"
                    .to_owned(),
        }
    }

    #[test]
    fn every_client_is_told_the_same_address_in_its_own_words() {
        let reached = reachable();
        for client in CLIENTS {
            let (written, _) = splice(client, "", &reached).expect("an entry is written");
            assert!(
                written.contains("sync"),
                "`{}` names the server: {written}",
                client.id
            );
            match client.wire {
                // The one that speaks no HTTP is told how to start the server
                // instead — over the same registry, so it still reaches every
                // project.
                Wire::Stdio { .. } => {
                    assert!(written.contains("--registry"), "{written}");
                    assert!(written.contains(&reached.registry), "{written}");
                    assert!(
                        !written.contains(&reached.token),
                        "a pipe needs no token, and writing one would put a secret in a file for nothing: {written}"
                    );
                }
                _ => {
                    assert!(written.contains(&reached.url), "{written}");
                    assert!(
                        written.contains("Bearer a-token"),
                        "the entry carries the token: {written}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_client_is_written_into_a_repository() {
        // The rule this whole module turns on. A file inside a checkout is a
        // commit announcing to a team that somebody is trying Sync.
        for client in CLIENTS {
            let shown = client.shown();
            assert!(
                shown.starts_with('~') || shown.contains('/'),
                "`{}` keeps its configuration outside the project: {shown}",
                client.id
            );
            assert_eq!(client.scope(), "installation", "`{}`", client.id);
        }
    }

    #[test]
    fn an_entry_pointing_somewhere_else_is_reported_rather_than_replaced() {
        let reached = reachable();
        let theirs = r#"{"mcpServers": {"sync": {"url": "http://example.com/mcp"}}}"#;
        let (state, detail) = describe(
            client("cursor").expect("a client"),
            theirs,
            Path::new("/tmp/mcp.json"),
            Some(&reached),
        );
        assert!(matches!(state, State::Foreign), "{state:?} {detail:?}");
    }

    #[test]
    fn our_own_entry_with_a_stale_token_is_ours_and_says_so() {
        let reached = reachable();
        let older = r#"{"mcpServers": {"sync": {
            "url": "http://127.0.0.1:41847/mcp",
            "headers": {"Authorization": "Bearer an-older-token"}
        }}}"#;
        let (state, detail) = describe(
            client("cursor").expect("a client"),
            older,
            Path::new("/tmp/mcp.json"),
            Some(&reached),
        );
        assert!(matches!(state, State::Connected), "{state:?}");
        assert!(
            detail.is_some_and(|said| said.contains("older token")),
            "connecting again is what fixes it, so it is not Foreign"
        );
    }

    #[test]
    fn a_grok_entry_is_enabled_and_carries_its_header_where_grok_keeps_it() {
        let reached = reachable();
        let (written, _) =
            splice(client("grok-cli").expect("a client"), "", &reached).expect("an entry");
        assert!(written.contains("[mcp_servers.sync]"), "{written}");
        assert!(written.contains("enabled = true"), "{written}");
        assert!(written.contains("[mcp_servers.sync.headers]"), "{written}");
    }

    #[test]
    fn a_zed_entry_goes_under_the_holder_zed_reads_and_carries_no_type() {
        let reached = reachable();
        let (written, _) =
            splice(client("zed").expect("a client"), "", &reached).expect("an entry");
        assert!(written.contains("\"context_servers\""), "{written}");
        assert!(
            !written.contains("\"type\""),
            "a remote server in Zed is a url and its headers, and a `type` beside them is a key Zed does not read: {written}"
        );
    }

    #[test]
    fn a_codex_entry_keeps_its_header_where_codex_looks_for_it() {
        let reached = reachable();
        let (written, _) =
            splice(client("codex-cli").expect("a client"), "", &reached).expect("an entry");
        assert!(
            written.contains("[mcp_servers.sync.http_headers]"),
            "{written}"
        );
    }
}
