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

namespace {

// Defensive initialise-once-on-first-call helper invoked at the head of every
// public FFI entry point that touches the OpenVDB library state (mesh adapter,
// I/O dispatch table, transform tables). External callers that reach the
// `ffi::ffi::*` cxx-bridge functions WITHOUT first constructing an
// `OpenVdbKernel` (which calls `crate::init::ensure_initialized`) would
// otherwise hit OpenVDB's I/O dispatch table missing the FloatGrid
// registration, producing a misleading "is not a FloatGrid" error from
// `read_vdb_grid_ffi`'s `gridPtrCast`. `openvdb::initialize` is idempotent
// per OpenVDB's documented contract so the cost of double-calling from
// kernel-mediated paths is one atomic-flag check.
void ensure_initialized_cpp_side() {
    openvdb::initialize();
}

} // namespace

// ---------------------------------------------------------------------------
// Mesh → Volume
// ---------------------------------------------------------------------------

std::unique_ptr<OpenVdbGridHandle> mesh_to_volume_ffi(
    rust::Slice<const std::array<float, 3>> verts,
    rust::Slice<const std::array<uint32_t, 3>> tris,
    double voxel_size,
    double half_width_voxels)
{
    // Defensive: callers that reach this FFI entry point without going
    // through `OpenVdbKernel::new()` (e.g. direct ffi::ffi::* bridge use
    // from a test or external crate) still get a registered I/O dispatch
    // table. Idempotent per OpenVDB's contract.
    ensure_initialized_cpp_side();

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

std::array<double, 3> grid_voxel_sizes(const OpenVdbGridHandle& h) {
    // Returns the diagonal of the grid's linear transform (per-axis voxel
    // size). For an isotropic grid (the meshToVolume default) all three are
    // equal; for an external .vdb with an anisotropic transform they differ.
    // The Rust caller is responsible for either enforcing isotropy or
    // propagating per-axis spacing into the SampledField.
    openvdb::Vec3d vs = h.grid->transform().voxelSize();
    return {vs.x(), vs.y(), vs.z()};
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

rust::String grid_name(const OpenVdbGridHandle& h) {
    // `Grid::getName()` returns the cached `MetaMap`-backed string by const
    // reference — pure read, no lazy init, no internal mutation. Safe to
    // call from `&self` on the Rust side under the Sync contract (see
    // src/kernel_real.rs:220-260's audited `&self` accessor list).
    return rust::String(h.grid->getName());
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

void write_vdb_grid_ffi(
    const OpenVdbGridHandle& h,
    rust::Str path,
    rust::Str grid_name)
{
    // Defensive: see comment on `ensure_initialized_cpp_side` near the top
    // of this file. `openvdb::io::File::write` registers grid types lazily
    // on the dispatch table so a never-initialised process can write to a
    // file but readers will reject the result.
    ensure_initialized_cpp_side();

    // Deep-copy the FloatGrid before mutating its metadata. This preserves
    // the `&self` contract of the Rust `OpenVdbKernel::write_vdb_grid`
    // entry point: the registered handle's FloatGrid is NEVER mutated by
    // a write call, so the function is sound to call concurrently from
    // threads holding `&OpenVdbKernel` (the `Sync` maintenance contract
    // in `src/kernel_real.rs:220-260` lists `write_vdb_grid` under the
    // audited `&self` methods).
    //
    // `FloatGrid::deepCopy()` returns a new `FloatGrid::Ptr` whose tree
    // shares no nodes with the original — topology, transform, metadata,
    // and active-voxel-count round-trip exactly, so downstream consumers
    // (tests, importers) see equivalent geometry. Cost is one full tree
    // walk plus copy; for v0.4 shells use-cases (sub-million-voxel narrow
    // bands) this is well under the I/O cost.
    auto grid_copy = h.grid->deepCopy();
    grid_copy->setName(std::string(grid_name));

    openvdb::GridPtrVec grids;
    grids.push_back(grid_copy);

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
    // Defensive: callers reaching this FFI entry directly bypass the
    // `crate::init::ensure_initialized` OnceLock guard upstream. Without
    // OpenVDB's I/O dispatch table populated, `gridPtrCast<FloatGrid>`
    // returns null and the wrong-type branch below fires with a
    // misleading message.
    ensure_initialized_cpp_side();

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
    bool found = false;
    bool wrong_type = false;

    for (auto it = vdb_file.beginName(); it != vdb_file.endName(); ++it) {
        if (it.gridName() == name_str) {
            found = true;
            openvdb::GridBase::Ptr base = vdb_file.readGrid(it.gridName());
            target = openvdb::gridPtrCast<openvdb::FloatGrid>(base);
            if (!target) {
                wrong_type = true;
            }
            break;
        }
    }
    // Close on every exit path — whether the grid was found, missing, or had
    // the wrong type. Earlier revisions threw inside the loop on the
    // wrong-type path, leaving close() unreached (relying on the stack
    // destructor instead). The unconditional close keeps the cleanup symmetric
    // with the not-found path and makes future edits next to the file safer.
    vdb_file.close();

    if (wrong_type) {
        throw std::runtime_error(
            std::string("read_vdb_grid_ffi: grid '") + name_str +
            "' in '" + path_str + "' is not a FloatGrid");
    }
    if (!found || !target) {
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

    // Defence-in-depth against a vendor-defined `bbox.empty()` heuristic
    // that admits a per-axis `max < min` while still reporting non-empty.
    // Without this guard, a non-positive `nx`/`ny`/`nz` would slip through
    // the GRID_DENSIFY_MAX_VOXELS check (a positive budget divided by a
    // non-positive divisor compares trivially) and lead to
    // `buf.reserve(static_cast<size_t>(<negative>))` — a huge allocation
    // on most platforms. See task 3095 review esc-3095-97 suggestion 4.
    if (nx <= 0 || ny <= 0 || nz <= 0) {
        throw std::runtime_error(
            std::string("grid_densify_to_buffer: malformed bbox: nx=") +
            std::to_string(nx) + ", ny=" + std::to_string(ny) +
            ", nz=" + std::to_string(nz) + " (each must be > 0)");
    }

    // Cap densified-buffer size to GRID_DENSIFY_MAX_VOXELS to prevent OOM
    // on oversized .vdb files. The check is performed in int64 arithmetic
    // so a multiplication overflow on a malformed bbox cannot bypass the
    // cap by wrapping to a small positive value. (Each operand is bounded
    // by the OpenVDB Coord domain (~2e9), so nx*ny is bounded by ~4e18,
    // still within int64 range; the cumulative product can overflow but
    // the early cap on nx*ny catches it before nz multiplies in.)
    int64_t total_voxels = nx;
    if (ny != 0 && total_voxels > GRID_DENSIFY_MAX_VOXELS / ny) {
        throw std::runtime_error(
            std::string("grid_densify_to_buffer: grid too large: nx*ny=") +
            std::to_string(nx * ny) + " exceeds budget " +
            std::to_string(GRID_DENSIFY_MAX_VOXELS));
    }
    total_voxels *= ny;
    if (nz != 0 && total_voxels > GRID_DENSIFY_MAX_VOXELS / nz) {
        throw std::runtime_error(
            std::string("grid_densify_to_buffer: grid too large: ") +
            "nx*ny*nz exceeds budget " +
            std::to_string(GRID_DENSIFY_MAX_VOXELS) + " (nx=" +
            std::to_string(nx) + ", ny=" + std::to_string(ny) +
            ", nz=" + std::to_string(nz) + ")");
    }
    total_voxels *= nz;

    rust::Vec<float> buf;
    buf.reserve(static_cast<size_t>(total_voxels));

    // Row-major axis-0 (X) outermost layout: buf[ix * ny * nz + iy * nz + iz]
    // contains the value at world-space coord (min.x()+ix, min.y()+iy, min.z()+iz).
    //
    // This matches the convention used by reify_expr::interp::interpolate_3d
    // (which expects values[ix * ny * nz + iy * nz + iz]) and the wider
    // workspace's row-major-axis-0-outermost convention documented in
    // reify-expr/src/{field_reductions.rs:255-287, sampled.rs:106-114,
    // interp.rs:7+377} and engine_eval::build_sampled_field.
    auto accessor = h.grid->getConstAccessor();
    for (int64_t ix = 0; ix < nx; ++ix) {
        for (int64_t iy = 0; iy < ny; ++iy) {
            for (int64_t iz = 0; iz < nz; ++iz) {
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

} // namespace reify_openvdb
