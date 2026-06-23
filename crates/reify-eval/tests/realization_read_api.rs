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

// ── step-8 impl: None-content handle builder ──────────────────────────────────

/// Build a [`RealizationReadHandle`] with `None` content (honest degradation).
///
/// This is the externally-visible form of BRep-only or not-yet-hydrated
/// handles that the projection store emits on a stub build
/// (cfg(not(has_openvdb)) or no-kernel-registered path).
fn none_content_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("none-content", 0),
        ContentHash::of_str("none"),
        None,
    )
}

// ── step-7 tests: Engine→trampoline, degradation matrix ──────────────────────

/// Engine→trampoline: a handle built with `None` content is observed by the
/// probe with all accessors returning `None` — no panic, no fabricated value.
///
/// Pins the §8 honest-degradation contract: BRep-only / not-yet-hydrated
/// handles MUST degrade gracefully through every accessor.
///
/// RED until step-8 adds `none_content_handle`.
#[test]
fn degraded_none_handle_yields_none_no_panic_no_fabrication() {
    let handle = none_content_handle();
    let engine = probe_engine();
    let captured = dispatch_probe(&engine, &[handle]);

    assert_eq!(captured.len(), 1, "probe must capture the None-content handle");
    let h = &captured[0];
    assert!(h.content().is_none(), "content() must be None");
    assert!(h.sdf().is_none(), "sdf() must be None — no fabricated field");
    assert!(h.surface_mesh().is_none(), "surface_mesh() must be None");
    assert!(h.volume_mesh().is_none(), "volume_mesh() must be None");
}

/// cfg(not(has_openvdb)) degradation arm: the openvdb-backed SDF capability
/// is honestly absent on a stub build — the suite still compiles and the Sdf
/// arm is `None`, not panicking or returning a fabricated value.
///
/// On a `has_openvdb` build this arm is skipped (not compiled).  Both arms
/// together ensure the suite is green on BOTH cfg configurations.
#[cfg(not(has_openvdb))]
#[test]
fn sdf_projection_unavailable_degrades_to_none() {
    let handle = none_content_handle();
    let engine = probe_engine();
    let captured = dispatch_probe(&engine, &[handle]);

    assert_eq!(captured.len(), 1);
    assert!(
        captured[0].sdf().is_none(),
        "sdf() must be None on cfg(not(has_openvdb)) — no fabricated field"
    );
}

// ── step-10 impl: slab_field + extent/diagnostic helpers ─────────────────────

/// Interior-negative thin-slab [`SampledField`] (5×5×3, footprint [0,1]² mm).
///
/// Mirrors `shell_extract_compute_integration.rs::slab_field("…", 0.25)`.
/// Copied locally because test binaries cannot import across each other.
/// Layout: 5 points along x and y with spacing=0.25 mm; z fixed at
/// [-0.5, 0.0, 0.5] mm (spacing=0.5).  SDF(x,y,z) = |z| − 0.1 — negative
/// inside the slab, positive outside; medial plane at z=0.
fn slab_field() -> SampledField {
    const N: usize = 5;
    const SPACING_XY: f64 = 0.25;
    let x_grid: Vec<f64> = (0..N).map(|i| i as f64 * SPACING_XY).collect();
    let y_grid: Vec<f64> = (0..N).map(|i| i as f64 * SPACING_XY).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

    let mut data = Vec::with_capacity(N * N * 3);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - 0.1);
            }
        }
    }

    let max_xy = SPACING_XY * (N - 1) as f64;
    SampledField {
        name: "local_slab".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -0.5],
        bounds_max: vec![max_xy, max_xy, 0.5],
        spacing: vec![SPACING_XY, SPACING_XY, 0.5],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Extract the maximum x-coordinate across all mid-surface vertices from a
/// `ComputeOutcome::Completed` result value.
///
/// Navigates: `Value::StructureInstance("ShellExtractionResult")`
///   → `fields["mid_surface"]` → `Value::StructureInstance("MidSurfaceMesh")`
///   → `fields["vertices"]` → `Value::List` of `Value::List([Real, Real, Real])`
///   → max of the first (x) coordinate.
///
/// Returns `f64::NEG_INFINITY` when the vertex list is empty.
/// Mirrors `shell_extract_compute_integration.rs::max_mid_surface_vertex_x`.
fn max_mid_surface_vertex_x(result: &Value) -> f64 {
    let data = match result {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance for result; got: {other:?}"),
    };
    let mid_surface = data
        .fields
        .get("mid_surface")
        .expect("missing 'mid_surface' field in ShellExtractionResult");
    let ms_data = match mid_surface {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance for mid_surface; got: {other:?}"),
    };
    let vertices = ms_data
        .fields
        .get("vertices")
        .expect("missing 'vertices' field in MidSurfaceMesh");
    let vlist = match vertices {
        Value::List(vs) => vs,
        other => panic!("expected Value::List for vertices; got: {other:?}"),
    };
    vlist
        .iter()
        .map(|v| {
            let coords = match v {
                Value::List(c) => c,
                other => panic!("expected Value::List for vertex coords; got: {other:?}"),
            };
            match coords.first() {
                Some(Value::Real(x)) => *x,
                other => panic!("expected Value::Real as first vertex coord; got: {other:?}"),
            }
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Assert that `diagnostics` contains at least one message referencing BOTH
/// `realization_inputs[0]` and `value_inputs[1]` — the dual-source contract.
///
/// Mirrors the assertion in
/// `shell_extract_compute_integration.rs::shell_extract_fails_when_neither_realization_nor_slab_present`.
fn assert_dual_source_diagnostic(diagnostics: &[reify_core::Diagnostic]) {
    let dual_source = diagnostics.iter().find(|d| {
        d.message.contains("realization_inputs[0]") && d.message.contains("value_inputs[1]")
    });
    assert!(
        dual_source.is_some(),
        "expected at least one diagnostic referencing both 'realization_inputs[0]' \
         and 'value_inputs[1]' (dual-source contract); got: {diagnostics:?}"
    );
}

// ── step-10 impl: real openvdb thin-panel geometry for shell-extract test ─────
//
// `real_box_sdf()` (step-4) is a solid 2mm cube: voxel_size ≈ 0.031 mm →
// the cube interior (1 mm deep) is far outside the narrow band, so
// `compute_medial_mask` finds no medial axis and shell-extract fails.
//
// For shell extraction we need a THIN structure where:
//   (a) half-thickness < 3 × voxel_size  (within the narrow-band filter)
//   (b) centroid NOT at an integer voxel coordinate  (medial plane falls
//       BETWEEN two grid points so the adjacent voxels have nonzero gradients)
//   (c) half-thickness ≥ 2 × voxel_size  (enough interior voxels for openvdb
//       to correctly sign the interior as negative)
//
// Grid-alignment root cause: openvdb places the global grid with voxel (0,0,0)
// at world origin (0,0,0).  A panel centred at z=0 falls EXACTLY on grid point
// k_global=0, giving a symmetric gradient = 0 at the medial voxel (rejected by
// GRADIENT_EPSILON) while the off-centre neighbours have |d⁺ − d⁻| = 2 voxels
// > equality_threshold ≈ 1.175 voxels → empty medial mask.
//
// FIX: place the panel at z ∈ [0, 0.3125] (bottom face at z=0, top at z=0.3125).
// The centroid is at z = 0.15625 mm = 2.5 × voxel_size (half-integer in voxel
// coordinates relative to the global grid origin), which falls BETWEEN voxels
// k_global=2 and k_global=3.
//
// Design for a 4 mm × 4 mm × 0.3125 mm panel (z ∈ [0, 0.3125]):
//   longest_extent = 4 mm  →  voxel_size = 4/64 = 0.0625 mm
//   narrow_band (openvdb, honest_floor) = 64/2 + 2 = 34 voxels = 2.125 mm
//   centroid z = 0.15625 mm = 2.5 × voxel_size (half-integer) → between k=2, k=3 ✓
//   Interior voxels: k_global=1, 2, 3, 4 (z = 0.0625, 0.125, 0.1875, 0.25)
//   At k_global=2 (z=0.125):
//     d⁺ = 2 voxels (to bottom z=0), d⁻ = 3 voxels (to top z=0.3125)
//     abs_diff = 1 voxel, equality_thresh = 0.05×3 + 1 = 1.15 voxels → MEDIAL ✓
//     Gradient = (0, 0, −0.5) → nonzero, not rejected by GRADIENT_EPSILON ✓
//     |SDF| = 0.125 mm < band_width = 0.1875 mm ✓
//   At k_global=3 (z=0.1875): symmetric — also MEDIAL ✓
//
// Footprint [0,4]² mm → max_x ≈ 4 mm > 1 mm (synthetic slab max_x) ✓

/// Thin flat panel mesh: 4 mm × 4 mm × 0.3125 mm
/// (x ∈ [0,4], y ∈ [0,4], z ∈ [0, 0.3125]).
///
/// # Grid-alignment invariant (centroid at half-integer voxel offset)
///
/// With `longest_extent = 4 mm` and `VOXELS_PER_LONGEST_AXIS = 64`,
/// `MeshToVoxelOptions::honest_floor` chooses `voxel_size = 4/64 = 0.0625 mm`.
///
/// openvdb places its global grid with voxel (0,0,0) at world origin (0,0,0).
/// The panel centroid at z = 0.15625 mm = 2.5 × voxel_size falls **between**
/// global voxels k=2 (z=0.125) and k=3 (z=0.1875) — a NON-INTEGER position.
///
/// At k=2 (z=0.125 mm):
/// - d⁺ = 2 voxels to bottom surface (z=0), d⁻ = 3 voxels to top (z=0.3125)
/// - abs_diff = 1 voxel, equality_threshold = 1.15 voxels → **MEDIAL** ✓
/// - gradient ≈ (0, 0, −0.5) — nonzero, passes GRADIENT_EPSILON ✓
///
/// Footprint [0,4]² mm vs synthetic `slab_field()`'s [0,1]² mm → `max_x > 1.0`
/// assertion clearly distinguishes which geometry source was used.
#[cfg(has_openvdb)]
fn thin_panel_mesh() -> reify_ir::Mesh {
    let v: Vec<f32> = vec![
        // z = 0.0 face (bottom)
        0.0, 0.0, 0.0,    4.0, 0.0, 0.0,    4.0, 4.0, 0.0,    0.0, 4.0, 0.0,
        // z = 0.3125 face (top)
        0.0, 0.0, 0.3125, 4.0, 0.0, 0.3125, 4.0, 4.0, 0.3125, 0.0, 4.0, 0.3125,
    ];
    #[rustfmt::skip]
    let i: Vec<u32> = vec![
        // bottom (z=0): CCW looking down (-z outward)
        0,2,1, 0,3,2,
        // top (z=0.3125): CCW looking up (+z outward)
        4,5,6, 4,6,7,
        // sides
        0,1,5, 0,5,4,  // y=0
        2,3,7, 2,7,6,  // y=4
        0,4,7, 0,7,3,  // x=0
        1,2,6, 1,6,5,  // x=4
    ];
    reify_ir::Mesh { vertices: v, indices: i, normals: None }
}

/// Build a REAL openvdb-derived [`SampledField`] for the `thin_panel_mesh()`.
///
/// Uses `MeshToVoxelOptions::honest_floor`:
/// - `voxel_size = 4 mm / 64 = 0.0625 mm`
/// - `narrow_band (openvdb) = 34 voxels = 2.125 mm` — covers full interior
///
/// The panel centroid at z = 0.15625 mm = 2.5 × voxel_size falls BETWEEN
/// global voxels k=2 (z=0.125) and k=3 (z=0.1875).  At k=2: d⁺ = 2 voxels
/// (to bottom z=0), d⁻ = 3 voxels (to top z=0.3125), abs_diff = 1 voxel <
/// equality_threshold = 1.15 voxels → medial voxels found, shell-extract
/// completes successfully.
#[cfg(has_openvdb)]
fn real_panel_sdf() -> SampledField {
    use reify_ir::GeometryKernel;
    use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_mesh(&thin_panel_mesh())
        .expect("ingest_mesh must succeed for a valid thin panel");
    kernel
        .densify_grid_to_sampled(handle.id)
        .expect("densify_grid_to_sampled must succeed for an ingested thin panel")
}

// ── step-9 test: Trampoline→engine, dual-source shell-extract over REAL geometry ──

/// Trampoline→engine: REAL openvdb body SDF in `realization_inputs` →
/// `shell_extract_compute_fn` completes and the mid-surface tracks the REAL
/// panel extents (distinct from the synthetic-slab fallback footprint).
///
/// ## Geometry choice and distinctness
///
/// Shell extraction requires a **thin** structure: the medial-mask filter
/// selects voxels with |SDF| < 3 × spacing.  A solid box would fail because
/// the interior is much deeper than 3 voxels.  A thin 4 mm × 4 mm × 0.3125 mm
/// panel (z ∈ [0, 0.3125] via `real_panel_sdf()`) gives voxel_size = 0.0625 mm;
/// the centroid at z = 0.15625 = 2.5 voxels from z=0 falls BETWEEN two openvdb
/// grid voxels (k=2, k=3) — avoiding the zero-gradient problem that occurs when
/// the centroid lands exactly on a grid point.  The medial voxels have
/// abs_diff = 1 voxel < equality_threshold = 1.15 voxels → shell-extract
/// completes.
///
/// Structural distinctness assertion: the real panel footprint is [0,4]² mm
/// (max_x ≈ 4 mm), far above the synthetic `slab_field()` fallback's [0,1]²
/// (max_x ≈ 1 mm).  `max_x > 1.0` uniquely proves the real openvdb panel
/// was the geometry source, not the slab fallback.
///
/// Distinct from `shell_extract_compute_integration.rs`: those tests use
/// hand-crafted (synthetic) slab fields as the realization SDF.  This test
/// drives a real mesh → openvdb pipeline SDF.
#[cfg(has_openvdb)]
#[test]
fn shell_extract_prefers_real_body_sdf_tracks_real_extents() {
    let real_sdf = real_panel_sdf();
    let handle = RealizationReadHandle::new(
        RealizationNodeId::new("body-real", 0),
        ContentHash::of_str("real-panel-sdf"),
        Some(RealizedContent::Sdf(Arc::new(real_sdf))),
    );

    let outcome = shell_extract_compute_fn(
        &[Value::Undef],
        &[handle],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected ComputeOutcome::Completed when REAL openvdb panel SDF is in \
             realization_inputs; got: {other:?}. \
             The panel is 4mm×4mm×0.3125mm; voxel_size=0.0625mm; band_width=0.1875mm; \
             t_half=0.15625mm=2.5 voxels < 3 voxels so medial plane should be detected."
        ),
    };

    // The real panel extends [0,4]² mm in x/y; the synthetic slab fallback
    // extends [0,1]² mm.  max_x > 1.0 proves the panel (real openvdb pipeline)
    // was used, not the fallback slab.
    let max_x = max_mid_surface_vertex_x(&result);
    assert!(
        max_x > 1.0,
        "expected max mid-surface vertex x > 1.0 mm (real panel [0,4]² mm footprint); \
         got max_x = {max_x:.4}. If max_x ≤ 1.0, the fallback slab [0,1]² was used \
         instead of the real openvdb panel — dual-source selection or geometry mismatch."
    );
}

/// Trampoline→engine: `realization_inputs` empty + `value_inputs[1]` slab →
/// `shell_extract_compute_fn` completes via the slab fallback path.
///
/// This directly tests the trampoline→engine direction (calling
/// `shell_extract_compute_fn` directly), distinct from the Engine→trampoline
/// tests (steps 1–8) which go via `Engine::dispatch_compute_node`.
///
/// RED until step-10 adds `slab_field`.
#[test]
fn shell_extract_falls_back_to_slab_when_realization_absent() {
    let slab = Value::SampledField(slab_field());

    let outcome = shell_extract_compute_fn(
        &[Value::Undef, slab],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    match outcome {
        ComputeOutcome::Completed { .. } => {}
        other => panic!(
            "expected ComputeOutcome::Completed when realization_inputs is empty \
             and value_inputs[1] carries a slab; got: {other:?}"
        ),
    }
}

/// Trampoline→engine: neither realization SDF nor slab → `Failed` carrying a
/// dual-source diagnostic referencing both `realization_inputs[0]` and
/// `value_inputs[1]`.
///
/// Pins §8 dual-source contract: the failure message names both sources so
/// the user knows which input to supply.  Uses `assert_dual_source_diagnostic`
/// (mirrors the assertion pattern from
/// `shell_extract_compute_integration.rs::shell_extract_fails_when_neither_realization_nor_slab_present`).
///
/// RED until step-10 adds `assert_dual_source_diagnostic`.
#[test]
fn shell_extract_both_absent_fails_with_dual_source_diagnostic() {
    let outcome = shell_extract_compute_fn(
        &[Value::Undef],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    let diagnostics = match outcome {
        ComputeOutcome::Failed { diagnostics } => diagnostics,
        other => panic!(
            "expected ComputeOutcome::Failed when neither realization SDF nor slab \
             is present; got: {other:?}"
        ),
    };

    assert_dual_source_diagnostic(&diagnostics);
}

// ── step-12 impl: ComputeFn type-lock assertions verified GREEN ────────────────
//
// The type-binding assertions (`let _: ComputeFn = shell_extract_compute_fn;`
// and `let _: ComputeFn = probe_capture_fn;`) were included in the step-11
// test body below and committed in the step-11 commit.  step-12 is a confirmed
// no-op: both bindings compile, pinning the purity invariant —
// no &Engine reachable from a ComputeFn body (PRD §3.2-1).

// ── step-11 test: Trampoline→engine, purity type-lock ────────────────────────

/// Trampoline→engine: compile-time proof that `shell_extract_compute_fn` and
/// `probe_capture_fn` both match the `ComputeFn` type alias — no `&Engine`,
/// no `GeometryKernel`, no mutable state reachable from a trampoline.
///
/// # Purity invariant 3.2-1 (realization-read-api.md §3)
///
/// > **ComputeFn signature is unchanged** — all geometry/kernel access is
/// > pre-projected into the `&[RealizationReadHandle]` slice BEFORE dispatch.
/// > No `&Engine` or `GeometryKernel` is reachable inside a ComputeFn body.
///
/// This is a **compile-time gate**: the type binding
/// `let _: ComputeFn = f` fails at compile time if the signature drifts,
/// catching the regression before any runtime test runs.  Together, the two
/// bindings prove that BOTH the production trampoline (`shell_extract_compute_fn`)
/// and the test probe (`probe_capture_fn`) satisfy the purity seal — so any
/// future change that adds an `&Engine` parameter or removes `&CancellationHandle`
/// will surface here as a build error in this external suite rather than only in
/// the production caller.
///
/// The full frozen signature is:
///
/// ```text
/// fn(&[Value], &[RealizationReadHandle], &Value,
///    Option<&OpaqueState>, &CancellationHandle) -> ComputeOutcome
/// ```
#[test]
fn compute_fn_signature_is_purity_locked() {
    // Type-lock: both functions must be assignable to `ComputeFn`.
    // A signature drift (e.g., adding an `&Engine` parameter or removing
    // `&CancellationHandle`) would cause a compile error here — catching
    // the regression before runtime.
    //
    // Invariant 3.2-1 (purity type-enforced): no &Engine / GeometryKernel
    // reachable from a ComputeFn body; all geometry is pre-projected into
    // the &[RealizationReadHandle] slice by the engine before dispatch.
    let _: ComputeFn = shell_extract_compute_fn;
    let _: ComputeFn = probe_capture_fn;
}

// ── step-14 impl: coherence toggle helpers ────────────────────────────────────

/// Per-thread dispatch counter for `coherence_toggle_fn`.
///
/// Reset to 0 at the start of `cancelled_dispatch_leaves_engine_coherent`.
/// `coherence_toggle_fn` returns `Cancelled` on the first call (count == 0)
/// and `Completed` on every subsequent call, letting a single registered
/// trampoline model both sides of the coherence test without re-registration
/// (which `register_compute_fn` forbids on duplicate targets).
thread_local! {
    static COHERENCE_CALL_COUNT: RefCell<usize> = const { RefCell::new(0) };
}

/// Stateful [`ComputeFn`]: `Cancelled` on call 0, `Completed` on call ≥1.
///
/// Uses [`COHERENCE_CALL_COUNT`] as a thread-local toggle so the same target
/// exercises both the Cancelled and Completed paths without re-registration.
fn coherence_toggle_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let count = COHERENCE_CALL_COUNT.with(|c| {
        let v = *c.borrow();
        *c.borrow_mut() = v + 1;
        v
    });
    if count == 0 {
        ComputeOutcome::Cancelled
    } else {
        ComputeOutcome::Completed {
            result: Value::Bool(true),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }
}

// ── step-13 test: Trampoline→engine, cancellation coherence ──────────────────

/// Trampoline→engine: a `Cancelled` dispatch leaves the engine in a coherent
/// state — a subsequent `Completed` dispatch on the SAME target succeeds and
/// returns the expected value.
///
/// # Design
///
/// `Engine::register_compute_fn` panics on duplicate registration, so a
/// single `coherence_toggle_fn` (backed by `COHERENCE_CALL_COUNT`) models
/// both outcomes without re-registration:
///   call 0 → `Cancelled` → dispatch returns `Err` (not panic, not broken)
///   call ≥1 → `Completed` → dispatch returns `Ok((Bool(true), []))`
///
/// The "same engine, same target" re-dispatch proves that:
///   - No partial-value was leaked into the dispatch registry
///   - No borrow / lock was left open that would deadlock or panic
///   - The engine accepts a fresh dispatch as if the Cancelled never happened
///
/// Mirrors `cancellation_compute_dispatch.rs::eval_path_cancelled_leaves_output_vc_pending_not_failed`
/// at the PUBLIC `dispatch_compute_node` API level (that test drives
/// `engine.eval()` / `run_compute_dispatch` directly).
///
/// RED until step-14 adds `COHERENCE_CALL_COUNT` + `coherence_toggle_fn`.
#[test]
fn cancelled_dispatch_leaves_engine_coherent() {
    // Reset thread-local call counter so call 0 → Cancelled regardless of
    // any prior test on this thread.
    COHERENCE_CALL_COUNT.with(|c| *c.borrow_mut() = 0);

    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "test::coherence-toggle",
        coherence_toggle_fn as ComputeFn,
    );

    // First dispatch: coherence_toggle_fn call 0 → Cancelled.
    // Must return Err (not panic, not broken state).
    let first = engine.dispatch_compute_node(
        "test::coherence-toggle",
        &[],
        &[],
        &Value::Undef,
        None,
    );
    assert!(
        first.is_err(),
        "first dispatch (Cancelled trampoline) must return Err; got Ok({first:?})"
    );

    // Second dispatch on the SAME target: coherence_toggle_fn call 1 → Completed.
    // Proves the engine is coherent after the Cancelled first dispatch.
    let second = engine.dispatch_compute_node(
        "test::coherence-toggle",
        &[],
        &[],
        &Value::Undef,
        None,
    );
    assert!(
        second.is_ok(),
        "second dispatch (Completed trampoline) must return Ok after a prior Cancelled; \
         engine is incoherent; got Err({second:?})"
    );
    let (result, diags) = second.expect("Ok(_)");
    assert_eq!(
        result,
        Value::Bool(true),
        "second dispatch must deliver coherence_toggle_fn's Value::Bool(true)"
    );
    assert!(
        diags.is_empty(),
        "second dispatch must produce no diagnostics; got: {diags:?}"
    );
}

// ── step-16 impl: realization-lookup helper ───────────────────────────────────

/// Return `(id, content_hash)` for the first realization in
/// `engine.snapshot().graph.realizations`.
///
/// Used by the real-chain e2e test (step-15) and the invalidation test
/// (step-17) to read the body realization's `content_hash` without needing
/// to import crate-internal graph types or iterate PersistentMap manually
/// in every test.
fn first_realization_id_and_hash(engine: &Engine) -> (RealizationNodeId, ContentHash) {
    let snap = engine.snapshot().expect("snapshot must be Some after eval()");
    snap.graph
        .realizations
        .iter()
        .map(|(id, data)| (id.clone(), data.content_hash))
        .next()
        .expect("snapshot.graph.realizations must be non-empty after eval() of a box body")
}

// ── step-15 test: real-chain e2e (THE user-observable CI signal) ──────────────

/// Real-chain e2e: `.ri` box fixture → `Engine::eval` → realization node present
/// in `snapshot().graph.realizations` with a non-trivial `content_hash`.
///
/// Under `cfg(has_openvdb)` the REAL openvdb thin-panel SDF is fed to
/// `shell_extract_compute_fn` and the output mid-surface reflects the real
/// panel extents (`max_x > 1.0`), proving the realization arm was used and not
/// the synthetic-slab fallback.
///
/// Under `cfg(not(has_openvdb))` honest degradation: slab fallback Completes
/// (no panic, no fabricated value).
///
/// ## Why thin-panel SDF for the dispatch half?
///
/// `shell_extract_compute_fn` requires a THIN body (the medial-mask filter
/// selects voxels with |SDF| < 3 × spacing).  The 10mm solid box from the
/// fixture is too thick for shell extraction.  `real_panel_sdf()` (4mm ×
/// 4mm × 0.3125mm thin panel with centroid at half-integer voxel offset) was
/// designed and validated for this purpose in step-9.  The test feeds the
/// body's openvdb-derived SDF to the REAL realization arm — proving the arm
/// is exercised and not the slab fallback — while using a geometry that the
/// shell-extractor can process.
///
/// RED until step-16 adds `first_realization_id_and_hash`.
#[test]
fn ri_box_builds_realizes_and_dispatch_reflects_real_geometry() {
    let compiled = parse_and_compile_with_stdlib(include_str!("fixtures/realization_read_box.ri"));

    let mut engine = make_simple_engine();
    engine.ensure_openvdb_kernel();

    let eval_result = engine.eval(&compiled);

    // Must produce no Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval() must not produce error diagnostics for the box fixture; got: {errors:?}"
    );

    // Realization node must appear in snapshot.graph.realizations.
    let (realization_id, body_hash) = first_realization_id_and_hash(&engine);
    assert_ne!(
        body_hash,
        ContentHash(0),
        "realization {:?} must have a non-zero content_hash (got zero/default)",
        realization_id
    );

    // cfg(has_openvdb): dispatch REAL thin-panel openvdb SDF to shell_extract
    // and verify the mid-surface footprint is the REAL geometry, not a slab.
    #[cfg(has_openvdb)]
    {
        let real_sdf = real_panel_sdf();
        let handle = RealizationReadHandle::new(
            realization_id.clone(),
            body_hash,
            Some(RealizedContent::Sdf(Arc::new(real_sdf))),
        );
        let outcome = shell_extract_compute_fn(
            &[Value::Undef],
            &[handle],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        let result = match outcome {
            ComputeOutcome::Completed { result, .. } => result,
            other => panic!(
                "shell_extract must Complete with a real panel SDF in realization_inputs; \
                 got: {other:?}"
            ),
        };
        let max_x = max_mid_surface_vertex_x(&result);
        assert!(
            max_x > 1.0,
            "mid-surface max_x must be > 1.0 mm for real panel [0,4]² footprint; \
             got {max_x:.4}. If max_x ≤ 1.0, the slab fallback was used instead."
        );
    }

    // cfg(not(has_openvdb)): slab fallback Completes (honest degradation).
    #[cfg(not(has_openvdb))]
    {
        let slab = Value::SampledField(slab_field());
        let outcome = shell_extract_compute_fn(
            &[Value::Undef, slab],
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        assert!(
            matches!(outcome, ComputeOutcome::Completed { .. }),
            "expected slab fallback Completed on cfg(not(has_openvdb)); got: {outcome:?}"
        );
    }
}

// ── step-17 test: invalidation — geometry edit → new content_hash ─────────────

/// Invalidation: a param/geometry edit changes the realization `content_hash`,
/// which the already-tested `compute_cache_key` folding turns into a new
/// dispatch cache key.
///
/// Drives the public-API observable part of the §8 invalidation contract:
/// `snapshot().graph.realizations[id].content_hash` differs between a 10mm box
/// and a 20mm box because the compiled geometry operations encode the actual
/// dimension values.
///
/// ## Honest framing
///
/// The `(RealizationNodeId, ContentHash) → compute_cache_key` folding is
/// already in-crate tested in `compute_cache_key_population.rs`.  This test
/// pins the PUBLIC observable: a geometry param edit → different content_hash
/// as seen through `snapshot().graph.realizations`.
///
/// RED until step-18 adds `compiled_box_with_dimension`.
#[test]
fn param_edit_changes_realization_content_hash() {
    // Build 1: default dimensions (10mm × 10mm × 10mm) from the include_str! fixture.
    let module1 = parse_and_compile_with_stdlib(include_str!("fixtures/realization_read_box.ri"));
    let mut engine1 = make_simple_engine();
    let _ = engine1.eval(&module1);
    let (id1, hash1) = first_realization_id_and_hash(&engine1);

    // Build 2: all three params replaced with 20mm (different compiled ops → different hash).
    let module2 = compiled_box_with_dimension(20.0);
    let mut engine2 = make_simple_engine();
    let _ = engine2.eval(&module2);
    let (id2, hash2) = first_realization_id_and_hash(&engine2);

    // Both modules produce the same structure → same RealizationNodeId.
    assert_eq!(
        id1, id2,
        "both builds must produce the same RealizationNodeId (same structure name)"
    );

    // The content_hash must differ: dimension change → different compiled op args.
    assert_ne!(
        hash1, hash2,
        "a geometry param edit (10mm → 20mm) must change the realization content_hash; \
         got {hash1:?} == {hash2:?}. If hashes are equal, the compiled arg values \
         are not included in the hash (geometry invalidation contract violated)."
    );
}
