//! Shared scaffolding for the two task #5467 (PRD2 α) end-to-end modules —
//! `let_tracing_transitive_e2e` and `instance_path_underdetermined_e2e`.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]`, like every
//! other module in this binary — see the anti-re-accretion rationale there.
//!
//! # Why this exists (review suggestion 5)
//!
//! Both e2e modules landed in the same change, both are `#[path]`-registered
//! into the SAME test binary, and both carried byte-identical copies of these
//! three helpers — roughly seventy lines of copy-paste inside ONE compilation
//! unit, not across a crate boundary. Two forks of one file drift: a fix to the
//! production-registry wiring, or to what counts as a `Severity::Error`, would
//! have to be made twice with nothing to catch the second miss.
//!
//! # What deliberately did NOT move here
//!
//! Each module keeps its own `SOLVER_TOL`. The two constants differ (1e-9 vs
//! 1e-6) because each is DERIVED from its own fixture's cost surface and
//! documents that derivation in situ; merging them would either pick one
//! arbitrarily or force a shared constant to carry two unrelated arguments.
//! Fixture sources and the assertions over them likewise stay with the module
//! whose signal they pin.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Compile + eval `src` through the REAL `SolverRegistry::production()`, and
/// assert the eval produced zero `Severity::Error` diagnostics.
///
/// LOAD-BEARING — the solver MUST be `SolverRegistry::production()`, never a
/// bare `DimensionalSolver`. `decompose_into_components` is reached ONLY from
/// `SolverRegistry::solve_inner`; a bare `DimensionalSolver` never decomposes,
/// receives every let-indirected constraint through the layer-1 filter, and
/// folds the dependent cells through `build_trial_values` — so it would resolve
/// these fixtures even with the decomposition half of α reverted, and every RED
/// written against it would be vacuous. `reify-constraints` is a normal
/// (non-dev) dependency of `reify-eval`, so `production()` is legal here.
///
/// The error assertion is load-bearing for B1 in its own right: a non-converged
/// solve surfaces as a `constraints could not be satisfied` error, and a
/// caller's value assertions must not be able to pass while the solve failed.
pub fn eval_through_production_registry(src: &str, what: &str) -> EvalResult {
    let compiled = compile_source_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the {what} source must compile without errors; got {errors:#?}",
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(reify_constraints::SolverRegistry::production()));
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "the {what} eval must emit no Severity::Error diagnostics — a \
         non-converged solve surfaces here, and the value assertions must not \
         be able to pass while the solve failed; got {eval_errors:#?}",
    );

    result
}

/// The SI magnitude of a resolved `Scalar` cell, or a panic naming what was
/// actually there.
///
/// An UNRESOLVED auto surfaces as `Value::Undef`, which is precisely the silent
/// failure both calling modules exist to report legibly rather than let a
/// zero-diagnostic count wave through. The panic names both ways an α auto
/// reaches this state, because a caller cannot tell them apart from the value
/// alone:
///
///   * LAYER 1 dropped the constraint that pins it, so it entered the
///     `ResolutionProblem` with no residual at all; or
///   * the decomposition returned no component holding it.
///
/// The `what` label is the caller's, and is where fixture-specific context
/// belongs — see each module's header for the failure it is pinning.
pub fn scalar_si(result: &EvalResult, id: &ValueCellId, what: &str) -> f64 {
    match result.values.get(id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected a resolved Scalar for {id:?} in the {what} eval; got \
             {other:?}. `Undef` means the auto was never solved: either layer \
             1 dropped the constraint that pins it (so it reached the solver \
             with no residual), or the decomposition returned no component for \
             it. Whether `W_UNDERDETERMINED` also fires depends on the SHAPE, \
             so do not read a quiet eval as a healthy one: layer 4 suppresses \
             the warning once its own probes see the auto as pinned, which for \
             a PARENT-side `let` the forward closure alone already does. That \
             is the silent-`Undef` case. For a CHILD-side `let` the closure \
             does not, and layer 4 stays loud. The value assertion is the only \
             signal that holds in both",
        ),
    }
}

/// Every `Underdetermined`-coded diagnostic on this eval, matched by CODE
/// rather than by substring on the rendered `W_UNDERDETERMINED` text.
pub fn underdetermined(result: &EvalResult) -> Vec<&reify_core::Diagnostic> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect()
}
