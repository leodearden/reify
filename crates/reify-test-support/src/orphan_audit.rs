use crate::git_env::sanitize;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build the (unexecuted) `audit-orphan-producers.sh` command: scope/format
/// args, `current_dir`, and [`sanitize`]. Split out of [`run_orphan_audit`] so
/// the sanitization can be asserted on directly via `get_envs()` — see
/// `build_audit_command_removes_every_repo_redirect_var` below — without
/// spawning the script.
fn build_audit_command(script: &Path, scope: &str, repo_root: &Path) -> Command {
    let mut cmd = Command::new(script);
    cmd.args(["--scope", scope, "--quiet", "--format", "json"])
        .current_dir(repo_root);
    sanitize(&mut cmd);
    cmd
}

/// The three possible outcomes of running the orphan-producer audit against a
/// scope.
///
/// [`run_orphan_audit`]'s `Option<serde_json::Value>` return collapses
/// `ExcludedScope` and `EnvUnavailable` to `None`, so every existing call
/// site's `let Some(..) else { return; }`/`else { return; }` shape keeps
/// compiling and behaving identically.
///
/// Deliberately module-private rather than re-exported: as of task 5698 no
/// caller needs to discriminate a legitimate skip from a hard failure — all
/// 9 existing external call sites use [`run_orphan_audit`] and tolerate both
/// as `None` — so this finer-grained outcome type stays private (reachable
/// from this module's own tests via `use super::*`) until a real external
/// consumer appears. Promote it and `run_orphan_audit_detailed` back to
/// `pub` at that point rather than speculatively ahead of one.
#[derive(Debug)]
enum OrphanAudit {
    /// A well-formed JSON envelope was produced.
    Envelope(serde_json::Value),
    /// `scope`'s crate segment is a literal member of `EXCLUDE_CRATES` — a
    /// legitimate, non-failing outcome that the script itself encodes as
    /// empty stdout.
    ExcludedScope,
    /// The environment cannot satisfy the script's prerequisites (missing
    /// `python3`/`git`, the script itself absent, or `repo_root` not inside a
    /// git work tree). Carries a short human-readable reason for logging.
    ///
    /// `#[allow(dead_code)]`: the payload is read only from
    /// `run_orphan_audit_on_excluded_crate_is_named_not_erased` in
    /// `#[cfg(test)]`. Now that this enum is module-private (task 5698
    /// amendment pass, review finding #5), it no longer gets the `pub`-item
    /// dead-code exemption, and the plain (non-test) library build has no
    /// other reader of this field.
    #[allow(dead_code)]
    EnvUnavailable(&'static str),
}

/// Crate names excluded from the orphan-producer audit — a Rust copy of
/// `scripts/audit-orphan-producers.sh:92`'s `EXCLUDE_CRATES = {...}` set
/// literal (the source of truth; this copy exists because the script cannot
/// be consulted at Rust compile/run time without a python round-trip).
///
/// Pinned against the script by
/// `exclude_crates_const_matches_audit_script_declaration`: a divergence
/// between the two fails `cargo test` rather than resting on a "keep in
/// sync" comment. That pin is the duplication-pinning pattern this codebase
/// reaches for whenever a value genuinely cannot be single-sourced across a
/// language boundary — and it is the last resort, not the default: where a
/// value CAN be single-sourced it should be, as [`sanitize`]'s own move into
/// `crate::git_env` did.
const EXCLUDE_CRATES: &[&str] = &["reify-test-support"];

/// True when `scope`'s crate segment (the path component immediately after
/// `crates`) is a LITERAL member of [`EXCLUDE_CRATES`].
///
/// Deliberately literal-only, not glob-aware — and needs no special-casing to
/// be so: a segment containing a glob metacharacter (e.g. the `reify-*` in
/// the default `crates/reify-*/src` scope) is never literally equal to a
/// single crate name, so plain set membership already returns `false` for
/// it. That is exactly right for the five call sites in this workspace that
/// pass a glob scope: their expansion always includes non-excluded crates,
/// so treating the glob itself as "excluded" would be wrong.
fn scope_is_excluded_crate(scope: &str) -> bool {
    scope
        .split('/')
        .skip_while(|&seg| seg != "crates")
        .nth(1)
        .is_some_and(|seg| EXCLUDE_CRATES.contains(&seg))
}

/// Resolve the git work tree that a child process spawned with
/// `.current_dir(repo_root)` would itself compute via `git rev-parse
/// --show-toplevel` — i.e. what `audit-orphan-producers.sh:66`'s own
/// `REPO_ROOT="$(git rev-parse --show-toplevel)"` will resolve to for this
/// child. Routed through the SAME [`sanitize`] the script spawn uses, so this
/// probe faithfully reproduces the child's exact view.
///
/// This does NOT re-test that [`sanitize`] works — it tests the premise
/// [`sanitize`] is supposed to establish: that the child resolves the SAME
/// repository the caller asked it to scan. That is why the probe still has
/// teeth despite reusing the spawn's own sanitizer: it independently catches
/// an incomplete [`crate::git_env::REPO_REDIRECT_VARS`] list, a `.git` file
/// pointing at another checkout, a symlinked or embedded checkout, or a
/// `CARGO_MANIFEST_DIR` `.parent()` walk that lands on the wrong level — any
/// of which would make the real script silently scan a different tree than
/// the one the caller specified.
///
/// Returns `Err(`[`ChildRepoRootFailure::NoRepository`]`)` only when
/// `repo_root` is genuinely not inside a git work tree at all — the ONE
/// outcome callers should treat as a graceful skip. Every other failure
/// (see [`ChildRepoRootFailure::Other`]'s doc) returns `Err(Other(..))` and
/// must NOT be treated as a graceful skip: a real repository is expected to
/// exist in those cases, and silently skipping would reintroduce exactly the
/// silent-green failure mode this task exists to close.
fn child_repo_root(repo_root: &Path) -> Result<PathBuf, ChildRepoRootFailure> {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--show-toplevel"])
        .current_dir(repo_root);
    sanitize(&mut cmd);

    // A spawn failure (e.g. `git` absent from PATH entirely) leaves no way to
    // tell whether a repository exists here — fold it into the
    // graceful-skip bucket alongside "genuinely no repository", consistent
    // with the python3/git presence probes one layer up in
    // run_orphan_audit_detailed.
    let output = cmd
        .output()
        .map_err(|_| ChildRepoRootFailure::NoRepository)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Only a truly repo-less directory's `git rev-parse --show-toplevel`
        // includes this exact phrase, e.g. "fatal: not a git repository (or
        // any of the parent directories): .git". A corrupt `.git` file
        // instead produces the shorter "fatal: not a git repository:
        // <path>" (no "parent directories" wording), and a dubious-
        // ownership rejection produces "detected dubious ownership in
        // repository" — neither contains this phrase, so both correctly
        // fall through to `Other` below rather than being misread as "no
        // repository here". Verified empirically for the repo-less and
        // corrupt-gitdir shapes; see
        // `child_repo_root_plain_non_git_dir_is_no_repository` and
        // `child_repo_root_corrupt_git_file_is_other_not_no_repository`.
        if stderr.contains("(or any of the parent directories)") {
            return Err(ChildRepoRootFailure::NoRepository);
        }
        return Err(ChildRepoRootFailure::Other(format!(
            "git rev-parse --show-toplevel exited {:?}; stderr: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(ChildRepoRootFailure::Other(format!(
            "git rev-parse --show-toplevel exited {:?} but produced empty stdout",
            output.status
        )));
    }
    Ok(PathBuf::from(trimmed))
}

/// Failure detail from [`child_repo_root`]'s `git rev-parse --show-toplevel`
/// probe. Distinguishes "genuinely no repository here" — the ONLY case
/// [`run_orphan_audit_at`] should treat as a graceful environmental skip —
/// from every other failure, where a real, usable repository is expected to
/// exist and the probe should panic instead of silently skipping.
///
/// Review finding (task 5698 amendment pass): a non-zero exit from `git
/// rev-parse --show-toplevel` is not only "no `.git` anywhere in the parent
/// chain". Git also exits non-zero for "detected dubious ownership in
/// repository" (a real condition under this project's shared
/// warm-lane/orchestrator ownership topology), for a corrupt `.git` file
/// pointing at a missing gitdir, or for a `safe.directory` misconfiguration —
/// and in every one of those cases a real repository DOES exist, so the
/// audit should have run. Collapsing them into a graceful skip would
/// silently reintroduce the exact silent-green failure mode this task exists
/// to close, and would do so by short-circuiting BEFORE the empty-stdout
/// hard-failure branch that would otherwise have caught it.
#[derive(Debug)]
enum ChildRepoRootFailure {
    /// `git` could not be spawned at all, or exited unsuccessfully with
    /// git's "no repository anywhere in the parent chain" diagnosis
    /// (typically exit 128). Both mean there is genuinely nothing here to
    /// scan.
    NoRepository,
    /// `git rev-parse --show-toplevel` failed, or produced unusable stdout
    /// despite exiting successfully, for some OTHER reason. Carries a
    /// description (exit status and/or stderr) for the caller's panic
    /// message.
    Other(String),
}

/// Injectable seam behind [`run_orphan_audit`]: takes `script` and
/// `repo_root` as parameters rather than resolving them internally, so tests
/// can point it at a decoy tree, a subdirectory root, or a nonexistent script
/// without touching the real repo or the real script location. Mirrors why
/// [`build_audit_command`] was split out one layer down.
fn run_orphan_audit_at(script: &Path, repo_root: &Path, scope: &str) -> OrphanAudit {
    // Graceful skip: check the script itself exists. (Moved here from
    // run_orphan_audit_detailed in task 5698 step 6 so this seam is
    // self-contained when called directly with a script path that caller
    // never probed — see missing_script_is_env_unavailable.)
    if !script.exists() {
        eprintln!(
            "scripts/audit-orphan-producers.sh not found at {:?}; skipping",
            script
        );
        return OrphanAudit::EnvUnavailable("audit-orphan-producers.sh not found on disk");
    }

    // Repo-root premise probe (task 5698 step 6): assert that a child spawned
    // in `repo_root` would itself resolve the SAME work tree, before spending
    // a real spawn on it. This is a SECOND, more specific diagnosis than the
    // empty-stdout panic below: that panic is the primary closure of "wrong
    // tree" and fires no matter how the wrong tree was reached, but it stays
    // silent when the scope is in EXCLUDE_CRATES (empty stdout is legitimate
    // there). This probe additionally catches a wrong tree scanned under an
    // EXCLUDE_CRATES scope, which the empty-stdout branch alone cannot.
    match child_repo_root(repo_root) {
        Err(ChildRepoRootFailure::NoRepository) => {
            eprintln!(
                "repo_root {repo_root:?} is not inside a git work tree; skipping orphan \
                 audit for scope {scope:?}"
            );
            return OrphanAudit::EnvUnavailable("repo root is not a git work tree");
        }
        Err(ChildRepoRootFailure::Other(detail)) => {
            panic!(
                "git rev-parse --show-toplevel could not resolve repo_root {repo_root:?} \
                 for a reason other than \"no repository here\" — a real, usable \
                 repository is expected to exist (e.g. this project's shared \
                 warm-lane/orchestrator topology can trigger a dubious-ownership \
                 safe.directory rejection, or the gitdir pointer is corrupt), so this is \
                 a hard failure rather than a graceful skip for scope {scope:?}: {detail}"
            );
        }
        Ok(resolved) => {
            let resolved_canon = resolved.canonicalize().unwrap_or_else(|e| {
                panic!("failed to canonicalize child-resolved repo root {resolved:?}: {e}")
            });
            let repo_root_canon = repo_root.canonicalize().unwrap_or_else(|e| {
                panic!("failed to canonicalize repo_root argument {repo_root:?}: {e}")
            });
            if resolved_canon != repo_root_canon {
                panic!(
                    "audit-orphan-producers.sh would run against a DIFFERENT repository \
                     than requested: repo_root argument was {repo_root:?} (canonicalized \
                     {repo_root_canon:?}), but the child's own `git rev-parse \
                     --show-toplevel` resolves to {resolved:?} (canonicalized \
                     {resolved_canon:?}). The audit would have silently enumerated the \
                     wrong checkout for scope {scope:?} — never a legitimate outcome."
                );
            }
        }
    }

    // Sanitize repo-redirect git env vars before spawning (via
    // build_audit_command -> crate::git_env::sanitize): the script's first
    // action is `REPO_ROOT="$(git rev-parse --show-toplevel)"`, so an ambient
    // redirect var, not this call's `repo_root`, would decide which checkout it
    // enumerates. Why such a var beats both cwd and an explicit `-C`, and what
    // that costs: `crate::git_env`'s module doc, where the sanitizer is
    // defined.
    let output = build_audit_command(script, scope, repo_root)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke audit-orphan-producers.sh: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if stdout.trim().is_empty() {
        if scope_is_excluded_crate(scope) {
            eprintln!(
                "audit-orphan-producers.sh produced empty output for scope {scope:?} \
                 — scope is in EXCLUDE_CRATES (exit status: {:?})",
                output.status
            );
            return OrphanAudit::ExcludedScope;
        }
        // Empty stdout for a scope that is NOT in EXCLUDE_CRATES is never a
        // legitimate outcome: it means the orphan-producer pin for this
        // scope has been silently disabled. Known causes: a wrong-tree
        // redirect (repo_root resolves to a checkout with no matching
        // sources), an EXCLUDE_CRATES edit, a crate rename, or a
        // discover_sources refactor in the script itself. A silent `None`
        // here is exactly the bug task 5698 closes — every caller would keep
        // reporting green while the pin does nothing.
        panic!(
            "audit-orphan-producers.sh produced empty output for scope {scope:?}, which \
             is NOT in EXCLUDE_CRATES — this is never a legitimate outcome for a real \
             scope. repo_root: {repo_root:?}; exit status: {:?}; stderr: {stderr:?}",
            output.status
        );
    }

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "audit-orphan-producers.sh output was not valid JSON: {e}\n\
             status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        )
    });

    OrphanAudit::Envelope(parsed)
}

/// Run `scripts/audit-orphan-producers.sh` against a specific crate scope and
/// return the parsed JSON envelope.
///
/// A thin `Option`-collapsing wrapper over the private `run_orphan_audit_detailed`,
/// kept with this exact signature so every existing call site's
/// `else { return; }`/`let Some(..) else` shape keeps compiling and behaving
/// identically. `run_orphan_audit_detailed` and the `OrphanAudit` enum it
/// returns are deliberately NOT part of this crate's public surface — see
/// `OrphanAudit`'s own doc for why — so this is currently the only way
/// external code can reach this functionality.
///
/// Collapses both the excluded-scope and environment-unavailable outcomes to
/// `None`. Returns `Some(json)` on a well-formed envelope. Panics exactly
/// when `run_orphan_audit_detailed` would panic: empty output for a scope
/// that is NOT excluded, or a resolved repo root that disagrees with what
/// the audit child itself would compute.
pub fn run_orphan_audit(scope: &str) -> Option<serde_json::Value> {
    match run_orphan_audit_detailed(scope) {
        OrphanAudit::Envelope(v) => Some(v),
        OrphanAudit::ExcludedScope | OrphanAudit::EnvUnavailable(_) => None,
    }
}

/// Resolve the on-disk path to `scripts/audit-orphan-producers.sh` and the
/// repo root it lives under, via two `.parent()` walks up from
/// `CARGO_MANIFEST_DIR` evaluated inside **this** crate
/// (`reify-test-support`), which always sits at
/// `<repo>/crates/reify-test-support/` regardless of which downstream crate
/// calls the public API.
///
/// Single-sourced for [`run_orphan_audit_detailed`] and this module's own
/// tests (which need the same two paths to construct decoy/mismatch
/// scenarios), so the production and test-time resolution can never drift
/// apart. Before the task 5698 amendment pass, the tests module carried an
/// independent copy of this exact walk (`resolve_real_script_and_root`),
/// which this replaces.
fn resolve_script_and_root() -> (PathBuf, PathBuf) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let script = Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-test-support has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .join("scripts/audit-orphan-producers.sh");
    let repo_root = script
        .parent()
        .expect("scripts/ dir exists")
        .parent()
        .expect("repo root exists")
        .to_path_buf();
    (script, repo_root)
}

/// Like [`run_orphan_audit`], but returns the full three-way [`OrphanAudit`]
/// outcome instead of collapsing two of them to `None`.
///
/// # Graceful-skip protocol
///
/// Returns [`OrphanAudit::EnvUnavailable`] (without panicking) when the
/// environment cannot satisfy the script's prerequisites:
/// - `python3` is absent from `PATH`
/// - `git` is absent from `PATH`
/// - `scripts/audit-orphan-producers.sh` does not exist on disk
/// - **As of task 5698**: the resolved `repo_root` is not inside a git work
///   tree at all (e.g. a source tarball with no `.git`) — genuinely
///   environmental, unlike the DIFFERENT-work-tree case below.
///
/// In each of those cases an explanatory message is printed to `stderr` so CI
/// logs remain informative, and the returned reason string names which one.
///
/// Returns [`OrphanAudit::ExcludedScope`] when `scope`'s crate segment is a
/// literal member of `EXCLUDE_CRATES`. Also non-failing, but distinct from
/// the above: the tooling ran fine — the scope is just deliberately excluded
/// from the audit.
///
/// Returns [`OrphanAudit::Envelope`] on success.
///
/// # Panics
///
/// - On a spawn error, or malformed JSON output — both always indicate a bug
///   in the audit script or its invocation, never an environmental
///   condition.
/// - **As of task 5698**: on empty output for a scope that is NOT in
///   `EXCLUDE_CRATES`. Before task 5698 this returned `None` exactly like the
///   excluded-crate case, which meant a wrong-tree redirect, an
///   `EXCLUDE_CRATES` edit, a crate rename, or a `discover_sources` script
///   refactor could silently disable the orphan-producer pin for a real scope
///   while every caller kept reporting green. That is never a legitimate
///   outcome, so it is now a hard failure instead of a silent skip — see
///   [`run_orphan_audit_at`]'s empty-stdout branch.
/// - **As of task 5698**: when `repo_root` IS inside a git work tree, but a
///   DIFFERENT one than the child itself resolves via `git rev-parse
///   --show-toplevel` from that directory. Tooling all present, but the
///   audit would run to completion against the wrong sources — a green
///   result that means nothing — so this is a hard failure rather than any
///   flavour of skip. See [`run_orphan_audit_at`]'s repo-root premise probe
///   (`child_repo_root`).
///
/// # `scope` argument
///
/// Pass a repo-relative path to a source directory, e.g.
/// `"crates/reify-audit/src"`.  This is forwarded as `--scope <scope>` to the
/// audit script.
///
/// # Repo-root resolution
///
/// The repo root is resolved at compile time via `env!("CARGO_MANIFEST_DIR")`
/// evaluated inside **this** crate (`reify-test-support`), which always sits at
/// `<repo>/crates/reify-test-support/`.  Two `.parent()` walks reach the repo
/// root regardless of which downstream crate calls this function.
fn run_orphan_audit_detailed(scope: &str) -> OrphanAudit {
    let (script, repo_root) = resolve_script_and_root();

    // Graceful skip: check python3 is available
    match Command::new("python3").arg("--version").output() {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!("python3 not on PATH; skipping orphan audit for scope {scope:?}");
            return OrphanAudit::EnvUnavailable("python3 not on PATH");
        }
        Err(e) => panic!("unexpected error probing python3: {e}"),
    }

    // Graceful skip: check git is available (audit-orphan-producers.sh:59-64
    // probes for both python3 AND git; missing git causes exit 3 which would
    // surface as a confusing JSON-parse panic without this probe).
    match Command::new("git").arg("--version").output() {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!("git not on PATH; skipping orphan audit for scope {scope:?}");
            return OrphanAudit::EnvUnavailable("git not on PATH");
        }
        Err(e) => panic!("unexpected error probing git: {e}"),
    }

    // The script-existence check now lives in run_orphan_audit_at itself
    // (task 5698 step 6), so that seam is self-contained when called
    // directly — see missing_script_is_env_unavailable.
    run_orphan_audit_at(&script, &repo_root, scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED premise (task 5698 step 1): at this branch base,
    /// `run_orphan_audit_at`'s empty-stdout branch does not yet distinguish
    /// "scope is legitimately in EXCLUDE_CRATES" from "the script scanned the
    /// wrong tree (or otherwise found zero real source files) for a scope
    /// that is NOT excluded" — both collapse to `ExcludedScope`.
    ///
    /// Measured in this worktree at the branch base: pointing the audit at a
    /// freshly `git init`ed decoy tree with the real (non-excluded) scope
    /// `crates/reify-audit/src` produces exit 0, 0-byte stdout, and the
    /// stderr line `audit-orphan-producers.sh: no source files matched` —
    /// byte-identical to the excluded-scope and nonexistent-scope cases. So
    /// nothing in the child's exit status or output can tell them apart; this
    /// test asserts on OUR panic message ONLY, never on the script's stderr
    /// text (which stays unchanged and ambiguous by design).
    ///
    /// Uses `catch_unwind` rather than `#[should_panic]`: the latter has no
    /// runtime skip, and this test must skip gracefully (not fail) on a
    /// git-less image, matching the convention every other test in this
    /// module already follows.
    #[test]
    fn wrong_tree_with_real_scope_panics() {
        // Graceful-skip convention used throughout this module: `git init`
        // below, and the script's own internal `git rev-parse
        // --show-toplevel`, both require git.
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!(
                "orphan_audit: skipping wrong_tree_with_real_scope_panics — git not available"
            );
            return;
        }

        let decoy = crate::temp_dirs::prefixed_tempdir("orphan-audit-decoy-");
        let mut git_init = Command::new("git");
        git_init
            .args(["init", "--initial-branch=main"])
            .current_dir(decoy.path());
        sanitize(&mut git_init);
        let status = git_init
            .status()
            .unwrap_or_else(|e| panic!("git init failed to spawn: {e}"));
        assert!(
            status.success(),
            "git init {:?} exited {:?}",
            decoy.path(),
            status.code()
        );

        let (script, _real_root) = resolve_script_and_root();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_orphan_audit_at(&script, decoy.path(), "crates/reify-audit/src")
        }));

        let payload = match result {
            Ok(audit) => panic!(
                "expected run_orphan_audit_at to panic for scope \
                 \"crates/reify-audit/src\" scanned under a decoy tree with no \
                 matching sources (empty stdout for a real, non-excluded scope must \
                 never be silently accepted as a legitimate outcome) — got: {audit:?}"
            ),
            Err(payload) => payload,
        };
        let message = reify_core::panic_payload_to_string(payload.as_ref());
        // Assert on wording unique to the empty-stdout branch, not the
        // shared "never a legitimate outcome" phrase that ALSO appears in
        // the repo-root-mismatch panic below — a bare substring match on
        // the shared phrase can't tell which branch actually fired, so it
        // would stay green even if a future canonicalization/TMPDIR-topology
        // change made the mismatch probe fire first instead of the
        // empty-stdout branch this test exists to cover (review finding #2,
        // task 5698 amendment pass).
        assert!(
            message.contains("NOT in EXCLUDE_CRATES"),
            "panicked, but not with the expected empty-stdout wording; got: {message}"
        );
        assert!(
            !message.contains("DIFFERENT repository"),
            "panicked with the repo-root-mismatch branch's wording instead of the \
             empty-stdout branch's — the decoy's own `git init` should make the \
             mismatch probe agree and let the empty-stdout branch fire instead; \
             got: {message}"
        );
    }

    /// RED premise (task 5698 step 5): `run_orphan_audit_at` does not yet
    /// verify that the child actually resolves the SAME repo root it was
    /// asked to scan. Pointing it at `<repo_root>/crates` (a real
    /// subdirectory, not `repo_root` itself) currently still produces a
    /// valid envelope: the script's own `REPO_ROOT="$(git rev-parse
    /// --show-toplevel)"` resolves past the subdirectory to the TRUE repo
    /// root and `cd`s there before scanning — silently auditing a different
    /// tree than the caller specified, with no error.
    ///
    /// Deliberately NOT environment poisoning (`GIT_DIR`/`GIT_WORK_TREE`):
    /// task 5605's `sanitize()` already strips those from the spawned child,
    /// so poisoning them would be vacuous — the same defect that made task
    /// 5656's original one-line design unreachable. Passing a real
    /// subdirectory as `repo_root` creates a genuine expected-vs-resolved
    /// disagreement with no environment manipulation at all, and models a
    /// real bug class: a `CARGO_MANIFEST_DIR` `.parent()` walk landing on the
    /// wrong level.
    #[test]
    fn child_repo_root_mismatch_panics() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!(
                "orphan_audit: skipping child_repo_root_mismatch_panics — git not available"
            );
            return;
        }

        let (script, repo_root) = resolve_script_and_root();
        let wrong_root = repo_root.join("crates");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_orphan_audit_at(&script, &wrong_root, "crates/reify-audit/src")
        }));

        let payload = match result {
            Ok(audit) => panic!(
                "expected run_orphan_audit_at to panic when repo_root ({wrong_root:?}) \
                 does not match what the child itself resolves via `git rev-parse \
                 --show-toplevel` — got: {audit:?}"
            ),
            Err(payload) => payload,
        };
        let message = reify_core::panic_payload_to_string(payload.as_ref());
        assert!(
            message.contains("DIFFERENT repository than requested"),
            "panicked, but not with the expected repo-root-mismatch wording; \
             got: {message}"
        );
    }

    /// Regression guard against [`child_repo_root_mismatch_panics`]'s probe
    /// becoming a false positive under this project's warm-lane topology.
    ///
    /// Measured at this branch base: sanitized `git rev-parse
    /// --show-toplevel` from the manifest-derived root returns EXACTLY that
    /// root, even inside a linked git worktree whose `.git` is a gitdir FILE
    /// (this project's normal topology, `worktrees/_lane-NN`) — so the probe
    /// agrees with the `CARGO_MANIFEST_DIR` two-`.parent()` walk and this
    /// call must NOT panic.
    ///
    /// Calls [`child_repo_root`] directly rather than going through
    /// [`run_orphan_audit_detailed`] (review finding #7, task 5698 amendment
    /// pass): the previous version spawned the full audit script — which
    /// this test doesn't need and which
    /// `run_orphan_audit_on_self_scope_returns_well_formed_envelope` already
    /// covers for the same scope — and its `EnvUnavailable` arm printed and
    /// silently fell through to a pass, so on any image lacking python3 it
    /// reported green without the probe under test ever running. Calling
    /// the probe directly removes the redundant spawn and makes an
    /// `EnvUnavailable`-shaped skip unreachable except on a genuinely
    /// git-less image (checked up front, same as every other test here).
    #[test]
    fn child_repo_root_agrees_with_manifest_walk() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!(
                "orphan_audit: skipping child_repo_root_agrees_with_manifest_walk — git \
                 not available"
            );
            return;
        }

        let (_script, repo_root) = resolve_script_and_root();
        match child_repo_root(&repo_root) {
            Ok(resolved) => {
                let resolved_canon = resolved
                    .canonicalize()
                    .unwrap_or_else(|e| panic!("canonicalize {resolved:?}: {e}"));
                let repo_root_canon = repo_root
                    .canonicalize()
                    .unwrap_or_else(|e| panic!("canonicalize {repo_root:?}: {e}"));
                assert_eq!(
                    resolved_canon, repo_root_canon,
                    "child_repo_root({repo_root:?}) resolved {resolved:?}, which does \
                     not canonicalize to the same path as the CARGO_MANIFEST_DIR walk \
                     — the mismatch probe in run_orphan_audit_at would incorrectly \
                     panic on every real invocation in this environment"
                );
            }
            Err(ChildRepoRootFailure::NoRepository) => eprintln!(
                "orphan_audit: skipping child_repo_root_agrees_with_manifest_walk — \
                 repo root is not inside a git work tree"
            ),
            Err(ChildRepoRootFailure::Other(detail)) => panic!(
                "child_repo_root({repo_root:?}) failed for a reason other than \"no \
                 repository here\" in what should be a normal checkout: {detail}"
            ),
        }
    }

    /// [`child_repo_root`] regression guard (review finding #4, task 5698
    /// amendment pass): a plain, never-`git init`ed directory is the ONE
    /// case that should classify as [`ChildRepoRootFailure::NoRepository`]
    /// — the only variant [`run_orphan_audit_at`] treats as a graceful
    /// [`OrphanAudit::EnvUnavailable`] skip rather than a panic. Calls
    /// [`child_repo_root`] directly: pure probe logic, no script spawn.
    #[test]
    fn child_repo_root_plain_non_git_dir_is_no_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!(
                "orphan_audit: skipping \
                 child_repo_root_plain_non_git_dir_is_no_repository — git not available"
            );
            return;
        }

        let dir = crate::temp_dirs::prefixed_tempdir("orphan-audit-no-git-");
        let result = child_repo_root(dir.path());
        assert!(
            matches!(result, Err(ChildRepoRootFailure::NoRepository)),
            "expected NoRepository for a plain directory with no .git anywhere in its \
             parent chain, got: {result:?}"
        );
    }

    /// [`child_repo_root`] regression guard (review finding #4, task 5698
    /// amendment pass): a directory whose `.git` FILE exists but points at a
    /// nonexistent gitdir produces git's `fatal: not a git repository:
    /// <path>` message — textually similar to, but a DIFFERENT diagnosis
    /// than, the `(or any of the parent directories)` message a truly
    /// repo-less directory produces (see
    /// `child_repo_root_plain_non_git_dir_is_no_repository`). This must
    /// classify as [`ChildRepoRootFailure::Other`], never
    /// [`ChildRepoRootFailure::NoRepository`]: a real repository was
    /// expected to exist here (e.g. a corrupted worktree gitdir pointer), so
    /// [`run_orphan_audit_at`] must panic rather than silently skip — the
    /// exact silent-green failure mode this task exists to close. Chosen
    /// over simulating git's "dubious ownership" rejection (also named in
    /// the review finding) because that needs a real UID mismatch this
    /// sandbox cannot produce hermetically; a corrupt `.git` file is one of
    /// the same finding's named examples and IS reproducible hermetically —
    /// verified empirically (bare `git rev-parse --show-toplevel` against a
    /// hand-written `.git` file pointing at `/nonexistent/path/.git` exits
    /// 128 with exactly that message, no "parent directories" wording).
    #[test]
    fn child_repo_root_corrupt_git_file_is_other_not_no_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!(
                "orphan_audit: skipping \
                 child_repo_root_corrupt_git_file_is_other_not_no_repository — git not \
                 available"
            );
            return;
        }

        let dir = crate::temp_dirs::prefixed_tempdir("orphan-audit-corrupt-git-");
        std::fs::write(dir.path().join(".git"), "gitdir: /nonexistent/path/.git\n")
            .unwrap_or_else(|e| panic!("write corrupt .git file: {e}"));

        let result = child_repo_root(dir.path());
        assert!(
            matches!(result, Err(ChildRepoRootFailure::Other(_))),
            "expected Other for a corrupt .git file (a real repository was intended to \
             exist here, so this must not be silently skipped as \"no repository\"), \
             got: {result:?}"
        );
    }

    /// RED premise (task 5698 step 5): `run_orphan_audit_at` does not yet
    /// check that `script` exists before spawning it — that check currently
    /// lives only in [`run_orphan_audit_detailed`], one layer up. Calling the
    /// seam directly with a nonexistent script hits the spawn's
    /// `unwrap_or_else` panic ("failed to invoke") instead of gracefully
    /// returning `EnvUnavailable`, so this test pins that the documented
    /// graceful-skip protocol survives the move of that check down into the
    /// seam (task 5698 step 6).
    #[test]
    fn missing_script_is_env_unavailable() {
        let (_script, repo_root) = resolve_script_and_root();
        let result = run_orphan_audit_at(
            Path::new("/nonexistent/audit-orphan-producers.sh"),
            &repo_root,
            "crates/reify-audit/src",
        );
        assert!(
            matches!(result, OrphanAudit::EnvUnavailable(_)),
            "expected EnvUnavailable for a nonexistent script path, got: {result:?}"
        );
    }

    /// Hand-rolled parse of an `EXCLUDE_CRATES = {"a", "b"}`-shaped Python
    /// set-literal declaration. No `regex` dependency exists anywhere in this
    /// workspace (checked before writing this test), so a small manual scan
    /// is used instead of pulling one in just for this. Returns `None` if no
    /// `EXCLUDE_CRATES = {` marker is found; otherwise returns whatever names
    /// it parsed (possibly empty), so the caller can distinguish "declaration
    /// not found" from "declaration found but parsed empty" and fail loudly
    /// on the latter rather than matching vacuously.
    fn parse_exclude_crates_declaration(source: &str) -> Option<Vec<String>> {
        let marker = "EXCLUDE_CRATES = {";
        let after_marker = source.find(marker)? + marker.len();
        let rest = &source[after_marker..];
        let end = rest.find('}')?;
        let body = &rest[..end];

        let mut names = Vec::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            if c == '"' || c == '\'' {
                let quote = c;
                let mut name = String::new();
                for c2 in chars.by_ref() {
                    if c2 == quote {
                        break;
                    }
                    name.push(c2);
                }
                names.push(name);
            }
        }
        Some(names)
    }

    /// Pins `orphan_audit.rs`'s `EXCLUDE_CRATES` const against
    /// `scripts/audit-orphan-producers.sh`'s own `EXCLUDE_CRATES = {...}`
    /// declaration (the source of truth for SET MEMBERSHIP) — the
    /// duplication-pinning pattern this codebase reaches for across a
    /// language boundary, rather than resting on a "keep in sync" comment.
    ///
    /// Pins ONLY the set's *contents* — NOT [`scope_is_excluded_crate`]'s
    /// matching *rule*. The script's own `discover_sources` excludes a
    /// matched directory or file when ANY of its path segments is a member
    /// of `EXCLUDE_CRATES` (`audit-orphan-producers.sh:126,132`);
    /// `scope_is_excluded_crate` only inspects the single segment
    /// immediately after `crates`. The two sides can therefore disagree for
    /// a scope shaped differently from every one of this workspace's 9
    /// current call sites — see `scope_is_excluded_crate_table` for the
    /// specific divergent cases this is known, and accepted, to not catch.
    ///
    /// RED at task 5698 step 1: `EXCLUDE_CRATES` does not exist yet, so this
    /// module does not compile.
    #[test]
    fn exclude_crates_const_matches_audit_script_declaration() {
        let (_script, repo_root) = resolve_script_and_root();
        let script_path = repo_root.join("scripts/audit-orphan-producers.sh");
        let source = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|e| panic!("read {script_path:?}: {e}"));

        let declared = parse_exclude_crates_declaration(&source).unwrap_or_else(|| {
            panic!(
                "could not find an `EXCLUDE_CRATES = {{...}}` declaration in \
                 {script_path:?} — has it moved or been reformatted? Update \
                 parse_exclude_crates_declaration's marker alongside whatever \
                 changed the script."
            )
        });
        assert!(
            !declared.is_empty(),
            "parsed an EXCLUDE_CRATES declaration from {script_path:?} but found \
             zero quoted names in it — this almost certainly means the parser \
             mis-parsed a reformatted declaration rather than that the set is \
             genuinely empty; a vacuous empty-set match would silently stop \
             catching drift, so this fails loudly instead"
        );

        let mut declared_sorted = declared.clone();
        declared_sorted.sort();
        let mut rust_sorted: Vec<String> =
            EXCLUDE_CRATES.iter().map(|s| s.to_string()).collect();
        rust_sorted.sort();

        assert_eq!(
            declared_sorted, rust_sorted,
            "orphan_audit.rs's EXCLUDE_CRATES const has drifted from \
             {script_path:?}'s EXCLUDE_CRATES = {{...}} declaration (the source \
             of truth) — update both together"
        );
    }

    /// Table-driven coverage of [`scope_is_excluded_crate`] (review finding
    /// #1, task 5698 amendment pass) — the single predicate that decides
    /// between a graceful [`OrphanAudit::ExcludedScope`] skip and a hard
    /// panic in [`run_orphan_audit_at`]'s empty-stdout branch. Needs neither
    /// `git` nor `python3`: pure string logic, no process ever spawned.
    ///
    /// The glob row (`"crates/reify-*/src"` -> `false`) is the most
    /// load-bearing: 5 of this workspace's 9 external call sites pass
    /// exactly that scope. If the `skip_while`/`nth(1)` logic ever
    /// regressed to match a prefix or the last path segment instead of the
    /// literal crate segment, every one of those 5 call sites would start
    /// silently skipping via `ExcludedScope` instead of exercising the real
    /// audit.
    ///
    /// This test pins ONLY [`scope_is_excluded_crate`]'s matching *rule* for
    /// these inputs — it does NOT assert that the rule agrees with
    /// `audit-orphan-producers.sh`'s own ANY-path-segment exclusion rule in
    /// general (see `exclude_crates_const_matches_audit_script_declaration`,
    /// which pins only the set's contents, one layer over from this).
    #[test]
    fn scope_is_excluded_crate_table() {
        let cases: &[(&str, bool)] = &[
            ("crates/reify-test-support/src", true),
            ("crates/reify-audit/src", false),
            // Glob scope: 5 of 9 external call sites pass exactly this
            // shape. `reify-*` contains a glob metacharacter, so it is never
            // literally equal to a single crate name and must stay `false`
            // — treating the glob itself as excluded would silently skip
            // real, non-excluded crates it expands to.
            ("crates/reify-*/src", false),
            ("crates/reify-test-support", true),
            // No `crates` path segment at all.
            ("gui/src-tauri/src", false),
        ];
        for (scope, expected) in cases {
            assert_eq!(
                scope_is_excluded_crate(scope),
                *expected,
                "scope_is_excluded_crate({scope:?}) should be {expected}"
            );
        }
    }

    /// Covers the empty-stdout → excluded-scope branch:
    /// `crates/reify-test-support/src` is in `EXCLUDE_CRATES` so the script
    /// exits 0 with no output.
    ///
    /// Post-5698 the four `None`-yielding branches no longer all converge on
    /// the same outcome: `ExcludedScope` and `EnvUnavailable` still both
    /// collapse to `None` through [`run_orphan_audit`] (pinned by the second
    /// assertion below, which is what all 9 existing call sites' `else {
    /// return; }`/`let Some(..) else` shape depends on) — but a wrong-tree or
    /// otherwise-empty result for a scope NOT in `EXCLUDE_CRATES` now panics
    /// instead (see `wrong_tree_with_real_scope_panics`). This test pins that
    /// the excluded-scope outcome specifically stays NAMED (`ExcludedScope`),
    /// not merely absent, by going through [`run_orphan_audit_detailed`]
    /// first.
    ///
    /// RED at task 5698 step 3: `run_orphan_audit_detailed` does not exist
    /// yet, so this module does not compile.
    #[test]
    fn run_orphan_audit_on_excluded_crate_is_named_not_erased() {
        match run_orphan_audit_detailed("crates/reify-test-support/src") {
            OrphanAudit::ExcludedScope => {}
            // Graceful-skip convention: missing python3/git/script/repo-root
            // is environmentally legitimate and indistinguishable here from
            // the excluded-scope case once collapsed through
            // `run_orphan_audit`; skip rather than fail.
            OrphanAudit::EnvUnavailable(reason) => {
                eprintln!(
                    "orphan_audit: skipping run_orphan_audit_on_excluded_crate_is_named_not_erased — {reason}"
                );
                return;
            }
            other => panic!(
                "expected ExcludedScope for EXCLUDE_CRATES scope \
                 \"crates/reify-test-support/src\" (or EnvUnavailable on a \
                 tooling-less image) — any other outcome, especially a panic \
                 from the empty-stdout hard-failure branch, means the \
                 excluded-crate case regressed into the hard-failure path; \
                 got: {other:?}"
            ),
        }

        assert!(
            run_orphan_audit("crates/reify-test-support/src").is_none(),
            "run_orphan_audit must still return None for an EXCLUDE_CRATES scope \
             — all 9 existing call sites' `else {{ return; }}`/`let Some(..) else` \
             shape depends on this backward-compatible collapse"
        );
    }

    /// Smoke-test: run the audit against `crates/reify-audit/src`.
    ///
    /// `crates/reify-audit/src` is a stable, assertion-friendly baseline: it
    /// has `orphan_count: 0` (all orphan producers carry `// G-allow:` markers)
    /// and produces a well-formed JSON envelope.
    ///
    /// Note: `crates/reify-test-support/src` is in `EXCLUDE_CRATES` in
    /// `audit-orphan-producers.sh`, which causes the script to emit empty stdout
    /// (exit 0) rather than a JSON envelope — so it cannot be used as a scope
    /// for testing the JSON-parse path.
    ///
    /// **Scope overlap is intentional.** This test shares the
    /// `crates/reify-audit/src` scope with `reify-audit/tests/g_allow.rs`.
    /// The purposes differ: this test verifies the helper's JSON-parse contract
    /// (`orphan_count` is a u64, `orphans` is an array); `g_allow.rs` verifies
    /// the domain assertion (`orphan_count == 0`).  On a full
    /// `cargo test --workspace` run the audit script fires twice against the
    /// same scope.  The cost is acceptable — the script is fast and a dedicated
    /// fixture directory would add maintenance overhead for minimal benefit.
    ///
    /// The test applies the same graceful-skip pattern as all downstream
    /// callers: if the environment lacks `python3` or `git`, we return early.
    #[test]
    fn run_orphan_audit_on_self_scope_returns_well_formed_envelope() {
        let Some(json) = run_orphan_audit("crates/reify-audit/src") else {
            // python3 / git / script absent — skip gracefully
            return;
        };

        assert!(
            json["orphan_count"].as_u64().is_some(),
            "expected orphan_count to be a u64; got: {:#}",
            json["orphan_count"]
        );
        assert!(
            json["orphans"].as_array().is_some(),
            "expected orphans to be an array; got: {:#}",
            json["orphans"]
        );
    }

    /// Asserts the constructed script-spawn `Command` records a removal
    /// (`env_remove` -> `(key, None)` in `get_envs()`) for every
    /// [`crate::git_env::REPO_REDIRECT_VARS`] entry — i.e. that this crate's
    /// script spawn sanitizes from the SHARED definition, not from a
    /// module-local copy. Uses [`crate::git_env::removed_vars`], the same
    /// removal-collecting helper the definition site's own tests use, so both
    /// read the `(key, None)` encoding through one place.
    ///
    /// This covers the WIRING only — that [`build_audit_command`] applies the
    /// sanitizer. That the sanitizer actually defeats an ambient redirect var
    /// against real git is proved once, at the definition site, by
    /// `crate::git_env`'s `sanitize_makes_dash_c_authoritative_against_real_git`.
    ///
    /// Unlike the two tests above, this one never spawns the script and needs
    /// neither `git` nor `python3` on `PATH` — it only inspects `Command`
    /// metadata, so the paths passed in are deliberately nonexistent. A
    /// regression that dropped [`sanitize`] from [`build_audit_command`], or
    /// dropped an entry from [`crate::git_env::REPO_REDIRECT_VARS`], fails
    /// this test even though the smoke tests above never poison the
    /// environment to notice.
    #[test]
    fn build_audit_command_removes_every_repo_redirect_var() {
        let cmd = build_audit_command(
            Path::new("/nonexistent/audit-orphan-producers.sh"),
            "crates/reify-audit/src",
            Path::new("/nonexistent/repo-root"),
        );
        let removed = crate::git_env::removed_vars(&cmd);
        for var in crate::git_env::REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "build_audit_command must REMOVE `{var}` (env_remove -> \
                 `(key, None)`), not merely leave it inherited; removals seen: \
                 {removed:?}"
            );
        }
    }
}
