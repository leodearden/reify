//! Compiler tests for built-in mathematical constants (pi, tau).

use reify_test_support::{compile_source, errors_only};
use reify_types::{BinOp, CompiledExprKind, Value};

/// Helper: get the default_expr for a value cell by member name.
fn get_cell_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_types::CompiledExpr {
    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("should have '{}' value cell", member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{}' should have a default expr", member))
}

// ─── step-1: pi and tau resolve to literal Real constants ────────────────────

#[test]
fn pi_compiles_to_literal_real() {
    let compiled = compile_source("structure S { let x = pi }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert!(
                (*v - std::f64::consts::PI).abs() < 1e-15,
                "expected PI, got {}",
                v
            );
        }
        other => panic!("expected Literal(Real(PI)), got {:?}", other),
    }
}

#[test]
fn tau_compiles_to_literal_real() {
    let compiled = compile_source("structure S { let x = tau }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert!(
                (*v - std::f64::consts::TAU).abs() < 1e-15,
                "expected TAU, got {}",
                v
            );
        }
        other => panic!("expected Literal(Real(TAU)), got {:?}", other),
    }
}

// ─── step-3: pi and tau in arithmetic expressions ────────────────────────────

#[test]
fn pi_in_multiplication() {
    let compiled = compile_source("structure S { let y = 2 * pi }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "y");
    match &expr.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(*op, BinOp::Mul, "expected Mul, got {:?}", op);
        }
        other => panic!("expected BinOp(Mul), got {:?}", other),
    }
}

#[test]
fn pi_in_division() {
    let compiled = compile_source("structure S { let z = pi / 2 }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "z");
    match &expr.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(*op, BinOp::Div, "expected Div, got {:?}", op);
        }
        other => panic!("expected BinOp(Div), got {:?}", other),
    }
}

#[test]
fn pi_plus_tau_expression() {
    let compiled = compile_source("structure S { let w = pi + tau }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "w");
    match &expr.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(*op, BinOp::Add, "expected Add, got {:?}", op);
        }
        other => panic!("expected BinOp(Add), got {:?}", other),
    }
}

// ─── step-5: user-defined `let pi` shadows the builtin ──────────────────────

#[test]
fn user_let_pi_shadows_builtin() {
    let src = "structure S {\n  let pi = 42\n  let x = pi\n}";
    let compiled = compile_source(src);
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    // x should be a ValueRef to the user-defined pi cell, NOT a Literal(Real(PI)).
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.member, "pi", "expected ref to 'pi' cell, got {:?}", id);
        }
        CompiledExprKind::Literal(Value::Real(_)) => {
            panic!("x resolved to builtin pi literal — user definition should shadow it");
        }
        other => panic!("expected ValueRef to user 'pi', got {:?}", other),
    }
}
