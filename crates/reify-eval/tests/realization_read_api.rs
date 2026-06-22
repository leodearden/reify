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
