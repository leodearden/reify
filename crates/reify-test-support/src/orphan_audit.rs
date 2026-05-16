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
    todo!("step-2 implements")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-test: run the audit against `crates/reify-test-support/src`.
    ///
    /// `reify-test-support` is in `EXCLUDE_CRATES` in
    /// `scripts/audit-orphan-producers.sh`, so the script returns a well-formed
    /// JSON envelope with `orphan_count: 0` and empty `orphans` / `allowed`
    /// arrays — a stable, assertion-friendly baseline.
    ///
    /// The test applies the same graceful-skip pattern as all downstream
    /// callers: if the environment lacks `python3` or `git`, we return early.
    #[test]
    fn run_orphan_audit_on_self_scope_returns_well_formed_envelope() {
        let Some(json) = run_orphan_audit("crates/reify-test-support/src") else {
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
