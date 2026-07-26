//! The single constructor for git invocations that target a SPECIFIC
//! repository.
//!
//! # The rule
//!
//! **Every git invocation that targets a specific repository must be built
//! through this module**, so that `-C <root>` is authoritative regardless of
//! an ambient environment. That covers production and test code alike: a
//! fixture helper that shells `git init` into a tempdir is exactly as
//! vulnerable as the shipped binary.
//!
//! **Carve-out:** a bare `git --version` availability probe opens no
//! repository, so no redirect var can affect it and it is exempt. The line is
//! sharp — anything with `-C`, a repository path, or an implicit cwd
//! repository is NOT exempt. (The three exempt probes in this workspace are
//! `tests/g_allow_repo_wide_hard_gate.rs`, `tests/ptodo_baseline.rs` and
//! `reify-test-support/src/orphan_audit.rs`, all `git --version`.)
//!
//! Also deliberately unsanitized: `reify-test-support`'s
//! `scripts/audit-orphan-producers.sh` spawn, which runs with
//! `.current_dir(repo_root)` against the LIVE repository. There an ambient
//! `GIT_DIR` points at that same repository and is therefore correct;
//! stripping it would be a behaviour change, not a fix.
//!
//! ## Sweep status
//!
//! `git grep -n 'Command::new("git")' -- '*.rs'` over the whole workspace
//! returns, besides this module and prose references to it, exactly the three
//! `git --version` probes named above. Every repo-targeting site — the three
//! `RealGitOps` methods (`spawn_once`, `is_gitignored`, `is_ancestor`) and the
//! six fixture helpers in `tests/cli.rs` and `tests/real_git_ops.rs` — is
//! routed through here. Re-run that grep when adding a git call site: a new
//! hit that is not a `--version` probe is a defect.
//!
//! # The failure mode this prevents
//!
//! Git exports `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` into a hook's
//! entire process tree, and those override an explicit `-C <root>`. For
//! `git commit --only`, `GIT_INDEX_FILE` points at a *temporary* index built
//! for that commit. So under `hooks/pre-commit` -> `hooks/project-checks` ->
//! `scripts/verify.sh` -> the workspace test run, an unsanitized
//! `git -C <tempdir> add .` writes the PARENT repository's temporary index
//! instead of the tempdir's, and an unsanitized `git -C <tempdir> ls-files`
//! reads that parent index — yielding a wrong file set rather than an error.
//! Both observed signatures came from this: a divergent PTODO finding set
//! (exit `Some(1)` where `Some(2)` was expected) from the production reader
//! path, and `git ["add", "."] exited Some(128)` from a fixture helper
//! colliding with the parent repo's `index.lock`.
//!
//! Note the asymmetry that makes this worth a shared module: the redirect
//! vars do not make git *fail* — they make it silently operate on a
//! different repository.
//!
//! # Why an enumerated set, not a wholesale `GIT_*` clear
//!
//! [`REPO_REDIRECT_VARS`] has a crisp defining criterion: these are exactly
//! the vars that answer *"which repository am I operating on"* — the question
//! `-C <root>` is supposed to answer authoritatively. Clearing all `GIT_*`
//! would additionally drop `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/
//! `GIT_CONFIG_NOSYSTEM` (which a harness may set precisely to *increase*
//! isolation), `GIT_TRACE*` (debuggability) and `GIT_AUTHOR_*`/
//! `GIT_COMMITTER_*` (commit determinism) — strictly more collateral for no
//! gain against the failure above. Those are left untouched.

use std::path::Path;
use std::process::Command;

/// Git environment variables that redirect *which repository* a command
/// operates on, and therefore can silently override an explicit `-C <root>`.
///
/// Removed — not overwritten — by [`sanitize`]. The unit test pins the three
/// hook-exported vars (`GIT_DIR`, `GIT_INDEX_FILE`, `GIT_WORK_TREE`) as a
/// containment FLOOR, so this list may grow without a lockstep test edit.
pub const REPO_REDIRECT_VARS: &[&str] = &[
    // Exported by git into a hook's process tree — the observed failure.
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_WORK_TREE",
    // Same class: each redirects part of "which repository / which objects".
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// Remove every [`REPO_REDIRECT_VARS`] entry from `cmd`'s environment.
///
/// Returns `&mut Command` so callers that need a shape other than
/// `git -C <root>` can still chain off it.
///
/// Removal (`env_remove`) rather than assignment is deliberate: there is no
/// correct value to assign, and an inherited-but-empty var is not the same as
/// an absent one to git.
///
/// Currently the only non-test caller is [`command`], because every
/// repo-targeting call site in this workspace wants the `git -C <root>` shape.
/// It stays public anyway: a caller needing another shape (e.g. a
/// `.current_dir()`-based invocation) must be able to reach the sanitizer
/// rather than re-derive [`REPO_REDIRECT_VARS`] by hand, which is exactly how
/// this class of bug reaches a new helper.
// G-allow: public opt-in for non-`-C` call sites; only non-test caller today is command() — see doc above; behaviour pinned by the git_env unit tests.
pub fn sanitize(cmd: &mut Command) -> &mut Command {
    for var in REPO_REDIRECT_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// A pre-sanitized `git -C <root>` command.
///
/// The path of least resistance for every repo-targeting call site — prefer
/// this over hand-rolling `Command::new("git").arg("-C")`, which is how the
/// bug above reaches a new helper.
pub fn command(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    sanitize(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::{REPO_REDIRECT_VARS, command, sanitize};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    /// The three vars git exports into a hook's entire process tree, each of
    /// which silently overrides an explicit `-C <root>`. This is a normative
    /// FLOOR asserted by containment, not a restatement of the constant — the
    /// sanitized set may grow without a lockstep edit here.
    const HOOK_EXPORTED_FLOOR: &[&str] = &["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"];

    /// Collect the vars a `Command` has marked for REMOVAL. `std` encodes an
    /// `env_remove` as a `(key, None)` pair in `get_envs()`; an overwrite would
    /// be `(key, Some(value))`.
    fn removed_vars(cmd: &Command) -> Vec<String> {
        cmd.get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn command_targets_git_dash_c_at_the_requested_root() {
        let cmd = command(Path::new("/some/root"));

        assert_eq!(cmd.get_program(), OsStr::new("git"));

        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(
            args.len() >= 2,
            "expected at least the `-C <root>` prefix, got {:?}",
            args
        );
        assert_eq!(args[0], OsStr::new("-C"));
        assert_eq!(args[1], OsStr::new("/some/root"));
    }

    #[test]
    fn command_removes_every_repo_redirect_var() {
        let cmd = command(Path::new("/some/root"));
        let removed = removed_vars(&cmd);

        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "`{}` must be REMOVED (env_remove -> `(key, None)`), not merely \
                 overwritten; removals seen: {:?}",
                var,
                removed
            );
        }
    }

    #[test]
    fn repo_redirect_vars_covers_the_hook_exported_floor() {
        for var in HOOK_EXPORTED_FLOOR {
            assert!(
                REPO_REDIRECT_VARS.contains(var),
                "REPO_REDIRECT_VARS must contain at least `{}` — git exports it \
                 into a hook's process tree, where it overrides `-C <root>`; \
                 current set: {:?}",
                var,
                REPO_REDIRECT_VARS
            );
        }
    }

    #[test]
    fn sanitize_applies_the_same_removals_to_a_caller_built_command() {
        // A caller that needs a non-`-C` shape can still opt in.
        let mut cmd = Command::new("git");
        cmd.arg("--version");
        sanitize(&mut cmd);

        let removed = removed_vars(&cmd);
        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "sanitize() must remove `{}`; removals seen: {:?}",
                var,
                removed
            );
        }
    }

    #[test]
    fn sanitize_returns_the_command_for_chaining() {
        let mut cmd = Command::new("git");
        let chained = sanitize(&mut cmd).arg("status");
        assert_eq!(
            chained.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("status")]
        );
    }
}
