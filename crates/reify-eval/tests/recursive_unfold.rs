//! Recursive sub-component unfolding tests (Task 205).
//!
//! Tests for eager structural unfolding of recursive structures in the evaluator.
//! Recursive subs (template.is_recursive && sub.guard_expr.is_some()) are unfolded
//! depth-first until the guard evaluates to false or the depth limit is reached.

use reify_eval::Engine;
use reify_test_support::builders::{binop, conditional_expr, gt, literal, value_ref_typed};
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
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
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

// ─── step-3: depth=1 (n=1), one child created, no grandchild ─────────────────

/// With n=1, the guard `n > 0` is true at depth 0 → creates S.child (n=0).
/// At S.child level, guard `n > 0` evaluates to 0 > 0 = false → S.child.child should NOT exist.
#[test]
fn unfold_recursive_depth_one_creates_child() {
    let template = build_recursive_s(1);
    let result = eval_single_template(template);

    // Top-level n should be 1
    let s_n = ValueCellId::new("S", "n");
    assert_eq!(result.values.get(&s_n), Some(&Value::Int(1)));

    // S.child.n should be 0 (1 - 1)
    let child_n = ValueCellId::new("S.child", "n");
    assert_eq!(
        result.values.get(&child_n),
        Some(&Value::Int(0)),
        "S.child.n should be 0 (= 1 - 1)"
    );

    // S.child.child.n must NOT exist — guard is false at child level (n=0)
    let grandchild_n = ValueCellId::new("S.child.child", "n");
    assert!(
        !result.values.contains(&grandchild_n),
        "S.child.child.n should not exist when guard is false at depth 1, but got {:?}",
        result.values.get(&grandchild_n)
    );
}

// ─── step-5: depth=3 (n=3), three children, no 4th ──────────────────────────

/// With n=3, unfolds 3 levels deep: S.child.n=2, S.child.child.n=1, S.child.child.child.n=0.
/// S.child.child.child.child must NOT exist (guard false at depth 3).
#[test]
fn unfold_recursive_depth_three_creates_tree() {
    let template = build_recursive_s(3);
    let result = eval_single_template(template);

    // Verify chain: n decrements by 1 at each level
    let cases = [
        ("S", 3i64),
        ("S.child", 2),
        ("S.child.child", 1),
        ("S.child.child.child", 0),
    ];
    for (entity, expected_n) in &cases {
        let id = ValueCellId::new(*entity, "n");
        assert_eq!(
            result.values.get(&id),
            Some(&Value::Int(*expected_n)),
            "{}.n should be {}",
            entity,
            expected_n
        );
    }

    // The 4th child must NOT exist
    let too_deep = ValueCellId::new("S.child.child.child.child", "n");
    assert!(
        !result.values.contains(&too_deep),
        "S.child.child.child.child.n should not exist (guard false at n=0)"
    );
}

// ─── step-19: boolean guard controls recursion ───────────────────────────────

/// S with param active: Bool = true.
/// sub child = S(active: !active) where active.
/// After eval(): S.child should exist with active=false, S.child.child should NOT exist.
#[test]
fn unfold_recursive_bool_guard() {
    use reify_test_support::builders::not;

    // guard: active (boolean reference)
    let guard = value_ref_typed("S", "active", Type::Bool);
    // arg: active = !active
    let negated = not(value_ref_typed("S", "active", Type::Bool));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("active".to_string(), negated)], guard)
        .build();

    let result = eval_single_template(template);

    // S.active = true
    let s_active = ValueCellId::new("S", "active");
    assert_eq!(result.values.get(&s_active), Some(&Value::Bool(true)));

    // S.child should exist with active=false
    let child_active = ValueCellId::new("S.child", "active");
    assert_eq!(
        result.values.get(&child_active),
        Some(&Value::Bool(false)),
        "S.child.active should be false (= !true)"
    );

    // S.child.child must NOT exist — guard is false (active=false)
    let grandchild_active = ValueCellId::new("S.child.child", "active");
    assert!(
        !result.values.contains(&grandchild_active),
        "S.child.child.active should not exist (guard false when active=false)"
    );
}

// ─── step-17: multiple params propagated through unfolding ───────────────────

/// S with param n: Int = 2 and param width: Real = 10.0.
/// sub child = S(n: n-1, width: width * 0.5) where n > 0.
/// After eval(): S.child.width = 5.0, S.child.child.width = 2.5.
#[test]
fn unfold_recursive_multiple_params() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let width_half = binop(
        BinOp::Mul,
        value_ref_typed("S", "width", Type::Real),
        literal(Value::Real(0.5)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .param(
            "S",
            "width",
            Type::Real,
            Some(CompiledExpr::literal(Value::Real(10.0), Type::Real)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "child",
            "S",
            vec![
                ("n".to_string(), n_minus_1),
                ("width".to_string(), width_half),
            ],
            guard,
        )
        .build();

    let result = eval_single_template(template);

    // S.width = 10.0
    let s_width = ValueCellId::new("S", "width");
    assert_eq!(result.values.get(&s_width), Some(&Value::Real(10.0)));

    // S.child.width = 5.0
    let child_width = ValueCellId::new("S.child", "width");
    assert_eq!(
        result.values.get(&child_width),
        Some(&Value::Real(5.0)),
        "S.child.width should be 5.0 (= 10.0 * 0.5)"
    );

    // S.child.child.width = 2.5
    let grandchild_width = ValueCellId::new("S.child.child", "width");
    assert_eq!(
        result.values.get(&grandchild_width),
        Some(&Value::Real(2.5)),
        "S.child.child.width should be 2.5 (= 5.0 * 0.5)"
    );
}

// ─── step-15: non-recursive sub elaboration unchanged ────────────────────────

/// Template A (non-recursive) with sub b = B().
/// Template B (non-recursive) with param x: Int = 5.
/// After eval(): A.b.x should be 5. Verifies existing non-recursive path is not broken.
#[test]
fn unfold_recursive_non_recursive_sub_unchanged() {
    let b_template = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "x",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .build();

    let a_template = TopologyTemplateBuilder::new("A")
        // is_recursive defaults to false
        .sub_component("b", "B", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a_template)
        .template(b_template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    let ab_x = ValueCellId::new("A.b", "x");
    assert_eq!(
        result.values.get(&ab_x),
        Some(&Value::Int(5)),
        "A.b.x should be 5 (non-recursive sub elaboration unchanged)"
    );
}

// ─── step-13: let bindings in unfolded child instances ───────────────────────

/// S with n=3, let doubled = n * 2.
/// After eval(): S.child.doubled = 4, S.child.child.doubled = 2, S.child.child.child.doubled = 0.
#[test]
fn unfold_recursive_with_let_bindings() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let doubled_expr = binop(
        BinOp::Mul,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(2)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .let_binding("S", "doubled", Type::Int, doubled_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    let cases = [
        ("S.child", 2i64),
        ("S.child.child", 1),
        ("S.child.child.child", 0),
    ];
    for (entity, expected_n) in &cases {
        let doubled_id = ValueCellId::new(*entity, "doubled");
        let expected_doubled = expected_n * 2;
        assert_eq!(
            result.values.get(&doubled_id),
            Some(&Value::Int(expected_doubled)),
            "{}.doubled should be {} (= {} * 2)",
            entity,
            expected_doubled,
            expected_n
        );
    }
}

// ─── step-11/23: leaves-first ordering (cross-level data dependency) ─────────

/// S(n=2) with `let total: Int = if n > 0 then n + S.child.total else n`.
///
/// This creates a genuine cross-level data dependency:
/// - S.child.child (n=0): total = 0 (else branch, base case)
/// - S.child (n=1): total = 1 + S.child.child.total = 1 + 0 = 1 (then branch)
///
/// The test MUST FAIL with a top-down (elaborate-then-recurse) implementation
/// because when S.child's let-bindings are evaluated, S.child.child hasn't been
/// created yet — so `S.child.total` resolves to Undef instead of 1.
///
/// With the correct leaves-first (recurse-then-elaborate) implementation,
/// S.child.child is elaborated first, then S.child can reference its value.
#[test]
fn unfold_recursive_leaves_first_order() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    // total = if n > 0 then n + S.child.total else n
    let total_expr = conditional_expr(
        gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0))),
        binop(
            BinOp::Add,
            value_ref_typed("S", "n", Type::Int),
            value_ref_typed("S.child", "total", Type::Int),
        ),
        value_ref_typed("S", "n", Type::Int),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "total", Type::Int, total_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    // S.child.child (n=0): else branch → total = 0
    let grandchild_total = ValueCellId::new("S.child.child", "total");
    assert_eq!(
        result.values.get(&grandchild_total),
        Some(&Value::Int(0)),
        "S.child.child.total should be 0 (n=0, else branch), got {:?}",
        result.values.get(&grandchild_total)
    );

    // S.child (n=1): then branch → total = 1 + S.child.child.total = 1 + 0 = 1
    let child_total = ValueCellId::new("S.child", "total");
    assert_eq!(
        result.values.get(&child_total),
        Some(&Value::Int(1)),
        "S.child.total should be 1 (= 1 + S.child.child.total = 1 + 0), got {:?}",
        result.values.get(&child_total)
    );
}

// ─── step-9: depth limit stops unfolding ─────────────────────────────────────

/// S(n=100) with default engine max_unfold_depth=5: only 5 levels of children are created.
/// S.child through S.child^5 should exist, S.child^6 should NOT.
#[test]
fn unfold_recursive_depth_limit_stops_unfolding() {
    let template = build_recursive_s(100);
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_depth(5);
    let result = engine.eval(&module);

    // Build the entity names for the chain
    let mut entity = "S".to_string();
    for level in 0..=5 {
        let id = ValueCellId::new(&entity, "n");
        let expected_n = 100i64 - level as i64;
        assert_eq!(
            result.values.get(&id),
            Some(&Value::Int(expected_n)),
            "level {} entity {} should have n={}",
            level,
            entity,
            expected_n
        );
        entity = format!("{}.child", entity);
    }

    // Level 6 (S.child^6) must NOT exist — depth limit hit
    let too_deep = ValueCellId::new(&entity, "n");
    assert!(
        !result.values.contains(&too_deep),
        "level 6 entity {} should not exist (depth limit 5 hit), but got {:?}",
        entity,
        result.values.get(&too_deep)
    );
}

// ─── step-7: Undef param skips unfolding ─────────────────────────────────────

/// S with param n: Int (no default, so Undef). Guard `n > 0` evaluates with Undef → not Bool(true).
/// S.child.* should not exist — sub remains placeholder.
#[test]
fn unfold_recursive_undef_param_no_unfold() {
    // Build S with no default value for n (Undef)
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let template = TopologyTemplateBuilder::new("S")
        .param("S", "n", Type::Int, None) // no default → Undef
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    // S.n has no default, so it may be absent or Undef — either way, not a positive integer.
    // The main assertion is that unfolding was skipped.

    // S.child.n must NOT exist — guard evaluates to Undef (not Bool(true)) when n has no value
    let child_n = ValueCellId::new("S.child", "n");
    assert!(
        !result.values.contains(&child_n),
        "S.child.n should not exist when guard evaluates to Undef, but got {:?}",
        result.values.get(&child_n)
    );
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

// ─── step-21: default depth limit of 64 ──────────────────────────────────────

/// S(n=200) with default Engine (max_unfold_depth=64).
/// Exactly 64 child levels should be created: S, S.child, ..., S.child^64.
/// The 65th level (S.child^65) must NOT exist — default depth limit hit.
#[test]
fn unfold_recursive_default_depth_limit_64() {
    let template = build_recursive_s(200);
    let module = reify_test_support::CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let checker = MockConstraintChecker::new();
    // Do NOT call set_max_unfold_depth — use the default (64)
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Build entity name chains and verify levels 0 through 64 all exist
    // Level 0 = "S" (root), Level k = "S" + ".child" * k
    let mut entity = "S".to_string();
    for level in 0..=64 {
        let id = ValueCellId::new(&entity, "n");
        let expected_n = 200i64 - level as i64;
        assert_eq!(
            result.values.get(&id),
            Some(&Value::Int(expected_n)),
            "level {} entity {} should have n={} (default depth limit 64 allows this level)",
            level,
            entity,
            expected_n
        );
        entity = format!("{}.child", entity);
    }

    // Level 65 (entity = "S.child" x 65) must NOT exist — depth limit reached
    let too_deep = ValueCellId::new(&entity, "n");
    assert!(
        !result.values.contains(&too_deep),
        "level 65 entity {} should not exist (default depth limit is 64), but got {:?}",
        entity,
        result.values.get(&too_deep)
    );
}

// ─── step-33: multiple recursive subs — cross-sub let reference ──────────────

/// Template S with param n: Int = 2, two recursive subs (left and right),
/// and let bindings:
///   let val: Int = n * 10
///   let sum: Int = S.left.val + S.right.val
///
/// With S(n=2): S.left and S.right each have n=1.
/// At S.left (n=1): S.left.left (n=0, val=0) and S.left.right (n=0, val=0).
/// So S.left.sum should be 0 + 0 = 0.
///
/// The current `elaborate_child_lets_only` with `recursive_sub_name: Some("left")`
/// only projects the "left" chain (S.left.left.val → S.left.val), NOT the "right"
/// chain (S.left.right.val → S.right.val). So S.left.sum resolves to Undef+0 = Undef.
/// After the fix, both chains are projected, so S.left.sum = 0 + 0 = 0 (Int).
#[test]
fn unfold_recursive_multiple_subs_cross_sub_let_reference() {
    // guard: n > 0
    let guard_left = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let guard_right = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    // args: n = n - 1
    let n_minus_1_left = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let n_minus_1_right = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    // let val: Int = n * 10
    let val_expr = binop(
        BinOp::Mul,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(10)),
    );

    // let sum: Int = S.left.val + S.right.val
    let sum_expr = binop(
        BinOp::Add,
        value_ref_typed("S.left", "val", Type::Int),
        value_ref_typed("S.right", "val", Type::Int),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "val", Type::Int, val_expr)
        .let_binding("S", "sum", Type::Int, sum_expr)
        .is_recursive(true)
        .sub_component_with_guard(
            "left",
            "S",
            vec![("n".to_string(), n_minus_1_left)],
            guard_left,
        )
        .sub_component_with_guard(
            "right",
            "S",
            vec![("n".to_string(), n_minus_1_right)],
            guard_right,
        )
        .build();

    let result = eval_single_template(template);

    // S.left.val = 1 * 10 = 10, S.right.val = 1 * 10 = 10
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left", "val")),
        Some(&Value::Int(10)),
        "S.left.val should be 10 (= 1 * 10)"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S.right", "val")),
        Some(&Value::Int(10)),
        "S.right.val should be 10 (= 1 * 10)"
    );

    // S.left.left.val = 0 * 10 = 0, S.left.right.val = 0 * 10 = 0
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left.left", "val")),
        Some(&Value::Int(0)),
        "S.left.left.val should be 0 (= 0 * 10)"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left.right", "val")),
        Some(&Value::Int(0)),
        "S.left.right.val should be 0 (= 0 * 10)"
    );

    // S.left.sum = S.left.left.val + S.left.right.val = 0 + 0 = 0
    // This requires BOTH "left" and "right" sub chains to be projected into child_values.
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left", "sum")),
        Some(&Value::Int(0)),
        "S.left.sum should be 0 (= S.left.left.val + S.left.right.val = 0 + 0), \
         failing means only one sub chain was projected into child_values"
    );

    // Similarly S.right.sum = 0
    assert_eq!(
        result.values.get(&ValueCellId::new("S.right", "sum")),
        Some(&Value::Int(0)),
        "S.right.sum should be 0 (= S.right.left.val + S.right.right.val = 0 + 0)"
    );
}

// ─── step-31: multiple recursive subs — all cross-sub children are created ────

/// Template S with TWO recursive subs (left and right), both with same guard/args.
/// With S(n=2), the full tree should be:
///   S.left (n=1), S.right (n=1)
///   S.left.left (n=0), S.left.right (n=0), S.right.left (n=0), S.right.right (n=0)
/// All leaves (n=0) stop unfolding (guard false).
///
/// The current implementation only recurses on the SAME sub chain, so S.left.right
/// and S.right.left are never created. This test verifies the fix.
#[test]
fn unfold_recursive_multiple_subs_all_children_created() {
    // guard: n > 0
    let guard_left = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let guard_right = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    // arg: n = n - 1
    let n_minus_1_left = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let n_minus_1_right = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "left",
            "S",
            vec![("n".to_string(), n_minus_1_left)],
            guard_left,
        )
        .sub_component_with_guard(
            "right",
            "S",
            vec![("n".to_string(), n_minus_1_right)],
            guard_right,
        )
        .build();

    let result = eval_single_template(template);

    // Level 1: both direct children should have n=1
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left", "n")),
        Some(&Value::Int(1)),
        "S.left.n should be 1"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S.right", "n")),
        Some(&Value::Int(1)),
        "S.right.n should be 1"
    );

    // Level 2: all 4 cross-sub children should have n=0
    for entity in &[
        "S.left.left",
        "S.left.right",
        "S.right.left",
        "S.right.right",
    ] {
        assert_eq!(
            result.values.get(&ValueCellId::new(*entity, "n")),
            Some(&Value::Int(0)),
            "{}.n should be 0 (cross-sub child must be created)",
            entity
        );
    }

    // Level 3: nothing should exist — guard is false at n=0
    assert!(
        !result
            .values
            .contains(&ValueCellId::new("S.left.left.left", "n")),
        "S.left.left.left.n should not exist (guard false at n=0)"
    );
    assert!(
        !result
            .values
            .contains(&ValueCellId::new("S.left.right.left", "n")),
        "S.left.right.left.n should not exist (guard false at n=0)"
    );
}

// ─── step-29: depth-limit truncation emits an Error-severity diagnostic ───────

/// When the depth limit truncates unfolding (guard is still true but depth >= max),
/// the evaluator must emit a Severity::Error diagnostic (not warning) so callers
/// know the result is potentially unsound — child references beyond the limit
/// resolve to Undef.
#[test]
fn unfold_recursive_depth_limit_emits_error_diagnostic() {
    let template = build_recursive_s(100);
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_depth(3);
    let result = engine.eval(&module);

    let has_error = result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains("truncated at depth limit"));
    assert!(
        has_error,
        "Expected an Error-severity diagnostic about depth truncation, got: {:?}",
        result.diagnostics
    );
}

// ─── step-27: depth=0 is rejected at the API boundary ────────────────────────

/// `set_max_unfold_depth(0)` must panic because depth=0 means the guard check
/// `depth >= max_depth` (0 >= 0) fires before any child entity is created,
/// silently leaving parent let-bindings that reference `child.*` as Undef.
/// Rejecting 0 at the API boundary prevents this silent data corruption.
#[test]
#[should_panic(expected = "max_unfold_depth must be >= 1")]
fn unfold_recursive_depth_limit_zero_rejected() {
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_depth(0); // must panic
}

// ─── step-25: cross-level dependency at depth 3 ───────────────────────────────

/// Regression test for leaves-first ordering at greater depth.
///
/// S(n=3) with `let total: Int = if n > 0 then n + S.child.total else n`.
/// Expected values (cascading cross-level dependency):
/// - S.child.child.child (n=0): total = 0 (else branch, base case)
/// - S.child.child (n=1): total = 1 + S.child.child.child.total = 1 + 0 = 1
/// - S.child (n=2): total = 2 + S.child.child.total = 2 + 1 = 3
///
/// All three assertions must produce Int values (not Undef), confirming the full
/// bottom-up evaluation chain works for two levels of cascading cross-level dependency.
#[test]
fn unfold_recursive_cross_level_three_deep() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    // total = if n > 0 then n + S.child.total else n
    let total_expr = conditional_expr(
        gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0))),
        binop(
            BinOp::Add,
            value_ref_typed("S", "n", Type::Int),
            value_ref_typed("S.child", "total", Type::Int),
        ),
        value_ref_typed("S", "n", Type::Int),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .let_binding("S", "total", Type::Int, total_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    // S.child.child.child (n=0): else branch → total = 0
    let ggchild_total = ValueCellId::new("S.child.child.child", "total");
    assert_eq!(
        result.values.get(&ggchild_total),
        Some(&Value::Int(0)),
        "S.child.child.child.total should be 0 (base case), got {:?}",
        result.values.get(&ggchild_total)
    );

    // S.child.child (n=1): then branch → total = 1 + 0 = 1
    let grandchild_total = ValueCellId::new("S.child.child", "total");
    assert_eq!(
        result.values.get(&grandchild_total),
        Some(&Value::Int(1)),
        "S.child.child.total should be 1 (= 1 + 0), got {:?}",
        result.values.get(&grandchild_total)
    );

    // S.child (n=2): then branch → total = 2 + 1 = 3
    let child_total = ValueCellId::new("S.child", "total");
    assert_eq!(
        result.values.get(&child_total),
        Some(&Value::Int(3)),
        "S.child.total should be 3 (= 2 + 1), got {:?}",
        result.values.get(&child_total)
    );
}

// ─── step-26: mutual recursion (two-node cycle A ↔ B) ──────────────────────

/// Two mutually-recursive templates:
///   A { param n: Int = 2; is_recursive = true; sub b = B(n: n-1) where n > 0 }
///   B { param n: Int = 0; is_recursive = true; sub a = A(n: n-1) where n > 0 }
///
/// Starting from entity A with n=2:
///   A(n=2) → A.b = B(n=1) → A.b.a = A(n=0) → guard false, stop.
///
/// Expected: A.b.n == 1, A.b.a.n == 0, A.b.a.b does NOT exist (guard stops at n=0).
///
/// This test FAILS against the current implementation because Phase 2 passes A's
/// all_recursive_subs (containing A's guard expression referencing A's ValueCellId keys)
/// into the recursive call for B. B's local_values is built from B's value_cells, so
/// the A-keyed guard refs are absent → Undef → silent return → B's entity is incomplete.
#[test]
fn unfold_mutual_recursion_two_node_cycle() {
    // Template A: param n: Int = 2, sub b = B(n: n-1) where n > 0
    let guard_a = gt(value_ref_typed("A", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1_a = binop(
        BinOp::Sub,
        value_ref_typed("A", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let template_a = TopologyTemplateBuilder::new("A")
        .param(
            "A",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard("b", "B", vec![("n".to_string(), n_minus_1_a)], guard_a)
        .build();

    // Template B: param n: Int = 0 (default irrelevant, overridden by arg), sub a = A(n: n-1) where n > 0
    let guard_b = gt(value_ref_typed("B", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1_b = binop(
        BinOp::Sub,
        value_ref_typed("B", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let template_b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard("a", "A", vec![("n".to_string(), n_minus_1_b)], guard_b)
        .build();

    // Build module with both templates
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // A.n should be 2 (top-level default)
    assert_eq!(
        result.values.get(&ValueCellId::new("A", "n")),
        Some(&Value::Int(2)),
        "A.n should be 2"
    );

    // A.b should be B with n=1 (= 2 - 1)
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b", "n")),
        Some(&Value::Int(1)),
        "A.b.n should be 1 (= A.n - 1 = 2 - 1)"
    );

    // A.b.a should be A with n=0 (= 1 - 1)
    // THIS IS THE KEY ASSERTION: with the bug, B's sub `a` is never unfolded because
    // the guard expression references A's ValueCellId keys but local_values has B's keys.
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b.a", "n")),
        Some(&Value::Int(0)),
        "A.b.a.n should be 0 (= A.b.n - 1 = 1 - 1). \
         If this fails, mutual recursion is broken: B's sub `a` was not unfolded \
         because Phase 2 used A's guard/sub declarations instead of B's."
    );

    // A.b.a.b should NOT exist (guard n > 0 is false at n=0)
    assert!(
        !result.values.contains(&ValueCellId::new("A.b.a.b", "n")),
        "A.b.a.b.n should not exist (guard false at n=0)"
    );
}

// ─── step-28a: mutual recursion three-node cycle A → B → C → A ──────────────

/// Three mutually-recursive templates:
///   A { param n: Int = 3; is_recursive = true; sub b = B(n: n-1) where n > 0 }
///   B { param n: Int = 0; is_recursive = true; sub c = C(n: n-1) where n > 0 }
///   C { param n: Int = 0; is_recursive = true; sub a = A(n: n-1) where n > 0 }
///
/// Starting from entity A with n=3:
///   A(n=3) → A.b = B(n=2) → A.b.c = C(n=1) → A.b.c.a = A(n=0) → guard false, stop.
///
/// This tests that template lookup chains correctly through a 3-node cycle,
/// with scope_template alternating A→B→C→A at each depth level.
#[test]
fn unfold_mutual_recursion_three_node_cycle() {
    // Template A: param n=3, sub b = B(n: n-1) where n > 0
    let template_a = TopologyTemplateBuilder::new("A")
        .param(
            "A",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "b",
            "B",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("A", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("A", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    // Template B: param n=0, sub c = C(n: n-1) where n > 0
    let template_b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "c",
            "C",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("B", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("B", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    // Template C: param n=0, sub a = A(n: n-1) where n > 0
    let template_c = TopologyTemplateBuilder::new("C")
        .param(
            "C",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "a",
            "A",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("C", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("C", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .template(template_c)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Verify chain: A(3) → A.b=B(2) → A.b.c=C(1) → A.b.c.a=A(0)
    assert_eq!(
        result.values.get(&ValueCellId::new("A", "n")),
        Some(&Value::Int(3)),
        "A.n should be 3"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b", "n")),
        Some(&Value::Int(2)),
        "A.b.n should be 2"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b.c", "n")),
        Some(&Value::Int(1)),
        "A.b.c.n should be 1"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b.c.a", "n")),
        Some(&Value::Int(0)),
        "A.b.c.a.n should be 0"
    );

    // A.b.c.a.b should NOT exist (guard false at n=0)
    assert!(
        !result.values.contains(&ValueCellId::new("A.b.c.a.b", "n")),
        "A.b.c.a.b.n should not exist (guard false at n=0)"
    );
}

// ─── step-28b: mutual recursion with let-bindings ────────────────────────────

/// Two mutually-recursive templates with let-bindings:
///   A { param n: Int = 2; let val: Int = n * 10; is_recursive = true; sub b = B(n: n-1) where n > 0 }
///   B { param n: Int = 0; let val: Int = n * 10; is_recursive = true; sub a = A(n: n-1) where n > 0 }
///
/// Starting from A(n=2):
///   A(n=2, val=20) → A.b = B(n=1, val=10) → A.b.a = A(n=0, val=0)
///
/// This verifies that Phase 3 (elaborate_child_lets_only) receives the correct
/// child-scoped recursive_sub_names for BFS traversal at each mutual recursion depth.
#[test]
fn unfold_mutual_recursion_with_let_bindings() {
    // Template A: param n=2, let val = n * 10, sub b = B(n: n-1) where n > 0
    let template_a = TopologyTemplateBuilder::new("A")
        .param(
            "A",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding(
            "A",
            "val",
            Type::Int,
            binop(
                BinOp::Mul,
                value_ref_typed("A", "n", Type::Int),
                literal(Value::Int(10)),
            ),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "b",
            "B",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("A", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("A", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    // Template B: param n=0, let val = n * 10, sub a = A(n: n-1) where n > 0
    let template_b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .let_binding(
            "B",
            "val",
            Type::Int,
            binop(
                BinOp::Mul,
                value_ref_typed("B", "n", Type::Int),
                literal(Value::Int(10)),
            ),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "a",
            "A",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("B", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("B", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // A(n=2): val = 2 * 10 = 20
    assert_eq!(
        result.values.get(&ValueCellId::new("A", "val")),
        Some(&Value::Int(20)),
        "A.val should be 20 (= 2 * 10)"
    );

    // A.b = B(n=1): val = 1 * 10 = 10
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b", "val")),
        Some(&Value::Int(10)),
        "A.b.val should be 10 (= 1 * 10)"
    );

    // A.b.a = A(n=0): val = 0 * 10 = 0
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b.a", "val")),
        Some(&Value::Int(0)),
        "A.b.a.val should be 0 (= 0 * 10)"
    );
}

// ─── step-37: mutual recursion with heterogeneous (non-overlapping) members ──

/// Two mutually-recursive templates with NON-OVERLAPPING member names beyond `n`:
///   A { param n: Int = 2; param width: Int = 5; let total: Int = width + A.b.height;
///       is_recursive = true; sub b = B(n: n-1) where n > 0 }
///   B { param n: Int = 0; param height: Int = 3;
///       is_recursive = true; sub a = A(n: n-1) where n > 0 }
///
/// Starting from A(n=2):
///   A(n=2, width=5) → A.b = B(n=1, height=3) → A.b.a = A(n=0, width=5) → guard false.
///
/// Key assertion: A.total = width + A.b.height = 5 + 3 = 8
///
/// This tests that the BFS projection in `elaborate_child_lets_only` uses
/// per-entity template lookups — NOT just child_template.value_cells at all depths.
/// With heterogeneous members, B's `height` is absent from A's value_cells, so the
/// current BFS (iterating A.value_cells for entity A.b) never constructs
/// ValueCellId("A.b", "height"), causing A.total to evaluate to Undef.
#[test]
fn unfold_mutual_recursion_heterogeneous_members() {
    // The let expr for `total`: width + A.b.height
    // References A.width (same template) and A.b.height (cross-template via sub path).
    // In the compiled form, cross-entity references use (entity, member) — so "A.b.height"
    // becomes ValueCellId("A.b", "height"). The BFS must project B's `height` member
    // from the global values into child_values so the let-binding can resolve it.

    // Template A: param n=2, param width=5, let total = width + <A.b.height via child_values>,
    //             sub b = B(n: n-1) where n > 0
    let template_a = TopologyTemplateBuilder::new("A")
        .param(
            "A",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .param(
            "A",
            "width",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .let_binding(
            "A",
            "total",
            Type::Int,
            // total = width + A.b.height
            // Cross-entity ref: entity="A.b" (the sub), member="height" (B's param).
            // After BFS projection: child_values has ValueCellId("A.b", "height") from global values.
            binop(
                BinOp::Add,
                value_ref_typed("A", "width", Type::Int),
                value_ref_typed("A.b", "height", Type::Int),
            ),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "b",
            "B",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("A", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("A", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    // Template B: param n=0, param height=3, sub a = A(n: n-1) where n > 0
    let template_b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .param(
            "B",
            "height",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(3), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "a",
            "A",
            vec![(
                "n".to_string(),
                binop(
                    BinOp::Sub,
                    value_ref_typed("B", "n", Type::Int),
                    literal(Value::Int(1)),
                ),
            )],
            gt(value_ref_typed("B", "n", Type::Int), literal(Value::Int(0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Basic params
    assert_eq!(
        result.values.get(&ValueCellId::new("A", "width")),
        Some(&Value::Int(5)),
        "A.width should be 5"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b", "height")),
        Some(&Value::Int(3)),
        "A.b.height should be 3 (B's param at depth 1)"
    );

    // KEY: cross-template let-binding that depends on child entity's member
    // A.total = A.width + A.b.height = 5 + 3 = 8
    // This will FAIL if the BFS projection doesn't look up B's value_cells for entity A.b.
    assert_eq!(
        result.values.get(&ValueCellId::new("A", "total")),
        Some(&Value::Int(8)),
        "A.total should be 8 (= width 5 + A.b.height 3). \
         If Undef, BFS projection in elaborate_child_lets_only iterates A's value_cells \
         for entity A.b (a B instance), missing B-specific members like 'height'."
    );
}

// ─── step-39: cyclic let-binding dependency detection ─────────────────────────

/// Recursive template S with mutually-dependent let-bindings:
///   S { param n: Int = 2; let a: Int = b + 1; let b: Int = a + 1;
///       sub child = S(n: n-1) where n > 0; is_recursive = true }
///
/// The let-bindings `a` and `b` form a circular dependency: a depends on b,
/// b depends on a. `topological_sort` (Kahn's algorithm) silently drops nodes
/// in cycles — they never appear in the sorted output.
///
/// Currently `elaborate_child_lets_only` has no cycle detection, so a and b
/// remain `Value::Undef` with no diagnostic. This test asserts that:
/// 1. An error-level diagnostic is emitted containing 'circular' or 'cycle'
///    and naming both `a` and `b`.
/// 2. S.a and S.b are Value::Undef (they can't be evaluated).
#[test]
fn cyclic_let_bindings_emit_diagnostic() {
    // let a = b + 1 (depends on S.b)
    let a_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Int),
        literal(Value::Int(1)),
    );
    // let b = a + 1 (depends on S.a)
    let b_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "a", Type::Int),
        literal(Value::Int(1)),
    );

    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "a", Type::Int, a_expr)
        .let_binding("S", "b", Type::Int, b_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    // The cyclic let-bindings are silently dropped by topological_sort (Kahn's algorithm
    // omits nodes in cycles). They may be absent (None) or Undef depending on whether
    // the cycle detection writes Undef explicitly. Either is acceptable — the key is the
    // diagnostic.
    let a_val = result.values.get(&ValueCellId::new("S", "a"));
    assert!(
        a_val.is_none() || a_val == Some(&Value::Undef),
        "S.a should be absent or Undef (circular dependency), got {:?}",
        a_val,
    );
    let b_val = result.values.get(&ValueCellId::new("S", "b"));
    assert!(
        b_val.is_none() || b_val == Some(&Value::Undef),
        "S.b should be absent or Undef (circular dependency), got {:?}",
        b_val,
    );

    // An error diagnostic should be emitted about the circular dependency.
    let has_cycle_error = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && (d.message.contains("circular") || d.message.contains("cycle"))
            && d.message.contains("a")
            && d.message.contains("b")
    });
    assert!(
        has_cycle_error,
        "Expected an error diagnostic about circular let-binding dependency naming 'a' and 'b', \
         got: {:?}",
        result.diagnostics
    );
}

// ─── BFS traversal gate tests ───────────────────────────────────────────────

/// Regression test: BFS in `elaborate_child_lets_only` should traverse through
/// structural intermediary templates that have zero value_cells.
///
/// Setup:
///   Template S: param n: Int = 2, let inner_n = S.w.back.n,
///               sub child = S(n: n-1) where n > 0,
///               sub w = W() where n > 0
///   Template W (wrapper): zero value_cells,
///               sub back = S(n: 0) where true
///
/// Entity tree from S(n=2):
///   S (n=2)
///   ├── S.child = S(n=1)
///   │   ├── S.child.child = S(n=0)  [guard false, leaf]
///   │   └── S.child.w = W()
///   │       └── S.child.w.back = S(n=0)  [leaf]
///   └── S.w = W()
///       └── S.w.back = S(n=0)  [leaf]
///
/// BUG: When elaborate_child_lets_only runs BFS for entity "S.child", the BFS
/// seeds include "S.child.w" → W. But W has zero value_cells, so found_any stays
/// false and "S.child.w.back" (an S with n=0) is never enqueued. The let-binding
/// `inner_n = S.w.back.n` at "S.child" cannot see the projected value through W.
#[test]
fn bfs_traverses_through_wrapper_with_zero_value_cells() {
    // Template S: param n, let inner_n = S.w.back.n, sub child=S(n-1), sub w=W()
    let guard_child = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let guard_w = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    let inner_n_expr = value_ref_typed("S.w.back", "n", Type::Int);

    let template_s = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "inner_n", Type::Int, inner_n_expr)
        .is_recursive(true)
        .sub_component_with_guard(
            "child",
            "S",
            vec![("n".to_string(), n_minus_1)],
            guard_child,
        )
        .sub_component_with_guard("w", "W", vec![], guard_w)
        .build();

    // Template W (wrapper): zero value_cells, sub back = S(n: 0) where true
    let template_w = TopologyTemplateBuilder::new("W")
        .is_recursive(true)
        .sub_component_with_guard(
            "back",
            "S",
            vec![("n".to_string(), literal(Value::Int(0)))],
            literal(Value::Bool(true)),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_s)
        .template(template_w)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Sanity: child entity exists with n=1
    assert_eq!(
        result.values.get(&ValueCellId::new("S.child", "n")),
        Some(&Value::Int(1)),
        "S.child.n should be 1"
    );

    // Sanity: deeper entity through wrapper exists
    assert_eq!(
        result.values.get(&ValueCellId::new("S.child.w.back", "n")),
        Some(&Value::Int(0)),
        "S.child.w.back.n should be 0 (leaf S through wrapper)"
    );

    // KEY ASSERTION: S.child's let inner_n should see S.child.w.back.n projected
    // through the wrapper W. Without the BFS fix, W has zero value_cells →
    // found_any=false → BFS stops at W → inner_n evaluates to Undef.
    assert_eq!(
        result.values.get(&ValueCellId::new("S.child", "inner_n")),
        Some(&Value::Int(0)),
        "S.child.inner_n should be 0 (projected through wrapper W with zero value_cells). \
         If Undef, the BFS gate on found_any prevented traversal through the wrapper."
    );
}

// ─── Missing template reference diagnostic tests ────────────────────────────

/// A recursive sub referencing a non-existent template should produce an
/// Error-severity diagnostic, not just a warning. This indicates a
/// post-compilation inconsistency (compiler should have validated template refs).
///
/// Setup:
///   Template S: param n: Int = 1,
///               sub child = "Nonexistent"(n: n-1) where n > 0
///               is_recursive = true
///
/// "Nonexistent" does not exist in the module → the unfold path should emit
/// Diagnostic::error mentioning "unknown structure".
#[test]
fn missing_template_ref_emits_error_diagnostic() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "child",
            "Nonexistent",
            vec![("n".to_string(), n_minus_1)],
            guard,
        )
        .build();

    let result = eval_single_template(template);

    // Should have an Error-severity diagnostic about the unknown structure.
    let has_error = result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains("unknown structure"));
    assert!(
        has_error,
        "Expected Error-severity diagnostic about unknown structure 'Nonexistent', \
         got: {:?}",
        result.diagnostics
    );

    // Should NOT have only warnings about it — the severity must be Error.
    let has_only_warning = result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Warning && d.message.contains("unknown structure"));
    assert!(
        !has_only_warning,
        "Unknown structure reference should be Error, not Warning: {:?}",
        result.diagnostics
    );
}

// ─── Cycle diagnostic UX: template name in message ──────────────────────────

/// The cyclic let-binding diagnostic should include the template name, not just
/// the entity path. Format: 'in template S (entity S.child): [a, b]' to match
/// the termination-check diagnostic style.
///
/// This is a separate test from `cyclic_let_bindings_emit_diagnostic` which only
/// checks for 'circular', 'a', and 'b'. This test specifically verifies the
/// template name appears in the message.
#[test]
fn cyclic_let_binding_diagnostic_includes_template_name() {
    // let a = b + 1 (depends on S.b)
    let a_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Int),
        literal(Value::Int(1)),
    );
    // let b = a + 1 (depends on S.a)
    let b_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "a", Type::Int),
        literal(Value::Int(1)),
    );

    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "a", Type::Int, a_expr)
        .let_binding("S", "b", Type::Int, b_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let result = eval_single_template(template);

    // The cycle diagnostic should include the template name "S" in the format
    // 'in template S (entity ...)' — not just the entity path.
    let has_template_in_diagnostic = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && d.message.contains("circular")
            && d.message.contains("template S")
    });
    assert!(
        has_template_in_diagnostic,
        "Expected cycle diagnostic to include 'template S' in message, got: {:?}",
        result.diagnostics
    );
}

// ─── BFS termination: cyclic structural intermediaries ─────────────────────

/// Two structural intermediaries (zero value_cells) forming a cycle: W1→W2→W1.
/// Without a termination check for structural intermediaries in the BFS at
/// `elaborate_child_lets_only`, the BFS generates ever-longer entity paths
/// (S.w1.next.next.next...) without bound, hanging the engine.
///
/// Setup:
///   Template S: param n: Int = 1,
///               sub w1 = W1() where n > 0
///   Template W1: zero value_cells,
///               sub next = W2() where true
///   Template W2: zero value_cells,
///               sub next = W1() where true
///
/// Entity tree from S(n=1):
///   S (n=1)
///   └── S.w1 = W1()
///       └── S.w1.next = W2()
///           └── S.w1.next.next = W1()
///               └── ... (infinite cycle)
///
/// The BFS in elaborate_child_lets_only descends unconditionally through
/// structural intermediaries (value_cells.is_empty() ⇒ found_any irrelevant).
/// With W1↔W2 cycling, the queue grows without bound.
///
/// Detection: spawn eval in a thread with a 5-second timeout. If eval hangs,
/// recv_timeout returns Err and the test fails.
#[test]
fn bfs_terminates_for_cyclic_structural_intermediaries() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        // Template S: param n: Int = 1, sub w1 = W1() where n > 0
        let guard_w1 = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));

        let template_s = TopologyTemplateBuilder::new("S")
            .param(
                "S",
                "n",
                Type::Int,
                Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
            )
            .is_recursive(true)
            .sub_component_with_guard("w1", "W1", vec![], guard_w1)
            .build();

        // Template W1: zero value_cells, sub next = W2() where true
        let template_w1 = TopologyTemplateBuilder::new("W1")
            .is_recursive(true)
            .sub_component_with_guard("next", "W2", vec![], literal(Value::Bool(true)))
            .build();

        // Template W2: zero value_cells, sub next = W1() where true
        let template_w2 = TopologyTemplateBuilder::new("W2")
            .is_recursive(true)
            .sub_component_with_guard("next", "W1", vec![], literal(Value::Bool(true)))
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template_s)
            .template(template_w1)
            .template(template_w2)
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = Engine::new(Box::new(checker), None);
        let result = engine.eval(&module);
        let _ = tx.send(result);
    });

    let result = rx.recv_timeout(Duration::from_secs(5)).expect(
        "BFS hung: elaborate_child_lets_only did not terminate within 5 seconds \
                 for cyclic structural intermediaries W1↔W2. The BFS gate \
                 `found_any || value_cells.is_empty()` unconditionally descends through \
                 structural intermediaries without checking if the entity was actually unfolded.",
    );

    // S.n=1 should exist
    assert_eq!(
        result.values.get(&ValueCellId::new("S", "n")),
        Some(&Value::Int(1)),
        "S.n should be 1"
    );
}

// ─── Phase 2 is_recursive guard ────────────────────────────────────────────

/// Phase 2 should NOT recursively unfold guarded subs of a non-recursive child template.
///
/// Setup:
///   Template A: is_recursive=true, param n: Int = 1, sub b = B(x: n) where n > 0
///   Template B: is_recursive=false, param x: Int = 0, sub c = C() where literal(true)
///   Template C: param y: Int = 99
///
/// Evaluate A(n=1):
///   Phase 1: A.b is created (B with x=1)
///   Phase 2: B is NOT recursive, so B's guarded sub `c` should NOT be recursively unfolded.
///
/// Assert: A.b.x == 1 (Phase 1 elaboration works) AND A.b.c.y does NOT exist.
///
/// BUG: Phase 2 filter only checks `guard_expr.is_some()`, not `child_template.is_recursive`.
/// So B's sub `c` gets recursively unfolded producing A.b.c.y=99 even though B is not recursive.
#[test]
fn non_recursive_child_guarded_sub_not_unfolded() {
    // Template A: is_recursive=true, param n: Int = 1, sub b = B(x: n) where n > 0
    let guard_a = gt(value_ref_typed("A", "n", Type::Int), literal(Value::Int(0)));
    let template_a = TopologyTemplateBuilder::new("A")
        .param(
            "A",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(1), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "b",
            "B",
            vec![("x".to_string(), value_ref_typed("A", "n", Type::Int))],
            guard_a,
        )
        .build();

    // Template B: is_recursive=false, param x: Int = 0, sub c = C() where literal(true)
    let template_b = TopologyTemplateBuilder::new("B")
        .param(
            "B",
            "x",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
        )
        .is_recursive(false)
        .sub_component_with_guard("c", "C", vec![], literal(Value::Bool(true)))
        .build();

    // Template C: param y: Int = 99
    let template_c = TopologyTemplateBuilder::new("C")
        .param(
            "C",
            "y",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(99), Type::Int)),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .template(template_c)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Phase 1: A.b.x should be 1 (arg x = A.n = 1)
    assert_eq!(
        result.values.get(&ValueCellId::new("A.b", "x")),
        Some(&Value::Int(1)),
        "A.b.x should be 1 (Phase 1 elaboration of B's params)"
    );

    // KEY ASSERTION: A.b.c.y should NOT exist because B is not recursive,
    // so Phase 2 should not recursively unfold B's guarded sub c.
    assert!(
        !result.values.contains(&ValueCellId::new("A.b.c", "y")),
        "A.b.c.y should NOT exist: B is not recursive, so Phase 2 should not \
         recursively unfold B's guarded sub 'c'. Got {:?}",
        result.values.get(&ValueCellId::new("A.b.c", "y"))
    );
}

// ─── Budget accounting: guard-false should not consume budget ──────────────

/// Budget should NOT be decremented when a guard evaluates to false.
///
/// Setup:
///   Template S: is_recursive=true, param n: Int = 2
///     sub left  = S(n: n-1) where n > 0
///     sub right = S(n: n-1) where n > 0
///   max_unfold_nodes = 3
///
/// Trace with FIXED budget (decrement after guard+depth):
///   unfold(S, depth=0, budget=3):
///     guard true, budget 3→2, Phase 1: S.left(n=1)
///     Phase 2 left:  unfold(S.left, depth=1, budget=2):
///       guard true, budget 2→1, Phase 1: S.left.left(n=0)
///       Phase 2 left:  unfold(S.left.left, depth=2, budget=1): guard false → return (no decrement)
///       Phase 2 right: unfold(S.left.left, depth=2, budget=1): guard false → return (no decrement)
///     Phase 2 right: unfold(S.left, depth=1, budget=1):
///       guard true, budget 1→0, Phase 1: S.left.right(n=0) ← CREATED
///
/// With BUGGY budget (decrement before guard):
///   S.left.left.left consumes budget (3→2→1→0 on guard-false), S.left.right never created.
///
/// Assert: S.left.right.n == 0 (exists).
#[test]
fn budget_not_consumed_when_guard_false() {
    // guard: n > 0
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    // arg: n = n - 1
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template_s = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "left",
            "S",
            vec![("n".to_string(), n_minus_1.clone())],
            guard.clone(),
        )
        .sub_component_with_guard("right", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_s)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_nodes(3);
    let result = engine.eval(&module);

    // S.left.right should exist with n=0 because guard-false calls at depth 2
    // should NOT consume budget, leaving budget=1 for S.left.right.
    assert_eq!(
        result.values.get(&ValueCellId::new("S.left.right", "n")),
        Some(&Value::Int(0)),
        "S.left.right.n should be 0: guard-false calls at leaf level should not \
         consume budget, leaving budget for sibling branches. \
         If missing, budget was wasted on guard-false calls."
    );
}

// ─── Budget accounting: depth-limit should not consume budget ──────────────

/// Budget should NOT be decremented when a depth-limit return prevents node creation.
///
/// Setup:
///   Template S: is_recursive=true, param n: Int = 999
///     sub left  = S(n: n-1) where n > -999  (guard always true for test range)
///     sub right = S(n: n-1) where n > -999
///   max_unfold_depth = 1, max_unfold_nodes = 2
///
/// Trace with FIXED budget (decrement after guard+depth):
///   unfold(S, depth=0, budget=2):
///     guard true, depth 0 < 1 ok, budget 2→1, Phase 1: S.left(n=998)
///     Phase 2 left:  unfold(S.left, depth=1, budget=1): guard true, depth 1 >= 1 → return (no decrement)
///     Phase 2 right: unfold(S.left, depth=1, budget=1): guard true, depth 1 >= 1 → return (no decrement)
///   S.right:
///   unfold(S, depth=0, budget=2): (separate budget for top-level right)
///     guard true, depth 0 < 1 ok, budget 2→1, Phase 1: S.right(n=998)
///     Phase 2 left:  unfold(S.right, depth=1, budget=1): depth-limited → return
///     Phase 2 right: unfold(S.right, depth=1, budget=1): depth-limited → return
///
/// With BUGGY budget (decrement before guard):
///   unfold(S, depth=0, budget=2):
///     budget 2→1, guard true, depth ok, Phase 1: S.left(n=998)
///     Phase 2 left:  unfold(S.left, depth=1, budget=1): budget 1→0, guard true, depth >= 1 → return
///     Phase 2 right: unfold(S.left, depth=1, budget=0): budget exhausted ERROR
///
/// Assert: NO budget-exhausted diagnostics (only depth-limit diagnostics allowed).
#[test]
fn budget_not_consumed_when_depth_limit_hit() {
    // guard: n > -999 (always true for test range starting at 999)
    let guard = gt(
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(-999)),
    );
    // arg: n = n - 1
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template_s = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(999), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard(
            "left",
            "S",
            vec![("n".to_string(), n_minus_1.clone())],
            guard.clone(),
        )
        .sub_component_with_guard("right", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_s)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_depth(1);
    engine.set_max_unfold_nodes(2);
    let result = engine.eval(&module);

    // Should NOT have any budget-exhausted diagnostics
    let has_budget_error = result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains("budget exhausted"));
    assert!(
        !has_budget_error,
        "Should not have budget-exhausted errors when depth limit prevents node creation. \
         Depth-limited returns should not consume budget. Got: {:?}",
        result.diagnostics
    );
}
