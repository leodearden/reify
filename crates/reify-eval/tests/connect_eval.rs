//! Integration tests for connect/chain evaluation.
//!
//! Tests the complete pipeline: parse → compile → Engine.check() → verify constraint results.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity};

/// Parse and compile source with two ports and a connect statement.
/// Run engine.check() and assert the connection's compatibility constraint
/// appears as Satisfied (since Out->In is compatible).
#[test]
fn eval_connect_constraints() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
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

    // Verify compilation produced a connection and its constraint
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    let compat_id = &template.connections[0].compatibility_constraint;

    // Check: eval + constraint checking
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // The compatibility constraint should appear in results as Satisfied
    let compat_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        compat_entry.is_some(),
        "expected compatibility constraint {:?} in results, got: {:?}",
        compat_id,
        result.constraint_results.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In connection should be Satisfied"
    );
}

/// Test that chain desugaring produces correct constraint results.
#[test]
fn eval_chain_constraints() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 2mm }
    port c : in T { param d : Length = 3mm }
    chain a -> b -> c
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

    // Should have 2 connections from chain desugaring
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 2);

    // Check
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // Both compatibility constraints should be Satisfied
    for conn in &template.connections {
        let entry = result
            .constraint_results
            .iter()
            .find(|e| e.id == conn.compatibility_constraint);
        assert!(
            entry.is_some(),
            "expected compatibility constraint {:?} for {}->{} in results",
            conn.compatibility_constraint, conn.left_port, conn.right_port
        );
        assert_eq!(
            entry.unwrap().satisfaction,
            Satisfaction::Satisfied,
            "connection {}->{}  should be Satisfied",
            conn.left_port, conn.right_port
        );
    }
}

/// Test that an incompatible connection (In -> In) produces a Violated constraint.
#[test]
fn eval_incompatible_connect_constraint() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    // Should have a direction error diagnostic, but still produce a connection
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    let compat_id = &template.connections[0].compatibility_constraint;

    // Use real constraint checker so literal Bool(false) evaluates to Violated
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(compat_entry.is_some(), "expected compatibility constraint in results");
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Violated,
        "In->In connection should be Violated"
    );
}
