// Include the cxx-generated header first for shared type definitions (Point3, BBox, TessResult)
#include "reify-kernel-occt/src/ffi.rs.h"
#include "occt_wrapper.h"

// OCCT primitives
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>

// OCCT booleans
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Common.hxx>

// OCCT fillet
#include <BRepFilletAPI_MakeFillet.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>

// OCCT transforms
#include <BRepBuilderAPI_Transform.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>

// OCCT properties
#include <BRepGProp.hxx>
#include <GProp_GProps.hxx>
#include <Bnd_Box.hxx>
#include <BRepBndLib.hxx>

// OCCT STEP export
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>

// OCCT BRep serialization
#include <BRepTools.hxx>
#include <BRep_Builder.hxx>

// OCCT mesh
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <TopLoc_Location.hxx>

#include <sstream>
#include <fstream>
#include <cstdio>
#include <mutex>
#include <stdexcept>

namespace occt {

// --- Primitive construction ---

std::unique_ptr<OcctShape> make_box(double width, double height, double depth) {
    try {
        gp_Pnt corner(-width / 2.0, -height / 2.0, -depth / 2.0);
        BRepPrimAPI_MakeBox maker(corner, width, height, depth);
        maker.Build();
        if (!maker.IsDone()) {
            throw std::runtime_error("BRepPrimAPI_MakeBox failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = maker.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT make_box: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT make_box: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT make_box: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> make_cylinder(double radius, double height) {
    try {
        BRepPrimAPI_MakeCylinder maker(radius, height);
        maker.Build();
        if (!maker.IsDone()) {
            throw std::runtime_error("BRepPrimAPI_MakeCylinder failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = maker.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT make_cylinder: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT make_cylinder: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT make_cylinder: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> make_sphere(double radius) {
    try {
        BRepPrimAPI_MakeSphere maker(radius);
        maker.Build();
        if (!maker.IsDone()) {
            throw std::runtime_error("BRepPrimAPI_MakeSphere failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = maker.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT make_sphere: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT make_sphere: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT make_sphere: unknown C++ exception");
    }
}

// --- Boolean operations ---

std::unique_ptr<OcctShape> boolean_fuse(const OcctShape& left, const OcctShape& right) {
    try {
        BRepAlgoAPI_Fuse fuse(left.shape, right.shape);
        fuse.Build();
        if (!fuse.IsDone()) {
            throw std::runtime_error("BRepAlgoAPI_Fuse failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = fuse.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT boolean_fuse: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT boolean_fuse: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT boolean_fuse: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> boolean_cut(const OcctShape& left, const OcctShape& right) {
    try {
        BRepAlgoAPI_Cut cut(left.shape, right.shape);
        cut.Build();
        if (!cut.IsDone()) {
            throw std::runtime_error("BRepAlgoAPI_Cut failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = cut.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT boolean_cut: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT boolean_cut: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT boolean_cut: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> boolean_common(const OcctShape& left, const OcctShape& right) {
    try {
        BRepAlgoAPI_Common common(left.shape, right.shape);
        common.Build();
        if (!common.IsDone()) {
            throw std::runtime_error("BRepAlgoAPI_Common failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = common.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT boolean_common: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT boolean_common: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT boolean_common: unknown C++ exception");
    }
}

// --- Modifications ---

std::unique_ptr<OcctShape> fillet_all_edges(const OcctShape& shape, double radius) {
    try {
        BRepFilletAPI_MakeFillet fillet(shape.shape);
        for (TopExp_Explorer ex(shape.shape, TopAbs_EDGE); ex.More(); ex.Next()) {
            fillet.Add(radius, TopoDS::Edge(ex.Current()));
        }
        fillet.Build();
        if (!fillet.IsDone()) {
            throw std::runtime_error("BRepFilletAPI_MakeFillet failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = fillet.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT fillet_all_edges: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT fillet_all_edges: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT fillet_all_edges: unknown C++ exception");
    }
}

// --- Transforms ---

std::unique_ptr<OcctShape> translate_shape(const OcctShape& shape, double dx, double dy, double dz) {
    try {
        gp_Trsf trsf;
        trsf.SetTranslation(gp_Vec(dx, dy, dz));
        BRepBuilderAPI_Transform transform(shape.shape, trsf, true);
        transform.Build();
        if (!transform.IsDone()) {
            throw std::runtime_error("BRepBuilderAPI_Transform (translate) failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = transform.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT translate_shape: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT translate_shape: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT translate_shape: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> rotate_shape(const OcctShape& shape, double ax, double ay, double az, double angle_rad) {
    try {
        gp_Ax1 axis(gp_Pnt(0, 0, 0), gp_Dir(ax, ay, az));
        gp_Trsf trsf;
        trsf.SetRotation(axis, angle_rad);
        BRepBuilderAPI_Transform transform(shape.shape, trsf, true);
        transform.Build();
        if (!transform.IsDone()) {
            throw std::runtime_error("BRepBuilderAPI_Transform (rotate) failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = transform.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT rotate_shape: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT rotate_shape: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT rotate_shape: unknown C++ exception");
    }
}

// --- Mirror / Pattern ---

std::unique_ptr<OcctShape> mirror_shape(const OcctShape& shape,
    double ox, double oy, double oz,
    double nx, double ny, double nz) {
    try {
        gp_Ax2 mirror_plane(gp_Pnt(ox, oy, oz), gp_Dir(nx, ny, nz));
        gp_Trsf trsf;
        trsf.SetMirror(mirror_plane);
        BRepBuilderAPI_Transform transform(shape.shape, trsf, true);
        transform.Build();
        if (!transform.IsDone()) {
            throw std::runtime_error("BRepBuilderAPI_Transform (mirror) failed");
        }
        auto result = std::make_unique<OcctShape>();
        result->shape = transform.Shape();
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT mirror_shape: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT mirror_shape: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT mirror_shape: unknown C++ exception");
    }
}

// --- Queries ---

double query_volume(const OcctShape& shape) {
    try {
        GProp_GProps props;
        BRepGProp::VolumeProperties(shape.shape, props);
        return props.Mass();
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT query_volume: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT query_volume: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT query_volume: unknown C++ exception");
    }
}

double query_area(const OcctShape& shape) {
    try {
        GProp_GProps props;
        BRepGProp::SurfaceProperties(shape.shape, props);
        return props.Mass();
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT query_area: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT query_area: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT query_area: unknown C++ exception");
    }
}

Point3 query_centroid(const OcctShape& shape) {
    try {
        GProp_GProps props;
        BRepGProp::VolumeProperties(shape.shape, props);
        gp_Pnt c = props.CentreOfMass();
        return Point3{c.X(), c.Y(), c.Z()};
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT query_centroid: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT query_centroid: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT query_centroid: unknown C++ exception");
    }
}

BBox query_bbox(const OcctShape& shape) {
    try {
        Bnd_Box box;
        BRepBndLib::Add(shape.shape, box);
        double xmin, ymin, zmin, xmax, ymax, zmax;
        box.Get(xmin, ymin, zmin, xmax, ymax, zmax);
        return BBox{xmin, ymin, zmin, xmax, ymax, zmax};
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT query_bbox: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT query_bbox: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT query_bbox: unknown C++ exception");
    }
}

// --- Export ---

// Process-global mutex for STEP export. OCCT's STEPControl_Writer (and its
// Transfer/Write pipeline) uses process-global state (XSAlgo session, interface
// model, shape naming tables) that is not thread-safe. This mutex serializes
// all concurrent export_step() calls across all kernel threads.
static std::mutex g_step_export_mutex;

rust::String export_step(const OcctShape& shape) {
    std::lock_guard<std::mutex> lock(g_step_export_mutex);
    try {
        STEPControl_Writer writer;
        writer.Transfer(shape.shape, STEPControl_AsIs);

        // Write to a temporary file, then read back
        char tmpname[] = "/tmp/reify_step_XXXXXX";
        int fd = mkstemp(tmpname);
        if (fd < 0) {
            throw std::runtime_error("Failed to create temp file for STEP export");
        }
        close(fd);

        IFSelect_ReturnStatus status = writer.Write(tmpname);
        if (status != IFSelect_RetDone) {
            std::remove(tmpname);
            throw std::runtime_error("STEPControl_Writer::Write failed");
        }

        std::ifstream ifs(tmpname);
        std::string content((std::istreambuf_iterator<char>(ifs)),
                            std::istreambuf_iterator<char>());
        ifs.close();
        std::remove(tmpname);

        return rust::String(content);
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT export_step: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT export_step: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT export_step: unknown C++ exception");
    }
}

// --- BRep serialization ---

rust::String serialize_brep(const OcctShape& shape) {
    try {
        std::ostringstream oss;
        ::BRepTools::Write(shape.shape, oss);
        std::string content = oss.str();
        if (content.empty()) {
            throw std::runtime_error("BRepTools::Write produced empty output");
        }
        return rust::String(content);
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT serialize_brep: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT serialize_brep: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT serialize_brep: unknown C++ exception");
    }
}

std::unique_ptr<OcctShape> deserialize_brep(const std::string& data) {
    try {
        ::BRep_Builder builder;
        auto result = std::make_unique<OcctShape>();
        std::istringstream iss(data);
        ::BRepTools::Read(result->shape, iss, builder);
        if (result->shape.IsNull()) {
            throw std::runtime_error("BRepTools::Read produced null shape");
        }
        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT deserialize_brep: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT deserialize_brep: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT deserialize_brep: unknown C++ exception");
    }
}

// --- Tessellation ---

TessResult tessellate_shape(const OcctShape& shape, double tolerance) {
    try {
        BRepMesh_IncrementalMesh mesh(shape.shape, tolerance);
        mesh.Perform();
        if (!mesh.IsDone()) {
            throw std::runtime_error("BRepMesh_IncrementalMesh failed");
        }

        TessResult result;
        uint32_t vertex_offset = 0;

        for (TopExp_Explorer ex(shape.shape, TopAbs_FACE); ex.More(); ex.Next()) {
            TopoDS_Face face = TopoDS::Face(ex.Current());
            TopLoc_Location loc;
            Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
            if (tri.IsNull()) continue;

            int nb_nodes = tri->NbNodes();
            int nb_tris = tri->NbTriangles();

            // Extract vertices
            for (int i = 1; i <= nb_nodes; ++i) {
                gp_Pnt p = tri->Node(i);
                if (!loc.IsIdentity()) {
                    p.Transform(loc.Transformation());
                }
                result.vertices.push_back(static_cast<float>(p.X()));
                result.vertices.push_back(static_cast<float>(p.Y()));
                result.vertices.push_back(static_cast<float>(p.Z()));
            }

            // Extract normals — use stored normals if available, else compute from triangles
            if (tri->HasNormals()) {
                for (int i = 1; i <= nb_nodes; ++i) {
                    gp_Dir n = tri->Normal(i);
                    if (!loc.IsIdentity()) {
                        n.Transform(loc.Transformation());
                    }
                    result.normals.push_back(static_cast<float>(n.X()));
                    result.normals.push_back(static_cast<float>(n.Y()));
                    result.normals.push_back(static_cast<float>(n.Z()));
                }
            } else {
                // Compute per-vertex normals by averaging face normals
                std::vector<gp_Vec> vertex_normals(nb_nodes, gp_Vec(0, 0, 0));
                for (int i = 1; i <= nb_tris; ++i) {
                    int n1, n2, n3;
                    tri->Triangle(i).Get(n1, n2, n3);
                    gp_Pnt p1 = tri->Node(n1);
                    gp_Pnt p2 = tri->Node(n2);
                    gp_Pnt p3 = tri->Node(n3);
                    gp_Vec v1(p1, p2);
                    gp_Vec v2(p1, p3);
                    gp_Vec face_normal = v1.Crossed(v2);
                    vertex_normals[n1 - 1] += face_normal;
                    vertex_normals[n2 - 1] += face_normal;
                    vertex_normals[n3 - 1] += face_normal;
                }
                for (int i = 0; i < nb_nodes; ++i) {
                    gp_Vec n = vertex_normals[i];
                    double mag = n.Magnitude();
                    if (mag > 1e-10) {
                        n /= mag;
                    }
                    if (!loc.IsIdentity()) {
                        n.Transform(loc.Transformation());
                    }
                    result.normals.push_back(static_cast<float>(n.X()));
                    result.normals.push_back(static_cast<float>(n.Y()));
                    result.normals.push_back(static_cast<float>(n.Z()));
                }
            }

            // Extract indices (1-based → 0-based + offset)
            for (int i = 1; i <= nb_tris; ++i) {
                int n1, n2, n3;
                tri->Triangle(i).Get(n1, n2, n3);
                result.indices.push_back(vertex_offset + static_cast<uint32_t>(n1 - 1));
                result.indices.push_back(vertex_offset + static_cast<uint32_t>(n2 - 1));
                result.indices.push_back(vertex_offset + static_cast<uint32_t>(n3 - 1));
            }

            vertex_offset += static_cast<uint32_t>(nb_nodes);
        }

        return result;
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT tessellate_shape: ") + e.GetMessageString());
    } catch (std::exception const& e) {
        throw std::runtime_error(std::string("OCCT tessellate_shape: unexpected: ") + e.what());
    } catch (...) {
        throw std::runtime_error("OCCT tessellate_shape: unknown C++ exception");
    }
}

} // namespace occt
