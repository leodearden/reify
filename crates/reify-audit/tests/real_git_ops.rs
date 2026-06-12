//! Real-git-ops integration tests.
//!
//! These tests build a real temporary git repository using `tempfile::tempdir()`
//! and `std::process::Command("git")` to validate that `RealGitOps` shells out
//! the correct git command with the correct argument form.  They exist because a
//! mock cannot catch a wrong range string (e.g. `^1..^2` instead of `^1..`) —
//! the exact production bug class this task fixes: RealGitOps was returning empty
//! while MockGitOps tests stayed green.
//!
//! Run with: `cargo test -p reify-audit real_git_ops`

use reify_audit::{GitOps, RealGitOps};
use std::process::Command;
use tempfile::TempDir;

// -----------------------------------------------------------------------
// Shared real-repo helpers
// -----------------------------------------------------------------------

/// Initialise a bare git repo in `dir` with identity + gpgsign disabled.
fn git_init(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git command failed to spawn");
        assert!(status.success(), "git {:?} exited {:?}", args, status.code());
    };
    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
}

/// Write `content` to `path` inside `dir`.
fn write_file(dir: &std::path::Path, path: &str, content: &str) {
    let full = dir.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Stage + commit all tracked changes in `dir`.
fn git_commit(dir: &std::path::Path, msg: &str) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git command failed to spawn");
        assert!(status.success(), "git {:?} exited {:?}", args, status.code());
    };
    run(&["add", "."]);
    run(&["commit", "-m", msg]);
}

/// Return the SHA of HEAD in `dir`.
fn rev_parse_head(dir: &std::path::Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD");
    assert!(out.status.success());
    String::from_utf8(out.stdout).expect("utf8").trim().to_string()
}

// -----------------------------------------------------------------------
// Step 1: diff_added_lines_in_commit against a real --no-ff merge commit
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::diff_added_lines_in_commit` returns the correct added
/// lines when given a real 2-parent merge commit.
///
/// Setup:
///   - commit A: `foo.rs` with two lines
///   - branch `feature`: append one line `    // TODO(impl pending)` → commit B
///   - merge B into main with `--no-ff` → merge commit M (2 parents)
///
/// Assertion: `diff_added_lines_in_commit(M, "foo.rs")` must return exactly
/// `vec![(3, "    // TODO(impl pending)")]` — the correct new-side line number
/// and the correct content (leading `+` stripped).
///
/// This test catches a wrong range string (`^1..^2`, `^..<commit>`, etc.) that
/// MockGitOps cannot detect because the mock returns whatever you put in.
#[test]
fn diff_added_lines_in_commit_real_merge() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // commit A — base file on main (2 lines)
    write_file(root, "foo.rs", "fn a() {}\nfn b() {}\n");
    git_commit(root, "base commit A");

    // branch feature — append one stub line
    let run_branch = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git spawn");
        assert!(status.success(), "git {:?} failed", args);
    };

    run_branch(&["checkout", "-b", "feature"]);
    // Append the stub line (line 3)
    write_file(root, "foo.rs", "fn a() {}\nfn b() {}\n    // TODO(impl pending)\n");
    git_commit(root, "feature: add stub");

    // Back to main and --no-ff merge
    run_branch(&["checkout", "main"]);
    run_branch(&["merge", "--no-ff", "-m", "Merge task/feature into main", "feature"]);

    let merge_sha = rev_parse_head(root);

    // --- the assertion that currently fails (RED: method does not yet exist) ---
    let git = RealGitOps::new(root);
    let added = git.diff_added_lines_in_commit(&merge_sha, "foo.rs");

    assert_eq!(
        added,
        vec![(3usize, "    // TODO(impl pending)".to_string())],
        "diff_added_lines_in_commit({}, foo.rs) should return exactly the appended line \
         at new-side line 3; got: {:?}",
        merge_sha,
        added,
    );
}

// -----------------------------------------------------------------------
// Step 5: file_lines_on against a real commit
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::file_lines_on` returns all lines of a file numbered
/// from 1, with no spurious trailing empty entry from a final newline.
///
/// Setup: single commit with `foo.rs` containing exactly three lines.
///
/// Also asserts that a missing path returns empty (fail-safe).
#[test]
fn file_lines_on_real_commit() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    write_file(root, "foo.rs", "fn a() {}\n    // stub\nfn b() {}\n");
    git_commit(root, "initial commit");

    let git = RealGitOps::new(root);

    let lines = git.file_lines_on("HEAD", "foo.rs");
    assert_eq!(
        lines,
        vec![
            (1usize, "fn a() {}".to_string()),
            (2, "    // stub".to_string()),
            (3, "fn b() {}".to_string()),
        ],
        "file_lines_on(HEAD, foo.rs) must return all 3 lines numbered from 1, \
         no trailing empty entry; got: {:?}",
        lines,
    );

    // Missing path must return empty (fail-safe)
    let missing = git.file_lines_on("HEAD", "does_not_exist.rs");
    assert!(
        missing.is_empty(),
        "file_lines_on for a missing path must return empty; got: {:?}",
        missing,
    );
}

// -----------------------------------------------------------------------
// ls_files: tracked-path enumeration (PTODO structural-lane git seam)
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::ls_files` returns exactly the set of tracked,
/// root-relative paths — including nested paths — and excludes an
/// untracked/uncommitted file.
///
/// Setup: commit `a.rs`, `dir/b.sh`, `crates/x/c.rs`; then write (but do NOT
/// `git add`/commit) `untracked.rs`.
///
/// Assertion: the returned set equals the three committed paths, and the
/// untracked file is absent. Order is not asserted (git's ls-files order is
/// not part of the contract); the detector sorts before use.
#[test]
fn ls_files_lists_tracked_paths_only() {
    use std::collections::BTreeSet;

    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    write_file(root, "a.rs", "fn a() {}\n");
    write_file(root, "dir/b.sh", "echo hi\n");
    write_file(root, "crates/x/c.rs", "fn c() {}\n");
    git_commit(root, "commit three tracked files");

    // An untracked file that must NOT appear in ls_files output.
    write_file(root, "untracked.rs", "fn untracked() {}\n");

    let git = RealGitOps::new(root);
    let listed: BTreeSet<String> = git.ls_files().into_iter().collect();

    let expected: BTreeSet<String> = ["a.rs", "dir/b.sh", "crates/x/c.rs"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        listed, expected,
        "ls_files must return exactly the tracked root-relative paths; got: {:?}",
        listed,
    );
    assert!(
        !listed.contains("untracked.rs"),
        "ls_files must not list an untracked/uncommitted file; got: {:?}",
        listed,
    );
}

// -----------------------------------------------------------------------
// last_commit_for_path: git history check for ζ inverse lane (task 4558)
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::last_commit_for_path` returns `Some(GitCommit)` whose
/// `sha` equals the most-recent commit touching the path (including the deletion
/// commit), and `None` for a path that was never committed.
///
/// This is a real-git-repo test because a wrong argument form (e.g. omitting
/// `--`) would shell out correctly but `MockGitOps` cannot catch it.
///
/// Setup:
///   - commit 1: add `deleted.rs`
///   - commit 2: `git rm deleted.rs` + commit (the deletion commit)
///
/// Assertions:
///   - `last_commit_for_path("deleted.rs")` → `Some(c)` with `c.sha == HEAD sha`
///   - `last_commit_for_path("never.rs")`   → `None`
#[test]
fn last_commit_for_path_real_repo() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // Commit 1: add deleted.rs
    write_file(root, "deleted.rs", "fn deleted() {}\n");
    git_commit(root, "add deleted.rs");

    // Commit 2: remove deleted.rs
    let rm_status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rm", "deleted.rs"])
        .status()
        .expect("git rm spawn");
    assert!(rm_status.success(), "git rm failed");
    git_commit(root, "delete deleted.rs");

    let deletion_sha = rev_parse_head(root);

    let git = RealGitOps::new(root);

    // deleted.rs has history — should return Some with sha == deletion commit
    let result = git.last_commit_for_path("deleted.rs");
    assert!(
        result.is_some(),
        "last_commit_for_path(\"deleted.rs\") must return Some; got None"
    );
    let commit = result.unwrap();
    assert_eq!(
        commit.sha, deletion_sha,
        "sha must equal the deletion commit HEAD; got {} expected {}",
        commit.sha, deletion_sha,
    );
    assert!(
        !commit.subject.is_empty(),
        "subject must be non-empty; got {:?}",
        commit.subject,
    );

    // never.rs was never committed — should return None
    let none = git.last_commit_for_path("never.rs");
    assert!(
        none.is_none(),
        "last_commit_for_path(\"never.rs\") must return None; got {:?}",
        none,
    );
}

// -----------------------------------------------------------------------
// Trailing-newline invariant: both forms yield the same logical line count
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::file_lines_on` handles a file with **no trailing
/// newline** identically to a file that ends with `\n`.
///
/// The rustdoc on `file_lines_on` states that `str::lines()` does not produce
/// a spurious empty entry for either form.  [`file_lines_on_real_commit`]
/// verifies the trailing-newline case; this test covers the complementary
/// no-trailing-newline case so the doc-claimed invariant is fully exercised.
///
/// Input:  `"a\nb"` (two logical lines, no final `\n`)
/// Expected: `vec![(1, "a"), (2, "b")]` — same logical line count as `"a\nb\n"`.
#[test]
fn file_lines_on_no_trailing_newline() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // Write a two-line file WITHOUT a trailing newline.
    write_file(root, "no_newline.rs", "a\nb");
    git_commit(root, "no-trailing-newline commit");

    let git = RealGitOps::new(root);

    let lines = git.file_lines_on("HEAD", "no_newline.rs");
    assert_eq!(
        lines,
        vec![
            (1usize, "a".to_string()),
            (2, "b".to_string()),
        ],
        "file_lines_on for a file WITHOUT a trailing newline must return 2 lines, \
         same logical count as if a trailing newline were present; got: {:?}",
        lines,
    );
}
