#![allow(clippy::expect_used, clippy::unwrap_used)]

//! What a disposable working tree does to the repository it came from.
//!
//! Driven against a real repository and real `git`, because every claim this
//! makes is a claim about git's behaviour rather than about our parsing: that
//! adding a tree detached leaves no branch behind, that naming the work creates
//! exactly the branch a person asked for, and that throwing a tree away takes
//! its commits with it. A test with a stubbed git would agree with whatever the
//! stub was written to believe.
//!
//! Skipped, loudly, on a machine with no git. The suite runs where the engine
//! does not, and a red build for a missing tool would say nothing about this
//! code.

use std::path::Path;
use std::process::Command;

use sync_lib::worktree::{create_at, find, worktree_adopt, worktree_discard, worktree_list};

fn git_is_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository with one commit on `main`, which is the least a tree can be
/// made from.
fn repository(at: &Path) {
    git(at, &["init", "--initial-branch=main"]);
    git(at, &["config", "user.email", "tests@example.invalid"]);
    git(at, &["config", "user.name", "Tests"]);
    std::fs::write(at.join("README.md"), "first\n").expect("a file to commit");
    git(at, &["add", "README.md"]);
    git(at, &["commit", "-m", "first"]);
}

/// A commit made inside the tree, which is what "the work" means here.
fn commit_in(tree: &Path, text: &str) -> String {
    std::fs::write(tree.join("worked.md"), text).expect("a file to commit");
    git(tree, &["add", "worked.md"]);
    git(tree, &["commit", "-m", "work done overnight"]);
    git(tree, &["rev-parse", "HEAD"])
}

#[test]
fn a_tree_is_made_without_naming_a_branch() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());

    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    assert!(Path::new(&tree.path).is_dir(), "the tree is on disk");
    assert_eq!(
        tree.base.as_deref(),
        Some("main"),
        "the branch it was aimed at is remembered, because nothing else says where the \
         work was going"
    );
    assert_eq!(
        git(
            project.path(),
            &["branch", "--list", "--format=%(refname:short)"]
        ),
        "main",
        "and the repository gained no reference of ours — branch naming belongs to \
         whoever owns the repository"
    );
    assert_eq!(
        git(
            Path::new(&tree.path),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "HEAD",
        "the tree is detached, which is what leaves the naming to a person"
    );
}

#[test]
fn the_projects_own_tree_is_not_listed_as_one_it_made() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());

    assert!(
        worktree_list(project.path().to_string_lossy().into_owned())
            .expect("a list")
            .is_empty(),
        "a project nobody ordered work in has no trees"
    );

    let made = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    let listed = worktree_list(project.path().to_string_lossy().into_owned()).expect("a list");

    assert_eq!(listed.len(), 1, "one tree, and not the project itself");
    assert_eq!(listed[0].path, made.path);
}

#[test]
fn taking_the_work_creates_the_branch_a_person_asked_for() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    let commit = commit_in(Path::new(&tree.path), "done\n");

    worktree_adopt(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
        // A spelling this application would never have invented, which is the
        // point: the convention is the repository owner's.
        "feature/NIK-42_overnight".to_owned(),
    )
    .expect("the branch is created");

    assert_eq!(
        git(project.path(), &["rev-parse", "feature/NIK-42_overnight"]),
        commit,
        "the branch names the commit that was made in the tree"
    );
    assert_eq!(
        git(
            Path::new(&tree.path),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "HEAD",
        "and the tree stays detached, so the branch is free for whoever named it"
    );
}

#[test]
fn a_tree_with_nothing_committed_has_no_work_to_name() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    let refused = worktree_adopt(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
        "overnight".to_owned(),
    )
    .expect_err("a branch at the commit it started from is a name for nothing");

    assert_eq!(refused.kind, "worktree_empty");
    assert_eq!(
        git(
            project.path(),
            &["branch", "--list", "--format=%(refname:short)"]
        ),
        "main",
        "and nothing was created on the way to refusing"
    );
}

#[test]
fn a_name_git_will_not_take_is_refused_before_anything_is_written() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    commit_in(Path::new(&tree.path), "done\n");

    let refused = worktree_adopt(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
        "not a branch..name".to_owned(),
    )
    .expect_err("git owns what a reference may be called");

    assert_eq!(refused.kind, "worktree_branch_name");
}

#[test]
fn throwing_a_tree_away_takes_the_work_that_was_not_named() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    commit_in(Path::new(&tree.path), "done\n");

    worktree_discard(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
    )
    .expect("the tree is removed");

    assert!(
        !Path::new(&tree.path).exists(),
        "the directory is gone, uncommitted files and all"
    );
    assert!(
        worktree_list(project.path().to_string_lossy().into_owned())
            .expect("a list")
            .is_empty(),
        "and git no longer holds it"
    );
    assert_eq!(
        git(
            project.path(),
            &["branch", "--list", "--format=%(refname:short)"]
        ),
        "main",
        "the work left no reference behind, which is what discarding it means"
    );
}

#[test]
fn work_that_was_named_survives_the_tree_being_thrown_away() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    let commit = commit_in(Path::new(&tree.path), "done\n");

    worktree_adopt(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
        "overnight".to_owned(),
    )
    .expect("the branch is created");
    worktree_discard(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
    )
    .expect("the tree is removed");

    assert_eq!(
        git(project.path(), &["rev-parse", "overnight"]),
        commit,
        "taking the work is what makes it outlive the place it was done in"
    );
}

/// Choosing a tree that already exists, which is what a menu of them is for.
///
/// The path is the whole of what a caller passes, so this is also the test that
/// says which paths are answers: git's list, and nothing else.
#[test]
fn a_tree_of_this_project_is_found_by_the_path_it_was_made_at() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let made = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    let found = find(project.path(), &made.path).expect("the tree this project holds");

    assert_eq!(found, made, "the same tree, base and all");
}

/// And a path that is not one of them is refused rather than used.
///
/// This is what keeps the choice of location with the installation: a caller
/// that could name any directory would raise an agent wherever it liked by
/// calling that directory a working tree.
#[test]
fn a_directory_that_is_not_this_projects_tree_is_refused() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    repository(elsewhere.path());
    let another = create_at(trees.path(), elsewhere.path(), "s1").expect("a working tree");

    let plain = find(project.path(), &elsewhere.path().to_string_lossy())
        .expect_err("an ordinary folder is not a tree of this project");
    assert_eq!(plain.kind, "worktree_unknown");

    let stranger =
        find(project.path(), &another.path).expect_err("and neither is another project's tree");
    assert_eq!(stranger.kind, "worktree_unknown");
}

/// What a menu of trees is drawn from, after a restart that forgot everything.
///
/// The branch a tree was aimed at is not something git records — ours are
/// detached on purpose — so it is written beside the tree when it is made and
/// read back here. Without it every tree in the menu would read the same:
/// a path, and nothing about where its work was going.
#[test]
fn a_listed_tree_still_says_what_it_was_made_from_and_whether_it_has_work() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let made = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    let before = worktree_list(project.path().to_string_lossy().into_owned()).expect("a list");
    assert_eq!(before[0].base.as_deref(), Some("main"), "aimed at main");
    assert_eq!(
        before[0].head, before[0].base_commit,
        "and nothing done in it yet, which is how a menu says empty"
    );

    let commit = commit_in(Path::new(&made.path), "done\n");
    let after = worktree_list(project.path().to_string_lossy().into_owned()).expect("a list");

    assert_eq!(
        after[0].base.as_deref(),
        Some("main"),
        "still aimed at main"
    );
    assert_eq!(
        after[0].base_commit, before[0].base_commit,
        "from the same start"
    );
    assert_eq!(after[0].head, commit, "and now holding work");
}

/// The tree is empty or not by what happened *in it*, never by where the
/// project has got to.
///
/// The project moves on its own: somebody commits while an agent works. Judging
/// emptiness against the project's `HEAD` calls an untouched tree full as soon
/// as the project advances — a branch named after work that was never done.
#[test]
fn a_tree_is_empty_even_when_the_project_has_moved_on_without_it() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    // The owner carries on in their own tree while nothing happens in this one.
    std::fs::write(project.path().join("meanwhile.md"), "typing\n").expect("a file to commit");
    git(project.path(), &["add", "meanwhile.md"]);
    git(
        project.path(),
        &["commit", "-m", "the owner's own afternoon"],
    );

    let refused = worktree_adopt(
        project.path().to_string_lossy().into_owned(),
        tree.path.clone(),
        "overnight".to_owned(),
    )
    .expect_err("nothing was done in the tree, whatever the project did");

    assert_eq!(refused.kind, "worktree_empty");
}

/// A tree whose directory is not there is left out of the list — and left alone
/// in the repository.
///
/// Pruning before listing would be the obvious alternative and is the reason
/// this test exists: a location on a disk that is not mounted looks exactly like
/// a tree somebody deleted, and pruning would throw away the administrative half
/// of every tree on it. Reading a list must not change the repository.
#[test]
fn a_tree_whose_directory_is_gone_is_left_out_and_left_alone() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());
    let tree = create_at(trees.path(), project.path(), "s1").expect("a working tree");

    std::fs::remove_dir_all(&tree.path).expect("the directory goes, as an unmounted disk would");

    assert!(
        worktree_list(project.path().to_string_lossy().into_owned())
            .expect("a list")
            .is_empty(),
        "a directory that is not there is not offered"
    );
    assert!(
        git(project.path(), &["worktree", "list", "--porcelain"]).contains(&tree.path),
        "and git still holds it: reading a list did not prune anything"
    );
}

/// Two trees of one project, and the only thing that tells them apart.
///
/// Both are made from `main`, at the same commit, with nothing committed in
/// either — which is every fact a menu has about them apart from the name. So
/// the names have to differ, and they have to be the same names git lists:
/// a directory keyed by the conversation while the window showed a word would
/// leave a person holding two spellings of one tree.
#[test]
fn two_trees_of_one_project_are_told_apart_by_their_names() {
    if !git_is_installed() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let project = tempfile::tempdir().expect("a temporary folder");
    let trees = tempfile::tempdir().expect("a temporary folder");
    repository(project.path());

    // The same key for both, which is what a caller does when the two trees
    // belong to one conversation — and what used to be a collision.
    let first = create_at(trees.path(), project.path(), "s1").expect("a working tree");
    let second = create_at(trees.path(), project.path(), "s1").expect("a second working tree");

    assert_ne!(first.path, second.path, "two trees, two directories");
    for tree in [&first, &second] {
        let name = Path::new(&tree.path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("a directory name");
        assert_eq!(
            name.split('-').count(),
            3,
            "a name somebody can say out loud rather than a key: {name}"
        );
        assert!(
            git(project.path(), &["worktree", "list", "--porcelain"]).contains(name),
            "and it is the name git lists, not a second one kept beside it: {name}"
        );
    }
}
