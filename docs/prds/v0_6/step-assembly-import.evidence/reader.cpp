// reader.cpp — read a STEP assembly via XDE and print its product-tree structure.
//
// Usage: ./reader <file.step> [--metres]
//   --metres : call SetSystemLengthUnit(1000.0) on the underlying STEPControl_Reader
//              before Transfer, to test conversion of output coordinates to metres.
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPControl_Reader.hxx>
#include <TopoDS_Shape.hxx>
#include <TopExp_Explorer.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <TopLoc_Location.hxx>
#include <gp_Trsf.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TCollection_AsciiString.hxx>
#include <TDF_Tool.hxx>
#include <Interface_Static.hxx>
#include <map>
#include <set>
#include <iostream>
#include <iomanip>

static std::string getName(const TDF_Label& L)
{
    Handle(TDataStd_Name) nameAttr;
    if (L.FindAttribute(TDataStd_Name::GetID(), nameAttr))
    {
        TCollection_AsciiString ascii(nameAttr->Get());
        return std::string(ascii.ToCString());
    }
    return "<no-name>";
}

static void printLoc(const TopLoc_Location& loc, const std::string& indent)
{
    if (loc.IsIdentity())
    {
        std::cout << indent << "  location: identity\n";
        return;
    }
    const gp_Trsf& t = loc.Transformation();
    std::cout << indent << "  location (row-major 3x4):\n";
    for (int r = 1; r <= 3; ++r)
    {
        std::cout << indent << "    [";
        for (int c = 1; c <= 4; ++c)
        {
            std::cout << std::fixed << std::setprecision(4) << t.Value(r, c);
            if (c < 4) std::cout << ", ";
        }
        std::cout << "]\n";
    }
}

static void printShapeStats(const TDF_Label& shapeLabel, const std::string& indent)
{
    TopoDS_Shape shape;
    if (!XCAFDoc_ShapeTool::GetShape(shapeLabel, shape)) return;
    int solids = 0;
    for (TopExp_Explorer ex(shape, TopAbs_SOLID); ex.More(); ex.Next()) solids++;
    Bnd_Box bbox;
    BRepBndLib::Add(shape, bbox);
    double xmin, ymin, zmin, xmax, ymax, zmax;
    bbox.Get(xmin, ymin, zmin, xmax, ymax, zmax);
    std::cout << indent << "  solids=" << solids
               << " bbox=[(" << xmin << "," << ymin << "," << zmin << ") - ("
               << xmax << "," << ymax << "," << zmax << ")]\n";
}

static std::map<std::string, int> g_productUseCount; // product label entry -> times referenced as component target

static void walk(const Handle(XCAFDoc_ShapeTool)& shapeTool, const TDF_Label& label, int depth, std::set<std::string>& visitedProducts)
{
    std::string indent(depth * 2, ' ');
    TCollection_AsciiString entryStr; TDF_Tool::Entry(label, entryStr);
    std::string entry(entryStr.ToCString());
    bool isAssembly = XCAFDoc_ShapeTool::IsAssembly(label);
    bool isComponent = XCAFDoc_ShapeTool::IsComponent(label);
    bool isSimple = XCAFDoc_ShapeTool::IsSimpleShape(label);
    bool isReference = XCAFDoc_ShapeTool::IsReference(label);

    std::cout << indent << "Label " << entry << " name='" << getName(label) << "'"
               << " assembly=" << isAssembly << " component=" << isComponent
               << " simple=" << isSimple << " reference=" << isReference << "\n";

    if (isComponent || isReference)
    {
        TDF_Label referredLabel;
        if (XCAFDoc_ShapeTool::GetReferredShape(label, referredLabel))
        {
            TCollection_AsciiString refEntryStr; TDF_Tool::Entry(referredLabel, refEntryStr);
            std::string refEntry(refEntryStr.ToCString());
            std::cout << indent << "  -> refers to product label " << refEntry
                       << " name='" << getName(referredLabel) << "'\n";
            g_productUseCount[refEntry]++;
            printLoc(shapeTool->GetLocation(label), indent);
            // Recurse into the referred product (its own components/shape), tagging shared.
            bool alreadyVisited = visitedProducts.count(refEntry) > 0;
            if (alreadyVisited)
                std::cout << indent << "  [SHARED PRODUCT: already visited " << refEntry << "]\n";
            visitedProducts.insert(refEntry);
            if (XCAFDoc_ShapeTool::IsAssembly(referredLabel))
            {
                TDF_LabelSequence comps;
                XCAFDoc_ShapeTool::GetComponents(referredLabel, comps);
                for (TDF_LabelSequence::Iterator it(comps); it.More(); it.Next())
                    walk(shapeTool, it.Value(), depth + 2, visitedProducts);
            }
            else
            {
                printShapeStats(referredLabel, indent);
            }
        }
        return;
    }

    if (isAssembly)
    {
        TDF_LabelSequence comps;
        XCAFDoc_ShapeTool::GetComponents(label, comps);
        for (TDF_LabelSequence::Iterator it(comps); it.More(); it.Next())
            walk(shapeTool, it.Value(), depth + 1, visitedProducts);
    }
    else if (isSimple)
    {
        printShapeStats(label, indent);
    }
}

int main(int argc, char** argv)
{
    if (argc < 2)
    {
        std::cerr << "usage: " << argv[0] << " <file.step> [--metres]\n";
        return 2;
    }
    std::string mode = (argc >= 3) ? std::string(argv[2]) : "";

    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    Handle(TDocStd_Document) doc;
    app->NewDocument("MDTV-XCAF", doc);

    STEPCAFControl_Reader reader;
    reader.SetNameMode(true);

    if (mode.rfind("--sys-unit=", 0) == 0)
    {
        double v = std::stod(mode.substr(11));
        reader.ChangeReader().SetSystemLengthUnit(v);
        std::cout << "[unit-mode] called ChangeReader().SetSystemLengthUnit(" << v << ") before ReadFile\n";
    }
    else if (mode.rfind("--static-unit=", 0) == 0)
    {
        std::string v = mode.substr(14);
        Interface_Static::SetCVal("xstep.cascade.unit", v.c_str());
        std::cout << "[unit-mode] called Interface_Static::SetCVal(\"xstep.cascade.unit\", \"" << v << "\") before ReadFile\n";
    }

    IFSelect_ReturnStatus stat = reader.ReadFile(argv[1]);
    std::cout << "ReadFile status=" << (int)stat << " (1=RetVoid,0=RetDone... check IFSelect_ReturnStatus)\n";
    if (stat != IFSelect_RetDone)
    {
        std::cerr << "ReadFile FAILED\n";
        return 1;
    }

    Standard_Boolean ok = reader.Transfer(doc);
    std::cout << "Transfer returned " << (ok ? "true" : "false") << "\n";

    Handle(XCAFDoc_ShapeTool) shapeTool = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    TDF_LabelSequence freeShapes;
    shapeTool->GetFreeShapes(freeShapes);
    std::cout << "Free (root) shapes: " << freeShapes.Length() << "\n";

    std::set<std::string> visited;
    for (TDF_LabelSequence::Iterator it(freeShapes); it.More(); it.Next())
        walk(shapeTool, it.Value(), 0, visited);

    std::cout << "\n--- shared-product use counts (label entry -> times referenced) ---\n";
    for (auto& kv : g_productUseCount)
        std::cout << kv.first << " -> " << kv.second << (kv.second > 1 ? "  [SHARED]" : "") << "\n";

    return 0;
}
