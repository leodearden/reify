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
