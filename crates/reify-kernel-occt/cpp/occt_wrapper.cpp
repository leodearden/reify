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

// OCCT mesh
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <TopLoc_Location.hxx>

#include <sstream>
#include <fstream>
#include <cstdio>

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
    }
}

double query_area(const OcctShape& shape) {
    try {
        GProp_GProps props;
        BRepGProp::SurfaceProperties(shape.shape, props);
        return props.Mass();
    } catch (Standard_Failure const& e) {
        throw std::runtime_error(std::string("OCCT query_area: ") + e.GetMessageString());
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
    }
}

// --- Export ---

rust::String export_step(const OcctShape& shape) {
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

            // Extract normals if available
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
    }
}

} // namespace occt
