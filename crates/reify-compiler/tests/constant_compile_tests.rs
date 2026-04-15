//! Compiler tests for built-in mathematical constants (pi, tau).

use reify_test_support::{compile_source, errors_only, parse_and_compile};
use reify_types::{CompiledExprKind, Value};

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
