//! Integration gate for task δ (#5020): coherent multi-aspect objective solves
//! and negative rejection, exercised via the SHIPPED
//! `examples/multi_aspect_objective.ri` (BT4 positive) and
//! `examples/multi_aspect_objective_mixed.ri` (BT1 negative).
//!
//! PRD: `docs/prds/v0_6/multi-aspect-objective-units-coherence.md`.
//!
//! # What is tested
//!
//! - POSITIVE (`multi_aspect_objective_positive_resolves_and_is_dimension_coherent`):
//!   `MultiAspectPart` combines a Money aspect (`unit_cost`) and a Mass aspect
//!   (`mass_per_mm`) into ONE normalised dimensionless `minimize` term. Compiles
//!   with no `ObjectiveDimensionIncoherent` diagnostic (a single-term objective
//!   never reaches the multi-term coherence guard) and evaluates with
//!   `thickness` resolving strictly off the `2mm` constraint boundary.
//! - NEGATIVE (`multi_aspect_objective_mixed_negative_emits_e_objective_mixed_dimension`):
//!   `MixedObjective` declares two same-sense `minimize` terms over incommensurable
//!   dimensions (Money, Mass), producing a `DiagnosticCode::ObjectiveDimensionIncoherent`
//!   Error naming both dimensions. This file intentionally fails `reify check` and
//!   is listed in `crates/reify-compiler/tests/examples_smoke.rs::SKIP_SET`.
//!
//! # Reuse
//!
//! Import set and compile→eval→assert skeleton mirror
//! `crates/reify-eval/tests/continuous_cost_min_example_e2e.rs`. The negative
//! assertion pattern mirrors
//! `crates/reify-compiler/tests/objective_dimension_coherence.rs::mixed_dimension_money_mass_emits_error`.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Path to the shipped NEGATIVE example, resolved relative to this crate's
/// manifest directory.
const NEGATIVE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/multi_aspect_objective_mixed.ri"
);

/// Integration gate (δ BT1 negative): `MixedObjective`'s two same-sense
/// mixed-dimension `minimize` decls (Money `cost`, Mass `mass`) lower to a
/// 2-term `WeightedSum` whose terms do not share one dimension, so
/// `reify check` must emit `DiagnosticCode::ObjectiveDimensionIncoherent` at
/// `Severity::Error` naming both dimensions.
#[test]
fn multi_aspect_objective_mixed_negative_emits_e_objective_mixed_dimension() {
    let src = std::fs::read_to_string(NEGATIVE_PATH).unwrap_or_else(|e| {
        panic!(
            "Could not read {}: {} — run the next impl step to create the example file",
            NEGATIVE_PATH, e
        )
    });

    let compiled = compile_source_with_stdlib(&src);

    let diag = compiled
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ObjectiveDimensionIncoherent))
        .unwrap_or_else(|| {
            panic!(
                "expected an ObjectiveDimensionIncoherent diagnostic, got: {:#?}",
                compiled.diagnostics
            )
        });

    assert_eq!(
        diag.severity,
        Severity::Error,
        "ObjectiveDimensionIncoherent must be Severity::Error, got {:?}",
        diag.severity
    );

    assert!(
        diag.message.contains("E_OBJECTIVE_MIXED_DIMENSION"),
        "message must contain \"E_OBJECTIVE_MIXED_DIMENSION\", got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Money"),
        "message must name the 'Money' dimension, got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Mass"),
        "message must name the 'Mass' dimension, got: {:?}",
        diag.message
    );
}

/// Path to the shipped POSITIVE example, resolved relative to this crate's
/// manifest directory.
const POSITIVE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/multi_aspect_objective.ri"
);

/// Integration gate (δ BT4 positive): `MultiAspectPart` combines a Money
/// aspect (`unit_cost`) and a Mass aspect (`mass_per_mm`) into ONE normalised
/// dimensionless `minimize` term. Both summands are dimensionless, so the
/// objective is a single-term `ObjectiveSet` that never reaches the
/// multi-term coherence guard — `reify check` must emit no
/// `ObjectiveDimensionIncoherent` diagnostic and no Error-severity
/// diagnostic at all. `thickness` (the scope's own `auto(free)` param) must
/// resolve to a finite, feasible `Length` strictly above the `2mm` constraint
/// boundary (0.002 m) — the drift-infeasible seed-fallback documented in
/// `examples/multi_aspect_objective.ri`.
#[test]
fn multi_aspect_objective_positive_resolves_and_is_dimension_coherent() {
    let src = std::fs::read_to_string(POSITIVE_PATH).unwrap_or_else(|e| {
        panic!(
            "Could not read {}: {} — run the next impl step to create the example file",
            POSITIVE_PATH, e
        )
    });

    // ── compile ────────────────────────────────────────────────────────────
    let compiled = compile_source_with_stdlib(&src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/multi_aspect_objective.ri should compile without errors: {:#?}",
        errors
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveDimensionIncoherent)),
        "a single normalised dimensionless objective term must not produce \
         ObjectiveDimensionIncoherent, got: {:#?}",
        compiled.diagnostics
    );

    // ── eval ───────────────────────────────────────────────────────────────
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    // ── FEASIBLE, OFF-BOUNDARY (structural, NOT precise-convergence) ───────
    let thickness_id = ValueCellId::new("MultiAspectPart", "thickness");
    let thickness_si = match result.values.get(&thickness_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for MultiAspectPart.thickness, got {:?}",
            other
        ),
    };
    assert!(
        thickness_si.is_finite(),
        "thickness must resolve to a finite value; got {:?}",
        thickness_si
    );
    // Sanity guard: strictly above the 2mm boundary (0.002 m) — feasible.
    // Mirrors continuous_cost_min_example_e2e.rs's rationale: the precise
    // seed-fallback value (~0.01 m) is a solver-internal detail, not the
    // user-observable signal. The signal here is "resolves feasibly with no
    // dimension error", not the exact converged value.
    assert!(
        thickness_si > 0.002,
        "thickness should be strictly above the 2mm boundary (0.002 m); got {:.6} m",
        thickness_si
    );

    // ── NO ObjectiveDimensionIncoherent at eval either ──────────────────────
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.code != Some(DiagnosticCode::ObjectiveDimensionIncoherent)),
        "eval must emit no ObjectiveDimensionIncoherent diagnostic; got: {:#?}",
        result.diagnostics,
    );

    // ── NO Error-severity eval diagnostics ───────────────────────────────────
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "must emit no Error-severity eval diagnostics; got: {:#?}",
        eval_errors,
    );
}
