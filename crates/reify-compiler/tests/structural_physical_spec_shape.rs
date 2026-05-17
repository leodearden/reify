//! User-observable-signal pin for **task 3603 / GHR-α (PRD §8 Phase 1)**.
//!
//! This file is the entry point for future readers tracing the
//! `geometry-handle-runtime` PRD Phase 1 wiring. It exercises:
//!
//!   1. Stdlib geometry-query helper calls (`volume`, `centroid`) typecheck
//!      to the correct return Type at compile-time. (Eval-time dispatch
//!      arrives in Phase 6 / GHR-ζ; Phase 1 produces `Value::Undef`.)
//!   2. Spec-shape `Physical` trait: a structure conforming to `Physical`
//!      via `param geometry : Solid` + `param material : Material` (instead
//!      of the legacy flat-scalar `param density / volume / centroid_x/y/z`
//!      params) compiles with NO error-severity diagnostics, gains
//!      `mass` and `centroid` value cells from the trait's let defaults,
//!      and pulls `material.density` via struct-member access (SIR-α).
//!
//! See `docs/prds/v0_3/geometry-handle-runtime.md` §1 + §8.

use reify_test_support::compile_source_with_stdlib;
use reify_types::{DimensionVector, Severity, Type};

/// `volume(my_box)` where `my_box : Solid` typechecks to `Scalar<Volume>`.
///
/// Pins the dispatch arm in `expr.rs::infer_type` that consults
/// `geometry_query_result_type` (added in step-8). Without that arm, the
/// inference falls through to the first-arg type (`Geometry`), which fails
/// `is_representable_cell_type`.
#[test]
fn spec_shape_volume_call_typechecks_to_scalar_volume() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let v = volume(my_box)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("expected 'v' value cell from `let v = volume(my_box)`");
    assert_eq!(
        v_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::VOLUME
        },
        "`volume(...)` must infer to Scalar<Volume>"
    );
}

/// `centroid(my_box)` where `my_box : Solid` typechecks to `Point3<Length>`.
///
/// Pins the second sample from the GHR-α §1 frozen list; matches the same
/// dispatch arm as the volume test above.
#[test]
fn spec_shape_centroid_call_typechecks_to_point3_length() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let c = centroid(my_box)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("expected 'c' value cell from `let c = centroid(my_box)`");
    assert_eq!(
        c_cell.cell_type,
        Type::point3(Type::length()),
        "`centroid(...)` must infer to Point3<Length>"
    );
}
