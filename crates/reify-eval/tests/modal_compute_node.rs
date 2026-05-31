//! Integration tests for task κ (3825): the modal ComputeNode warm-state
//! cache + cooperative cancellation, exercised end-to-end through the public
//! §3.4 dispatch seam (`Engine::run_compute_dispatch`) against the registered
//! `"modal::free_vibration"` target.
//!
//! These mirror `tests/cancellation_compute_dispatch.rs` Test D (lines 350-429):
//! seed a Final output VC via `cache_store_mut().put`, then drive
//! `run_compute_dispatch` directly. The 5 modal `value_inputs` are built INLINE
//! here (the `modal_ops` `cfg(test)` helpers are crate-internal and unavailable
//! to this `tests/` crate), using only public `reify_ir`/`reify_core` types.
//!
//! Three observable signals from PRD §3.4 / the κ contract:
//!
//! - **(a) PRE-CANCELLED → PENDING** — a pre-cancelled handle makes the
//!   trampoline short-circuit before the costly eigensolve; `run_compute_dispatch`
//!   returns `Err(DispatchError::Cancelled)` and leaves the output VC `Pending`
//!   (prior best on display, cache untouched). Green after step-10.
//!
//! - **(b) COMPLETED DONATES WARM STATE + COST** — a fresh handle Completes:
//!   the output VC flips `Final`, and the trampoline's assembled `(K, M)` cache
//!   is donated under `NodeId::Compute(c_id)` with a positive `cost_per_byte`.
//!   The `cost_per_byte > 0` half is **RED until step-12** sizes the cache (until
//!   then the donated cost is `0.0`).
//!
//! - **(c) SECOND DISPATCH REUSES WARM STATE** — a second dispatch on the same
//!   `c_id` differing only in `n_modes` sources the donated cache, HITs it
//!   (geometry + material + element_order unchanged), and returns a valid
//!   `ModalResult` — the warm-state round-trip drives no error.

use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn, DispatchError};
use reify_test_support::make_simple_engine;
use reify_core::{ComputeNodeId, DimensionVector, ValueCellId, VersionId};
use reify_ir::{DeterminacyState, Freshness, StructureInstanceData, StructureTypeId, Value};

/// Steel density (kg/m³), mirroring the modal_ops `cfg(test)` fixture.
const STEEL_DENSITY: f64 = 7850.0;

/// Build an `ElasticMaterial`-shaped `Value::StructureInstance` carrying steel
/// elastic constants plus a positive `density` (so the consistent mass matrix
/// `M` is assemblable — the trampoline's density guard passes). Mirrors the
/// runtime material shape the trampoline reads.
fn material(density: f64) -> Value {
    struct_instance(
        "ElasticMaterial",
        vec![
            (
                "youngs_modulus".to_string(),
                Value::Scalar { si_value: 205e9, dimension: DimensionVector::PRESSURE },
            ),
            ("poisson_ratio".to_string(), Value::Real(0.29)),
            (
                "density".to_string(),
                Value::Scalar { si_value: density, dimension: DimensionVector::MASS_DENSITY },
            ),
        ],
    )
}

/// A `Length` scalar (SI metres), as the trampoline reads geometry inputs.
fn length_scalar(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
}

/// A `FixedSupport { target }` instance — the runtime support shape the
/// trampoline's BC realization reads to clamp a face by name.
fn fixed_support(target: &str) -> Value {
    struct_instance(
        "FixedSupport",
        vec![("target".to_string(), Value::String(target.to_string()))],
    )
}

/// Build a `Value::StructureInstance` with the registry-free
/// `StructureTypeId(u32::MAX)` sentinel the trampoline uses.
fn struct_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: type_name.to_string(),
        version: 1,
        fields: fields.into_iter().collect(),
    }))
}

/// The 5 flat modal `value_inputs` matching the `fn modal_analysis` signature:
/// `[material, length, width, height, options]`. A single `x_min` clamp keeps
/// `K_free` SPD (well-posed eigenproblem); `reference_direction` is +Z. The
/// coarse cantilever geometry (0.02 × 0.05 × 0.1 m) matches the modal_ops
/// in-module fixture so the dense eigensolve path is taken.
fn modal_value_inputs(n_modes: i64) -> Vec<Value> {
    let options = struct_instance(
        "ModalOptions",
        vec![
            ("n_modes".to_string(), Value::Int(n_modes)),
            (
                "boundary_conditions".to_string(),
                Value::List(vec![fixed_support("x_min")]),
            ),
            (
                "reference_direction".to_string(),
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
            ),
        ],
    );
    vec![
        material(STEEL_DENSITY),
        length_scalar(0.02),
        length_scalar(0.05),
        length_scalar(0.1),
        options,
    ]
}

/// Number of `Mode`s in a `Completed` `ModalResult` value (panics if the value
/// is not a well-shaped `ModalResult` with a `modes` list).
fn modes_len(value: &Value) -> usize {
    let Value::StructureInstance(data) = value else {
        panic!("expected a ModalResult StructureInstance, got {value:?}");
    };
    assert_eq!(
        data.type_name, "ModalResult",
        "dispatch result must be a ModalResult, got {}",
        data.type_name,
    );
    match data.fields.get(&"modes".to_string()) {
        Some(Value::List(modes)) => modes.len(),
        other => panic!("ModalResult.modes must be a List; got {other:?}"),
    }
}

/// Register the public modal trampoline under its production target on a fresh
/// engine. Each test owns its engine (the registry panics on duplicate targets,
/// and `make_simple_engine` does NOT auto-register compute fns).
fn engine_with_modal_target() -> reify_eval::Engine {
    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "modal::free_vibration",
        reify_eval::modal_ops::solve_modal_analysis_trampoline as ComputeFn,
    );
    engine
}

/// Seed a Final output VC carrying a prior best, so `begin_compute_dispatch`
/// has a `last_substantive` to keep on display when a dispatch is cancelled.
fn seed_final_output(engine: &mut reify_eval::Engine, cell: &ValueCellId) {
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (a) PRE-CANCELLED → output VC stays Pending
// ═══════════════════════════════════════════════════════════════════════════

/// A pre-cancelled handle must drive the modal trampoline to
/// `ComputeOutcome::Cancelled` (the on-entry poll short-circuits before any
/// mesh-build / assembly / eigensolve), so `run_compute_dispatch` returns
/// `Err(DispatchError::Cancelled)` and leaves the seeded output VC `Pending`
/// (prior best on display) rather than `Final`/`Failed`.
#[test]
fn modal_dispatch_precancelled_leaves_output_vc_pending() {
    let mut engine = engine_with_modal_target();

    let cell = ValueCellId::new("ModalFixture", "result");
    let c_id = ComputeNodeId::new("ModalFixture", 0);
    seed_final_output(&mut engine, &cell);

    let inputs = modal_value_inputs(3);

    // Pre-cancel before dispatch.
    let handle = CancellationHandle::new();
    handle.cancel();

    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "modal::free_vibration",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
    );

    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "pre-cancelled modal dispatch must return Err(DispatchError::Cancelled), got {result:?}",
    );

    let node = NodeId::Value(cell.clone());
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "cancelled dispatch must leave the output VC Pending (prior best on display); got {:?}",
        engine.freshness(&node),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (b) + (c) COMPLETED donates warm state + cost; SECOND dispatch reuses it
// ═══════════════════════════════════════════════════════════════════════════

/// A fresh (un-cancelled) dispatch Completes: the output VC flips `Final` and
/// the assembled `(K, M)` cache is donated under `NodeId::Compute(c_id)` with a
/// positive `cost_per_byte`. A second dispatch on the same `c_id` differing only
/// in `n_modes` then sources that donated cache, HITs it, and returns a valid
/// `ModalResult` — the warm-state round-trip through the engine drives no error.
///
/// The `cost_per_byte > 0` assertion is **RED until step-12** sizes the cache:
/// until then the trampoline reports `cost_per_byte = None`, which
/// `complete_compute_dispatch_atomically` stores as `0.0`.
#[test]
fn modal_dispatch_completed_donates_warm_state_then_reuses() {
    let mut engine = engine_with_modal_target();

    let cell = ValueCellId::new("ModalFixture", "result");
    let c_id = ComputeNodeId::new("ModalFixture", 0);
    seed_final_output(&mut engine, &cell);

    // ── (b) first fresh dispatch → Completed, VC Final, warm state donated ──
    let inputs3 = modal_value_inputs(3);
    let handle = CancellationHandle::new();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "modal::free_vibration",
        &inputs3,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
    );
    let (value, _diags) = result.expect("fresh modal dispatch must Ok");
    assert!(
        modes_len(&value) >= 1,
        "fresh modal dispatch must return a valid ModalResult with ≥1 mode",
    );

    let node = NodeId::Value(cell.clone());
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "Completed modal dispatch must flip the output VC to Final",
    );

    // Read cost_per_byte BEFORE any get_warm_state take (the cache pairs the
    // cost-clear with the warm-state take). RED until step-12: the donated cost
    // is 0.0 until the cache is sized.
    let compute_node = NodeId::Compute(c_id.clone());
    match engine.cache_store().cost_per_byte_of(&compute_node) {
        Some(v) => assert!(
            v > 0.0,
            "donated warm state must carry a positive cost_per_byte (RED until step-12); got {v}",
        ),
        None => panic!("Completed dispatch with donated warm state must record a cost_per_byte"),
    }

    // The trampoline's assembled (K, M) cache must be donated to the Compute node.
    let entry = engine
        .cache_store()
        .get(&compute_node)
        .expect("a Completed modal dispatch must create a Compute(c_id) entry carrying warm state");
    assert!(
        entry.warm_state.is_some(),
        "Completed modal dispatch must donate warm state under NodeId::Compute(c_id)",
    );

    // ── (c) second dispatch, same c_id, only n_modes differs → reuse, valid ──
    let inputs5 = modal_value_inputs(5);
    let handle2 = CancellationHandle::new();
    let result2 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "modal::free_vibration",
        &inputs5,
        &[],
        &Value::Undef,
        &handle2,
        VersionId(3),
    );
    let (value2, _diags2) = result2.expect("second modal dispatch (warm-state reuse) must Ok");
    assert!(
        modes_len(&value2) >= 1,
        "the warm-state round-trip must drive a valid ModalResult on the second dispatch",
    );
}
