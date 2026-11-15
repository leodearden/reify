//! Constraint-verdict pins for a dimensioned param default (task #5763 — PRD
//! `docs/prds/v0_6/dimensioned-construction-strictness.md` §10.3 / §11 ε).
//!
//! A constraint over a dimensioned param default reaches a DEFINITE verdict:
//! `Satisfied` (exit 0) for a positive force, `Violated` (exit 1) for a
//! negative one. The bare-literal input class that once left the same
//! constraint permanently `Indeterminate` with exit 0 is rejected at the
//! declaration since #5646, so it can no longer reach the checker.
//!
//! ## Why the fixture is a local `structure def` valued through its default
//!
//! `StepForceShape` mirrors stdlib `StepForce`'s `param magnitude : Force` and
//! `constraint magnitude > 0N`, but it is declared in the module under check
//! and valued through its own param DEFAULT. Constructor args never reach a
//! structure-def constraint (the template's own default decides) and
//! prelude/stdlib template constraints are never evaluated — owned by #5765 —
//! so `StepForce(magnitude: …)`, or a `let p = …(magnitude: …)` ctor form,
//! would make every pin here vacuous. Do not "fix" the fixture back to either.
//!
//! ## Residuals, by owner
//!
//! - R1: an `Undef` with every leaf defined still maps to an `Indeterminate`
//!   WARNING (exit 0) — owned by #10027.
//! - R2: `==` gives a definite `false` where `>` gives `Indeterminate` on the
//!   same incomparable operands — owned by #10027.
//! - R3: ctor args and prelude template constraints never reach the checker —
//!   owned by #5765.
//!
//! The engine is driven the way `reify check` drives it (stdlib compile,
//! `SolverPosture::Solve(ResolutionProfile::FRESH)`, `SimpleConstraintChecker`,
//! `Engine::check`), and its exit code is read off what that boundary exposes:
//! a compile Error or a `Violated` verdict fails, `Satisfied` with no Error
//! diagnostic passes. Indeterminacy is read off `StepForceShape#constraint[0]`'s
//! own entry, never module-wide: an Indeterminate warning elsewhere does not
//! move the exit code, and prelude constraints may start reporting one once
//! #5765 lands.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ConstraintNodeId, DiagnosticCode};
use reify_eval::{CheckResult, ConstraintCheckEntry, Engine, SolverPosture};
use reify_ir::{ResolutionProfile, Satisfaction};
use reify_test_support::{
    assert_error_code_present, assert_no_error_diagnostics, collect_errors,
    compile_source_with_stdlib, parse_and_compile_with_stdlib,
};

fn step_force_shape_source(magnitude_default: &str) -> String {
    format!(
        "structure def StepForceShape {{\n    param magnitude : Force = {magnitude_default}\n    constraint magnitude > 0N\n}}"
    )
}

fn check_step_force_shape(magnitude_default: &str) -> (CheckResult, ConstraintCheckEntry) {
    let compiled = parse_and_compile_with_stdlib(&step_force_shape_source(magnitude_default));
    let mut engine = Engine::new(
        SolverPosture::Solve(ResolutionProfile::FRESH),
        Box::new(SimpleConstraintChecker),
        None,
    );
    let result = engine.check(&compiled);
    let wanted = ConstraintNodeId::new("StepForceShape", 0);
    let entry = result
        .constraint_results
        .iter()
        .find(|entry| entry.id == wanted)
        .cloned()
        .unwrap_or_else(|| {
            let ids: Vec<_> = result.constraint_results.iter().map(|e| &e.id).collect();
            panic!("no verdict for {wanted:?}; checked constraints: {ids:?}")
        });
    (result, entry)
}

#[test]
fn positive_force_default_is_definitely_satisfied() {
    let (result, entry) = check_step_force_shape("10N");

    assert_eq!(
        (entry.satisfaction, &entry.indeterminate_reason),
        (Satisfaction::Satisfied, &None),
        "a positive force default must be definitely Satisfied; diagnostics: {:?}",
        result.diagnostics
    );
    assert_no_error_diagnostics(
        &result.diagnostics,
        "a Satisfied check must carry no Error diagnostic (the exit-0 shape)",
    );
}

#[test]
fn negative_force_default_is_definitely_violated() {
    let (result, entry) = check_step_force_shape("-10N");

    assert_eq!(
        (entry.satisfaction, &entry.indeterminate_reason),
        (Satisfaction::Violated, &None),
        "a negative force default must be definitely Violated; diagnostics: {:?}",
        result.diagnostics
    );
    assert_error_code_present(
        &result.diagnostics,
        DiagnosticCode::ConstraintViolated,
        "a Violated check must carry a ConstraintViolated Error (the exit-1 shape)",
    );
}

/// Decompose-time before-image of this input: `INDETERMINATE … operator
/// undefined for these operand kinds: Real`, exit 0 (PRD §10.3); rejected at
/// the declaration since #5646, so `reify check` exits 1 before any constraint
/// is evaluated.
#[test]
fn bare_literal_force_default_is_rejected_at_compile_time() {
    for bare_default in ["-10.0", "10.0"] {
        let module = compile_source_with_stdlib(&step_force_shape_source(bare_default));
        let errors = collect_errors(&module.diagnostics);
        assert!(
            errors.iter().any(|d| {
                d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch)
                    && d.message.contains("'magnitude'")
            }),
            "a bare `{bare_default}` Force default must be a ParamDefaultTypeMismatch Error \
             naming 'magnitude'; errors: {errors:?}"
        );
    }
}

/// The §10.3 symmetry: a dimensionless field against a dimensioned threshold is
/// a hard compile error through the same comparison guard. This is the `param`
/// form; the compiler-layer `let` form is reify-compiler's
/// `dimensionless_ratio_gt_dimensioned_threshold_emits_dimension_mismatch`.
#[test]
fn dimensionless_param_against_length_threshold_is_rejected_at_compile_time() {
    let module = compile_source_with_stdlib(
        "structure def EfficiencyShape {\n    param efficiency : Real = 0.85\n    constraint efficiency > 5mm\n}",
    );
    assert_error_code_present(
        &module.diagnostics,
        DiagnosticCode::DimensionMismatch,
        "a Real param compared against a Length must be a DimensionMismatch Error",
    );
}
