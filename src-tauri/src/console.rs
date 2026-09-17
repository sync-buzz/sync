//! What the console asks for, and what it is allowed to ask.
//!
//! A line typed into the console that is neither a process nor a declared verb
//! is a piece of work: an agent is raised for it, it is told the line, and the
//! answer comes back as one block. This is where that is started and where the
//! ones still going are listed.
//!
//! # Why this is not a second way of talking to an agent
//!
//! Everything below goes through [`crate::sessions`], and nothing here knows
//! how an agent is raised or spoken to. What it adds is the two facts the
//! session layer has no business deciding: *which* agent — the one chosen in
//! settings, rather than one a webview named — and that the session belongs to
//! the console rather than to the list of conversations.
//!
//! # Why the window does not name the agent
//!
//! Because a person did, once, in settings, and the console is not a place to
//! choose again. The window naming one per line would be the choice made in two
//! places, and the second of them is a text field.

use std::path::PathBuf;

use sync_terminal::{Opening, Size, TerminalId, Terminals};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, Runtime, State};

use crate::project::ProjectError;
use crate::sessions::live::Sessions;
use crate::sessions::{SessionRow, row};
use crate::terminal::TerminalEvent;

/// Start a piece of work for a line somebody typed.
///
/// Answers as soon as the agent is up and has been told the line — not when it
/// has finished. A turn runs on its own task ([`crate::sessions::session_prompt`]),
/// so what this returns is the work, not its outcome, and the outcome arrives
/// on the subscription like every other.
///
/// # Errors
///
/// [`ProjectError`] when nothing was typed, when no agent has been chosen to
/// work through, and whatever raising an agent answers with.
#[tauri::command(async)]
pub async fn console_work_start<R: Runtime>(
    app: AppHandle<R>,
    sessions: State<'_, Sessions>,
    project: String,
    // What the person called it, which is how they will address it again. Kept
    // as they typed it — matching it is somebody else's job and is done without
    // regard to case, but what a list shows is what they wrote.
    name: String,
    text: String,
) -> Result<SessionRow, ProjectError> {
    if text.trim().is_empty() {
        return Err(ProjectError::new(
            "nothing_typed",
            "there is nothing to ask about".to_owned(),
        ));
    }
    if name.trim().is_empty() {
        return Err(ProjectError::new(
            "nothing_addressed",
            "a piece of work is addressed by name".to_owned(),
        ));
    }

    // Refused before anything is raised, and the refusal says where the choice
    // is made: a person who has never opened that card has no reason to know
    // that this line needed one.
    let agent = crate::flagship::working_agent(&app).ok_or_else(|| {
        ProjectError::new(
            "no_flagship",
            "no agent is chosen to work through yet — choose one in Settings".to_owned(),
        )
    })?;

    let session =
        crate::sessions::raise_for_console(&app, &agent, std::path::Path::new(&project)).await?;

    // Named before it is spoken to, because the first thing said is what a
    // conversation is otherwise called: a work addressed as `@review` that
    // listed itself by the first words of the question would be unaddressable
    // by the name it was given.
    session.set_title(&name);

    // Said straight away rather than handed back for the window to say, because
    // a session raised and never spoken in is not a conversation and holds a
    // process for nothing. The two are one action to whoever typed the line.
    crate::sessions::session_prompt(
        app.clone(),
        sessions,
        session.key.clone(),
        text,
        Vec::new(),
        Vec::new(),
    )
    .await?;

    Ok(row(&session))
}

/// The console's work, for the project whose window is asking.
///
/// By project, because a console belongs to one: work started in another
/// project goes on running and is none of this window's business to draw. What
/// is still going and what has finished are both here — a row carries its own
/// status, and deciding which of them to put on the shelf is the window's.
#[tauri::command(async)]
pub fn console_works(sessions: State<'_, Sessions>, project: String) -> Vec<SessionRow> {
    let project = std::path::Path::new(&project);
    let mut rows: Vec<SessionRow> = sessions
        .all()
        .iter()
        .filter(|session| session.console && session.project == project)
        .map(row)
        .collect();
    rows.sort_by_key(|row| row.opened_at_ms);
    rows
}

/**
 * Who the console's processes belong to.
 *
 * Spelled with a dot so that it cannot collide with a package: an extension's
 * id is lower-case letters, digits and hyphens, so no manifest can ever claim
 * this name. The terminals a package opens and the ones the console opens are
 * held in one registry, and what keeps them apart is whose name is on them.
 */
const SHELL_OPENER: &str = "console.shell";

/**
 * Run a line in a process, and answer with the terminal it is running in.
 *
 * Unlike [`crate::terminal::terminal_open`] this one is *told what to run*: a
 * package asking for a terminal gets the person's login shell and types into
 * it, while the line typed into a console is a command a person wrote, and
 * handing it to the shell as an argument is what the prefix `!` means.
 *
 * # This door is not guarded, and pretending otherwise would be worse
 *
 * Everything in this webview shares one origin, so an installed package can
 * call this and run anything this account can run. No capability is checked,
 * because there is nothing here to check one against: a package's code and the
 * shell's code are the same origin to Tauri, and a check that cannot tell them
 * apart is a check that refuses nobody.
 *
 * What it is bounded by is *where*: the folder is the one the host wrote down
 * for that tab, not one the caller names. That is worth having and it is not a
 * boundary — the honest sentence is that running commands from this window is
 * as open as running them from a terminal, and closing it means the console in
 * a webview of its own.
 *
 * The login shell, because a command a person writes expects their own
 * environment: an application started from the Dock inherits no profile, and
 * `-lc` is what gets it back.
 *
 * # Errors
 *
 * When nothing was typed, when the tab has no folder, or when the system
 * refuses to open a pty.
 */
#[tauri::command(async)]
pub async fn console_shell_start<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    tab: String,
    line: String,
    size: Size,
) -> Result<TerminalId, String> {
    if line.trim().is_empty() {
        return Err("there is nothing to run".to_owned());
    }
    let cwd = app
        .state::<Working>()
        .of(&tab)
        .ok_or_else(|| "this tab has no folder yet".to_owned())?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let opening = Opening {
        cwd,
        size,
        program: vec![shell, "-lc".to_owned(), line],
        env: Vec::new(),
    };

    app.state::<Terminals>()
        .open(&project, SHELL_OPENER, &opening)
        .map_err(|error| error.to_string())
}

/**
 * Watch what a console's process is saying, from an offset the caller names.
 *
 * The same watching every terminal gets — one implementation, reached with the
 * console's name in place of a package's. A second one here would be a second
 * answer to what happens when a reader falls behind, and the answer is the hard
 * part.
 *
 * # Errors
 *
 * When nothing is open under that id.
 */
#[tauri::command(async)]
pub fn console_shell_watch<R: Runtime>(
    app: AppHandle<R>,
    id: TerminalId,
    from: u64,
    events: Channel<TerminalEvent>,
) -> Result<(), String> {
    crate::terminal::terminal_watch(app, SHELL_OPENER.to_owned(), id, from, events)
}

/// Ends one of the console's processes.
#[tauri::command(async)]
pub fn console_shell_close(terminals: State<'_, Terminals>, id: TerminalId) {
    terminals.close(&id, SHELL_OPENER);
}

/// Where each of a window's tabs is working, as the host knows it.
///
/// **Held here rather than taken as an argument, and that is the whole of the
/// boundary.** A root that arrives with the call is a root the caller chooses,
/// so a check against it checks nothing: anything running in this webview —
/// an installed package included — would pass `/` and read the disk. What is
/// enforced has to be what the host wrote down.
#[derive(Default)]
pub struct Working {
    folders: std::sync::Mutex<std::collections::HashMap<String, PathBuf>>,
    /// How many tabs this installation has handed out names for.
    minted: std::sync::atomic::AtomicU64,
}

impl Working {
    /// A name for a tab, minted here so that no caller can choose one.
    ///
    /// A name the webview picked would be a name anything in the webview could
    /// pick again: two windows would collide on `tab1`, and a made-up one would
    /// carry a folder belonging to no tab anybody can see.
    fn mint(&self) -> String {
        let counted = self
            .minted
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("tab{counted}")
    }

    fn set(&self, tab: &str, at: PathBuf) {
        // A poisoned lock is somebody else's panic, not a reason to lose the
        // folder: what is inside is a map, and a map is no less valid for the
        // thread that was holding it having died.
        let mut held = self
            .folders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.insert(tab.to_owned(), at);
    }

    fn of(&self, tab: &str) -> Option<PathBuf> {
        self.folders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tab)
            .cloned()
    }

    /// Forgets a tab. A folder nobody is working in is a folder nobody may read.
    fn forget(&self, tab: &str) {
        self.folders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(tab);
    }
}

/// What a folder holds, as a console completes a path with it.
///
/// Names and nothing else — no size, no time, no content. What a completion
/// needs is what can be typed next; everything else would be this window
/// reading somebody's disk to draw a list nobody asked for.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// Whether the walk can continue into it.
    pub folder: bool,
}

/**
 * What stands in one folder, for completing a path.
 *
 * # What the root is, and what it is worth
 *
 * The folder is the one the host wrote down for this tab, never one the caller
 * names: a root that arrives with the call is a root the caller chose, and a
 * check against it checks nothing.
 *
 * It bounds *this* command and no more. Reading a folder is the smaller of the
 * two doors the console opens — [`console_shell_start`] runs commands from the
 * same webview and is not guarded at all — so what this buys is that path
 * completion cannot become a quiet way to enumerate a disk. It does not make
 * the window safe against code running inside it, and nothing in this file
 * does.
 *
 * # Errors
 *
 * When either path cannot be resolved, when the target is outside the root, or
 * when the folder cannot be read.
 */
#[tauri::command(async)]
pub async fn console_folder<R: Runtime>(
    app: AppHandle<R>,
    tab: String,
    path: String,
) -> Result<Vec<Entry>, String> {
    let root = app
        .state::<Working>()
        .of(&tab)
        .ok_or_else(|| "this tab has no folder yet".to_owned())?;
    // Resolved before comparing, because `..` and a symbolic link are the two
    // ways a path says one thing and means another. Both roads out are closed
    // by the same check: what the system says it is has to be under what the
    // host wrote down for this tab.
    let asked = tokio::fs::canonicalize(&path)
        .await
        .map_err(|error| format!("that folder could not be read: {error}"))?;
    if !asked.starts_with(&root) {
        return Err("that is outside this tab's folder — change it with cd".to_owned());
    }

    let mut reading = tokio::fs::read_dir(&asked)
        .await
        .map_err(|error| format!("that folder could not be read: {error}"))?;
    let mut found = Vec::new();
    while let Ok(Some(entry)) = reading.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let folder = entry
            .file_type()
            .await
            .map(|kind| kind.is_dir())
            .unwrap_or(false);
        found.push(Entry { name, folder });
    }
    // Folders first, then by name: the same order the completion draws, decided
    // once here rather than in whichever list happens to be showing it.
    found.sort_by(|left, right| {
        right
            .folder
            .cmp(&left.folder)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(found)
}

/**
 * Work somewhere else, and answer with where that turned out to be.
 *
 * What a tab is working in is a fact the host holds, so that anything checked
 * against it is checked against something the caller did not choose.
 *
 * **A relative path is resolved against the tab's own folder**, which is what a
 * person means by `cd docs`. Resolved against this process's folder — the
 * default for a bare path — it would land wherever the application happened to
 * be started, which is nowhere anybody typed.
 *
 * # What this does not defend against
 *
 * The same thing [`console_shell_start`] does not: a package sharing this
 * origin can move a tab. Since it can also run commands outright through
 * [`console_shell_start`], moving a tab is the smaller of the two, and both are
 * closed by the same thing — a console that does not share a webview with
 * anybody's code.
 *
 * # Errors
 *
 * When the tab is not one this host minted, or the path is not a folder.
 */
#[tauri::command(async)]
pub async fn console_cd<R: Runtime>(
    app: AppHandle<R>,
    tab: String,
    path: String,
) -> Result<String, String> {
    let held = app.state::<Working>();
    // Refused for a name this host never handed out: a tab nobody can see is a
    // folder nobody is working in, and remembering one would be inventing a
    // place for whoever asked.
    let from = held
        .of(&tab)
        .ok_or_else(|| "that is not a tab of this window".to_owned())?;

    let asked = PathBuf::from(expanded(&path));
    let asked = if asked.is_absolute() {
        asked
    } else {
        from.join(asked)
    };
    let at = tokio::fs::canonicalize(&asked)
        .await
        .map_err(|error| format!("that folder could not be opened: {error}"))?;
    if !at.is_dir() {
        return Err("that is a file, not a folder".to_owned());
    }
    let said = at.to_string_lossy().into_owned();
    held.set(&tab, at);
    Ok(said)
}

/// `~` as a person writes it. A shell would expand it, and this does not run one.
fn expanded(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_owned();
    };
    // `~somebody` is another account's home, which this does not look up.
    if !(rest.is_empty() || rest.starts_with('/')) {
        return path.to_owned();
    }
    match std::env::var("HOME") {
        Ok(home) => format!("{home}{rest}"),
        Err(_) => path.to_owned(),
    }
}

/// Opens a tab, and answers with the name the host will know it by.
///
/// Minted here rather than taken, which is what keeps two windows from meeting
/// on `tab1` and what makes every folder held below belong to a tab somebody
/// can see.
///
/// # Errors
///
/// When the project folder cannot be resolved.
#[tauri::command(async)]
pub async fn console_tab<R: Runtime>(app: AppHandle<R>, project: String) -> Result<String, String> {
    let at = tokio::fs::canonicalize(&project)
        .await
        .map_err(|error| format!("that project could not be opened: {error}"))?;
    let held = app.state::<Working>();
    let tab = held.mint();
    held.set(&tab, at);
    Ok(tab)
}

/// Closes a tab. A folder nobody is working in is a folder nobody may read.
#[tauri::command(async)]
pub fn console_tab_close<R: Runtime>(app: AppHandle<R>, tab: String) {
    app.state::<Working>().forget(&tab);
}
