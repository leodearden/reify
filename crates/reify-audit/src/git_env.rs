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
//! **Known gap, stated as a gap:** `reify-test-support`'s
//! `scripts/audit-orphan-producers.sh` spawn
//! (`crates/reify-test-support/src/orphan_audit.rs`) is NOT sanitized. It
//! runs `.current_dir(repo_root)` against the LIVE repository, where
//! `repo_root` is derived from `CARGO_MANIFEST_DIR` — i.e. the checkout the
//! crate was compiled in — while the script's first action is
//! `REPO_ROOT="$(git rev-parse --show-toplevel)"`. Leaving it alone rests on
//! the premise that an ambient `GIT_DIR` names that same checkout, and that
//! premise is ASSUMED, not enforced. Two facts bound the risk:
//!
//! - Measured on this worktree: an ambient `GIT_DIR`/`GIT_WORK_TREE` beats
//!   BOTH cwd and `-C`. `GIT_DIR=/tmp/gwt_a/.git GIT_WORK_TREE=/tmp/gwt_a
//!   git -C /tmp/gwt_b rev-parse --show-toplevel` prints `/tmp/gwt_a`;
//!   dropping the two vars prints `/tmp/gwt_b`.
//! - Under this project's warm-lane topology, tests routinely execute inside
//!   `worktrees/_lane-NN` while an ambient hook environment may name a
//!   different checkout — which is exactly the assumption this module argues
//!   against making.
//!
//! The blast radius is bounded but real: an orphan audit that enumerates the
//! wrong tree rather than erroring. That file is outside this change's lock
//! scope, so it is recorded here rather than fixed here. Closing it costs one
//! [`sanitize`] call and cannot regress anything, because the script
//! rediscovers its own root from cwd; the alternative is to assert the
//! premise (compare the child's `rev-parse --show-toplevel` against
//! `repo_root` and fail loudly on mismatch).
//!
//! ## Sweep status
//!
//! `git grep -n 'Command::new("git")' -- '*.rs'` over the whole workspace
//! returns, besides this module and prose references to it, exactly the three
//! `git --version` probes named above. Every repo-targeting *git* site — the
//! three `RealGitOps` methods (`spawn_once`, `is_gitignored`, `is_ancestor`)
//! and the six fixture helpers in `tests/cli.rs` and `tests/real_git_ops.rs` —
//! is routed through here. The grep does NOT catch the orphan-audit gap above,
//! because that site spawns a *shell script* that runs git internally; a
//! sweep for new call sites must consider both shapes. Re-run that grep when
//! adding a git call site: a new hit that is not a `--version` probe is a
//! defect.
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
    /// case, and exactly what the integration-test replay harness poisons with
    /// (`tests/common/git_env.rs::hook_git_env`).
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

    /// Both entry points — the `git -C <root>` shape from [`command`] and a
    /// caller-built command that opted in via [`sanitize`] — must produce the
    /// same removals. One test rather than two: `command` is a two-line
    /// wrapper around `sanitize`, so a second near-identical test adds no
    /// independent signal beyond "sanitize is reachable directly", which this
    /// one covers by running both through the same loop.
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

    /// The deletion guard. `both_entry_points_remove_every_repo_redirect_var`
    /// is self-referential — it asserts the code removes exactly what the same
    /// constant lists, so dropping an entry from [`REPO_REDIRECT_VARS`] would
    /// keep it green. This test names each var independently, so a deletion
    /// has to be argued for here too.
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
