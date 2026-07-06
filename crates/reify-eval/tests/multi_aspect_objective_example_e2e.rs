//! Integration gate for task δ (#5020): coherent multi-aspect objective solves
//! + negative rejection, exercised via the SHIPPED `examples/multi_aspect_objective.ri`
//! (BT4 positive) and `examples/multi_aspect_objective_mixed.ri` (BT1 negative).
//!
//! PRD: `docs/prds/v0_6/multi-aspect-objective-units-coherence.md`.
//!
//! # What is tested
//!
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

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

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
