//! End-to-end eval tests for constraint def instantiations (task 198).
//!
//! Verifies that constraint defs compiled with labeled predicates produce:
//! - ConstraintCheckEntry with label==Some("DefName[N]")
//! - Diagnostic messages that use the label instead of the raw ConstraintNodeId
//! - Individual satisfaction states per predicate

use reify_test_support::{check_source, error_diags, parse_and_compile};
use reify_types::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Diagnostic,
    DiagnosticLabel, Satisfaction, Severity, SourceSpan,
};

/// Shared fixture: violated single-predicate constraint def instantiation.
/// `thickness = 1` means `wall > 2` is Violated, producing label `MinWall#0[0]`.
const MIN_WALL_SOURCE: &str = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param thickness: Length = 1
    constraint MinWall(wall: thickness)
}
"#;

// ── step-3: violated constraint def produces labeled diagnostic ───────────────

/// A violated constraint def instantiation should produce:
/// - A ConstraintCheckEntry with satisfaction==Violated and label==Some("MinWall#0[0]")
/// - At least one Error diagnostic containing the string "MinWall#0[0]"
#[test]
fn violated_constraint_def_produces_labeled_diagnostic() {
    let result = check_source(MIN_WALL_SOURCE);

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
        Some("MinWall#0[0]".to_string()),
        "expected label Some(\"MinWall#0[0]\"), got: {:?}",
        entry.label
    );

    // At least one Error diagnostic containing "MinWall#0[0]"
    let error_diags = error_diags(&result.diagnostics);
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic"
    );
    let has_label = error_diags.iter().any(|d| d.message.contains("MinWall#0[0]"));
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
/// The violated diagnostic should mention "Bounded#0[1]" but not "Bounded#0[0]".
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
        .find(|e| e.label == Some("Bounded#0[0]".to_string()))
        .expect("expected entry with label Bounded[0]");
    let bounded1 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("Bounded#0[1]".to_string()))
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
    let has_bounded1 = error_msgs.iter().any(|m| m.contains("Bounded#0[1]"));
    let has_bounded0 = error_msgs.iter().any(|m| m.contains("Bounded#0[0]"));
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
/// - A ConstraintCheckEntry with satisfaction==Satisfied and label==Some("MinWall#0[0]")
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
        Some("MinWall#0[0]".to_string()),
        "expected label Some(\"MinWall#0[0]\"), got: {:?}",
        entry.label
    );

    let error_diags = error_diags(&result.diagnostics);
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
    let error_diags = error_diags(&result.diagnostics);
    assert!(
        error_diags.is_empty(),
        "expected no Error diagnostics when guard is false, got: {:?}",
        error_diags
    );
}

// ── step-19 (task 848.3): guarded constraint def active-and-violated case ─────

/// Complement to `guarded_constraint_def_inactive_when_guard_false`: when the
/// `where` guard evaluates true AND the constraint predicate is violated,
/// exactly one Violated constraint result must be produced and the error
/// diagnostic must carry the friendly constraint-instance label.
///
/// Covers the active-true, predicate-false branch of the guard machinery —
/// the only combination the existing test suite did not exercise end-to-end.
#[test]
fn guarded_constraint_def_violated_when_guard_true() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param active: Bool = true
    param t: Length = 1
    constraint MinWall(wall: t) where active
}
"#;
    let result = check_source(source);

    // Guard is true → the constraint is checked and the predicate (1 > 2) is violated.
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result when guard is true, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Violated,
        "expected Violated when guard is true and predicate fails, got: {:?}",
        entry.satisfaction
    );
    assert_eq!(
        entry.label,
        Some("MinWall#0[0]".to_string()),
        "expected label Some(\"MinWall#0[0]\") per task-845 labeling, got: {:?}",
        entry.label
    );

    // Exactly one Error-severity diagnostic, and its message carries the label.
    let error_diags = error_diags(&result.diagnostics);
    assert_eq!(
        error_diags.len(),
        1,
        "expected exactly one Error diagnostic, got: {:?}",
        error_diags
    );
    assert!(
        error_diags[0].message.contains("MinWall#0[0]"),
        "expected error diagnostic containing 'MinWall#0[0]', got: {:?}",
        error_diags[0].message
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

    let t0 = find_entry("Triple#0[0]");
    let t1 = find_entry("Triple#0[1]");
    let t2 = find_entry("Triple#0[2]");

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
    let error_diags = error_diags(&result.diagnostics);
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

// ── step-12: labeled_diagnostics replaces id_str in d.labels[i].message ───────

/// A `ConstraintChecker` that always reports `Violated` and emits a Diagnostic
/// with BOTH `message` and a `labels[0].message` that contain the raw
/// ConstraintNodeId string.
///
/// Used to verify that `Engine::labeled_diagnostics` replaces the id_str in
/// every per-label message — not only in the top-level `.message` field.
/// Prior to task 846.1, the helper only rewrote `d.message` and left
/// `d.labels[i].message` carrying the opaque "S#constraint[0]" style id.
struct LabelEmittingChecker;

impl ConstraintChecker for LabelEmittingChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| {
                let id_str = id.to_string();
                let label = DiagnosticLabel::new(
                    SourceSpan::empty(0),
                    format!("near {}", id_str),
                );
                let diagnostic = Diagnostic::error(format!("constraint {} violated", id_str))
                    .with_label(label);
                ConstraintResult {
                    id: id.clone(),
                    satisfaction: Satisfaction::Violated,
                    diagnostics: ConstraintDiagnostics {
                        messages: vec![diagnostic],
                    },
                }
            })
            .collect()
    }
}

/// When a labeled constraint (from a constraint def instantiation) produces a
/// Diagnostic with a non-empty `labels: Vec<DiagnosticLabel>`, both `d.message`
/// AND every `d.labels[i].message` must have the raw ConstraintNodeId string
/// replaced by the friendly label (e.g. "MinWall#0[0]").
///
/// This test uses a custom checker that deliberately seeds both fields with
/// the id_str, then asserts neither carries the opaque "S#constraint[" form
/// after engine-level post-processing.
#[test]
fn labeled_diagnostics_replaces_id_in_labels_messages() {
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
    let compiled = parse_and_compile(source);
    let mut engine = reify_eval::Engine::new(Box::new(LabelEmittingChecker), None);
    let result = engine.check(&compiled);

    let error_diags = error_diags(&result.diagnostics);
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic"
    );

    for d in &error_diags {
        // d.message must be rewritten — already covered today but we assert
        // it here so the test locks the full contract, not just the labels case.
        assert!(
            d.message.contains("MinWall#0[0]"),
            "expected main message containing 'MinWall#0[0]', got: {:?}",
            d.message
        );
        assert!(
            !d.message.contains("S#constraint["),
            "main message should NOT contain raw ConstraintNodeId 'S#constraint[', got: {:?}",
            d.message
        );

        // Every label's message must also be rewritten.
        assert!(
            !d.labels.is_empty(),
            "test setup expected non-empty labels on each diagnostic"
        );
        for lbl in &d.labels {
            assert!(
                lbl.message.contains("MinWall#0[0]"),
                "expected label message containing 'MinWall#0[0]', got: {:?}",
                lbl.message
            );
            assert!(
                !lbl.message.contains("S#constraint["),
                "label message should NOT contain raw ConstraintNodeId 'S#constraint[', got: {:?}",
                lbl.message
            );
        }
    }
}

// ── step-13: non-embedding checker does not panic on labeled constraint ────────

/// A `ConstraintChecker` that always reports `Violated` with a domain-specific
/// message that deliberately does NOT embed the raw `ConstraintNodeId` string.
///
/// Used to verify that `Engine::labeled_diagnostics` does not panic (debug_assert
/// must be demoted to `tracing::debug!`) when the checker omits the raw id, and
/// that the domain message passes through to users verbatim.
struct NonEmbeddingChecker;

impl ConstraintChecker for NonEmbeddingChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Violated,
                diagnostics: ConstraintDiagnostics {
                    messages: vec![Diagnostic::error("wall thickness below minimum")],
                },
            })
            .collect()
    }
}

/// A `ConstraintChecker` that emits an Error diagnostic whose message does NOT
/// contain the raw `ConstraintNodeId` string is a valid third-party checker:
/// before the fix, the `debug_assert!` in `labeled_diagnostics` would panic in
/// debug builds; after the fix it should be demoted to `tracing::debug!` so that:
/// 1. The call to `engine.check()` returns without panicking.
/// 2. Exactly one `Severity::Error` diagnostic is present.
/// 3. The diagnostic message is the domain text "wall thickness below minimum"
///    unchanged — the engine must pass it through verbatim when the raw id is absent.
/// 4. The engine still attaches the friendly label (`Some("MinWall#0[0]")`) to the
///    `ConstraintCheckEntry` even when the checker's message omits the raw id —
///    confirming that label attachment is independent of message content.
#[test]
fn non_embedding_checker_does_not_panic_on_labeled_constraint() {
    let compiled = parse_and_compile(MIN_WALL_SOURCE);
    let mut engine = reify_eval::Engine::new(Box::new(NonEmbeddingChecker), None);
    // Before the fix this panics with debug_assert! ("id format drift?").
    // After the fix it must return normally.
    let result = engine.check(&compiled);

    // The engine must surface exactly one ConstraintCheckEntry, with the
    // Violated satisfaction reported by NonEmbeddingChecker and the engine-
    // attached friendly label — even though the checker's diagnostic message
    // never embedded the raw ConstraintNodeId.
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
        "expected Violated (NonEmbeddingChecker always reports Violated), got: {:?}",
        entry.satisfaction
    );
    assert_eq!(
        entry.label,
        Some("MinWall#0[0]".to_string()),
        "expected engine-attached friendly label Some(\"MinWall#0[0]\") \
         even though NonEmbeddingChecker's message omits the raw ConstraintNodeId, \
         got: {:?}",
        entry.label
    );

    let errors = error_diags(&result.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Error diagnostic, got: {:?}",
        errors
    );
    assert_eq!(
        errors[0].message,
        "wall thickness below minimum",
        "expected domain message to pass through verbatim, got: {:?}",
        errors[0].message
    );
}

/// The drift branch in `labeled_diagnostics` must emit a `tracing::debug!`
/// event — exactly once per labeled constraint whose Error-severity message
/// omits the raw `ConstraintNodeId` — so that first-party Display-drift is
/// observable via debug logging without penalising third-party checkers at
/// WARN level.
///
/// Uses `CountingSubscriberBuilder` with `Level::DEBUG` and a `target_prefix`
/// of `"reify_eval::engine_constraints"` — the module path of `labeled_diagnostics`,
/// where the drift signal is emitted — so that only events from that specific module
/// are counted, pinning the assertion to the drift branch and avoiding interference
/// from debug instrumentation elsewhere in the crate or its transitive dependencies.
#[test]
fn drift_signal_fires_for_non_embedding_checker() {
    use std::sync::atomic::Ordering;
    use reify_test_support::CountingSubscriberBuilder;

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_eval::engine_constraints")
        .build();
    // Clone the Arc before moving `counters` into the closure so we can read
    // the count after the subscriber is dropped.
    let debug_arc = counters[&tracing::Level::DEBUG].clone();

    let compiled = parse_and_compile(MIN_WALL_SOURCE);
    let mut engine = reify_eval::Engine::new(Box::new(NonEmbeddingChecker), None);

    tracing::subscriber::with_default(subscriber, || {
        let _ = engine.check(&compiled);
    });

    let count = debug_arc.load(Ordering::Acquire);
    assert_eq!(
        count,
        1,
        "expected exactly one DEBUG drift signal for a single labeled constraint \
         whose Error message omits the raw ConstraintNodeId; got {count}"
    );
}
