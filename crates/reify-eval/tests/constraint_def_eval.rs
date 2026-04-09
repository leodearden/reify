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
    let has_label = error_diags.iter().any(|d| d.message.contains("MinWall[0]"));
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

// ── step-10: guarded constraint def inactive when guard is false ──────────────

/// When a constraint def instantiation has a where clause and the guard
/// evaluates to false, no constraint should be checked and no violation
/// diagnostics should be emitted.
#[test]
fn guarded_constraint_def_inactive_when_guard_false() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param active: Bool = false
    param t: Length = 1
    constraint MinWall(wall: t) where active
}
"#;
    let result = check_source(source);

    // Guard is false → no constraint results at all
    assert!(
        result.constraint_results.is_empty(),
        "expected no constraint results when guard is false, got: {:?}",
        result.constraint_results
    );

    // No violation diagnostics
    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected no Error diagnostics when guard is false, got: {:?}",
        error_diags
    );
}

// ── step-8: predicates checked independently (transparent to solver) ──────────

/// 3-predicate constraint def with individually distinct satisfaction states:
/// - Triple[0] (a > 0): x=5, 5 > 0 → Satisfied
/// - Triple[1] (a > b): x=5, y=10, 5 > 10 → Violated
/// - Triple[2] (a > c): z has no default → Indeterminate
///
/// This proves each predicate is checked independently.
#[test]
fn constraint_def_predicates_transparent_to_checking() {
    let source = r#"
constraint def Triple {
    param a: Length
    param b: Length
    param c: Length
    a > 0
    a > b
    a > c
}
structure S {
    param x: Length = 5
    param y: Length = 10
    param z: Length
    constraint Triple(a: x, b: y, c: z)
}
"#;
    let result = check_source(source);

    // Exactly 3 constraint results
    assert_eq!(
        result.constraint_results.len(),
        3,
        "expected 3 constraint results, got: {:?}",
        result.constraint_results
    );

    let find_entry = |label: &str| {
        result
            .constraint_results
            .iter()
            .find(|e| e.label == Some(label.to_string()))
            .unwrap_or_else(|| panic!("expected entry with label '{label}'"))
    };

    let t0 = find_entry("Triple[0]");
    let t1 = find_entry("Triple[1]");
    let t2 = find_entry("Triple[2]");

    assert_eq!(
        t0.satisfaction,
        Satisfaction::Satisfied,
        "Triple[0] (a > 0 with a=5) should be Satisfied"
    );
    assert_eq!(
        t1.satisfaction,
        Satisfaction::Violated,
        "Triple[1] (a > b with a=5, b=10) should be Violated"
    );
    assert_eq!(
        t2.satisfaction,
        Satisfaction::Indeterminate,
        "Triple[2] (a > c with c=Undef) should be Indeterminate"
    );
}

// ── step-12: inline constraint diagnostics are unchanged (regression) ─────────

/// An inline constraint (not from a constraint def) has no label.
/// When violated, the diagnostic message should still use the raw
/// ConstraintNodeId format 'S#constraint[0]'. This is a regression test
/// ensuring the labeled_diagnostics helper is a no-op when label is None.
#[test]
fn inline_constraint_diagnostics_unchanged() {
    let source = r#"
structure S {
    param thickness: Length = 1
    constraint thickness > 2
}
"#;
    let result = check_source(source);

    // Should have exactly 1 violated constraint
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
    // Inline constraint has no label
    assert_eq!(
        entry.label, None,
        "inline constraint should have no label, got: {:?}",
        entry.label
    );

    // The diagnostic should use the raw ConstraintNodeId format
    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic"
    );
    let has_raw_id = error_diags
        .iter()
        .any(|d| d.message.contains("S#constraint[0]"));
    assert!(
        has_raw_id,
        "expected inline diagnostic containing 'S#constraint[0]' (raw ConstraintNodeId), got: {:?}",
        error_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
