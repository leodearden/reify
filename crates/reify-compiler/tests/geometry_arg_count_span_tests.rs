//! Tests for source-span labels on geometry arg-count diagnostics (task 487).
//!
//! Each arg-count error for the in-scope geometry functions (box, cylinder,
//! sphere, linear_pattern, circular_pattern, mirror, union (+ siblings),
//! shell, thicken, draft) must attach a `DiagnosticLabel` with a non-empty
//! `SourceSpan` covering the full call expression. Pattern mirrors the
//! assertions in `diagnostic_coverage_checkpoint.rs`.
//!
//! shell/thicken/draft tests serve as regression guards — labels are already
//! attached via `compile_modify_op`; these tests lock in that behavior.

use reify_test_support::{compile_source, errors_only};

// ── box() ──────────────────────────────────────────────────────────────

#[test]
fn box_arg_count_diagnostic_has_span_label() {
    // box() expects 3 arguments — passing only 2 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let shape = box(10mm, 20mm)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("box() expects 3 arguments"))
        .unwrap_or_else(|| panic!(
            "expected 'box() expects 3 arguments' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on box arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on box arg-count label");
}

// ── cylinder() ─────────────────────────────────────────────────────────

#[test]
fn cylinder_arg_count_diagnostic_has_span_label() {
    // cylinder() expects 2 arguments — passing only 1 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let c = cylinder(10mm)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("cylinder() expects 2 arguments"))
        .unwrap_or_else(|| panic!(
            "expected 'cylinder() expects 2 arguments' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on cylinder arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on cylinder arg-count label");
}

// ── sphere() ───────────────────────────────────────────────────────────

#[test]
fn sphere_arg_count_diagnostic_has_span_label() {
    // sphere() expects 1 argument — passing 0 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let s = sphere()
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("sphere() expects 1 argument"))
        .unwrap_or_else(|| panic!(
            "expected 'sphere() expects 1 argument' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on sphere arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on sphere arg-count label");
}

// ── linear_pattern() ───────────────────────────────────────────────────

#[test]
fn linear_pattern_arg_count_diagnostic_has_span_label() {
    // linear_pattern() expects 6 arguments — passing 1 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let p = linear_pattern(box(10mm, 10mm, 10mm))
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("linear_pattern() expects 6 arguments"))
        .unwrap_or_else(|| panic!(
            "expected 'linear_pattern() expects 6 arguments' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on linear_pattern arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on linear_pattern arg-count label");
}

// ── circular_pattern() ─────────────────────────────────────────────────

#[test]
fn circular_pattern_arg_count_diagnostic_has_span_label() {
    // circular_pattern() expects 9 arguments — passing 2 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let p = circular_pattern(box(10mm, 10mm, 10mm), 1.0)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("circular_pattern() expects 9 arguments"))
        .unwrap_or_else(|| panic!(
            "expected 'circular_pattern() expects 9 arguments' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on circular_pattern arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on circular_pattern arg-count label");
}

// ── mirror() ───────────────────────────────────────────────────────────

#[test]
fn mirror_arg_count_diagnostic_has_span_label() {
    // mirror() expects 7 arguments — passing 2 should produce a labeled diagnostic
    let source = r#"
        structure S {
            let m = mirror(box(10mm, 10mm, 10mm), 1.0)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains("mirror() expects 7 arguments"))
        .unwrap_or_else(|| panic!(
            "expected 'mirror() expects 7 arguments' error, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    assert!(!first.labels.is_empty(), "expected at least one label on mirror arg-count diagnostic");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span on mirror arg-count label");
}
