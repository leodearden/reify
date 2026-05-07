// OpenVDB C++ wrapper implementation for the reify-kernel-openvdb cxx-bridge.
//
// All functions are in namespace reify_openvdb (matching the bridge namespace
// in src/ffi.rs). Error paths throw std::runtime_error; cxx maps these to
// Err(cxx::Exception) on the Rust side via the Result<> return types.

#include "openvdb_wrapper.h"

#include <openvdb/math/Coord.h>
#include <openvdb/math/Transform.h>
#include <openvdb/tools/MeshToVolume.h>
#include <openvdb/tools/Interpolation.h>
#include <openvdb/io/File.h>
#include <openvdb/Grid.h>

#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

namespace reify_openvdb {

// ---------------------------------------------------------------------------
// Library lifecycle
// ---------------------------------------------------------------------------

void openvdb_initialize() {
    // openvdb::initialize() is idempotent (safe to call multiple times).
    openvdb::initialize();
}

rust::String openvdb_version_string() {
    return rust::String(openvdb::getLibraryVersionString());
}

// ---------------------------------------------------------------------------
// Mesh → Volume
// ---------------------------------------------------------------------------

std::unique_ptr<OpenVdbGridHandle> mesh_to_volume_ffi(
    rust::Slice<const std::array<float, 3>> verts,
    rust::Slice<const std::array<uint32_t, 3>> tris,
    double voxel_size,
    double half_width_voxels)
{
    if (verts.empty() || tris.empty()) {
        throw std::runtime_error(
            "mesh_to_volume_ffi: mesh must have at least one vertex and one triangle");
    }

    // Convert Rust slices to openvdb point/index vectors.
    std::vector<openvdb::Vec3s> points;
    points.reserve(verts.size());
    for (const auto& v : verts) {
        points.emplace_back(v[0], v[1], v[2]);
    }

    std::vector<openvdb::Vec3I> triangles;
    triangles.reserve(tris.size());
    for (const auto& t : tris) {
        triangles.emplace_back(t[0], t[1], t[2]);
    }

    // Build a linear transform with the requested voxel size.
    openvdb::math::Transform::Ptr xform =
        openvdb::math::Transform::createLinearTransform(voxel_size);

    // meshToLevelSet handles the world→index-space conversion internally.
    // It accepts world-space Vec3s points directly, unlike the lower-level
    // meshToVolume which expects index-space points from the mesh adapter.
    openvdb::FloatGrid::Ptr grid = openvdb::tools::meshToLevelSet<openvdb::FloatGrid>(
        *xform,
        points,
        triangles,
        static_cast<float>(half_width_voxels));

    if (!grid) {
        throw std::runtime_error("mesh_to_volume_ffi: meshToVolume returned null grid");
    }

    return std::make_unique<OpenVdbGridHandle>(std::move(grid));
}

// ---------------------------------------------------------------------------
// Grid queries
// ---------------------------------------------------------------------------

size_t grid_active_voxel_count(const OpenVdbGridHandle& h) {
    return static_cast<size_t>(h.grid->activeVoxelCount());
}

double grid_sample_sdf(const OpenVdbGridHandle& h, double x, double y, double z) {
    // BoxSampler gives trilinear interpolation of the SDF values.
    auto accessor = h.grid->getConstAccessor();
    openvdb::tools::GridSampler<
        openvdb::FloatGrid::ConstAccessor,
        openvdb::tools::BoxSampler> sampler(accessor, h.grid->transform());
    return static_cast<double>(
        sampler.wsSample(openvdb::Vec3d(x, y, z)));
}

// ---------------------------------------------------------------------------
// Grid metadata accessors
// ---------------------------------------------------------------------------

std::array<double, 3> grid_bbox_min(const OpenVdbGridHandle& h) {
    auto bbox = h.grid->evalActiveVoxelBoundingBox();
    // Convert index-space min to world space.
    openvdb::Vec3d ws = h.grid->indexToWorld(bbox.min().asVec3d());
    return {ws.x(), ws.y(), ws.z()};
}

std::array<double, 3> grid_bbox_max(const OpenVdbGridHandle& h) {
    auto bbox = h.grid->evalActiveVoxelBoundingBox();
    // Convert index-space max to world space.
    openvdb::Vec3d ws = h.grid->indexToWorld(bbox.max().asVec3d());
    return {ws.x(), ws.y(), ws.z()};
}

double grid_voxel_size(const OpenVdbGridHandle& h) {
    // Return the first diagonal element of the linear map (isotropic grids only).
    return h.grid->transform().voxelSize()[0];
}

rust::String grid_units(const OpenVdbGridHandle& h) {
    openvdb::MetaMap::ConstPtr meta = h.grid;
    openvdb::StringMetadata::ConstPtr units_meta =
        meta->getMetadata<openvdb::StringMetadata>("units");
    if (!units_meta) {
        return rust::String("");
    }
    return rust::String(units_meta->value());
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

void write_vdb_grid_ffi(
    const OpenVdbGridHandle& h,
    rust::Str path,
    rust::Str grid_name)
{
    // Set the grid name in its metadata.
    h.grid->setName(std::string(grid_name));

    openvdb::GridPtrVec grids;
    grids.push_back(h.grid);

    // Use a named string to avoid the "most vexing parse" with openvdb::io::File.
    std::string path_str{path};
    openvdb::io::File vdb_file{path_str};
    vdb_file.write(grids);
    vdb_file.close();
}

std::unique_ptr<OpenVdbGridHandle> read_vdb_grid_ffi(
    rust::Str path,
    rust::Str grid_name)
{
    std::string path_str{path};
    std::string name_str{grid_name};

    openvdb::io::File vdb_file{path_str};
    try {
        vdb_file.open();
    } catch (const openvdb::IoError& e) {
        throw std::runtime_error(
            std::string("read_vdb_grid_ffi: failed to open '") +
            path_str + "': " + e.what());
    }

    openvdb::FloatGrid::Ptr target;

    for (auto it = vdb_file.beginName(); it != vdb_file.endName(); ++it) {
        if (it.gridName() == name_str) {
            openvdb::GridBase::Ptr base = vdb_file.readGrid(it.gridName());
            target = openvdb::gridPtrCast<openvdb::FloatGrid>(base);
            if (!target) {
                throw std::runtime_error(
                    std::string("read_vdb_grid_ffi: grid '") + name_str +
                    "' in '" + path_str + "' is not a FloatGrid");
            }
            break;
        }
    }
    vdb_file.close();

    if (!target) {
        throw std::runtime_error(
            std::string("read_vdb_grid_ffi: grid '") + name_str +
            "' not found in '" + path_str + "'");
    }

    return std::make_unique<OpenVdbGridHandle>(std::move(target));
}

rust::Vec<float> grid_densify_to_buffer(const OpenVdbGridHandle& h) {
    auto bbox = h.grid->evalActiveVoxelBoundingBox();
    if (bbox.empty()) {
        return rust::Vec<float>{};
    }

    auto min = bbox.min();
    auto max = bbox.max();
    int64_t nx = static_cast<int64_t>(max.x()) - min.x() + 1;
    int64_t ny = static_cast<int64_t>(max.y()) - min.y() + 1;
    int64_t nz = static_cast<int64_t>(max.z()) - min.z() + 1;

    rust::Vec<float> buf;
    buf.reserve(static_cast<size_t>(nx * ny * nz));

    auto accessor = h.grid->getConstAccessor();
    for (int64_t iz = 0; iz < nz; ++iz) {
        for (int64_t iy = 0; iy < ny; ++iy) {
            for (int64_t ix = 0; ix < nx; ++ix) {
                openvdb::Coord coord(
                    static_cast<int>(min.x() + ix),
                    static_cast<int>(min.y() + iy),
                    static_cast<int>(min.z() + iz));
                buf.push_back(accessor.getValue(coord));
            }
        }
    }

    return buf;
}

std::array<uint64_t, 3> grid_active_bbox_dims(const OpenVdbGridHandle& h) {
    auto bbox = h.grid->evalActiveVoxelBoundingBox();
    if (bbox.empty()) {
        return {0, 0, 0};
    }
    auto min = bbox.min();
    auto max = bbox.max();
    return {
        static_cast<uint64_t>(max.x() - min.x() + 1),
        static_cast<uint64_t>(max.y() - min.y() + 1),
        static_cast<uint64_t>(max.z() - min.z() + 1)
    };
}

} // namespace reify_openvdb
