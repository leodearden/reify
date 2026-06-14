//! End-to-end integration tests for FEA diagnostic mapping (task 2929).
//!
//! Tests the full `.ri` → parse_and_compile_with_stdlib → make_simple_engine
//! → register_compute_fns → engine.eval pipeline for each diagnostic fixture.
//!
//! Strategy (per plan):
//!   - Fixtures that are `.ri`-triggerable (no-loads, no-supports, thin-body)
//!     are tested e2e against the assembled eval harness.
//!   - Non-convergence and singular/degenerate are covered by solver-side
//!     classifier unit tests in reify-solver-elastic/src/diagnostics.rs and
//!     conversion unit tests in reify-eval/src/compute_targets/fea_diagnostics.rs;
//!     they cannot be driven to failure through the well-conditioned cantilever.
//!
//! Convention (matching solve_elastic_static_e2e.rs):
//!   - Never assert spans or labels.
//!   - Assert: severity + code + key message substring.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── step-5: RED — no-loads fixture ───────────────────────────────────────────
//
// Fails until step-6 wires the no-loads detection in the trampoline.
//
// (step-6 GREEN is done; this test now passes.)

/// No-loads fixture: a structure with a FixedSupport but zero loads.
///
/// Expects:
/// - No `Severity::Error` diagnostics.
/// - Exactly one `Severity::Warning` diagnostic with
///   `code == Some(DiagnosticCode::FeaNoLoads)` and message containing "No loads".
#[test]
fn no_loads_fixture_emits_fea_no_loads_warning() {
    let source = include_str!("fixtures/fea_no_loads.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from no-loads fixture, got: {:#?}",
        errors
    );

    // Exactly one FeaNoLoads warning with the expected message substring.
    let fea_no_loads: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FeaNoLoads)
        })
        .collect();
    assert_eq!(
        fea_no_loads.len(),
        1,
        "expected exactly one FeaNoLoads warning, got {}: {:#?}",
        fea_no_loads.len(),
        eval_result.diagnostics
    );
    assert!(
        fea_no_loads[0].message.contains("No loads"),
        "FeaNoLoads message must contain 'No loads', got: {}",
        fea_no_loads[0].message
    );
}

// ── step-7: RED — no-supports fixture ────────────────────────────────────────
//
// Fails until step-8 wires the under-constrained detection in the trampoline.

/// No-supports fixture: a structure with a PointLoad but an EMPTY supports list.
///
/// Expects:
/// - No `Severity::Error` diagnostics (the root face is auto-clamped by the
///   cantilever model so the solve still returns an ElasticResult).
/// - Exactly one `Severity::Warning` diagnostic with
///   `code == Some(DiagnosticCode::FeaUnderConstrained)` and message containing
///   "supports".
#[test]
fn no_supports_fixture_emits_fea_under_constrained_warning() {
    let source = include_str!("fixtures/fea_no_supports.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No error-severity diagnostics (the cantilever still solves via auto-clamp).
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from no-supports fixture, got: {:#?}",
        errors
    );

    // Exactly one FeaUnderConstrained warning with the expected message substring.
    let under_constrained: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FeaUnderConstrained)
        })
        .collect();
    assert_eq!(
        under_constrained.len(),
        1,
        "expected exactly one FeaUnderConstrained warning, got {}: {:#?}",
        under_constrained.len(),
        eval_result.diagnostics
    );
    assert!(
        under_constrained[0].message.contains("supports"),
        "FeaUnderConstrained message must contain 'supports', got: {}",
        under_constrained[0].message
    );
}

// ── step-9: RED — thin-body fixture ──────────────────────────────────────────
//
// Fails (thin_body_fixture test) until step-10 wires thin_body_advisory.
// The cantilever_smoke guard passes even before step-10 (no FeaThinBody emitted).

/// Thin-body fixture: plate with aspect ratio ≈ 100 (1000mm × 1000mm × 10mm).
///
/// Expects:
/// - Exactly one `Severity::Warning` diagnostic with
///   `code == Some(DiagnosticCode::FeaThinBody)` and message referencing
///   "element_order" (part of the actionable text from the triage table).
#[test]
fn thin_body_fixture_emits_fea_thin_body_warning() {
    let source = include_str!("fixtures/fea_thin_body.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let thin_body: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FeaThinBody)
        })
        .collect();
    assert_eq!(
        thin_body.len(),
        1,
        "expected exactly one FeaThinBody warning, got {}: {:#?}",
        thin_body.len(),
        eval_result.diagnostics
    );
    assert!(
        thin_body[0].message.contains("element_order"),
        "FeaThinBody message must reference 'element_order', got: {}",
        thin_body[0].message
    );
}

/// Standard cantilever (ratio = max_dim/min_dim = 1.0m/0.1m = 10, exactly at
/// threshold) must NOT trigger FeaThinBody — the advisory fires only when ratio
/// STRICTLY exceeds the threshold.
#[test]
fn cantilever_smoke_does_not_emit_fea_thin_body() {
    let source = include_str!("../../../examples/fea_cantilever_smoke.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let thin_body: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FeaThinBody))
        .collect();
    assert!(
        thin_body.is_empty(),
        "standard cantilever (ratio=10, at threshold) must not emit FeaThinBody, got: {:#?}",
        thin_body
    );
}
