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

// ─── task-4160: rectangle + circle profile acceptance/rejection ──────────────
//
// Once rectangle/circle infer Surface+closed+planar, the existing α-check
// accepts them at extrude/revolve/loft profile slots and rejects them at
// the Curve path slot.  Zero changes to check_profile_preconditions needed.
//
// RED until step-6 adds "rectangle"/"circle" to GEOMETRY_FUNCTION_NAMES and
// the surface()-dimension dispatch arm in geometry_traits_inference.rs.

/// `extrude(rectangle(...), dist)` — Surface profile is accepted.
///
/// RED until step-6 wires rectangle as a geometry function.
#[test]
fn extrude_of_rectangle_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = extrude(rectangle(20mm, 10mm), 3mm) }",
    );
    assert_eq!(n, 0, "extrude(rectangle(...)) must be accepted (Surface profile), got {n}");
}

/// `extrude(circle(...), dist)` — Surface profile is accepted.
///
/// RED until step-6 wires circle as a geometry function.
#[test]
fn extrude_of_circle_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = extrude(circle(8mm), 2mm) }",
    );
    assert_eq!(n, 0, "extrude(circle(...)) must be accepted (Surface profile), got {n}");
}

/// `revolve(rectangle(...), ...)` — Surface profile is accepted.
///
/// RED until step-6 wires rectangle as a geometry function.
#[test]
fn revolve_of_rectangle_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = revolve(rectangle(20mm, 10mm), 0mm, 0mm, 0mm, 0.0, 1.0, 0.0, 3.14) }",
    );
    assert_eq!(n, 0, "revolve(rectangle(...)) must be accepted (Surface profile), got {n}");
}

/// `pipe(rectangle(...), radius)` — Surface operand at the Curve path slot
/// must be rejected.
///
/// RED until step-6 wires rectangle as a geometry function.
#[test]
fn pipe_of_rectangle_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = pipe(rectangle(20mm, 10mm), 2mm) }",
    );
    assert!(n >= 1, "pipe(rectangle(...)) must be rejected (Surface≠Curve path), got {n}");
}

// ─── task-4161: polygon + ellipse profile acceptance/rejection ───────────────
//
// Once polygon/ellipse infer Surface+closed+planar, the existing α-check
// accepts them at extrude/revolve/loft profile slots and rejects them at
// curve-path slots (pipe/sweep path), mirroring rectangle/circle.
//
// RED until step-6 adds "polygon"/"ellipse" to GEOMETRY_FUNCTION_NAMES and
// wires the surface()/surface_nonconvex() inference arms.

/// `extrude(polygon(0mm,0mm, 10mm,0mm, 10mm,10mm), dist)` — Surface profile
/// is accepted with no `GeometryProfileRequired`.
///
/// RED until step-6 wires polygon as a geometry function.
#[test]
fn extrude_of_polygon_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = extrude(polygon(0mm,0mm, 10mm,0mm, 10mm,10mm), 3mm) }",
    );
    assert_eq!(n, 0, "extrude(polygon(...)) must be accepted (Surface profile), got {n}");
}

/// `extrude(ellipse(10mm, 5mm), dist)` — Surface profile is accepted with no
/// `GeometryProfileRequired`.
///
/// RED until step-6 wires ellipse as a geometry function.
#[test]
fn extrude_of_ellipse_is_accepted() {
    let n = profile_required_count(
        "structure def S { let r = extrude(ellipse(10mm, 5mm), 3mm) }",
    );
    assert_eq!(n, 0, "extrude(ellipse(...)) must be accepted (Surface profile), got {n}");
}

/// `pipe(polygon(...), radius)` — Surface operand at the Curve path slot must
/// be rejected with `GeometryProfileRequired`.
///
/// RED until step-6 wires polygon as a geometry function.
#[test]
fn pipe_of_polygon_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = pipe(polygon(0mm,0mm, 10mm,0mm, 10mm,10mm), 2mm) }",
    );
    assert!(n >= 1, "pipe(polygon(...)) must be rejected (Surface≠Curve path), got {n}");
}

/// `pipe(ellipse(...), radius)` — Surface operand at the Curve path slot must
/// be rejected with `GeometryProfileRequired`.
///
/// RED until step-6 wires ellipse as a geometry function.
#[test]
fn pipe_of_ellipse_is_rejected() {
    let n = profile_required_count(
        "structure def S { let r = pipe(ellipse(10mm, 5mm), 2mm) }",
    );
    assert!(n >= 1, "pipe(ellipse(...)) must be rejected (Surface≠Curve path), got {n}");
}
