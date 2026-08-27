//! Opening a local folder as a project.
//!
//! Sync keeps a project's knowledge in Git, so a folder is only a project once
//! it is a repository. This module answers the questions the opening flow asks,
//! in the order it can ask them: is this folder inside a repository, make it
//! one, has it been opened as a project before, and remember that it was.
//!
//! What a project is called, what it is, and the language it writes in are
//! stored where the rest of the project's knowledge is — as one record in the
//! project's own memory. *Which* record, and what it looks like as an envelope,
//! is the sidecar's business: this module asks for settings and is handed
//! settings. That is what makes them the *project's* settings rather than this
//! machine's: they travel with the repository, and a person opening a project
//! that already exists is not asked to invent them a second time.
//!
//! The list of recently opened projects is the opposite kind of fact — it
//! belongs to this installation, not to any project — so it lives in the
//! application's configuration directory and never touches a repository.
//!
//! Git is driven through its own command line rather than through a library.
//! The engine already requires `git` to be installed, and `git init` is one
//! process doing exactly what a person would type — a linked library would add
//! a build dependency to gain nothing here.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Output;

use serde::{Deserialize, Serialize};
use sync_memory::{MemoryClient, ProjectSettings};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::memory::MemorySessions;

/// A failure, in the shape the frontend branches on.
///
/// `kind` is a closed vocabulary the interface switches on; the message is for
/// people and carries git's own words wherever it has any.
#[derive(Debug, Serialize)]
pub struct ProjectError {
    pub kind: String,
    pub message: String,
}

impl ProjectError {
    pub(crate) fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.into(),
        }
    }
}

pub use sync_memory::InstalledExtension;

/// Whether this repository has been opened as a project before.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettingsProbe {
    /// Present when the project already exists, in which case the opening flow
    /// has nothing to ask.
    pub settings: Option<ProjectSettings>,
    /// Why memory could not be consulted, when it could not be.
    ///
    /// `None` settings and an unreachable engine are not the same answer, and
    /// collapsing them would let a momentary engine failure present an existing
    /// project as a new one and overwrite what it already knew. The flow still
    /// asks — there is nothing else it can do — but it says why.
    pub memory_error: Option<String>,
}

/// A project this installation has opened before.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: String,
    pub name: String,
}

/// How many are kept. A menu is a shortcut, not a history: past the first
/// handful nobody scans it, and the folder picker is one item away.
const RECENT_LIMIT: usize = 8;

/// A project this installation will answer for, and the name it answers to.
///
/// Not the recent list, and deliberately not built on it. That list is a menu —
/// eight entries, oldest dropped — and a project falling off a menu is a
/// project nobody has opened lately. Falling out of *this* one would be a
/// project an agent can no longer reach, which is not the same event and must
/// not share a cause.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredProject {
    /// The repository root, absolute.
    pub path: String,
    /// What the window calls it.
    pub name: String,
    /// What everyone else calls it **here**.
    ///
    /// Normally the identifier in the project's own record, which is the same
    /// for everyone who opened that repository. It differs only where it had
    /// to: two repositories on one machine can derive the same identifier, and
    /// one of them has to answer to something else locally. The record is never
    /// touched for that — every text naming the project goes on meaning what it
    /// meant, and the divergence stays on the machine that has the collision.
    pub identifier: String,
}

/// What registering a project did.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    /// The project already answering to this identifier, when it is a different
    /// one. Nothing is written when this is present.
    ///
    /// Two repositories named the same thing derive the same identifier, and
    /// nothing about either of them is wrong — they simply cannot both answer
    /// to it on one machine. Which of them gives way is a question for the
    /// person, so this reports rather than renames: a suffix invented here
    /// would be exactly the machine-local name the identifier exists to avoid.
    pub taken_by: Option<RegisteredProject>,
}

/// Put `project` into `registry`, unless its identifier belongs to another one.
///
/// Re-registering a path that is already there replaces its entry in place: the
/// name follows the window, and a project keeps its position in the file rather
/// than moving to the end every time it is opened.
fn register(registry: &mut Vec<RegisteredProject>, project: RegisteredProject) -> Registration {
    if let Some(taken_by) = registry
        .iter()
        .find(|held| held.identifier == project.identifier && held.path != project.path)
    {
        return Registration {
            taken_by: Some(taken_by.clone()),
        };
    }
    match registry.iter_mut().find(|held| held.path == project.path) {
        Some(held) => *held = project,
        None => registry.push(project),
    }
    Registration { taken_by: None }
}

const REGISTERED_PROJECTS_FILE: &str = "registered-projects.json";

/// The projects this installation answers for.
///
/// Read by the window, and by whatever serves agents: an identifier is only
/// resolvable because this file says which path it names.
#[tauri::command(async)]
pub fn projects_registered<R: Runtime>(app: AppHandle<R>) -> Vec<RegisteredProject> {
    // An unreadable registry is an empty one, for the reason the recent list
    // is: there is nothing a person could do about it here, and the next
    // registration writes a good one.
    let Ok(path) = configuration_file(&app, REGISTERED_PROJECTS_FILE) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Register a project, or report which one already holds its identifier.
///
/// The identifier is the caller's: normally the one in the project's record,
/// and something else where the person had to settle a collision. Either way it
/// has to be an identifier, because it is what an agent will type.
#[tauri::command(async)]
pub fn project_register<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, crate::server::RunningServer>,
    project: RegisteredProject,
) -> Result<Registration, ProjectError> {
    if !sync_memory::mapping::is_identifier(&project.identifier) {
        return Err(ProjectError::new(
            "invalid_identifier",
            format!(
                "`{}` is not an identifier: letters and digits, separated by `-`, \
                 at most {} characters",
                project.identifier,
                sync_memory::mapping::IDENTIFIER_LIMIT
            ),
        ));
    }
    let mut registry = projects_registered(app.clone());
    let registration = register(&mut registry, project);
    if registration.taken_by.is_some() {
        return Ok(registration);
    }
    let path = configuration_file(&app, REGISTERED_PROJECTS_FILE)?;
    write_configuration(&path, &registry)?;
    // **The file is the protocol.** The server re-reads this registry whenever
    // it has changed (`sync-mcp/projects.rs`), so a project registered while it
    // is running is one it hears about on the next call that asks which
    // projects there are.
    //
    // It used to be restarted here instead, from before the server could
    // re-read. That is now the wrong thing twice over: it is unnecessary, and
    // every window on the machine holds a live connection to that process — so
    // registering a project would have taken everybody's memory away mid-write
    // to tell the server something it was about to notice anyway.
    let _ = running;
    Ok(registration)
}

/// Forget the project at `path`: take it out of the menu and stop answering
/// for it.
///
/// One gesture, both lists, because to a person there is one project and they
/// asked to be rid of it. Leaving it registered would keep it reachable by an
/// agent after it disappeared from the window, which is precisely the kind of
/// thing nobody would think to check.
///
/// Answers with the recent list, which is what the window redraws.
#[tauri::command(async)]
pub fn project_forget<R: Runtime>(
    app: AppHandle<R>,
    running: State<'_, crate::server::RunningServer>,
    schedule: State<'_, crate::schedule::ScheduleFile>,
    work: State<'_, crate::work::WorkFile>,
    path: String,
) -> Result<Vec<RecentProject>, ProjectError> {
    let mut registry = projects_registered(app.clone());
    registry.retain(|held| held.path != path);
    write_configuration(
        &configuration_file(&app, REGISTERED_PROJECTS_FILE)?,
        &registry,
    )?;

    let mut recent = recent_projects_load(app.clone());
    recent.retain(|entry| entry.path != path);
    write_configuration(&configuration_file(&app, RECENT_PROJECTS_FILE)?, &recent)?;
    // And the third list, for the same reason as the second. A project left
    // behind there would go on ticking after it disappeared from the window —
    // which is worse than being reachable by an agent, because nothing about it
    // is visible anywhere at all.
    crate::schedule::forget(&app, &schedule, &path);
    // And the fourth. Work ordered for a project a person has forgotten is an
    // account of something they asked to be rid of, and it names a folder the
    // window will not show again.
    crate::work::forget(&app, &work, &path);
    // Out of the registry means out of reach, and the server notices by
    // re-reading the file rather than by being restarted — see the note in
    // [`project_register`] on why restarting it here would now cost every open
    // window its memory.
    let _ = running;
    Ok(recent)
}

/// What this installation shows of a project, as opposed to what the project
/// is.
///
/// Hiding a type is one person deciding they do not work with artifacts. It is
/// not a fact about the project — the records are still there, and an agent
/// still writes them — so it lives beside the recent list in this
/// installation's configuration and never touches the repository. A colleague
/// pulling the same project is not missing a type because of a checkbox somebody
/// else unticked.
///
/// Arranging the sidebar is the same kind of decision, which is why the order
/// of the sections is here rather than in the project's record. A person moves
/// a section to the top because that is where they work; a drag of a row is not
/// a claim about the project, and writing one into `refs/memory/*` would commit
/// somebody's habits to a colleague's column.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    /// Kinds the window does not list. Kind names, as the store spells them.
    #[serde(default)]
    pub hidden_types: Vec<String>,
    /// The sections this person put in the order they want them, by area key.
    ///
    /// Not the whole column and not required to be: it is what was arranged,
    /// and a section installed since is one nobody has placed yet. The window
    /// resolves that against what it actually has — see `use-section-order.ts`
    /// — so a key that no longer names anything costs nothing and a section
    /// missing from the list still appears.
    #[serde(default)]
    pub sections: Vec<String>,
}

/// A change to what this installation shows of a project.
///
/// The two fields are decided in two different columns — the navigator's type
/// filter and the sidebar — so a write says only what it changed. `None` leaves
/// what was stored; an empty list is a list somebody emptied, which is what
/// *Show All Types* means and has to stay distinguishable from silence. Sending
/// the whole view instead would let either column quietly erase the other's
/// setting, and neither of them mentions the other anywhere.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectViewChange {
    #[serde(default)]
    pub hidden_types: Option<Vec<String>>,
    #[serde(default)]
    pub sections: Option<Vec<String>>,
}

/// What this installation shows of a project.
#[tauri::command(async)]
pub fn project_view_load<R: Runtime>(app: AppHandle<R>, project: String) -> ProjectView {
    // A missing or unreadable file is "nothing is hidden", which is the state
    // every project starts in and the one a person can fix by ticking a box.
    project_views(&app).remove(&project).unwrap_or_default()
}

/// Remember what this installation shows of a project.
///
/// Answers with the whole view as it now stands, so a caller that wrote one
/// half of it can see the half it did not touch rather than assume it.
#[tauri::command(async)]
pub fn project_view_save<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    view: ProjectViewChange,
) -> Result<ProjectView, ProjectError> {
    let mut views = project_views(&app);
    let stored = views.entry(project).or_default();
    apply(stored, view);
    let saved = stored.clone();

    let path = configuration_file(&app, PROJECT_VIEWS_FILE)?;
    write_configuration(&path, &views)?;
    Ok(saved)
}

/// The change, over what was stored.
///
/// Its own function because it is the whole of what the command decides, and
/// what it decides is the thing that goes wrong silently: a field the caller
/// did not mention is a field it did not change, and treating an absent one as
/// an empty one would erase a column nobody touched without any error to show
/// for it.
fn apply(stored: &mut ProjectView, change: ProjectViewChange) {
    if let Some(hidden_types) = change.hidden_types {
        stored.hidden_types = hidden_types;
    }
    if let Some(sections) = change.sections {
        stored.sections = sections;
    }
}

fn project_views<R: Runtime>(app: &AppHandle<R>) -> BTreeMap<String, ProjectView> {
    configuration_file(app, PROJECT_VIEWS_FILE)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

const PROJECT_VIEWS_FILE: &str = "project-views.json";
const RECENT_PROJECTS_FILE: &str = "recent-projects.json";

/// What a folder is, as far as opening a project is concerned.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderProbe {
    /// The folder that was chosen, absolute.
    pub path: String,
    /// The name to offer as the project's, taken from the folder.
    pub name: String,
    /// The work tree the folder belongs to, or `None` when it belongs to none.
    /// It differs from `path` when a folder *inside* a repository was chosen,
    /// which is the one case where the project is not the folder picked.
    pub repository_root: Option<String>,
}

/// Describe a folder without changing it.
#[tauri::command(async)]
pub fn project_probe(path: String) -> Result<FolderProbe, ProjectError> {
    probe(Path::new(&path))
}

/// Make a folder a Git repository, then describe it again.
///
/// Both commands here are `#[tauri::command(async)]` for the same reason the
/// memory commands are: they wait on a child process, and a plain
/// `#[tauri::command]` would do that on the main thread and freeze the window.
#[tauri::command(async)]
pub fn project_initialize_repository(path: String) -> Result<FolderProbe, ProjectError> {
    let path = Path::new(&path);
    require_directory(path)?;

    let output = run_git(path, &["init"])?;
    if !output.status.success() {
        return Err(ProjectError::new(
            "git_failed",
            git_message(&output, "git init did not create a repository."),
        ));
    }

    // Project memory is deliberately not initialized here. It belongs to the
    // memory session, which the opening flow does not start yet.
    probe(path)
}

/// Where this repository's code came from, as `origin` names it.
///
/// **Not the memory's remote.** `memory_remote_set` configures where the
/// project's *knowledge* is pushed, and the two are deliberately different
/// things — a project may keep its memory somewhere its code is not. This is
/// the code's, and it is the one an extension means when it asks what
/// repository this project is.
///
/// Answered whole and unparsed, in whatever spelling git holds it: `git@…`,
/// `https://…`, a path on this disk. What a URL *means* is not this layer's
/// question — the core may not know what GitHub is, and a build that turned
/// this into an owner and a repository would have decided that for every
/// extension that ever reads it.
///
/// `None` for a repository nobody has given an `origin`, which is an ordinary
/// state and not a failure: `git remote get-url` exits non-zero for it, the
/// same way `rev-parse` does for a folder outside a repository.
///
/// # Errors
///
/// When git cannot be run at all, which no amount of retrying will fix.
#[tauri::command(async)]
pub fn project_remote(path: String) -> Result<Option<String>, ProjectError> {
    let path = Path::new(&path);
    require_directory(path)?;

    let output = run_git(path, &["remote", "get-url", "origin"])?;
    if !output.status.success() {
        return Ok(None);
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if url.is_empty() { None } else { Some(url) })
}

fn probe(path: &Path) -> Result<FolderProbe, ProjectError> {
    require_directory(path)?;

    Ok(FolderProbe {
        path: path.display().to_string(),
        name: folder_name(path),
        repository_root: repository_root(path)?,
    })
}

fn require_directory(path: &Path) -> Result<(), ProjectError> {
    if path.is_dir() {
        return Ok(());
    }
    Err(ProjectError::new(
        "not_a_directory",
        format!("{} is not a folder.", path.display()),
    ))
}

/// The name to offer for the project. A path with no last component — the
/// volume root — has no name to take, and the person naming the project is
/// about to see the field anyway.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_owned()
}

/// The work tree `git` reports for a folder.
///
/// A folder outside any repository is an answer, not a failure: git exits
/// non-zero and says so, and offering to change that is the whole job of the
/// opening flow. Only being unable to run git at all is an error.
fn repository_root(path: &Path) -> Result<Option<String>, ProjectError> {
    let output = run_git(path, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Ok(None);
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!root.is_empty()).then_some(root))
}

fn run_git(directory: &Path, arguments: &[&str]) -> Result<Output, ProjectError> {
    Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ProjectError::new(
                "git_missing",
                "Git is not installed, or is not on this application's PATH.",
            ),
            _ => ProjectError::new("git_failed", format!("could not run git: {error}")),
        })
}

/// What git said about a failure, or a sentence of our own when it said
/// nothing worth repeating.
fn git_message(output: &Output, fallback: &str) -> String {
    let reported = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if reported.is_empty() {
        fallback.to_owned()
    } else {
        reported
    }
}

/// Read the project's own record of what it is called.
///
/// This is what decides whether the opening flow asks anything at all: a
/// repository whose memory already carries a project record has been opened
/// before, and re-asking would be the application forgetting rather than the
/// person changing their mind.
#[tauri::command(async)]
pub fn project_settings_load<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
) -> ProjectSettingsProbe {
    match sessions.with_session(&app, &project, MemoryClient::project_settings) {
        Ok(settings) => ProjectSettingsProbe {
            settings,
            memory_error: None,
        },
        Err(error) => ProjectSettingsProbe {
            settings: None,
            memory_error: Some(error.message),
        },
    }
}

/// Write the project's record, creating the project's memory if this is its
/// first write.
///
/// One type definition travels in the same transaction: `project`. The engine
/// runs a strict schema and rejects a record whose kind it has no definition
/// for, so on a repository that has never held memory that definition has to
/// land no later than the record it describes — and one transaction is what
/// makes this all-or-nothing rather than a half-created project.
///
/// Nothing else is published. A new project knows what it is called and nothing
/// about what it may say; the types it works in are created in the window or by
/// an agent, when there is something to say in them.
#[tauri::command(async)]
pub fn project_settings_save<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, MemorySessions>,
    project: String,
    settings: ProjectSettings,
) -> Result<(), ProjectError> {
    // Refused here rather than corrected. An identifier is what people and
    // agents call this project by, so a value the window would not have
    // produced is a value somebody would have to unlearn later — and there is
    // no later, because it is written once and never edited.
    if !sync_memory::mapping::is_identifier(&settings.identifier) {
        return Err(ProjectError::new(
            "invalid_identifier",
            format!(
                "`{}` is not an identifier: letters and digits, separated by `-`, \
                 at most {} characters",
                settings.identifier,
                sync_memory::mapping::IDENTIFIER_LIMIT
            ),
        ));
    }
    sessions
        .with_session(&app, &project, |client| client.update_project(&settings))
        .map_err(|error| ProjectError::new("memory_failed", error.message))?;
    Ok(())
}

/// The identifier a project of this name would get.
///
/// The rule lives in Rust and is asked for rather than reimplemented in the
/// window: an identifier the window derived one way and the record another
/// would be two projects with one name. Answers with an empty string when the
/// name holds nothing to build one from, which the form shows as a field
/// waiting to be filled rather than as an error.
#[tauri::command]
#[must_use]
pub fn project_identifier_suggest(name: String) -> String {
    sync_memory::mapping::identifier_from_name(&name)
}

/// The projects this installation has opened, most recent first.
#[tauri::command(async)]
pub fn recent_projects_load<R: Runtime>(app: AppHandle<R>) -> Vec<RecentProject> {
    // A missing, unreadable or malformed list is an empty menu section, not a
    // failure: nothing is lost, and there is nothing for a person to do about
    // it. The list is a convenience this module wrote itself.
    let Ok(path) = configuration_file(&app, RECENT_PROJECTS_FILE) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Record that a project was opened, moving it to the front of the list.
#[tauri::command(async)]
pub fn recent_projects_record<R: Runtime>(
    app: AppHandle<R>,
    project: RecentProject,
) -> Result<Vec<RecentProject>, ProjectError> {
    let mut recent = recent_projects_load(app.clone());
    recent.retain(|entry| entry.path != project.path);
    recent.insert(0, project);
    recent.truncate(RECENT_LIMIT);

    let path = configuration_file(&app, RECENT_PROJECTS_FILE)?;
    write_configuration(&path, &recent)?;

    Ok(recent)
}

/// One of this installation's own files, in the application's configuration
/// directory. Nothing a project owns is kept here.
pub(crate) fn configuration_file<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
) -> Result<PathBuf, ProjectError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(name))
        .map_err(|error| {
            ProjectError::new(
                "configuration_failed",
                format!("could not resolve the configuration directory: {error}"),
            )
        })
}

pub(crate) fn write_configuration<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), ProjectError> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| {
            ProjectError::new(
                "configuration_failed",
                format!("could not create the configuration directory: {error}"),
            )
        })?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| ProjectError::new("configuration_failed", error.to_string()))?;
    std::fs::write(path, text).map_err(|error| {
        ProjectError::new(
            "configuration_failed",
            format!("could not write {}: {error}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{ProjectView, ProjectViewChange, RegisteredProject, apply, register};

    fn project(path: &str, name: &str, identifier: &str) -> RegisteredProject {
        RegisteredProject {
            path: path.to_owned(),
            name: name.to_owned(),
            identifier: identifier.to_owned(),
        }
    }

    #[test]
    fn a_registered_project_is_one_the_installation_answers_for() {
        let mut registry = Vec::new();
        assert!(
            register(&mut registry, project("/a", "Atlas", "ATLAS"))
                .taken_by
                .is_none()
        );
        assert_eq!(registry, vec![project("/a", "Atlas", "ATLAS")]);
    }

    #[test]
    fn registering_a_path_again_replaces_it_where_it_stands() {
        let mut registry = vec![
            project("/a", "Atlas", "ATLAS"),
            project("/b", "Beacon", "BEACON"),
        ];
        register(&mut registry, project("/a", "Atlas Renamed", "ATLAS"));
        assert_eq!(
            registry,
            vec![
                project("/a", "Atlas Renamed", "ATLAS"),
                project("/b", "Beacon", "BEACON"),
            ],
            "the name follows the window and the position does not move"
        );
    }

    #[test]
    fn an_identifier_another_project_holds_is_reported_and_nothing_is_written() {
        let mut registry = vec![project("/a", "Playground", "PLAYGROUND")];
        let registration = register(
            &mut registry,
            project("/elsewhere", "Playground", "PLAYGROUND"),
        );
        assert_eq!(
            registration.taken_by,
            Some(project("/a", "Playground", "PLAYGROUND")),
            "the person is told which project already answers to it"
        );
        assert_eq!(
            registry,
            vec![project("/a", "Playground", "PLAYGROUND")],
            "and the registry is untouched — no suffix was invented"
        );
    }

    #[test]
    fn the_registry_does_not_forget_what_the_menu_would_have_dropped() {
        let mut registry = Vec::new();
        for nth in 0..20 {
            register(
                &mut registry,
                project(
                    &format!("/p{nth}"),
                    &format!("Project {nth}"),
                    &format!("PROJECT-{nth}"),
                ),
            );
        }
        assert_eq!(
            registry.len(),
            20,
            "an agent reaching a project must not depend on how recently a person opened it"
        );
    }

    fn view(hidden: &[&str], sections: &[&str]) -> ProjectView {
        ProjectView {
            hidden_types: hidden.iter().map(|entry| (*entry).to_owned()).collect(),
            sections: sections.iter().map(|entry| (*entry).to_owned()).collect(),
        }
    }

    /// The half of this file that fails silently if it fails at all.
    ///
    /// Two columns write here and neither mentions the other: the navigator's
    /// type filter and the sidebar's order. A write that carried the whole view
    /// would let either of them erase the other's setting on its way past, with
    /// no error anywhere and nothing to see until the next launch.
    #[test]
    fn a_column_that_says_nothing_about_the_other_leaves_it_alone() {
        let mut stored = view(&["artifact"], &["records/records", "chat/chat"]);

        apply(
            &mut stored,
            ProjectViewChange {
                hidden_types: Some(vec!["question".to_owned()]),
                sections: None,
            },
        );

        assert_eq!(stored.hidden_types, vec!["question".to_owned()]);
        assert_eq!(
            stored.sections,
            vec!["records/records".to_owned(), "chat/chat".to_owned()],
            "hiding a type must not rearrange the sidebar"
        );

        apply(
            &mut stored,
            ProjectViewChange {
                hidden_types: None,
                sections: Some(vec!["chat/chat".to_owned(), "records/records".to_owned()]),
            },
        );

        assert_eq!(
            stored.hidden_types,
            vec!["question".to_owned()],
            "and rearranging the sidebar must not show a type again"
        );
        assert_eq!(
            stored.sections,
            vec!["chat/chat".to_owned(), "records/records".to_owned()]
        );
    }

    /// An empty list is a list somebody emptied — *Show All Types* is exactly
    /// that — and it has to stay distinguishable from a column saying nothing.
    #[test]
    fn an_empty_list_clears_and_an_absent_one_does_not() {
        let mut stored = view(&["artifact"], &["records/records"]);
        apply(
            &mut stored,
            ProjectViewChange {
                hidden_types: Some(Vec::new()),
                sections: None,
            },
        );
        assert!(stored.hidden_types.is_empty());
        assert_eq!(stored.sections, vec!["records/records".to_owned()]);
    }

    /// The names the window actually sends, because a field that crosses this
    /// boundary under the wrong spelling arrives as a default and says nothing.
    #[test]
    fn the_window_and_the_store_spell_the_fields_the_same_way() {
        let change: ProjectViewChange =
            serde_json::from_value(serde_json::json!({ "sections": ["records/records"] }))
                .expect("the window's own payload should deserialize");
        assert!(
            change.hidden_types.is_none(),
            "a field the window did not send is absent, not empty"
        );
        assert_eq!(change.sections, Some(vec!["records/records".to_owned()]));

        let stored = serde_json::to_value(view(&["artifact"], &["records/records"]))
            .expect("a stored view should serialize");
        assert_eq!(stored["hiddenTypes"][0], "artifact");
        assert_eq!(stored["sections"][0], "records/records");
    }
}
