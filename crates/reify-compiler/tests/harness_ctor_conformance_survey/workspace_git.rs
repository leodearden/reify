//! The workspace root, and the only sanctioned way this survey reaches git.

use std::process::Command;

// The workspace's SINGLE definition of the repo-redirect sanitizer, not a local
// twin: `sanitize` is what `git_at_workspace_root` below applies, and
// `REPO_REDIRECT_VARS`/`removed_vars` are what its contract tests read, so this
// survey cannot drift from the set it is supposed to remove.
// (`crates/reify-test-support/src/git_env.rs`.)
use reify_test_support::git_env::{REPO_REDIRECT_VARS, removed_vars, sanitize};

/// Absolute path to the workspace root, resolved at compile time from this
/// crate's manifest directory (two levels up).
///
/// Same rooting idiom as `reify_test_support::examples_corpus::examples_dir()`,
/// pointed one level higher: β's whole point is that the sweep is NOT
/// examples-scoped.
pub(crate) const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

// ─── step 21/22: one sanitized git constructor for every call site ───────────

/// A pre-sanitized `git -C <workspace root>` command — the ONLY way this
/// survey reaches git.
///
/// Every git call here targets a specific repository (this workspace), and the
/// workspace rule for that shape is stated at `reify_audit::git_env`: build it
/// through a sanitized `-C <root>` constructor, because git exports
/// `GIT_DIR`/`GIT_INDEX_FILE`/`GIT_WORK_TREE` into a hook's entire process tree
/// and those OVERRIDE an explicit `-C`. The failure is silent — the command
/// operates on a *different* repository rather than erroring — which for this
/// survey would mean enumerating some other tree's `.ri` files into the survey,
/// or resolving the stamped anchor against the wrong object store. The rule's
/// one carve-out is a bare `git --version` availability probe — which is
/// exactly [`git_is_available`] below, and the ONLY spawn in this survey that
/// does not come from here; every repo-targeting call site routes through this
/// constructor.
///
/// `reify_audit::git_env::command` is the same shape one crate up, and is
/// deliberately NOT used: `reify-audit` is not a dependency of
/// `reify-compiler`, and adding that edge to reuse a four-line constructor
/// would be a far larger change than the sanctioned below-the-edge pattern its
/// own doc describes — route repo-targeting spawns through
/// `reify_test_support::git_env::sanitize` directly, which is the same single
/// sanitizer either way.
///
/// Returns an owned `Command` rather than borrowing `sanitize`'s `&mut` return,
/// because two of the three consumers need `.output()` and one needs only
/// `.status`.
pub(crate) fn git_at_workspace_root(args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(WORKSPACE_ROOT);
    cmd.args(args);
    sanitize(&mut cmd);
    cmd
}

/// Whether a `git` binary can be spawned at all.
///
/// The sanctioned carve-out from the `-C`-constructor rule above: a bare
/// `git --version` names no repository, so there is nothing for an ambient
/// `GIT_DIR` to redirect and nothing to sanitize against.
///
/// Exists so the GATE-RESIDENT tests can SKIP rather than red when git is
/// absent. Everything in this survey that reads git — the corpus enumeration
/// and the stamp guard — is a survey concern, not a compiler concern, and
/// `reify-compiler`'s test suite did not require git on PATH before this survey
/// existed. A survey generator must not be the thing that makes it a hard
/// requirement. Distinguishing "git said no" (a real finding, still a hard
/// failure) from "there is no git" (nothing to say) is the whole point: the
/// panics in [`git_read`] / [`git_succeeds`] / `scan_tracked_ri_corpus` stay
/// exactly as they were for every caller that gets past this probe.
pub(crate) fn git_is_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("git")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

#[test]
fn git_at_workspace_root_removes_every_repo_redirect_var() {
    // Iterates the SHARED constant rather than naming the vars locally, so a
    // future GROWTH of the sanitized set is covered here with no edit to this
    // file. Same shape as `reify_audit::git_env`'s own wiring test
    // (`command_removes_every_repo_redirect_var`), which likewise reads the
    // `(key, None)` removal encoding through the definition site's
    // `removed_vars` rather than a hand-copied local twin.
    //
    // What this deliberately does NOT re-prove: that the removals actually
    // defeat a real ambient redirect var, and that an unsanitized `-C` loses to
    // one. That is spawned against real git at the definition site, in
    // `reify_test_support::git_env`'s
    // `sanitize_makes_dash_c_authoritative_against_real_git`. Re-spawning git
    // here would buy no new signal and would add a git-availability skip path.
    let cmd = git_at_workspace_root(&["rev-parse", "HEAD"]);
    let removed = removed_vars(&cmd);
    for var in REPO_REDIRECT_VARS {
        assert!(
            removed.iter().any(|r| r == var),
            "git_at_workspace_root must REMOVE `{var}` (env_remove -> `(key, None)`), \
             not merely overwrite it; removals seen: {removed:?}"
        );
    }
}

#[test]
fn git_at_workspace_root_targets_git_dash_c_at_the_workspace_root() {
    // The other half of the guarantee, and the half sanitization alone cannot
    // give: a refactor could keep every `env_remove` in place while dropping
    // the `-C`. Every git call in this survey runs from the crate directory,
    // never the repo root, so the `-C` is what makes the command name THIS
    // repository at all — and an exact, ordered argv comparison is what pins
    // that the caller's args follow the prefix rather than replace it.
    let cmd = git_at_workspace_root(&["rev-parse", "HEAD"]);

    assert_eq!(cmd.get_program(), std::ffi::OsStr::new("git"));

    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        ["-C", WORKSPACE_ROOT, "rev-parse", "HEAD"],
        "expected exactly `-C <workspace root>` followed by the caller's args, in that order"
    );
}

/// Run one git command at the workspace root and return its trimmed stdout.
///
/// Every invocation is `-C WORKSPACE_ROOT` (the test process's own CWD is the
/// crate directory, not the repo root), and every one keeps the non-zero-exit
/// assertion: a git read that silently failed would feed an empty string into
/// `stamp_decision`, which is precisely the "looks clean" reading that must
/// never be reachable by accident.
pub(crate) fn git_read(args: &[&str]) -> String {
    let out = git_at_workspace_root(args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "harness_ctor_conformance_survey::workspace_git: cannot run `git {}` in {WORKSPACE_ROOT}: {e}",
                args.join(" ")
            )
        });
    assert!(
        out.status.success(),
        "harness_ctor_conformance_survey::workspace_git: `git {}` in {WORKSPACE_ROOT} exited {:?}: {}",
        args.join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Run one git command at the workspace root and report only whether it
/// SUCCEEDED.
///
/// For the git PREDICATES — `cat-file -e`, `merge-base --is-ancestor` — whose
/// entire answer is the exit code, and where a non-zero exit is the finding
/// rather than an infrastructure failure. [`git_read`] would panic on it.
pub(crate) fn git_succeeds(args: &[&str]) -> bool {
    git_at_workspace_root(args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "harness_ctor_conformance_survey::workspace_git: cannot run `git {}` in {WORKSPACE_ROOT}: {e}",
                args.join(" ")
            )
        })
        .status
        .success()
}
