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
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_test_support::{
    collect_errors, decompose_point3, make_simple_engine, parse_and_compile_with_stdlib, read_f64,
};
use reify_types::{ExportFormat, Satisfaction, Value, ValueCellId};

// ── Path constants ────────────────────────────────────────────────────────────

const CMB_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/counter_mass_balance.ri"
);

const DP_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/dock_pickup.ri"
);

// ── Cached source + compile helpers for counter_mass_balance ──────────────────

/// Read `examples/kinematic/counter_mass_balance.ri`, caching the result.
fn cmb_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(CMB_PATH).unwrap_or_else(|e| panic!("{CMB_PATH} should exist: {e}"))
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

/// The `.ri` file exists, is non-empty, and compiles with stdlib without any
/// Error-severity diagnostics.
///
/// Compilation errors would panic inside `cmb_compiled()` →
/// `parse_and_compile_with_stdlib`; this test also guards the non-empty file
/// check explicitly so a missing/empty file produces a readable failure.
#[test]
fn counter_mass_balance_compiles_clean() {
    let source = cmb_source();
    assert!(
        !source.is_empty(),
        "counter_mass_balance.ri should be non-empty"
    );
    // Compile (or return the cached module); panics on any Error diagnostic.
    let _ = cmb_compiled();
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
///
/// Note: `Engine::check` internally calls `eval` first, so no separate
/// `engine.eval()` pre-call is needed.
#[test]
fn counter_mass_balance_constraints_all_satisfied() {
    let compiled = cmb_compiled();

    let mut engine = make_simple_engine();
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

// ═══════════════════════════════════════════════════════════════════════════════
// dock_pickup source cache
// ═══════════════════════════════════════════════════════════════════════════════

/// Read `examples/kinematic/dock_pickup.ri`, caching the result.
fn dp_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(DP_PATH).unwrap_or_else(|e| panic!("{DP_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile `dock_pickup.ri` with stdlib, caching the result.
fn dp_compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(dp_source()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// dock_pickup tests
// ═══════════════════════════════════════════════════════════════════════════════

/// The `dock_pickup.ri` file exists, is non-empty, and compiles with stdlib
/// without any Error-severity diagnostics.
///
/// Mirrors `counter_mass_balance_compiles_clean`.
#[test]
fn dock_pickup_compiles_clean() {
    let source = dp_source();
    assert!(!source.is_empty(), "dock_pickup.ri should be non-empty");
    // Compile (or return the cached module); panics on any Error diagnostic.
    let _ = dp_compiled();
}

/// Pure-eval (no OCCT): asserts structural cells produced by `engine.eval()`.
///
/// Checks:
///   - No Error-severity diagnostics from eval.
///   - `DockPickup.snap_count == Value::Int(5)` (5-step sweep over j_x).
///   - `DockPickup.id_head` and `DockPickup.id_park` are both `Value::Int(_)`
///     and are distinct (the OCCT-gated test pins semantic correctness via the
///     clearance assertion; pinning exact insertion-order values here would make
///     every test break if body-ID assignment ever changes).
#[test]
fn dock_pickup_eval_produces_expected_structural_cells() {
    let compiled = dp_compiled();

    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    // snap_count must be Int(5).
    let snap_count_id = ValueCellId::new("DockPickup", "snap_count");
    let snap_count = result
        .values
        .get(&snap_count_id)
        .expect("DockPickup.snap_count not found");
    assert_eq!(
        snap_count,
        &Value::Int(5),
        "snap_count should be Int(5), got {snap_count:?}"
    );

    // id_head and id_park must both be Int-typed and distinct.
    // Current insertion-order semantics: "head_solid" → 0, "parked_tool_solid" → 1;
    // the .ri header documents this but we don't pin the exact values here so that
    // a future body-ID scheme change doesn't break all kinematic tests at once.
    let id_head_id = ValueCellId::new("DockPickup", "id_head");
    let id_head = result
        .values
        .get(&id_head_id)
        .expect("DockPickup.id_head not found");
    assert!(
        matches!(id_head, Value::Int(_)),
        "id_head should be Value::Int, got {id_head:?}"
    );

    let id_park_id = ValueCellId::new("DockPickup", "id_park");
    let id_park = result
        .values
        .get(&id_park_id)
        .expect("DockPickup.id_park not found");
    assert!(
        matches!(id_park, Value::Int(_)),
        "id_park should be Value::Int, got {id_park:?}"
    );

    assert_ne!(
        id_head, id_park,
        "id_head and id_park must be distinct body IDs, both got {id_head:?}"
    );
}

/// OCCT-gated: asserts kinematic-query cells after `engine.build()` with the
/// real OCCT kernel.
///
/// Mirrors `mechanism_interference_smoke::disjoint_cubes_no_pairs_and_positive_clearance`
/// but drives dock_pickup.ri instead of an inline source string.
///
/// Expected results (head [0,20]mm³ vs. parked tool [600,620]mm³, gap = 580mm):
///   - `pairs`     → `Value::List([])` (no colliding pairs)
///   - `collide`   → `Value::Bool(false)`
///   - `clearance` → length Scalar ≈ 0.580 m (within 1e-6 m tolerance)
#[test]
fn dock_pickup_build_with_occt_resolves_clearance() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping dock_pickup_build_with_occt_resolves_clearance: OCCT not available");
        return;
    }

    let compiled = dp_compiled();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine =
        reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())));
    let result = engine.build(compiled, ExportFormat::Step);

    // pairs must be an empty list (no collisions).
    let pairs_id = ValueCellId::new("DockPickup", "pairs");
    let pairs = result
        .values
        .get(&pairs_id)
        .expect("DockPickup.pairs not found");
    match pairs {
        Value::List(items) => {
            assert!(
                items.is_empty(),
                "interferes(s) must be empty for head vs. parked_tool (580mm gap), got {items:?}"
            );
        }
        other => panic!("interferes(s) must be Value::List, got {other:?}"),
    }

    // collide must be false.
    let collide_id = ValueCellId::new("DockPickup", "collide");
    let collide = result
        .values
        .get(&collide_id)
        .expect("DockPickup.collide not found");
    assert_eq!(
        collide,
        &Value::Bool(false),
        "interferes_with(s, id_head, id_park) must be false (580mm gap), got {collide:?}"
    );

    // clearance must be ≈ 0.580 m.
    let clearance_id = ValueCellId::new("DockPickup", "clearance");
    let clearance = result
        .values
        .get(&clearance_id)
        .expect("DockPickup.clearance not found");
    let clearance_m = read_f64(clearance, "DockPickup.clearance");
    let expected_m = 0.580_f64;
    assert!(
        (clearance_m - expected_m).abs() < 1e-6,
        "min_clearance must be ≈{expected_m} m (580mm gap), got {clearance_m} m"
    );
}

/// `engine.check()` on `dock_pickup.ri` produces a non-empty
/// `constraint_results` list in which every entry is `Satisfaction::Satisfied`.
///
/// Mirrors `counter_mass_balance_constraints_all_satisfied`.
/// Pins that the reify-level `constraint snap_count == 5` assertion is actually
/// evaluated by the constraint checker — if the .ri had no constraints or the
/// constraint regressed, this test would fail.
///
/// Note: `Engine::check` internally calls `eval` first, so no separate
/// `engine.eval()` pre-call is needed.
#[test]
fn dock_pickup_constraints_all_satisfied() {
    let compiled = dp_compiled();

    let mut engine = make_simple_engine();
    let check_result = engine.check(compiled);
    assert!(
        !check_result.constraint_results.is_empty(),
        "dock_pickup.ri should have at least one constraint (snap_count == 5)"
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
