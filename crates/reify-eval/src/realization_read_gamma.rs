//! γ (task 4509) β→γ seam integration: a probe-trampoline test that drives a
//! `Value::GeometryHandle` arg through `build_compute_realization_inputs`
//! (β lowering) → `project_realization_read_handle` (γ projection) → a
//! registered probe [`ComputeFn`], then asserts the realized content the probe
//! observes on its `&[RealizationReadHandle]`.
//!
//! ## Why in-crate (not `tests/`)
//!
//! The seam this exercises is crate-private: `build_compute_realization_inputs`
//! is `pub(crate)` and the realization→handle map (`Engine::realization_handles`)
//! is a private field that must be seeded hermetically (no public seam, and the
//! production build path can't deliver a `Value::GeometryHandle` to a compute
//! target until task 4091 — see the dormant-arm note in `realization_content`).
//! This mirrors the sibling β integration test in
//! `engine_compute.rs::tests::beta_lowering`, which is in-crate for the same
//! reason.
//!
//! See `docs/prds/v0_6/realization-read-api.md` task γ §3.3 / §9.

use std::cell::RefCell;
use std::collections::BTreeMap;

use reify_core::{ContentHash, KernelId, RealizationNodeId};
use reify_ir::{
    ElementOrderTag, GeometryHandleId, GeometryKernel, OpaqueState, ReprKind, Value, VolumeMesh,
};
use reify_test_support::mocks::{
    FailingMockGeometryKernel, MockConstraintChecker, MockGeometryKernel,
};

use crate::Engine;
use crate::engine_compute::{ComputeFn, ComputeOutcome, RealizationReadHandle};
use crate::graph::{CancellationHandle, EvaluationGraph, RealizationNodeData};

// ── Probe trampoline ─────────────────────────────────────────────────────────

thread_local! {
    /// Per-thread capture slot: [`probe_capture_fn`] stores a clone of the
    /// `realization_inputs` slice it was invoked with so the test can inspect
    /// the content the β→γ projection delivered.  Each `#[test]` runs on its
    /// own thread, so this isolates captures across tests without a lock; the
    /// tests also `clear()` it at entry for defensiveness against thread reuse.
    static PROBE_CAPTURED: RefCell<Vec<RealizationReadHandle>> = const { RefCell::new(Vec::new()) };
}

/// Probe [`ComputeFn`]: captures the `realization_inputs` it receives into the
/// thread-local [`PROBE_CAPTURED`] slot, then returns `Completed`.
///
/// Purity-preserving (PRD §3.2-1): it only *reads* its inputs — the capture is
/// test-only observation of the slice the dispatch machinery handed it, not a
/// compute side effect, and the `ComputeFn` signature is unchanged.
fn probe_capture_fn(
    _value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    PROBE_CAPTURED.with(|slot| {
        *slot.borrow_mut() = realization_inputs.to_vec();
    });
    ComputeOutcome::Completed {
        result: Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

// ── Fixtures ───────────────────────────────────────────────────────────────

/// Build an `Engine` with a single geometry kernel injected under `name` (the
/// producing-kernel registry name) and an empty capability registry — the γ
/// projection resolves kernels from `geometry_kernels` keyed by
/// `produced_kernel`, not from the dispatch registry.
fn engine_with_kernel(name: &str, kernel: Box<dyn GeometryKernel>) -> Engine {
    let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
    kernels.insert(name.to_string(), kernel);
    Engine::with_test_kernels_and_registry(
        Box::new(MockConstraintChecker::new()),
        kernels,
        BTreeMap::new(),
        Some(name.to_string()),
    )
}

/// Canonical single-P1-tet [`VolumeMesh`] fixture (matches the content-arm
/// fixture in `realization_content`).
fn make_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
            0.0, 0.0, 1.0, // v3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

/// A `Value::GeometryHandle` carrying `realization_ref` (the only field the
/// β lowering inspects). Mirrors `beta_lowering::make_geometry_handle_value`.
fn make_geometry_handle_value(realization_ref: RealizationNodeId) -> Value {
    Value::GeometryHandle {
        realization_ref,
        upstream_values_hash: [0u8; 32],
        kernel_handle: Some(GeometryHandleId(0)),
    }
}

/// Seed a kernel-backed realization: insert the `RealizationNodeData` with
/// `produced_kernel` set AND register the engine-side `realization_handles`
/// entry, so the γ projection can resolve `(kernel, handle)`.
fn seed_kernel_realization(
    engine: &mut Engine,
    graph: &mut EvaluationGraph,
    node_id: RealizationNodeId,
    content_hash: ContentHash,
    produced_repr: ReprKind,
    produced_kernel: KernelId,
    handle: GeometryHandleId,
) {
    graph.realizations.insert(
        node_id.clone(),
        RealizationNodeData {
            id: node_id.clone(),
            operations: vec![],
            content_hash,
            produced_repr,
            geometry_cell: None,
            produced_kernel: Some(produced_kernel),
        },
    );
    engine.realization_handles.insert(node_id, handle);
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Content path: a VolumeMesh realization produced by a content-capable kernel
/// projects (via the β lowering) to a handle whose `volume_mesh()` the probe
/// trampoline observes as `Some` — element_order preserved, P1 connectivity
/// (`tet_indices.len() % 4 == 0`), ≥1 tet — with zero projection diagnostics.
#[test]
fn probe_receives_volume_mesh_content_through_beta_gamma_seam() {
    PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    let mock = MockGeometryKernel::new().with_volume_mesh_output(make_volume_mesh());
    let mut engine = engine_with_kernel("gmsh", Box::new(mock));
    engine.register_compute_fn("test::gamma_probe", probe_capture_fn as ComputeFn);

    let mut graph = EvaluationGraph::default();
    let r0 = RealizationNodeId::new("E", 0);
    let h = ContentHash::of_str("vmesh-content");
    seed_kernel_realization(
        &mut engine,
        &mut graph,
        r0.clone(),
        h,
        ReprKind::VolumeMesh,
        KernelId::Gmsh,
        GeometryHandleId(1),
    );

    // β lowering: project the GeometryHandle arg into a RealizationReadHandle.
    let arg_values = vec![make_geometry_handle_value(r0.clone())];
    let (inputs, handles, proj_diags) =
        engine.build_compute_realization_inputs(&arg_values, &graph);

    assert_eq!(inputs, vec![r0.clone()], "lowering must contribute R0");
    assert!(
        proj_diags.is_empty(),
        "content projection must emit no diagnostic; got {proj_diags:?}"
    );

    // Dispatch path: invoke the registered probe with the projected handles.
    let dispatched =
        engine.dispatch_compute_node("test::gamma_probe", &[], &handles, &Value::Undef, None);
    assert!(dispatched.is_ok(), "probe trampoline must complete: {dispatched:?}");

    // The probe captured the handles it was invoked with — assert the content.
    let captured = PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert_eq!(captured.len(), 1, "probe must observe exactly one handle");
    let vm = captured[0]
        .volume_mesh()
        .expect("the probe's handle must carry VolumeMesh content");
    assert_eq!(
        vm.element_order,
        ElementOrderTag::P1,
        "element_order must survive the β→γ seam"
    );
    assert_eq!(
        vm.tet_indices.len() % 4,
        0,
        "P1 tet_indices must be a multiple of 4; got len {}",
        vm.tet_indices.len()
    );
    assert!(vm.tet_indices.len() / 4 > 0, "projected mesh must carry ≥1 tet");
}

/// Degradation path: a VolumeMesh realization whose producing kernel's
/// `volume_mesh` returns `Err` (FailingMockGeometryKernel inherits the trait
/// default-Err) surfaces exactly one projection diagnostic from the β lowering,
/// and the probe observes a handle carrying `None` content — no panic.
#[test]
fn probe_receives_none_content_and_diagnostic_for_degraded_kernel() {
    PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());

    let mut engine = engine_with_kernel("gmsh", Box::new(FailingMockGeometryKernel));
    engine.register_compute_fn("test::gamma_probe", probe_capture_fn as ComputeFn);

    let mut graph = EvaluationGraph::default();
    let r0 = RealizationNodeId::new("E", 0);
    let h = ContentHash::of_str("vmesh-degraded");
    seed_kernel_realization(
        &mut engine,
        &mut graph,
        r0.clone(),
        h,
        ReprKind::VolumeMesh,
        KernelId::Gmsh,
        GeometryHandleId(1),
    );

    let arg_values = vec![make_geometry_handle_value(r0.clone())];
    let (inputs, handles, proj_diags) =
        engine.build_compute_realization_inputs(&arg_values, &graph);

    assert_eq!(inputs, vec![r0.clone()], "lowering still contributes R0 when degraded");
    assert_eq!(
        proj_diags.len(),
        1,
        "a degraded kernel must surface exactly one projection diagnostic"
    );

    let dispatched =
        engine.dispatch_compute_node("test::gamma_probe", &[], &handles, &Value::Undef, None);
    assert!(
        dispatched.is_ok(),
        "dispatch must not panic on a degraded (None-content) handle: {dispatched:?}"
    );

    let captured = PROBE_CAPTURED.with(|slot| slot.borrow().clone());
    assert_eq!(captured.len(), 1, "probe still observes one (degraded) handle");
    assert!(
        captured[0].content().is_none(),
        "degraded handle must carry no content"
    );
    assert!(
        captured[0].volume_mesh().is_none(),
        "degraded handle volume_mesh() must be None"
    );
}
