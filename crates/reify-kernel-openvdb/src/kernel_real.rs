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
//! `write_vdb_grid`, and `open_vdb_grid_for_test` are additional public
//! surface area exposed for the v0.4 shells use-case (direct callers) and
//! for integration tests. They are NOT part of the `GeometryKernel` trait —
//! trait dispatch routes through `execute()` / `query()`.

use std::collections::HashMap;
use std::path::Path;

use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

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
}

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
// Mutating methods (`realize_voxel_from_mesh`, `open_vdb_grid_for_test`)
// take `&mut self` and rely on Rust's borrow checker for exclusive
// access, NOT on `Sync`.
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

const VOXEL_BOOL_STUB_MSG: &str = "OpenVDB voxel-Boolean execution requires Voxel handles on both operands. \
     Direct-call voxelization via realize_voxel_from_mesh_with_options is available; \
     the descriptor now declares (Convert{from:Mesh}, Voxel) so BRep→Mesh→Voxel chains \
     are dispatchable. Full execute()-trait routing (GeometryOp Mesh-input variant) \
     remains future work (task ε).";

impl GeometryKernel for OpenVdbKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        // Voxel Boolean execution through execute() is deferred — see
        // VOXEL_BOOL_STUB_MSG. The capability descriptor declares the Booleans
        // so the dispatcher BFS can enumerate them, but execution requires
        // the full dispatcher chain from BRep→Mesh→Voxel which isn't routed yet.
        Err(GeometryError::OperationFailed(VOXEL_BOOL_STUB_MSG.into()))
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
