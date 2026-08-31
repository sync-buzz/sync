//! A working tree made for one conversation, and thrown away with it.
//!
//! An agent is ordinarily raised in the project's own working tree — the tree
//! somebody has open, with whatever is half-finished in it. That is the right
//! place for a conversation a person is watching and the wrong place for one
//! nobody is: work ordered while the machine is unattended lands in the same
//! files, and there is no gesture that takes it back. This module is the other
//! choice. A tree is made from `HEAD`, used once, and either named or removed.
//!
//! # No branch is created
//!
//! The tree is added detached, so **the repository gains no reference at all**
//! while the work is being done. Branch naming is a convention of whoever owns
//! the repository, and a name this application invented would appear in
//! everybody's `git branch` output as something they did not choose and cannot
//! predict. The name is asked for at the moment the work is taken —
//! [`worktree_adopt`] — and until then Sync has put nothing in the repository
//! to be named.
//!
//! What that costs is worth saying plainly: commits made in a detached tree are
//! reachable through that tree's `HEAD` and nothing else, so
//! [`worktree_discard`] throws the work away. That is the gesture rather than an accident — the whole point
//! of working somewhere disposable is the right to say no in the morning — but
//! it is a deletion and whatever offers it has to say so first.
//!
//! [`worktree_adopt`] leaves the tree detached after creating the branch. A branch
//! checked out in one tree cannot be checked out in another, so pointing this
//! tree at the new branch would take it away from the person who just named it.
//!
//! # Isolation here means reversibility, not safety
//!
//! An agent working in a tree still has a shell, and a shell reaches the whole
//! machine. `docs/background.md` §9 already declines to promise a sandbox and
//! this changes nothing about that. What a separate tree gives is that the
//! files an agent edited are files nobody else is looking at, and removing them
//! costs one command.
//!
//! # Where the trees live
//!
//! In this installation's configuration, not in the project: which disk has
//! room is a fact about a machine, and a path remembered in a repository would
//! be wrong on the next machine that cloned it. Each project gets a directory
//! of its own underneath, named after its folder, so that a person looking at
//! the location sees which project they are looking at. The trees themselves
//! are named in words rather than keyed, because two of them made from `main`
//! this afternoon are otherwise the same in every fact anybody holds about
//! them — see [`name_in`].
//!
//! Git is driven through its own command line, for the reason
//! [`crate::project`] gives: the engine already requires git, and a linked
//! library would buy nothing here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::project::{ProjectError, configuration_file, git_message, run_git, write_configuration};

/// Where this installation keeps its choice of location.
const FILE: &str = "worktrees.json";

/// The directory the default location is made under, inside application data.
const DEFAULT_DIRECTORY: &str = "worktrees";

/// Which tree a conversation is to be held in, as the caller asks for it.
///
/// Two answers rather than a flag, because the gesture this serves is a menu
/// and not a switch: a person choosing where to work picks the tree from
/// yesterday as readily as a fresh one, and two conversations in one tree is an
/// ordinary thing to want — the second is how somebody carries on work an agent
/// left half-done.
///
/// A path is all the second answer carries. Which paths are trees of this
/// project is not the caller's to assert, and [`find`] is where that is settled.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Choice {
    /// The word `new`: make one.
    New(New),
    /// One that is already there.
    Existing { path: String },
}

/// The only spelling [`Choice::New`] accepts, so that a typo is a refusal
/// rather than a silently different answer.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum New {
    New,
}

/// One tree, as the window draws it and as a person decides about it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Where it is. Also its identity: nothing else about a detached tree is
    /// stable, and this is what every operation below takes.
    pub path: String,
    /// The branch the work was aimed at: what `HEAD` was on when the tree was
    /// made, or the branch the tree is on when somebody made it themselves.
    ///
    /// It is not a promise to merge there — nothing here merges — but a person
    /// choosing between three trees in a menu is choosing against this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The commit it started from, which answers the same question when the
    /// project itself was detached.
    pub base_commit: String,
    /// Where the tree is now.
    ///
    /// Equal to [`Self::base_commit`] until something is committed in it, which
    /// is how a menu can say *empty* without reading the tree's history. The
    /// two are separate fields because they answer different questions and a
    /// single one would have made "what was chosen" and "what happened since"
    /// the same string.
    pub head: String,
}

/// What this application writes beside a tree it made, so that the tree can
/// still say where it came from tomorrow.
///
/// Git records only what a tree is checked out at, and ours are detached on
/// purpose — so the branch the work was aimed at is a fact only the moment of
/// creation knows. It goes in the tree's own administrative directory rather
/// than in this installation's configuration: git removes that directory when
/// the tree is removed, which makes the note's lifetime the tree's without
/// anything having to remember to clean it up.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Origin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base: Option<String>,
    base_commit: String,
}

/// The name of that note, inside the tree's administrative directory.
const ORIGIN_FILE: &str = "sync-origin.json";

/// The location every tree is made under.
///
/// # Errors
///
/// [`ProjectError`] when the machine cannot say where configuration or
/// application data lives.
#[tauri::command(async)]
pub fn worktree_location<R: Runtime>(app: AppHandle<R>) -> Result<String, ProjectError> {
    location(&app).map(|path| path.to_string_lossy().into_owned())
}

/// Choose where trees are made, or go back to the default with `None`.
///
/// The directory is created here rather than at the first tree: somebody
/// setting this is standing in front of the answer, and a path that cannot be
/// made is worth refusing now instead of at three in the morning.
///
/// # Errors
///
/// [`ProjectError`] when the path cannot be created, or when configuration
/// cannot be written.
#[tauri::command(async)]
pub fn worktree_set_location<R: Runtime>(
    app: AppHandle<R>,
    path: Option<String>,
) -> Result<String, ProjectError> {
    let chosen = match path {
        Some(path) if !path.trim().is_empty() => Some(path.trim().to_owned()),
        _ => None,
    };
    if let Some(path) = chosen.as_deref() {
        std::fs::create_dir_all(path).map_err(|error| {
            ProjectError::new(
                "worktree_location",
                format!("could not use {path}: {error}"),
            )
        })?;
    }
    write_configuration(
        &configuration_file(&app, FILE)?,
        &Configuration { location: chosen },
    )?;
    worktree_location(app)
}

/// Every tree this installation made for a project, freshest git answer each
/// time.
///
/// The main working tree is not one of them: git lists it first and it is the
/// project itself.
///
/// # Errors
///
/// [`ProjectError`] when git cannot be run, or when the folder is not a
/// repository.
#[tauri::command(async)]
pub fn worktree_list(project: String) -> Result<Vec<Worktree>, ProjectError> {
    list(&PathBuf::from(project))
}

/// The same list, for callers inside this application.
///
/// # Errors
///
/// [`ProjectError`] when git cannot be run, or when the folder is not a
/// repository.
pub fn list(project: &Path) -> Result<Vec<Worktree>, ProjectError> {
    // **Nothing is pruned here.** Git keeps listing a tree whose directory has
    // gone, and the obvious fix — prune before listing — makes reading a list
    // change the repository: a location on a disk that is not mounted right now
    // looks exactly like a tree somebody deleted, and pruning would throw away
    // the administrative half of every tree on it. So the ones whose directory
    // is absent are left out of the answer and left alone on disk.
    let output = run_git(project, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(ProjectError::new(
            "worktree_failed",
            git_message(&output, "could not list this repository's working trees."),
        ));
    }
    Ok(parse_list(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .filter(|tree| Path::new(&tree.path).is_dir())
        .map(with_origin)
        .collect())
}

/// One of this project's trees, by the path a caller named.
///
/// **The path is checked against git's own list rather than trusted.** Where
/// trees live is this installation's choice, and a caller that could name any
/// directory would have taken that choice away — a package could raise an agent
/// anywhere on the machine by calling it a working tree.
///
/// # Errors
///
/// [`ProjectError`] when the repository cannot be asked, or when it holds no
/// tree at that path.
pub fn find(project: &Path, path: &str) -> Result<Worktree, ProjectError> {
    // Canonical on both sides, because git answers canonically and a caller is
    // passing back a path that has been through the window and a JSON boundary.
    let wanted = std::fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .into_owned();
    list(project)?
        .into_iter()
        .find(|tree| tree.path == wanted)
        .ok_or_else(|| {
            ProjectError::new(
                "worktree_unknown",
                format!("this project has no working tree at {path}."),
            )
        })
}

/// Make a tree for one conversation, detached at the project's `HEAD`.
///
/// The directory is named by [`name_in`], never by anything a person typed. A
/// directory name is not a branch name — nobody has a convention for it, and it
/// has to be unique without anybody being asked. `key` is one of this
/// application's own minted keys, and it is what the name falls back to.
///
/// # Errors
///
/// [`ProjectError`] when the folder is not a repository, when it has no commit
/// to branch from, or when git refuses.
pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    project: &Path,
    key: &str,
) -> Result<Worktree, ProjectError> {
    create_at(&location(app)?, project, key)
}

/// The whole of making a tree, with the location already answered.
///
/// Split from [`create`] because everything above the location is git and a
/// directory, and a test that had to build an application handle to reach it
/// would be testing Tauri.
pub fn create_at(location: &Path, project: &Path, key: &str) -> Result<Worktree, ProjectError> {
    let base_commit = head_commit(project).ok_or_else(|| {
        ProjectError::new(
            "worktree_unavailable",
            "this project has no commit to make a working tree from. A repository with \
             nothing committed has no `HEAD`, and a tree made from nothing is not a copy \
             of anything.",
        )
    })?;
    // `None` is a project that is itself detached, which is a state and not a
    // failure — the commit above is what the tree is made from either way.
    let base = branch_at_head(project);

    // Made before a name is asked for, because asking is reading the directory:
    // a name is free when nothing of that name is in there, and a directory
    // that is not there yet answers that for every name at once.
    let parent = location.join(directory_for(project));
    std::fs::create_dir_all(&parent).map_err(|error| {
        ProjectError::new(
            "worktree_location",
            format!("could not use {}: {error}", parent.display()),
        )
    })?;
    let path = parent.join(name_in(&parent, key));

    let output = run_git(
        project,
        &[
            "worktree",
            "add",
            "--detach",
            &path.to_string_lossy(),
            &base_commit,
        ],
    )?;
    if !output.status.success() {
        return Err(ProjectError::new(
            "worktree_failed",
            git_message(&output, "git would not add a working tree."),
        ));
    }

    // Written before the tree is answered for, so that a tree which exists is a
    // tree that can say where it came from. Quiet on failure: a note that could
    // not be written costs a menu the branch it would have shown, and refusing
    // the whole tree over it would be the larger loss.
    write_origin(
        &path,
        &Origin {
            base: base.clone(),
            base_commit: base_commit.clone(),
        },
    );

    Ok(Worktree {
        // Canonical, because git answers `worktree list` canonically and the
        // two have to be the same string: on macOS a tree made under `/var` is
        // listed under `/private/var`, and a row holding one spelling while the
        // list holds the other is a tree nothing can match to its conversation.
        path: path
            .canonicalize()
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned(),
        base,
        head: base_commit.clone(),
        base_commit,
    })
}

/// Take the work: give the tree's commit the name a person chose.
///
/// The name is checked by git rather than by a rule written here — git owns
/// what a reference may be called, and a second opinion would differ from it
/// eventually.
///
/// A tree with nothing committed in it is refused. The branch would point at
/// the commit the tree was made from, which is a name for work that was never
/// done.
///
/// # Errors
///
/// [`ProjectError`] when the name is not one git accepts, when a branch of that
/// name is already there, or when nothing was committed in the tree.
#[tauri::command(async)]
pub fn worktree_adopt(project: String, path: String, branch: String) -> Result<(), ProjectError> {
    let project = PathBuf::from(project);
    let branch = branch.trim().to_owned();

    let reference = format!("refs/heads/{branch}");
    let checked = run_git(&project, &["check-ref-format", &reference])?;
    if !checked.status.success() {
        return Err(ProjectError::new(
            "worktree_branch_name",
            format!("`{branch}` is not a name git will take for a branch."),
        ));
    }

    // Through the list, so that this is a tree of this project and so that both
    // ends of the comparison below come from the same answer.
    let tree = find(&project, &path)?;
    // **Against where the tree started, not against the project.** The project
    // moves on its own — somebody commits while the agent works — so comparing
    // with its `HEAD` would call an untouched tree full whenever the project had
    // advanced, and call a tree that did work empty whenever the two happened to
    // meet at the same commit.
    if tree.head == tree.base_commit {
        return Err(ProjectError::new(
            "worktree_empty",
            "nothing was committed in that working tree, so there is no work to name.",
        ));
    }

    let output = run_git(&project, &["branch", &branch, &tree.head])?;
    if !output.status.success() {
        return Err(ProjectError::new(
            "worktree_failed",
            git_message(&output, "git would not create that branch."),
        ));
    }
    Ok(())
}

/// Throw the tree away.
///
/// `--force` because the tree is disposable by construction: an agent left
/// files half-written in it and refusing to remove it until they are tidy would
/// be asking somebody to curate what they have already decided to discard.
///
/// **Commits made in the tree go with it** unless [`worktree_adopt`] named them
/// first. Whatever offers this says so before it is called.
///
/// # Errors
///
/// [`ProjectError`] when git refuses to remove it.
#[tauri::command(async)]
pub fn worktree_discard(project: String, path: String) -> Result<(), ProjectError> {
    let project = PathBuf::from(project);
    // The same check `worktree_adopt` makes, and for the same reason: one rule
    // about which paths are this project's, in one place, with one refusal.
    let tree = find(&project, &path)?;
    let output = run_git(&project, &["worktree", "remove", "--force", &tree.path])?;
    if !output.status.success() {
        return Err(ProjectError::new(
            "worktree_failed",
            git_message(&output, "git would not remove that working tree."),
        ));
    }
    let _ = run_git(&project, &["worktree", "prune"]);
    Ok(())
}

/// The tree a [`Choice`] names: made now, or found among the ones that exist.
///
/// `key` is spent only when a tree is made, so choosing an existing one costs
/// nothing.
///
/// # Errors
///
/// [`ProjectError`] when a new tree cannot be made, or when the named one is
/// not a tree of this project.
pub fn resolve<R: Runtime>(
    app: &AppHandle<R>,
    project: &Path,
    key: &str,
    choice: Choice,
) -> Result<Worktree, ProjectError> {
    match choice {
        Choice::New(New::New) => create(app, project, key),
        Choice::Existing { path } => find(project, &path),
    }
}

/// This installation's choice, or the default under application data.
pub(crate) fn location<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, ProjectError> {
    let configured = configuration_file(app, FILE)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Configuration>(&text).ok())
        .and_then(|configuration| configuration.location);
    if let Some(path) = configured {
        return Ok(PathBuf::from(path));
    }
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(DEFAULT_DIRECTORY))
        .map_err(|error| {
            ProjectError::new(
                "worktree_location",
                format!("could not resolve where application data lives: {error}"),
            )
        })
}

/// What this installation was told, if anything.
#[derive(Debug, Default, Deserialize, Serialize)]
struct Configuration {
    /// `None` is the default location, and staying `None` is what makes the
    /// default follow the machine rather than freezing the path it had the day
    /// somebody opened the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    location: Option<String>,
}

/// Where a tree's administrative directory is: `.git/worktrees/<name>` in the
/// repository it belongs to, which git answers for.
fn administrative_directory(tree: &Path) -> Option<PathBuf> {
    let output = run_git(tree, &["rev-parse", "--absolute-git-dir"]).ok()?;
    if !output.status.success() {
        return None;
    }
    let directory = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!directory.is_empty()).then(|| PathBuf::from(directory))
}

fn write_origin(tree: &Path, origin: &Origin) {
    let Some(directory) = administrative_directory(tree) else {
        return;
    };
    if let Ok(text) = serde_json::to_string(origin) {
        let _ = std::fs::write(directory.join(ORIGIN_FILE), text);
    }
}

/// The tree, with what it was made from when this application made it.
///
/// A tree somebody made themselves has no note and keeps what git said about
/// it: the branch it is checked out on, and its own commit as the start. That
/// is not a lesser answer — a tree a person made *is* aimed at the branch it is
/// on — and it is what lets their trees sit in the same menu as ours.
fn with_origin(tree: Worktree) -> Worktree {
    let Some(origin) = administrative_directory(Path::new(&tree.path))
        .and_then(|directory| std::fs::read_to_string(directory.join(ORIGIN_FILE)).ok())
        .and_then(|text| serde_json::from_str::<Origin>(&text).ok())
    else {
        return tree;
    };
    Worktree {
        base: origin.base,
        base_commit: origin.base_commit,
        ..tree
    }
}

/// The commit a working tree is on, or `None` when there is not one — an empty
/// repository, or a path that is not a working tree at all.
fn head_commit(tree: &Path) -> Option<String> {
    let output = run_git(tree, &["rev-parse", "HEAD"]).ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!commit.is_empty()).then_some(commit)
}

/// The branch `HEAD` names, when it names one. A detached `HEAD` answers
/// `None`, which git reports by exiting non-zero and saying nothing.
fn branch_at_head(tree: &Path) -> Option<String> {
    let output = run_git(tree, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!branch.is_empty()).then_some(branch)
}

/// The directory a project's trees are made in, under the location.
///
/// The folder's name so that a person can see whose trees these are, and a hash
/// of the whole path so that two projects called `sync` are two directories.
/// The hash is written here rather than taken from the standard library:
/// `DefaultHasher` does not promise the same number across releases of the
/// compiler, and a directory that moves when the toolchain is upgraded would
/// strand every tree that was in the old one.
fn directory_for(project: &Path) -> String {
    let name = project
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("project");
    format!("{name}-{:016x}", fingerprint(project))
}

/// What one tree is called, inside the directory its project's trees live in.
///
/// Three words, because a tree has to be told apart from the tree beside it and
/// nothing else about it can do that: two made from `main` this afternoon carry
/// the same branch, the same commit and the same emptiness, so a menu offering
/// them has the name and nothing more. A key — `s4` — is unique and says
/// nothing, which makes it a fine identity and a poor answer to *which one*.
///
/// **The name is the directory, not a label kept beside it.** A word shown in
/// the window while the path stayed keyed would be two names for one tree: the
/// one somebody chose in a menu, and the one git lists, the settings show and
/// the agent's shell is standing in. One name is worth the collisions.
///
/// And they do collide — the lists are finite — so a name already taken here is
/// dropped and another asked for. The key ends it, because a tree that could
/// not be made is a worse outcome than a tree called something unpronounceable.
fn name_in(parent: &Path, key: &str) -> String {
    (0..NAMES_ASKED_FOR)
        .filter_map(|_| petname::petname(3, "-"))
        .find(|name| !parent.join(name).exists())
        .unwrap_or_else(|| key.to_owned())
}

/// How many names are asked for before the key is used instead.
const NAMES_ASKED_FOR: u8 = 8;

/// FNV-1a over the path's bytes. Not a security property: this only has to
/// separate two folders with the same name and be the same number tomorrow.
fn fingerprint(path: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What `git worktree list --porcelain` said, without the main tree.
///
/// The format is one paragraph per tree, blank-line separated: `worktree
/// <path>`, `HEAD <commit>`, and then either `branch <ref>` or `detached`. The
/// first paragraph is the repository's own working tree, which is the project
/// and not something this application made.
fn parse_list(output: &str) -> Vec<Worktree> {
    let mut trees = Vec::new();
    let mut path: Option<String> = None;
    let mut commit = String::new();
    let mut branch: Option<String> = None;

    let mut finish =
        |path: &mut Option<String>, commit: &mut String, branch: &mut Option<String>| {
            if let Some(path) = path.take() {
                let head = std::mem::take(commit);
                trees.push(Worktree {
                    path,
                    base: branch.take(),
                    // All git can say by itself: a tree it knows nothing else
                    // about started where it stands. `with_origin` replaces
                    // both for a tree this application made.
                    base_commit: head.clone(),
                    head,
                });
            }
            commit.clear();
            *branch = None;
        };

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            finish(&mut path, &mut commit, &mut branch);
            path = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            commit = rest.to_owned();
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            branch = Some(rest.to_owned());
        }
    }
    finish(&mut path, &mut commit, &mut branch);

    // The main tree, dropped after parsing rather than skipped during it: git
    // states the order, and a parser that also encoded it would be two places
    // knowing the same thing.
    if !trees.is_empty() {
        trees.remove(0);
    }
    trees
}

#[cfg(test)]
mod tests {
    use super::{Worktree, directory_for, name_in, parse_list};
    use std::path::Path;

    #[test]
    fn two_projects_with_one_name_get_two_directories() {
        let first = directory_for(Path::new("/Users/someone/work/sync"));
        let second = directory_for(Path::new("/Volumes/spare/sync"));
        assert_ne!(first, second);
        assert!(first.starts_with("sync-"), "{first}");
        assert!(second.starts_with("sync-"), "{second}");
    }

    /// The name a tree gets is free at the moment it is given.
    ///
    /// Nothing else enforces this: git would refuse to add a second tree at a
    /// path that is taken, so a name handed out twice is a tree that could not
    /// be made rather than a tree with a confusing name.
    #[test]
    fn a_name_is_never_one_already_standing_in_the_directory() {
        let parent = tempfile::tempdir().expect("a temporary folder");
        let first = name_in(parent.path(), "s1");
        std::fs::create_dir(parent.path().join(&first)).expect("a tree of that name");
        let second = name_in(parent.path(), "s1");
        assert_ne!(first, second, "{first} was taken");
    }

    /// The key is the last resort and not the ordinary answer.
    ///
    /// Exhausting the real word lists in a test is not possible, so what is
    /// checked is the other end: a directory where the key is the one name
    /// taken still gets words, because the words are what a person reads.
    #[test]
    fn a_tree_is_called_by_words_rather_than_by_the_key_it_was_made_for() {
        let parent = tempfile::tempdir().expect("a temporary folder");
        // A file rather than a directory, because `exists` is what the minting
        // asks and a name is taken either way.
        std::fs::write(parent.path().join("s1"), "").expect("the key, taken too");
        let name = name_in(parent.path(), "s1");
        assert_eq!(name.split('-').count(), 3, "a minted name: {name}");
    }

    #[test]
    fn the_directory_is_the_same_one_next_time() {
        assert_eq!(
            directory_for(Path::new("/Users/someone/work/sync")),
            directory_for(Path::new("/Users/someone/work/sync")),
        );
    }

    #[test]
    fn the_projects_own_tree_is_not_one_of_the_trees_it_made() {
        let listed = "worktree /Users/someone/work/sync\n\
                      HEAD 1111111111111111111111111111111111111111\n\
                      branch refs/heads/main\n\
                      \n\
                      worktree /Users/someone/Library/sync/worktrees/sync-abc/s-1\n\
                      HEAD 2222222222222222222222222222222222222222\n\
                      detached\n";
        assert_eq!(
            parse_list(listed),
            vec![Worktree {
                path: "/Users/someone/Library/sync/worktrees/sync-abc/s-1".to_owned(),
                base: None,
                base_commit: "2222222222222222222222222222222222222222".to_owned(),
                head: "2222222222222222222222222222222222222222".to_owned(),
            }]
        );
    }

    #[test]
    fn a_repository_with_only_its_own_tree_lists_nothing() {
        let listed = "worktree /Users/someone/work/sync\n\
                      HEAD 1111111111111111111111111111111111111111\n\
                      branch refs/heads/main\n";
        assert!(parse_list(listed).is_empty());
    }
}
