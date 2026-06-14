//! INV-2 erasure boundary regression pin (task 4233 δ step-9).
//!
//! Pins the D1 invariant from β: generic function evaluation is type-erased —
//! the result of `id<T>(x: T) -> T` is identical to a monomorphic `id_length`
//! and the eval trace does not depend on the inferred type argument.
//!
//! These tests PASS immediately (confirming the gate) because the erasure
//! machinery landed in β. If they ever go RED, it signals an erasure regression.
//!
//! Uses the same compile_source + cell_expr + eval_expr pattern as
//! crates/reify-compiler/tests/fn_generic_call_inference_tests.rs.

use reify_core::DimensionVector;
use reify_ir::{Value, ValueMap};
use reify_test_support::compile_source;

/// Locate the `default_expr` of a named value cell in the first template.
fn cell_expr<'a>(
    module: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = &module.templates[0];
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default_expr"))
}

/// INV-2: a generic call `id(5mm)` evaluates identically to its monomorphic
/// equivalent `id_length(5mm)` — the eval trace is type-erased (D1).
///
/// Both cells must eval to Value::Scalar{ si_value: 0.005, LENGTH }.
#[test]
fn generic_call_eval_identical_to_monomorphic() {
    let source = "fn id<T>(x: T) -> T { x } \
                  fn id_length(x: Length) -> Length { x } \
                  structure S { let g = id(5mm)  let m = id_length(5mm) }";
    let module = compile_source(source);

    let expected = Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    };

    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    let g_val = reify_expr::eval_expr(cell_expr(&module, "g"), &ctx);
    assert_eq!(
        g_val, expected,
        "id(5mm) [generic] should evaluate to 5mm scalar, got {g_val:?}"
    );

    let m_val = reify_expr::eval_expr(cell_expr(&module, "m"), &ctx);
    assert_eq!(
        m_val, expected,
        "id_length(5mm) [monomorphic] should evaluate to 5mm scalar, got {m_val:?}"
    );

    assert_eq!(g_val, m_val, "INV-2: generic eval must equal monomorphic eval");
}

/// D1 (type erasure): `id<T>(x: T) -> T` evals its argument verbatim for any
/// inferred T — Int, Real, Bool. The body is evaluated monomorphically-by-value
/// regardless of the inferred type argument.
#[test]
fn generic_call_is_type_arg_agnostic() {
    let source = "fn id<T>(x: T) -> T { x } \
                  structure S { let i = id(3)  let r = id(2.5)  let b = id(true) }";
    let module = compile_source(source);

    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    let i_val = reify_expr::eval_expr(cell_expr(&module, "i"), &ctx);
    assert_eq!(i_val, Value::Int(3), "id(3) should eval to Int(3), got {i_val:?}");

    let r_val = reify_expr::eval_expr(cell_expr(&module, "r"), &ctx);
    assert_eq!(r_val, Value::Real(2.5), "id(2.5) should eval to Real(2.5), got {r_val:?}");

    let b_val = reify_expr::eval_expr(cell_expr(&module, "b"), &ctx);
    assert_eq!(b_val, Value::Bool(true), "id(true) should eval to Bool(true), got {b_val:?}");
}
