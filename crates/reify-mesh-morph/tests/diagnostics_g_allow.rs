//! Pin: `record_morphed`, `record_quality_remesh`, `record_ineligible`, and
//! `record_panicked` in `crates/reify-mesh-morph/src/diagnostics.rs` each carry
//! a `// G-allow:` marker noting that the engine call-site wiring is deferred
//! (the events fire from the engine integration in `reify-eval`'s
//! `engine_build.rs`; the snapshot consumer is the downstream debug-RPC task).
//!
//! User-observable signal:
//!   `cargo test -p reify-mesh-morph --test diagnostics_g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` with
//! `--scope crates/reify-mesh-morph/src` and asserts *list membership* —
//! each of the four named recorders must be ABSENT from `orphans[]` and
//! PRESENT in `allowed[]` with a non-empty reason.
//!
//! The reason assertion checks the STABLE substrings "engine" and "deferred"
//! rather than volatile task numbers: the engine-wiring task has already been
//! renumbered once (#2947 → #3429), so pinning on a number would be brittle.
//!
//! Crucially, we do NOT assert `orphan_count == 0`: reify-mesh-morph has
//! pre-existing baseline orphans in boundary/elasticity/laplacian/lib/quality
//! that are outside this task's scope and would make such an assertion spurious.
//!
//! Graceful skip: if `python3`, `git`, or the script are absent from PATH/disk,
//! the shared helper prints a note to stderr and returns `None`; the test then
//! returns early. Mirrors `crates/reify-mesh-morph/tests/stats_g_allow.rs`.
//! The shared helper is in `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

/// The four `pub fn` recorders in diagnostics.rs whose only callers are
/// same-crate `#[cfg(test)]` code; engine call-site wiring is deferred.
const TARGET_FNS: &[&str] = &[
    "record_morphed",
    "record_quality_remesh",
    "record_ineligible",
    "record_panicked",
];

#[test]
fn diagnostics_record_fns_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-mesh-morph/src") else {
        return;
    };

    let diagnostics_suffix = "crates/reify-mesh-morph/src/diagnostics.rs";

    for fn_name in TARGET_FNS {
        // (a) must NOT appear in orphans[] (for the diagnostics.rs file)
        let in_orphans = result["orphans"]
            .as_array()
            .expect("orphans must be an array")
            .iter()
            .any(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(diagnostics_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            });

        assert!(
            !in_orphans,
            "`{fn_name}` in {diagnostics_suffix} is still listed as an orphan — \
             the `// G-allow:` marker may be missing or misplaced.\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );

        // (b) must appear in allowed[] exactly once, for the diagnostics.rs file
        let matching_allowed: Vec<_> = result["allowed"]
            .as_array()
            .expect("allowed must be an array")
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(diagnostics_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        assert_eq!(
            matching_allowed.len(),
            1,
            "`{fn_name}` in {diagnostics_suffix} must appear exactly once in allowed[]; \
             found {} entries.\nFull allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        // (c) the allow_reason must be non-empty and cite the deferred engine
        //     wiring via the stable substrings "engine" and "deferred".
        let reason = matching_allowed[0]["allow_reason"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !reason.is_empty(),
            "`{fn_name}` allow_reason must be non-empty"
        );
        assert!(
            reason.contains("engine") && reason.contains("deferred"),
            "`{fn_name}` allow_reason must cite the deferred engine wiring \
             (stable substrings \"engine\" and \"deferred\"); got: {reason:?}"
        );
    }
}
