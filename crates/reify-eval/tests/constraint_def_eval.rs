//! End-to-end eval tests for constraint def instantiations (task 198).
//!
//! Verifies that constraint defs compiled with labeled predicates produce:
//! - ConstraintCheckEntry with label==Some("DefName[N]")
//! - Diagnostic messages that use the label instead of the raw ConstraintNodeId
//! - Individual satisfaction states per predicate

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_types::{ModulePath, Satisfaction, Severity};

// ── Helper ────────────────────────────────────────────────────────────────────

fn check_source(source: &str) -> reify_eval::CheckResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("constraint_def_eval_test"));
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

    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);
    engine.check(&compiled)
}

// ── step-3: violated constraint def produces labeled diagnostic ───────────────

/// A violated constraint def instantiation should produce:
/// - A ConstraintCheckEntry with satisfaction==Violated and label==Some("MinWall[0]")
/// - At least one Error diagnostic containing the string "MinWall[0]"
#[test]
fn violated_constraint_def_produces_labeled_diagnostic() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param thickness: Length = 1
    constraint MinWall(wall: thickness)
}
"#;
    let result = check_source(source);

    // Exactly one constraint result
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Violated,
        "expected Violated, got: {:?}",
        entry.satisfaction
    );
    assert_eq!(
        entry.label,
        Some("MinWall[0]".to_string()),
        "expected label Some(\"MinWall[0]\"), got: {:?}",
        entry.label
    );

    // At least one Error diagnostic containing "MinWall[0]"
    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic"
    );
    let has_label = error_diags
        .iter()
        .any(|d| d.message.contains("MinWall[0]"));
    assert!(
        has_label,
        "expected at least one Error diagnostic containing 'MinWall[0]', got: {:?}",
        error_diags
    );
}

// ── step-6: multi-predicate individual violations ────────��────────────────────

/// Two-predicate Bounded constraint: w=15, lo=1, hi=10.
/// Bounded[0] (x >= lo: 15 >= 1) is Satisfied.
/// Bounded[1] (x <= hi: 15 <= 10) is Violated.
/// The violated diagnostic should mention "Bounded[1]" but not "Bounded[0]".
#[test]
fn multi_predicate_individual_violations() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param w: Length = 15
    constraint Bounded(x: w, lo: 1, hi: 10)
}
"#;
    let result = check_source(source);

    // Exactly two constraint results
    assert_eq!(
        result.constraint_results.len(),
        2,
        "expected 2 constraint results, got: {:?}",
        result.constraint_results
    );

    // Find by label
    let bounded0 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("Bounded[0]".to_string()))
        .expect("expected entry with label Bounded[0]");
    let bounded1 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("Bounded[1]".to_string()))
        .expect("expected entry with label Bounded[1]");

    assert_eq!(
        bounded0.satisfaction,
        Satisfaction::Satisfied,
        "Bounded[0] (x >= lo) should be Satisfied with w=15, lo=1"
    );
    assert_eq!(
        bounded1.satisfaction,
        Satisfaction::Violated,
        "Bounded[1] (x <= hi) should be Violated with w=15, hi=10"
    );

    // Diagnostic should mention Bounded[1] but not Bounded[0]
    let error_msgs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| &d.message)
        .collect();
    assert!(
        !error_msgs.is_empty(),
        "expected at least one Error diagnostic"
    );
    let has_bounded1 = error_msgs.iter().any(|m| m.contains("Bounded[1]"));
    let has_bounded0 = error_msgs.iter().any(|m| m.contains("Bounded[0]"));
    assert!(
        has_bounded1,
        "expected diagnostic mentioning 'Bounded[1]', got: {:?}",
        error_msgs
    );
    assert!(
        !has_bounded0,
        "expected no diagnostic mentioning 'Bounded[0]' (it is satisfied), got: {:?}",
        error_msgs
    );
}

// ── step-4: satisfied constraint def has label, no error ─────────────────────

/// A satisfied constraint def instantiation should produce:
/// - A ConstraintCheckEntry with satisfaction==Satisfied and label==Some("MinWall[0]")
/// - No Error diagnostics
#[test]
fn satisfied_constraint_def_has_label_no_error() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param thickness: Length = 5
    constraint MinWall(wall: thickness)
}
"#;
    let result = check_source(source);

    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "expected Satisfied, got: {:?}",
        entry.satisfaction
    );
    assert_eq!(
        entry.label,
        Some("MinWall[0]".to_string()),
        "expected label Some(\"MinWall[0]\"), got: {:?}",
        entry.label
    );

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        error_diags
    );
}
