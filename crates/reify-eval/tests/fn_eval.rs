//! E2E integration tests for user-defined function evaluation.
//!
//! Tests the complete pipeline: parse → compile → Engine.eval() → check values.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_test_support::mocks::MockConstraintChecker;

/// Parse source with a user function, compile, eval via Engine,
/// and verify the function result is accessible.
#[test]
fn e2e_user_fn_double_in_let() {
    let source = r#"
fn double(x: Int) -> Int { x + x }

structure S {
    let v = double(3)
}
"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check that v = double(3) = 6.0
    let v_id = ValueCellId::new("S", "v");
    let v_val = result
        .values
        .get(&v_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", v_id));
    match v_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 6.0).abs() < 1e-12, "expected 6.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            // Int(3) + Int(3) = Int(6) is also valid
            assert_eq!(*v, 6, "expected 6, got {}", v);
        }
        other => panic!("expected Real(6.0) or Int(6), got {:?}", other),
    }
}
