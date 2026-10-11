// plain_reader.cpp — read the test STEP file with plain STEPControl_Reader (no XDE),
// via ReadFile + TransferRoots + OneShape, to show what's lost without XDE.
#include <STEPControl_Reader.hxx>
#include <TopoDS_Shape.hxx>
#include <TopExp_Explorer.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TDF_Label.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <TDataStd_Name.hxx>
#include <iostream>

int main(int argc, char** argv)
{
    if (argc < 2) { std::cerr << "usage: " << argv[0] << " <file.step>\n"; return 2; }

    STEPControl_Reader reader;
    IFSelect_ReturnStatus stat = reader.ReadFile(argv[1]);
    std::cout << "ReadFile status=" << (int)stat << "\n";
    if (stat != IFSelect_RetDone) { std::cerr << "ReadFile FAILED\n"; return 1; }

    Standard_Integer nbRoots = reader.NbRootsForTransfer();
    std::cout << "NbRootsForTransfer=" << nbRoots << "\n";

    Standard_Integer nbTransferred = reader.TransferRoots();
    std::cout << "TransferRoots() returned nbTransferred=" << nbTransferred << "\n";
    std::cout << "NbShapes()=" << reader.NbShapes() << "\n";

    TopoDS_Shape oneShape = reader.OneShape();
    std::cout << "OneShape().IsNull()=" << oneShape.IsNull() << "\n";
    std::cout << "OneShape().ShapeType()=" << (int)oneShape.ShapeType()
               << " (0=COMPOUND,1=COMPSOLID,2=SOLID,...)\n";

    int nSolids = 0, nCompounds = 0, nShells = 0;
    for (TopExp_Explorer ex(oneShape, TopAbs_SOLID); ex.More(); ex.Next()) nSolids++;
    for (TopExp_Explorer ex(oneShape, TopAbs_COMPOUND); ex.More(); ex.Next()) nCompounds++;
    std::cout << "solids found (TopExp_Explorer TopAbs_SOLID) = " << nSolids << "\n";
    std::cout << "nested compounds found = " << nCompounds << "\n";

    Bnd_Box bbox;
    BRepBndLib::Add(oneShape, bbox);
    double xmin, ymin, zmin, xmax, ymax, zmax;
    bbox.Get(xmin, ymin, zmin, xmax, ymax, zmax);
    std::cout << "overall bbox=[(" << xmin << "," << ymin << "," << zmin << ") - ("
               << xmax << "," << ymax << "," << zmax << ")]\n";

    // Is there any way to recover product/instance NAMES from a plain TopoDS_Shape? No —
    // TopoDS_Shape carries no attribute/name storage. Demonstrate: shapes are anonymous.
    std::cout << "Product/instance names available from TopoDS_Shape alone: NO "
               << "(TopoDS_Shape has no name/label attribute; XDE's TDF_Label + TDataStd_Name "
               << "is the only carrier of names in this API)\n";
    return 0;
}
