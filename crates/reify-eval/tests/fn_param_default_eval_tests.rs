//! End-to-end eval tests for fn_param default consumption (task 3688, step-1).
//!
//! Tests A and B exercise the full parse → compile → eval pipeline for functions
//! with parameter defaults. The file is written RED-first (TDD): Test A fails
//! until step-2 implements default-padding at the call site.
//!
//! Test A — all-defaulted call: currently RED because `compile` emits
//! "no matching overload for f()" (0-arg call, 1-param function, no padding yet).
//! Test B — explicit arg overrides default: currently resolves via the normal
//! exact-match path (1-Real-arg call, 1-Real-param function) and is GREEN.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity, Value, ValueCellId};

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
