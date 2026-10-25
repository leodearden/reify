//! The workspace's single constructor and sanitizer for git invocations that target a specific repository.

#[cfg(test)]
mod tests {
    use super::{REPO_REDIRECT_VARS, command, removed_vars, sanitize};
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
    /// case, and exactly what `reify-audit`'s integration-test replay harness
    /// poisons with (`crates/reify-audit/tests/common/git_env.rs`, which
    /// independently asserts its own poison set is contained in
    /// [`REPO_REDIRECT_VARS`] from the consumer's side).
    const HOOK_EXPORTED_FLOOR: &[&str] = &["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"];

    /// The deletion guard for [`REPO_REDIRECT_VARS`], and the only test of that
    /// set anywhere that is NOT self-referential.
    ///
    /// Every other test of it — here and in consumer crates — iterates the
    /// same constant to assert the code removes what it lists, so dropping an
    /// entry keeps them all green. This test names each var independently, so
    /// a deletion has to be argued for here too. It lives in the crate that
    /// DEFINES the constant so an editor working only in this crate gets that
    /// signal locally rather than from a reverse dependency.
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

    /// [`sanitize`]'s core behaviour: every [`REPO_REDIRECT_VARS`] entry is
    /// marked for REMOVAL rather than overwritten or left inherited.
    ///
    /// Self-referential by construction — it asserts the fn removes exactly
    /// what the same constant lists, so it stays green through a DELETION from
    /// [`REPO_REDIRECT_VARS`]; `repo_redirect_vars_covers_the_removal_floor`
    /// above is the guard against that, not this.
    #[test]
    fn sanitize_removes_every_repo_redirect_var() {
        let mut cmd = Command::new("git");
        sanitize(&mut cmd);

        let removed = removed_vars(&cmd);

        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "sanitize() must REMOVE `{var}` (env_remove -> `(key, None)`), \
                 not merely overwrite it or leave it inherited; removals seen: \
                 {removed:?}"
            );
        }
    }

    /// The `&mut Command` half of [`sanitize`]'s contract: callers needing a
    /// shape other than `git -C <root>` chain off the sanitized command.
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
    /// shape it builds — the one thing a dropped `sanitize(&mut cmd)` line in
    /// [`command`] would break without any other unit test noticing.
    ///
    /// Self-referential by construction, like
    /// `sanitize_removes_every_repo_redirect_var`.
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

    /// Close the loop on REAL git behaviour rather than on `Command` metadata.
    ///
    /// The tests above inspect what [`sanitize`] *records*. That is necessary
    /// but not sufficient: they would all stay green if git ignored the redirect
    /// vars entirely, or if a later `env_remove` failed to override an earlier
    /// value. This one spawns git twice against the same `-C <root>` and asserts
    /// both halves of the claim:
    ///
    /// - **HAZARD** — with the hook's exported vars present,
    ///   `git -C <root> rev-parse --show-toplevel` reports the DECOY, not
    ///   `<root>`. If this half ever stops holding, this crate is unnecessary
    ///   and should be deleted rather than trusted.
    /// - **FIX** — applying the same vars and THEN [`sanitize`] reports
    ///   `<root>`.
    ///
    /// Both halves hand-roll `-C` rather than calling [`command`]: the poison
    /// must land in a controlled order relative to sanitizing, and [`command`]
    /// sanitizes first, so a later `.env(..)` would re-poison it.
    ///
    /// Setting the vars via `.env(..)` and then sanitizing is a faithful
    /// stand-in for an ambient environment, not a weaker one: `std` records
    /// `env_remove` as a `(key, None)` entry in the command's env map, and the
    /// final child environment is built by applying those entries over the
    /// inherited `env::vars_os()`. A `None` entry deletes an inherited value by
    /// exactly the same mechanism it deletes an explicitly-set one.
    ///
    /// The poison reaches the CHILD only. This test never touches its own
    /// process environment — `std::env::set_var` is process-global and would
    /// race sibling tests under `cargo test`'s thread-per-test model.
    #[test]
    fn sanitize_makes_dash_c_authoritative_against_real_git() {
        // A git-less image must skip, not fail.
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("reify-git-env: skipping real-git test — git not available");
            return;
        }

        let target = tempdir("git-env-target-");
        let decoy = tempdir("git-env-decoy-");
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

        let poison = |cmd: &mut Command| {
            cmd.arg("-C")
                .arg(target.path())
                .env("GIT_DIR", &decoy_git_dir)
                .env("GIT_WORK_TREE", decoy.path())
                .env("GIT_INDEX_FILE", decoy_git_dir.join("index"));
        };

        // HAZARD: an unsanitized `-C <target>` obeys the poison instead.
        let mut hazard = Command::new("git");
        poison(&mut hazard);
        assert_eq!(
            toplevel(&mut hazard),
            canonical(decoy.path()),
            "PREMISE CHECK: a hook's exported git env must still override an \
             explicit `-C <root>`. If this fails, git's behaviour changed and \
             this crate's reason to exist should be re-examined, not patched."
        );

        // FIX: the same poison, then sanitize -> `-C <target>` wins.
        let mut fixed = Command::new("git");
        poison(&mut fixed);
        sanitize(&mut fixed);
        assert_eq!(
            toplevel(&mut fixed),
            canonical(target.path()),
            "sanitize() must make `-C <root>` authoritative against real git, \
             not merely record removals on the Command"
        );
    }

    /// A self-deleting temp dir whose `prefix` attributes a dir that survives
    /// a SIGKILL (which `Drop` cannot cover) to this crate's tests. Bind the
    /// guard to a named local — `tempdir(..).path().to_path_buf()` compiles
    /// but deletes the dir at the end of that statement.
    fn tempdir(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .unwrap_or_else(|e| panic!("create temp dir with prefix {prefix:?}: {e}"))
    }
}
