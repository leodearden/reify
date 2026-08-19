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
//! This module is the ENTRY POINT: what the sanitizer removes, why, and the
//! measured evidence are all below, so a reader needs nothing from above the
//! edge. `reify_audit::git_env` re-exports both items and adds the
//! `git -C <root>` constructor plus the workspace rule and call-site sweep
//! enforcing its use — further reading, not a prerequisite.
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
//! (exit `Some(1)` where `Some(2)` was expected) from `reify-audit`'s
//! production reader path, and `git ["add", "."] exited Some(128)` from a
//! fixture helper colliding with the parent repo's `index.lock`.
//!
//! Note the asymmetry that makes this worth a single shared definition: the
//! redirect vars do not make git *fail* — they make it silently operate on a
//! different repository.
//!
//! # Measured
//!
//! An ambient redirect var beats BOTH cwd and an explicit `-C`:
//! `GIT_DIR=/tmp/gwt_a/.git GIT_WORK_TREE=/tmp/gwt_a git -C /tmp/gwt_b
//! rev-parse --show-toplevel` prints `/tmp/gwt_a`; dropping the two vars prints
//! `/tmp/gwt_b`. `sanitize_makes_dash_c_authoritative_against_real_git`, in this
//! module's tests, re-checks both halves of that against real git on every run
//! rather than resting on this paragraph.
//!
//! Under this project's warm-lane topology the ambient case is routine, not
//! hypothetical: tests execute inside `worktrees/_lane-NN` while a hook
//! environment may name a different checkout entirely. The blast radius is
//! whatever the caller does to the wrong repository — for `crate::orphan_audit`'s
//! script spawn, whose first action is
//! `REPO_ROOT="$(git rev-parse --show-toplevel)"`, an audit silently enumerating
//! the wrong tree rather than erroring.

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
/// strictly more collateral for no gain against the failure mode this module's
/// doc analyses above. Those are left untouched.
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
/// test of this set — here and above the dependency edge — iterates the set
/// itself to assert the code removes what it lists, so all of them stay green
/// through a deletion.
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
///
/// # What does NOT enforce this `pub`
///
/// No orphan-producer sweep reaches this item: the crate is excluded from that
/// audit at the script level (`scripts/audit-orphan-producers.sh`'s
/// `EXCLUDE_CRATES`, whose Rust mirror is `crate::orphan_audit`'s own
/// `EXCLUDE_CRATES`), and `discover_sources` skips any path containing that
/// segment, so a `// G-allow:` marker here would be dead ceremony. The `pub` is
/// justified by the re-export in `reify_audit::git_env` and its callers, not by
/// a gate: if that consumer goes away, demote or delete this item in the same
/// change, because no sweep will say so.
///
/// The exclusion assumes a test-support crate's publics are reached only from
/// test files. Two items now break that assumption — this one and
/// `crate::ignore_hygiene::extract_ignore_reason` — both reached from
/// `reify-audit`'s PRODUCTION path. Whether that warrants a dep-free
/// `reify-git-env` leaf crate the sweep does cover, or a narrower
/// `EXCLUDE_CRATES`, is tracked as follow-up ticket
/// `tkt_0RSN6D9381MSERHKK41GP3GE72` (filed from task 5657's review); decide it
/// there rather than re-deriving an answer at each new item.
pub fn sanitize(cmd: &mut Command) -> &mut Command {
    for var in REPO_REDIRECT_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// Collect the vars `cmd` has marked for REMOVAL. `std` encodes an `env_remove`
/// as a `(key, None)` pair in `get_envs()`; an overwrite would be
/// `(key, Some(value))`, and an untouched var does not appear at all.
///
/// Shared with `crate::orphan_audit`'s
/// `build_audit_command_removes_every_repo_redirect_var` rather than inlined at
/// each assertion site, so this crate's two assertion sites read that encoding
/// through one place. IN-CRATE only: `#[cfg(test)] pub(crate)` is invisible to
/// dependent crates, so `reify_audit::git_env`'s tests keep their own copy and
/// this is not the workspace's single reader of the `(key, None)` encoding.
#[cfg(test)]
pub(crate) fn removed_vars(cmd: &Command) -> Vec<String> {
    cmd.get_envs()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{REPO_REDIRECT_VARS, removed_vars, sanitize};
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
    /// [`REPO_REDIRECT_VARS`] from the other side of the dependency edge).
    const HOOK_EXPORTED_FLOOR: &[&str] = &["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"];

    /// The deletion guard for [`REPO_REDIRECT_VARS`], and the only test of that
    /// set anywhere that is NOT self-referential.
    ///
    /// Every other test of it — in this crate and above the dependency edge —
    /// iterates the same constant to assert the code removes what it lists, so
    /// dropping an entry keeps them all green. This test names each var
    /// independently, so a deletion has to be argued for here too. It lives in
    /// the crate that DEFINES the constant so an editor working only in
    /// `reify-test-support` gets that signal locally rather than from a reverse
    /// dependency.
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
    ///   `<root>`. If this half ever stops holding, this module is unnecessary
    ///   and should be deleted rather than trusted.
    /// - **FIX** — applying the same vars and THEN [`sanitize`] reports
    ///   `<root>`.
    ///
    /// It belongs at the DEFINITION site because this crate's own
    /// `crate::orphan_audit` spawns depend on the vars actually being DEFEATED,
    /// not merely on the removals being recorded. Left in a reverse dependency,
    /// that proof could be refactored away without anything here announcing the
    /// loss, leaving the production-path spawns covered by metadata assertions
    /// alone.
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
        // Graceful-skip convention this module follows from
        // `crate::orphan_audit`'s tests: a git-less image must skip, not fail.
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("git_env: skipping real-git test — git not available");
            return;
        }

        let target = crate::temp_dirs::prefixed_tempdir("git-env-target-");
        let decoy = crate::temp_dirs::prefixed_tempdir("git-env-decoy-");
        for dir in [target.path(), decoy.path()] {
            // The `git -C <root>` shape `reify_audit::git_env::command` builds,
            // hand-rolled because that constructor lives one crate UP.
            let mut init = Command::new("git");
            init.arg("-C")
                .arg(dir)
                .args(["init", "--initial-branch=main"]);
            sanitize(&mut init);
            let status = init.status().expect("git init failed to spawn");
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
             this module's reason to exist should be re-examined, not patched."
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
}
