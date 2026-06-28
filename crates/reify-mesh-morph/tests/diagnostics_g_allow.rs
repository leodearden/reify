//! Pin: `record_morphed`, `record_quality_remesh`, `record_ineligible`, and
//! `record_panicked` in `crates/reify-mesh-morph/src/diagnostics.rs` are now
//! WIRED — they have real (non-test) callers and are therefore no longer
//! orphan producers.
//!
//! ## History
//!
//! These four recorders were originally deferred orphans (only same-crate
//! `#[cfg(test)]` callers), allow-listed via `// G-allow:` markers pending the
//! engine integration. Task #4744 (the morph-engine wiring) landed that
//! integration: `compose_morph` (`lib.rs`) records exactly one diagnostic
//! counter on every morph outcome (steps 8/10), and the engine reaches it
//! through `register_morph_producer` → `MeshMorphProducer::try_morph` →
//! `compose_morph` (steps 17/18). The recorders now have a genuine non-test
//! caller, so the scoped orphan audit no longer lists them as orphans.
//!
//! This test therefore inverts its original assertion: instead of pinning the
//! recorders as allow-listed deferred orphans, it pins that they REMAIN wired —
//! each must be ABSENT from `orphans[]`. A regression that severed the
//! `compose_morph` call edge (returning a recorder to orphan status without a
//! `// G-allow:` marker) would re-list it in `orphans[]` and fail this test.
//!
//! The `// G-allow:` markers in `diagnostics.rs` are now vestigial (a `G-allow`
//! on a non-orphan is ignored by the audit) but are left in place — removing
//! them is out of this test's scope and they are harmless.
//!
//! User-observable signal:
//!   `cargo test -p reify-mesh-morph --test diagnostics_g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` with
//! `--scope crates/reify-mesh-morph/src` and asserts *list non-membership* —
//! each of the four named recorders must be ABSENT from `orphans[]`.
//!
//! We do NOT assert `orphan_count == 0`: reify-mesh-morph has pre-existing
//! baseline orphans in boundary/elasticity/laplacian/lib/quality/diagnostics
//! (e.g. cross-crate-consumed `snapshot`/`format_summary`/`reset`, invisible to
//! a `--scope crates/reify-mesh-morph/src` run) that are outside this task's
//! scope and would make such an assertion spurious.
//!
//! Graceful skip: if `python3`, `git`, or the script are absent from PATH/disk,
//! the shared helper prints a note to stderr and returns `None`; the test then
//! returns early. Mirrors `crates/reify-mesh-morph/tests/stats_g_allow.rs`.
//! The shared helper is in `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

/// The four `pub fn` recorders in diagnostics.rs. Originally deferred orphans;
/// now wired via `compose_morph` (their first real non-test caller).
const TARGET_FNS: &[&str] = &[
    "record_morphed",
    "record_quality_remesh",
    "record_ineligible",
    "record_panicked",
];

#[test]
fn diagnostics_record_fns_are_wired_not_orphans() {
    let Some(result) = run_orphan_audit("crates/reify-mesh-morph/src") else {
        return;
    };

    let diagnostics_suffix = "crates/reify-mesh-morph/src/diagnostics.rs";

    for fn_name in TARGET_FNS {
        // Each recorder must NOT appear in orphans[] (for the diagnostics.rs
        // file): it has a real non-test caller (`compose_morph`), so it is
        // wired, not an orphan. A regression that severed that call edge — and
        // left no `// G-allow:` marker — would re-list it here.
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
            "`{fn_name}` in {diagnostics_suffix} is listed as an orphan — its real \
             caller (`compose_morph`) may have been severed without leaving a \
             `// G-allow:` marker.\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );
    }
}
