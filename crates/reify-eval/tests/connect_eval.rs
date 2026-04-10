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
        result
            .constraint_results
            .iter()
            .map(|e| &e.id)
            .collect::<Vec<_>>()
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
            conn.compatibility_constraint,
            conn.left_port,
            conn.right_port
        );
        assert_eq!(
            entry.unwrap().satisfaction,
            Satisfaction::Satisfied,
            "connection {}->{}  should be Satisfied",
            conn.left_port,
            conn.right_port
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
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

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
    assert!(
        compat_entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Violated,
        "In->In connection should be Violated"
    );
}

// ── task-247/step-11: eval_auto_match_propagates_port_mappings ────────

/// Auto-matched Out->In ports with 2 matching members (d and angle).
/// Verifies the compiled port_mappings contains both identity pairs (sorted
/// alphabetically) and that engine.check() evaluates the compatibility as Satisfied.
#[test]
fn eval_auto_match_propagates_port_mappings() {
    let source = r#"
trait MechPort {
    param d : Length
    param angle : Real
}
structure def S {
    port a : out MechPort {
        param d : Length = 5mm
        param angle : Real = 0.0
    }
    port b : in MechPort {
        param d : Length = 5mm
        param angle : Real = 0.0
    }
    connect a -> b
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1);
    // Auto-match: sorted alphabetically → [("angle","angle"), ("d","d")]
    assert_eq!(
        template.connections[0].port_mappings,
        vec![
            ("angle".to_string(), "angle".to_string()),
            ("d".to_string(), "d".to_string()),
        ],
        "expected auto-generated identity mappings sorted alphabetically"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_id = &template.connections[0].compatibility_constraint;
    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In auto-matched connection should be Satisfied"
    );
}

// ── task-247/step-12: eval_explicit_mapping_constraint_satisfied ──────

/// Compile with explicit `{ d -> d }` mapping and run engine.check().
/// Constraint should be Satisfied and compiled port_mappings equals the explicit pair.
#[test]
fn eval_explicit_mapping_constraint_satisfied() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { d -> d }
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d in compiled template"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_id = &template.connections[0].compatibility_constraint;
    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In with explicit mapping should be Satisfied"
    );
}

// ── task-247/step-13: eval_mixed_params_and_mappings_connector_created

/// BoltSet connector body with a param and an explicit mapping.
/// Verifies: no error diagnostics, connector sub-component present in S,
/// compatibility constraint Satisfied, port_mappings holds the explicit pair.
#[test]
fn eval_mixed_params_and_mappings_connector_created() {
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 10.9, d -> d }
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    assert_eq!(s_template.connections.len(), 1);

    let conn = &s_template.connections[0];
    assert!(conn.connector_sub.is_some(), "expected connector_sub");

    let connector_name = conn.connector_sub.as_ref().unwrap();
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name);
    assert!(
        connector_sub.is_some(),
        "expected connector sub-component in S template"
    );
    assert_eq!(connector_sub.unwrap().structure_name, "BoltSet");

    assert_eq!(
        conn.port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d preserved"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == conn.compatibility_constraint);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "mixed params+mappings connection should be Satisfied"
    );
}

// ── task-247/step-14: m10_connect_advanced_ri_parses_and_compiles ─────

/// Absolute path to the port-mapping example file.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_connect_advanced.ri"
);

/// Read m10_connect_advanced.ri, verify it parses without errors, compiles
/// without Error-severity diagnostics, and produces at least one template.
#[test]
fn m10_connect_advanced_ri_parses_and_compiles() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_connect_advanced.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
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

    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── task-247/step-16: m10_connect_advanced_ri_all_constraints_satisfied

/// End-to-end integration test: parse + compile + engine.check() on the example
/// file. Every constraint result must be Satisfied (no violations).
#[test]
fn m10_connect_advanced_ri_all_constraints_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_connect_advanced.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
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

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
    );

    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
