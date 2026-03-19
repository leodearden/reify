#pragma once
#include "rust/cxx.h"
#include <TopoDS_Shape.hxx>
#include <memory>

namespace occt {

/// Opaque wrapper around TopoDS_Shape for crossing the FFI boundary.
struct OcctShape {
    TopoDS_Shape shape;
};

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

// --- Queries ---

double query_volume(const OcctShape& shape);
double query_area(const OcctShape& shape);
Point3 query_centroid(const OcctShape& shape);
BBox query_bbox(const OcctShape& shape);

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
