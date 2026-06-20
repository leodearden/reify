//! θ2 (task 4531) edit-path unified-driver test binary.
//!
//! Pins the design-doc "warm output == cold output becomes structural" claim on
//! the EDIT surface: `edit_param` / `edit_source` / `edit_check` must order their
//! value re-evaluation through the SAME unified driver
//! (`engine_fixpoint::run_unified_pass`) as cold/build/concurrent, retiring edit's
//! hand-maintained second scheduler (solver wave-2 + Phase-3 flip dedup) before the
//! ι (#4362) cutover.
//!
//! The shared differential harness (`common/differential.rs`) is `#[path]`-included
//! so this binary reuses the θ projection + parity helpers
//! (`assert_edit_matches_cold`, `assert_edit_source_matches_cold`,
//! `project_eval_values`) with zero edits to existing shared files.
//!
//! Steps land RED tests here incrementally (guard flip via edit, solver autos via
//! edit, collection grow → upstream edit, edit_source/edit_check mirror, P0 latency
//! gate). This file starts with the harness smoke tests that prove the pre-1
//! infrastructure is wired and GREEN on the existing edit behavior.
#![allow(dead_code, unused_imports)]

#[path = "common/differential.rs"]
mod differential;

use differential::{
    BRACKET_EDIT_SRC, WARM_PREDICATE_K5_SRC, WARM_PREDICATE_SRC, assert_edit_matches_cold,
    bracket_source,
};
use reify_core::ValueCellId;
use reify_eval::BuildScheduler;
use reify_ir::Value;

// ─────────────────────────────────────────────────────────────────────────────
// pre-1 harness smoke tests.
//
// These exercise `assert_edit_matches_cold` on a known-good pure-scalar fixture
// (`WARM_PREDICATE_SRC` k=2.0 → edit k=5.0 → cold `WARM_PREDICATE_K5_SRC` k=5.0),
// which the LEGACY edit_param already satisfies — so the prerequisite is GREEN
// before any production change. The structural RED tests arrive in later steps.
// ─────────────────────────────────────────────────────────────────────────────

/// pre-1: the edit-vs-cold parity harness wires up and is GREEN on the existing
/// `edit_param` behavior under `LegacyMultiPass` — editing `WarmPredicate.k` from
/// 2.0 to 5.0 yields the same values as a cold eval of the k=5.0 source.
#[test]
fn harness_edit_param_matches_cold_legacy() {
    assert_edit_matches_cold(
        WARM_PREDICATE_SRC,
        &[(ValueCellId::new("WarmPredicate", "k"), Value::Real(5.0))],
        WARM_PREDICATE_K5_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );
}

/// pre-1: the same parity holds under `UnifiedDag` — `edit_param` is
/// scheduler-agnostic by construction (never reads `build_scheduler`), so the
/// harness must be GREEN under both schedulers.
#[test]
fn harness_edit_param_matches_cold_unified() {
    assert_edit_matches_cold(
        WARM_PREDICATE_SRC,
        &[(ValueCellId::new("WarmPredicate", "k"), Value::Real(5.0))],
        WARM_PREDICATE_K5_SRC,
        BuildScheduler::UnifiedDag,
        false,
    );
}

/// pre-1: the bracket latency fixture is loadable both inline
/// ([`BRACKET_EDIT_SRC`]) and from disk ([`bracket_source`]). The on-disk
/// `examples/bracket.ri` shares the `Bracket` structure shape, so both compile and
/// the inline fixture is non-empty — the deterministic input for the step-15 P0
/// latency gate.
#[test]
fn harness_bracket_fixture_loads() {
    assert!(BRACKET_EDIT_SRC.contains("structure Bracket"));
    let on_disk = bracket_source();
    assert!(
        on_disk.contains("structure Bracket"),
        "examples/bracket.ri should define `structure Bracket`"
    );
}
