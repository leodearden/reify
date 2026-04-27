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
