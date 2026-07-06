//! Runtime evaluation tests for the `cost(collection)` subtree-cost
//! aggregation builtin (task 5015, M-WHOLE γ).
//!
//! Compile-side structural assertions live in
//! `crates/reify-compiler/tests/cost_subtree_aggregate_compile_tests.rs`;
//! this binary locks the runtime end-to-end behaviour: `cost(self.descendants)`
//! sums the `line_cost` of every `Costed`-conforming descendant into a
//! `Scalar<MONEY>` total, via the `apply_cost_aggregation` pre-eval rewrite
//! pass in `reify-eval/src/structural_query.rs` (mirrors the landed
//! `filter(_,Trait)` mechanism, task 3991).
//!
//! Uses `parse_and_compile_with_stdlib` + `make_simple_engine` (the
//! `cost_aggregation_eval.rs` pattern) because fixtures need stdlib-defined
//! `Costed`, `USD`, and `Money`.
//!
//! Step numbering mirrors plan.json step IDs.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// `CapScrew` definition reused verbatim from `examples/cost_aggregation.ri`
/// (values locked by `cost_aggregation_eval.rs`): 0.12USD * 24 = 2.88 USD.
const CAP_SCREW_DEF: &str = r#"
structure def CapScrew : Costed {
    param supplier          : String = "McMaster-Carr"
    param part_number       : String = "91251A190"
    param unit_cost         : Money  = 0.12USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = 24.0
}
"#;

/// `MotorMount` definition reused verbatim from `examples/cost_aggregation.ri`:
/// 8.50USD * 4 = 34.00 USD.
const MOTOR_MOUNT_DEF: &str = r#"
structure def MotorMount : Costed {
    param supplier          : String = "Misumi"
    param part_number       : String = "BNVAS25-30"
    param unit_cost         : Money  = 8.50USD
    param lead_time         : Time   = 72h
    param quantity_produced : Real   = 4.0
}
"#;

/// Assert `val` is a `Value::Scalar` with the given `si_value` (within
/// floating-point tolerance) and `DimensionVector::MONEY` dimension.
#[track_caller]
fn assert_money_eq(val: &Value, expected: f64, context: &str) {
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - expected).abs() < 1e-9,
                "{context}: expected si_value {expected}, got {si_value}",
            );
            assert_eq!(
                *dimension,
                DimensionVector::MONEY,
                "{context}: expected MONEY dimension, got {:?}",
                dimension
            );
        }
        other => panic!(
            "{context}: expected Value::Scalar {{ .. }}, got {:?}",
            other
        ),
    }
}

// ─── step-3: headline — two DIRECT Costed subs sum to 36.88 USD ────────────

/// `cost(self.descendants)` over a `Rig` with two direct Costed subs
/// (`bolts`: CapScrew, `mounts`: MotorMount) sums their `line_cost`s:
/// 2.88 + 34.00 = 36.88 USD.
///
/// RED today: `cost(...)` is a resolved-but-unreduced `FunctionCall` (no
/// "cost" rewrite in reify-eval) — the generic evaluator returns Undef for
/// an unrecognized function name.
#[test]
fn cost_subtree_aggregate_sums_direct_costed_subs() {
    let source = format!(
        r#"
{CAP_SCREW_DEF}
{MOTOR_MOUNT_DEF}
structure Rig {{
    sub bolts = CapScrew()
    sub mounts = MotorMount()
    let total : Money = cost(self.descendants)
}}
"#
    );

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
        "expected zero Error diagnostics, got: {:#?}",
        eval_errors
    );

    let id = ValueCellId::new("Rig", "total");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "Rig.total not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    assert_money_eq(val, 36.88, "Rig.total");
}

// ─── step-5: Costed filter + empty-list typed-zero (RED) ───────────────────

/// MIXED tree: `Mix` has a direct Costed sub (`bolts`: CapScrew) alongside a
/// non-Costed structural sub (`grp`: Group) whose own child (`Group.gizmo`:
/// Plain) is also non-Costed. `cost(self.descendants)` must sum ONLY the
/// Costed CapScrew (2.88 USD) — Group and Group.gizmo are excluded, so their
/// absent `line_cost` cells never poison the sum with Undef.
///
/// RED after step-4: step-4 emits a `ValueRef(line_cost)` for EVERY
/// descendant (no Costed filter yet), so the non-Costed refs resolve Undef
/// and `.sum` poisons the whole result to Undef.
#[test]
fn cost_subtree_aggregate_excludes_non_costed_descendants() {
    let source = format!(
        r#"
{CAP_SCREW_DEF}
structure Plain {{}}
structure Group {{
    sub gizmo = Plain()
}}
structure Mix {{
    sub bolts = CapScrew()
    sub grp = Group()
    let total : Money = cost(self.descendants)
}}
"#
    );

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
        "expected zero Error diagnostics, got: {:#?}",
        eval_errors
    );

    let id = ValueCellId::new("Mix", "total");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "Mix.total not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    assert_money_eq(val, 2.88, "Mix.total");
}

/// EMPTY case: `NoCost` has only a non-Costed sub (`a`: Plain).
/// `cost(self.descendants)` must yield a Money-dimensioned zero
/// (`Scalar{0.0, MONEY}`), NOT Undef and NOT a dimensionless `Real(0)`.
///
/// RED after step-4: with no Costed filter, the sole descendant's
/// `ValueRef(line_cost)` still resolves Undef (Plain has no `line_cost`
/// cell), so `.sum` yields Undef rather than reaching the empty-list branch.
#[test]
fn cost_subtree_aggregate_empty_is_money_typed_zero() {
    let source = r#"
        structure Plain {}
        structure NoCost {
            sub a = Plain()
            let total : Money = cost(self.descendants)
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
        "expected zero Error diagnostics, got: {:#?}",
        eval_errors
    );

    let id = ValueCellId::new("NoCost", "total");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "NoCost.total not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    assert_money_eq(val, 0.0, "NoCost.total");
}

// ─── step-7: end-to-end example fixture (RED until step-8 creates the file) ─

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/cost_subtree_aggregate.ri"
);

/// Reads `examples/cost_subtree_aggregate.ri`, compiles, and evaluates it.
///
/// The example exercises ALL depth-1 paths in one model: direct Costed subs
/// (`bolt`: CapScrew, `mount`: MotorMount), a top-level `List<CapScrew>`
/// collection (`screws`, count fixed to 2 by a constraint), and a non-Costed
/// structural node (`bracket`: Bracket) that must be excluded.
///
/// Hand-derived total: bolt 2.88 + mount 34.00 + screws[0..1] 2*2.88=5.76
/// = 42.64 USD (bracket excluded).
///
/// Also asserts the EXPLICIT-FILTER passthrough form
/// `cost(filter(self.descendants, Costed))` equals the bare
/// `cost(self.descendants)` total — confirming composition with
/// `apply_trait_filters` and that the double Costed filter is idempotent.
///
/// RED today: `examples/cost_subtree_aggregate.ri` does not exist —
/// `read_to_string` fails.
#[test]
fn example_cost_subtree_aggregate_ri_evals_expected_total() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).unwrap_or_else(|e| {
        panic!(
            "failed to read examples/cost_subtree_aggregate.ri (created by step-8): {}",
            e
        )
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
        "expected zero Error diagnostics evaluating cost_subtree_aggregate.ri, got: {:#?}",
        eval_errors
    );

    let subtree_cost_id = ValueCellId::new("Subassembly", "subtree_cost");
    let subtree_cost = result.values.get(&subtree_cost_id).unwrap_or_else(|| {
        panic!(
            "Subassembly.subtree_cost not found in eval result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });
    assert_money_eq(subtree_cost, 42.64, "Subassembly.subtree_cost");

    let subtree_cost_filtered_id = ValueCellId::new("Subassembly", "subtree_cost_filtered");
    let subtree_cost_filtered = result
        .values
        .get(&subtree_cost_filtered_id)
        .unwrap_or_else(|| {
            panic!(
                "Subassembly.subtree_cost_filtered not found in eval result; available cells: {:?}",
                result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
            )
        });
    assert_money_eq(
        subtree_cost_filtered,
        42.64,
        "Subassembly.subtree_cost_filtered",
    );

    assert_eq!(
        subtree_cost, subtree_cost_filtered,
        "cost(self.descendants) and cost(filter(self.descendants, Costed)) \
         should be equal (idempotent double Costed filter)"
    );
}
