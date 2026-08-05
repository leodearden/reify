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
//! **Resolved non-central case:** `reify-test-support`'s
//! `scripts/audit-orphan-producers.sh` spawn
//! (`crates/reify-test-support/src/orphan_audit.rs`) sanitizes the same
//! [`REPO_REDIRECT_VARS`] set. It reaches the sanitizer directly rather than
//! calling into this crate, because the dependency edge runs `reify-audit` ->
//! `reify-test-support` and cannot be inverted. **As of task 5657** that is no
//! longer a duplication at all: the const and the fn were moved DOWN into
//! `reify_test_support::git_env`, the workspace's single definition, and this
//! module re-exports them. `orphan_audit.rs` and [`command`] therefore share
//! ONE sanitizer, with no drift surface left to pin — the duplicate copy and
//! the test that held the two lists equal are both gone. Without the
//! sanitization, the script's first action,
//! `REPO_ROOT="$(git rev-parse --show-toplevel)"`, would have been vulnerable
//! to exactly the failure mode documented below: an ambient
//! `GIT_DIR`/`GIT_WORK_TREE` beats BOTH cwd and `-C` (measured:
//! `GIT_DIR=/tmp/gwt_a/.git GIT_WORK_TREE=/tmp/gwt_a git -C /tmp/gwt_b
//! rev-parse --show-toplevel` prints `/tmp/gwt_a`; dropping the two vars
//! prints `/tmp/gwt_b`), and under this project's warm-lane topology tests
//! routinely execute inside `worktrees/_lane-NN` while an ambient hook
//! environment may name a different checkout. The blast radius would have
//! been an orphan audit silently enumerating the wrong tree rather than
//! erroring.
//!
//! **As of task 5698**, the shared sanitizer covers a SECOND `orphan_audit.rs`
//! site too: `child_repo_root`, a `git rev-parse --show-toplevel` probe run
//! against the caller-supplied `repo_root` before the script spawn. Where the
//! sanitization above stops an ambient redirect var from overriding an
//! otherwise-correct `repo_root`, this probe covers a channel sanitization
//! cannot: a `repo_root` that was wrong to begin with — a `CARGO_MANIFEST_DIR`
//! `.parent()` walk landing on the wrong level, a `.git` file pointing at
//! another checkout, a symlinked or embedded checkout. It asserts the
//! property the sanitization is meant to establish — that the child resolves
//! the SAME work tree the caller intended — by comparing the caller's
//! `repo_root` against what the (sanitized) child itself resolves; a
//! disagreement is now a hard panic. That closes the "blast radius" named
//! above for this second channel too: without the probe, a wrong `repo_root`
//! would have been an orphan audit silently enumerating the wrong tree rather
//! than erroring, exactly like the unsanitized-redirect-var case above.
//!
//! ## Sweep status
//!
//! `git grep -n 'Command::new("git")' -- '*.rs'` over the whole workspace
//! returns, besides this module and prose references to it, the `git
//! --version` probes named above. **As of task 5698**, the same grep also
//! returns two more accounted-for, non-exempt sites, both in
//! `crates/reify-test-support/src/orphan_audit.rs`: the `child_repo_root`
//! probe described above, and a `git init` test fixture in
//! `wrong_tree_with_real_scope_panics` (builds a decoy repo to audit; "a
//! fixture helper that shells `git init` into a tempdir" is literally the
//! example the rule at the top of this module names). Neither is a
//! `--version` probe, so neither qualifies for the carve-out above; both are
//! legitimate because both are routed through the shared
//! [`reify_test_support::git_env::sanitize`] — the same one [`sanitize`] here
//! re-exports — for the same dependency-direction reason the script spawn is
//! (see "Resolved non-central case" above). Every OTHER repo-targeting
//! *git* site — the three `RealGitOps` methods (`spawn_once`,
//! `is_gitignored`, `is_ancestor`) and the six fixture helpers in
//! `tests/cli.rs` and `tests/real_git_ops.rs` — is routed through here. The
//! grep does NOT catch the orphan-audit *script spawn* described above,
//! because that site spawns a *shell script* that runs git internally rather
//! than calling `git` directly; a sweep for new call sites must consider both
//! shapes. Re-run that grep when adding a git call site: a new hit that is
//! neither a `--version` probe nor one of the two `orphan_audit.rs` sites
//! named above is a defect.
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
//! Argued once, on [`REPO_REDIRECT_VARS`]'s own doc in
//! `reify_test_support::git_env` — the crate that now defines it. Restating it
//! here would rebuild, in prose, exactly the drift surface task 5657 removed
//! for the constant itself.

use std::path::Path;
use std::process::Command;

/// The sanitized variable set and the sanitizer itself, re-exported from
/// `reify_test_support::git_env`.
///
/// The definitions live one crate DOWN because the dependency edge runs
/// `reify-audit` -> `reify-test-support` (see this crate's `Cargo.toml`), and
/// `reify-test-support`'s own `orphan_audit` spawn needs the same sanitizer —
/// so only that direction lets both sides share one item without inverting the
/// edge. The re-export keeps `reify_audit::git_env::{REPO_REDIRECT_VARS,
/// sanitize, command}` intact for every caller above it, and [`command`] below
/// builds the `git -C <root>` shape on top.
///
/// See `reify_test_support::git_env` for why removal rather than assignment,
/// and why an enumerated set rather than a wholesale `GIT_*` clear. The
/// deletion guard for the set (`repo_redirect_vars_covers_the_removal_floor`)
/// lives there too, next to the definition it guards. What stays in this
/// module's tests below is what is genuinely local to this crate: the
/// `git -C <root>` shape [`command`] builds, and the real-git behavioural test
/// that proves the sanitization actually beats an ambient redirect var.
pub use reify_test_support::git_env::{REPO_REDIRECT_VARS, sanitize};

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

    /// Both entry points — the `git -C <root>` shape from [`command`] and a
    /// caller-built command that opted in via [`sanitize`] — must produce the
    /// same removals. One test rather than two: `command` is a two-line
    /// wrapper around `sanitize`, so a second near-identical test adds no
    /// independent signal beyond "sanitize is reachable directly", which this
    /// one covers by running both through the same loop. That reachability is
    /// the part this test contributes now that the items are re-exported: it
    /// fails to compile if the re-export stops carrying either name.
    ///
    /// Self-referential by construction — it asserts the code removes exactly
    /// what the same constant lists, so it stays green through a DELETION from
    /// [`REPO_REDIRECT_VARS`]. The guard against that is
    /// `repo_redirect_vars_covers_the_removal_floor`, which lives with the
    /// definition in `reify_test_support::git_env`.
    #[test]
    fn both_entry_points_remove_every_repo_redirect_var() {
        // A caller that needs a non-`-C` shape can still opt in.
        let mut opted_in = Command::new("git");
        opted_in.arg("--version");
        sanitize(&mut opted_in);

        for (label, cmd) in [
            ("command()", command(Path::new("/some/root"))),
            ("sanitize() on a caller-built command", opted_in),
        ] {
            let removed = removed_vars(&cmd);
            for var in REPO_REDIRECT_VARS {
                assert!(
                    removed.iter().any(|r| r == var),
                    "{label} must REMOVE `{var}` (env_remove -> `(key, None)`), not \
                     merely overwrite it; removals seen: {removed:?}"
                );
            }
        }
    }

    /// Close the loop on REAL git behaviour rather than on `Command` metadata.
    ///
    /// Every other test here inspects what [`sanitize`] *records*. That is
    /// necessary but not sufficient: those tests would stay green if git
    /// ignored the vars, or if a later `env_remove` failed to override an
    /// earlier value. This one spawns git twice against the same `-C <root>`
    /// and asserts both halves of the claim:
    ///
    /// - **HAZARD** — with the hook's exported vars present,
    ///   `git -C <root> rev-parse --show-toplevel` reports the DECOY, not
    ///   `<root>`. If this half ever stops holding, this whole module is
    ///   unnecessary and should be deleted rather than trusted.
    /// - **FIX** — applying the same vars and THEN [`sanitize`] reports
    ///   `<root>`.
    ///
    /// Setting the vars via `.env(..)` and then sanitizing is a faithful stand-in
    /// for an ambient environment, not a weaker one: `std` records `env_remove`
    /// as a `(key, None)` entry in the command's env map, and the final child
    /// environment is built by applying those entries over the inherited
    /// `env::vars_os()`. A `None` entry deletes an inherited value by exactly
    /// the same mechanism it deletes an explicitly-set one.
    ///
    /// The poison reaches the CHILD only. This test never touches its own
    /// process environment — `std::env::set_var` is process-global and would
    /// race sibling tests under `cargo test`'s thread-per-test model.
    #[test]
    fn sanitize_makes_dash_c_authoritative_against_real_git() {
        // Graceful skip if git is absent, matching the probe convention in
        // tests/g_allow_repo_wide_hard_gate.rs.
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("git_env: skipping real-git test — git not available");
            return;
        }

        let target = tempfile::tempdir().expect("target repo tempdir");
        let decoy = tempfile::tempdir().expect("decoy repo tempdir");
        for dir in [target.path(), decoy.path()] {
            let status = command(dir)
                .args(["init", "--initial-branch=main"])
                .status()
                .expect("git init failed to spawn");
            assert!(
                status.success(),
                "git init {dir:?} exited {:?}",
                status.code()
            );
        }

        // `--show-toplevel` canonicalizes, so compare against canonical paths
        // (TMPDIR is a symlink on some platforms).
        let canonical = |p: &Path| {
            std::fs::canonicalize(p)
                .expect("canonicalize repo path")
                .to_string_lossy()
                .into_owned()
        };
        let decoy_git_dir = decoy.path().join(".git");

        let toplevel = |cmd: &mut Command| -> String {
            let out = cmd
                .args(["rev-parse", "--show-toplevel"])
                .output()
                .expect("git rev-parse failed to spawn");
            assert!(
                out.status.success(),
                "git rev-parse exited {:?}; stderr: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        // HAZARD: an unsanitized `-C <target>` obeys the poison instead.
        let mut hazard = Command::new("git");
        hazard
            .arg("-C")
            .arg(target.path())
            .env("GIT_DIR", &decoy_git_dir)
            .env("GIT_WORK_TREE", decoy.path())
            .env("GIT_INDEX_FILE", decoy_git_dir.join("index"));
        assert_eq!(
            toplevel(&mut hazard),
            canonical(decoy.path()),
            "PREMISE CHECK: a hook's exported git env must still override an \
             explicit `-C <root>`. If this fails, git's behaviour changed and \
             this module's reason to exist should be re-examined, not patched."
        );

        // FIX: the same poison, then sanitize -> `-C <target>` wins.
        let mut fixed = Command::new("git");
        fixed
            .arg("-C")
            .arg(target.path())
            .env("GIT_DIR", &decoy_git_dir)
            .env("GIT_WORK_TREE", decoy.path())
            .env("GIT_INDEX_FILE", decoy_git_dir.join("index"));
        sanitize(&mut fixed);
        assert_eq!(
            toplevel(&mut fixed),
            canonical(target.path()),
            "sanitize() must make `-C <root>` authoritative against real git, not \
             merely record removals on the Command"
        );
    }
}
