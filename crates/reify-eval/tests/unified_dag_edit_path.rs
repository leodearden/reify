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
use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{GeometryKernel, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

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

// ─────────────────────────────────────────────────────────────────────────────
// step-3 (RED): edit_param re-evaluates in the unified DRIVER's schedule order.
//
// The fixture is chosen so the LEGACY `compute_eval_set` order (level-by-level
// Kahn, `dirty::compute_levels`) differs from `run_unified_pass`'s GLOBAL
// DebugOrd-priority Kahn order. Editing `p` dirties {a, b, c, z}:
//   a = p          (reads param p — external to the eval_set ⇒ in-degree 0)
//   b = a          (reads a)
//   c = b          (reads b)              a→b→c is a depth-2 chain
//   z = p          (reads param p — external ⇒ in-degree 0, DebugOrd-large)
//
// Within the eval_set {a,b,c,z}:
//   • LEGACY level order:  [a, z, b, c]  — level 0 = {a, z}, so the shallow
//     sibling `z` is emitted BEFORE the chain's interior `b`/`c`.
//   • DRIVER global Kahn:  [a, b, c, z]  — once `a` is popped, `b` (DebugOrd <
//     `z`) is immediately ready and drains the whole chain before `z`.
//
// The Started-event sequence therefore distinguishes the two orderings. Legacy
// edit_param iterates `compute_eval_set` order → [a, z, b, c]; after step-4 the
// executor walks the driver schedule → [a, b, c, z]. RED until step-4.
// ─────────────────────────────────────────────────────────────────────────────

const DRIVER_ORDER_P1_SRC: &str = r#"structure DriverOrder {
    param p: Real = 1.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

/// The post-edit-equivalent cold reference: same module with `p = 2.0`.
const DRIVER_ORDER_P2_SRC: &str = r#"structure DriverOrder {
    param p: Real = 2.0
    let a = p * 1.0
    let b = a * 1.0
    let c = b * 1.0
    let z = p * 2.0
}"#;

/// Construct a fresh kernel-backed engine pinned to `scheduler`. Mirrors the
/// inline constructor used across the unified-dag test binaries.
fn fresh_engine(scheduler: BuildScheduler) -> Engine {
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new()) as Box<dyn GeometryKernel>),
    );
    engine.set_build_scheduler(scheduler);
    engine
}

/// step-3 (RED until step-4): `edit_param` must re-evaluate the dirty∩demand value
/// cells in the unified driver's Kahn schedule order — observed via the journal
/// `EvalEvent{kind: Started}` sequence — AND produce values equal to a cold `eval()`
/// of the post-edit-equivalent module. This pins "edit rides the same ordering core
/// as cold" (structural warm==cold), which legacy `compute_eval_set` order does not
/// guarantee.
#[test]
fn edit_param_revaluates_in_driver_schedule_order() {
    let p = ValueCellId::new("DriverOrder", "p");

    // (1) Value parity — the edited value map equals cold eval() of the p=2.0
    // module. Already GREEN under legacy (documents the full warm==cold claim).
    assert_edit_matches_cold(
        DRIVER_ORDER_P1_SRC,
        &[(p.clone(), Value::Real(2.0))],
        DRIVER_ORDER_P2_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
    );

    // (2) Ordering — the Started-event sequence over the eval_set must equal the
    // driver's Kahn order [a, b, c, z], NOT legacy level order [a, z, b, c].
    let compiled = compile_source(DRIVER_ORDER_P1_SRC);
    let mut engine = fresh_engine(BuildScheduler::LegacyMultiPass);
    engine.eval(&compiled);

    let len_before = engine.journal().all_events().len();
    engine
        .edit_param(p.clone(), Value::Real(2.0))
        .expect("edit_param must succeed");

    // Only Value nodes emit Started events inside the value loop, so this is the
    // re-evaluation order restricted to the eval_set.
    let started: Vec<NodeId> = engine.journal().all_events()[len_before..]
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Started))
        .map(|e| e.node_id.clone())
        .collect();

    let v = |field: &str| NodeId::Value(ValueCellId::new("DriverOrder", field));
    assert_eq!(
        started,
        vec![v("a"), v("b"), v("c"), v("z")],
        "edit_param must re-evaluate in the unified driver's Kahn order [a, b, c, z]; \
         legacy compute_eval_set level-order is [a, z, b, c] (RED until step-4 routes \
         the value loop through run_unified_pass_seeded). Observed: {started:?}"
    );
}
