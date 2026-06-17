//! Compiler tests for built-in mathematical constants (pi, tau, e).

use reify_test_support::{compile_source, errors_only, parse_and_compile};
use reify_core::Type;
use reify_ir::{BinOp, CompiledExprKind, Value};

/// Helper: get the default_expr for a value cell by member name.
fn get_cell_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
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
        other => panic!(
            "expected Literal(Int(42)) for user 'pi' cell, got {:?}",
            other
        ),
    }
}

#[test]
fn user_param_pi_shadows_builtin() {
    // Use 1.5 rather than 1.0 as a purely defensive measure: the compiler correctly handles
    // Real-annotated params regardless (type_resolution maps the annotation), but 1.5 is
    // unambiguously a float literal at the parser level and avoids any future edge-case
    // where whole-number float literals might be emitted as Int.
    let src = "structure S {\n  param pi: Real = 1.5\n  let x = pi\n}";
    let compiled = compile_source(src);
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    // x should be a ValueRef to the parameter's cell, NOT the builtin literal.
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.member, "pi",
                "expected ref to 'pi' param cell, got {:?}",
                id
            );
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
        other => panic!(
            "expected Literal(Real(1.5)) for user 'pi' param cell, got {:?}",
            other
        ),
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
structure PiPart { param diameter : Length = 5mm }
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
            // Exact __list_*__<field> naming is tested in collection_sub_tests.rs;
            // here we only check the prefix to focus on shadowing.
            assert!(
                id.member.starts_with("__list_pi__"),
                "expected member starting with '__list_pi__' for collection sub 'pi', got {:?}",
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

// ─── task-1806 step-1: case-variant builtin names produce 'did you mean' hints

/// Assert that compiling `structure S { let x = <input> }` produces an
/// "unresolved name" error that suggests `<expected_hint>` as the correct spelling.
///
/// Extracted from the four structurally-identical hint tests to eliminate
/// boilerplate and make adding new case variants (e.g. "pI", "tAu") trivial.
fn assert_suggests_hint(input: &str, expected_hint: &str) {
    let src = format!("structure S {{ let x = {} }}", input);
    let compiled = compile_source(&src);
    let errors = errors_only(&compiled);
    assert!(
        !errors.is_empty(),
        "expected a compile error for '{}', got no diagnostics",
        input
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("unresolved name") && d.message.contains("did you mean")),
        "expected 'unresolved name' with 'did you mean' hint for '{}', got: {:?}",
        input,
        errors
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains(&format!("`{}`", expected_hint))),
        "expected hint to suggest `{}` for '{}', got: {:?}",
        expected_hint,
        input,
        errors
    );
}

#[test]
fn pi_uppercase_suggests_pi() {
    assert_suggests_hint("Pi", "pi");
}

#[test]
fn pi_all_caps_suggests_pi() {
    assert_suggests_hint("PI", "pi");
}

#[test]
fn tau_titlecase_suggests_tau() {
    assert_suggests_hint("Tau", "tau");
}

#[test]
fn tau_all_caps_suggests_tau() {
    assert_suggests_hint("TAU", "tau");
}

// ─── task-1806 step-4: unrelated names do NOT produce 'did you mean' hints ───

#[test]
fn unrelated_name_no_did_you_mean_hint() {
    let compiled = compile_source("structure S { let x = Foo }");
    let errors = errors_only(&compiled);
    assert!(!errors.is_empty(), "expected a compile error for 'Foo'");
    assert!(
        errors.iter().any(|d| d.message.contains("unresolved name")),
        "expected 'unresolved name' error, got: {:?}",
        errors
    );
    assert!(
        !errors.iter().any(|d| d.message.contains("did you mean")),
        "expected NO 'did you mean' hint for unrelated name 'Foo', got: {:?}",
        errors
    );
}

// ─── task-1806 step-5: lowercase pi and tau still compile without hint ────────

#[test]
fn lowercase_pi_no_hint() {
    // Verifies that the exact spelling resolves successfully — no hint emitted.
    // (A non-empty errors Vec after this assert would be a false pass, so the
    // redundant "did you mean" iteration guard has been removed.)
    let compiled = compile_source("structure S { let x = pi }");
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors for lowercase 'pi', got: {:?}",
        errors
    );
}

#[test]
fn lowercase_tau_no_hint() {
    // Same as above for tau.
    let compiled = compile_source("structure S { let x = tau }");
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors for lowercase 'tau', got: {:?}",
        errors
    );
}

// ─── task-1806 step-6: user-defined Pi in scope does NOT produce a hint ───────

#[test]
fn user_defined_pi_caps_in_scope_no_hint() {
    // The assert!(errors.is_empty()) below is sufficient: if errors is empty,
    // the subsequent "did you mean" scan over an empty Vec is a no-op.
    // Pattern follows lowercase_pi_no_hint and lowercase_tau_no_hint.
    let src = "structure S {\n  let Pi = 42\n  let x = Pi\n}";
    let compiled = compile_source(src);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors when user defines 'Pi' and uses it, got: {:?}",
        errors
    );
}

// ─── task-4174: e (Euler's number) compiler builtin ─────────────────────────

#[test]
fn e_compiles_to_literal_real() {
    let compiled = compile_source("structure S { let x = e }");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert!(
                (*v - std::f64::consts::E).abs() < 1e-15,
                "expected E, got {}",
                v
            );
        }
        other => panic!("expected Literal(Real(E)), got {:?}", other),
    }
}

#[test]
fn e_uppercase_suggests_e() {
    assert_suggests_hint("E", "e");
}

#[test]
fn lowercase_e_no_hint() {
    // Verifies that the exact spelling resolves successfully — no hint emitted.
    let compiled = compile_source("structure S { let x = e }");
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors for lowercase 'e', got: {:?}",
        errors
    );
}

#[test]
fn user_let_e_shadows_builtin() {
    // Documents that single-letter builtins like `e` are shadowable — a user-defined
    // `let e` takes precedence over the builtin Euler's number, mirroring user_let_pi_shadows_builtin.
    let src = "structure S {\n  let e = 42\n  let x = e\n}";
    let compiled = compile_source(src);
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let expr = get_cell_expr(&compiled, "x");
    // x should be a ValueRef to the user-defined e cell, NOT a Literal(Real(E)).
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.member, "e", "expected ref to 'e' cell, got {:?}", id);
        }
        CompiledExprKind::Literal(Value::Real(_)) => {
            panic!("x resolved to builtin e literal — user definition should shadow it");
        }
        other => panic!("expected ValueRef to user 'e', got {:?}", other),
    }
    // The e cell itself should hold the user-provided literal 42, not E ≈ 2.718.
    let e_expr = get_cell_expr(&compiled, "e");
    match &e_expr.kind {
        CompiledExprKind::Literal(Value::Int(42)) => {}
        other => panic!(
            "expected Literal(Int(42)) for user 'e' cell, got {:?}",
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
