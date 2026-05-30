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

use reify_audit::RealGitOps;
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
