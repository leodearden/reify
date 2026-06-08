//! Engine-level tests for the `RepresentationWithin` assertion dispatch
//! interception (Determinacy γ, task-4199).
//!
//! # Non-OCCT tests (step-5 / step-6)
//!
//! Verify that `Engine::dispatch_constraints` correctly intercepts
//! `RepresentationWithin` constraint expressions, evaluates them against
//! `self.achieved_repr_tol` (injected via a test-instrumentation setter), and
//! weaves results back in caller (input) order.
//!
//! These tests use a non-kernel engine (no OCCT) so that the full pipeline
//! can be exercised in CI without a geometry backend.  The
//! `set_achieved_repr_tol_for_test` setter is the test-instrumentation seam
//! added alongside `set_capture_repr_tol` (engine_admin.rs) — it does NOT
//! exist until step-6 implements it, so the tests below are RED until then.
//!
//! # OCCT-gated tests (step-7 / step-8)
//!
//! End-to-end tests that use a real OCCT kernel to tessellate curved geometry
//! and verify the full dispatch-interception + tessellation pipeline.  These
//! tests are added in step-7.

use reify_ir::Satisfaction;
use reify_test_support::{make_simple_engine, parse_and_compile};
use std::collections::BTreeMap;

// ── Shared DSL fixture ────────────────────────────────────────────────────────

/// A module with two constraints in the **same** template (`Checker`):
///
/// - Constraint index 0: `RepresentationWithin(subject, 1um)` — the assertion.
/// - Constraint index 1: `w > 0.0` — an ordinary always-`Satisfied` predicate.
///
/// `MyGeom` supplies the named structure type for `subject`; it has no
/// geometry (non-kernel engine) so `subject.self` is Undef at eval time.
/// The type-name scan fallback in `eval_representation_within` resolves
/// the achieved-tol key from the struct name `"MyGeom"` → key
/// `"MyGeom#realization[0]"` in the injected map.
///
/// Both constraints live in the **same** template so they pass through a
/// **single** `dispatch_constraints` call — this exercises within-batch order
/// preservation when the interception peels constraint 0 and leaves
/// constraint 1 for the language-level checker.
const INTERCEPTION_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

// Checker carries BOTH a RepresentationWithin assertion (constraint index 0)
// AND an ordinary always-satisfied constraint (index 1) in a single template.
// Placing both constraints here exercises the within-batch order-preservation
// invariant of dispatch_constraints: the engine-side result for index 0 must
// appear before the checker-side result for index 1 in the returned list.
structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1um)
    constraint w > 0.0
}
"#;

// ── BT1: over-bound → Violated ────────────────────────────────────────────────

/// BT1: achieved value ABOVE the bound (5e-3 m > 1 μm = 1e-6 m) → `Violated`.
///
/// Also verifies:
/// - The ordinary constraint (`w > 0.0` with `w = 5.0`) is `Satisfied`.
/// - **Input-order preservation**: RepresentationWithin (constraint index 0)
///   appears before the ordinary constraint (index 1) in the result list,
///   proving `dispatch_constraints` weaves interception results back in the
///   original entry order.
///
/// RED until step-6 adds `set_achieved_repr_tol_for_test` and wires the
/// interception into `dispatch_constraints`.
#[test]
fn dispatch_interception_over_bound_yields_violated() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Inject achieved_repr_tol via the test-instrumentation setter.
    // "MyGeom#realization[0]" = 5e-3 m ≫ 1 μm bound → must yield Violated.
    //
    // RED: `set_achieved_repr_tol_for_test` does not exist until step-6.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    // Checker has two constraints → exactly 2 constraint results.
    assert_eq!(
        result.constraint_results.len(),
        2,
        "Checker has 2 constraints (RepresentationWithin + w>0) → 2 results; \
         got {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, e.satisfaction))
            .collect::<Vec<_>>()
    );

    // ── RepresentationWithin (entity="Checker", index=0) ──────────────────────
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "BT1: achieved 5e-3 m > bound 1 μm → Violated"
    );

    // ── Ordinary constraint (entity="Checker", index=1) ───────────────────────
    let ord_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 1)
        .expect("must have Checker#constraint[1] (w > 0.0)");
    assert_eq!(
        ord_entry.satisfaction,
        Satisfaction::Satisfied,
        "w=5.0 > 0.0 → Satisfied (ordinary constraint unaffected by interception)"
    );

    // ── Input-order preservation ───────────────────────────────────────────────
    // The RepresentationWithin result (index 0) must appear at a LOWER position
    // in the output list than the ordinary result (index 1), matching the order
    // of entries in the dispatch batch.
    let rw_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 0)
        .unwrap();
    let ord_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 1)
        .unwrap();
    assert!(
        rw_pos < ord_pos,
        "BT1: RepresentationWithin (pos {rw_pos}) must precede the ordinary \
         constraint (pos {ord_pos}) — dispatch_constraints must preserve \
         within-batch input order even when interleaving engine-side and \
         checker-side results"
    );
}

// ── BT2: under-bound → Satisfied ─────────────────────────────────────────────

/// BT2: achieved value BELOW the bound (1e-9 m ≪ 1 μm) → `Satisfied`.
///
/// RED until step-6.
#[test]
fn dispatch_interception_under_bound_yields_satisfied() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1 μm bound → Satisfied.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 1e-9_f64);
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "BT2: achieved 1e-9 m < bound 1 μm → Satisfied"
    );
}

// ── BT3: no entry → Indeterminate ────────────────────────────────────────────

/// BT3: no entry in `achieved_repr_tol` for the subject → `Indeterminate`.
///
/// C1 invariant: absent key ⇒ realization not run ⇒ never a false `Violated`.
///
/// RED until step-6.
#[test]
fn dispatch_interception_no_entry_yields_indeterminate() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Empty map — no key matching "MyGeom#realization[*]" → Indeterminate.
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(BTreeMap::new());

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "BT3 / C1: no achieved entry → Indeterminate (never a false Violated)"
    );
}
