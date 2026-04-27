#pragma once
#include "rust/cxx.h"
#include <TopoDS_Shape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <cstdint>
#include <memory>
#include <vector>

namespace occt {

/// Opaque wrapper around TopoDS_Shape for crossing the FFI boundary.
///
/// IMMUTABLE POST-CONSTRUCTION INVARIANT: Once `shape` is assigned (e.g.
/// `result->shape = maker.Shape()`), no FFI operation mutates it in place.
/// Every "modification" (translate, boolean, fillet, etc.) returns a fresh
/// OcctShape. The three topology-map caches below are therefore safe to
/// populate lazily and never need invalidation.
///
/// THREAD-SAFETY NOTE: The cache slots are unsynchronized (`mutable` without
/// a mutex). Safety relies on OcctShape being accessed only via the
/// `!Send + !Sync` `OcctKernel`, which pins all accesses to a single thread.
/// If a future change wraps OcctShape in something `Sync`, a synchronization
/// primitive (e.g. `std::call_once`) must be added to the lazy accessors.
///
/// STRONG-EXCEPTION-GUARANTEE: Each lazy accessor builds the map into a local
/// `unique_ptr` and moves it into the cache slot only after MapShapes /
/// MapShapesAndAncestors returns successfully.  On a throw the local dies, the
/// slot stays null, and the build counter stays at 0, so the next call retries
/// cleanly and observability remains honest.  See the per-accessor comments in
/// occt_wrapper.cpp for the rationale; do NOT reorder back to the
/// assign-then-fill pattern.
struct OcctShape {
    TopoDS_Shape shape;

    // --- Lazy topology-map cache slots ---
    // Null until first use; built exactly once per shape lifetime.
    mutable std::unique_ptr<TopTools_IndexedMapOfShape> face_map_cache_;
    mutable std::unique_ptr<TopTools_IndexedMapOfShape> edge_map_cache_;
    mutable std::unique_ptr<TopTools_IndexedDataMapOfShapeListOfShape> edge_face_map_cache_;

    // Build counters: each increments exactly once, on the cache miss that
    // populates the corresponding slot. Zero-cost on cache-hit fast paths.
    mutable uint32_t face_map_builds_ = 0;
    mutable uint32_t edge_map_builds_ = 0;
    mutable uint32_t edge_face_map_builds_ = 0;

    // --- Lazy accessor methods ---
    // Each returns a const reference to the corresponding cached map, building
    // it (and incrementing the counter) on the first call only.
    const TopTools_IndexedMapOfShape& face_map() const;
    const TopTools_IndexedMapOfShape& edge_map() const;
    const TopTools_IndexedDataMapOfShapeListOfShape& edge_face_map() const;
};

/// Opaque vector of TopoDS_Shape for passing N shapes across the CXX FFI boundary.
/// Uses push/build semantics: new_shape_vec() creates, shape_vec_push() adds shapes.
struct OcctShapeVec {
    std::vector<TopoDS_Shape> shapes;
};

/// Create an empty OcctShapeVec.
std::unique_ptr<OcctShapeVec> new_shape_vec();

/// Push a shape into the vector (mutable borrow via Pin).
void shape_vec_push(OcctShapeVec& vec, const OcctShape& shape);

// Shared types — defined by cxx bridge. Forward-declared here for function signatures.
struct Point3;
struct BBox;
struct TessResult;
struct TopologyCacheBuildCounts;

// --- Primitive construction ---

/// Create a box centered at origin with given dimensions (in meters).
std::unique_ptr<OcctShape> make_box(double width, double height, double depth);

/// Create a cylinder along Z axis (in meters).
std::unique_ptr<OcctShape> make_cylinder(double radius, double height);

/// Create a sphere centered at origin (in meters).
std::unique_ptr<OcctShape> make_sphere(double radius);

// --- Boolean operations ---

std::unique_ptr<OcctShape> boolean_fuse(const OcctShape& left, const OcctShape& right);
std::unique_ptr<OcctShape> boolean_cut(const OcctShape& left, const OcctShape& right);
std::unique_ptr<OcctShape> boolean_common(const OcctShape& left, const OcctShape& right);

// --- Modifications ---

std::unique_ptr<OcctShape> fillet_all_edges(const OcctShape& shape, double radius);
std::unique_ptr<OcctShape> chamfer_all_edges(const OcctShape& shape, double distance);

// --- Transforms ---

std::unique_ptr<OcctShape> translate_shape(const OcctShape& shape, double dx, double dy, double dz);
std::unique_ptr<OcctShape> rotate_shape(const OcctShape& shape, double ax, double ay, double az, double angle_rad);
std::unique_ptr<OcctShape> scale_shape(const OcctShape& shape, double factor, double cx, double cy, double cz);
std::unique_ptr<OcctShape> rotate_around_shape(const OcctShape& shape, double px, double py, double pz, double ax, double ay, double az, double angle_rad);

// --- Mirror / Pattern ---

std::unique_ptr<OcctShape> mirror_shape(const OcctShape& shape,
    double ox, double oy, double oz,
    double nx, double ny, double nz);

std::unique_ptr<OcctShape> linear_pattern(const OcctShape& shape,
    double dx, double dy, double dz,
    uint32_t count, double spacing);

std::unique_ptr<OcctShape> circular_pattern(const OcctShape& shape,
    double ox, double oy, double oz,
    double ax, double ay, double az,
    uint32_t count, double total_angle);

std::unique_ptr<OcctShape> linear_pattern_2d(const OcctShape& shape,
    double dx1, double dy1, double dz1,
    uint32_t count1, double spacing1,
    double dx2, double dy2, double dz2,
    uint32_t count2, double spacing2);

std::unique_ptr<OcctShape> arbitrary_pattern(const OcctShape& shape,
    const rust::Vec<double>& flat_transforms, uint32_t num_transforms);

// --- Thicken / Shell ---

std::unique_ptr<OcctShape> thicken_shape(const OcctShape& shape, double offset);

std::unique_ptr<OcctShape> shell_shape(const OcctShape& shape, double thickness,
    const rust::Vec<uint32_t>& face_indices);

// --- Draft ---

std::unique_ptr<OcctShape> draft_shape(const OcctShape& shape, double angle_rad,
    const OcctShape& plane_shape);

// --- Wire helpers / Loft ---

/// Create a circular wire profile at a given Z height (for loft profiles).
std::unique_ptr<OcctShape> make_circle_wire(double radius, double z_height);

/// Create a flat circular face (disk) at a given Z height (for extrude profiles).
std::unique_ptr<OcctShape> make_circle_face(double radius, double z_height);

/// Create a straight line wire between two 3D points (for sweep paths).
std::unique_ptr<OcctShape> make_line_wire(double x1, double y1, double z1,
    double x2, double y2, double z2);

// --- Curve constructors ---

/// Create a circular arc wire.
std::unique_ptr<OcctShape> make_arc_wire(
    double cx, double cy, double cz,
    double radius,
    double start_angle, double end_angle,
    double ax, double ay, double az);

/// Create a helix wire on a cylindrical surface.
std::unique_ptr<OcctShape> make_helix_wire(
    double radius, double pitch, double height);

/// Create a polyline wire from N >= 2 points (flat coords array of 3*N doubles).
/// Produces N-1 line edges connecting consecutive points.  Stable kernel FFI
/// primitive: backing wire for planned polygon faces, multi-segment sweep/pipe
/// paths (make_pipe, make_pipe_shell), and BRepAdaptor_CompCurve composite testing.
std::unique_ptr<OcctShape> make_polyline_wire(
    rust::Slice<const double> coords, size_t n_points);

/// Create an interpolated B-spline curve through points (flat coords, n_points triples).
std::unique_ptr<OcctShape> make_interp_curve(
    rust::Slice<const double> coords, size_t n_points);

/// Create a Bézier curve from control points (flat coords, n_points triples).
std::unique_ptr<OcctShape> make_bezier_curve(
    rust::Slice<const double> coords, size_t n_points);

/// Create a NURBS (B-spline) curve from poles, weights, flat knots, and degree.
std::unique_ptr<OcctShape> make_nurbs_curve(
    rust::Slice<const double> pole_coords, size_t n_poles,
    rust::Slice<const double> weights,
    rust::Slice<const double> flat_knots,
    int degree);

/// Loft through N wire profiles (N >= 2) to create a solid.
std::unique_ptr<OcctShape> loft_profiles(const OcctShapeVec& profiles);

// --- Sweep ---

/// Sweep a profile along a wire path (BRepOffsetAPI_MakePipe).
std::unique_ptr<OcctShape> make_pipe(const OcctShape& profile, const OcctShape& spine);

/// Sweep a profile along a spine path, with an auxiliary guide wire
/// constraining orientation (BRepOffsetAPI_MakePipeShell + SetMode).
/// `spine` is the path the section follows; `guide` biases section
/// orientation via SetMode(guide, /*KeepContact=*/Standard_False).
std::unique_ptr<OcctShape> make_pipe_shell(const OcctShape& profile,
                                           const OcctShape& spine,
                                           const OcctShape& guide);

/// Loft through >= 2 section profiles along a guide wire spine, via
/// BRepOffsetAPI_MakePipeShell. The first guide is the spine; each
/// profile is added as a section via `.Add(...)`. If a second guide is
/// present, it is applied via `SetMode(aux, /*KeepContact=*/false)`
/// as an auxiliary-orientation constraint.
std::unique_ptr<OcctShape> loft_guided_profiles(const OcctShapeVec& profiles,
                                                const OcctShapeVec& guides);

// --- Sweep / Extrude / Revolve ---

/// Extrude a profile shape by a direction vector (dx, dy, dz).
/// The direction vector must have non-zero magnitude.
std::unique_ptr<OcctShape> make_prism(const OcctShape& profile, double dx, double dy, double dz);

/// Revolve a profile shape around an axis by angle_rad radians.
/// Axis defined by origin point (ox,oy,oz) and direction (ax,ay,az).
std::unique_ptr<OcctShape> make_revolve(const OcctShape& profile,
    double ox, double oy, double oz,
    double ax, double ay, double az,
    double angle_rad);

/// Create a rectangular face (planar) centered at (cx, cy, cz) with
/// given width (X direction) and height (Y direction) in the XY plane.
std::unique_ptr<OcctShape> make_rect_face(double width, double height,
    double cx, double cy, double cz);

// --- Wire queries ---

/// Return the normalised start-tangent of a wire (unit vector at the first
/// parameter of the wire's composite curve). Throws std::runtime_error if the
/// shape is not a wire or the start-tangent has zero magnitude.
Point3 wire_start_tangent(const OcctShape& wire);

// --- Queries ---

double query_volume(const OcctShape& shape);
double query_area(const OcctShape& shape);
Point3 query_centroid(const OcctShape& shape);
BBox query_bbox(const OcctShape& shape);

double query_distance(const OcctShape& shape1, const OcctShape& shape2);
double query_moment_of_inertia(const OcctShape& shape, double ax, double ay, double az);

/// Return the number of times each topology-map cache slot has been built for
/// `shape`. Each counter is 0 on a fresh shape and increments to 1 on first
/// use via the lazy accessors `face_map()`, `edge_map()`, or `edge_face_map()`.
/// Exposed as an observability hook for deterministic cache-effectiveness tests.
TopologyCacheBuildCounts topology_cache_build_counts(const OcctShape& shape);

/// Return the 0-based global indices of faces sharing at least one edge with
/// the face at `face_index`. Indices follow the canonical
/// `TopExp::MapShapes(..., TopAbs_FACE, ...)` order — a 0-based view of the
/// 1-based `TopTools_IndexedMapOfShape`, deduplicated by `TopoDS_Shape::IsSame`.
/// Excludes the queried face itself; deduplicated; returned in ascending order.
/// Throws std::runtime_error if `face_index` is out of range.
rust::Vec<uint32_t> adjacent_faces(const OcctShape& shape, uint32_t face_index);

/// Return the 0-based global indices of edges shared between the faces at
/// `face_a_index` and `face_b_index`, using `TopoDS_Shape::IsSame` for
/// matching. Indices follow the canonical
/// `TopExp::MapShapes(..., TopAbs_EDGE, ...)` order — a 0-based view of the
/// 1-based `TopTools_IndexedMapOfShape`, deduplicated by `TopoDS_Shape::IsSame`.
/// Returns an empty vector if `face_a_index == face_b_index`. Deduplicated;
/// returned in ascending order. Throws std::runtime_error if either index is
/// out of range.
rust::Vec<uint32_t> shared_edges(const OcctShape& shape, uint32_t face_a_index, uint32_t face_b_index);

// --- Conformance queries ---

/// Check whether `shape` is watertight (closed, no free edges).
///
/// Backed by `BRepCheck_Analyzer.IsValid()`. Returns `false` immediately for
/// shape types other than SOLID/COMPSOLID/SHELL: COMPOUND is excluded because
/// `IsValid()` reports topological consistency, not closure — a compound of
/// open faces can spuriously pass. FACE/WIRE/EDGE/VERTEX are also excluded
/// as they never enclose a volume. Callers testing a COMPOUND should iterate
/// its sub-shapes individually.
bool is_watertight(const OcctShape& shape);

/// Check whether every edge of `shape` has at most 2 parent faces.
///
/// Backed by walking the cached `edge_face_map` (lazy `TopExp::MapShapesAndAncestors`).
/// Returns `false` iff any edge has 3+ incident faces. Shapes with no face
/// incidence (wires, edges, vertices) trivially return `true`.
bool is_manifold(const OcctShape& shape);

/// Check whether all shells of `shape` are consistently oriented.
///
/// Backed by `ShapeAnalysis_Shell::CheckOrientedShells(shape, alsofree=Standard_False)`.
/// Returns `true` iff every connected edge has opposite (FORWARD/REVERSED)
/// orientations on its two incident faces. Shapes with no shells loaded
/// (wires, isolated faces, vertices) trivially return `true`.
bool is_orientable(const OcctShape& shape);

// --- Test fixture helpers ---
// These functions build deliberately malformed or exotic shapes that are only
// useful for conformance integration tests. They are gated by `#[cfg(has_occt)]`
// (not `cfg(test)`) in the Rust layer so that integration-test crates — which
// compile the library in normal (non-test) mode — can call them.

/// Build three planar faces sharing a common edge, assembled into a compound.
/// The shared edge has 3 incident faces, making the compound non-manifold.
std::unique_ptr<OcctShape> make_nonmanifold_compound_for_test();

/// Build a 10×10×10 mm box with one face removed, wrapped in a solid.
/// The resulting open shell causes BRepCheck_Analyzer::IsValid() to return false.
std::unique_ptr<OcctShape> make_malformed_solid_for_test();

/// Build a shell of two faces sharing a common edge, where both faces use
/// the shared edge in the same orientation — violating the consistency
/// requirement for an oriented shell.
std::unique_ptr<OcctShape> make_nonorientable_shell_for_test();

// --- Export ---

/// Export shape to STEP format, returns the STEP file content as a string.
rust::String export_step(const OcctShape& shape);

// --- BRep serialization ---

/// Serialize a shape to OCCT BRep ASCII format.
rust::String serialize_brep(const OcctShape& shape);

/// Deserialize a shape from OCCT BRep ASCII format.
std::unique_ptr<OcctShape> deserialize_brep(const std::string& data);

// --- Tessellation ---

TessResult tessellate_shape(const OcctShape& shape, double tolerance);

} // namespace occt
