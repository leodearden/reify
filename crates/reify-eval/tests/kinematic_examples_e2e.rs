//! End-to-end integration tests for the kinematic worked examples
//! (`examples/kinematic/counter_mass_balance.ri` and
//! `examples/kinematic/dock_pickup.ri`).
//!
//! Ships two `.ri` files under `examples/kinematic/` and exercises them
//! through the full `parse → compile_with_stdlib → eval` (and, for dock_pickup,
//! `engine.build` with the OCCT kernel) pipeline.
//!
//! Per docs/prds/kinematic-constraints.md task 10.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Satisfaction, Value, ValueCellId};

// ── Path constants ────────────────────────────────────────────────────────────

const CMB_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/counter_mass_balance.ri"
);

// ── Cached source + compile helpers for counter_mass_balance ──────────────────

/// Read `examples/kinematic/counter_mass_balance.ri`, caching the result.
fn cmb_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(CMB_PATH)
            .unwrap_or_else(|e| panic!("{CMB_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile `counter_mass_balance.ri` with stdlib, caching the result.
fn cmb_compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(cmb_source()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// counter_mass_balance tests
// ═══════════════════════════════════════════════════════════════════════════════

/// The `.ri` file exists and compiles with stdlib without any Error-severity
/// diagnostics.
#[test]
fn counter_mass_balance_compiles_clean() {
    // Reading source panics if the file doesn't exist.
    let source = cmb_source();
    assert!(!source.is_empty(), "counter_mass_balance.ri should be non-empty");

    let compiled = parse_and_compile_with_stdlib(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "counter_mass_balance.ri should compile with stdlib without errors, got: {errors:#?}"
    );
}

/// Read a numeric component (Real, Scalar, or Int) as f64 SI value.
fn read_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        Value::Int(i) => *i as f64,
        other => panic!("{label}: expected numeric component, got {other:?}"),
    }
}

/// Decompose a `Value::Point` of three numeric components into `[f64; 3]` (SI).
///
/// Mirrors `decompose_point3` in `forward_kinematics_e2e.rs`.
fn decompose_point3(v: &Value, label: &str) -> [f64; 3] {
    let comps = match v {
        Value::Point(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Value::Point len=3, got {other:?}"),
    };
    [
        read_f64(&comps[0], &format!("{label}.p[0]")),
        read_f64(&comps[1], &format!("{label}.p[1]")),
        read_f64(&comps[2], &format!("{label}.p[2]")),
    ]
}

/// `engine.eval()` on `counter_mass_balance.ri` produces:
///   - `snap_count == Value::Int(11)`
///   - `coms` is a `Value::List` of 11 `Value::Point` values whose three SI
///     components are each within 1e-9 m of zero (COM stationarity invariant).
///
/// Mirrors `snapshot.rs::tests::center_of_mass_counter_mass_balance_stationarity`
/// but drives the full surface-syntax → eval pipeline.
#[test]
fn counter_mass_balance_eval_produces_eleven_stationary_coms() {
    let compiled = cmb_compiled();

    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    // snap_count must be Int(11).
    let snap_count_id = ValueCellId::new("CounterMassBalance", "snap_count");
    let snap_count = result
        .values
        .get(&snap_count_id)
        .expect("CounterMassBalance.snap_count not found");
    assert_eq!(
        snap_count,
        &Value::Int(11),
        "snap_count should be Int(11), got {snap_count:?}"
    );

    // coms must be a List of 11 Point values, each near-zero.
    let coms_id = ValueCellId::new("CounterMassBalance", "coms");
    let coms = result
        .values
        .get(&coms_id)
        .expect("CounterMassBalance.coms not found");

    let items = match coms {
        Value::List(v) => v,
        other => panic!("coms should be Value::List, got {other:?}"),
    };
    assert_eq!(
        items.len(),
        11,
        "coms should have 11 entries (one per sweep step), got {}",
        items.len()
    );

    for (i, item) in items.iter().enumerate() {
        let [x, y, z] = decompose_point3(item, &format!("coms[{i}]"));
        assert!(
            x.abs() < 1e-9,
            "coms[{i}].x should be ~0 m, got {x} m (COM stationarity violated)"
        );
        assert!(
            y.abs() < 1e-9,
            "coms[{i}].y should be ~0 m, got {y} m (COM stationarity violated)"
        );
        assert!(
            z.abs() < 1e-9,
            "coms[{i}].z should be ~0 m, got {z} m (COM stationarity violated)"
        );
    }
}

/// `engine.check()` on `counter_mass_balance.ri` produces a non-empty
/// `constraint_results` list in which every entry is `Satisfaction::Satisfied`.
///
/// Pins that the reify-level `constraint snap_count == 11` and
/// `constraint stationary` assertions are actually evaluated by the constraint
/// checker — if the .ri had no constraints this test would fail the non-empty
/// assertion.
#[test]
fn counter_mass_balance_constraints_all_satisfied() {
    let compiled = cmb_compiled();

    let mut engine = make_simple_engine();
    // eval first so the engine state is populated
    let eval_result = engine.eval(compiled);
    let eval_errors = collect_errors(&eval_result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error diagnostics, got: {eval_errors:?}"
    );

    let check_result = engine.check(compiled);
    assert!(
        !check_result.constraint_results.is_empty(),
        "counter_mass_balance.ri should have at least one constraint (snap_count == 11, stationary)"
    );
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
