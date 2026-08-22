//! The single constructor for git invocations that target a SPECIFIC
//! repository.
//!
//! [`REPO_REDIRECT_VARS`] and [`sanitize`] are defined one crate down, in
//! `reify_test_support::git_env` (the dependency direction); see there for the
//! failure mode and the evidence. This module adds the `git -C <root>` shape
//! [`command`] builds, the workspace rule that every repo-targeting invocation
//! be built through it, and how to sweep for breaches of that rule.
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
//! repository, so no redirect var can reach it. That is the only exemption,
//! and it rests on git's own semantics rather than on how a command happens to
//! be used today — anything carrying `-C`, a repository path, or an implicit
//! cwd repository is NOT exempt, including a `Command` that nothing currently
//! spawns. "It is only read back through `get_envs()`" stops being true the
//! day someone appends a `.status()`, so it is not a line worth drawing.
//!
//! **Sites below the dependency edge:** `reify-test-support` cannot call into
//! this crate — the edge runs `reify-audit` -> `reify-test-support` — so it
//! hand-rolls the `-C <root>` shape and routes its own repo-targeting spawns
//! through [`reify_test_support::git_env::sanitize`] directly. That satisfies
//! the rule through the same single sanitizer, and each such site carries its
//! own argument at its own definition. The workspace's one deliberate breach
//! lives there too: the HAZARD half of that module's real-git proof stays
//! unsanitized precisely to demonstrate that an ambient redirect var still
//! overrides `-C`. It is a premise check rather than a call site, and would
//! have no teeth sanitized.
//!
//! ## Sweeping for new call sites
//!
//! Two shapes reach git and a sweep must consider both.
//! `git grep -n 'Command::new("git")' -- '*.rs'` finds direct invocations; it
//! does NOT find a spawn of a shell script that runs git internally, which is
//! the shape of `reify-test-support`'s orphan-audit spawn (whose first action
//! is `REPO_ROOT="$(git rev-parse --show-toplevel)"`). A site of either shape
//! that is neither the `--version` carve-out nor routed through [`command`] or
//! the shared sanitizer is a defect.
//!
//! Re-run that sweep when adding a call site rather than trusting a list here:
//! an enumerated census of call sites in a doc comment rots the moment one is
//! renamed, and silently reads as authoritative while it does. Within this
//! crate the rule currently holds with no exceptions — production `RealGitOps`
//! and the `tests/` fixture helpers alike build through [`command`].

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
