//! Boundary 2 (compiler → eval) — Consumer-side tests.
//!
//! These tests verify that the evaluator can accept well-formed CompiledModules
//! and evaluate them correctly.

use reify_core::Type;
use reify_ir::{Satisfaction, ValueMap};
use reify_test_support::*;

/// Accept well-formed CompiledModule → evaluate to values.
#[test]

fn accept_well_formed_compiled_module() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);
    assert!(!result.values.is_empty());
}

/// Handle empty realizations (no geometry).
#[test]

fn handle_empty_realizations() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);
    // Should succeed even without geometry kernel
    assert!(result.diagnostics.is_empty());
}

/// Evaluate bracket with defaults → all params Determined.
#[test]

fn bracket_defaults_evaluated() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // All 5 params should have values
    for name in &[
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
    ] {
        let id = vcid("Bracket", name);
        let val = result.values.get(&id);
        assert!(val.is_some(), "{} should be present", name);
        assert!(!val.unwrap().is_undef(), "{} should not be undef", name);
    }
}

/// Evaluate bracket → volume let = width * height * thickness.
#[test]

fn bracket_volume_computed() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    let volume = result
        .values
        .get(&vcid("Bracket", "volume"))
        .expect("volume should be present");

    // 80mm * 100mm * 5mm = 0.08 * 0.1 * 0.005 = 4e-5 m³
    let v = volume.as_f64().expect("volume should be numeric");
    assert!(
        (v - 4e-5).abs() < 1e-12,
        "volume should be 4e-5 m³, got {}",
        v
    );
}

/// Check bracket → all 3 constraints Satisfied with default values.
#[test]

fn bracket_constraints_satisfied() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new(); // Real checker tested in boundary3
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be satisfied",
            entry.id
        );
    }
}

/// ValueRef to Undef cell → propagate Undef correctly.
#[test]
fn undef_propagation_through_eval_expr() {
    // This test uses eval_expr directly (doesn't need Engine implementation)
    let thickness_ref =
        reify_ir::CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
    let two_mm = reify_ir::CompiledExpr::literal(mm(2.0), Type::length());
    let expr =
        reify_ir::CompiledExpr::binop(reify_ir::BinOp::Gt, thickness_ref, two_mm, Type::Bool);

    // Empty value map → thickness is Undef
    let values = ValueMap::new();
    let result = reify_expr::eval_expr(&expr, &reify_expr::EvalContext::simple(&values));
    assert!(result.is_undef(), "should propagate undef");
}
