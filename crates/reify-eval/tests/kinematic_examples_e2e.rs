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
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Satisfaction, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_test_support::{
    collect_errors, decompose_point3, make_simple_engine, parse_and_compile_with_stdlib, read_f64,
};

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
///   - `DockPickup.snap_count == Value::Int(11)` (11-step sweep over j_x,
///     0mm..500mm at 50mm steps).
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

    // snap_count must be Int(11).
    let snap_count_id = ValueCellId::new("DockPickup", "snap_count");
    let snap_count = result
        .values
        .get(&snap_count_id)
        .expect("DockPickup.snap_count not found");
    assert_eq!(
        snap_count,
        &Value::Int(11),
        "snap_count should be Int(11), got {snap_count:?}"
    );

    // id_head and id_park must both be Int-typed and distinct.
    // Current insertion-order semantics: "head_solid" → 0, "dock_solid" → 1;
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
/// Expected results at home position (j_x = 0mm):
///   head [0,20]mm³ vs. dock [300,700]×[0,20]×[0,20]mm³, gap = 300 − 20 = 280mm.
///   - `pairs`     → `Value::List([])` (no colliding pairs)
///   - `collide`   → `Value::Bool(false)`
///   - `clearance` → length Scalar ≈ 0.280 m (within 1e-6 m tolerance)
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

    // pairs must be an empty list (no collisions at home position).
    let pairs_id = ValueCellId::new("DockPickup", "pairs");
    let pairs = result
        .values
        .get(&pairs_id)
        .expect("DockPickup.pairs not found");
    match pairs {
        Value::List(items) => {
            assert!(
                items.is_empty(),
                "interferes(s) must be empty for head vs. dock (280mm gap at home), got {items:?}"
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
        "interferes_with(s, id_head, id_park) must be false (280mm gap at home), got {collide:?}"
    );

    // clearance must be ≈ 0.280 m.
    let clearance_id = ValueCellId::new("DockPickup", "clearance");
    let clearance = result
        .values
        .get(&clearance_id)
        .expect("DockPickup.clearance not found");
    let clearance_m = read_f64(clearance, "DockPickup.clearance");
    let expected_m = 0.280_f64;
    assert!(
        (clearance_m - expected_m).abs() < 1e-6,
        "min_clearance must be ≈{expected_m} m (280mm gap at home), got {clearance_m} m"
    );
}

/// OCCT-gated: asserts that the swept `clearances` cell in `dock_pickup.ri`
/// (resolved via `flat_map(snaps, |s| [min_clearance(s, id_head, id_park)])`)
/// produces a monotone non-increasing list that transitions from strictly
/// positive clearance (head outside the dock) to zero/interfering (head inside
/// the dock) as the prismatic joint advances.
///
/// Expected shape (step-4 geometry: 11-step sweep 0mm..500mm):
///   - `clearances` is a `Value::List` of 11 SI-meter length Scalars.
///   - `clearances[0]` > 1mm (≈ 280mm; head at origin, dock left face at 300mm).
///   - Sequence is monotone non-increasing (each ≤ prev + 1e-6 m).
///   - `clearances.last()` ≈ 0 m (< 1e-6; head fully inside the dock).
///   - At least one strictly-positive (> 1e-3) AND at least one near-zero
///     (< 1e-6) entry (the transition is present).
///
/// RED (step-3): `dock_pickup.ri` has no `clearances` cell yet → the
/// `ValueCellId` lookup panics. GREEN after step-4 adds `clearances` and
/// `flat_map` sweep geometry.
#[test]
fn dock_pickup_clearance_sweep() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping dock_pickup_clearance_sweep: OCCT not available");
        return;
    }

    let compiled = dp_compiled();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine =
        reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())));
    let result = engine.build(compiled, ExportFormat::Step);

    // Look up the swept clearances cell.
    let clearances_id = ValueCellId::new("DockPickup", "clearances");
    let clearances_val = result
        .values
        .get(&clearances_id)
        .expect("DockPickup.clearances not found (step-4 must add clearances = flat_map(snaps, |s| [min_clearance(s, id_head, id_park)]))");

    // Must be a List of 11 elements.
    let clearances: Vec<f64> = match clearances_val {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                11,
                "clearances must have 11 elements (11-step sweep), got {}",
                items.len()
            );
            items
                .iter()
                .enumerate()
                .map(|(i, v)| read_f64(v, &format!("DockPickup.clearances[{i}]")))
                .collect()
        }
        other => panic!("DockPickup.clearances must be Value::List, got {other:?}"),
    };

    // clearances[0] must be strictly positive (≈ 0.280 m).
    assert!(
        clearances[0] > 1e-3,
        "clearances[0] must be > 1mm (head clear of dock at home), got {} m",
        clearances[0]
    );

    // Sequence must be monotone non-increasing.
    for i in 1..clearances.len() {
        assert!(
            clearances[i] <= clearances[i - 1] + 1e-6,
            "clearances must be monotone non-increasing: clearances[{}]={} > clearances[{}]={} + 1e-6",
            i,
            clearances[i],
            i - 1,
            clearances[i - 1]
        );
    }

    // At least one strictly positive entry (outside the dock).
    let has_positive = clearances.iter().any(|&c| c > 1e-3);
    assert!(
        has_positive,
        "clearances must have at least one entry > 1mm (head must start outside the dock)"
    );

    // Last entry must be near zero (head inside the dock → interfering).
    let last = *clearances
        .last()
        .expect("clearances is non-empty (len==11)");
    assert!(
        last < 1e-6,
        "clearances.last() must be ≈ 0 (head inside dock at 500mm), got {last} m"
    );

    // At least one near-zero entry.
    let has_near_zero = clearances.iter().any(|&c| c < 1e-6);
    assert!(
        has_near_zero,
        "clearances must have at least one entry < 1e-6 m (head must end inside the dock)"
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
        "dock_pickup.ri should have at least one constraint (snap_count == 11)"
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
