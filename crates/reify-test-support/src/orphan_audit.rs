use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

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

    let output = Command::new(&script)
        .args(["--scope", scope, "--quiet", "--format", "json"])
        .current_dir(repo_root)
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
    /// Exercises the EXCLUDE_CRATES → empty-stdout → `None` branch (lines 96-103
    /// of `run_orphan_audit`).
    ///
    /// `reify-test-support` is the sole entry in `EXCLUDE_CRATES` in
    /// `scripts/audit-orphan-producers.sh:92`. Passing this scope causes the
    /// script's `discover_sources` to return zero files; the script then exits
    /// 0 with empty stdout, and the helper's empty-stdout guard returns `None`.
    ///
    /// Why no graceful-skip dance: all four `None`-yielding branches in
    /// `run_orphan_audit` (python3 missing, git missing, script missing,
    /// EXCLUDE_CRATES) yield `None`, so a flat `assert!(result.is_none())` is
    /// universally valid across CI environments. Do NOT add a `python3
    /// --version` probe — it would be redundant.
    ///
    /// Mutation property: removing the empty-stdout guard (lines 96-103) causes
    /// `serde_json::from_str("")` to panic, which fails this test.
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
}
