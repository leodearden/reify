use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

/// Git environment variables that redirect *which repository* a command
/// operates on — the same defining criterion as, and an independent copy of,
/// `reify_audit::git_env::REPO_REDIRECT_VARS`. A copy rather than a re-export:
/// the dependency edge runs `reify-audit` -> `reify-test-support` (see
/// `Cargo.toml`), so this crate cannot depend on `reify-audit` to reuse its
/// copy directly.
///
/// `crates/reify-audit/src/git_env.rs` carries a unit test
/// (`repo_redirect_vars_matches_reify_test_support_orphan_audit_copy`)
/// asserting the two lists stay equal, so a divergence between them now fails
/// `cargo test` rather than resting on a "keep in sync" comment.
pub const REPO_REDIRECT_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_WORK_TREE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// Remove every [`REPO_REDIRECT_VARS`] entry from `cmd`'s environment. See
/// [`REPO_REDIRECT_VARS`] for why this crate keeps its own copy of this
/// function rather than calling `reify_audit::git_env::sanitize`.
pub fn sanitize(cmd: &mut Command) -> &mut Command {
    for var in REPO_REDIRECT_VARS {
        cmd.env_remove(var);
    }
    cmd
}

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
/// [`OrphanAudit::ExcludedScope`] and [`OrphanAudit::EnvUnavailable`] to
/// `None`, so every existing call site's `let Some(..) else { return; }`/
/// `else { return; }` shape keeps compiling and behaving identically. Callers
/// that need to discriminate a legitimate skip from a hard failure should
/// call `run_orphan_audit_detailed` instead (added in task 5698 step 4).
#[derive(Debug)]
pub enum OrphanAudit {
    /// A well-formed JSON envelope was produced.
    Envelope(serde_json::Value),
    /// `scope`'s crate segment is a literal member of `EXCLUDE_CRATES` — a
    /// legitimate, non-failing outcome that the script itself encodes as
    /// empty stdout.
    ExcludedScope,
    /// The environment cannot satisfy the script's prerequisites (missing
    /// `python3`/`git`, the script itself absent, or `repo_root` not inside a
    /// git work tree). Carries a short human-readable reason for logging.
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
/// sync" comment — the same duplication-pinning pattern
/// `crates/reify-audit/src/git_env.rs`'s
/// `repo_redirect_vars_matches_reify_test_support_orphan_audit_copy` uses one
/// layer out.
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

/// Injectable seam behind [`run_orphan_audit`]: takes `script` and
/// `repo_root` as parameters rather than resolving them internally, so tests
/// can point it at a decoy tree, a subdirectory root, or a nonexistent script
/// without touching the real repo or the real script location. Mirrors why
/// [`build_audit_command`] was split out one layer down.
fn run_orphan_audit_at(script: &Path, repo_root: &Path, scope: &str) -> OrphanAudit {
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
/// # Graceful-skip protocol
///
/// Returns `None` (without panicking) when the environment cannot satisfy the
/// script's prerequisites:
/// - `python3` is absent from `PATH`
/// - `git` is absent from `PATH`
/// - `scripts/audit-orphan-producers.sh` does not exist on disk
///
/// In each of those cases an explanatory message is printed to `stderr` so CI
/// logs remain informative.
///
/// Returns `Some(json)` on success.  Panics on hard failures: spawn errors or
/// malformed JSON output (which always indicates a bug in the audit script or
/// its invocation).
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
pub fn run_orphan_audit(scope: &str) -> Option<serde_json::Value> {
    // Resolve script path: CARGO_MANIFEST_DIR = crates/reify-test-support
    // Go up two parents → repo root → scripts/audit-orphan-producers.sh
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
        .expect("repo root exists");

    // Graceful skip: check python3 is available
    match Command::new("python3").arg("--version").output() {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!("python3 not on PATH; skipping orphan audit for scope {scope:?}");
            return None;
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
            return None;
        }
        Err(e) => panic!("unexpected error probing git: {e}"),
    }

    // Graceful skip: check the script itself exists
    if !script.exists() {
        eprintln!(
            "scripts/audit-orphan-producers.sh not found at {:?}; skipping",
            script
        );
        return None;
    }

    // Sanitize repo-redirect git env vars before spawning: the script's first
    // action is `REPO_ROOT="$(git rev-parse --show-toplevel)"`, and an
    // ambient GIT_DIR/GIT_WORK_TREE (exported into a hook's entire process
    // tree) overrides BOTH cwd and an explicit `-C`. Full rationale and
    // failure mode, and why this crate keeps its own REPO_REDIRECT_VARS/
    // sanitize copy rather than calling reify_audit::git_env::sanitize (the
    // dependency edge runs the other way): crates/reify-audit/src/git_env.rs
    // module doc, "Resolved non-central case".
    let output = build_audit_command(&script, scope, repo_root)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke audit-orphan-producers.sh: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Guard: empty stdout means the scope is in EXCLUDE_CRATES or the script
    // produced no envelope.  `serde_json::from_str("")` would panic with a
    // "not valid JSON" error whose accompanying "status: ExitStatus(0)" would
    // be misleading.  Surface a clear skip-style message instead.
    if stdout.trim().is_empty() {
        eprintln!(
            "audit-orphan-producers.sh produced empty output for scope {scope:?} \
             — scope may be in EXCLUDE_CRATES (exit status: {:?})",
            output.status
        );
        return None;
    }

    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "audit-orphan-producers.sh output was not valid JSON: {e}\n\
             status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        )
    });

    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Resolve the on-disk path to the real `audit-orphan-producers.sh`
    /// script, and the repo root it lives under — the same two-`.parent()`
    /// walk [`run_orphan_audit`] uses, duplicated here so tests can pass the
    /// real script and a DECOY `repo_root` (or vice versa) to
    /// [`run_orphan_audit_at`] independently.
    fn resolve_real_script_and_root() -> (PathBuf, PathBuf) {
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

        let (script, _real_root) = resolve_real_script_and_root();
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
        assert!(
            message.contains("never a legitimate outcome"),
            "panicked, but not with the expected wrong-tree/no-sources wording; \
             got: {message}"
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
    /// declaration (the source of truth [`scope_is_excluded_crate`] must
    /// agree with) — the same duplication-pinning pattern
    /// `crates/reify-audit/src/git_env.rs`'s
    /// `repo_redirect_vars_matches_reify_test_support_orphan_audit_copy` uses
    /// one layer out, rather than resting on a "keep in sync" comment.
    ///
    /// RED at task 5698 step 1: `EXCLUDE_CRATES` does not exist yet, so this
    /// module does not compile.
    #[test]
    fn exclude_crates_const_matches_audit_script_declaration() {
        let (_script, repo_root) = resolve_real_script_and_root();
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

    /// Covers the empty-stdout → `None` branch: `crates/reify-test-support/src` is in
    /// `EXCLUDE_CRATES` so the script exits 0 with no output. A flat `is_none()` assertion
    /// is environment-safe — all four `None`-yielding branches converge on the same outcome.
    #[test]
    fn run_orphan_audit_on_excluded_crate_returns_none() {
        let result = run_orphan_audit("crates/reify-test-support/src");
        assert!(
            result.is_none(),
            "expected None for EXCLUDE_CRATES scope `crates/reify-test-support/src` \
             (the script emits empty stdout for excluded crates); got Some(_): {:#?}",
            result
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

    /// Mirrors `git_env.rs`'s `both_entry_points_remove_every_repo_redirect_var`:
    /// asserts the constructed script-spawn `Command` records a removal
    /// (`env_remove` -> `(key, None)` in `get_envs()`) for every
    /// [`REPO_REDIRECT_VARS`] entry. Unlike the two tests above, this one
    /// never spawns the script and needs neither `git` nor `python3` on
    /// `PATH` — it only inspects `Command` metadata, so the paths passed in
    /// are deliberately nonexistent. A regression that dropped [`sanitize`]
    /// from [`build_audit_command`], or dropped an entry from
    /// [`REPO_REDIRECT_VARS`], fails this test even though the smoke tests
    /// above never poison the environment to notice.
    #[test]
    fn build_audit_command_removes_every_repo_redirect_var() {
        let cmd = build_audit_command(
            Path::new("/nonexistent/audit-orphan-producers.sh"),
            "crates/reify-audit/src",
            Path::new("/nonexistent/repo-root"),
        );
        let removed: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for var in REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "build_audit_command must REMOVE `{var}` (env_remove -> \
                 `(key, None)`), not merely leave it inherited; removals seen: \
                 {removed:?}"
            );
        }
    }
}
