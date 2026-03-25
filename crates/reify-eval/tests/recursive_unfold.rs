//! Recursive sub-component unfolding tests (Task 205).
//!
//! Tests for eager structural unfolding of recursive structures in the evaluator.
//! Recursive subs (template.is_recursive && sub.guard_expr.is_some()) are unfolded
//! depth-first until the guard evaluates to false or the depth limit is reached.

use reify_eval::Engine;
use reify_test_support::builders::{binop, gt, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::*;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a simple recursive structure S with:
///   param n: Int = `n_default`
///   sub child = S(n: n-1) where n > 0
///   is_recursive = true
fn build_recursive_s(n_default: i64) -> reify_compiler::TopologyTemplate {
    // guard: n > 0  (references S.n)
    let guard = gt(
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(0)),
    );
    // arg: n = n - 1  (references S.n)
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(n_default), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build()
}

/// Create a single-template module and run eval on it.
fn eval_single_template(template: reify_compiler::TopologyTemplate) -> reify_eval::EvalResult {
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module)
}

// ─── step-1: depth=0 (n=0), guard false, no children created ─────────────────

/// With n=0, the guard `n > 0` evaluates to false at the top level.
/// No child instances should be created: S.child.* should NOT exist.
#[test]
fn unfold_recursive_depth_zero_no_children() {
    let template = build_recursive_s(0);
    let result = eval_single_template(template);

    // Top-level n should be 0
    let s_n = ValueCellId::new("S", "n");
    assert_eq!(result.values.get(&s_n), Some(&Value::Int(0)));

    // S.child.n must NOT exist — guard is false (0 > 0 = false)
    let child_n = ValueCellId::new("S.child", "n");
    assert!(
        !result.values.contains(&child_n),
        "S.child.n should not exist when guard is false (n=0), but got {:?}",
        result.values.get(&child_n)
    );
}
