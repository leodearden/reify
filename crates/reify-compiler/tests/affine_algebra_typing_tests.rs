//! Compiler typing tests for the AffineMap algebra free-functions
//! (task 3961, PRD §4.3 task γ): `affine_compose`, `affine_inverse`, and
//! `determinant` must resolve their call-site cell types correctly rather than
//! falling through to the first-arg fallback.
//!
//! RED today: `expr.rs`'s result-type cascade has no algebra arm, so
//! `affine_compose(...)` → AffineMap(3) (accidentally correct via first-arg),
//! `affine_inverse(...)` → AffineMap(3) (WRONG, should be Option<AffineMap(3)>),
//! `determinant(AffineMap)` → AffineMap(3) (WRONG, should be Real).

use reify_core::Type;
use reify_test_support::compile_source;

/// Helper: find a value cell by name within a named template.
fn find_cell_type(source: &str, template_name: &str, cell_name: &str) -> Type {
    let compiled = compile_source(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| panic!("template '{template_name}' not found"));
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == cell_name)
        .unwrap_or_else(|| panic!("value cell '{cell_name}' not found in {template_name}"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{cell_name}' has no default_expr"))
        .result_type
        .clone()
}

#[test]
fn affine_compose_types_as_affine_map_3() {
    let source = r#"
        structure AlgebraHost {
            let c = affine_compose(affine_scale(2.0, 1.0, 0.5), affine_shear_xy(0.5))
        }
    "#;
    let ty = find_cell_type(source, "AlgebraHost", "c");
    assert_eq!(
        ty,
        Type::AffineMap(3),
        "affine_compose must type as AffineMap(3), got {:?}",
        ty
    );
}

#[test]
fn affine_inverse_types_as_option_affine_map_3() {
    let source = r#"
        structure AlgebraHost {
            let c = affine_compose(affine_scale(2.0, 1.0, 0.5), affine_shear_xy(0.5))
            let inv = affine_inverse(c)
        }
    "#;
    let ty = find_cell_type(source, "AlgebraHost", "inv");
    assert_eq!(
        ty,
        Type::Option(Box::new(Type::AffineMap(3))),
        "affine_inverse must type as Option(AffineMap(3)), got {:?}",
        ty
    );
}

#[test]
fn determinant_of_affine_map_types_as_real() {
    let source = r#"
        structure AlgebraHost {
            let c = affine_compose(affine_scale(2.0, 3.0, 4.0), affine_shear_xy(0.5))
            let d = determinant(c)
        }
    "#;
    let ty = find_cell_type(source, "AlgebraHost", "d");
    assert_eq!(
        ty,
        Type::Real,
        "determinant(AffineMap) must type as Real, got {:?}",
        ty
    );
}

#[test]
fn affine_inverse_is_distinct_from_affine_constructors() {
    // affine_inverse returns Option<AffineMap> — if it were in the constructor
    // list it would type as AffineMap(3) (wrong). This test verifies the typing
    // is Option(AffineMap(3)) as a proxy for "not in the constructor arm".
    let source = r#"
        structure AlgebraHost {
            let a = affine_scale(1.0, 2.0, 3.0)
            let inv = affine_inverse(a)
        }
    "#;
    let ty = find_cell_type(source, "AlgebraHost", "inv");
    assert_ne!(
        ty,
        Type::AffineMap(3),
        "affine_inverse must NOT type as AffineMap(3); that would indicate it's in the constructor arm"
    );
}

#[test]
fn no_zero_arg_warning_for_determinant_with_affine_map_arg() {
    // determinant(affine_map) has an arg, so the zero-arg fallback warning
    // must not fire. This is a sanity check rather than a hard requirement.
    let source = r#"
        structure AlgebraHost {
            let c = affine_scale(2.0, 3.0, 4.0)
            let d = determinant(c)
        }
    "#;
    let compiled = compile_source(source);
    let zero_arg_warning = compiled
        .diagnostics
        .iter()
        .find(|d| d.message.contains("cannot infer return type of zero-arg function"));
    assert!(
        zero_arg_warning.is_none(),
        "determinant(AffineMap) must not emit the zero-arg fallback warning, got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );
}
