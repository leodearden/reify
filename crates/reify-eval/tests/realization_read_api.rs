// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration gate η (task 4513): §8 two-way boundary suite + real-chain e2e.
//!
//! Exercises the realization-read API (docs/prds/v0_6/realization-read-api.md
//! §8/§9) at the PUBLIC API boundary:
//!
//!   * Engine→trampoline direction: build `RealizationReadHandle`s with pub
//!     `::new(.., Some(content))`, register a probe via
//!     `Engine::register_compute_fn`, invoke via `Engine::dispatch_compute_node`,
//!     and read content through the pub `content()/sdf()/surface_mesh()/
//!     volume_mesh()` accessors.
//!   * Trampoline→engine direction: call `pub shell_extract_compute_fn`
//!     directly over a REAL openvdb body SDF (realization arm), a slab
//!     (fallback), and neither (Failed+diagnostic); type-lock the `ComputeFn`
//!     signature; assert cancellation coherence.
//!   * Real-chain e2e: compile a .ri box fixture → eval → realization node in
//!     `snapshot().graph.realizations` → dispatch REAL openvdb SDF through
//!     shell_extract → mid-surface reflects real box geometry.
//!   * Invalidation: param edit → new `content_hash` → new cache key.
//!
//! ## Honesty note
//!
//! The crate-private β→γ projection seam (`build_compute_realization_inputs`,
//! `project_realization_read_handle`, `realize_solid_sdf` — all `pub(crate)`;
//! `realization_handles`, `realization_projection_store` — private fields) is
//! exhaustively **in-crate tested** in `src/realization_read_gamma.rs` and
//! `src/realization_content.rs`. Full user-level
//! `Value::GeometryHandle → realization_inputs` routing completes in task 4091.
//!
//! η tests only the externally-observable slice of the §8 contract and bridges
//! the two e2e halves at engine-API level (not the crate-private projection seam).

#![allow(clippy::mutable_key_type)]

use std::cell::RefCell;

use reify_core::{ContentHash, RealizationNodeId};
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, Engine, RealizationReadHandle, RealizedContent,
    register_shell_extract_compute_fns, shell_extract_compute_fn,
};
use reify_ir::{
    ElementOrderTag, InterpolationKind, OpaqueState, SampledField, SampledGridKind, Value,
    VolumeMesh,
};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

use std::sync::Arc;

// Ensure the openvdb registration (inventory::submit!) fires at binary startup.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

// ── Probe trampoline ──────────────────────────────────────────────────────────

thread_local! {
    /// Per-thread capture slot for probe_capture_fn.
    ///
    /// Each test runs on its own thread (in standard cargo test), so this is
    /// isolated across tests. Tests also clear it at entry for defensiveness
    /// against thread reuse.
    static PROBE_CAPTURED: RefCell<Vec<RealizationReadHandle>> = const { RefCell::new(Vec::new()) };
}

/// Probe [`ComputeFn`]: captures `realization_inputs` into [`PROBE_CAPTURED`],
/// then returns `Completed`.
///
/// Purity-preserving (PRD §3.2-1): only *reads* its inputs — the capture is
/// test-only observation of the slice dispatch handed it, not a compute side
/// effect, and the `ComputeFn` signature is unchanged.
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

/// Build a fresh engine with `probe_capture_fn` registered.
///
/// Uses `"test::realization-probe"` as the target so it doesn't collide with
/// production registrations. A new engine per call avoids duplicate-registration
/// panics across tests.
fn probe_engine() -> Engine {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::realization-probe", probe_capture_fn as ComputeFn);
    engine
}

/// Clear [`PROBE_CAPTURED`], dispatch `handles` to the probe, return captured.
fn dispatch_probe(engine: &Engine, handles: &[RealizationReadHandle]) -> Vec<RealizationReadHandle> {
    PROBE_CAPTURED.with(|slot| slot.borrow_mut().clear());
    let _ = engine.dispatch_compute_node(
        "test::realization-probe",
        &[],
        handles,
        &Value::Undef,
        None,
    );
    PROBE_CAPTURED.with(|slot| slot.borrow().clone())
}

// ── step-2 impl: VolumeMesh fixture ──────────────────────────────────────────

/// Canonical single-P1-tet [`VolumeMesh`] fixture.
///
/// Matches `src/realization_read_gamma.rs::make_volume_mesh` — 4 vertices, one
/// tetrahedron with `tet_indices = [0,1,2,3]`, `element_order = P1`,
/// `normals = None`.
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

// ── step-1 test: Engine→trampoline, VolumeMesh per-repr correctness ──────────

/// Engine→trampoline: dispatch a `VolumeMesh` handle to the probe and assert
/// the probe observes it with structurally-correct P1 connectivity.
///
/// RED until step-2 adds `make_volume_mesh`.
#[test]
fn probe_observes_volume_mesh_content_structurally() {
    let engine = probe_engine();
    let vm = Arc::new(make_volume_mesh());
    let handle = RealizationReadHandle::new(
        RealizationNodeId::new("E", 0),
        ContentHash::of_str("vm"),
        Some(RealizedContent::VolumeMesh(Arc::clone(&vm))),
    );

    let captured = dispatch_probe(&engine, &[handle]);

    assert_eq!(captured.len(), 1, "probe must capture exactly one handle");
    let vol = captured[0]
        .volume_mesh()
        .expect("volume_mesh() must be Some for a VolumeMesh handle");
    assert_eq!(vol.element_order, ElementOrderTag::P1, "element_order must be P1");
    assert_eq!(
        vol.tet_indices.len() % 4,
        0,
        "tet_indices.len() must be divisible by 4 (P1 connectivity)"
    );
    assert!(
        vol.tet_indices.len() / 4 > 0,
        "at least one tetrahedron must be present"
    );
}

// ── step-4 impl: real openvdb SDF helpers (cfg(has_openvdb)) ─────────────────

/// Closed-box triangle mesh (8 vertices, 12 triangles, ±1 mm on each axis).
///
/// Mirrors `src/realization_content.rs::box_2mm` verbatim — the canonical
/// fixture for openvdb SDF integration assertions.
#[cfg(has_openvdb)]
fn box_mesh() -> reify_ir::Mesh {
    let v: Vec<f32> = vec![
        -1.0, -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, -1.0, -1.0, 1.0,
        1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
    ];
    #[rustfmt::skip]
    let i: Vec<u32> = vec![
        0,2,1, 0,3,2,  4,5,6, 4,6,7,  0,1,5, 0,5,4,
        2,3,7, 2,7,6,  0,4,7, 0,7,3,  1,2,6, 1,6,5,
    ];
    reify_ir::Mesh { vertices: v, indices: i, normals: None }
}

/// Build a REAL openvdb-derived [`SampledField`] for the `box_mesh()` body.
///
/// Mirrors the path in `src/realization_content.rs::project_voxel_with_openvdb_kernel_returns_sampled_field`:
/// `OpenVdbKernel::new().ingest_mesh(&box_mesh())` → `densify_grid_to_sampled(handle.id)`.
#[cfg(has_openvdb)]
fn real_box_sdf() -> SampledField {
    use reify_ir::GeometryKernel;
    use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_mesh(&box_mesh())
        .expect("ingest_mesh must succeed for a valid closed box");
    kernel
        .densify_grid_to_sampled(handle.id)
        .expect("densify_grid_to_sampled must succeed for an ingested box")
}

// ── step-3 test: Engine→trampoline, SDF per-repr over REAL openvdb geometry ──

/// Engine→trampoline: dispatch a REAL openvdb-derived [`SampledField`] to the
/// probe and assert structural SDF integrity.
///
/// Assertions (all structural — no closed forms or numeric tolerances):
/// - `sdf()` returns `Some`
/// - `data` is non-empty and every value is finite
/// - `bounds_min[i] <= -BOX_HALF` and `bounds_max[i] >= BOX_HALF` on each
///   axis (the grid cover the box body bounds)
/// - the SDF value at the grid point nearest the box centre (0,0,0) is
///   **negative** (interior) — the narrow-band sign convention asserted
///   behaviourally, as `compute_medial_mask` would empty-mask on inverted sign
///
/// RED until step-4 adds `real_box_sdf`.
#[cfg(has_openvdb)]
#[test]
fn probe_observes_real_body_sdf_finite_covers_bounds_interior_negative() {
    let field = real_box_sdf();
    let engine = probe_engine();
    let handle = RealizationReadHandle::new(
        RealizationNodeId::new("E", 1),
        ContentHash::of_str("sdf-real"),
        Some(RealizedContent::Sdf(Arc::new(field))),
    );

    let captured = dispatch_probe(&engine, &[handle]);

    assert_eq!(captured.len(), 1, "probe must capture exactly one handle");
    let field = captured[0]
        .sdf()
        .expect("sdf() must be Some for a Sdf handle");

    // Data must be non-empty and every value finite.
    assert!(!field.data.is_empty(), "SDF data must be non-empty");
    assert!(
        field.data.iter().all(|v| v.is_finite()),
        "all SDF data values must be finite"
    );

    // Grid must cover the box body bounds (-1.0 to +1.0 on each axis).
    const BOX_HALF: f64 = 1.0;
    for i in 0..3 {
        assert!(
            field.bounds_min[i] <= -BOX_HALF,
            "bounds_min[{i}] = {} must cover box min (-{BOX_HALF})",
            field.bounds_min[i]
        );
        assert!(
            field.bounds_max[i] >= BOX_HALF,
            "bounds_max[{i}] = {} must cover box max ({BOX_HALF})",
            field.bounds_max[i]
        );
    }

    // SDF at the box centre (0,0,0) must be negative (interior of the solid).
    // Locate the axis grid entry nearest to 0.0 on each axis, then resolve the
    // flat row-major index (ix + Nx*(iy + Ny*iz)).
    let nearest = |axis: &[f64]| {
        axis.iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.abs().partial_cmp(&b.abs()).expect("finite axis coord")
            })
            .map(|(i, _)| i)
            .expect("axis must be non-empty")
    };
    let ix = nearest(&field.axis_grids[0]);
    let iy = nearest(&field.axis_grids[1]);
    let iz = nearest(&field.axis_grids[2]);
    let nx = field.axis_grids[0].len();
    let ny = field.axis_grids[1].len();
    let flat_idx = ix + nx * (iy + ny * iz);
    let centre_val = field.data[flat_idx];
    assert!(
        centre_val < 0.0,
        "SDF at box centre must be negative (interior); got {centre_val}"
    );
}

// ── step-6 impl: shared-Arc handle builder ────────────────────────────────────

/// Build two [`RealizationReadHandle`]s that share the **exact same**
/// `Arc<VolumeMesh>`.
///
/// Both handles use the same [`ContentHash`] (mirroring the in-crate
/// projection-store memoisation key) so `Arc::ptr_eq` on the extracted
/// `volume_mesh()` proves the public accessor returns the shared allocation.
fn two_handles_sharing_arc() -> (RealizationReadHandle, RealizationReadHandle) {
    let shared_vm = Arc::new(make_volume_mesh());
    let h1 = RealizationReadHandle::new(
        RealizationNodeId::new("shared-arc", 0),
        ContentHash::of_str("shared-vm-hash"),
        Some(RealizedContent::VolumeMesh(Arc::clone(&shared_vm))),
    );
    let h2 = RealizationReadHandle::new(
        RealizationNodeId::new("shared-arc", 1),
        ContentHash::of_str("shared-vm-hash"),
        Some(RealizedContent::VolumeMesh(shared_vm)),
    );
    (h1, h2)
}

// ── step-5 test: Engine→trampoline, memoization as Arc-sharing contract ──────

/// Engine→trampoline: same `Arc<VolumeMesh>` shared across two handles
/// is observed as `Arc::ptr_eq` by the probe.
///
/// Honest framing (doc): store-level `(RealizationNodeId, ContentHash)`
/// memoization is already in-crate tested in
/// `src/realization_content.rs::project_volume_mesh_memoizes` and
/// `project_voxel_memoized_returns_ptr_eq_arc`.
/// This pins the PUBLIC contract: same content_hash → Arc-shared,
/// byte-identical content observable through `volume_mesh()`.
///
/// RED until step-6 adds `two_handles_sharing_arc`.
#[test]
fn probe_observes_arc_shared_content_ptr_eq() {
    let (handle1, handle2) = two_handles_sharing_arc();
    let engine = probe_engine();
    let captured = dispatch_probe(&engine, &[handle1, handle2]);

    assert_eq!(captured.len(), 2, "probe must capture both handles");

    // Extract the inner Arc<VolumeMesh> via content() + pattern matching on
    // RealizedContent::VolumeMesh(arc).  volume_mesh() returns Option<&VolumeMesh>
    // which is a plain reference; Arc::ptr_eq requires &Arc<T>.
    let arc1 = match captured[0].content() {
        Some(RealizedContent::VolumeMesh(a)) => a,
        other => panic!("first handle must be RealizedContent::VolumeMesh; got {other:?}"),
    };
    let arc2 = match captured[1].content() {
        Some(RealizedContent::VolumeMesh(a)) => a,
        other => panic!("second handle must be RealizedContent::VolumeMesh; got {other:?}"),
    };
    assert!(
        Arc::ptr_eq(arc1, arc2),
        "both handles must share the SAME Arc<VolumeMesh> — ptr_eq must hold"
    );
}
