//! The single constructor for git invocations that target a SPECIFIC
//! repository.
//!
//! [`REPO_REDIRECT_VARS`] and [`sanitize`] are defined one crate down, in
//! `reify_test_support::git_env` (the dependency direction); see there for the
//! failure mode and the evidence. This module adds the `git -C <root>` shape
//! [`command`] builds, the workspace rule that every repo-targeting invocation
//! be built through it, and the call-site sweep enforcing that rule.
//!
//! # The rule
//!
//! **Every git invocation that targets a specific repository must be built
//! through this module**, so that `-C <root>` is authoritative regardless of
//! an ambient environment. That covers production and test code alike: a
//! fixture helper that shells `git init` into a tempdir is exactly as
//! vulnerable as the shipped binary.
//!
//! **Carve-outs:** a bare `git --version` availability probe opens no
//! repository, so no redirect var can affect it and it is exempt; so is a
//! `Command` that is never spawned at all, built only to be read back through
//! `get_envs()`/`get_args()`. The line is sharp — anything with `-C`, a
//! repository path, or an implicit cwd repository is NOT exempt. (The exempt
//! sites in this workspace are the `git --version` probes in
//! `tests/g_allow_repo_wide_hard_gate.rs`, `tests/ptodo_baseline.rs` and
//! `reify-test-support`'s `orphan_audit.rs` and `git_env.rs`, plus two
//! never-spawned metadata fixtures in `reify-test-support`'s `git_env.rs`
//! tests — `sanitize_removes_every_repo_redirect_var` and
//! `sanitize_returns_the_command_for_chaining`.)
//!
//! **Sites below the dependency edge:** `orphan_audit.rs` reaches
//! [`reify_test_support::git_env::sanitize`] directly rather than through this
//! module, because the dependency edge runs `reify-audit` ->
//! `reify-test-support`. Its two repo-targeting sites — the
//! `scripts/audit-orphan-producers.sh` spawn, whose first action is
//! `REPO_ROOT="$(git rev-parse --show-toplevel)"`, and the `child_repo_root`
//! probe that checks the spawned child resolves the work tree the caller
//! intended — satisfy the rule above through that one shared sanitizer without
//! calling into this crate. Each carries its own argument at its own definition;
//! `child_repo_root`'s doc explains what its probe catches that sanitization
//! alone cannot (a `repo_root` that was wrong to begin with).
//!
//! ## Sweep status
//!
//! `git grep -n 'Command::new("git")' -- '*.rs'` over the whole workspace
//! returns, besides this module and prose references to it, only sites the
//! carve-outs above cover. Every repo-targeting *git* site in this crate — the
//! three `RealGitOps` methods (`spawn_once`, `is_gitignored`, `is_ancestor`)
//! and the six fixture helpers in `tests/cli.rs` and `tests/real_git_ops.rs` —
//! is routed through [`command`].
//!
//! Below the edge, in `reify-test-support`, four non-exempt sites are routed
//! through the shared [`reify_test_support::git_env::sanitize`] instead, for the
//! dependency-direction reason above: `orphan_audit.rs`'s `child_repo_root`
//! probe and its `git init` decoy fixture in `wrong_tree_with_real_scope_panics`
//! ("a fixture helper that shells `git init` into a tempdir" is literally the
//! example the rule names), plus the `git -C` init loop and the FIX spawn in
//! `git_env.rs`'s own `sanitize_makes_dash_c_authoritative_against_real_git`.
//! That test also holds the workspace's ONE deliberately unsanitized site — its
//! HAZARD half, which exists to prove an ambient redirect var still overrides
//! `-C`. It is a premise check rather than a call site, and must stay
//! unsanitized to have any teeth.
//!
//! The grep does NOT catch the orphan-audit *script spawn*, because that site
//! spawns a *shell script* that runs git internally rather than calling `git`
//! directly; a sweep for new call sites must consider both shapes. Re-run the
//! grep when adding a git call site: a new hit that is neither carved out above
//! nor one of the sites named above is a defect.

use std::path::Path;
use std::process::Command;

/// The sanitized variable set and the sanitizer itself, re-exported from
/// `reify_test_support::git_env`.
///
/// They live one crate DOWN because the dependency edge runs `reify-audit` ->
/// `reify-test-support` (see this crate's `Cargo.toml`) and that crate's own
/// `orphan_audit` spawn needs the same sanitizer, so only that direction lets
/// both sides share one item without inverting the edge. The re-export keeps
/// `reify_audit::git_env::{REPO_REDIRECT_VARS, sanitize, command}` intact for
/// every caller above it, and [`command`] below builds the `git -C <root>`
/// shape on top. Everything about the set itself is argued at the definition;
/// this module's tests cover only the wiring local here.
pub use reify_test_support::git_env::{REPO_REDIRECT_VARS, sanitize};

/// A pre-sanitized `git -C <root>` command.
///
/// The path of least resistance for every repo-targeting call site — prefer
/// this over hand-rolling `Command::new("git").arg("-C")`, which is how the
/// failure mode [`sanitize`] prevents reaches a new helper. That failure mode
/// is described where the sanitizer is defined, in
/// [`reify_test_support::git_env`].
pub fn command(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    sanitize(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::{REPO_REDIRECT_VARS, command};
    // The `(key, None)` removal encoding is read through the definition site's
    // helper rather than a local twin. Imported here rather than added to this
    // module's `pub use`, which is the production surface.
    use reify_test_support::git_env::removed_vars;
    use std::ffi::OsStr;
    use std::path::Path;

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

    /// [`command`] must actually APPLY the sanitization to the `-C <root>`
    /// shape it builds — the wiring that is local to this crate, and the one
    /// thing a dropped `sanitize(&mut cmd)` line in [`command`] would break
    /// without any other unit test here noticing.
    ///
    /// Self-referential by construction — it asserts the code removes exactly
    /// what the same constant lists, so it stays green through a DELETION from
    /// [`REPO_REDIRECT_VARS`]. The guard against that, and the proof that the
    /// removals defeat a real ambient redirect var, are both at the definition
    /// site (`reify_test_support::git_env`'s tests).
    #[test]
    fn command_removes_every_repo_redirect_var() {
        let cmd = command(Path::new("/some/root"));
        let removed = removed_vars(&cmd);
        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "command() must REMOVE `{var}` (env_remove -> `(key, None)`), not \
                 merely overwrite it; removals seen: {removed:?}"
            );
        }
    }
}
