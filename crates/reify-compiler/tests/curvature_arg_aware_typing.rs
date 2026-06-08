//! Compiler-typing integration test for **task 4315**: arg-type-aware
//! compile-time return typing for `curvature()`.
//!
//! Pins the user-observable `reify check` signal after the expr.rs wiring in
//! step-4:
//!   - `curvature(faces(sphere)[0], pt)` — inline `faces(...)[i]` → `Matrix<2,2,Curvature>`
//!   - `curvature(edges(cyl)[0], pt)`    — inline `edges(...)[i]` → `Scalar<Curvature>`
//!
//! Tests **fail** until step-4 wires `geometry_query_arg_aware_result_type`
//! into `expr.rs`'s `is_geometry_query` arm — at that point both cells are
//! expected to have different types, but right now they both type as
//! `Scalar<Curvature>` (no arg-type discrimination).
//!
//! Uses `reify_test_support::{compile_source_with_stdlib, errors_only}` and
//! the `value_cells` cell-type introspection pattern from
//! `structural_physical_spec_shape.rs`.

use reify_core::{DimensionVector, Type};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// Source snippet: two curvature calls, one with an inline surface arg
/// (faces(...)[0]) and one with an inline curve arg (edges(...)[0]).
/// Uses a point3 literal as the second arg to satisfy the call signature.
const SOURCE: &str = r#"
structure def CurvatureTypingCheck {
    let pt_s = point3(5mm, 0mm, 0mm)
    let pt_c = point3(10mm, 0mm, 0mm)
    let k_surf = curvature(faces(sphere(5mm))[0], pt_s)
    let k_curve = curvature(edges(cylinder(10mm, 20mm))[0], pt_c)
}
"#;

/// No error-severity diagnostics after compilation.
#[test]
fn curvature_arg_aware_typing_compiles_without_errors() {
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no error-severity diagnostics, got: {errors:#?}"
    );
}

/// `curvature(faces(sphere)[0], pt)` must type as `Matrix<2,2,Curvature>`.
///
/// Fails until step-4 wires geometry_query_arg_aware_result_type into expr.rs.
#[test]
fn curvature_inline_surface_arg_types_as_matrix_2x2_curvature() {
    let compiled = compile_source_with_stdlib(SOURCE);
    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let k_surf_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "k_surf")
        .expect("expected 'k_surf' value cell");
    let expected = Type::Matrix {
        m: 2,
        n: 2,
        quantity: Box::new(Type::Scalar {
            dimension: DimensionVector::CURVATURE,
        }),
    };
    assert_eq!(
        k_surf_cell.cell_type, expected,
        "`curvature(faces(...)[i], pt)` must compile-type as Matrix{{2,2,Curvature}}, \
         got {:?}",
        k_surf_cell.cell_type
    );
}

/// `curvature(edges(cyl)[0], pt)` must type as `Scalar<Curvature>`.
///
/// Confirms the curve path falls through to the existing default.
#[test]
fn curvature_inline_curve_arg_types_as_scalar_curvature() {
    let compiled = compile_source_with_stdlib(SOURCE);
    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let k_curve_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "k_curve")
        .expect("expected 'k_curve' value cell");
    let expected = Type::Scalar {
        dimension: DimensionVector::CURVATURE,
    };
    assert_eq!(
        k_curve_cell.cell_type, expected,
        "`curvature(edges(...)[i], pt)` must compile-type as Scalar<Curvature>, \
         got {:?}",
        k_curve_cell.cell_type
    );
}
