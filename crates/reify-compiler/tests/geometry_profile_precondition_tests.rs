//! Tests for the geometry **profile-precondition** seam (PRD
//! `docs/prds/geometry-primitive-constructors.md` task α, Phase 1):
//!
//! 1. the four new stdlib marker traits `Planar` / `Curve` / `Surface` /
//!    `Solid` resolve (this file's first test), and
//! 2. the eight profile-consumer geometry ops (`extrude`, `extrude_symmetric`,
//!    `revolve`, `loft`, `loft_guided`, `sweep`, `sweep_guided`, `pipe`) emit
//!    `DiagnosticCode::GeometryProfileRequired` when a statically-known operand
//!    has the wrong dimensionality, while remaining **permissive** for opaque
//!    `param`/`let` operands (the FunctionCall-guard — PRD decision 5).
//!
//! The data-model and inference-table tests live in the sibling file
//! `geometry_traits_inference_tests.rs`; this file is reserved for the stdlib
//! markers and the end-to-end consumer-wiring behaviour.

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ─── stdlib markers: Planar + dimensionality (Curve/Surface/Solid) ──────────

/// Declaring geometry params with the four new marker traits (`Planar`,
/// `Surface`, `Curve`, `Solid`) must resolve cleanly — no
/// `DiagnosticCode::UnresolvedTrait`, and no error naming any of the four as
/// unresolved/unknown.
///
/// RED until the markers are added to
/// `crates/reify-compiler/stdlib/geometry_traits.ri` (step-2): an undeclared
/// trait used as a param type produces an `unresolved trait` diagnostic, so
/// before the markers exist this test fails loudly. After they are declared,
/// the param annotations resolve and the test passes.
#[test]
fn marker_traits_planar_and_dimensionality_resolve() {
    let source = r#"
        structure def UsesMarkers {
            param a : Planar
            param b : Surface
            param c : Curve
            param d : Solid
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    // (a) No UnresolvedTrait diagnostics whatsoever.
    let unresolved: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedTrait))
        .collect();
    assert!(
        unresolved.is_empty(),
        "expected the four new marker traits (Planar/Surface/Curve/Solid) to resolve, \
         but got UnresolvedTrait diagnostic(s): {:?}",
        unresolved
    );

    // (b) Defensive: no error message names any of the four markers as
    // unresolved/unknown (catches a differently-coded "unknown type" path).
    for marker in ["Planar", "Surface", "Curve", "Solid"] {
        let flagged: Vec<_> = errors
            .iter()
            .filter(|d| {
                d.message.contains(marker)
                    && (d.message.contains("unresolved") || d.message.contains("unknown"))
            })
            .collect();
        assert!(
            flagged.is_empty(),
            "marker trait {:?} should be declared and resolve, but an error flagged it as \
             unresolved/unknown: {:?}",
            marker,
            flagged
        );
    }
}

// ─── consumer wiring: profile-precondition diagnostic (end-to-end) ──────────
//
// The profile-consuming ops emit `GeometryProfileRequired` ONLY for a
// statically-known mismatched operand (a nested geometry constructor →
// FunctionCall CompiledExpr). `param`/`let` operands compile to ValueRefs and
// are skipped (permissive back-compat — PRD decision 5). RED on the rejection
// cases until step-12 wires `check_profile_preconditions`; the permissive cases
// are regression guards.

/// Count of `GeometryProfileRequired` error diagnostics produced by `source`.
fn profile_required_count(source: &str) -> usize {
    let compiled = compile_source_with_stdlib(source);
    errors_only(&compiled)
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryProfileRequired))
        .count()
}

/// `extrude(box(...))` — a statically-Solid operand at a Surface-profile slot is
/// rejected.
#[test]
fn extrude_of_box_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = extrude(box(10mm, 10mm, 10mm), 5mm) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for extrude(box(...)), got {n}");
}

/// `revolve(box(...))` — Solid profile rejected.
#[test]
fn revolve_of_box_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = revolve(box(10mm, 10mm, 10mm), 0mm, 0mm, 0mm, 0.0, 1.0, 0.0, 3.14) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for revolve(box(...)), got {n}");
}

/// `extrude_symmetric(box(...))` — Solid profile rejected.
#[test]
fn extrude_symmetric_of_box_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = extrude_symmetric(box(10mm, 10mm, 10mm), 5mm) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for extrude_symmetric(box(...)), got {n}");
}

/// `extrude(line_segment(...))` — a statically-Curve operand at a Surface-profile
/// slot is rejected (a Curve is not a Surface).
#[test]
fn extrude_of_curve_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = extrude(line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm), 5mm) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for extrude(line_segment(...)), got {n}");
}

/// `pipe(box(...))` — a Solid operand at a Curve-path slot is rejected.
#[test]
fn pipe_of_box_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = pipe(box(10mm, 10mm, 10mm), 5mm) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for pipe(box(...)), got {n}");
}

/// `sweep(box(...), box(...))` — Solid path (and Solid profile) rejected.
#[test]
fn sweep_with_solid_path_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = sweep(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm)) }",
    );
    assert!(n >= 1, "expected GeometryProfileRequired for sweep(box(...), box(...)), got {n}");
}

/// PERMISSIVE: `extrude(p, ...)` for `param p : Solid` — `p` is a ValueRef, not a
/// FunctionCall, so the check is skipped (the load-bearing back-compat pin).
#[test]
fn extrude_of_solid_param_is_accepted() {
    let n = profile_required_count(
        "structure def S { param p : Solid  let r = extrude(p, 5mm) }",
    );
    assert_eq!(n, 0, "param operand must be permissive, got {n} GeometryProfileRequired");
}

/// PERMISSIVE: `pipe(line_segment(...))` — a statically-Curve operand at a
/// Curve-path slot is accepted.
#[test]
fn pipe_of_curve_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = pipe(line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm), 5mm) }",
    );
    assert_eq!(n, 0, "Curve path must be accepted, got {n} GeometryProfileRequired");
}

/// PERMISSIVE (sweep_degenerate-style): `loft` over let-bound extrudes of params.
/// Every profile operand is a ValueRef → skipped.
#[test]
fn loft_over_let_bound_params_is_accepted() {
    let n = profile_required_count(
        "structure def S { \
            param a : Solid  param b : Solid  param c : Solid  \
            let s1 = extrude(a, 5mm)  let s2 = extrude(b, 5mm)  let s3 = extrude(c, 5mm)  \
            let r = loft(s1, s2, s3) \
        }",
    );
    assert_eq!(n, 0, "let-bound profile operands must be permissive, got {n} GeometryProfileRequired");
}
