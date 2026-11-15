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
//! diagnostic passes.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode};
use reify_eval::{CheckResult, ConstraintCheckEntry, Engine, SolverPosture};
use reify_ir::{ResolutionProfile, Satisfaction};
use reify_test_support::{collect_errors, parse_and_compile_with_stdlib};

fn step_force_shape_source(magnitude_default: &str) -> String {
    format!(
        "structure def StepForceShape {{\n    param magnitude : Force = {magnitude_default}\n    constraint magnitude > 0N\n}}"
    )
}

fn check_as_reify_check_does(source: &str) -> (CheckResult, ConstraintCheckEntry) {
    let compiled = parse_and_compile_with_stdlib(source);
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

fn carries_code(diagnostics: &[Diagnostic], code: DiagnosticCode) -> bool {
    diagnostics.iter().any(|d| d.code == Some(code))
}

fn has_error_with_code(diagnostics: &[Diagnostic], code: DiagnosticCode) -> bool {
    collect_errors(diagnostics)
        .iter()
        .any(|d| d.code == Some(code))
}

#[test]
fn positive_force_default_is_definitely_satisfied() {
    let (result, entry) = check_as_reify_check_does(&step_force_shape_source("10N"));

    assert_eq!(
        (entry.satisfaction, &entry.indeterminate_reason),
        (Satisfaction::Satisfied, &None),
        "a positive force default must be definitely Satisfied; diagnostics: {:?}",
        result.diagnostics
    );
    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "a Satisfied check must carry no Error diagnostic (the exit-0 shape); errors: {errors:?}"
    );
    assert!(
        !carries_code(&result.diagnostics, DiagnosticCode::ConstraintIndeterminate),
        "no constraint may come back Indeterminate; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn negative_force_default_is_definitely_violated() {
    let (result, entry) = check_as_reify_check_does(&step_force_shape_source("-10N"));

    assert_eq!(
        (entry.satisfaction, &entry.indeterminate_reason),
        (Satisfaction::Violated, &None),
        "a negative force default must be definitely Violated; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        has_error_with_code(&result.diagnostics, DiagnosticCode::ConstraintViolated),
        "a Violated check must carry a ConstraintViolated Error (the exit-1 shape); \
         diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        !carries_code(&result.diagnostics, DiagnosticCode::ConstraintIndeterminate),
        "no constraint may come back Indeterminate; diagnostics: {:?}",
        result.diagnostics
    );
}
