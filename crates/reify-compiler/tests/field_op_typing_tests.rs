//! Compiler typing tests for the std.fields α field-op free-functions
//! (task 4219, PRD docs/prds/v0_6/std-fields-api.md §5.1).
//!
//! KEY INVARIANT (§5.1): `sample(fn_field(|p| …), point)` must type as the
//! **codomain** of the field, NOT as `Field<…>` or as the function's first
//! argument.  This is the headline fix delivered by the field-op compiler arm.
//!
//! Model: `affine_constructor_typing_tests.rs` — same compile-source → find
//! template → find cell → assert result_type pattern.
//!
//! RED today: `expr.rs`'s NoUserFunctions cascade has no `is_field_op` arm,
//! so `fn_field(|p| 2.0 * p)` types as its first-arg fallback
//! (`Function{[Real]→Real}`) and `sample(fn_field(…), 3.0)` inherits the
//! same first-arg fallback instead of resolving to `Real` (the codomain).

use reify_core::Type;
use reify_test_support::compile_source;

#[test]
fn field_op_sample_types_as_codomain_not_field() {
    // `fn_field(|p| 2.0 * p)` — one Real param, Real body → Field<Real, Real>
    // `sample(fn_field(|p| 2.0 * p), 3.0)` — sample of that field at 3.0 → Real
    let source = r#"
        structure FieldHost {
            let f = fn_field(|p| 2.0 * p)
            let s = sample(fn_field(|p| 2.0 * p), 3.0)
        }
    "#;
    let compiled = compile_source(source);

    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "FieldHost")
        .expect("FieldHost template must compile");

    // Cell `f` must type as Field<Real, Real>
    let f_cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "f")
        .expect("value cell 'f' must exist in FieldHost");
    let f_expr = f_cell
        .default_expr
        .as_ref()
        .expect("cell 'f' must have a default_expr");
    assert_eq!(
        f_expr.result_type,
        Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        },
        "fn_field(|p| 2.0 * p) must type as Field<Real,Real> (PRD §5.1), got {:?}",
        f_expr.result_type
    );

    // Cell `s` must type as Real — THE §5.1 INVARIANT
    let s_cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "s")
        .expect("value cell 's' must exist in FieldHost");
    let s_expr = s_cell
        .default_expr
        .as_ref()
        .expect("cell 's' must have a default_expr");
    assert_eq!(
        s_expr.result_type,
        Type::dimensionless_scalar(),
        "sample(fn_field(|p| 2.0 * p), 3.0) must type as Real (codomain), not Field or \
         Function (PRD §5.1 THE FIX), got {:?}",
        s_expr.result_type
    );

    // fn_field is a registered builtin: it must NOT emit the zero-arg-function
    // fallback warning (matches the affine_constructor_typing_tests.rs pattern).
    let zero_arg_warning = compiled.diagnostics.iter().find(|d| {
        d.message
            .contains("cannot infer return type of zero-arg function")
    });
    assert!(
        zero_arg_warning.is_none(),
        "fn_field/sample must not emit the zero-arg fallback warning; got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );
}
