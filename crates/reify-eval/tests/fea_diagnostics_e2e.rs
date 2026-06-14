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
