//! Git-environment helpers shared by the reify-audit integration test
//! binaries.
//!
//! Two helpers, both deliberately thin — neither duplicates the sanitized
//! variable list, which lives once in [`reify_audit::git_env`]:
//!
//! - [`git_cmd`] — the constructor every fixture-repo helper should use.
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

use std::path::Path;
use std::process::Command;

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

/// Re-run this test binary's `filter`-matching tests under a poisoned
/// *ambient* git environment, and assert they all still pass.
///
/// Call this from a test whose own name does NOT match `filter`, so the
/// replay cannot select itself. The `REIFY_AUDIT_HOOK_ENV_REPLAY` guard is
/// the second line of defence: inside the replayed child this function
/// returns immediately, so even a self-matching filter terminates.
///
/// The child inherits exactly the three variables git exports into a hook's
/// process tree, pointed at a throwaway decoy repository. The decoy carries a
/// stale `index.lock`, so an unsanitized index write fails loudly rather than
/// silently landing in the wrong repository.
///
/// `current_exe()` is the libtest binary itself, which accepts a filter
/// positional plus `--test-threads`/`--nocapture`. Under nextest this is the
/// per-test binary, and nextest's own process-per-test invocation is
/// unaffected because the child is spawned by us, not by nextest.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env(filter: &str) {
    // Re-entrancy guard: we ARE the replayed child. Do not recurse.
    if std::env::var_os(REPLAY_GUARD).is_some() {
        return;
    }

    let decoy = tempfile::tempdir().expect("create decoy repo tempdir");
    let status = git_cmd(decoy.path())
        .args(["init", "--initial-branch=main"])
        .status()
        .expect("git init decoy failed to spawn");
    assert!(
        status.success(),
        "decoy git init exited {:?}",
        status.code()
    );

    let decoy_git_dir = decoy.path().join(".git");
    std::fs::write(decoy_git_dir.join("index.lock"), b"")
        .expect("plant stale index.lock in decoy repo");

    let exe = std::env::current_exe().expect("current_exe");
    let out = Command::new(&exe)
        .args([filter, "--test-threads=1", "--nocapture"])
        .env(REPLAY_GUARD, "1")
        // Exactly what git exports into a hook's process tree.
        .env("GIT_DIR", &decoy_git_dir)
        .env("GIT_WORK_TREE", decoy.path())
        .env("GIT_INDEX_FILE", decoy_git_dir.join("index"))
        .output()
        .expect("re-exec self under poisoned ambient git env");

    assert!(
        out.status.success(),
        "re-running `{}` (filter {:?}) under an ambient hook git env must pass \
         exactly as it does without one; child exited {:?}\n\
         --- child stdout ---\n{}\n--- child stderr ---\n{}",
        exe.display(),
        filter,
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    drop(decoy);
}
