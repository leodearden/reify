//! `ManifoldKernel` — Manifold mesh-Boolean kernel adapter.
//!
//! Manifold C++ FFI is wired via `manifold3d` 0.1 (the
//! `zmerlynn/manifold-csg` fork). The kernel maintains a per-handle
//! `HashMap<u64, manifold3d::Manifold>` store mirroring `OcctKernel`'s
//! storage pattern (`crates/reify-kernel-occt/src/lib.rs:456-466`).
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/lib.rs` — storage pattern (HashMap of
//! per-handle native shapes, `next_id` counter, `store/get_*` helpers).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # KernelAttributeHook impl (PRD line 70)
//!
//! ManifoldKernel is the first concrete impl of
//! [`reify_types::KernelAttributeHook`] — see PRD
//! `docs/prds/v0_2/persistent-naming-v2.md` line 70 ("Multi-kernel
//! propagation via `KernelAttributeHook` trait"). The
//! [`GeometryKernel::attribute_hook`] override on `ManifoldKernel` returns
//! `Some(self)` so the engine-side dispatcher
//! (`reify_eval::propagate_via_kernel_attribute_hook`) routes Manifold ops
//! through the hook.
//!
//! ## Task-9-pending stub semantics
//!
//! [`KernelAttributeHook::propagate_attributes`] currently returns
//! `Ok(KernelAttributeOutcome::Discarded)` and emits a
//! `tracing::warn!(reason="task_9_pending", …)` event before returning.
//! The Manifold C++ FFI is wired and the manifold3d accessors
//! (`originalID`, `MeshGL.run_*`, merge vectors, etc.) are reachable from
//! this crate, but the actual MeshGL walk is implemented in
//! persistent-naming-v2 PRD task 9 (a separate task that depends on this
//! crate's FFI wiring). The trait surface is stable across that swap; only
//! the body changes.

use std::collections::HashMap;

use manifold3d::Manifold;
use reify_types::{
    BRepKind, ExportError, ExportFormat, FeatureId, GeometryError, GeometryHandle,
    GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook,
    KernelAttributeOutcome, Mesh, QueryError, TessError, TopologyAttributeTable, Value,
};

/// Error message used by the v0.2 stub paths (`query`/`export`/`tessellate`)
/// that have not yet been wired to real FFI. Boolean ops (`Union`,
/// `Difference`, `Intersection`) and `tessellate` are wired by this task;
/// `query`/`export` remain follow-up work for v0.2.
const STUB_MSG: &str = "Manifold query/export not yet implemented for v0.2; \
    boolean ops and tessellate are wired via manifold3d 0.1, but query/export \
    are follow-up work (see docs/prds/v0_2/multi-kernel.md).";

/// Manifold mesh-Boolean kernel adapter, backed by `manifold3d` 0.1.
///
/// Mirrors `OcctKernel`'s storage shape (`crates/reify-kernel-occt/src/lib.rs:456-466`):
/// per-handle native shapes in a `HashMap<u64, _>` with a monotonic
/// `next_id` counter. Manifold's [`Manifold`] is `Send + Sync` (per the
/// `unsafe impl` blocks in `manifold-csg`'s `manifold.rs`), so
/// `ManifoldKernel` auto-derives `Send + Sync` without needing an
/// actor-thread analogue of `OcctKernelHandle`.
pub struct ManifoldKernel {
    /// Per-handle stored Manifolds. Inserted by [`Self::store`] (called from
    /// `execute` boolean arms and from the `test-fixtures` ingestion path);
    /// looked up by `tessellate` and the boolean arms.
    shapes: HashMap<u64, Manifold>,
    /// Monotonic id counter; first allocated handle is `1` (matches OCCT).
    /// `0` and `u64::MAX` are reserved (the latter is `GeometryHandleId::INVALID`).
    next_id: u64,
}

impl ManifoldKernel {
    /// Construct a new `ManifoldKernel` with empty storage.
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store a `Manifold` and return its newly-allocated handle.
    ///
    /// `repr` is fixed to [`BRepKind::Solid`] because manifold3d's
    /// `Manifold` represents a coherent solid mesh (the type is named after
    /// the manifold property — closed orientable surfaces). There is no
    /// `BRepKind::Mesh` variant; `Solid` is the closest semantic match for
    /// what manifold owns.
    fn store(&mut self, manifold: Manifold) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, manifold);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: BRepKind::Solid,
        }
    }

    /// Test-only ingestion path for `reify_types::Mesh` inputs.
    ///
    /// Widens the input mesh's f32 vertices to f64 (per Decision 4 in the
    /// task plan: "Reify's tolerance regime is f64; manifold internals stay
    /// f64 throughout") and the u32 indices to u64 (per the
    /// `from_mesh_f64` API signature), then constructs a `Manifold` via
    /// `Manifold::from_mesh_f64`. Panics on invalid mesh input — acceptable
    /// for a test fixture.
    ///
    /// Gated on `cfg(any(test, feature = "test-fixtures"))` so the API is
    /// reachable from in-crate `mod tests` (cfg(test)) AND from cross-crate
    /// integration tests in `tests/` (which set the `test-fixtures` feature
    /// via the self-dev-dep in `Cargo.toml`).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn store_mesh_for_test(&mut self, mesh: &Mesh) -> GeometryHandleId {
        let vert_props_f64: Vec<f64> = mesh.vertices.iter().map(|&v| v as f64).collect();
        let tri_indices_u64: Vec<u64> = mesh.indices.iter().map(|&i| i as u64).collect();
        let manifold = Manifold::from_mesh_f64(&vert_props_f64, 3, &tri_indices_u64)
            .expect("store_mesh_for_test: input Mesh must be a valid manifold");
        self.store(manifold).id
    }
}

impl Default for ManifoldKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for ManifoldKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match op {
            GeometryOp::Union { left, right } => {
                // NOTE(step-9/step-10): the `.ok_or(OperationFailed(...))` here
                // is the placeholder error path that step-9's RED test pins;
                // step-10 introduces a centralized `get_manifold` helper that
                // returns `GeometryError::InvalidReference` instead. Until
                // step-10 lands, do NOT change this to InvalidReference — that
                // would defeat the RED→GREEN sequence the plan prescribes.
                let l = self.shapes.get(&left.0).ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "Manifold Union: left handle {left:?} not found"
                    ))
                })?;
                let r = self.shapes.get(&right.0).ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "Manifold Union: right handle {right:?} not found"
                    ))
                })?;
                let result = l.union(r);
                Ok(self.store(result))
            }
            GeometryOp::Difference { left, right } => {
                let l = self.shapes.get(&left.0).ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "Manifold Difference: left handle {left:?} not found"
                    ))
                })?;
                let r = self.shapes.get(&right.0).ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "Manifold Difference: right handle {right:?} not found"
                    ))
                })?;
                let result = l.difference(r);
                Ok(self.store(result))
            }
            // Other ops continue to return the stub error until later steps
            // wire them in (step-6: Intersection).
            _ => Err(GeometryError::OperationFailed(STUB_MSG.into())),
        }
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(STUB_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(STUB_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(STUB_MSG.into()))
    }
    // extract_edges, extract_faces, execute_with_history, query_many all use
    // the trait defaults — they error in the standard "not supported" fashion.

    /// Override the trait default to advertise that ManifoldKernel implements
    /// [`KernelAttributeHook`]. Per PRD line 70, ManifoldKernel is the first
    /// concrete impl: returning `Some(self)` here is what makes the engine-
    /// side dispatcher (`reify-eval::propagate_via_kernel_attribute_hook`)
    /// route attribute propagation to [`Self::propagate_attributes`] rather
    /// than `KernelAttributeOutcome::FellThrough`.
    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(self)
    }
}

/// First concrete impl of [`KernelAttributeHook`] — see PRD line 70.
///
/// In the v0.2 stub, the body unconditionally returns
/// `Ok(KernelAttributeOutcome::Discarded)`. The `tracing::warn!` diagnostic
/// (required by the `Discarded` contract) is added in step 6 of the plan;
/// for now the impl is a pure stub so the structural plumbing
/// (`attribute_hook() → Some` → `propagate_attributes() → Ok(Discarded)`)
/// can be tested first.
///
/// When real Manifold C++ FFI lands in a follow-up task, the body switches
/// to walk `MeshGL` merge vectors + per-triangle `faceID` / `originalID`
/// to copy parent attributes onto result face handles, returning
/// `Propagated` on success and `Discarded` (with a `reason="heavy_remeshing"`
/// flavoured WARN) on lossy remeshing — the trait surface is stable across
/// that swap.
impl KernelAttributeHook for ManifoldKernel {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        op: &GeometryOp,
        parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        // v0.2 stub: real Manifold FFI is deferred. Emit a WARN diagnostic
        // (operator visibility for the intentional attribute-loss path) and
        // return Discarded. The `KernelAttributeOutcome::Discarded` contract
        // mandates that hook impls emit their own diagnostic before
        // returning, so consumers do not need to surface a duplicate.
        //
        // `target: "reify_kernel_manifold::kernel"` matches the module path
        // of this impl so a `RUST_LOG=reify_kernel_manifold::kernel=warn`
        // (or the broader `reify_kernel_manifold=warn`) operator filter sees
        // the event. `reason="deferred_ffi"` is the structured-fields key by
        // which a future `reason="heavy_remeshing"` (when real FFI lands)
        // can be distinguished.
        tracing::warn!(
            target: "reify_kernel_manifold::kernel",
            reason = "deferred_ffi",
            op = ?op,
            parents = parent_handles.len(),
            "Manifold attribute propagation discarded — real FFI deferred (v0.2 stub)"
        );
        Ok(KernelAttributeOutcome::Discarded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the keepable structural property that the macro
    /// `reify_test_support::assert_stub_kernel_errors!` was previously
    /// generating: `ManifoldKernel` is `Send + Sync` and round-trips through a
    /// `Box<dyn GeometryKernel>` upcast. The macro's other generated tests
    /// (which pinned "every method returns Err with substring 'Manifold'") are
    /// intentionally NOT preserved here — they directly contradict the
    /// post-FFI contract where Union/Difference/Intersection succeed on valid
    /// handles.
    #[test]
    fn manifold_kernel_implements_geometry_kernel_trait() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ManifoldKernel>();
        let _boxed: Box<dyn reify_types::GeometryKernel> = Box::new(ManifoldKernel::new());
    }

    /// Construct a closed unit cube as a `reify_types::Mesh` literal: 8
    /// vertices, 12 outward-facing triangles. Used by the boolean-op tests
    /// below to populate input handles via `store_mesh_for_test`.
    ///
    /// Vertices are in the unit `[0, 1]³` corner-block; the optional
    /// `offset` parameter shifts the cube by `(dx, dy, dz)` so two cubes
    /// can be made to overlap.
    ///
    /// Triangle winding follows right-hand-rule outward normals (so the
    /// resulting Manifold is well-oriented and Boolean operations succeed).
    /// This helper lives in `mod tests` rather than at module scope because
    /// it is only used by `test-fixtures`-gated tests.
    #[cfg(feature = "test-fixtures")]
    fn unit_cube_mesh(offset: [f32; 3]) -> Mesh {
        let [dx, dy, dz] = offset;
        Mesh {
            vertices: vec![
                // 0..7 → (x, y, z) for the 8 cube corners
                0.0 + dx, 0.0 + dy, 0.0 + dz, // 0
                1.0 + dx, 0.0 + dy, 0.0 + dz, // 1
                1.0 + dx, 1.0 + dy, 0.0 + dz, // 2
                0.0 + dx, 1.0 + dy, 0.0 + dz, // 3
                0.0 + dx, 0.0 + dy, 1.0 + dz, // 4
                1.0 + dx, 0.0 + dy, 1.0 + dz, // 5
                1.0 + dx, 1.0 + dy, 1.0 + dz, // 6
                0.0 + dx, 1.0 + dy, 1.0 + dz, // 7
            ],
            #[rustfmt::skip]
            indices: vec![
                // -Z bottom (outward = -Z, so CW from +Z view)
                0, 2, 1,  0, 3, 2,
                // +Z top
                4, 5, 6,  4, 6, 7,
                // -Y front
                0, 1, 5,  0, 5, 4,
                // +Y back
                3, 7, 6,  3, 6, 2,
                // -X left
                0, 4, 7,  0, 7, 3,
                // +X right
                1, 2, 6,  1, 6, 5,
            ],
            normals: None,
        }
    }

    /// Pin macro-helper: structural `Ok(GeometryHandle)` shape for the three
    /// boolean op tests below. Match-on-Ok rather than `assert_eq!` because
    /// `GeometryError` does not derive `PartialEq`.
    #[cfg(feature = "test-fixtures")]
    fn assert_ok_handle(result: Result<GeometryHandle, GeometryError>, label: &str) {
        match result {
            Ok(GeometryHandle { id, .. }) => {
                assert_ne!(
                    id,
                    GeometryHandleId::INVALID,
                    "{label} must return a real (non-INVALID) handle id",
                );
            }
            other => panic!(
                "{label} of two valid stored cubes must return Ok(GeometryHandle); got {other:?}"
            ),
        }
    }

    /// RED for step-1 of task 3093: pins that `execute(GeometryOp::Union)`
    /// over two stored unit cubes returns `Ok(GeometryHandle { .. })`.
    ///
    /// Currently fails because (a) `store_mesh_for_test` does not yet exist
    /// on `ManifoldKernel`, and (b) the `execute` impl returns the stub
    /// error. Step-2 makes both true.
    ///
    /// Match-on-Ok-with-id rather than `assert_eq!` because `GeometryError`
    /// does not derive `PartialEq`. We don't pin the `repr` field literal
    /// (the field type is `BRepKind`, which has no `Mesh` variant — manifold
    /// meshes are stored under whichever `BRepKind` the impl assigns; the
    /// structural shape `Ok(GeometryHandle { .. })` is what this test pins).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn union_of_two_stored_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel.store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]));
        let r = kernel.store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]));

        let result = kernel.execute(&GeometryOp::Union {
            left: l,
            right: r,
        });

        assert_ok_handle(result, "Union");
    }

    /// RED for step-3 of task 3093: pins that
    /// `execute(GeometryOp::Difference)` over two overlapping stored unit
    /// cubes returns `Ok(GeometryHandle { .. })`.
    ///
    /// Cubes overlap by 0.5 in x so the difference is a non-degenerate
    /// volume (no early empty-result short-circuit). Currently fails
    /// because the `Difference` arm of `execute` returns the stub error
    /// from step-2; step-4 wires it to `Manifold::difference`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn difference_of_two_stored_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel.store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]));
        let r = kernel.store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]));

        let result = kernel.execute(&GeometryOp::Difference {
            left: l,
            right: r,
        });

        assert_ok_handle(result, "Difference");
    }

    /// RED for step-5 of task 3093: pins that
    /// `execute(GeometryOp::Intersection)` over two overlapping stored
    /// unit cubes returns `Ok(GeometryHandle { .. })`.
    ///
    /// Cubes overlap by 0.5 in x so the intersection has non-empty volume.
    /// We deliberately do NOT pin the geometric volume here (that's a
    /// query, exercised separately) — only the structural handle-return
    /// contract. Currently fails because the `Intersection` arm of
    /// `execute` returns the stub error; step-6 wires it to
    /// `Manifold::intersection`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersection_of_two_overlapping_cubes_returns_ok_handle_with_nonempty_volume() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel.store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]));
        let r = kernel.store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]));

        let result = kernel.execute(&GeometryOp::Intersection {
            left: l,
            right: r,
        });

        assert_ok_handle(result, "Intersection");
    }

    /// PRD docs/prds/v0_2/persistent-naming-v2.md line 70: ManifoldKernel is
    /// the first concrete impl of `KernelAttributeHook`. This test pins the
    /// "ManifoldKernel opts into the hook AND is reachable through the
    /// trait-object accessor" contract — a regression that loses the override
    /// (e.g. removed `attribute_hook()` impl on ManifoldKernel) would silently
    /// fall back to the `None` default and the engine-side dispatcher would
    /// route Manifold ops to `FellThrough`, defeating the multi-kernel
    /// propagation pipeline this task builds.
    ///
    /// Bound as `&dyn GeometryKernel` (not `&ManifoldKernel`) because the
    /// engine-side dispatcher invokes the accessor through a trait object —
    /// asserting via the typed concrete reference would let an accidental
    /// `&self`/`&dyn` divergence slip through.
    #[test]
    fn manifold_kernel_advertises_attribute_hook_via_geometry_kernel_trait() {
        let kernel = ManifoldKernel::new();
        let kernel_ref: &dyn reify_types::GeometryKernel = &kernel;
        assert!(
            kernel_ref.attribute_hook().is_some(),
            "ManifoldKernel must override `attribute_hook()` to return Some(self) — \
             enforces PRD line 70 'first concrete impl of KernelAttributeHook' contract \
             reachable through the trait-object accessor",
        );
    }

    /// PRD line 70: heavy remeshing within tolerance (and, in this v0.2 stub,
    /// deferred FFI) discards attributes with a `tracing::warn!` diagnostic.
    ///
    /// Three properties are pinned by this test:
    /// (a) `propagate_attributes` returns `Ok(KernelAttributeOutcome::Discarded)`
    ///     for the v0.2 stub regardless of inputs — the trait surface model.
    /// (b) `table` is left unchanged: the stub does not write spurious entries.
    /// (c) Exactly one WARN-level event fires at the `reify_kernel_manifold::kernel`
    ///     target, matching the `Discarded` contract that hook impls emit
    ///     their own diagnostic before returning.
    ///
    /// Reuses the `CountingSubscriberBuilder` pattern from
    /// `crates/reify-eval/src/kernel_registry.rs:329-353`. Synthetic op +
    /// handle slices avoid dragging actual kernel state into the test.
    #[test]
    fn manifold_kernel_attribute_hook_returns_discarded_and_emits_warn_diagnostic() {
        use reify_test_support::CountingSubscriberBuilder;
        use reify_types::TopologyAttributeTable;
        use std::sync::atomic::Ordering;

        let kernel = ManifoldKernel::new();
        let mut table = TopologyAttributeTable::default();
        let op = GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        };
        let parents = [GeometryHandleId(1), GeometryHandleId(2)];
        let result = GeometryHandleId(3);
        let feature_id = FeatureId::new("test#realization[0]");

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            // Qualified prefix intentionally pins the `crate::module` tracing target
            // (mirrors `target: "reify_kernel_manifold::kernel"` in the impl above).
            // If the `KernelAttributeHook` impl moves to a different submodule, update
            // both the `target:` literal in `propagate_attributes` and this prefix.
            .target_prefix("reify_kernel_manifold::kernel")
            .build();
        let warn_count = counters[&tracing::Level::WARN].clone();

        let outcome = tracing::subscriber::with_default(subscriber, || {
            kernel.propagate_attributes(&mut table, &op, &parents, result, &feature_id)
        });

        // (a) Outcome is Ok(Discarded) for the v0.2 stub.
        // Match-on-outcome rather than `assert_eq!` because `QueryError` does
        // not derive `PartialEq` (would require widening reify-types' surface
        // for a single test assertion).
        match outcome {
            Ok(KernelAttributeOutcome::Discarded) => {}
            other => panic!(
                "v0.2 Manifold stub must return Ok(Discarded) — real FFI is deferred; got {other:?}"
            ),
        }

        // (b) Table is unchanged: stub does not write spurious entries.
        assert!(
            table.is_empty(),
            "Manifold Discarded path must not write to TopologyAttributeTable — \
             attributes were lost, not propagated",
        );

        // (c) Exactly one WARN event at the reify_kernel_manifold::kernel target.
        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "Manifold Discarded path must emit exactly one WARN event at \
             reify_kernel_manifold::kernel target — operator visibility for the \
             intentional attribute-loss diagnostic per PRD line 70",
        );
    }
}
