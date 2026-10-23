#pragma once
// OpenVDB C++ wrapper for the reify-kernel-openvdb cxx-bridge.
//
// All functions live in namespace reify_openvdb to match the bridge's
// namespace declaration in src/ffi.rs.
//
// The opaque struct OpenVdbGridHandle wraps openvdb::FloatGrid::Ptr so that
// cxx can manage its lifetime via std::unique_ptr<OpenVdbGridHandle>.

#include "rust/cxx.h"

#include <openvdb/openvdb.h>
#include <openvdb/tools/MeshToVolume.h>
#include <openvdb/tools/VolumeToMesh.h>
#include <openvdb/tools/Interpolation.h>
#include <openvdb/io/File.h>
#include <tbb/global_control.h>

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

namespace reify_openvdb {

// ---------------------------------------------------------------------------
// Opaque grid handle
// ---------------------------------------------------------------------------

/// Wrapper around openvdb::FloatGrid::Ptr.
///
/// Exposed to Rust as an opaque type via std::unique_ptr<OpenVdbGridHandle>.
/// All ownership transfer across the FFI boundary goes through unique_ptr,
/// giving RAII cleanup without manual Box::into_raw/from_raw discipline.
struct OpenVdbGridHandle {
    openvdb::FloatGrid::Ptr grid;

    explicit OpenVdbGridHandle(openvdb::FloatGrid::Ptr g) : grid(std::move(g)) {}
};

// ---------------------------------------------------------------------------
// Library lifecycle
// ---------------------------------------------------------------------------

void openvdb_initialize();
rust::String openvdb_version_string();

// ---------------------------------------------------------------------------
// Mesh → Volume
// ---------------------------------------------------------------------------

std::unique_ptr<OpenVdbGridHandle> mesh_to_volume_ffi(
    rust::Slice<const std::array<float, 3>> verts,
    rust::Slice<const std::array<uint32_t, 3>> tris,
    double voxel_size,
    double half_width_voxels);

// ---------------------------------------------------------------------------
// Dense SDF → level set
// ---------------------------------------------------------------------------

/// Build a level-set FloatGrid from a dense block of SDF samples laid out
/// X-outermost row-major (`data[(ix * ny + iy) * nz + iz]`), voxel (0,0,0)
/// centred at world `origin`. Every voxel of the block is set ACTIVE with its
/// value clamped to ±band_half_width, which is also the background (D-8 of
/// docs/prds/v0_6/implicit-solids.md). Throws std::runtime_error on an empty
/// axis, a size mismatch, a block over GRID_DENSIFY_MAX_VOXELS, or a
/// non-finite / non-positive argument or sample.
std::unique_ptr<OpenVdbGridHandle> dense_sdf_to_level_set_ffi(
    rust::Slice<const float> data,
    std::array<uint32_t, 3> dims,
    std::array<double, 3> origin,
    double voxel_size,
    double band_half_width);

// ---------------------------------------------------------------------------
// Volume → Mesh (marching cubes)
// ---------------------------------------------------------------------------

/// Run `openvdb::tools::volumeToMesh` on the registered grid, appending the
/// resulting triangle soup into the caller-allocated output vectors.
///
/// Parameters:
/// - `h`: the OpenVDB grid handle.
/// - `iso_level`: isovalue (0.0 for the zero level-set / SDF surface).
/// - `adaptivity`: adaptivity in [0, 1]. 0.0 = uniform (all quads same size);
///   1.0 = maximum adaptivity (larger polygons in flat regions).
/// - `out_vertices`: output flat vertex array `[x0, y0, z0, x1, y1, z1, ...]` (f32).
/// - `out_indices`: output flat triangle index array `[i0, i1, i2, ...]` (u32), 3 per tri.
///
/// Quads (Vec4I) from `volumeToMesh` are triangulated into two triangles each:
///   (i, j, k) and (i, k, l) for a quad (i, j, k, l).
///
/// Single call: marching cubes runs exactly once (no double execution).
void volume_to_mesh_ffi(
    const OpenVdbGridHandle& h,
    double iso_level,
    double adaptivity,
    rust::Vec<float>& out_vertices,
    rust::Vec<uint32_t>& out_indices);

// ---------------------------------------------------------------------------
// Grid queries
// ---------------------------------------------------------------------------

size_t grid_active_voxel_count(const OpenVdbGridHandle& h);
double grid_sample_sdf(const OpenVdbGridHandle& h, double x, double y, double z);

// ---------------------------------------------------------------------------
// Grid metadata accessors
// ---------------------------------------------------------------------------

std::array<double, 3> grid_bbox_min(const OpenVdbGridHandle& h);
std::array<double, 3> grid_bbox_max(const OpenVdbGridHandle& h);
/// Per-axis voxel sizes (X, Y, Z) read from the grid's linear transform.
/// Anisotropic transforms produce three distinct values; isotropic transforms
/// produce three equal values. The Rust caller decides whether to enforce
/// isotropy or accept the per-axis spacing as-is.
std::array<double, 3> grid_voxel_sizes(const OpenVdbGridHandle& h);
rust::String grid_units(const OpenVdbGridHandle& h);
/// Read the grid's name from its cached `MetaMap` entry. Pure read of an
/// already-materialised string — no lazy initialisation, no tree walk.
/// Safe to call from `&self` callers under the `Sync` audit list at
/// `src/kernel_real.rs:220-260`.
rust::String grid_name(const OpenVdbGridHandle& h);

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

void write_vdb_grid_ffi(
    const OpenVdbGridHandle& h,
    rust::Str path,
    rust::Str grid_name);

std::unique_ptr<OpenVdbGridHandle> read_vdb_grid_ffi(
    rust::Str path,
    rust::Str grid_name);

/// Materialise the active bounding box as a flat row-major X-outermost
/// `Vec<float>` of length `nx * ny * nz`. Sparse grids whose active set
/// occupies a small fraction of the bbox still pay the FULL bbox cost —
/// background voxels get expanded into explicit values in the buffer. For
/// a 1m³ slab at voxel_size=1mm this is 10⁹ floats ≈ 4 GB.
///
/// To prevent OOM on oversized inputs, the function rejects voxel counts
/// that exceed `GRID_DENSIFY_MAX_VOXELS` by throwing
/// `std::runtime_error` (which cxx maps to `Err(cxx::Exception)` on the
/// Rust side; `read_vdb_file` then surfaces it as
/// `IngestError::FileReadError { detail: "grid too large: …" }`). The cap
/// is set to ~256M floats ≈ 1 GB which covers the v0.4 shells use-case
/// (typical realize_voxel_from_mesh outputs are well under this) while
/// rejecting accidental loads of dense terabyte-class .vdb files.
///
/// Future work: a sparse-aware materialisation that honours the active
/// tile structure would let `lower_to_sampled` consume the grid without
/// the bbox densification round-trip — see also the v0.4 shells PRD's
/// "voxel-medial extractor" arm. Not blocking for v0.4.
rust::Vec<float> grid_densify_to_buffer(const OpenVdbGridHandle& h);

/// Maximum bbox voxel count accepted by [`grid_densify_to_buffer`].
/// 256M voxels = 1 GiB at 4 bytes/float. Any larger and we throw rather
/// than allocate.
constexpr int64_t GRID_DENSIFY_MAX_VOXELS = 256LL * 1024LL * 1024LL;

// ---------------------------------------------------------------------------
// TBB parallelism limit: compiled into every build, but the only Rust caller
// is the test-fixtures-gated `tbb_scope` module.
// ---------------------------------------------------------------------------

/// Holds TBB to one thread for as long as it lives (tbb::global_control is
/// process-global and RAII-scoped). The value is hard-wired so no invalid
/// thread count can cross the FFI.
struct TbbParallelismLimit {
    tbb::global_control control;

    TbbParallelismLimit() : control(tbb::global_control::max_allowed_parallelism, 1) {}
};

std::unique_ptr<TbbParallelismLimit> tbb_single_thread_limit_ffi();
size_t tbb_max_allowed_parallelism_ffi();

} // namespace reify_openvdb
