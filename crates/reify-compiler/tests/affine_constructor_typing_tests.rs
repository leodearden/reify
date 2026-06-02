//! Compiler typing tests for the AffineMap constructor free-functions
//! (task 3960, PRD §4.2 task β): `affine_scale`, `affine_identity`, etc. must
//! resolve their call-site cell type to `Type::AffineMap(3)` rather than the
//! first-arg fallback, and the zero-arg `affine_identity()` must NOT trip the
//! "cannot infer return type of zero-arg function" warning.
//!
//! RED today: `expr.rs`'s result-type cascade has no `is_affine_map_constructor`
//! arm, so `affine_scale(...)` types as its first arg (`Real`) and
//! `affine_identity()` falls to the zero-arg fallback (typed `Real` + warning).

use reify_core::Type;
use reify_test_support::compile_source;

#[test]
fn affine_constructors_type_as_affine_map_3() {
    let source = r#"
        structure AffineHost {
            let m = affine_scale(2.0, 1.0, 0.5)
            let i = affine_identity()
        }
    "#;
    let compiled = compile_source(source);

    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "AffineHost")
        .expect("AffineHost template");

    for cell_name in ["m", "i"] {
        let cell = host
            .value_cells
            .iter()
            .find(|vc| vc.id.member.as_str() == cell_name)
            .unwrap_or_else(|| panic!("value cell '{cell_name}' not found in AffineHost"));
        let default_expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("cell '{cell_name}' has no default_expr"));
        assert_eq!(
            default_expr.result_type,
            Type::AffineMap(3),
            "cell '{cell_name}' must type as AffineMap(3), got {:?}",
            default_expr.result_type
        );
    }

    // affine_identity() is zero-arg, but registration means it must NOT trip the
    // first-arg/zero-arg fallback warning.
    let zero_arg_warning = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message
                .contains("cannot infer return type of zero-arg function")
        });
    assert!(
        zero_arg_warning.is_none(),
        "affine_identity() must not emit the zero-arg fallback warning, got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );
}
