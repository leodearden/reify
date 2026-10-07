// writer.cpp — build a test STEP assembly via XDE and write it out.
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <TopoDS_Compound.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_StepModelType.hxx>
#include <TCollection_ExtendedString.hxx>
#include <Interface_Static.hxx>
#include <iostream>

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

int main()
{
    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    Handle(TDocStd_Document) doc;
    app->NewDocument("MDTV-XCAF", doc);

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

    // Ensure STEP writer emits millimetres explicitly (file's own unit declaration).
    Interface_Static::SetCVal("write.step.unit", "MM");

    STEPCAFControl_Writer writer;
    writer.SetNameMode(true);
    if (!writer.Transfer(doc, STEPControl_AsIs))
    {
        std::cerr << "Transfer FAILED\n";
        return 1;
    }
    IFSelect_ReturnStatus stat = writer.Write("test_assembly.step");
    if (stat != IFSelect_RetDone)
    {
        std::cerr << "Write FAILED, status=" << (int)stat << "\n";
        return 1;
    }
    std::cout << "Wrote test_assembly.step OK\n";
    return 0;
}
