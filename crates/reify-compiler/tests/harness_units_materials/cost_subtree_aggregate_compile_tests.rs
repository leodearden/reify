//! Compiler typing / diagnostic tests for the `cost(collection)` subtree-cost
//! aggregate builtin (task 5015, M-WHOLE γ).
//!
//! Observable signals:
//!   (a) `cost(self.descendants)` compiles with zero Error diagnostics and
//!       the result cell types to `Type::Scalar { dimension: MONEY }` (the
//!       `Money` alias), via a `CompiledExprKind::FunctionCall` node named
//!       `"cost"` — mirroring the landed `filter(list, Trait)` compile
//!       intercept (task 3991).
//!   (b) A user-defined `fn cost(...)` wins over the builtin: the call
//!       dispatches as a `CompiledExprKind::UserFunctionCall` (intercept
//!       suppressed), mirroring the `generate` non-shadowing guard
//!       (`generate_combinator_tests.rs::user_defined_generate_is_not_shadowed_by_builtin`).

use reify_core::{DimensionVector, Severity, Type};
use reify_ir::CompiledExprKind;
use reify_test_support::{compile_source, compile_source_with_stdlib};

// ─── (a) cost(self.descendants) compiles clean + types Scalar<MONEY> ───

/// `cost(self.descendants)` over a structure with a `Costed`-conforming sub
/// compiles with zero Error diagnostics, and the `total` value cell's
/// `default_expr` is a `CompiledExprKind::FunctionCall` named `"cost"` typed
/// `Type::Scalar { dimension: DimensionVector::MONEY }` — so the `: Money`
/// annotation on the let binding type-checks.
///
/// RED today: `cost` is unhandled by the compiler — it falls through to
/// generic overload resolution, which finds no candidate and emits an
/// unresolved-function poison literal + Error diagnostic.
#[test]
fn cost_over_descendants_compiles_clean_and_types_money_function_call() {
    let source = r#"
        structure def CapScrew : Costed {
            param supplier          : String = "McMaster-Carr"
            param part_number       : String = "91251A190"
            param unit_cost         : Money  = 0.12USD
            param lead_time         : Time   = 24h
            param quantity_produced : Real   = 24.0
        }
        structure Rig {
            sub bolts = CapScrew()
            let total : Money = cost(self.descendants)
        }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // (a) Zero Error diagnostics — no "no matching overload for cost" /
    // unresolved-function poison.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for cost(self.descendants), got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) The `total` cell types to Scalar<MONEY> (== the `Money` alias).
    let rig_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Rig")
        .expect("Rig template");

    let total_cell = rig_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "total")
        .expect("total value cell");

    let default_expr = total_cell
        .default_expr
        .as_ref()
        .expect("total has default_expr");

    assert_eq!(
        default_expr.result_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "cost(self.descendants) should type to Scalar<MONEY>, got {:?}",
        default_expr.result_type
    );

    // (c) The compiled node is a FunctionCall named "cost" (the compile
    // intercept), not a fallthrough UserFunctionCall/poison literal.
    match &default_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(
                function.name, "cost",
                "expected FunctionCall named 'cost', got {:?}",
                function.name
            );
            assert_eq!(
                args.len(),
                1,
                "expected exactly 1 arg (the compiled self.descendants), got {}",
                args.len()
            );
        }
        other => panic!(
            "expected CompiledExprKind::FunctionCall for cost(...), got: {:?}",
            other
        ),
    }
}

// ─── (b) user-defined `fn cost` wins over the builtin ───

/// A user-defined `fn cost(x: Real) -> Real` must win over the builtin
/// intercept: the call `cost(1.0)` dispatches as a
/// `CompiledExprKind::UserFunctionCall` (intercept suppressed), mirroring the
/// `!functions.iter().any(|f| f.name == "cost")` guard used by the landed
/// `filter`/`generate` intercepts.
///
/// RED today: `cost` is not intercepted at all, so this call already
/// resolves generically — but once step-2 adds the builtin intercept without
/// the user-override guard, this test would catch the regression (the
/// builtin would incorrectly shadow the user's `fn cost`).
#[test]
fn cost_user_defined_fn_suppresses_builtin_intercept() {
    let source = r#"
        fn cost(x: Real) -> Real {
            x
        }
        structure Rig {
            let total = cost(1.0)
        }
    "#;

    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for user-defined cost(...), got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let rig_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Rig")
        .expect("Rig template");

    let total_cell = rig_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "total")
        .expect("total value cell");

    let default_expr = total_cell
        .default_expr
        .as_ref()
        .expect("total has default_expr");

    match &default_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "cost",
                "expected UserFunctionCall named 'cost', got {:?}",
                function_name
            );
        }
        other => panic!(
            "expected UserFunctionCall (user fn should suppress the cost(...) \
             builtin intercept), got: {:?}",
            other
        ),
    }
}
