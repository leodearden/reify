//! Compiler tests for built-in mathematical constants (pi, tau).

use reify_test_support::{compile_source, errors_only, parse_and_compile};
use reify_types::{BinOp, CompiledExprKind, Type, Value};

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
    // The pi cell itself should hold the user-provided literal 42, not the builtin Real constant.
    let pi_expr = get_cell_expr(&compiled, "pi");
    match &pi_expr.kind {
        CompiledExprKind::Literal(Value::Int(42)) => {}
        other => panic!("expected Literal(Int(42)) for user 'pi' cell, got {:?}", other),
    }
}

#[test]
fn user_param_pi_shadows_builtin() {
    // Use 1.5 (genuinely fractional) so the compiler emits Literal(Real(1.5)), not Int.
    let src = "structure S {\n  param pi: Real = 1.5\n  let x = pi\n}";
    let compiled = compile_source(src);
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    // x should be a ValueRef to the parameter's cell, NOT the builtin literal.
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.member, "pi", "expected ref to 'pi' param cell, got {:?}", id);
        }
        CompiledExprKind::Literal(Value::Real(_)) => {
            panic!("x resolved to builtin pi literal — param definition should shadow it");
        }
        other => panic!("expected ValueRef to param 'pi', got {:?}", other),
    }
    // The pi param cell should hold the user-provided default 1.5, not the builtin pi ≈ 3.14159.
    let pi_expr = get_cell_expr(&compiled, "pi");
    match &pi_expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert!(
                (*v - 1.5_f64).abs() < 1e-15,
                "expected param default 1.5, got {}",
                v
            );
        }
        other => panic!("expected Literal(Real(1.5)) for user 'pi' param cell, got {:?}", other),
    }
}

// ─── collection sub-name shadows builtin constant ────────────────────────────

#[test]
fn collection_sub_named_pi_shadows_builtin() {
    // `sub pi : List<PiPart>` declares a collection sub whose name coincides with the
    // builtin `pi` constant. The compiler checks collection_sub_names BEFORE
    // resolve_builtin_constant, so `let x = pi` must resolve to the collection list,
    // not the Real constant.
    let src = "\
structure PiPart { param diameter : Scalar = 5mm }
structure S {
  sub pi : List<PiPart>
  constraint pi.count == 2
  let x = pi
}";
    let compiled = parse_and_compile(src);
    // Find template S (not PiPart).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template 'S'");
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");
    let expr = cell
        .default_expr
        .as_ref()
        .expect("'x' should have a default expr");
    // x must resolve to the collection list ValueRef, NOT Literal(Real(PI)).
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert!(
                id.member.starts_with("__list_pi__"),
                "expected member starting with '__list_pi__', got {:?}",
                id.member
            );
            assert!(
                matches!(&expr.result_type, Type::List(_)),
                "expected result_type Type::List(_), got {:?}",
                expr.result_type
            );
        }
        CompiledExprKind::Literal(Value::Real(_)) => {
            panic!("x resolved to builtin pi literal — collection sub name should shadow it");
        }
        other => panic!(
            "expected ValueRef with __list_pi__ member for collection sub 'pi', got {:?}",
            other
        ),
    }
}

// ─── step-7: pi works under #no_prelude ─────────────────────────────────────

#[test]
fn pi_works_under_no_prelude() {
    let compiled = compile_source("#no_prelude\nstructure S { let x = pi }");
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "pi should resolve even with #no_prelude, got: {:?}",
        errors
    );
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
