//! End-to-end eval tests for fn_param default consumption (task 3688).
//!
//! Tests A/B (step-1/step-2): single-param function with a default.
//! Tests C/D (step-3): multi-param with provided-prefix + defaulted-suffix.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// Test A: all-defaulted call evaluates to the default value.
///
/// `fn f(x : Real = 1.0) -> Real { x }`
/// `structure S { let v = f() }`
///
/// Expects: `S.v = 1.0`
///
/// Currently RED: compile emits "no matching overload for f()" because
/// default-padding at the call site is not yet implemented (task 3688 step-2).
#[test]
fn fn_param_default_all_defaulted_eval() {
    let source = r#"
fn f(x : Real = 1.0) -> Real { x }

structure S {
    let v = f()
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_default_a"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    // Currently RED: fails here with "no matching overload for f()"
    // Will pass when step-2 implements default-padding.
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let v_id = ValueCellId::new("S", "v");
    let v_val = result
        .values
        .get(&v_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in eval result", v_id));
    match v_val {
        Value::Real(v) => {
            assert!((v - 1.0).abs() < 1e-12, "expected 1.0, got {}", v);
        }
        other => panic!("expected Value::Real(1.0), got {:?}", other),
    }
}

/// Test B: explicit arg overrides the default.
///
/// `fn f(x : Real = 1.0) -> Real { x }`
/// `structure S { let v = f(7.0) }`
///
/// Expects: `S.v = 7.0`
///
/// Resolves via normal exact-match path (1 Real arg, 1 Real param).
/// Serves as a regression guard that explicit calls are unaffected by default-padding.
#[test]
fn fn_param_default_explicit_overrides_eval() {
    let source = r#"
fn f(x : Real = 1.0) -> Real { x }

structure S {
    let v = f(7.0)
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_default_b"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let v_id = ValueCellId::new("S", "v");
    let v_val = result
        .values
        .get(&v_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in eval result", v_id));
    match v_val {
        Value::Real(v) => {
            assert!((v - 7.0).abs() < 1e-12, "expected 7.0, got {}", v);
        }
        other => panic!("expected Value::Real(7.0), got {:?}", other),
    }
}

/// Test C: provided prefix + defaulted suffix.
///
/// `fn g(a : Real, b : Real = 2.0) -> Real { a + b }`
/// `structure S { let v = g(10.0) }`
///
/// Expects: `S.v = 12.0` (10.0 + default 2.0)
#[test]
fn fn_param_default_partial_application_eval() {
    let source = r#"
fn g(a : Real, b : Real = 2.0) -> Real { a + b }

structure S {
    let v = g(10.0)
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_default_c"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let v_id = ValueCellId::new("S", "v");
    let v_val = result
        .values
        .get(&v_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in eval result", v_id));
    match v_val {
        Value::Real(v) => {
            assert!(
                (v - 12.0).abs() < 1e-12,
                "expected 12.0 (10.0 + 2.0), got {}",
                v
            );
        }
        other => panic!("expected Value::Real(12.0), got {:?}", other),
    }
}

/// Test D: all args supplied to a function with defaults — normal exact-match path.
///
/// `fn g(a : Real, b : Real = 2.0) -> Real { a + b }`
/// `structure S { let v = g(10.0, 5.0) }`
///
/// Expects: `S.v = 15.0` (10.0 + 5.0; default for b is overridden)
#[test]
fn fn_param_default_all_supplied_eval() {
    let source = r#"
fn g(a : Real, b : Real = 2.0) -> Real { a + b }

structure S {
    let v = g(10.0, 5.0)
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_default_d"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let v_id = ValueCellId::new("S", "v");
    let v_val = result
        .values
        .get(&v_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in eval result", v_id));
    match v_val {
        Value::Real(v) => {
            assert!(
                (v - 15.0).abs() < 1e-12,
                "expected 15.0 (10.0 + 5.0), got {}",
                v
            );
        }
        other => panic!("expected Value::Real(15.0), got {:?}", other),
    }
}
