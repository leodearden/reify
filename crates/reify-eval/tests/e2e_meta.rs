//! E2E tests for meta block access: parse → compile → Engine.eval().
//!
//! Exercises the full parse→compile→eval pipeline for `meta.key` expressions,
//! ensuring integration across the parser, compiler, and evaluator boundaries.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

// ---------------------------------------------------------------------------
// step-13: E2E — let binding using meta.key resolves to Value::String
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with meta block + let binding using `meta.key`,
/// compile, eval, assert the let binding resolves to the expected string.
#[test]
fn e2e_meta_access_let_binding() {
    let source = r#"
        structure def Widget {
            meta {
                description = "A widget"
            }
            let desc : String = meta.description
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

    // Assert
    let desc_id = ValueCellId::new("Widget", "desc");
    assert_eq!(
        result.values.get(&desc_id),
        Some(&Value::String("A widget".to_string())),
        "Widget.desc should resolve to 'A widget' via meta.description"
    );
}

// ---------------------------------------------------------------------------
// step-15: E2E — multiple meta keys in one block
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with two meta keys, two let bindings each reading
/// a different key.  Both should resolve to their respective string values.
#[test]
fn e2e_meta_access_multiple_keys() {
    let source = r#"
        structure def Gear {
            meta {
                name = "Gear",
                version = "2.0"
            }
            let n : String = meta.name
            let v : String = meta.version
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

    // Assert both keys
    let n_id = ValueCellId::new("Gear", "n");
    let v_id = ValueCellId::new("Gear", "v");

    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::String("Gear".to_string())),
        "Gear.n should resolve to 'Gear' via meta.name"
    );
    assert_eq!(
        result.values.get(&v_id),
        Some(&Value::String("2.0".to_string())),
        "Gear.v should resolve to '2.0' via meta.version"
    );
}

// ---------------------------------------------------------------------------
// step-17: E2E — meta.key in a constraint expression
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with `constraint meta.tag == "valid"`.
/// The constraint expression contains a MetaAccess node; eval should not panic,
/// and the constraint result should be Satisfied (MockConstraintChecker default).
#[test]
fn e2e_meta_access_in_constraint() {
    let source = r#"
        structure def S {
            meta {
                tag = "valid"
            }
            constraint meta.tag == "valid"
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

    // Check (eval + constraint evaluation) — must not panic when meta.key
    // appears in a constraint expression
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // Assert no constraint violations
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
