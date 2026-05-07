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
double grid_voxel_size(const OpenVdbGridHandle& h);
rust::String grid_units(const OpenVdbGridHandle& h);

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

rust::Vec<float> grid_densify_to_buffer(const OpenVdbGridHandle& h);
std::array<uint64_t, 3> grid_active_bbox_dims(const OpenVdbGridHandle& h);

} // namespace reify_openvdb
