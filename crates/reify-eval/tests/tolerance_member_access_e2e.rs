//! E2E tests for `RepresentationWithin` with member-access subjects (task #3467).
//!
//! Verifies that `engine.check()` correctly reports `Violated`/`Satisfied` for
//! `RepresentationWithin(bracket.fea_subject, 1mm)` — a member-access subject
//! rather than a bare param.
//!
//! Uses the non-OCCT inject-then-check pattern from `representation_within_assertion.rs`
//! (BT1-BT3): compile with stdlib, inject `achieved_repr_tol` via the test-
//! instrumentation setter, call `engine.check()`, assert the satisfaction.
//!
//! # RED/GREEN
//!
//! RED (before step-3): Gate 3 of `match_representation_within_shape` only
//! accepts `ValueRef` subjects — the `IndexAccess` member-access shape is
//! silently dropped → the constraint resolves `Indeterminate` (not `Violated`).
//!
//! GREEN (after step-3): Gate 3 is widened to also accept `IndexAccess{ValueRef,
//! Literal(String)}:StructureRef` → the constraint is recognized, resolved via
//! the type-name scan ("FeaFace#realization["), and compared against the bound.

use reify_ir::Satisfaction;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use std::collections::BTreeMap;

// ── Shared fixture from examples/fea_bracket_member_access.ri ────────────────

/// The pre-2 fixture source, shared with `examples_smoke` (compile_with_stdlib).
///
/// FeaFace + Bracket + FeaCheck carrying
/// `constraint RepresentationWithin(bracket.fea_subject, 1mm)`.
///
/// Uses only built-in constructs (mm, Real, structures) — no stdlib-only types.
const MEMBER_ACCESS_SOURCE: &str = include_str!("../../../examples/fea_bracket_member_access.ri");

// ── POSITIVE member-access tests ─────────────────────────────────────────────

/// MA-1 (over-bound): achieved tolerance (5e-3 m) > 1mm bound → `Violated`.
///
/// Injects `"FeaFace#realization[0]": 5e-3` into `achieved_repr_tol`.
/// The type-name scan in `resolve_repr_tol_key` finds the key via the
/// `"FeaFace"` struct name from `bracket.fea_subject`'s result_type.
///
/// RED until step-3 widens Gate 3: before the widening, the IndexAccess
/// subject is dropped → constraint resolves `Indeterminate` (not `Violated`),
/// failing the assertion.
#[test]
fn ma1_member_access_over_bound_yields_violated() {
    let compiled = parse_and_compile_with_stdlib(MEMBER_ACCESS_SOURCE);

    // Confirm no Error diagnostics from compile_with_stdlib.
    {
        use reify_core::Severity;
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "MA-1: compile_with_stdlib must produce zero Error diagnostics; \
             got: {:#?}",
            errors
        );
    }

    let mut engine = make_simple_engine();

    // Inject achieved_repr_tol: 5e-3 m > 1mm (1e-3 m) → must yield Violated.
    let mut map = BTreeMap::new();
    map.insert("FeaFace#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    // Find the FeaCheck constraint (entity="FeaCheck", index=0).
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "FeaCheck" && e.id.index == 0)
        .expect("must have FeaCheck#constraint[0] (RepresentationWithin)");

    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "MA-1: member-access subject — achieved 5e-3 m > 1mm bound → Violated. \
         RED until step-3: currently returns Indeterminate (subject dropped)"
    );
}

/// MA-2 (under-bound): achieved tolerance (1e-9 m) ≪ 1mm bound → `Satisfied`.
///
/// Injects `"FeaFace#realization[0]": 1e-9` into `achieved_repr_tol`.
///
/// RED until step-3: before the widening, the IndexAccess subject is dropped
/// → `Indeterminate` (not `Satisfied`), failing the assertion.
#[test]
fn ma2_member_access_under_bound_yields_satisfied() {
    let compiled = parse_and_compile_with_stdlib(MEMBER_ACCESS_SOURCE);

    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1mm = 1e-3 m bound → Satisfied.
    let mut map = BTreeMap::new();
    map.insert("FeaFace#realization[0]".to_string(), 1e-9_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "FeaCheck" && e.id.index == 0)
        .expect("must have FeaCheck#constraint[0] (RepresentationWithin)");

    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "MA-2: member-access subject — achieved 1e-9 m < 1mm bound → Satisfied. \
         RED until step-3: currently returns Indeterminate (subject dropped)"
    );
}

// ── REGRESSION: bare-param subjects still work ───────────────────────────────

/// Inline bare-param fixture (no member access, no stdlib needed).
///
/// Mirrors the BT1-BT3 pattern in representation_within_assertion.rs: the
/// subject is a bare param `subject : MyGeom`, not a member access.
const BARE_PARAM_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}
structure Checker {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// REG-1: bare-param subject (no member access) — achieved 5e-3 m > 1mm → Violated.
///
/// Regression: the step-3 widening is purely additive; existing ValueRef path
/// must remain byte-identical. This test is GREEN before AND after step-3.
#[test]
fn reg1_bare_param_over_bound_yields_violated() {
    let compiled = parse_and_compile_with_stdlib(BARE_PARAM_SOURCE);

    let mut engine = make_simple_engine();
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");

    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "REG-1: bare-param ValueRef subject — achieved 5e-3 m > 1mm bound → Violated \
         (must pass before AND after step-3 widening)"
    );
}

/// REG-2: bare-param subject (no member access) — achieved 1e-9 m ≪ 1mm → Satisfied.
///
/// Regression: GREEN before AND after step-3.
#[test]
fn reg2_bare_param_under_bound_yields_satisfied() {
    let compiled = parse_and_compile_with_stdlib(BARE_PARAM_SOURCE);

    let mut engine = make_simple_engine();
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 1e-9_f64);
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
        "REG-2: bare-param ValueRef subject — achieved 1e-9 m < 1mm bound → Satisfied \
         (must pass before AND after step-3 widening)"
    );
}
