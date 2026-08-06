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
/// # Why an enumerated set, not a wholesale `GIT_*` clear
///
/// This set has a crisp defining criterion: these are exactly the vars that
/// answer *"which repository am I operating on"* — the question `-C <root>` is
/// supposed to answer authoritatively. Clearing all `GIT_*` would additionally
/// drop `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/`GIT_CONFIG_NOSYSTEM` (which a
/// harness may set precisely to *increase* isolation), `GIT_TRACE*`
/// (debuggability) and `GIT_AUTHOR_*`/`GIT_COMMITTER_*` (commit determinism) —
/// strictly more collateral for no gain against the failure mode
/// `reify_audit::git_env`'s module doc analyses. Those are left untouched.
///
/// This doc is the workspace's ONE copy of that argument, living with the
/// constant it justifies; `reify_audit::git_env` points here rather than
/// restating it.
///
/// Removed — not overwritten — by [`sanitize`].
///
/// # Deletion guard
///
/// `repo_redirect_vars_covers_the_removal_floor`, in this module's tests, names
/// each entry independently as a containment FLOOR, so this list may still GROW
/// without a lockstep test edit, but an entry cannot be silently DELETED from
/// it. That guard sits next to the definition it guards on purpose: every other
/// test of this set iterates the set itself (this module's sanitizer tests and
/// `orphan_audit`'s `build_audit_command_removes_every_repo_redirect_var`), so
/// all of them stay green through a deletion.
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
    use super::{REPO_REDIRECT_VARS, sanitize};
    use std::ffi::OsStr;
    use std::process::Command;

    /// Every var the design intends [`sanitize`] to remove, paired with the
    /// reason it qualifies as *"which repository am I operating on"*.
    ///
    /// Asserted by CONTAINMENT (floor ⊆ [`REPO_REDIRECT_VARS`]), so the
    /// sanitized set may still GROW without a lockstep edit here — but an
    /// entry cannot be silently DELETED from [`REPO_REDIRECT_VARS`] without a
    /// deliberate edit to this list too. Forcing that second edit is the whole
    /// point: a self-referential test that only checks "the code removes what
    /// the same constant lists" stays green through a deletion.
    const REMOVAL_FLOOR: &[(&str, &str)] = &[
        ("GIT_DIR", "names the repository directly"),
        (
            "GIT_INDEX_FILE",
            "names the index `add`/`ls-files` read and write",
        ),
        ("GIT_WORK_TREE", "names the working tree"),
        (
            "GIT_OBJECT_DIRECTORY",
            "redirects where new objects are written",
        ),
        (
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "adds foreign object stores to reads",
        ),
        (
            "GIT_COMMON_DIR",
            "redirects the shared dir a worktree resolves against",
        ),
        ("GIT_NAMESPACE", "changes which ref namespace is visible"),
        ("GIT_PREFIX", "changes how relative pathspecs resolve"),
    ];

    /// The subset git exports into a hook's ENTIRE process tree — the sharpest
    /// case, and exactly what `reify-audit`'s integration-test replay harness
    /// poisons with (`crates/reify-audit/tests/common/git_env.rs::hook_git_env`,
    /// which independently asserts its own three vars are contained in
    /// [`REPO_REDIRECT_VARS`] from the other side of the dependency edge).
    const HOOK_EXPORTED_FLOOR: &[&str] = &["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"];

    /// The deletion guard for [`REPO_REDIRECT_VARS`], and the only test of that
    /// set anywhere that is NOT self-referential.
    ///
    /// Every other test of it — this crate's
    /// `orphan_audit::tests::build_audit_command_removes_every_repo_redirect_var`
    /// and `reify_audit::git_env`'s
    /// `both_entry_points_remove_every_repo_redirect_var` — iterates the same
    /// constant to assert the code removes what it lists, so dropping an entry
    /// keeps them all green. This test names each var independently, so a
    /// deletion has to be argued for here too. It lives in the crate that
    /// DEFINES the constant so an editor working only in `reify-test-support`
    /// gets that signal locally rather than from a reverse dependency.
    #[test]
    fn repo_redirect_vars_covers_the_removal_floor() {
        for (var, why) in REMOVAL_FLOOR {
            assert!(
                REPO_REDIRECT_VARS.contains(var),
                "REPO_REDIRECT_VARS must contain `{var}` — it {why}, which is the \
                 question `-C <root>` is supposed to answer authoritatively. If \
                 removing it is genuinely correct, delete it from REMOVAL_FLOOR in \
                 the same change and say why. Current set: {REPO_REDIRECT_VARS:?}"
            );
        }

        // Keep the two test-side lists from drifting apart.
        for var in HOOK_EXPORTED_FLOOR {
            assert!(
                REMOVAL_FLOOR.iter().any(|(v, _)| v == var),
                "HOOK_EXPORTED_FLOOR entry `{var}` must also appear in \
                 REMOVAL_FLOOR; the hook-exported set is a subset of the \
                 repo-redirect set by construction"
            );
        }
    }

    /// [`sanitize`]'s core behaviour, asserted in the module that DEFINES it:
    /// every [`REPO_REDIRECT_VARS`] entry is marked for REMOVAL rather than
    /// overwritten or left inherited. `std` encodes an `env_remove` as a
    /// `(key, None)` pair in `get_envs()`; an overwrite would be
    /// `(key, Some(value))`.
    ///
    /// Self-referential by construction — it asserts the fn removes exactly
    /// what the same constant lists, so it stays green through a DELETION from
    /// [`REPO_REDIRECT_VARS`]; `repo_redirect_vars_covers_the_removal_floor`
    /// above is the guard against that, not this. What this contributes is
    /// that the defining module covers its own core behaviour directly,
    /// instead of borrowing that coverage from consumers whose spawns may be
    /// refactored away without anything here announcing the loss.
    #[test]
    fn sanitize_removes_every_repo_redirect_var() {
        let mut cmd = Command::new("git");
        sanitize(&mut cmd);

        let removed: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();

        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "sanitize() must REMOVE `{var}` (env_remove -> `(key, None)`), \
                 not merely overwrite it or leave it inherited; removals seen: \
                 {removed:?}"
            );
        }
    }

    /// The part of [`sanitize`]'s contract no caller in this crate exercises:
    /// the `&mut Command` return. In-crate call sites discard it in statement
    /// position, so this is the only in-crate exercise of the chaining shape —
    /// without it that half of the contract would rest entirely on a reverse
    /// dependency.
    ///
    /// The removal half is covered by
    /// `sanitize_removes_every_repo_redirect_var` above, and the set itself by
    /// `repo_redirect_vars_covers_the_removal_floor`.
    #[test]
    fn sanitize_returns_the_command_for_chaining() {
        let mut cmd = Command::new("git");
        let chained = sanitize(&mut cmd).arg("status");
        assert_eq!(
            chained.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("status")],
            "sanitize() must return the same command it sanitized, so callers \
             needing a shape other than `git -C <root>` can opt in mid-builder"
        );
    }
}
