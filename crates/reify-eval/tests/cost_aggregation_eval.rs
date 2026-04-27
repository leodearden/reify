//! Runtime evaluation tests for the cost-aggregation stdlib idiom (task 2381).
//!
//! Compile-side structural assertions live in
//! `crates/reify-compiler/tests/cost_aggregation_tests.rs`; this binary locks
//! the runtime end-to-end behaviour: two `Costed`-conforming structures'
//! `line_cost`s aggregate via `[ ... ].sum` into a `Scalar<MONEY>` total on
//! the assembly.
//!
//! Test-function names contain `cost_aggregation` so the
//! `cargo test -p reify-eval -- cost_aggregation` filter (per this task's
//! testStrategy) picks them up.
//!
//! Uses `parse_and_compile_with_stdlib` + `make_simple_engine` (the
//! pattern from `crates/reify-eval/tests/purpose_activation.rs:560+`)
//! because the source depends on stdlib-defined `Costed`, `USD`, and `h`.

use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{DimensionVector, Severity, Value, ValueCellId};

/// Absolute path to the canonical cost-aggregation example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from
/// `crates/reify-eval/tests/stress_large_assembly.rs:21–25`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/cost_aggregation.ri"
);

// ─── step-7: two Costed structures aggregate via .sum ────────────────────────

/// Two structures conforming to `Costed` (Bolt @ 10USD * 3 = 30USD,
/// Motor @ 5USD * 4 = 20USD) must aggregate via `[ ... ].sum` into a
/// `Scalar<MONEY>` total of `50.0` on the assembly's `total_cost` cell.
///
/// Locks the runtime contract that the trait-let `line_cost` cell is
/// reachable through `self.<sub>.line_cost` member access (the same
/// pattern as `examples/large_assembly.ri:252+` for `self.b01.mass`) and
/// that `.sum` over `List<Scalar<MONEY>>` preserves the MONEY dimension.
#[test]
fn cost_aggregation_eval_two_costed_structures_aggregate_via_sum() {
    let source = r#"
structure def Bolt : Costed {
    param supplier         : String = "Acme"
    param part_number      : String = "B-001"
    param unit_cost        : Money  = 10USD
    param lead_time        : Time   = 24h
    param quantity_produced : Real  = 3.0
}

structure def Motor : Costed {
    param supplier         : String = "Acme"
    param part_number      : String = "M-001"
    param unit_cost        : Money  = 5USD
    param lead_time        : Time   = 48h
    param quantity_produced : Real  = 4.0
}

structure def Assembly {
    sub b = Bolt()
    sub m = Motor()
    let total_cost : Money = [self.b.line_cost, self.m.line_cost].sum
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

    let id = ValueCellId::new("Assembly", "total_cost");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "Assembly.total_cost not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 50.0).abs() < 1e-9,
                "expected total_cost si_value 50.0 (Bolt 10*3 + Motor 5*4), got {}",
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
}

// ─── step-11: examples/cost_aggregation.ri evaluates total_cost = 36.88 USD ──

/// The canonical example file's `AssemblyBOM.total_cost` must evaluate to
/// the expected money sum: `0.12USD * 24 + 8.50USD * 4 = 2.88 + 34.00 = 36.88`.
///
/// Locks both that the example file's literal cost+quantity values produce
/// the right line-level totals (CapScrew 2.88, MotorMount 34.00) and that
/// `[...].sum` correctly aggregates them into the Money-dimensioned total.
///
/// USD has factor 1.0 and offset 0.0 in `units.ri:74`, so the SI value is
/// the raw 36.88.
///
/// RED until the example file is in place AND the trait-let `line_cost`
/// cell threads through the eval graph correctly via `self.<sub>.line_cost`
/// member access.
#[test]
fn cost_aggregation_example_evaluates_total_cost_to_expected_money_value() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).unwrap_or_else(|e| {
        panic!("failed to read examples/cost_aggregation.ri: {}", e)
    });

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
        "expected zero Error diagnostics from evaluating cost_aggregation.ri, got: {:#?}",
        eval_errors
    );

    let id = ValueCellId::new("AssemblyBOM", "total_cost");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "AssemblyBOM.total_cost not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 36.88).abs() < 1e-9,
                "expected total_cost si_value 36.88 (CapScrew 0.12*24 + MotorMount 8.50*4), \
                 got {}",
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
}
