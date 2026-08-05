//! The workspace's single definition of the repo-redirect git-env sanitizer.
//!
//! `REPO_REDIRECT_VARS` and `sanitize` live HERE, in the lower crate, because
//! the dependency edge runs `reify-audit` -> `reify-test-support` (see
//! `crates/reify-audit/Cargo.toml`'s `[dependencies]` entry, where
//! `reify-test-support` is a production dependency). Both sides need the same
//! sanitizer — this crate's own `orphan_audit` spawns and probes, and
//! `reify-audit`'s repo-targeting git commands — and only this direction is
//! reachable without inverting the edge.
//!
//! `reify_audit::git_env` re-exports both items, and builds the `git -C <root>`
//! constructor (`reify_audit::git_env::command`) on top of them; that module's
//! doc carries the full failure-mode analysis this sanitizer exists to prevent.

use std::process::Command;

/// Git environment variables that redirect *which repository* a command
/// operates on, and therefore can silently override an explicit `-C <root>`.
///
/// The set is enumerated rather than a wholesale `GIT_*` clear because it has
/// a crisp defining criterion: these are exactly the vars that answer *"which
/// repository am I operating on"* — the question `-C <root>` is supposed to
/// answer authoritatively. Clearing all `GIT_*` would additionally drop
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/`GIT_CONFIG_NOSYSTEM` (which a
/// harness may set precisely to *increase* isolation), `GIT_TRACE*`
/// (debuggability) and `GIT_AUTHOR_*`/`GIT_COMMITTER_*` (commit determinism) —
/// strictly more collateral for no gain. Those are left untouched.
///
/// Removed — not overwritten — by [`sanitize`].
///
/// `reify_audit::git_env`'s `repo_redirect_vars_covers_the_removal_floor` names
/// each entry independently as a containment FLOOR, so this list may still GROW
/// without a lockstep test edit, but an entry cannot be silently DELETED from
/// it.
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
/// `git -C <root>` can still chain off it — this crate's own `orphan_audit`
/// sites are exactly that case (`.current_dir(..)`, and a spawn of a shell
/// script that runs git internally).
///
/// Removal (`env_remove`) rather than assignment is deliberate: there is no
/// correct value to assign, and an inherited-but-empty var is not the same as
/// an absent one to git.
///
/// `reify_audit::git_env::command` is the `git -C <root>` constructor built on
/// top of this, and is the path of least resistance for a repo-targeting call
/// site above the dependency edge. A caller needing another shape must be able
/// to reach this sanitizer rather than re-derive [`REPO_REDIRECT_VARS`] by
/// hand, which is exactly how this class of bug reaches a new helper.
pub fn sanitize(cmd: &mut Command) -> &mut Command {
    for var in REPO_REDIRECT_VARS {
        cmd.env_remove(var);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::process::Command;

    /// Direct coverage of this crate's own sanitizer.
    ///
    /// Not redundant with `reify-audit`'s fuller `git_env` suite:
    /// `reify-test-support` cannot depend on `reify-audit` (that would invert
    /// the dependency edge), and `scripts/verify.sh` runs narrowed /
    /// affected-crate cargo passes (`--narrow`, `AFFECTED_ALL_FLAGS`), so a
    /// `-p reify-test-support`-only run would otherwise have ZERO direct
    /// coverage of this crate's own new public module.
    ///
    /// Asserts REMOVAL, not mere overwrite: `std` records an `env_remove` as a
    /// `(key, None)` pair in `get_envs()`, an assignment as `(key, Some(..))`.
    /// The command is shaped like a real caller here
    /// (`orphan_audit::child_repo_root` — `.current_dir(..)`, no `-C`), but is
    /// never spawned: only `Command` metadata is inspected, so the working
    /// directory is deliberately nonexistent and neither `git` on `PATH` nor a
    /// real temporary directory is needed (same shape as
    /// `orphan_audit::tests::build_audit_command_removes_every_repo_redirect_var`).
    #[test]
    fn sanitize_removes_every_repo_redirect_var() {
        let mut cmd = Command::new("git");
        cmd.args(["rev-parse", "--show-toplevel"])
            .current_dir(std::path::Path::new("/nonexistent/repo-root"));

        // The returned `&mut Command` must chain, so a caller needing a shape
        // other than `git -C <root>` can opt in mid-builder.
        let chained = super::sanitize(&mut cmd).arg("--version");
        assert_eq!(
            chained.get_args().collect::<Vec<_>>(),
            vec![
                OsStr::new("rev-parse"),
                OsStr::new("--show-toplevel"),
                OsStr::new("--version"),
            ],
            "sanitize() must return the same command it sanitized, so callers can chain"
        );

        let removed: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for var in super::REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "sanitize() must REMOVE `{var}` (env_remove -> `(key, None)`), not \
                 merely overwrite it; removals seen: {removed:?}"
            );
        }
    }
}
