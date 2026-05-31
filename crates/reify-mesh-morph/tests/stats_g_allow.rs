//! Pin: `record_morph_attempt`, `record_remesh`, and `record_rejection` in
//! `crates/reify-mesh-morph/src/stats.rs` each carry a `// G-allow:` marker
//! citing tasks #2947-#2949 (engine call-site wiring is deferred).
//!
//! User-observable signal:
//!   `cargo test -p reify-mesh-morph --test stats_g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` with
//! `--scope crates/reify-mesh-morph/src` and asserts *list membership* —
//! each of the three named functions must be ABSENT from `orphans[]` and
//! PRESENT exactly once in `allowed[]` with a non-empty allow_reason
//! (membership-only; no substring pin on the marker text).
//!
//! Crucially, we do NOT assert `orphan_count == 0`: reify-mesh-morph has 8
//! pre-existing baseline orphans in boundary/elasticity/laplacian/lib/quality
//! that are outside this task's scope and would make such an assertion spurious.
//!
//! Graceful skip: if `python3`, `git`, or the script are absent from PATH/disk,
//! the test prints a note to stderr and returns. Mirrors
//! `crates/reify-audit/tests/g_allow.rs`.
//! The shared helper is in `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

/// The three `pub fn` in stats.rs whose only callers are same-crate
/// `#[cfg(test)]` code; engine wiring is deferred to tasks #2947-#2949.
const TARGET_FNS: &[&str] = &["record_morph_attempt", "record_remesh", "record_rejection"];

#[test]
fn stats_record_fns_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-mesh-morph/src") else {
        return;
    };

    let stats_suffix = "crates/reify-mesh-morph/src/stats.rs";

    for fn_name in TARGET_FNS {
        // (a) must NOT appear in orphans[] (for the stats.rs file)
        let in_orphans = result["orphans"]
            .as_array()
            .expect("orphans must be an array")
            .iter()
            .any(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(stats_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            });

        assert!(
            !in_orphans,
            "`{fn_name}` in {stats_suffix} is still listed as an orphan — \
             the `// G-allow:` marker may be missing or misplaced.\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );

        // (b) must appear in allowed[] with a reason citing tasks #2947 and #2949
        let matching_allowed: Vec<_> = result["allowed"]
            .as_array()
            .expect("allowed must be an array")
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(stats_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        assert_eq!(
            matching_allowed.len(),
            1,
            "`{fn_name}` in {stats_suffix} must appear exactly once in allowed[]; \
             found {} entries.\nFull allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        let reason = matching_allowed[0]["allow_reason"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !reason.is_empty(),
            "`{fn_name}` in stats.rs must have a non-empty allow_reason; got: {reason:?}"
        );
    }
}
