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
use reify_ir::{ExportError, ExportFormat, FeatureId, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, KernelAttributeOutcome, Mesh, QueryError, TessError, TopologyAttributeTable, Value};

/// Error message used by the v0.2 stub paths (`query`/`export`) that
/// have not yet been wired to real FFI. Boolean ops (`Union`,
/// `Difference`, `Intersection`) and `tessellate` are now wired via
/// `manifold3d` 0.1; `query`/`export` remain follow-up work for v0.2.
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
    /// `repr` is `None`: Manifold's `Manifold` belongs to the
    /// [`ReprKind::Mesh`] family — there is no meaningful B-rep sub-shape
    /// classification for a mesh kernel, so `repr` carries `None` per task
    /// 3179's architectural decision (option (b)). See also task 3093 review
    /// esc-3093-33, which first identified the semantic abuse.
    fn store(&mut self, manifold: Manifold) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, manifold);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: None,
        }
    }

    /// Look up a stored [`Manifold`] by handle, returning
    /// [`GeometryError::InvalidReference`] when the id is not present.
    ///
    /// Mirrors `OcctKernel::get_shape` (`crates/reify-kernel-occt/src/lib.rs:516-523`).
    /// Centralising the lookup in one helper keeps the InvalidReference
    /// surface uniform across `execute`'s boolean arms — `tessellate`
    /// surfaces the same shape via [`TessError::InvalidHandle`] (the
    /// per-trait variant; `GeometryError` and `TessError` are sibling
    /// error enums).
    fn get_manifold(&self, id: GeometryHandleId) -> Result<&Manifold, GeometryError> {
        self.shapes
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))
    }

    /// Test-only ingestion path for `reify_types::Mesh` inputs.
    ///
    /// Widens the input mesh's f32 vertices to f64 (per Decision 4 in the
    /// task plan: "Reify's tolerance regime is f64; manifold internals stay
    /// f64 throughout") and the u32 indices to u64 (per the
    /// `from_mesh_f64` API signature), then constructs a `Manifold` via
    /// `Manifold::from_mesh_f64`. Returns
    /// `Err(GeometryError::OperationFailed)` on invalid mesh input — the
    /// underlying manifold3d error is surfaced in the `OperationFailed`
    /// payload so a winding-order regression in a fixture is debuggable
    /// rather than presenting as a generic "must be a valid manifold"
    /// message.
    ///
    /// Gated on `cfg(any(test, feature = "test-fixtures"))` so the API is
    /// reachable from in-crate `mod tests` (cfg(test)) AND from cross-crate
    /// integration tests in `tests/` (which set the `test-fixtures` feature
    /// via the self-dev-dep in `Cargo.toml`).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn store_mesh_for_test(&mut self, mesh: &Mesh) -> Result<GeometryHandleId, GeometryError> {
        let vert_props_f64: Vec<f64> = mesh.vertices.iter().map(|&v| v as f64).collect();
        let tri_indices_u64: Vec<u64> = mesh.indices.iter().map(|&i| i as u64).collect();
        let manifold =
            Manifold::from_mesh_f64(&vert_props_f64, 3, &tri_indices_u64).map_err(|e| {
                GeometryError::OperationFailed(format!(
                    "store_mesh_for_test: input Mesh must be a valid manifold; \
                     manifold3d::from_mesh_f64 reported: {e:?}"
                ))
            })?;
        Ok(self.store(manifold).id)
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
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.union(r);
                Ok(self.store(result))
            }
            GeometryOp::Difference { left, right } => {
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.difference(r);
                Ok(self.store(result))
            }
            GeometryOp::Intersection { left, right } => {
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.intersection(r);
                Ok(self.store(result))
            }
            // Non-boolean ops are out of scope for the v0.2 manifold
            // adapter — see `STUB_MSG`.
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

    /// Materialise the stored [`Manifold`] as a `reify_types::Mesh`.
    ///
    /// `tolerance` is intentionally unused at this layer — manifold meshes
    /// are exact, and the underlying [`Manifold`] carries its own tolerance
    /// set at construction (see `manifold-csg`'s tolerance-tracking
    /// invariants). Callers passing non-zero values are not rejected; the
    /// argument is accepted for trait-conformance with [`GeometryKernel`].
    ///
    /// f64→f32 narrowing happens at this boundary because Reify's
    /// `Mesh.vertices: Vec<f32>` is the boundary contract (per Decision 4
    /// in the task plan: "narrow at the boundary; manifold internals stay
    /// f64"). `n_props` from `to_mesh_f64` is `3` (xyz) for the position-
    /// only meshes this kernel ingests; we extract only the first three
    /// properties per vertex to stay robust against manifold internally
    /// growing the property block (e.g. merge-tag layers).
    fn tessellate(&self, handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        let manifold = self
            .shapes
            .get(&handle.0)
            .ok_or(TessError::InvalidHandle(handle))?;

        let (vert_props_f64, n_props, tri_indices_u64) = manifold.to_mesh_f64();

        // Empty/degenerate-manifold short-circuit. A boolean op that
        // produces no overlap (e.g. `Intersection` of disjoint cubes) can
        // surface as `n_props == 0` or empty `vert_props_f64`; without
        // this guard, `vert_props_f64.len() / n_props` panics with
        // divide-by-zero in release builds. Returning an empty `Mesh` is
        // the structurally honest answer — callers can detect it via
        // `mesh.vertices.is_empty()`.
        if n_props == 0 || vert_props_f64.is_empty() {
            return Ok(Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                normals: None,
            });
        }

        // For valid (non-empty) manifolds, manifold3d guarantees at least
        // xyz; surface a runtime `TessError` rather than panicking on a
        // corrupted result so callers can recover.
        if n_props < 3 {
            return Err(TessError::TessellationFailed(format!(
                "manifold3d::to_mesh_f64 returned n_props={n_props}; \
                 need at least 3 (xyz) for a Reify Mesh",
            )));
        }

        // Extract xyz triplets from each n_props-sized vertex block.
        // For our position-only meshes n_props == 3, but manifold may
        // internally maintain additional property layers; we deliberately
        // copy only the first three.
        let n_verts = vert_props_f64.len() / n_props;
        let mut vertices: Vec<f32> = Vec::with_capacity(n_verts * 3);
        for v in 0..n_verts {
            let base = v * n_props;
            vertices.push(vert_props_f64[base] as f32);
            vertices.push(vert_props_f64[base + 1] as f32);
            vertices.push(vert_props_f64[base + 2] as f32);
        }

        // u64→u32 narrowing: manifold's u64 indices are nominal; in
        // practice meshes that fit Reify's Vec<u32> contract have
        // <= 4-billion vertices. We use `u32::try_from` rather than
        // `as u32` so a corrupted Manifold (or future contract change)
        // surfaces as an observable `TessError::TessellationFailed`
        // rather than silently truncating to a structurally invalid
        // Mesh whose downstream consumers would index out-of-bounds.
        let indices: Vec<u32> = tri_indices_u64
            .iter()
            .map(|&i| {
                u32::try_from(i).map_err(|_| {
                    TessError::TessellationFailed(format!(
                        "manifold3d returned triangle index {i} > u32::MAX; \
                         Reify Mesh.indices is Vec<u32>",
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Mesh {
            vertices,
            indices,
            normals: None,
        })
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
/// The body unconditionally returns `Ok(KernelAttributeOutcome::Discarded)`
/// and emits a structured WARN diagnostic (required by the `Discarded`
/// contract). The Manifold C++ FFI is wired (boolean ops + tessellate go
/// through `manifold3d` 0.1) and the manifold3d accessors needed for real
/// propagation (`originalID`, `MeshGL.run_*`, `merge_from_vert`/
/// `merge_to_vert`, `face_id`) are reachable from this crate; the actual
/// `MeshGL` walk is implemented in persistent-naming-v2 PRD task 9 (a
/// separate task that depends on this crate's FFI wiring).
///
/// When PRD task 9 lands, the body switches to walk `MeshGL` merge
/// vectors + per-triangle `faceID` / `originalID` to copy parent
/// attributes onto result face handles, returning `Propagated` on success
/// and `Discarded` (with a `reason="heavy_remeshing"` flavoured WARN) on
/// lossy remeshing — the trait surface is stable across that swap.
impl KernelAttributeHook for ManifoldKernel {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        op: &GeometryOp,
        parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        // v0.2 stub: FFI is wired but the MeshGL walk that implements
        // real attribute propagation is PRD task 9 (persistent-naming-v2).
        // Emit a WARN diagnostic (operator visibility for the intentional
        // attribute-loss path) and return Discarded. The
        // `KernelAttributeOutcome::Discarded` contract mandates that hook
        // impls emit their own diagnostic before returning, so consumers
        // do not need to surface a duplicate.
        //
        // `target: "reify_kernel_manifold::kernel"` matches the module
        // path of this impl so a `RUST_LOG=reify_kernel_manifold::kernel=warn`
        // (or the broader `reify_kernel_manifold=warn`) operator filter
        // sees the event. `reason="task_9_pending"` is the structured-
        // fields key by which a future `reason="heavy_remeshing"` (when
        // PRD task 9 lands the real walk) can be distinguished.
        tracing::warn!(
            target: "reify_kernel_manifold::kernel",
            reason = "task_9_pending",
            op = ?op,
            parents = parent_handles.len(),
            "Manifold attribute propagation discarded — MeshGL walk pending (PRD task 9)"
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
        let _boxed: Box<dyn reify_ir::GeometryKernel> = Box::new(ManifoldKernel::new());
    }

    // The `unit_cube_mesh` helper used by the boolean-op tests below
    // lives in [`crate::test_fixtures`] so the same fixture is shared by
    // the cross-crate integration tests under `tests/` (avoids drift).
    #[cfg(feature = "test-fixtures")]
    use crate::test_fixtures::unit_cube_mesh;

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
    /// does not derive `PartialEq`. The `repr: None` contract is pinned
    /// separately by `manifold_kernel_handle_repr_is_none_for_non_brep_kernel`;
    /// this test only pins the structural `Ok(GeometryHandle { .. })` shape.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn union_of_two_stored_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let result = kernel.execute(&GeometryOp::Union { left: l, right: r });

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
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let result = kernel.execute(&GeometryOp::Difference { left: l, right: r });

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
    ///
    /// Renamed during amendment round 2 (was
    /// `…_returns_ok_handle_with_nonempty_volume`) so the name matches what
    /// the assertions actually pin: the structural Ok-handle shape, not the
    /// non-empty volume. The disjoint-input empty-mesh contract is exercised
    /// separately by
    /// [`tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh`].
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersection_of_two_overlapping_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let result = kernel.execute(&GeometryOp::Intersection { left: l, right: r });

        assert_ok_handle(result, "Intersection");
    }

    /// Pins the empty-/degenerate-manifold short-circuit in
    /// [`ManifoldKernel::tessellate`] (kernel.rs `n_props == 0 ||
    /// vert_props_f64.is_empty()` branch).
    ///
    /// Two cubes offset 5 units in x cannot overlap, so
    /// `Manifold::intersection` returns an empty Manifold. Without the
    /// short-circuit, `tessellate` would panic with a divide-by-zero in
    /// release builds when computing `vert_props_f64.len() / n_props`. The
    /// structurally honest answer is an empty `Mesh` (no vertices, no
    /// indices) — callers detect it via `mesh.vertices.is_empty()`.
    ///
    /// Added during amendment round 2 (was previously uncovered: a
    /// regression that removed the short-circuit would only surface as a
    /// release-build panic on disjoint-input boolean callers).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        // Offset >> 1.0 so the two cubes share no volume.
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([5.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let intersection_handle = kernel
            .execute(&GeometryOp::Intersection { left: l, right: r })
            .expect("Intersection of two valid (disjoint) cubes must Ok-return a handle");

        let mesh = kernel.tessellate(intersection_handle.id, 0.0).expect(
            "tessellate of empty/degenerate Manifold must Ok-return an empty Mesh, \
                 not panic via the divide-by-zero short-circuit guard",
        );

        assert!(
            mesh.vertices.is_empty(),
            "tessellated empty intersection must have zero vertices; got {} f32s",
            mesh.vertices.len(),
        );
        assert!(
            mesh.indices.is_empty(),
            "tessellated empty intersection must have zero indices; got {} u32s",
            mesh.indices.len(),
        );
    }

    /// RED for step-9 of task 3093: pins that `execute(GeometryOp::Union
    /// { left, right })` with handles unknown to the kernel returns
    /// `Err(GeometryError::InvalidReference(_))`.
    ///
    /// Currently fails because the Union arm propagates a generic
    /// `OperationFailed("…not found")` (per the placeholder in step-2).
    /// Step-10 introduces a centralised `get_manifold` helper that returns
    /// `InvalidReference(id)` and wires all three boolean arms +
    /// `tessellate` to use it.
    ///
    /// Match-on-variant rather than equality because `GeometryError` does
    /// not derive `PartialEq`. Either the left or right id may be the
    /// surfaced one — the test accepts whichever the impl looks up first.
    #[test]
    fn execute_union_with_unknown_handle_returns_invalid_reference() {
        let mut kernel = ManifoldKernel::new();
        let result = kernel.execute(&GeometryOp::Union {
            left: GeometryHandleId(99),
            right: GeometryHandleId(100),
        });

        match result {
            Err(GeometryError::InvalidReference(GeometryHandleId(99)))
            | Err(GeometryError::InvalidReference(GeometryHandleId(100))) => {}
            other => panic!(
                "execute(Union) with unknown handles must return \
                 Err(GeometryError::InvalidReference(99 or 100)); got {other:?}"
            ),
        }
    }

    /// Pins the per-trait error variant choice for the `tessellate` lookup
    /// path: an unknown handle surfaces as
    /// `Err(TessError::InvalidHandle(handle))`, NOT
    /// `GeometryError::InvalidReference` (which is the sibling variant
    /// reserved for `execute`'s handle-lookup path).
    ///
    /// `execute_union_with_unknown_handle_returns_invalid_reference` above
    /// pins the `execute` side; this test pins the `tessellate` side so
    /// the asymmetry between the two trait surfaces (`GeometryError` vs
    /// `TessError`) is locked in. A regression that unifies the two error
    /// types or reroutes `tessellate` through `get_manifold` (which returns
    /// `GeometryError`) would silently change the surfaced variant.
    ///
    /// Added during amendment round 2 (was previously uncovered).
    #[test]
    fn tessellate_with_unknown_handle_returns_invalid_handle() {
        let kernel = ManifoldKernel::new();
        let result = kernel.tessellate(GeometryHandleId(99), 0.0);

        match result {
            Err(TessError::InvalidHandle(GeometryHandleId(99))) => {}
            other => panic!(
                "tessellate(GeometryHandleId(99), …) on an empty kernel must return \
                 Err(TessError::InvalidHandle(GeometryHandleId(99))); got {other:?}"
            ),
        }
    }

    /// RED for step-7 of task 3093: pins that `tessellate(handle, 0.0)`
    /// over a stored Union result returns a non-empty `Mesh` whose index
    /// count is a multiple of three.
    ///
    /// Tolerance is `0.0` because manifold meshes are exact — the
    /// underlying [`Manifold`] carries its own tolerance set at
    /// construction, and the `tessellate` boundary intentionally ignores
    /// the caller-supplied tolerance for the v0.2 path. Step-8 wires
    /// `tessellate` via `Manifold::to_mesh_f64()`.
    ///
    /// Currently fails because `tessellate` returns the stub
    /// `TessError::TessellationFailed`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn tessellate_of_stored_union_returns_nonempty_mesh() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let union_handle = kernel
            .execute(&GeometryOp::Union { left: l, right: r })
            .expect("Union of two valid cubes must succeed");

        let mesh = kernel
            .tessellate(union_handle.id, 0.0)
            .expect("tessellate of stored Union must succeed");

        assert!(
            !mesh.vertices.is_empty(),
            "tessellated Union mesh must have at least one vertex",
        );
        assert!(
            !mesh.indices.is_empty(),
            "tessellated Union mesh must have at least one triangle",
        );
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "tessellated Union mesh indices must be a multiple of 3 (triangles)",
        );
        assert_eq!(
            mesh.vertices.len() % 3,
            0,
            "tessellated Union mesh vertices must be a multiple of 3 (xyz triplets)",
        );
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
        let kernel_ref: &dyn reify_ir::GeometryKernel = &kernel;
        assert!(
            kernel_ref.attribute_hook().is_some(),
            "ManifoldKernel must override `attribute_hook()` to return Some(self) — \
             enforces PRD line 70 'first concrete impl of KernelAttributeHook' contract \
             reachable through the trait-object accessor",
        );
    }

    /// PRD line 70: heavy remeshing within tolerance (and, in this v0.2 stub,
    /// the pending PRD task 9 MeshGL walk) discards attributes with a
    /// `tracing::warn!` diagnostic.
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
        use reify_ir::TopologyAttributeTable;
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
                "v0.2 Manifold stub must return Ok(Discarded) — MeshGL walk pending PRD task 9; got {other:?}"
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

    /// Pins the architectural rule that [`ManifoldKernel`] must not misclassify
    /// its handles as `Some(BRepKind::Solid)` — a Manifold mesh belongs to the
    /// [`ReprKind::Mesh`] family, not the B-rep family, so there is no
    /// meaningful B-rep sub-shape classification and `repr` must be `None`.
    ///
    /// # Context
    ///
    /// - **Task 3179**: Resolves the BRepKind semantic abuse for non-B-rep
    ///   kernels (architectural decision to widen
    ///   `GeometryHandle.repr: BRepKind` → `Option<BRepKind>`).
    /// - **Task 3093 review esc-3093-33**: The original acknowledgement of the
    ///   semantic abuse — Manifold's `store` carried an inline comment "There
    ///   is no `BRepKind::Mesh` variant; `Solid` is the closest semantic
    ///   match", explicitly noting the misclassification.
    /// - **Architectural rule**: `BRepKind` is documented as a *B-rep
    ///   sub-shape classifier for geometry handles managed by the OCCT
    ///   kernel*. Non-B-rep kernels (Mesh/Sdf/Voxel/VolumeMesh families per
    ///   [`ReprKind`]) genuinely have no B-rep sub-shape. `None` is
    ///   structurally honest; `Some(BRepKind::Solid)` was a forced lie.
    ///   The coarse kernel-family classifier lives in [`ReprKind`], not in
    ///   `BRepKind`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn manifold_kernel_handle_repr_is_none_for_non_brep_kernel() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");
        let r = kernel
            .store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold");

        let handle = kernel
            .execute(&GeometryOp::Union { left: l, right: r })
            .expect("Union of two valid stored cubes must return Ok(GeometryHandle)");

        assert!(
            handle.repr.is_none(),
            "ManifoldKernel handles must carry `repr: None` — Manifold meshes \
             belong to ReprKind::Mesh and have no meaningful B-rep sub-shape \
             classification. See task 3179 option (b) and task 3093 review \
             esc-3093-33.",
        );
    }

    /// RED for item 4 of task 3186: pins that `store_mesh_for_test` returns
    /// `Err(GeometryError::OperationFailed(_))` when given an invalid
    /// (non-manifold) mesh.
    ///
    /// A single open triangle is structurally not a closed orientable manifold
    /// (it has three boundary edges with no closing surface), so
    /// `Manifold::from_mesh_f64` must reject it. Match-on-variant rather than
    /// equality because `GeometryError` does not derive `PartialEq` — mirrors
    /// `execute_union_with_unknown_handle_returns_invalid_reference` (lines
    /// 528-543).
    ///
    /// This test does not need `#[cfg(feature = "test-fixtures")]` because it
    /// lives inside the unit `mod tests` block, which is compiled under
    /// `cfg(test)` — the gating predicate `cfg(any(test, feature =
    /// "test-fixtures"))` is satisfied by `cfg(test)` alone.
    ///
    /// Pins the post-conversion `Result` contract: `store_mesh_for_test`
    /// previously returned `GeometryHandleId` and panicked on bad input;
    /// task 3186 step-2 (GREEN) converted the signature to
    /// `Result<GeometryHandleId, GeometryError>`. This test is GREEN as
    /// merged — see git history for the RED→GREEN transition.
    #[test]
    fn store_mesh_for_test_with_invalid_mesh_returns_err_operation_failed() {
        let mut kernel = ManifoldKernel::new();
        // A single open triangle — three vertices, one triangle face.
        // Not a closed manifold: three boundary edges, no closing surface.
        // `Manifold::from_mesh_f64` requires closed orientable surfaces and
        // must fail on this input.
        let bad_mesh = Mesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // v0
                1.0, 0.0, 0.0, // v1
                0.0, 1.0, 0.0, // v2
            ],
            indices: vec![0, 1, 2],
            normals: None,
        };

        let result = kernel.store_mesh_for_test(&bad_mesh);

        match result {
            Err(GeometryError::OperationFailed(msg)) => assert!(
                !msg.is_empty(),
                "OperationFailed payload must surface the manifold3d error — an empty message \
                 would hide the root cause from fixture authors debugging winding-order \
                 regressions (doc comment promises the underlying manifold3d error is surfaced)",
            ),
            other => panic!(
                "store_mesh_for_test with a single-triangle (non-manifold) mesh must return \
                 Err(GeometryError::OperationFailed(_)); got {other:?}"
            ),
        }
    }
}
