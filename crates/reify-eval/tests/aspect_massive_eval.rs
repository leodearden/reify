//! Runtime evaluation tests for the two-aspect (cost + mass) aggregation
//! idiom (task 5019).
//!
//! Compile-side structural assertions live in
//! `crates/reify-compiler/tests/aspect_massive_tests.rs`; this binary locks
//! the runtime end-to-end behaviour: a `Bracket` conforming to BOTH
//! `Costed` and `Massive` aggregates BOTH aspects — `line_cost` (Money) and
//! `mass` (Mass) — via `[ ... ].sum` into `total_cost` / `total_mass` on the
//! assembly.
//!
//! Test-function names contain `aspect_massive` so the
//! `cargo test -p reify-eval -- aspect_massive` filter picks them up.
//!
//! Uses `parse_and_compile_with_stdlib` + `make_simple_engine` (the pattern
//! from `crates/reify-eval/tests/cost_aggregation_eval.rs`) because the
//! source depends on stdlib-defined `Costed`, `Massive`, `USD`, `kg`, and `h`.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Absolute path to the canonical two-aspect aggregation example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from
/// `crates/reify-eval/tests/cost_aggregation_eval.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/aspect_massive.ri"
);

// ─── .sum dimension-preservation probe over Mass literals ───────────────────

/// Minimal probe: `.sum` over a `List<Mass>` literal evaluates to a
/// `Scalar<MASS>` whose si_value is the arithmetic sum. Complements the
/// example-file test below by isolating the `.sum` aggregation primitive on
/// the Mass dimension (no trait conformance, no sub-component member
/// access) — so a regression in `.sum` dimension-preservation for a
/// non-Money dimension fails here rather than as a downstream miscalculation
/// in the example test. Modeled on
/// `cost_aggregation_eval_sum_over_money_list_literal_preserves_dim`.
#[test]
fn aspect_massive_eval_sum_over_mass_list_literal_preserves_dim() {
    let source = r#"
structure def MassSumProbe {
    let total : Mass = [0.4kg, 0.4kg].sum
}
"#;

    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected zero Error diagnostics from eval, got: {:#?}",
        eval_errors
    );

    let id = ValueCellId::new("MassSumProbe", "total");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "MassSumProbe.total not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - 0.8).abs() < 1e-9,
                "expected total si_value 0.8 (0.4kg + 0.4kg), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "expected MASS dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── step-5: examples/aspect_massive.ri evaluates total_cost/total_mass ─────

/// The canonical example file's `AssemblyBracket.total_cost` and
/// `AssemblyBracket.total_mass` must evaluate to the expected two-aspect
/// sums: `3USD + 3USD = 6 USD` and `0.4kg + 0.4kg = 0.8 kg`.
///
/// Locks both that the example file's literal cost/mass values produce the
/// right per-part totals and that `[...].sum` correctly aggregates BOTH
/// aspects independently into their respective dimensioned totals.
///
/// USD and kg both have factor 1.0 and no offset (units.ri), so the SI
/// values are the raw 6.0 / 0.8.
#[test]
fn aspect_massive_example_evaluates_total_cost_and_total_mass_to_expected_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("failed to read examples/aspect_massive.ri: {}", e));

    let compiled = parse_and_compile_with_stdlib(&source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected zero Error diagnostics from evaluating aspect_massive.ri, got: {:#?}",
        eval_errors
    );

    let total_cost_id = ValueCellId::new("AssemblyBracket", "total_cost");
    let total_cost_val = result.values.get(&total_cost_id).unwrap_or_else(|| {
        panic!(
            "AssemblyBracket.total_cost not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    match total_cost_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - 6.0).abs() < 1e-9,
                "expected total_cost si_value 6.0 (Bracket 3USD * 2), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MONEY,
                "expected MONEY dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }

    let total_mass_id = ValueCellId::new("AssemblyBracket", "total_mass");
    let total_mass_val = result.values.get(&total_mass_id).unwrap_or_else(|| {
        panic!(
            "AssemblyBracket.total_mass not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    match total_mass_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - 0.8).abs() < 1e-9,
                "expected total_mass si_value 0.8 (Bracket 0.4kg * 2), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "expected MASS dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}
