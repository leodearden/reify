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

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
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

// ── step-7: call-site return-type substitution (B1 / B2 / B5) ─────────────────

/// B1: `id<T>(x: T) -> T` called as `id(5mm)` must SUBSTITUTE the return type —
/// `result_type == Scalar<LENGTH>`, not the raw `TypeParam("T")`. Zero Error
/// diagnostics.
///
/// RED until step-8: the Resolved arm does `result_type = return_type.clone()`
/// verbatim, so `result_type` is `TypeParam("T")` (unsubstituted).
#[test]
fn generic_id_call_substitutes_return_type() {
    let module = compile_source("fn id<T>(x: T) -> T { x } structure S { let v = id(5mm) }");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for id(5mm), got: {errors:?}"
    );

    let v_expr = cell_expr(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::length(),
        "id(5mm) result_type should be substituted to Scalar<LENGTH>, got {:?}",
        v_expr.result_type
    );
}

/// B2: `single<T>(x: T) -> List<T>` called as `single(5mm)` must substitute the
/// inner type-arg: `result_type == List<Scalar<LENGTH>>`. Eval (INV-2, type
/// erased) yields `List([5mm scalar])` — exercises inner-arg substitution at
/// eval (PRD §ref :1335).
///
/// RED until step-8: `result_type` is the raw `List<TypeParam("T")>`.
#[test]
fn generic_single_call_substitutes_to_list_and_evals() {
    let module =
        compile_source("fn single<T>(x: T) -> List<T> { [x] } structure S { let v = single(5mm) }");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for single(5mm), got: {errors:?}"
    );

    let v_expr = cell_expr(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::List(Box::new(Type::length())),
        "single(5mm) result_type should be List<Scalar<LENGTH>>, got {:?}",
        v_expr.result_type
    );

    // INV-2: eval is type-erased — value correct regardless of substitution.
    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let v_val = reify_expr::eval_expr(v_expr, &ctx);
    assert_eq!(
        v_val,
        reify_ir::Value::List(vec![reify_ir::Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        }]),
        "single(5mm) should evaluate to List([5mm length scalar]), got {v_val:?}"
    );
}

/// B5: `constant_field<D, C>(value: C) -> Field<D, C>` called as
/// `constant_field(42.5)` binds only `C` (→ Real); `D` stays unbound. The
/// result type retains the NESTED unbound `TypeParam("D")` inside `Field<…>`
/// and this is TOLERATED — NO Error diagnostic (it is pinned later by an
/// enclosing call, PRD §8 / D3-decision).
///
/// RED until step-8: `result_type` is the raw `Field<TypeParam(D), TypeParam(C)>`
/// (C not yet substituted to Real).
#[test]
fn generic_constant_field_call_substitutes_codomain_tolerates_unbound_domain() {
    let module = compile_source(
        "fn constant_field<D, C>(value: C) -> Field<D, C> { value } \
         structure S { let v = constant_field(42.5) }",
    );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "B5: constant_field(42.5) must check clean (nested unbound D is tolerated), got: {errors:?}"
    );

    let v_expr = cell_expr(&module, "v");
    assert_eq!(
        v_expr.result_type,
        Type::Field {
            domain: Box::new(Type::TypeParam("D".to_string())),
            codomain: Box::new(Type::Real),
        },
        "constant_field(42.5) result_type should be Field<TypeParam(D), Real> (C bound, D unbound), got {:?}",
        v_expr.result_type
    );
}

// ── step-9: type-argument conflict diagnostic (B4) ───────────────────────────

/// B4: `pair<T>(a: T, b: T) -> T` called as `pair(1, 1.5)` binds `T:Int` from
/// the first arg then sees `T:Real` from the second — a type-argument conflict.
/// The call site must emit `DiagnosticCode::FnTypeArgConflict` referencing the
/// conflicting param `'T'`.
///
/// (`1` lexes as Int, `1.5` as Real — a guaranteed Int/Real mismatch pair.)
///
/// RED until step-10: step-8 ignores the `unify` Err, so no FnTypeArgConflict
/// is emitted.
#[test]
fn generic_pair_call_conflicting_args_emits_conflict_diagnostic() {
    let module =
        compile_source("fn pair<T>(a: T, b: T) -> T { a } structure S { let v = pair(1, 1.5) }");

    let conflict_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnTypeArgConflict));
    assert!(
        conflict_diag.is_some(),
        "expected a FnTypeArgConflict diagnostic for pair(1, 1.5); got diagnostics: {:?}",
        module.diagnostics
    );
    let diag = conflict_diag.unwrap();
    assert!(
        diag.message.contains("'T'"),
        "FnTypeArgConflict message should reference the conflicting param \"'T'\" (quoted), got: {:?}",
        diag.message
    );
}

// ── step-11: unresolved type-argument diagnostic + B5 regression guard ───────

/// `make<T>() -> T` called as `make()` has zero params, so nothing pins `T`:
/// after substitution the result type is a BARE top-level `TypeParam("T")` —
/// a wholly-undetermined type. The call site must emit
/// `DiagnosticCode::FnTypeArgUnresolved`.
///
/// RED until step-12: step-10 does not yet emit FnTypeArgUnresolved for a
/// bare-unbound result type.
#[test]
fn generic_make_call_bare_unbound_emits_unresolved_diagnostic() {
    let module = compile_source("fn make<T>() -> T { 0 } structure S { let v = make() }");

    let unresolved_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnTypeArgUnresolved));
    assert!(
        unresolved_diag.is_some(),
        "expected a FnTypeArgUnresolved diagnostic for make() (result type is a bare \
         unbound TypeParam); got diagnostics: {:?}",
        module.diagnostics
    );
}

/// B5 regression guard: a NESTED unbound type-param (e.g. `Field<TypeParam(D),
/// Real>` from `constant_field(42.5)`) is TOLERATED and must NOT emit
/// `FnTypeArgUnresolved` — only a BARE top-level `Type::TypeParam` triggers it.
///
/// Must stay green after step-12 (the bare-only rule must not over-trigger).
#[test]
fn generic_constant_field_nested_unbound_does_not_emit_unresolved() {
    let module = compile_source(
        "fn constant_field<D, C>(value: C) -> Field<D, C> { value } \
         structure S { let v = constant_field(42.5) }",
    );

    let unresolved_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnTypeArgUnresolved));
    assert!(
        unresolved_diag.is_none(),
        "B5: constant_field(42.5) must NOT emit FnTypeArgUnresolved — a nested unbound \
         param Field<TypeParam(D), Real> is tolerated; got: {:?}",
        unresolved_diag
    );
}
