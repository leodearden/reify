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

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

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
    pub fn active_voxel_count(
        &self,
        handle: GeometryHandleId,
    ) -> Result<usize, QueryError> {
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
        let path_str = path
            .to_str()
            .ok_or_else(|| ExportError::IoError("path is not valid UTF-8".into()))?;
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
    /// The method is always-public (not cfg-gated) because integration tests
    /// compile the lib without `cfg(test)` set, so a `#[cfg(test)]` gate
    /// would hide it from `tests/*.rs`. The name `_for_test` signals the
    /// intended usage.
    ///
    /// Returns `Err(GeometryError::OperationFailed)` if the file can't be
    /// opened, the grid is absent, or the grid isn't a `FloatGrid`.
    pub fn open_vdb_grid_for_test(
        &mut self,
        path: &Path,
        grid_name: &str,
    ) -> Result<GeometryHandleId, GeometryError> {
        let path_str = path
            .to_str()
            .ok_or_else(|| GeometryError::OperationFailed("path is not valid UTF-8".into()))?;
        let grid_ptr = openvdb_ffi::read_vdb_grid_ffi(path_str, grid_name)
            .map_err(|e| GeometryError::OperationFailed(format!("read_vdb_grid_ffi: {e}")))?;
        let id = self.alloc_id();
        self.handles.insert(id, grid_ptr);
        Ok(id)
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
// safely transferred across thread boundaries. The kernel's `&self` methods
// (`active_voxel_count`, `sample_sdf_at`) only access the FloatGrid through
// const accessors, which are thread-safe for concurrent reads.
//
// This mirrors the reasoning for the OCCT kernel's `OcctKernelHandle` (which
// enforces thread safety via a dedicated actor); here we use unsafe impls
// directly because OpenVDB lacks OCCT's global-state thread-affinity concerns.
unsafe impl Send for OpenVdbKernel {}
unsafe impl Sync for OpenVdbKernel {}

// ---------------------------------------------------------------------------
// GeometryKernel trait implementation
// ---------------------------------------------------------------------------

const VOXEL_BOOL_STUB_MSG: &str =
    "OpenVDB voxel-Boolean execution requires Voxel handles on both operands. \
     Direct-call voxelization via realize_voxel_from_mesh is available; \
     dispatcher routing for Convert{from:Mesh}→Voxel is deferred until OCCT \
     declares (Convert{from:BRep}, Mesh) in its capability descriptor (v0.3).";

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
