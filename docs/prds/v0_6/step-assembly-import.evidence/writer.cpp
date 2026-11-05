// writer.cpp — build a test STEP assembly via XDE and write it out.
//
// Usage: ./writer                     writes test_assembly.step (the Container assembly)
//        ./writer --rotated <out.step> writes the rotated-occurrence assembly (Frame)
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <TopoDS_Compound.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <gp_Ax1.hxx>
#include <gp_Dir.hxx>
#include <gp_Pln.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_StepModelType.hxx>
#include <TCollection_ExtendedString.hxx>
#include <Interface_Static.hxx>
#include <cmath>
#include <cstring>
#include <iostream>
#include <string>

static TopoDS_Shape makeBoxMM(double dx, double dy, double dz)
{
    return BRepPrimAPI_MakeBox(dx, dy, dz).Shape();
}

static TopLoc_Location locAt(double x, double y, double z)
{
    gp_Trsf t;
    t.SetTranslation(gp_Vec(x, y, z));
    return TopLoc_Location(t);
}

// Rotate `deg` degrees about `axis` through the origin, then translate by (x, y, z).
static TopLoc_Location locRot(const gp_Dir& axis, double deg, double x, double y, double z)
{
    gp_Trsf rotation;
    rotation.SetRotation(gp_Ax1(gp_Pnt(0.0, 0.0, 0.0), axis), deg * M_PI / 180.0);
    gp_Trsf translation;
    translation.SetTranslation(gp_Vec(x, y, z));
    return TopLoc_Location(translation * rotation);
}

static TopLoc_Location locRotZ(double deg, double x, double y, double z)
{
    return locRot(gp_Dir(0.0, 0.0, 1.0), deg, x, y, z);
}

static void setName(const TDF_Label& label, const char* name)
{
    TDataStd_Name::Set(label, TCollection_ExtendedString(name));
}

static Handle(TDocStd_Document) newXcafDocument()
{
    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    Handle(TDocStd_Document) doc;
    app->NewDocument("MDTV-XCAF", doc);
    return doc;
}

static int writeStepMM(const Handle(TDocStd_Document)& doc, const char* path)
{
    // Ensure STEP writer emits millimetres explicitly (file's own unit declaration).
    Interface_Static::SetCVal("write.step.unit", "MM");

    STEPCAFControl_Writer writer;
    writer.SetNameMode(true);
    if (!writer.Transfer(doc, STEPControl_AsIs))
    {
        std::cerr << "Transfer FAILED\n";
        return 1;
    }
    IFSelect_ReturnStatus stat = writer.Write(path);
    if (stat != IFSelect_RetDone)
    {
        std::cerr << "Write FAILED, status=" << (int)stat << "\n";
        return 1;
    }
    std::cout << "Wrote " << path << " OK\n";
    return 0;
}

static int writeContainerAssembly()
{
    Handle(TDocStd_Document) doc = newXcafDocument();

    Handle(XCAFDoc_ShapeTool) shapeTool = XCAFDoc_DocumentTool::ShapeTool(doc->Main());

    // --- Product: CornerCasting (178x162x118 mm box), shared across 4 occurrences ---
    TopoDS_Shape cornerShape = makeBoxMM(178.0, 162.0, 118.0);
    TDF_Label cornerProductLabel = shapeTool->AddShape(cornerShape, /*makeAssembly*/false);
    TDataStd_Name::Set(cornerProductLabel, TCollection_ExtendedString("CornerCasting"));

    // --- Product: Panel (thin box) and Rail (box), children of subassembly SideWall ---
    TopoDS_Shape panelShape = makeBoxMM(400.0, 5.0, 300.0);
    TDF_Label panelProductLabel = shapeTool->AddShape(panelShape, false);
    TDataStd_Name::Set(panelProductLabel, TCollection_ExtendedString("Panel"));

    TopoDS_Shape railShape = makeBoxMM(400.0, 20.0, 20.0);
    TDF_Label railProductLabel = shapeTool->AddShape(railShape, false);
    TDataStd_Name::Set(railProductLabel, TCollection_ExtendedString("Rail"));

    // --- Subassembly product: SideWall, containing Panel + Rail as components ---
    TDF_Label sideWallLabel = shapeTool->NewShape();
    TDataStd_Name::Set(sideWallLabel, TCollection_ExtendedString("SideWall"));
    TDF_Label panelCompLabel = shapeTool->AddComponent(sideWallLabel, panelProductLabel, locAt(0, 0, 0));
    TDataStd_Name::Set(panelCompLabel, TCollection_ExtendedString("Panel-1"));
    TDF_Label railCompLabel = shapeTool->AddComponent(sideWallLabel, railProductLabel, locAt(0, 300.0, 0));
    TDataStd_Name::Set(railCompLabel, TCollection_ExtendedString("Rail-1"));

    // --- Product: Weldment — a compound of TWO disjoint solids (multi-body) ---
    TopoDS_Shape weldSolidA = makeBoxMM(50.0, 50.0, 50.0);
    TopoDS_Shape weldSolidB = makeBoxMM(30.0, 30.0, 30.0);
    {
        gp_Trsf t;
        t.SetTranslation(gp_Vec(200.0, 0.0, 0.0));
        BRepBuilderAPI_Transform xform(weldSolidB, t, true);
        weldSolidB = xform.Shape();
    }
    TopoDS_Compound weldCompound;
    BRep_Builder bb;
    bb.MakeCompound(weldCompound);
    bb.Add(weldCompound, weldSolidA);
    bb.Add(weldCompound, weldSolidB);
    TDF_Label weldmentProductLabel = shapeTool->AddShape(weldCompound, false);
    TDataStd_Name::Set(weldmentProductLabel, TCollection_ExtendedString("Weldment"));

    // --- Root assembly: Container ---
    TDF_Label containerLabel = shapeTool->NewShape();
    TDataStd_Name::Set(containerLabel, TCollection_ExtendedString("Container"));

    // 4 occurrences of CornerCasting at 4 distinct locations
    double cornerPositions[4][3] = {
        {0.0, 0.0, 0.0},
        {1000.0, 0.0, 0.0},
        {0.0, 800.0, 0.0},
        {1000.0, 800.0, 0.0},
    };
    for (int i = 0; i < 4; ++i)
    {
        TDF_Label compLabel = shapeTool->AddComponent(
            containerLabel, cornerProductLabel,
            locAt(cornerPositions[i][0], cornerPositions[i][1], cornerPositions[i][2]));
        std::string nm = "CornerCasting-" + std::to_string(i + 1);
        TDataStd_Name::Set(compLabel, TCollection_ExtendedString(nm.c_str()));
    }

    // 1 occurrence of subassembly SideWall
    TDF_Label sideWallCompLabel = shapeTool->AddComponent(containerLabel, sideWallLabel, locAt(0.0, 0.0, 500.0));
    TDataStd_Name::Set(sideWallCompLabel, TCollection_ExtendedString("SideWall-1"));

    // 1 occurrence of Weldment
    TDF_Label weldmentCompLabel = shapeTool->AddComponent(containerLabel, weldmentProductLabel, locAt(500.0, 500.0, 0.0));
    TDataStd_Name::Set(weldmentCompLabel, TCollection_ExtendedString("Weldment-1"));

    shapeTool->UpdateAssemblies();
    return writeStepMM(doc, "test_assembly.step");
}

// Rotated occurrences (incl. a rotated sub-assembly), two distinct products both
// named "Pin", a zero-solid (face-only) product, and an unreferenced second root.
static int writeRotatedAssembly(const char* path)
{
    Handle(TDocStd_Document) doc = newXcafDocument();
    Handle(XCAFDoc_ShapeTool) shapeTool = XCAFDoc_DocumentTool::ShapeTool(doc->Main());

    // Asymmetric so a 90 deg turn is visible in a placed bbox.
    TDF_Label bracketLabel = shapeTool->AddShape(makeBoxMM(100.0, 50.0, 20.0), false);
    setName(bracketLabel, "Bracket");

    TDF_Label pinALabel = shapeTool->AddShape(makeBoxMM(10.0, 10.0, 60.0), false);
    setName(pinALabel, "Pin");

    TDF_Label hingeLabel = shapeTool->NewShape();
    setName(hingeLabel, "Hinge");
    setName(shapeTool->AddComponent(hingeLabel, pinALabel, locRotZ(45.0, 5.0, 0.0, 0.0)), "Pin-1");

    TDF_Label frameLabel = shapeTool->NewShape();
    setName(frameLabel, "Frame");
    setName(shapeTool->AddComponent(frameLabel, bracketLabel, locRotZ(90.0, 200.0, 0.0, 0.0)), "Bracket-1");
    setName(shapeTool->AddComponent(frameLabel, hingeLabel,
                                    locRot(gp_Dir(1.0, 0.0, 0.0), 30.0, 0.0, 100.0, 50.0)),
            "Hinge-1");

    // A SECOND, distinct product that is also named "Pin".
    TDF_Label pinBLabel = shapeTool->AddShape(makeBoxMM(8.0, 8.0, 40.0), false);
    setName(pinBLabel, "Pin");
    setName(shapeTool->AddComponent(frameLabel, pinBLabel, locAt(300.0, 300.0, 0.0)), "Pin-2");

    // A product with no solids: one planar face.
    TopoDS_Shape face =
        BRepBuilderAPI_MakeFace(gp_Pln(gp_Pnt(0.0, 0.0, 0.0), gp_Dir(0.0, 0.0, 1.0)), 0.0, 50.0, 0.0, 30.0).Shape();
    TDF_Label labelLabel = shapeTool->AddShape(face, false);
    setName(labelLabel, "Label");
    setName(shapeTool->AddComponent(frameLabel, labelLabel, locAt(0.0, 0.0, 200.0)), "Label-1");

    // Referenced by no component, so it reads back as a second free root.
    TDF_Label spareLabel = shapeTool->AddShape(makeBoxMM(20.0, 20.0, 20.0), false);
    setName(spareLabel, "Spare");

    shapeTool->UpdateAssemblies();
    return writeStepMM(doc, path);
}

int main(int argc, char** argv)
{
    if (argc == 1)
        return writeContainerAssembly();
    if (argc == 3 && std::strcmp(argv[1], "--rotated") == 0)
        return writeRotatedAssembly(argv[2]);
    std::cerr << "usage: " << argv[0] << " [--rotated <out.step>]\n";
    return 2;
}
