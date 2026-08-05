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
    /// (`orphan_audit::child_repo_root` — `.current_dir(..)`, no `-C`), and
    /// needs neither a spawn nor `git` on `PATH`.
    #[test]
    fn sanitize_removes_every_repo_redirect_var() {
        let mut cmd = Command::new("git");
        cmd.args(["rev-parse", "--show-toplevel"])
            .current_dir(std::env::temp_dir());

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
