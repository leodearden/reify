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

// ─── headline Phase-1 integration: spec-shape Bracket lowering pin ───────────

/// Spec-shape `Bracket : Physical` lowers `param geometry : Solid` to a
/// **realization** (not a value cell), and a `value_cell` for `geometry`
/// does NOT exist.
///
/// This is the unique coverage this file contributes over its sibling
/// `structural_physical_tests.rs`, which pins clean compilation + presence
/// of `mass` / `centroid` / `material` value cells + the `Physical` trait
/// bound. The "Solid-typed params lower to realizations" invariant
/// (rooted in `is_representable_cell_type` rejecting `Type::Geometry`) is
/// not pinned anywhere else in the structural_physical test files; this
/// test covers it as the cross-product check between SIR-α struct-member
/// access, the geometry-query dispatch arm, and the realization-lowering
/// path for `Solid` params.
///
/// Sibling test for the redundant trait-schema + `mass`/`centroid` checks:
/// see `physical_trait_has_geometry_and_material_params_only` +
/// `physical_trait_has_mass_and_centroid_lets` +
/// `bracket_conforms_to_physical_with_geometry_and_material` in
/// `structural_physical_tests.rs`.
#[test]
fn spec_shape_physical_bracket_lowers_geometry_to_realization() {
    let source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
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
        "spec-shape `Bracket : Physical` should compile clean (no errors); got: {:?}",
        errors
    );

    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket template should be compiled");

    // `geometry : Solid` lowers to a realization (not a value cell) because
    // `Type::Geometry` is unrepresentable per `is_representable_cell_type`
    // — mirrors `solid_param_tests::solid_param_compiles_as_realization`.
    assert!(
        !bracket.realizations.is_empty(),
        "Bracket should have at least one realization (from `param geometry : Solid = box(...)`); got none"
    );
    assert!(
        !bracket
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "geometry"),
        "Bracket must NOT have a value cell for 'geometry' (Solid-typed params \
         lower to realizations); got members: {:?}",
        bracket
            .value_cells
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );
}
