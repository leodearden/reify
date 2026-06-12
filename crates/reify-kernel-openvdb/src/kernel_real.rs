//! Real `OpenVdbKernel` — backed by the cxx-bridge FFI to libopenvdb 13.x.
//!
//! Only compiled when `cfg(has_openvdb)` is set by `build.rs` (i.e. when
//! `/opt/reify-deps/lib/libopenvdb.so` or an equivalent is found).
//!
//! # Design
//!
//! - Mirrors `crates/reify-kernel-occt/src/lib.rs` (real kernel pattern).
//! - Existing stub `OpenVdbKernel` in `kernel.rs` stays under
//!   `cfg(not(has_openvdb))` and is NOT compiled when this module is active.
//! - `src/lib.rs` does a cfg-conditional `pub use` so external callers see
//!   a single `reify_kernel_openvdb::OpenVdbKernel` regardless of build mode.
//!
//! # Public non-trait methods
//!
//! `realize_voxel_from_mesh`, `active_voxel_count`, `sample_sdf_at`,
//! `write_vdb_grid`, `open_vdb_grid_for_test`, and `densify_grid_to_sampled`
//! are additional public surface area exposed for the v0.4 shells use-case
//! (direct callers) and for integration tests. They are NOT part of the
//! `GeometryKernel` trait — trait dispatch routes through `execute()` / `query()`.

use std::collections::HashMap;
use std::path::Path;

use reify_core::Type;
use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, SampledField, TessError, Value};

use crate::ffi::ffi as openvdb_ffi;
use crate::init::ensure_initialized;

// ---------------------------------------------------------------------------
// OpenVdbKernel (real implementation)
// ---------------------------------------------------------------------------

/// Real OpenVDB kernel backed by libopenvdb 13.x FFI.
///
/// Stores grid handles in a `HashMap<GeometryHandleId, cxx::UniquePtr<OpenVdbGridHandle>>`.
/// IDs are assigned monotonically from a u64 counter.
///
/// `Send + Sync`: `cxx::UniquePtr<T>` is `Send` when `T: Send`; `OpenVdbGridHandle`
/// wraps only a `FloatGrid::Ptr` (heap-allocated, no thread-local state) so
/// the auto-derived `Send + Sync` fire without `unsafe impl`.
pub struct OpenVdbKernel {
    handles: HashMap<GeometryHandleId, cxx::UniquePtr<openvdb_ffi::OpenVdbGridHandle>>,
    next_id: u64,
}

impl OpenVdbKernel {
    /// Construct a new `OpenVdbKernel` and initialise the OpenVDB library.
    pub fn new() -> Self {
        ensure_initialized();
        Self {
            handles: HashMap::new(),
            next_id: 1,
        }
    }

    /// Allocate the next monotonically-increasing handle ID.
    fn alloc_id(&mut self) -> GeometryHandleId {
        let id = GeometryHandleId(self.next_id);
        self.next_id += 1;
        id
    }

    // -----------------------------------------------------------------------
    // Public non-trait methods (direct callers / v0.4 shells use-case)
    // -----------------------------------------------------------------------

    /// Convert a triangle-soup mesh to a narrow-band signed-distance-field
    /// `FloatGrid` via `openvdb::tools::meshToVolume` and register the result
    /// as a new `GeometryHandle`.
    ///
    /// - `verts`: slice of `[x, y, z]` world-space vertex positions.
    /// - `tris`: slice of `[i0, i1, i2]` triangle indices into `verts`.
    /// - `voxel_size`: side length of one voxel (same units as `verts`).
    /// - `half_width_voxels`: narrow-band half-width in voxels (e.g. `3.0`).
    ///
    /// Returns `Err(GeometryError::OperationFailed)` if the mesh is empty or
    /// degenerate (propagated from the C++ `std::runtime_error`).
    pub fn realize_voxel_from_mesh(
        &mut self,
        verts: &[[f32; 3]],
        tris: &[[u32; 3]],
        voxel_size: f64,
        half_width_voxels: f64,
    ) -> Result<GeometryHandleId, GeometryError> {
        let grid_ptr = openvdb_ffi::mesh_to_volume_ffi(verts, tris, voxel_size, half_width_voxels)
            .map_err(|e| GeometryError::OperationFailed(format!("mesh_to_volume_ffi: {e}")))?;

        let id = self.alloc_id();
        self.handles.insert(id, grid_ptr);
        Ok(id)
    }

    /// Convert a [`reify_ir::Mesh`] to a narrow-band SDF `FloatGrid` using
    /// the parameters in `opts`, and register the result as a new handle.
    ///
    /// Reshapes the flat `mesh.vertices` (`Vec<f32>` in xyz order) and
    /// `mesh.indices` (`Vec<u32>` in triangle order) into the `[[f32;3]]`/
    /// `[[u32;3]]` slices expected by [`Self::realize_voxel_from_mesh`], then
    /// delegates with `opts.voxel_size` and `opts.narrow_band`.
    ///
    /// Returns `Err(GeometryError::OperationFailed)` if:
    /// - the vertex or index count is not a multiple of 3 (malformed flat mesh),
    /// - `opts.voxel_size` or `opts.narrow_band` is not positive and finite, or
    /// - the underlying FFI call fails (empty/degenerate mesh).
    pub fn realize_voxel_from_mesh_with_options(
        &mut self,
        mesh: &Mesh,
        opts: &crate::MeshToVoxelOptions,
    ) -> Result<GeometryHandleId, GeometryError> {
        if !mesh.vertices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh.vertices length {} is not a multiple of 3 (expected flat xyz layout)",
                mesh.vertices.len(),
            )));
        }
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh.indices length {} is not a multiple of 3 (expected flat triangle layout)",
                mesh.indices.len(),
            )));
        }
        if !(opts.voxel_size > 0.0 && opts.voxel_size.is_finite()) {
            return Err(GeometryError::OperationFailed(format!(
                "opts.voxel_size must be positive and finite; got {}",
                opts.voxel_size,
            )));
        }
        if !(opts.narrow_band > 0.0 && opts.narrow_band.is_finite()) {
            return Err(GeometryError::OperationFailed(format!(
                "opts.narrow_band must be positive and finite; got {}",
                opts.narrow_band,
            )));
        }
        let verts: Vec<[f32; 3]> = mesh
            .vertices
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let tris: Vec<[u32; 3]> = mesh
            .indices
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        self.realize_voxel_from_mesh(&verts, &tris, opts.voxel_size, opts.narrow_band)
    }

    /// Return the number of active voxels in the grid registered under
    /// `handle`.
    ///
    /// Returns `Err(QueryError::InvalidHandle)` if the handle is not
    /// registered.
    pub fn active_voxel_count(&self, handle: GeometryHandleId) -> Result<usize, QueryError> {
        let grid = self
            .handles
            .get(&handle)
            .ok_or(QueryError::InvalidHandle(handle))?;
        Ok(openvdb_ffi::grid_active_voxel_count(grid))
    }

    /// Sample the signed-distance field at world-space point `(x, y, z)` using
    /// trilinear (BoxSampler) interpolation.
    ///
    /// - Negative: interior.
    /// - Positive: exterior.
    /// - Near-zero: at the surface.
    /// - Saturated `±(half_width × voxel_size)` outside the narrow band.
    ///
    /// Returns `Err(QueryError::InvalidHandle)` if the handle is not registered.
    pub fn sample_sdf_at(
        &self,
        handle: GeometryHandleId,
        x: f64,
        y: f64,
        z: f64,
    ) -> Result<f64, QueryError> {
        let grid = self
            .handles
            .get(&handle)
            .ok_or(QueryError::InvalidHandle(handle))?;
        Ok(openvdb_ffi::grid_sample_sdf(grid, x, y, z))
    }

    /// Write the grid registered under `handle` to a `.vdb` file at `path`
    /// under the given `grid_name`.
    ///
    /// Returns `Err(ExportError::IoError)` if the path is not writable or the
    /// underlying `openvdb::io::File::write` throws.
    ///
    /// # `&self` no-mutation invariant
    ///
    /// Takes `&self` and is registered on the [`Sync`] audit list at the
    /// bottom of this file. The C++ side (`cpp/openvdb_wrapper.cpp::
    /// write_vdb_grid_ffi`) `deepCopy()`s the registered FloatGrid before
    /// applying `setName(grid_name)` to the copy — the registered grid is
    /// NEVER mutated by a write call. This is regression-guarded by
    /// `tests/grid_io_tests.rs::write_vdb_grid_does_not_mutate_registered_handle_grid_name`.
    /// Any future revision that introduces an in-place mutation of the
    /// registered grid (e.g. lifting the `setName` out of the deep-copy
    /// arm) MUST flip the signature back to `&mut self` AND update the
    /// Sync audit list — otherwise concurrent readers from
    /// `sample_sdf_at` / `active_voxel_count` could race.
    ///
    /// # Metadata round-trip gap (units)
    ///
    /// This function writes only the grid name and the active-voxel data;
    /// it does NOT propagate a caller-supplied `units` string into the
    /// `StringMetadata("units")` slot that [`crate::ingest::read_vdb_file`]
    /// reads via `grid_units`. Consequently, a grid that was loaded with
    /// declared units, written via `write_vdb_grid`, and re-read will lose
    /// the units metadata — `OpenVdbGridSource.units` will be `None` on the
    /// second read and unit-validated workflows must re-supply the units
    /// out-of-band.
    ///
    /// A follow-up extension can either accept an `Option<&str>` `units`
    /// parameter and call `grid->insertMeta("units", openvdb::StringMetadata)`
    /// before writing, or expose a separate `write_vdb_grid_with_metadata`
    /// API that takes a richer metadata struct. The current signature
    /// matches the v0.4 shells use-case (in-process realize → write → read
    /// where the codomain dimension is the contract) and is preserved for
    /// stability.
    pub fn write_vdb_grid(
        &self,
        handle: GeometryHandleId,
        path: &Path,
        grid_name: &str,
    ) -> Result<(), ExportError> {
        let grid = self
            .handles
            .get(&handle)
            .ok_or(ExportError::InvalidHandle(handle))?;
        let path_str = path.to_str().ok_or_else(|| {
            ExportError::IoError(format!(
                "path is not valid UTF-8: {} (cxx requires &str; lossy display shown)",
                path.display()
            ))
        })?;
        openvdb_ffi::write_vdb_grid_ffi(grid, path_str, grid_name)
            .map_err(|e| ExportError::IoError(format!("write_vdb_grid_ffi: {e}")))?;
        Ok(())
    }

    /// Open a `.vdb` file and register the named `FloatGrid` as a new handle.
    ///
    /// Intended for use by integration tests that need to round-trip a grid
    /// written by [`Self::write_vdb_grid`] back into a handle they can pass
    /// to `active_voxel_count` / `sample_sdf_at`.
    ///
    /// Gated behind the `test-fixtures` Cargo feature so the test-only API
    /// surface does not leak into production builds. The integration tests
    /// in `tests/*.rs` activate the feature via the self-dev-dep in
    /// `Cargo.toml`. Mirrors the pattern at
    /// `crates/reify-kernel-occt/src/lib.rs`'s `store_*_for_test` family.
    ///
    /// Returns `Err(GeometryError::OperationFailed)` if the file can't be
    /// opened, the grid is absent, or the grid isn't a `FloatGrid`.
    #[cfg(feature = "test-fixtures")]
    pub fn open_vdb_grid_for_test(
        &mut self,
        path: &Path,
        grid_name: &str,
    ) -> Result<GeometryHandleId, GeometryError> {
        let path_str = path.to_str().ok_or_else(|| {
            GeometryError::OperationFailed(format!(
                "path is not valid UTF-8: {} (cxx requires &str; lossy display shown)",
                path.display()
            ))
        })?;
        let grid_ptr = openvdb_ffi::read_vdb_grid_ffi(path_str, grid_name)
            .map_err(|e| GeometryError::OperationFailed(format!("read_vdb_grid_ffi: {e}")))?;
        let id = self.alloc_id();
        self.handles.insert(id, grid_ptr);
        Ok(id)
    }

    /// Read the registered grid's `MetaMap`-backed name.
    ///
    /// Used by integration tests to pin the `&self` no-mutation invariant
    /// for [`Self::write_vdb_grid`] — capture the name pre-write, write
    /// the grid under a different name, then re-read and assert
    /// equality. The fix in `cpp/openvdb_wrapper.cpp::write_vdb_grid_ffi`
    /// (deep-copy before `setName`) preserves the registered grid's
    /// name across the export.
    ///
    /// Gated behind the `test-fixtures` Cargo feature so the test-only
    /// API surface does not leak into production builds. Mirrors
    /// [`Self::open_vdb_grid_for_test`].
    ///
    /// Sound under the [`Sync`] audit list at the bottom of this file:
    /// `Grid::getName()` is a pure read of the cached `MetaMap` entry
    /// — no lazy init, no tree walk.
    #[cfg(feature = "test-fixtures")]
    pub fn grid_name_for_test(&self, handle: GeometryHandleId) -> Result<String, QueryError> {
        let grid = self
            .handles
            .get(&handle)
            .ok_or(QueryError::InvalidHandle(handle))?;
        Ok(openvdb_ffi::grid_name(grid))
    }

    /// Densify the registered SDF grid into a flat f64 buffer and lower it to
    /// a [`SampledField`] via the shared `lower_to_sampled` pipeline.
    ///
    /// # What this does
    ///
    /// Mirrors the grid→SampledField extraction path in `read_vdb_file`
    /// (ingest.rs:574–649), but starts from an **already-registered handle**
    /// instead of a file open. The shared `build_realized_grid_source` helper
    /// (ingest.rs) captures the drift-prone invariants (Regular3D, X-outermost
    /// axis convention, f32→f64 conversion, units-empty→None) in one place.
    ///
    /// Codomain is `Type::dimensionless_scalar()` (dimensionless raw SDF).
    /// `meshToLevelSet` writes no units metadata → `grid_units` returns "" →
    /// `OpenVdbGridSource.units = None` →
    /// `validate_grid_units(None, &Type::dimensionless_scalar()) = Ok(())` → no `UnitMismatch`.
    ///
    /// # `&mut self` — Sync-audit deviation from PRD §7.1
    ///
    /// The PRD §7.1 proposed `&self`, but `grid_bbox_min`, `grid_bbox_max`, and
    /// `grid_densify_to_buffer` all call `h.grid->evalActiveVoxelBoundingBox()`
    /// (cpp/openvdb_wrapper.cpp:131, 138, 274).  The Sync maintenance contract
    /// at the bottom of this file explicitly prohibits new `&self` methods that
    /// call that API: "a future `&self` 'get bbox' accessor would race".
    /// Using `&mut self` sidesteps the concern via borrow-checker exclusivity
    /// — the same approach used by `realize_voxel_from_mesh` and
    /// `open_vdb_grid_for_test`.  Consumer γ (`realize_solid_sdf`) already
    /// holds `&mut self` to call `ingest_mesh`, so chaining densify is free.
    ///
    /// Condition to relax back to `&self` (per PRD §7.1): confirm that
    /// `evalActiveVoxelBoundingBox()` is non-caching on libopenvdb 13.x under
    /// all active build configurations, OR migrate the kernel to the
    /// `Mutex<HashMap<…>>` actor pattern.
    ///
    /// # Errors
    ///
    /// - `Err(QueryError::InvalidHandle(handle))` — handle not registered.
    /// - `Err(QueryError::QueryFailed(_))` — densification overflows the
    ///   C++-side `GRID_DENSIFY_MAX_VOXELS` cap (~256M voxels), or
    ///   `lower_to_sampled` rejects the resulting source (empty grid,
    ///   degenerate bbox, etc.).
    pub fn densify_grid_to_sampled(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<SampledField, QueryError> {
        let grid = self
            .handles
            .get(&handle)
            .ok_or(QueryError::InvalidHandle(handle))?;

        // Read per-axis metadata from the registered grid via FFI.
        let voxel_sizes = openvdb_ffi::grid_voxel_sizes(grid);
        let bbox_min = openvdb_ffi::grid_bbox_min(grid);
        let bbox_max = openvdb_ffi::grid_bbox_max(grid);
        let units_str = openvdb_ffi::grid_units(grid);

        // Densify all active voxels into a flat f32 buffer (X-outermost).
        // The C++ side caps at GRID_DENSIFY_MAX_VOXELS (~256M) and throws
        // std::runtime_error on overflow; cxx maps it to Err(cxx::Exception).
        let raw_buffer = openvdb_ffi::grid_densify_to_buffer(grid).map_err(|e| {
            QueryError::QueryFailed(format!("densify_grid_to_sampled: {e}"))
        })?;

        // Build the in-memory source using the shared helper (ingest.rs) so
        // the Regular3D + X-outermost + f32→f64 + units-None invariants live
        // in one place, shared with read_vdb_file.
        let source = crate::ingest::build_realized_grid_source(
            voxel_sizes,
            bbox_min,
            bbox_max,
            &units_str,
            raw_buffer,
        );

        // Lower to SampledField.  Codomain = Type::dimensionless_scalar() (dimensionless SDF);
        // meshToLevelSet writes no units → grid_units="" → units=None →
        // validate_grid_units(None, &Type::dimensionless_scalar()) = Ok(()) — no UnitMismatch.
        let outcome = crate::ingest::lower_to_sampled(
            &source,
            SDF_FIELD_NAME,
            &Type::dimensionless_scalar(),
        )
        .map_err(|e| QueryError::QueryFailed(format!("densify_grid_to_sampled: {e}")))?;

        Ok(outcome.field)
    }
}

/// Fixed field name used by `densify_grid_to_sampled`.
///
/// The name feeds the `W_FIELD_OUT_OF_BOUNDS` diagnostic message only —
/// it has no semantic significance for the densified SDF.
const SDF_FIELD_NAME: &str = "openvdb_voxel_sdf";

impl Default for OpenVdbKernel {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: `cxx::UniquePtr<OpenVdbGridHandle>` is the sole owner of its
// heap-allocated `openvdb::FloatGrid::Ptr`. OpenVDB grids are heap-allocated
// objects with no thread-local storage; ownership (the UniquePtr) can be
// safely transferred across thread boundaries.
//
// MAINTENANCE CONTRACT for `unsafe impl Sync`
// -------------------------------------------
//
// `Sync` is justified ONLY by the current set of `&self` methods, all of
// which are read-only over the FloatGrid tree:
//
//   - `active_voxel_count` → `grid_active_voxel_count` →
//     `FloatGrid::activeVoxelCount()`: walks immutable tree topology.
//   - `sample_sdf_at` → `grid_sample_sdf` → `BoxSampler` over a
//     `ConstAccessor`: read-only against the tree.
//   - `write_vdb_grid` → `write_vdb_grid_ffi`: the C++ side
//     `FloatGrid::deepCopy()`s the registered grid before any `setName`
//     or metadata mutation, so the registered grid is NEVER mutated by
//     a write call (regression-guarded by
//     `tests/grid_io_tests.rs::write_vdb_grid_does_not_mutate_registered_handle_grid_name`
//     and the comment block above
//     `cpp/openvdb_wrapper.cpp::write_vdb_grid_ffi`).
//   - `grid_name_for_test` → `grid_name` → `Grid::getName()`: pure read
//     of the cached `MetaMap` entry, no lazy init, no tree walk.
//
// Mutating methods (`realize_voxel_from_mesh`, `open_vdb_grid_for_test`,
// `densify_grid_to_sampled`) take `&mut self` and rely on Rust's borrow
// checker for exclusive access, NOT on `Sync`.
//
// `densify_grid_to_sampled` uses `&mut self` rather than the PRD §7.1 `&self`
// because it calls `grid_bbox_min` / `grid_bbox_max` / `grid_densify_to_buffer`,
// all of which invoke `h.grid->evalActiveVoxelBoundingBox()` — the exact API
// this comment lists below as prohibited for new `&self` methods.  Using
// `&mut self` is the established safe pattern (borrow-checker exclusivity).
// Condition to relax: confirm evalActiveVoxelBoundingBox() is non-caching on
// libopenvdb 13.x, OR migrate to Mutex<HashMap<…>>.
//
// DO NOT add a new `&self` method that internally calls any of the
// following OpenVDB APIs without first replacing this `unsafe impl Sync`
// with a `parking_lot::Mutex<HashMap<…>>` (mirroring the OCCT actor
// pattern). These APIs mutate hidden internal state on first read and
// therefore race under concurrent `&self` callers:
//
//   - `Grid::evalActiveVoxelBoundingBox()` — caches bbox internally on
//     first call in some OpenVDB versions; a future `&self` "get bbox"
//     accessor would race.
//   - `Tree::nodeCount()` and friends — leaf-level counters cache the
//     first walk's result.
//   - `Transform::indexToWorld(..)` on non-linear (frustum/non-affine)
//     transforms — internal LUT lazy initialisation.
//   - Any leaf-level metadata accessor that materialises lazy data
//     (`getMetadata` chains that promote `nullptr` to a default-initialised
//     entry).
//   - Any in-place mutation of the registered FloatGrid (e.g. `setName`,
//     `insertMeta`, `setTransform`, `setBackground`). If a future write
//     entry point would otherwise require such a mutation, deep-copy the
//     grid first (mirror the pattern in `write_vdb_grid_ffi`) so the
//     registered handle stays unchanged.
//
// Today none of the `&self` methods reach those paths — the I/O accessors
// (`grid_bbox_*`, `grid_densify_to_buffer`, `grid_voxel_sizes`) are used
// only from `read_vdb_file` against a freshly-constructed handle that has
// not yet been shared, so concurrency does not arise even though they
// would otherwise be on the audit list.
//
// If you find yourself adding such an accessor: replace the unsafe Sync
// impl with `Mutex<HashMap<...>>` rather than auditing more entries onto
// this list. The maintenance burden of keeping this list accurate scales
// linearly; the Mutex cost is one branch per call.
unsafe impl Send for OpenVdbKernel {}
unsafe impl Sync for OpenVdbKernel {}

// ---------------------------------------------------------------------------
// GeometryKernel trait implementation
// ---------------------------------------------------------------------------

// Planning vs execution contract note (cross-referenced from register.rs docstring):
//
// The `(Convert{from:Mesh}, Voxel)` entry in `openvdb_capability_descriptor()` (register.rs)
// is a PLANNING declaration that lets the dispatcher BFS route BRep→Mesh→Voxel two-stage
// chains. The executable Mesh→Voxel primitive is `realize_voxel_from_mesh_with_options`
// (defined above). Trait-`execute()` of a terminal Voxel op intentionally returns
// `GeometryError::OperationFailed` — graceful degradation, pinned by
// `tests/dispatcher_integration.rs::openvdb_two_stage_chain_terminal_op_execute_degrades_gracefully`.
// Full execute()-trait routing requires a GeometryOp Mesh-input variant and is task ε scope.
const VOXEL_BOOL_STUB_MSG: &str = "OpenVDB voxel-Boolean execution requires Voxel handles on both operands. \
     Direct-call voxelization via realize_voxel_from_mesh_with_options is available; \
     the descriptor now declares (Convert{from:Mesh}, Voxel) so BRep→Mesh→Voxel chains \
     are dispatchable. Full execute()-trait routing (GeometryOp Mesh-input variant) \
     remains future work (task ε).";

impl GeometryKernel for OpenVdbKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        // Voxel op execution through execute() degrades gracefully — see
        // VOXEL_BOOL_STUB_MSG and the planning-vs-execution contract note above.
        // The capability descriptor declares (Convert{from:Mesh},Voxel) and the
        // three Booleans so the dispatcher BFS can reach Voxel; execute() returns
        // OperationFailed (not a panic, not Ok(_)) until task ε wires the
        // realize_voxel_from_mesh_with_options wrapper into engine dispatch.
        Err(GeometryError::OperationFailed(VOXEL_BOOL_STUB_MSG.into()))
    }

    /// Convert a triangle-soup [`Mesh`] to a narrow-band SDF `FloatGrid` via
    /// [`Self::realize_voxel_from_mesh_with_options`] (done #3095) and register
    /// the result as a new `GeometryHandle`.
    ///
    /// # Resolution policy (PRD §3b honest-floor)
    ///
    /// Options are derived from the mesh bounding box via
    /// [`crate::MeshToVoxelOptions::honest_floor`]:
    /// - `voxel_size = h = longest_extent / VOXELS_PER_LONGEST_AXIS` — scales
    ///   with the part so resolution is meaningful across unit systems.
    /// - `narrow_band` wide enough that the level-set band covers the full
    ///   interior (`narrow_band × h ≥ longest_extent/2`), preventing the
    ///   interior-saturation artefact where deep-interior voxels read
    ///   `-half_width × voxel_size` instead of the true SDF value.
    ///
    /// # Returns
    ///
    /// - `Ok(GeometryHandle { id, repr: None })` — `repr` is `None` because
    ///   the Voxel kernel has no BRep sub-shape (mirrors the
    ///   `GeometryHandle` contract at `geometry.rs:121-128`).
    /// - `Err(GeometryError::OperationFailed(_))` for:
    ///   - malformed flat buffers (`vertices.len()` or `indices.len()` not
    ///     a multiple of 3) — validated here before calling `honest_floor`
    ///     so the diagnostic names the actual cause (buffer layout) rather
    ///     than the misleading "bbox extent" message that `honest_floor`
    ///     would produce (it uses `chunks_exact(3)` which drops the trailing
    ///     partial triplet);
    ///   - empty / degenerate / non-finite meshes (honest_floor returns None);
    ///   - invalid opts or FFI failure (propagated from
    ///     `realize_voxel_from_mesh_with_options`).
    fn ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        // Validate flat-buffer lengths before calling honest_floor so the error
        // message names the true cause.  honest_floor's chunks_exact(3) silently
        // drops a trailing partial triplet and would return None with the generic
        // "bbox extent" message instead of the precise layout error below.
        if !mesh.vertices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh.vertices length {} is not a multiple of 3 (expected flat xyz layout)",
                mesh.vertices.len(),
            )));
        }
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh.indices length {} is not a multiple of 3 (expected flat triangle layout)",
                mesh.indices.len(),
            )));
        }
        let opts = crate::MeshToVoxelOptions::honest_floor(mesh).ok_or_else(|| {
            GeometryError::OperationFailed(
                "OpenVdbKernel::ingest_mesh: cannot derive honest-floor voxel size \
                 — mesh has zero or non-finite bounding-box extent"
                    .into(),
            )
        })?;
        let id = self.realize_voxel_from_mesh_with_options(mesh, &opts)?;
        Ok(GeometryHandle { id, repr: None })
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(VOXEL_BOOL_STUB_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(VOXEL_BOOL_STUB_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(VOXEL_BOOL_STUB_MSG.into()))
    }
}
