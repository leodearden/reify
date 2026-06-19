//! Engine-level activation/eval tests for the §12 determinacy intrinsics
//! `AllParamsDetermined` / `AllGeometryDetermined` (task-4197 α).
//!
//! Mirrors `purpose_activation.rs:1200-1268` but uses the compiler-sugar
//! intrinsic form instead of the hand-written `forall __p in subject.params:
//! determined(__p)` expression.  These tests lock the cross-crate composition:
//! desugar (reify-compiler) → reflective expansion (reify-eval) → determined
//! predicate evaluation (reify-eval) → Satisfaction::Satisfied / Violated.
//!
//! Both tests use a locally-declared `design_review` purpose (no stdlib dep).

use reify_core::Severity;
use reify_ir::{CompiledExprKind, Satisfaction};
use reify_test_support::{make_simple_engine, parse_and_compile};

// ── BT4-1: AllParamsDetermined — Satisfied for a fully-determined structure ──

/// A structure whose only param has a concrete default value (`= 1.0`) is
/// fully determined.  `AllParamsDetermined(subject)` desugars to
/// `forall __p in subject.params: determined(__p)` at compile time; the engine
/// expands `.params` at activation and evaluates `determined` per cell.
/// All cells are determined → `forall` true → `Satisfaction::Satisfied`.
///
/// Also asserts that the compiled constraint's expr kind is
/// `CompiledExprKind::Quantifier { .. }` (proves the desugar rode the
/// existing reflective path, not a new eval primitive).
#[test]
fn all_params_determined_satisfied_for_fully_determined_structure() {
    let source = r#"
structure DeterminedThing {
    param x : Real = 1.0
}

purpose design_review(subject : Structure) {
    constraint AllParamsDetermined(subject)
}
"#;
    let compiled = parse_and_compile(source);

    // Fixture must compile cleanly.
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "fixture must produce exactly one compiled purpose"
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    // The desugar must have produced a Quantifier, not a plain FunctionCall.
    let constraint_expr = &compiled.compiled_purposes[0].constraints[0].expr;
    assert!(
        matches!(constraint_expr.kind, CompiledExprKind::Quantifier { .. }),
        "AllParamsDetermined must desugar to a Quantifier expression; \
         got {:?}",
        constraint_expr.kind
    );

    // Engine eval + activation.
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    engine.activate_purpose("design_review", "DeterminedThing");

    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    let purpose_result = constraint_results
        .iter()
        .find(|e| {
            e.id.entity
                .starts_with("purpose:design_review@DeterminedThing")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:design_review@DeterminedThing'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedThing.x has a default (= 1.0) so AllParamsDetermined must be Satisfied; \
         all params are determined → forall true → Satisfied.",
    );
}

// ── BT4-2: AllParamsDetermined — Violated for an undetermined param ──────────

/// A structure whose param has no default value is not fully determined.
/// `AllParamsDetermined(subject)` desugars to the reflective forall; the engine
/// evaluates `determined(UndeterminedThing.x)` → false → `Satisfaction::Violated`.
///
/// Mirrors `purpose_activation.rs::manufacturing_ready_violates_for_undetermined_params`
/// but uses the intrinsic form.
#[test]
fn all_params_determined_violated_for_undetermined_param() {
    let source = r#"
structure UndeterminedThing {
    param x : Real
}

purpose design_review(subject : Structure) {
    constraint AllParamsDetermined(subject)
}
"#;
    let compiled = parse_and_compile(source);

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    engine.activate_purpose("design_review", "UndeterminedThing");

    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    let purpose_result = constraint_results
        .iter()
        .find(|e| {
            e.id.entity
                .starts_with("purpose:design_review@UndeterminedThing")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:design_review@UndeterminedThing'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Violated,
        "UndeterminedThing.x has no default so AllParamsDetermined must be Violated; \
         determined(UndeterminedThing.x) = false → forall false → Violated.",
    );
}

// ── BT4-3: AllGeometryDetermined — Satisfied for a fully-determined structure ──

/// A structure whose only geometric (Length-typed) param has a concrete default
/// value is fully determined geometrically.  `AllGeometryDetermined(subject)`
/// desugars to `forall __p in subject.geometric_params: determined(__p)` at
/// compile time; the engine expands `.geometric_params` at activation and
/// evaluates `determined` per cell.  All geometric cells are determined →
/// `forall` true → `Satisfaction::Satisfied`.
///
/// Also asserts that the compiled constraint's expr kind is
/// `CompiledExprKind::Quantifier { .. }` (proves the desugar rode the existing
/// reflective path for the geometric variant, not a new eval primitive).
///
/// Pins the `geometric_params` expansion path introduced by task-4137 for the
/// `AllGeometryDetermined` intrinsic end-to-end.
#[test]
fn all_geometry_determined_satisfied_for_fully_determined_structure() {
    let source = r#"
structure DeterminedGeomThing {
    param width : Length = 80mm
}

purpose geometry_review(subject : Structure) {
    constraint AllGeometryDetermined(subject)
}
"#;
    let compiled = parse_and_compile(source);

    // Fixture must compile cleanly.
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "fixture must produce exactly one compiled purpose"
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    // The desugar must have produced a Quantifier (geometric variant).
    let constraint_expr = &compiled.compiled_purposes[0].constraints[0].expr;
    assert!(
        matches!(constraint_expr.kind, CompiledExprKind::Quantifier { .. }),
        "AllGeometryDetermined must desugar to a Quantifier expression; \
         got {:?}",
        constraint_expr.kind
    );

    // Engine eval + activation.
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    engine.activate_purpose("geometry_review", "DeterminedGeomThing");

    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    let purpose_result = constraint_results
        .iter()
        .find(|e| {
            e.id.entity
                .starts_with("purpose:geometry_review@DeterminedGeomThing")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:geometry_review@DeterminedGeomThing'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedGeomThing.width has a default (= 80mm) so AllGeometryDetermined \
         must be Satisfied; all geometric params are determined → forall true → Satisfied.",
    );
}

// ── BT4-4: AllGeometryDetermined — Violated for an undetermined geometric param ──

/// A structure whose geometric (Length-typed) param has no default value is not
/// fully determined geometrically.  `AllGeometryDetermined(subject)` desugars to
/// the reflective forall for `.geometric_params`; the engine evaluates
/// `determined(UndetGeomThing.width)` → false → `Satisfaction::Violated`.
///
/// Pins the end-to-end chain: desugar → geometric_params expansion (task-4137)
/// → determined predicate → Violated, for the AllGeometryDetermined intrinsic.
#[test]
fn all_geometry_determined_violated_for_undetermined_geometric_param() {
    let source = r#"
structure UndetGeomThing {
    param width : Length
}

purpose geometry_review(subject : Structure) {
    constraint AllGeometryDetermined(subject)
}
"#;
    let compiled = parse_and_compile(source);

    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture produced unexpected error diagnostics: {:?}",
        compiled.diagnostics
    );

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    engine.activate_purpose("geometry_review", "UndetGeomThing");

    let (constraint_results, _) = engine
        .check_constraints_with_values(&eval_result.values)
        .expect("check_constraints_with_values must not return an error");

    let purpose_result = constraint_results
        .iter()
        .find(|e| {
            e.id.entity
                .starts_with("purpose:geometry_review@UndetGeomThing")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a purpose-injected constraint with entity prefix \
                 'purpose:geometry_review@UndetGeomThing'; found ids: {:?}",
                constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        purpose_result.satisfaction,
        Satisfaction::Violated,
        "UndetGeomThing.width has no default so AllGeometryDetermined must be Violated; \
         determined(UndetGeomThing.width) = false → forall false → Violated.",
    );
}
