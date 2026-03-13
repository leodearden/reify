# Open source geometry kernels: a comprehensive landscape guide

**OpenCASCADE remains the only production-grade open source B-rep kernel in 2026**, but a vibrant ecosystem of alternatives—spanning mesh-based, implicit/SDF, volumetric, and emerging Rust-based projects—is rapidly reshaping the landscape. The gap between open source and commercial kernels like Parasolid persists, primarily in Boolean robustness and advanced filleting, but it is narrowing. Meanwhile, entirely different geometric paradigms (implicit surfaces, voxel-based kernels, guaranteed-manifold mesh Booleans) are proving that B-rep is not the only path to production-quality geometry.

This report categorizes every major open source geometry kernel and library by representation type, evaluates each on capabilities and maturity, and maps the trajectory of this critical infrastructure layer for CAD, manufacturing, and computational design.

---

## The four paradigms of open source geometry

Open source geometry kernels fall into four distinct categories, each with fundamentally different trade-offs:

**B-rep (Boundary Representation) kernels** define solids by their bounding surfaces—NURBS, analytical surfaces, trimmed patches—and are the foundation of traditional CAD. They excel at precise engineering geometry but demand extraordinary algorithmic complexity for operations like Booleans and fillets. **Mesh-based kernels** operate on triangle meshes, trading parametric precision for speed and simplicity. **Implicit/SDF kernels** represent geometry as mathematical functions where the boundary is a zero-level set, making CSG operations trivial (just min/max) but meshing and precision more challenging. **Volumetric/voxel kernels** discretize space into grids, offering unbreakable Booleans at the cost of resolution-dependent accuracy.

Understanding which paradigm a project belongs to is essential. A mesh Boolean library like Manifold and a B-rep kernel like OpenCASCADE solve fundamentally different problems, even though both can produce 3D shapes.

---

## B-rep and CAD kernels: OpenCASCADE dominates, Rust challengers emerge

### OpenCASCADE Technology (OCCT)

OpenCASCADE is the undisputed heavyweight of open source B-rep modeling—roughly **2 million lines of C++** providing the full stack of CAD kernel capabilities. Originally developed in the early 1990s by Matra Datavision for their Euclid CAD software, it was open-sourced in 1999 and is now maintained by Open Cascade SAS (part of Capgemini) under the **LGPL-2.1** license.

The stable release is **v7.9.3** (December 2025), with **v8.0.0 RC4** available and full 8.0 expected in Q1 2026. Version 8.0 is a landmark modernization: it upgrades the minimum standard to C++17, introduces the `occ::` namespace, redesigns geometry evaluation with a new `EvalD*` API, and delivers **up to 75% faster STEP reading** compared to v7.7. The GitHub repository (~2,000 stars, ~514 forks) shows consistent activity with quarterly maintenance releases.

OCCT supports the full gamut of B-rep geometry: analytical curves and surfaces (lines, circles, ellipses, planes, cylinders, cones, spheres, tori), Bézier and B-spline curves/surfaces, NURBS, Boolean operations, fillets, chamfers, shape healing, and data exchange in **STEP, IGES, STL, BREP, OBJ, glTF**, and 25+ formats. Its ecosystem is enormous: **FreeCAD, CadQuery, Build123d, Gmsh, SALOME, KiCad**, and dozens more depend on it. Bindings exist for Python (PythonOCC), JavaScript/WASM (opencascade.js), C#, and Tcl.

The weaknesses are well-known. Boolean operations can fail or crash on complex geometry—a problem FreeCAD users encounter regularly. The API is verbose with a steep learning curve, the codebase carries decades of legacy patterns (though v8.0 addresses much of this), and the library footprint is large (~2 GB with FreeCAD). Documentation has historically been sparse, though it is improving. Despite these issues, **no other open source project comes close to matching OCCT's breadth** of CAD functionality.

### Open Cascade Community Edition (OCE)

OCE was a community fork started in 2011 by Thomas Paviot to provide better CMake integration and gather community patches when OCCT's repository required a CLA even for read-only access. **It is now effectively deprecated.** The last significant independent release was OCE-0.18.3 (based on OCCT 6.8/6.9 era). Since OCCT moved to GitHub with public access, OCE's raison d'être has evaporated. FreeCAD and other major projects have migrated to upstream OCCT. The repository now exists primarily as a patch-holding branch.

### Truck: the most promising Rust-based kernel

Truck is an open source shape processing kernel written in Rust, developed by **RICOS Co. Ltd.**, a Japanese scientific computing company. With ~1,300 GitHub stars and an **Apache-2.0** license, it takes a modular "Ship of Theseus" approach—a collection of small, replaceable crates covering base types, NURBS geometry, topology, modeling, meshing, STEP I/O, and WebGPU rendering.

What distinguishes Truck from other Rust efforts is that it already supports **NURBS curves and surfaces, B-rep topology, STEP import/export, and Boolean operations**. Recent 2025 development added assembly structure handling, RBF surface approximation for fillets, higher-order derivations, and a published tutorial book. It compiles to WebAssembly and has JavaScript bindings, making it viable for browser-based CAD. The **CADmium** project uses Truck as its kernel.

The limitations are real: Boolean operations are not as robust as OCCT or Parasolid, fillets remain in prototyping stage (described as "surprisingly one of the hardest features"), and there is no IGES support. Community commentary on Hacker News suggests that **"once Truck lands stable fillets, it will prove itself as the perfect successor to OpenCascade"**—but that milestone remains ahead.

### Fornjot: ambitious but slowing

Fornjot is a Rust B-rep kernel created by Hanno Braun, with ~1,800 GitHub stars and a remarkably permissive **Zero-Clause BSD** license. It supports basic sketching, extrusion, and Boolean operations, with models defined as Rust code. However, development has slowed—Braun has stated he is working in "reduced capacity." Critically, Fornjot **lacks NURBS support, STEP I/O, and curved surface handling**. It is best understood as an experimental project exploring what a clean-slate Rust kernel could look like, rather than a practical alternative to OCCT.

### BRL-CAD and SolveSpace: specialized alternatives

**BRL-CAD** (~945 stars, LGPL/BSD) is one of the oldest open source codebases in existence, under continuous development since 1979 at the U.S. Army Ballistic Research Laboratory. It is fundamentally **CSG-centric** rather than B-rep-centric, with an extensive library of CSG primitives and a high-performance ray tracing engine. It excels at military vulnerability analysis and ballistic simulation but is not designed as an embeddable B-rep kernel.

**SolveSpace** (~3,745 stars, GPL-3.0) is a lightweight parametric CAD with its own integrated geometry kernel in only ~30–50K lines of C++. Its standout feature is an excellent **constraint solver** (available as the standalone library `libslvs`, reused by projects like Dune3D). However, it **lacks fillets and chamfers on 3D bodies**—a major limitation—and its NURBS Boolean operations can fail on complex geometry.

---

## Computational geometry libraries: CGAL leads the field

### CGAL: the gold standard for algorithmic geometry

The Computational Geometry Algorithms Library is the most comprehensive open source computational geometry library available, with **~5,700 GitHub stars, 114,000+ commits**, and activity on 357 of the last 365 days. Founded in 1996 by a consortium including Inria, ETH Zurich, and Tel-Aviv University, CGAL provides mathematically rigorous algorithms with **exact arithmetic kernels** that eliminate floating-point robustness issues entirely.

CGAL v6.1 (September 2025) covers an extraordinary breadth: 2D/3D Delaunay and constrained Delaunay triangulations, Voronoi diagrams, Boolean operations on Nef polyhedra, mesh generation and isosurface extraction, arrangements of curves and surfaces, convex hulls, alpha shapes, spatial searching (AABB and KD trees), and geometry processing (remeshing, simplification, parameterization). The dual license—**GPL v3+ for open source, commercial license via GeometryFactory**—is restrictive for commercial use without payment.

CGAL is not a CAD kernel. It lacks sweep, revolve, and full NURBS modeling. But it is the computational backbone behind OpenSCAD's geometry engine and is used extensively in GIS, robotics, medical imaging, and scientific computing. Its primary weakness is the steep learning curve imposed by heavy C++ template usage. Python bindings exist via cgal-swig-bindings and scikit-geometry but remain limited.

### Other notable computational geometry libraries

**Geogram** (~2,400 stars, BSD 3-Clause) from Inria's ALICE project is optimized for extreme scale—handling hundreds of millions of points for cosmology-scale simulations. It includes exact arithmetic predicates, highly optimized Delaunay triangulation, and Boolean operations on triangle meshes. Developed primarily by Bruno Levy, it won the SGP Software Award in 2023.

**libigl** (~4,900 stars, MPL-2.0) is a header-only C++ library focused on geometry processing research, offering a MATLAB-like API for mesh deformation, parameterization, and analysis. **Open3D** (~13,000 stars, MIT) is the most popular 3D data processing library, emphasizing point cloud processing, 3D reconstruction, and ML integration with GPU acceleration—but it is not a geometry kernel. **trimesh** (~3,400 stars, MIT) provides pure-Python mesh manipulation. **GEOS** (~1,400 stars, LGPL) is the 2D spatial geometry standard underpinning PostGIS, QGIS, and Shapely.

---

## Implicit and SDF kernels: a different philosophy

### libfive and Fidget: Matt Keeter's evolving vision

**libfive** (~1,500 stars, MPL 2.0) defines geometry as mathematical functions where the boundary is the zero-level set. Created by Matt Keeter, it provides feature-preserving watertight meshing, a standard library of CSG operations, and bindings in C, Python, and Guile Scheme. It has been used commercially (notably by nTopology). CSG operations become trivial min/max operations on functions, and resolution is theoretically unlimited.

Keeter's focus has shifted to **Fidget** (~400 stars, MPL 2.0), a Rust-based successor emphasizing raw evaluation speed. Fidget includes a **hand-written JIT compiler** for x86_64 and aarch64 that achieves a **31× speedup** over bytecode interpretation—nearly matching GPU-based approaches on CPU. It supports WebAssembly compilation, Manifold Dual Contouring for meshing, and Rhai scripting. Though experimental and described as having "Lego-kit-without-a-manual energy," Fidget represents the cutting edge of implicit surface evaluation.

### ImplicitCAD and Curv

**ImplicitCAD** (~1,500 stars, AGPL v3+) is a Haskell-based implicit CAD tool with built-in **rounded and bevelled CSG operations** and partial OpenSCAD compatibility. Development activity is low, and the Haskell ecosystem limits adoption. **Curv** (~1,100 stars, Apache 2.0) was an elegant language for mathematical art with GPU-accelerated SDF rendering, but its GitHub repository has been archived and the creator is not actively developing the core.

---

## Mesh and volumetric kernels: Manifold breaks through

### Manifold: redefining mesh Booleans

Manifold (~1,500 stars, Apache 2.0) has rapidly become the **de facto mesh Boolean engine** for the open source ecosystem. Created by Emmett Lalish (formerly at Google, now at Wētā FX), it implements the first known **guaranteed-manifold mesh Boolean algorithm**—every output is topologically valid by construction. Performance claims are dramatic: **100–1,000× faster than OpenSCAD's CGAL backend**.

The adoption trajectory tells the story. **OpenSCAD made Manifold its default backend** in August 2025. **Blender** uses it as a Boolean solver in Geometry Nodes. **BRL-CAD** integrated it and saw success rates jump from 88.9% to 98.9% with 7× speedups. Bindings span JavaScript/WASM, Python, C, Rust, and C#, with the ManifoldCAD.org web editor demonstrating browser-based CSG modeling. The limitation is inherent to the paradigm: mesh representation is approximate for curved surfaces, and Manifold requires manifold input (no open surfaces or non-manifold geometry).

### OpenVDB and PicoGK: volumetric approaches

**OpenVDB** (~5,000 stars, Apache 2.0) is the Academy Award-winning sparse volumetric data structure maintained by the Academy Software Foundation. It is the industry standard for VFX volumetrics (fire, water, smoke) and level set operations, with deep integration into Houdini, Maya, and Blender. NVIDIA's fVDB extends it for AI-driven spatial intelligence. While not a traditional geometry kernel, its CSG operations on volumes are trivially robust.

**PicoGK** (Apache 2.0) from LEAP 71 builds on OpenVDB to create a **voxel-based computational engineering kernel** in C#. Boolean operations are "unbreakable" by design. It targets additive manufacturing of complex engineered objects—rocket engines, heat exchangers, lattice structures—and won the 3D Pioneers Challenge 2024.

---

## How open source compares to Parasolid and ACIS

The commercial kernel landscape is dominated by **Parasolid** (Siemens) and **ACIS** (Dassault Systèmes/Spatial), both dating to the 1980s and licensed at roughly **$100K/year** on a revenue-percentage basis.

Parasolid is the most widely licensed kernel in the world, used by Siemens NX, SOLIDWORKS, Onshape, Shapr3D, and 20+ other applications. Its key advantages over open source alternatives include **convergent modeling** (hybrid B-rep + mesh geometry, unique to Parasolid), the most robust Boolean operations in the industry with superior handling of tangent and coincident geometry, reliable complex filleting, native multi-threading, and decades of industrial hardening across millions of models.

ACIS powers 350+ applications with 2+ million seats, offering a broader set of procedural surface types (variable_blend, net, skin, tube, law surfaces) than either Parasolid or OCCT. **C3D Kernel** from Russia offers royalty-free licensing at lower cost, positioning as a Parasolid/ACIS alternative particularly for the Russian market. **CGM** (Dassault) underpins CATIA with tolerant modeling designed in from the ground up.

The specific gaps between OCCT and commercial kernels are:

- **Boolean robustness**: Parasolid and ACIS have decades of edge-case hardening. OCCT's Booleans can fail or segfault on complex geometry, though v8.0 brings significant improvements.
- **Advanced fillets**: Commercial kernels handle large-radius, multi-edge, and feature-retaining fillets far more reliably. SolveSpace lacks 3D fillets entirely.
- **Surface type breadth**: OCCT's geometry set is based on STEP ISO 10303-42 and is "really a subset" of what Parasolid and ACIS offer, lacking intersection curves and specialized blend surfaces.
- **Multi-threading**: Parasolid is properly multi-threaded; OCCT is largely single-threaded.
- **Professional support**: Commercial kernels include dedicated support teams and regression testing across hundreds of thousands of models.

Building a production-quality B-rep kernel is described by practitioners as requiring **"many thousands of years of collective engineering time,"** with each individual capability like fillets or offset surfaces potentially being "a year-long research project in itself."

---

## Emerging projects reshaping the ecosystem

Several newer projects are worth tracking. **OpenGeometry** (opengeometry.io) is a Rust/WASM geometry kernel targeting architectural and BIM applications in the browser. **Dune3D** cleverly combines SolveSpace's constraint solver with OpenCASCADE's geometry kernel, addressing FreeCAD's topological naming problem. **replicad** (replicad.xyz) brings CadQuery-style code-CAD to JavaScript via OpenCascade.js. **Build123d** offers a modern Python interface to OCCT with both algebra and builder modes. **CADmium** uses Truck as a Rust kernel compiled to WASM for browser-based parametric CAD.

The pattern is clear: rather than building kernels from scratch, many emerging projects **compose existing open source kernels** (OCCT, Truck, SolveSpace's solver, Manifold) into new applications, often targeting the browser via WebAssembly.

| Kernel | Type | Language | Stars | License | Maturity | Key strength |
|---|---|---|---|---|---|---|
| **OpenCASCADE** | B-rep | C++ | ~2,000 | LGPL-2.1 | Production | Only full open source B-rep kernel |
| **Truck** | B-rep | Rust | ~1,300 | Apache-2.0 | Early | NURBS + STEP + WASM |
| **Fornjot** | B-rep | Rust | ~1,800 | 0BSD | Pre-alpha | Clean Rust architecture |
| **CGAL** | Comp. geometry | C++ | ~5,700 | GPL/Commercial | Production | Exact arithmetic, broadest algorithms |
| **Manifold** | Mesh | C++ | ~1,500 | Apache-2.0 | Production | Guaranteed-manifold Booleans |
| **libfive** | Implicit/SDF | C++ | ~1,500 | MPL-2.0 | Mature | Proven SDF kernel |
| **Fidget** | Implicit/SDF | Rust | ~400 | MPL-2.0 | Experimental | JIT-compiled evaluation |
| **OpenVDB** | Volumetric | C++ | ~5,000 | Apache-2.0 | Production | Industry standard, ASWF backed |
| **PicoGK** | Voxel | C#/C++ | Growing | Apache-2.0 | Active | Unbreakable Booleans for manufacturing |
| **BRL-CAD** | CSG | C/C++ | ~945 | LGPL/BSD | Production | Oldest open source CAD, CSG specialist |
| **SolveSpace** | Own kernel | C++ | ~3,745 | GPL-3.0 | Production | Lightweight, excellent constraint solver |
| **Geogram** | Comp. geometry | C++ | ~2,400 | BSD-3 | Production | Extreme-scale performance |

## Conclusion

The open source geometry kernel ecosystem in 2026 is no longer a story of "OpenCASCADE or nothing." While OCCT remains the sole production-grade open source B-rep kernel—and its v8.0 modernization is the most significant upgrade in years—the landscape has diversified along multiple axes. **Manifold has proven that mesh-based Booleans can be both fast and guaranteed-correct**, earning integration into OpenSCAD, Blender, and BRL-CAD. **Truck is the Rust-based kernel closest to challenging OCCT**, with real NURBS and STEP support, though robust fillets remain the critical unsolved problem. **Fidget points toward a future where implicit surface evaluation rivals GPU speeds on CPU alone.**

The most pragmatic near-term strategy for anyone building CAD tools is combining existing kernels—OCCT for B-rep, Manifold for mesh Booleans, SolveSpace's solver for constraints—rather than attempting a ground-up replacement. The commercial kernel gap is real but narrowing, and for workflows centered on additive manufacturing or generative design, alternative paradigms like voxel-based PicoGK or SDF-based libfive may actually be superior to traditional B-rep. The next five years will likely determine whether a Rust-native kernel can finally break the three-decade duopoly of Parasolid and ACIS—or whether composing open source components will render that question moot.