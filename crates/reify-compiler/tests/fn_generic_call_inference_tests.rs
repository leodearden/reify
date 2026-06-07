//! Call-site type-argument inference (unification) end-to-end tests — task 4231
//! (generic-user-fns β).
//!
//! Exercises B1–B5 + INV-2/INV-6 through the full compile pipeline:
//!   - overload resolution selects a generic candidate (step-5/6),
//!   - return-type substitution at the call site (step-7/8),
//!   - `E_FN_TYPE_ARG_CONFLICT` (step-9/10) and `E_FN_TYPE_ARG_UNRESOLVED`
//!     (step-11/12) diagnostics.
//!
//! Uses `reify_test_support::compile_source` (resolves `5mm`/`Length`/`List`
//! with no stdlib) + `reify_expr` eval (INV-2 type erasure). Call-site type is
//! read via `module.templates[0].value_cells[].default_expr`.

use reify_core::DimensionVector;
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

// ── step-5 (ii): overload resolution selects the generic candidate ───────────

/// `id<T>(x: T) -> T` called as `id(5mm)` must lower to a `UserFunctionCall`
/// (NOT poison) and evaluate to the 5mm scalar — INV-2: the value is correct
/// even before return-type substitution (step-8) runs.
///
/// RED until step-6: today the `TypeParam` param fails `param_ty == arg_ty`,
/// so the call resolves to `NoMatch` → poison literal.
#[test]
fn generic_id_call_resolves_and_evaluates() {
    let module = compile_source("fn id<T>(x: T) -> T { x } structure S { let v = id(5mm) }");

    let v_expr = cell_expr(&module, "v");
    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "id");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected UserFunctionCall for id(5mm), got {other:?}"),
    }

    // INV-2: eval is type-erased — value correct even before return-type
    // substitution. id(5mm) returns its argument verbatim.
    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let v_val = reify_expr::eval_expr(v_expr, &ctx);
    assert_eq!(
        v_val,
        reify_ir::Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        },
        "id(5mm) should evaluate to the 5mm length scalar, got {v_val:?}"
    );
}
