//! Real-git-ops integration tests.
//!
//! These tests build a real temporary git repository using `tempfile::tempdir()`
//! and `common::git_env::git_cmd` — the shared `reify_audit::git_env`
//! constructor, which yields a `git -C <dir>` with the repo-redirect
//! environment variables stripped — to validate that `RealGitOps` shells out
//! the correct git command with the correct argument form.  They exist because a
//! mock cannot catch a wrong range string (e.g. `^1..^2` instead of `^1..`) —
//! the exact production bug class this task fixes: RealGitOps was returning empty
//! while MockGitOps tests stayed green.
//!
//! Run with: `cargo test -p reify-audit real_git_ops`

use reify_audit::{GitOps, RealGitOps};
use tempfile::TempDir;

mod common;

// -----------------------------------------------------------------------
// Shared real-repo helpers
// -----------------------------------------------------------------------

/// Every test in this file — including the two injected-spawn-failure retry
/// tests, which build a real repo before injecting — calls `git_init` and
/// `git_commit`. So every test here is exposed to an ambient hook git
/// environment, where `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` are exported
/// into the whole process tree and override `-C <tempdir>`.
///
/// That measured fact is why the filter is empty: it re-runs EVERY test here
/// inside a child process that has the poison ambient, which is the real hook
/// condition rather than a simulation of it, and no test in the selection is
/// along for the ride. The helper's `REIFY_AUDIT_HOOK_ENV_REPLAY` guard stops
/// this test recursing when the child reaches it, and it is itself counted in
/// the selection (it passes trivially in the child, via that guard).
///
/// A caveat for whoever reads a failure here: an empty filter means a future
/// test added to this file is re-run too, and a failure of *that* test would
/// surface both in the clean parent run and again inside this one. The
/// helper's assertion message says so — always diagnose against the clean
/// parent run first.
///
/// The floor of 11 is today's selection (10 real tests + this one). It exists
/// because libtest exits 0 on a zero-match filter; with an empty filter that
/// cannot happen today, but the floor also catches a test being deleted or
/// moved out of this binary, which would silently shrink the proof.
#[test]
fn real_git_ops_helpers_survive_ambient_hook_git_env() {
    common::git_env::replay_self_under_hook_git_env(&[""], 11);
}

/// Run `git <args…>` against the repository at `dir` and assert it succeeded.
///
/// The single entry point for every repo-targeting git invocation in this
/// file, so the `reify_audit::git_env` sanitizing is applied in exactly one
/// place. Without it an ambient hook `GIT_INDEX_FILE`/`GIT_DIR` overrides
/// `-C <dir>` and the command silently operates on the parent repository.
fn git_run(dir: &std::path::Path, args: &[&str]) {
    let status = common::git_env::git_cmd(dir)
        .args(args)
        .status()
        .expect("git command failed to spawn");
    assert!(status.success(), "git {:?} exited {:?}", args, status.code());
}

/// Initialise a bare git repo in `dir` with identity + gpgsign disabled.
fn git_init(dir: &std::path::Path) {
    let run = |args: &[&str]| git_run(dir, args);
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
    let run = |args: &[&str]| git_run(dir, args);
    run(&["add", "."]);
    run(&["commit", "-m", msg]);
}

/// Return the SHA of HEAD in `dir`.
fn rev_parse_head(dir: &std::path::Path) -> String {
    let out = common::git_env::git_cmd(dir)
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
    // Folded onto the shared `git_run` helper rather than kept as a fourth
    // private copy of the same closure.
    let run_branch = |args: &[&str]| git_run(root, args);

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
    let rm_status = common::git_env::git_cmd(root)
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

/// Pin that `RealGitOps::rename_target_for_path` resolves a real rename — and
/// that it composes with `last_commit_for_path`, which is how the ζ inverse
/// lane reaches it (the lane already holds the commit that last touched the
/// cited path, and asks "did THAT commit rename it?").
///
/// This must be a real-git-repo test: a wrong argument form — omitting `-M`
/// (so git reports the rename as an unrelated `D`/`A` pair), omitting
/// `--format=` (so the commit header lines pollute the parse), or reading the
/// wrong TAB field — would shell out perfectly happily, and `MockGitOps`,
/// which returns whatever the test puts in, could never catch it.
///
/// Setup:
///   - commit 1: add `old.rs` and `doomed.rs`
///   - commit 2: `git mv old.rs sub/new.rs` (the rename commit)
///   - commit 3: `git rm doomed.rs` (a genuine delete, for the fail-safe pin)
///
/// Assertions:
///   - `last_commit_for_path("old.rs")` → `Some(c)` with `c.sha == rename sha`
///     (the two seam calls compose: the lane's existing call hands this one its
///     sha)
///   - `rename_target_for_path("old.rs", rename_sha)` → `Some("sub/new.rs")`
///   - fail-safe, all `None`:
///     (a) a genuine delete commit queried for its deleted path
///     (b) a bogus sha (`git show` exits non-zero with `fatal: bad object`)
///     (c) a path present in the rename commit that is not its rename SOURCE
///     (querying the rename TARGET must not match)
#[test]
fn rename_target_for_path_real_repo() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // Commit 1: add old.rs (to be renamed) and doomed.rs (to be deleted).
    write_file(root, "old.rs", "fn old() {}\n");
    write_file(root, "doomed.rs", "fn doomed() {}\n");
    git_commit(root, "add old.rs and doomed.rs");

    // Commit 2: rename old.rs → sub/new.rs. `git mv` requires the destination
    // directory to exist.
    std::fs::create_dir_all(root.join("sub")).expect("create sub/");
    git_run(root, &["mv", "old.rs", "sub/new.rs"]);
    git_commit(root, "rename old.rs to sub/new.rs");
    let rename_sha = rev_parse_head(root);

    // Commit 3: genuine delete of an unrelated file.
    git_run(root, &["rm", "doomed.rs"]);
    git_commit(root, "delete doomed.rs");
    let delete_sha = rev_parse_head(root);

    let git = RealGitOps::new(root);

    // The two seam calls compose: the commit the inverse lane already resolved
    // for the cited path IS the rename commit.
    let last = git.last_commit_for_path("old.rs");
    assert!(
        last.is_some(),
        "last_commit_for_path(\"old.rs\") must return Some after the rename; got None"
    );
    assert_eq!(
        last.as_ref().unwrap().sha,
        rename_sha,
        "last_commit_for_path(\"old.rs\") must resolve to the rename commit; got {:?} expected {}",
        last,
        rename_sha,
    );

    // Core assertion: the rename target is recovered from that commit.
    let target = git.rename_target_for_path("old.rs", &rename_sha);
    assert_eq!(
        target,
        Some("sub/new.rs".to_string()),
        "rename_target_for_path(\"old.rs\", <rename sha>) must return the new path; got {:?}",
        target,
    );

    // Fail-safe (a): a genuine delete commit has no `R` line → None, so the
    // inverse lane keeps emitting task-cites-deleted-path.
    let deleted = git.rename_target_for_path("doomed.rs", &delete_sha);
    assert!(
        deleted.is_none(),
        "a genuine delete must not resolve a rename target; got {:?}",
        deleted,
    );

    // Fail-safe (b): a bogus sha makes git exit non-zero (`fatal: bad object`),
    // which run_or_warn turns into None — a git failure can never manufacture a
    // renamed classification.
    let bogus = git.rename_target_for_path(
        "old.rs",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    );
    assert!(
        bogus.is_none(),
        "a bogus sha must return None; got {:?}",
        bogus,
    );

    // Fail-safe (c): the rename TARGET appears in the rename commit's diff, but
    // it is not the rename SOURCE — matching on it would invert the relation.
    let target_as_source = git.rename_target_for_path("sub/new.rs", &rename_sha);
    assert!(
        target_as_source.is_none(),
        "querying the rename TARGET as if it were the source must return None; got {:?}",
        target_as_source,
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

// -----------------------------------------------------------------------
// Transient spawn-failure retry (task #4800)
// -----------------------------------------------------------------------

/// Pin that `RealGitOps::ls_files` recovers from a single transient spawn
/// failure and returns the real tracked-file list.
///
/// This test exercises the spawn-retry path added to `RealGitOps::run()` to
/// de-flake PTODO infra tests under merge-verify load.  Under load, the OS
/// can return `EAGAIN`/`ENOMEM` on `fork`/`exec`, causing `Command::output()`
/// to return `Err`.  Without a retry the error propagates through
/// `run_or_warn` → `ls_files` → empty `vec![]` → zero PTODO findings → exit 0
/// — the exit-code flip that causes (c-dirty)/(d-orphan) to fail.
///
/// RED-before-retry:  `inject_spawn_failures(1)` injects one `Err(WouldBlock)`
/// before the seam was added.  `run()` calls `spawn_once` exactly once → hits
/// the injected error → `run_or_warn` returns `None` → `ls_files` returns
/// `vec![]` → the collected set is empty ≠ the 3-path expected set → FAIL.
///
/// GREEN-after-retry:  `run()` calls `spawn_with_retry`, which retries after
/// the single injected `Err` and succeeds on the second real `spawn_once`
/// invocation → `ls_files` returns the 3 real paths → assertion passes.
///
/// The assertion pins only *recovery* (non-empty, correct set), NOT the retry
/// cap or backoff timing — those are tunables.
#[test]
fn run_retries_transient_spawn_failure() {
    use std::collections::BTreeSet;

    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // Commit three tracked files — mirrors ls_files_lists_tracked_paths_only.
    write_file(root, "a.rs", "fn a() {}\n");
    write_file(root, "dir/b.sh", "echo hi\n");
    write_file(root, "crates/x/c.rs", "fn c() {}\n");
    git_commit(root, "commit three tracked files");

    let git = RealGitOps::new(root);

    // Inject one transient spawn failure.  Without a retry the first
    // spawn_once returns Err → run_or_warn → None → ls_files → vec![].
    // With a retry, the second spawn_once hits real git and recovers.
    git.fail_next_spawns(1);

    let listed: BTreeSet<String> = git.ls_files().into_iter().collect();

    let expected: BTreeSet<String> = ["a.rs", "dir/b.sh", "crates/x/c.rs"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        listed, expected,
        "ls_files must recover from 1 injected transient spawn failure and \
         return the real tracked-file set; got: {:?}",
        listed,
    );
}

/// Pins the exhaustion / degradation contract of `spawn_with_retry`.
///
/// When more failures are injected than `MAX_ATTEMPTS` allows, every retry
/// hits an injected `Err`, the retry budget is exhausted, and the last `Err`
/// propagates through `run_or_warn` → `ls_files` → `vec![]`.  This is the
/// "degrades exactly as before" contract stated in the design decisions.
///
/// The test also exercises the `last_err.expect(...)` line inside
/// `spawn_with_retry` and the final retry-cap boundary that would be missed
/// by a regression (e.g. an off-by-one in `MAX_ATTEMPTS`).
///
/// 16 injected failures is deliberately generous — it exhausts for any
/// plausible `MAX_ATTEMPTS` value without being coupled to the exact cap.
/// If the cap were bumped to 4+ the test would still exhaust (rather than
/// silently becoming a non-exhaustion test that passes for the wrong reason).
#[test]
fn run_exhausts_retries_and_degrades_to_empty() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    git_init(root);

    // Commit one tracked file so there is something real to list if git runs.
    write_file(root, "a.rs", "fn a() {}\n");
    git_commit(root, "commit one tracked file");

    let git = RealGitOps::new(root);

    // Inject 16 failures — well above any plausible MAX_ATTEMPTS — so every
    // spawn_once returns Err and the retry loop exhausts.
    // run_or_warn -> None -> ls_files -> vec![].
    git.fail_next_spawns(16);

    let listed = git.ls_files();

    assert!(
        listed.is_empty(),
        "ls_files must degrade to vec![] when all retry attempts are exhausted; \
         got: {:?}",
        listed,
    );
}

// -----------------------------------------------------------------------
// changed_paths_in_commit — the commit's OWN delta (task 6345, Defect 2)
// -----------------------------------------------------------------------

/// Pin that `changed_paths_in_commit` reports a commit's own delta, and that
/// `diff_changed_paths("main", <commit>)` demonstrably cannot once `<commit>`
/// is an ancestor of main.
///
/// `main..<commit>` is a two-point TREE diff. Once `<commit>` has been merged,
/// main and the commit agree on exactly the paths the commit introduced, so
/// the task's own files are EXCLUDED by construction — the reverse-delta of
/// whatever landed AFTER it is returned instead. MEASURED on the live repo:
/// for merge `bc8f74a4d4`, `main..M` returned 6 paths and every one of the
/// task's own six files was absent from that set.
///
/// This must run against REAL git: a mock returns whatever you seeded, so it
/// cannot catch a wrong range string. That is this binary's stated purpose.
#[test]
fn changed_paths_in_commit_returns_the_commits_own_delta() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let run = |args: &[&str]| git_run(root, args);

    git_init(root);

    // Base commit on main.
    write_file(root, "base.txt", "base\n");
    git_commit(root, "base commit");

    // A task branch whose deliverable is `task_file.rs`, merged --no-ff.
    run(&["checkout", "-b", "feat"]);
    write_file(root, "task_file.rs", "fn task() {}\n");
    git_commit(root, "feat: the task's deliverable");
    run(&["checkout", "main"]);
    run(&["merge", "--no-ff", "-m", "Merge task/feat into main", "feat"]);
    let merge_sha = rev_parse_head(root);

    // Main advances past M, so M is a non-tip ancestor — the production shape.
    write_file(root, "later.txt", "unrelated later work\n");
    git_commit(root, "unrelated commit after the merge");

    let git = RealGitOps::new(root);

    // The degenerate leg this fix exists to replace.
    let reverse = git.diff_changed_paths("main", &merge_sha);
    assert!(
        !reverse.contains(&"task_file.rs".to_string()),
        "main..<merge> must NOT surface the merge's own file once the merge is an \
         ancestor of main — if it does, the premise of this fix changed; got: {:?}",
        reverse,
    );

    // The correct leg.
    let own = git.changed_paths_in_commit(&merge_sha);
    assert!(
        own.contains(&"task_file.rs".to_string()),
        "changed_paths_in_commit({}) must contain the merge's own deliverable; got: {:?}",
        merge_sha,
        own,
    );

    // A DELETION is reported too. This is the property the pre-done gate's
    // deletion/rename rescue depends on: a removed file has
    // path_tracked_on(main, p) == false, and only the landing commit's own
    // delta can show that the removal was the deliverable.
    run(&["rm", "base.txt"]);
    git_commit(root, "remove base.txt");
    let del_sha = rev_parse_head(root);
    let deleted = git.changed_paths_in_commit(&del_sha);
    assert!(
        deleted.contains(&"base.txt".to_string()),
        "changed_paths_in_commit({}) must report the deleted path; got: {:?}",
        del_sha,
        deleted,
    );

    // Fail-safe: an unreachable / recycled SHA yields an empty vec rather than
    // panicking — matching the contract of diff_added_lines_in_commit.
    let unreachable = git.changed_paths_in_commit("0000000000000000000000000000000000000000");
    assert!(
        unreachable.is_empty(),
        "changed_paths_in_commit on an unreachable SHA must fail safe to empty; got: {:?}",
        unreachable,
    );
}

// -----------------------------------------------------------------------
// Rename detection must be OFF on both path-listing seams (task 6345)
// -----------------------------------------------------------------------

/// Pin that both `--name-only` path-listing seams report BOTH sides of a
/// rename — the destination *and* the vanished source.
///
/// `diff.renames` has defaulted to true since git 2.9 and this repo sets no
/// override (`git config --get diff.renames` exits 1). With detection on,
/// `--name-only` collapses a detected rename to the DESTINATION path alone and
/// the old path never appears. Measured on a throwaway repo: after
/// `git mv a/old.rs a/new.rs`, `git diff --name-only HEAD^1..HEAD` prints
/// `a/new.rs` alone, while `--no-renames` prints both `a/new.rs` and
/// `a/old.rs`.
///
/// That silently breaks the pre-done gate. Both seams feed corroboration legs
/// that only ever SUBTRACT from the "absent from main" set, so a task that
/// declares its pre-rename path in `metadata.files` finds that path neither
/// tracked on main (it was renamed away) nor present in its landing commit's
/// delta (detection hid it) — and the flip is refused for work that in fact
/// landed.
///
/// Both seams are covered here because both are reachable from
/// `changed_paths_for_claim`: the ancestor arm calls `changed_paths_in_commit`
/// (the landed case), the non-ancestor arm calls `diff_changed_paths(main, …)`
/// (the un-landed D-1 branch-tip case).
///
/// `git mv` with content unchanged makes rename detection fire at 100%
/// similarity, so the defect is deterministic rather than dependent on git's
/// similarity threshold. And it must be a REAL-git test: a mock returns
/// whatever the fixture seeded, so it is structurally incapable of catching a
/// defect in git's argv.
#[test]
fn changed_paths_in_commit_reports_both_sides_of_a_rename() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let run = |args: &[&str]| git_run(root, args);

    git_init(root);

    // Base commit on main: the file the landed task will rename, plus a second
    // file for the un-landed branch-tip leg below.
    write_file(root, "a/old.rs", "fn task() {}\n");
    write_file(root, "b/stays.rs", "fn stays() {}\n");
    git_commit(root, "base commit");

    // --- Seam 1: changed_paths_in_commit, on a landed --no-ff merge. ---
    run(&["checkout", "-b", "feat"]);
    run(&["mv", "a/old.rs", "a/new.rs"]);
    git_commit(root, "feat: move old.rs to new.rs");
    run(&["checkout", "main"]);
    run(&["merge", "--no-ff", "-m", "Merge task/feat into main", "feat"]);
    let merge_sha = rev_parse_head(root);

    let git = RealGitOps::new(root);

    let own = git.changed_paths_in_commit(&merge_sha);
    assert!(
        own.contains(&"a/new.rs".to_string()),
        "changed_paths_in_commit({}) must report the rename DESTINATION; got: {:?}",
        merge_sha,
        own,
    );
    assert!(
        own.contains(&"a/old.rs".to_string()),
        "changed_paths_in_commit({}) must also report the rename SOURCE — a task \
         declaring its pre-rename path is otherwise refused by the pre-done gate \
         for work that did land; got: {:?}",
        merge_sha,
        own,
    );

    // --- Seam 2: diff_changed_paths(main, <tip>), on an UN-landed branch. ---
    // This is the arm `changed_paths_for_claim` takes when the claimed commit
    // is not yet an ancestor of main, where the two-point tree diff IS the
    // right question — but it collapses renames identically.
    run(&["checkout", "-b", "feat2"]);
    run(&["mv", "b/stays.rs", "b/moved.rs"]);
    git_commit(root, "feat2: move stays.rs to moved.rs");
    let tip_sha = rev_parse_head(root);
    run(&["checkout", "main"]);

    let tip = git.diff_changed_paths("main", &tip_sha);
    assert!(
        tip.contains(&"b/moved.rs".to_string()),
        "diff_changed_paths(main, {}) must report the rename DESTINATION; got: {:?}",
        tip_sha,
        tip,
    );
    assert!(
        tip.contains(&"b/stays.rs".to_string()),
        "diff_changed_paths(main, {}) must also report the rename SOURCE — the \
         un-landed branch-tip leg has the identical false-refusal defect; got: {:?}",
        tip_sha,
        tip,
    );
}
