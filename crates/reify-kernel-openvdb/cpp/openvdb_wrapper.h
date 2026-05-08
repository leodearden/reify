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
#include <openvdb/tools/Interpolation.h>
#include <openvdb/io/File.h>

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

} // namespace reify_openvdb
