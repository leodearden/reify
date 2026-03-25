#pragma once
#include "rust/cxx.h"
#include <TopoDS_Shape.hxx>
#include <memory>
#include <vector>

namespace occt {

/// Opaque wrapper around TopoDS_Shape for crossing the FFI boundary.
struct OcctShape {
    TopoDS_Shape shape;
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

/// Return the number of shapes in the vector.
size_t shape_vec_len(const OcctShapeVec& vec);

// Shared types — defined by cxx bridge. Forward-declared here for function signatures.
struct Point3;
struct BBox;
struct TessResult;

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

// --- Transforms ---

std::unique_ptr<OcctShape> translate_shape(const OcctShape& shape, double dx, double dy, double dz);
std::unique_ptr<OcctShape> rotate_shape(const OcctShape& shape, double ax, double ay, double az, double angle_rad);

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

/// Create a flat circular face (disk) at a given Z height (for sweep/extrude profiles).
std::unique_ptr<OcctShape> make_circle_face(double radius, double z_height);

/// Create a straight line wire between two 3D points (for sweep paths).
std::unique_ptr<OcctShape> make_line_wire(double x1, double y1, double z1,
    double x2, double y2, double z2);

/// Loft through N wire profiles (N >= 2) to create a solid.
std::unique_ptr<OcctShape> loft_profiles(const OcctShapeVec& profiles);

// --- Sweep ---

/// Sweep a profile along a wire path (BRepOffsetAPI_MakePipe).
std::unique_ptr<OcctShape> make_pipe(const OcctShape& profile, const OcctShape& spine);

// --- Queries ---

double query_volume(const OcctShape& shape);
double query_area(const OcctShape& shape);
Point3 query_centroid(const OcctShape& shape);
BBox query_bbox(const OcctShape& shape);

double query_distance(const OcctShape& shape1, const OcctShape& shape2);
double query_moment_of_inertia(const OcctShape& shape, double ax, double ay, double az);

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
