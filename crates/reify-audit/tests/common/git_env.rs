//! Git-environment helpers shared by the reify-audit integration test
//! binaries.
//!
//! Everything here is deliberately thin, and nothing here duplicates a
//! variable list: the *sanitized* set lives once in
//! [`reify_audit::git_env::REPO_REDIRECT_VARS`], and the *poisoned* set lives
//! once in [`hook_git_env`] (which asserts it is a subset of the sanitized
//! one, so the two cannot drift apart silently).
//!
//! - [`git_cmd`] — the constructor every fixture-repo helper should use.
//! - [`decoy_repo`] / [`poison_with_hook_git_env`] — build a stand-in for the
//!   parent repository a hook would point at, and apply the hook's exported
//!   environment to a command.
//! - [`replay_self_under_hook_git_env`] — the outer harness that proves the
//!   fix under a real *ambient* environment rather than a per-child one.
//!
//! # Why a replay harness
//!
//! The reported condition is a hook environment: `hooks/pre-commit` ->
//! `hooks/project-checks` -> `scripts/verify.sh` -> the workspace test run,
//! with `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` exported into the whole
//! process tree. Reproducing that *inside* a test would mean mutating the
//! test process's own environment, and `std::env::set_var` is process-global:
//! under nextest's process-per-test isolation it would appear to work, hiding
//! the hazard, while under `cargo test`'s thread-per-test model it would race
//! and intermittently poison sibling tests — trading a deterministic bug for
//! a flaky one.
//!
//! So instead of poisoning ourselves, we re-exec ourselves poisoned: spawn
//! `current_exe()` with the poison in the CHILD's environment, where it is
//! genuinely ambient for every test that child runs.
//!
//! # Why the replay counts tests
//!
//! libtest exits 0 when a filter matches ZERO tests (measured:
//! `./cli-<hash> 'cli::nonexistent_filter_xyz'` prints
//! `test result: ok. 0 passed; … 35 filtered out` and exits 0). Asserting only
//! on the child's exit status would therefore turn this harness into a silent
//! green the instant a filter stops matching — a rename, a dropped `mod`
//! wrapper, or a test moving to another binary — which is precisely the
//! failure class this whole change set exists to close. So the replay lists
//! the selection first, requires it to be non-empty and at least the caller's
//! declared floor, and then requires the poisoned run to actually account for
//! every listed test.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Environment variable marking the replayed child process, so the replay
/// test does not recurse when the child re-runs it.
const REPLAY_GUARD: &str = "REIFY_AUDIT_HOOK_ENV_REPLAY";

/// A pre-sanitized `git -C <dir>` command for fixture-repo setup.
///
/// Thin by design: the sanitized variable list lives once, in
/// [`reify_audit::git_env::REPO_REDIRECT_VARS`]. A fixture helper that shells
/// a bare `Command::new("git")` is exactly as vulnerable as production code
/// was — an ambient `GIT_INDEX_FILE` overrides `-C <tempdir>`, so
/// `git -C <tempdir> add .` writes the PARENT repository's index (observed as
/// `git ["add", "."] exited Some(128)`, colliding with the parent's
/// `index.lock`).
#[allow(dead_code)]
pub fn git_cmd(dir: &Path) -> Command {
    reify_audit::git_env::command(dir)
}

/// A throwaway repository standing in for the parent repo a hook would point
/// at, carrying a stale `index.lock`.
///
/// The lock is what makes an unsanitized index WRITE fail loudly as well as
/// wrongly, so a regression cannot hide as a silent no-op.
#[allow(dead_code)]
pub struct DecoyRepo {
    /// Held only to keep the temporary directory alive for the decoy's
    /// lifetime; every path below points inside it.
    _dir: TempDir,
    git_dir: PathBuf,
    work_tree: PathBuf,
}

#[allow(dead_code)]
impl DecoyRepo {
    /// The decoy's `.git` directory — what an ambient `GIT_DIR` would name.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The decoy's working tree — what an ambient `GIT_WORK_TREE` would name.
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }
}

/// Build a [`DecoyRepo`]: `git init` into a fresh tempdir, then plant a stale
/// `index.lock`.
///
/// The init itself goes through [`git_cmd`] like every other repo-targeting
/// call. That is load-bearing, not decorative: a caller of this function may
/// itself be re-run inside a poisoned replay child, where a bare
/// `Command::new("git")` would re-init the *harness's* decoy instead of
/// creating this one.
#[allow(dead_code)]
pub fn decoy_repo() -> DecoyRepo {
    let dir = tempfile::tempdir().expect("create decoy repo tempdir");

    let status = git_cmd(dir.path())
        .args(["init", "--initial-branch=main"])
        .status()
        .expect("git init decoy failed to spawn");
    assert!(
        status.success(),
        "decoy git init exited {:?}",
        status.code()
    );

    let git_dir = dir.path().join(".git");
    std::fs::write(git_dir.join("index.lock"), b"").expect("plant stale index.lock in decoy repo");

    let work_tree = dir.path().to_path_buf();
    DecoyRepo {
        _dir: dir,
        git_dir,
        work_tree,
    }
}

/// The exact `(name, value)` set git exports into a hook's entire process
/// tree, pointed at `decoy`.
///
/// The single home for the poisoned list on the test side. Every var here is
/// asserted to be one [`reify_audit::git_env::sanitize`] removes, so adding a
/// fourth var without teaching the sanitizer about it fails loudly here
/// instead of silently reducing coverage.
fn hook_git_env(decoy: &DecoyRepo) -> Vec<(&'static str, PathBuf)> {
    let vars = vec![
        ("GIT_DIR", decoy.git_dir().to_path_buf()),
        ("GIT_WORK_TREE", decoy.work_tree().to_path_buf()),
        ("GIT_INDEX_FILE", decoy.git_dir().join("index")),
    ];

    for (name, _) in &vars {
        assert!(
            reify_audit::git_env::REPO_REDIRECT_VARS.contains(name),
            "poisoning with `{}` proves nothing unless the sanitizer removes it — \
             add it to reify_audit::git_env::REPO_REDIRECT_VARS; current set: {:?}",
            name,
            reify_audit::git_env::REPO_REDIRECT_VARS,
        );
    }

    vars
}

/// Apply a hook's exported git environment, pointed at `decoy`, to `cmd`.
///
/// Poisons the CHILD only. Callers must never touch their own process
/// environment — see the module doc on `std::env::set_var`.
#[allow(dead_code)]
pub fn poison_with_hook_git_env<'a>(cmd: &'a mut Command, decoy: &DecoyRepo) -> &'a mut Command {
    for (name, value) in hook_git_env(decoy) {
        cmd.env(name, value);
    }
    cmd
}

/// Re-run this test binary's `filters`-matching tests under a poisoned
/// *ambient* git environment, and assert they all still pass.
///
/// `filters` are libtest positional filters, OR-combined (measured: passing
/// two names lists exactly those two). A single `""` selects every test in the
/// binary.
///
/// `expected_min` is the caller's declared floor on how many tests the
/// selection must contain — the guard against a vacuous pass. Set it to the
/// count you actually intend to cover; the selection may grow past it freely,
/// but it may not silently shrink below it.
///
/// Call this from a test whose own name does NOT match `filters`, so the
/// replay cannot select itself. The `REIFY_AUDIT_HOOK_ENV_REPLAY` guard is
/// the second line of defence: inside the replayed child this function
/// returns immediately, so even a self-matching filter terminates.
///
/// `current_exe()` is the libtest binary itself, which accepts filter
/// positionals plus `--list`/`--test-threads`/`--nocapture`. Under nextest
/// this is the per-test binary, and nextest's own process-per-test invocation
/// is unaffected because the child is spawned by us, not by nextest.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env(filters: &[&str], expected_min: usize) {
    // Re-entrancy guard: we ARE the replayed child. Do not recurse.
    if std::env::var_os(REPLAY_GUARD).is_some() {
        return;
    }

    let exe = std::env::current_exe().expect("current_exe");

    // Non-vacuity, step 1: what does this selection actually cover? Listing
    // runs in a CLEAN environment on purpose — the selection is what we want
    // to compare the poisoned run against.
    let listed = list_matching_tests(&exe, filters);
    assert!(
        listed.len() >= expected_min.max(1),
        "the replay filters {:?} select {} test(s), below the declared floor of {} — \
         libtest exits 0 on a zero-match filter, so this harness would have been a \
         vacuous pass. A test was probably renamed, moved to another binary, or lost \
         its `mod` wrapper. Selected: {:?}",
        filters,
        listed.len(),
        expected_min.max(1),
        listed,
    );

    let decoy = decoy_repo();

    let mut cmd = Command::new(&exe);
    cmd.args(filters)
        .args(["--test-threads=1", "--nocapture"])
        .env(REPLAY_GUARD, "1");
    poison_with_hook_git_env(&mut cmd, &decoy);

    let out = cmd
        .output()
        .expect("re-exec self under poisoned ambient git env");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "a test in `{}` matching {:?} failed while re-run under an ambient hook git \
         env. The failure may or may not be env-related: compare this test against \
         the same test in the clean parent run — if it failed there too, diagnose it \
         there, not here. Child exited {:?}\n\
         --- child stdout ---\n{}\n--- child stderr ---\n{}",
        exe.display(),
        filters,
        out.status.code(),
        stdout,
        stderr,
    );

    // Non-vacuity, step 2: a green exit is only meaningful if the poisoned run
    // actually accounted for every test the clean listing selected.
    let (passed, ignored) = parse_passed_and_ignored(&stdout).unwrap_or_else(|| {
        panic!(
            "could not find libtest's `test result:` summary in the replayed child's \
             stdout; cannot confirm the run was non-vacuous\n\
             --- child stdout ---\n{}\n--- child stderr ---\n{}",
            stdout, stderr,
        )
    });

    assert_eq!(
        passed + ignored,
        listed.len(),
        "the poisoned replay reported {} passed + {} ignored, but the same filters \
         {:?} list {} test(s) — the child ran a different set than the parent \
         selected, so a green exit proves nothing. Selected: {:?}\n\
         --- child stdout ---\n{}",
        passed,
        ignored,
        filters,
        listed.len(),
        listed,
        stdout,
    );
    assert!(
        passed >= expected_min.max(1),
        "the poisoned replay reported only {} passing test(s), below the declared \
         floor of {}\n--- child stdout ---\n{}",
        passed,
        expected_min.max(1),
        stdout,
    );
}

/// The test names `filters` select in `exe`, via libtest's `--list`.
///
/// `--list` prints one `<name>: test` line per selected test (benchmarks get
/// `: benchmark`), then a blank line and an `N tests, M benchmarks` summary.
fn list_matching_tests(exe: &Path, filters: &[&str]) -> Vec<String> {
    let out = Command::new(exe)
        .args(filters)
        .arg("--list")
        .output()
        .expect("list this test binary's matching tests");
    assert!(
        out.status.success(),
        "`--list` on {} with filters {:?} exited {:?}\nstderr: {}",
        exe.display(),
        filters,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_string)
        .collect()
}

/// Extract `(passed, ignored)` from libtest's summary line, e.g.
/// `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out`.
///
/// Takes the LAST such line, since `--nocapture` interleaves test output that
/// could in principle contain the same prefix.
fn parse_passed_and_ignored(stdout: &str) -> Option<(usize, usize)> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("test result:"))?;

    let field = |suffix: &str| -> Option<usize> {
        line.split(';')
            .map(str::trim)
            .find_map(|seg| seg.strip_suffix(suffix))
            .and_then(|prefix| prefix.split_whitespace().next_back())
            .and_then(|n| n.parse().ok())
    };

    Some((field(" passed")?, field(" ignored")?))
}
