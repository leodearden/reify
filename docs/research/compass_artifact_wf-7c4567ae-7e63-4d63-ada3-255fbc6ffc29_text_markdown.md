# Designing a geometry modeling kernel from scratch

**A geometry kernel is the mathematical and algorithmic core that defines how shapes are represented, created, modified, and queried.** Building one from scratch requires navigating a vast design space spanning a dozen representation schemes, each with fundamentally different tradeoffs in precision, performance, robustness, and expressiveness. The choice of representation is not merely technical — it determines what your system can model, how fast it operates, where it fails, and what downstream workflows it supports. This report maps the full landscape: from classical B-rep and CSG through implicit fields and neural representations, covering the constraint solvers, Boolean algorithms, assembly frameworks, and volumetric property models that a complete kernel requires. The central engineering tension is between the exact, boundary-centric world of traditional CAD (B-rep/NURBS, proven across decades of manufacturing) and the volumetric, function-centric world of implicit modeling (SDFs, F-Rep, emerging for additive manufacturing and generative design). No single representation dominates all axes; the frontier is hybrid architectures that compose multiple schemes.

---

## Geometric representations: the foundation of every kernel

Every geometry kernel begins with a choice of how to represent shape. This choice propagates through the entire system — it determines what operations are natural, what is expensive, and where robustness problems appear.

### Boundary representation (B-rep)

B-rep defines a solid by its bounding surfaces. The model comprises two inseparable layers: **topology** (connectivity relationships) and **geometry** (mathematical shape definitions). The topological hierarchy runs vertex → edge → loop → face → shell → solid. Each topological entity references underlying geometry: vertices reference points, edges reference curves, and faces reference surfaces — typically NURBS.

The dominant data structure is the **half-edge** (Doubly Connected Edge List). Each undirected edge is split into two directed half-edges. Each half-edge stores: `origin` (vertex), `twin` (opposite half-edge), `next`/`prev` (adjacent half-edges in the face loop), and `face` (incident face). This yields unambiguous O(1) adjacency traversal. The older **winged-edge** (Baumgart, 1972) stores four wing edges per edge but introduces traversal ambiguity. For non-manifold topology (edges shared by more than two faces), Weiler's **radial-edge** structure extends the half-edge with a cyclic list of face-uses around each edge.

In practice, B-rep faces are bounded portions of NURBS surfaces. A **trimmed NURBS surface** pairs an underlying surface S(u,v) with trim curves in parameter space. The fundamental fragility: trim curves from adjacent faces are independently approximated, creating gaps at seams — typically under 10⁻⁶ meters, but non-zero. This makes every B-rep kernel inherently tolerant. "Equal" never means exactly the same; it means within tolerance.

B-rep is the industry standard (Parasolid, ACIS, OpenCascade). Its strength is exact analytical geometry supporting manufacturing workflows (STEP exchange, CNC toolpaths, GD&T). Its weakness is fragility: Boolean operations, fillets, and shells all require complex topology manipulation that can fail on edge cases.

### Constructive Solid Geometry (CSG)

CSG represents solids as a **binary tree** of primitives (sphere, box, cylinder, cone, torus, half-space) combined by Boolean set operations (∪, ∩, −) and rigid transformations. The tree is evaluated via **point membership classification** (PMC): recursively classify a query point as IN/ON/OUT against each leaf primitive, then combine results according to Boolean operators.

CSG trees are always valid by construction — the class of regular sets is closed under regularized Booleans. Storage is compact (parameterized primitives + tree structure). The key limitation: **no explicit boundary exists**. Rendering, meshing, and surface queries require either ray casting (natural for CSG — intersect ray with each primitive, combine 1D intervals) or boundary evaluation (converting to B-rep), which is computationally expensive and numerically difficult.

**Regularized Boolean operations** (Requicha & Voelcker) define A ∪* B = closure(interior(A ∪ B)), ensuring results are always valid solids without dangling faces or edges. This mathematical framework, formalized in the 1970s–80s at the University of Rochester, remains foundational.

### Implicit representations and signed distance fields

An implicit surface is defined by f: ℝ³ → ℝ where f(x) = 0 is the boundary, f(x) < 0 is interior, and f(x) > 0 is exterior. A **signed distance field** (SDF) is the special case where |f(x)| equals the Euclidean distance to the nearest boundary point, satisfying the Eikonal equation |∇f| = 1.

Boolean operations become trivial arithmetic: **union = min(f_A, f_B)**, intersection = max(f_A, f_B), difference = max(f_A, −f_B). The defining advantage is **smooth blending**: replacing min/max with smooth minimum functions creates organic fillets. The polynomial smooth minimum `smin(a, b, k) = mix(a, b, h) − k·h·(1−h)` where `h = clamp(0.5 + 0.5(a−b)/k, 0, 1)` provides a compact-support blend controlled by radius k. These operations never fail — a stark contrast to B-rep Boolean fragility.

SDFs can be analytical (function code, O(1) per primitive) or discretized (3D scalar grids, O(N³) memory). Analytical SDFs support real-time **sphere tracing** (ray marching): advance along a ray by f(current_position) at each step, converging rapidly since |∇f| ≤ 1. Discretized SDFs support physics simulation via level-set methods, evolving surfaces by solving Hamilton-Jacobi PDEs.

The weakness: no parametric surface representation. Sharp features (edges, corners) require special handling. Converting to B-rep for manufacturing requires meshing (Marching Cubes, Dual Contouring) followed by surface fitting — losing the exact geometry that manufacturing workflows demand.

### NURBS, T-splines, and analytic surfaces

**NURBS** (Non-Uniform Rational B-Splines) are the mathematical backbone of B-rep surface geometry. A NURBS curve is C(u) = Σ wᵢPᵢNᵢ,p(u) / Σ wᵢNᵢ,p(u), where Pᵢ are control points, wᵢ are weights, and Nᵢ,p are B-spline basis functions defined recursively via the Cox-de Boor formula over a knot vector U = {u₀, ..., uₘ}. Key properties include **local support** (moving one control point affects only p+1 spans), **convex hull** containment, **variation diminishing** behavior, and the ability to exactly represent conics through rational weighting. Surfaces extend to tensor products S(u,v).

**T-splines** generalize NURBS by allowing **T-junctions** in the control mesh, eliminating the requirement that control point rows span the entire grid. This enables **local refinement** without propagating control points globally — a significant advantage for adaptive modeling and isogeometric analysis. At extraordinary points (valence ≠ 4), surfaces are C¹ continuous versus C² elsewhere. Analysis-suitable T-splines (AST-splines) guarantee linear independence needed for FEA integration.

### Meshes, voxels, and point clouds

**Polygon meshes** use indexed face sets (vertex buffer + index buffer) for GPU-friendly storage, or half-edge structures for editing. **Subdivision surfaces** (Catmull-Clark for quads, Loop for triangles) generalize B-spline surfaces to arbitrary topology, with C² continuity everywhere except C¹ at extraordinary vertices. Limit positions are computable directly from initial mesh via limit stencils.

**Octrees** provide hierarchical spatial decomposition, with **sparse voxel octrees** pruning empty subtrees for efficient storage. Surface extraction uses **Marching Cubes** (Lorensen & Cline, 1987) for basic isosurface meshing or **Dual Contouring** (Ju et al., 2002) for sharp feature preservation via Quadratic Error Function (QEF) minimization at Hermite data points.

**Point clouds** represent unstructured point sets requiring normal estimation (PCA on k-nearest neighbors) and surface reconstruction. **Screened Poisson reconstruction** (Kazhdan & Hoppe, 2013) casts reconstruction as solving ∇²χ = ∇·V̄ on an adaptive octree, producing watertight surfaces in O(N) time.

### Sweep, medial axis, convex decomposition, and FRep

**Sweep representations** define solids by moving a profile along a path. Translational sweeps produce extrusions; rotational sweeps produce surfaces of revolution; general sweeps use varying cross-sections along spine curves with Frenet or rotation-minimizing frames. **Minkowski sums** (A ⊕ B = {a + b | a ∈ A, b ∈ B}) generalize sweeps and are fundamental to motion planning and offsetting.

The **medial axis transform** (MAT) encodes the locus of centers of maximally inscribed spheres, providing a complete shape descriptor (the original shape is exactly reconstructable). Computation typically uses Voronoi-based methods. MAT is unstable — small boundary perturbations create large spurious branches — requiring angle-based filtering or Q-MAT error-bounded simplification. Applications include feature recognition (encoding local thickness), mesh generation, and shape retrieval.

**Convex decomposition** partitions non-convex meshes into convex parts for physics simulation. The **V-HACD** algorithm voxelizes the mesh, builds a dual graph of triangles, iteratively collapses edges while monitoring concavity error, and outputs convex hulls. The newer **CoACD** uses collision-aware concavity metrics and multi-step tree search, better preserving fine geometric details.

**Functional Representation** (FRep) generalizes implicit surfaces using **R-functions** (Rvachev functions) — real-valued functions whose sign depends only on argument signs. The R₀ system provides C^k continuous Boolean operations: union f₁ ∨ f₂ = f₁ + f₂ + √(f₁² + f₂²), intersection f₁ ∧ f₂ = f₁ + f₂ − √(f₁² + f₂²). Unlike simple min/max, R-functions maintain differentiability. FRep supports **Constructive Hypervolume** models attaching vector-valued attributes (material, photometric properties) to point sets.

---

## Constraint systems drive parametric design

### Mathematical formulation of geometric constraints

Geometric constraints are equations on entity parameters that reduce degrees of freedom. A coincident constraint on two points (x₁ − x₂ = 0, y₁ − y₂ = 0) removes 2 DOF. A tangent constraint between a line and circle (|ax_c + by_c + c|/√(a²+b²) − r = 0) removes 1 DOF. Parallel (cross product = 0), perpendicular (dot product = 0), distance, angle, and radius constraints each contribute equations to the system. A well-constrained sketch has total DOF = Σ(entity DOFs) − Σ(constraint reductions) = 0.

### Three solver architectures

**Variational (simultaneous) solvers** formulate all constraints as F(x) = 0 and solve via Newton-Raphson iteration: x_{k+1} = x_k − J(x_k)⁻¹·F(x_k). Each constraint class implements gradient computation for the Jacobian. FreeCAD's planegcs uses DogLeg (trust-region) as default, with Levenberg-Marquardt and BFGS as alternatives. Under-constrained systems solve min ‖x − x₀‖² subject to F(x) = 0; over-constrained systems use QR decomposition for redundancy detection.

**Graph-based solvers** dominate 2D CAD. The constraint problem maps to a graph (vertices = entities, edges = constraints). **Owen's decomposition** (1991) identifies rigid clusters via tri-connected decomposition. **Hoffmann's DR-Planning** uses the "deficit" concept: deficit(G) = 2|V| − |E|, with **Laman's condition** guaranteeing generic rigidity when |E| = 2|V| − 3 and all subgraphs satisfy |E'| ≤ 2|V'| − 3. Decomposition identifies well-constrained subgraphs solvable via closed-form geometric constructions.

**Sequential (constructive) solvers** solve constraints step-by-step using geometric constructions. Very fast for decomposable problems but cannot handle all configurations.

The practical recommendation is a **hybrid**: graph-based decomposition for analysis, numerical solver for computation of each subproblem. The industry-standard commercial solver is Siemens' **D-Cubed DCM**, integrated in SolidWorks, AutoCAD, and Creo.

### Parametric vs. direct modeling

**Parametric (history-based) modeling** maintains an ordered feature tree where each operation (extrude, revolve, fillet) is stored with its parameters and references. Changing a parameter triggers replay from the modification point forward. The parent-child dependency graph captures design intent and enables part families. The critical challenge is the **persistent naming problem**: when topology changes during regeneration, referenced entities may disappear. Solutions include topology-based naming (face adjacency graphs, per Kripac), geometry-based disambiguation, and feature-face naming (Bidarra & Bronsvoort).

**Direct modeling** (push-pull editing) manipulates B-rep geometry without history. It works with any geometry and avoids feature failures, but captures no parametric intelligence. **Siemens Synchronous Technology** (2008) pioneered the hybrid approach: a decision engine recognizes geometric relationships from B-rep during direct edits, maintaining "Live Rules" that preserve parallelism, concentricity, and other constraints without an explicit history tree. Onshape takes a different hybrid approach: direct edits become parametric features in the history tree.

---

## Feature-based modeling captures design intent

Features encode not just geometry but manufacturing and design purpose. A hole implies a drilling operation with tolerances; a fillet implies stress relief. **Feature recognition** from B-rep uses **Attributed Adjacency Graphs** (AAG) — nodes for faces, arcs for shared edges with concavity attributes. Feature matching reduces to subgraph isomorphism against templates. Modern approaches use graph neural networks: **AAGNet** (2024) applies GNN on geometric AAGs with multi-task learning for semantic, instance, and base segmentation, achieving state-of-the-art accuracy on intersecting features — the classical failure case for rule-based methods.

Feature trees store ordered sequences of operations. Dependencies form a DAG: features reference entities from earlier features, creating parent-child relationships. Feature reordering is constrained by dependencies. Feature **suppression** temporarily disables features and their dependents; **rollback** moves the evaluation point backward in history. Feature **interactions** — where features merge faces, lose edges, or split faces — are a persistent challenge. Self-validating features (Bidarra & Bronsvoort) embed semantic constraints that automatically verify validity after each operation.

The **feature ontology problem** is that no universal feature taxonomy exists. The same cylindrical through-hole is a "passage" (form feature), a "drilled hole" (machining feature), a "core pin" (injection molding feature), or a "fastener bore" (assembly feature). ISO STEP AP242 and OWL-based ontologies attempt standardization but remain incomplete.

---

## Boolean operations: the hardest problem in solid modeling

### Why Booleans fail

Boolean operations follow a canonical pipeline: surface-surface intersection → edge classification / imprinting → face splitting → result assembly. The difficulty is not the algorithm but the **combinatorial explosion of edge cases**. Near-tangent intersections produce ill-conditioned intersection curves. Coincident faces create degenerate area-intersections rather than curves. Sliver faces cause numerical instability. Edge-on-edge and vertex-on-face contacts require special-case logic that is the most bug-prone code in any kernel.

Robust kernels like Parasolid have accumulated thousands of topology-change variants over decades, yet users still encounter failures. As the OpenCascade documentation warns: the kernel "sometimes has problems producing boolean results when the input objects share an edge or a face."

### Exact arithmetic vs. tolerance

Geometric predicates (orientation tests, in-circle/in-sphere) rely on sign evaluation of determinants. With IEEE 754 double-precision, roundoff errors near zero produce **wrong signs**, causing topological inconsistencies, crashes, or silent corruption.

**Shewchuk's adaptive precision** (1997) represents values as non-overlapping sums of floating-point numbers ("expansions") with multi-stage evaluation: Stage A uses standard doubles with precomputed error bounds (fast path for easy cases); Stages B and C progressively increase precision. For "easy" cases, overhead is ~2× naive; worst case ~30×.

**CGAL's exact computation paradigm** provides layered kernels. The `Exact_predicates_inexact_constructions_kernel` (EPIC) guarantees exact predicate signs using interval arithmetic filters — if the filter succeeds (most of the time), no exact arithmetic is needed. The `Exact_predicates_exact_constructions_kernel` (EPEC) adds lazy exact constructions via operation DAGs, computing exact values only when required. Performance overhead ranges from **25%** (Delaunay triangulation) to **50×** (complex constructions with full exact arithmetic).

Commercial kernels take a different path: **tolerance-based modeling**. ACIS uses per-entity tolerances (TEDGE, TVERTEX) automatically maintained by the system. Parasolid uses a tolerance hierarchy where each topological entity carries its own precision guarantee. This is pragmatic but introduces the fundamentally different category of problems associated with tolerance management.

### Euler operators guarantee topological validity

**Euler operators** are atomic topology-editing operations preserving the Euler-Poincaré invariant V − E + F = 2(S − G) + (L − F). The five make operators — MEV (make-edge-vertex), MEF (make-edge-face), MSFV (make-shell-face-vertex), MSG (make-shell-genus), MEKL (make-edge-kill-loop) — and their inverses form a complete set: every valid polyhedron can be constructed from an initial one via a finite sequence of Euler operations (Mäntylä, 1984). All higher-level modeling operations decompose into Euler operator sequences, serving as the "assembly language" of B-rep.

---

## Assembly modeling, tolerances, and inter-part relationships

### Assembly approaches and mate constraints

Each rigid body in 3D has **6 DOF** (3 translation + 3 rotation, configuration space ℝ³ × SO(3)). Mates impose holonomic constraints Φ(q) = 0, reducing DOF: a coincident-planar mate removes 1 DOF, point-coincidence removes 3, concentric removes 2, parallel removes 2. Assembly constraint solving is essentially an inverse kinematics problem — solve Φ(q) = 0 for component positions via Newton-Raphson on the constraint Jacobian.

The **Chebychev-Grübler-Kutzbach criterion** M = 6(N−1) − Σcᵢ predicts mechanism mobility but fails for mechanisms with special geometric conditions (parallel axes, symmetric links) that create dependent constraints. Practical kernels compute **numerical rank of the constraint Jacobian** at the current configuration.

**Bottom-up assembly** designs parts independently then positions them via constraints. **Top-down assembly** starts from a master layout/skeleton, deriving parts in context. The kernel must support a hierarchical assembly DAG, persistent part instance transforms, and inter-part associative references.

### GD&T and tolerance modeling

**Geometric Dimensioning and Tolerancing** (ASME Y14.5, ISO GPS) defines allowable geometric variation via tolerance zones: cylindrical zones for hole positions, parallel-plane zones for flatness, annular zones for concentricity. A Datum Reference Frame (DRF) establishes the measurement origin from up to three mutually perpendicular datum planes. **Semantic PMI** (Product Manufacturing Information) embeds machine-readable GD&T linked to topological features, enabling automated CMM programming and tolerance analysis. STEP AP 242 is the exchange standard. Stack-up analysis uses worst-case (arithmetic sum), statistical (RSS), or **Monte Carlo** methods.

### Interference detection

Two-phase architecture: **broad phase** (BVH with AABB/OBB, spatial hashing, or sweep-and-prune) rapidly eliminates non-colliding pairs; **narrow phase** uses the **GJK algorithm** (Gilbert-Johnson-Keerthi) which computes distance between convex shapes via support functions on the Minkowski difference, converging in O(1) iterations with warm-starting. Non-convex shapes require convex decomposition first.

---

## Volumetric properties and microstructure representation

### Heterogeneous and functionally graded materials

Traditional B-rep assumes homogeneous solids — geometry equals boundary only. **Heterogeneous objects** require interior material information. Representation approaches include voxel-based (each voxel stores material composition), **trivariate B-spline volumes** (Elber's V-reps mapping parametric domains to spatial coordinates and material properties), and implicit/F-rep (separate scalar fields for material distributions with Σfᵢ = 1).

**Functionally graded materials** (FGMs) use continuous spatial variation of composition, avoiding abrupt interfaces. Volume fraction is typically modeled as power law V_c(z) = (z/h)^p, with effective properties interpolated via Voigt (upper bound), Reuss (lower bound), or Hashin-Shtrikman bounds.

### Lattice structures via implicit fields

**TPMS** (Triply Periodic Minimal Surfaces) are the dominant representation for lattice infill, defined by level-set equations:

- **Gyroid**: cos(x)sin(y) + cos(y)sin(z) + cos(z)sin(x) = c
- **Schwarz P**: cos(x) + cos(y) + cos(z) = c
- **Diamond**: sin(x)sin(y)sin(z) + sin(x)cos(y)cos(z) + cos(x)sin(y)cos(z) + cos(x)cos(y)sin(z) = c

where x,y,z are scaled by 2π/L (L = unit cell size) and c controls volume fraction. These minimal surfaces (zero mean curvature) eliminate nodal stress concentrations inherent in strut-based lattices (BCC, FCC, octet truss). Generation is trivial in an implicit kernel: evaluate on a grid, extract isosurface. In B-rep, representing a lattice with millions of struts is prohibitively expensive — this is perhaps the strongest argument for implicit modeling in additive manufacturing.

### Multi-scale modeling

**Computational homogenization** solves nested boundary value problems: a macro-scale structural problem extracts deformation gradients at each material point, which become boundary conditions on a Representative Volume Element (RVE) solved at the micro-scale. The **Hill-Mandel condition** ensures energetic consistency between scales. The FE² method (full FEM at both scales) is accurate but extremely expensive; practical alternatives include transformation field analysis, POD reduced bases, and machine learning surrogates trained on RVE responses.

---

## Non-rigid bodies, shells, and manufacturing-specific modeling

### Sheet metal and mid-surface extraction

**Sheet bodies** are non-manifold B-rep models with zero-thickness faces. **Mid-surface extraction** reduces thin-walled solids to shells for FEA, using face-pairing algorithms (identifying approximately parallel, closely-spaced face pairs) or feature-based decomposition. **Sheet metal modeling** requires bend allowance BA = (π/180)(R + K·T)·A, where the **K-factor** (ratio of neutral axis position to thickness, typically **0.33–0.50**) depends on material, R/T ratio, and forming method. Flat pattern development sums flange lengths and bend allowances; bend relief cuts prevent tearing at bend terminations.

### Topology optimization and the interpretation gap

**SIMP** (Solid Isotropic Material with Penalization) assigns density ρ_e ∈ [0,1] per element with penalized stiffness E(ρ) = ρ^p·E₀ (p ≈ 3). Density filtering (convolution with cone kernel, radius r_min) prevents checkerboards; **Heaviside projection** pushes results toward binary 0/1. **Level-set methods** evolve the boundary via ∂φ/∂t + V_n|∇φ| = 0, producing crisp boundaries but struggling to nucleate new holes.

The **interpretation gap** between topology optimization output (density fields or meshes) and parametric CAD remains a major unsolved problem. The pipeline — thresholding → smoothing → quad remeshing → NURBS fitting → feature reconstruction — produces models requiring significant manual cleanup. This gap is where implicit modeling kernels have a structural advantage: topology optimization results map naturally to implicit fields without meshing intermediaries.

### Composite layup and deformable geometry

**Ply-based composite modeling** tracks individual layers defined by material, fiber orientation (measured relative to a rosette/reference coordinate system), thickness, and boundary on the tool surface. Draping simulation uses **kinematic (fishnet) models** that track shear angle between warp and weft fibers, flagging regions exceeding the material's locking angle (typically 30–50°). Zone-based approaches group regions by stacking sequence for preliminary sizing; ply-based approaches provide manufacturing-ready detail with flat patterns and ply books.

**Free-form deformation** (FFD, Sederberg & Parry, 1986) embeds objects in a lattice of control points: X' = Σᵢⱼₖ Bᵢ(s)Bⱼ(t)Bₖ(u)·P'ᵢⱼₖ, using Bernstein or B-spline bases. **Cage-based deformation** uses generalized barycentric coordinates — mean value coordinates (fast, closed-form, may have negative weights), harmonic coordinates (strictly positive, solve Laplace equation), or Green coordinates (shape-preserving, quasi-conformal).

---

## Modern and emerging approaches reshaping the field

### Convergent modeling bridges mesh and B-rep worlds

**Siemens' Convergent Modeling** (Parasolid v33.0+) enables facet bodies and B-rep bodies to coexist in a single model, supporting Boolean operations between them without conversion. Engineers can import scan data (STL), apply B-rep operations (fillets, chamfers, holes), and integrate into assemblies. This matters because topology optimization and generative design produce mesh outputs that need functional surfaces for assembly mating and secondary machining. Materialise adopted Parasolid in Magics 26, confirming that mesh-only representation is insufficient for advanced AM workflows.

### Implicit modeling kernels rewrite the rules

**nTopology** (nTop) uses a custom implicit kernel where every body is a single mathematical equation. Fields (scalar, distance, vector) drive design parameters spatially — simulation results vary lattice strut diameters, wall thickness, and material composition. Boolean operations are **mathematically guaranteed to never fail** (min/max on field values). nTop 5.0 demonstrated **up to 1000× performance improvement** for Booleans on lists of 10,000–100,000 primitives. Direct slicing for AM eliminates meshing intermediaries.

**Hyperganic** took the voxel-based approach further, with an assembly-language-optimized kernel where each voxel represents a material particle. Its spiritual successor **PicoGK** (LEAP 71) is an open-source kernel built on OpenVDB with a RISC-like reduced instruction set: minimal voxel operations at the core, higher-level functionality layered above.

### GPU acceleration enables new scales

SDF evaluation is embarrassingly parallel — each point evaluates independently, mapping naturally to GPU SIMD architectures. CUDA-based SDF generators achieve **5–60× speedup** over single-core CPU. **Synchronized Tracing** (ACM TOG 2023) provides tile-based GPU ray tracing for implicit CSG trees of thousands of primitives with on-the-fly tree pruning. The **Manifold library** (Emmett Lalish) implements parallel mesh Booleans using Thrust/CUDA at O(n log n), achieving floating-point robustness without exact arithmetic by ensuring "the same question is never asked two different ways."

GPU challenges remain significant: warp divergence from branching in CSG tree evaluation, FP32 vs. FP64 precision concerns for tolerances critical in CAD, and memory constraints for complex voxel grids.

### Neural implicit representations: promising but not yet engineering-ready

**DeepSDF** (Park et al., CVPR 2019) represents shape classes as f_θ(z, x) ≈ SDF(x), where a single MLP conditioned on latent codes z encodes an entire shape space. Shape interpolation, completion from partial observations, and compact storage (orders of magnitude smaller than voxels) are strengths. Variants like SIREN (periodic activations for sharp features) and NGLoD (octree-based multi-resolution) address specific limitations.

For engineering applications, the limitations are fundamental. Neural networks are function approximators with **inherent approximation error** — they cannot guarantee micron-level tolerances. There is no concept of dimensions, constraints, or parametric relationships. No topological entities (edges, faces) exist to apply standard CAD operations. Changing one feature requires retraining. **These representations currently serve research and visualization rather than manufacturing CAD.**

### Differentiable geometry enables optimization-driven design

Making geometry differentiable enables gradient-based shape optimization through the design→simulation→evaluation pipeline. **SDFDiff** (CVPR 2020) differentiates rendering of SDF surfaces via sphere tracing. The **Walk on Spheres** method provides differentiable PDE solving agnostic to boundary representation, supporting large topological changes at cost independent of parameter count. **CadQuery** and **Build123d** provide Python-based programmatic CAD on OpenCascade, increasingly used as target languages for neural CAD program synthesis (CAD-Recode, CAD-Coder) with **170,000+ shape-program pairs** available for training.

---

## Engineering tradeoffs across representation schemes

The choice of representation is a multi-dimensional optimization problem. No single scheme dominates all axes.

| Axis | B-rep/NURBS | CSG | Implicit/SDF | Mesh | Voxel/Octree |
|------|------------|-----|-------------|------|-------------|
| **Expressiveness** | Exact analytic + freeform surfaces | Limited to primitive vocabulary | Smooth, organic, procedural shapes | Arbitrary topology | Any topology; resolution-limited |
| **Precision** | Exact (NURBS, arbitrary precision) | Exact (analytic primitives) | Analytical: exact. Grid: resolution-limited | Approximate (tessellation error) | Resolution-limited |
| **Modeling freedom** | Parametric history, rich editing | Edit tree nodes/params | Modify function params, field-driven | Direct mesh editing | Sculpting, voxel painting |
| **Boolean speed** | Slow, fragile (SSI + classification) | Fast PMC (ray-based) | Trivial (min/max, O(1) per point) | Moderate (mesh intersection) | Trivial (pointwise) |
| **Query speed** | O(1) adjacency; O(log n) spatial | O(depth) PMC per point | O(1) per evaluation (analytical) | O(1) vertex; O(log n) spatial | O(1) lookup; O(log n) octree |
| **Memory** | Compact for simple shapes; grows with trim complexity | Very compact (tree + params) | O(1) analytical; O(N³) grid | O(N) vertices + faces | O(N³) dense; O(N² log N) sparse |
| **GPU suitability** | Poor (branching, complex topology) | Moderate (ray casting) | Excellent (embarrassingly parallel) | Good (indexed rendering) | Good (parallel evaluation) |
| **Robustness** | Fragile (tolerance-dependent) | Always valid by construction | Always valid | Topology-dependent | Always valid |
| **Implementation complexity** | Very high (decades of edge cases) | Moderate | Low to moderate | Low to moderate | Low |
| **Composability** | STEP exchange; interop via tessellation | Converts to B-rep (expensive) | Converts via meshing | Converts via surface fitting | Converts via MC/DC |

**Precision vs. robustness** is the most fundamental tension. B-rep/NURBS provides exact geometry that manufacturing requires but introduces fragility at every topology-modifying operation. Implicit representations are mathematically robust (Booleans never fail) but approximate — converting back to B-rep for manufacturing introduces the very precision loss the conversion was meant to avoid.

**Memory vs. fidelity** trades sharply for volumetric representations. A 1024³ voxel grid at 32 bits per voxel requires 4 GB; sparse voxel octrees and OpenVDB reduce this dramatically but add implementation complexity. Analytical SDFs store only function code (O(1)) but are limited to composable primitive vocabularies.

**Implementation complexity vs. manufacturing utility** explains why B-rep dominates despite its fragility. The entire manufacturing ecosystem — STEP exchange, CNC toolpath generation, GD&T, CMM inspection — is built around explicit boundary representations. An implicit kernel must eventually produce B-rep output for these workflows, inheriting all the conversion problems.

**The hybrid architecture thesis** is that the optimal geometry kernel combines representations, using each where it excels: B-rep for precision interfaces (mating surfaces, threaded holes, GD&T features), implicit for complex internal structures (lattices, TPMS, generative geometry), and meshes for scan data and visualization. Siemens' Convergent Modeling and nTopology's Materialise integration represent the two leading industrial approaches to this hybrid future.

---

## Existing kernels and what they teach us

### Commercial kernels

**Parasolid** (Siemens) is the most widely licensed kernel, powering 350+ applications including NX, SolidWorks, Solid Edge, Onshape, and Shapr3D. Its Convergent Modeling extension is a unique capability enabling B-rep operations on mesh data. Parasolid traces to the late 1970s **Romulus** kernel at Shape Data Ltd. (Cambridge, UK) — the first commercial B-rep solid modeler.

**ACIS** (Spatial/Dassault) introduced the **entity-attribute-callback** architecture where all objects derive from ENTITY, with user-definable attributes and change-tracking callbacks. ACIS pioneered **tolerant modeling** with per-entity tolerance values (TEDGE, TCOEDGE, TVERTEX) and **cellular topology** organizing models into non-overlapping cells. Created 1985–89 by the same Cambridge team (Braid, Lang, Grayer) who built Romulus.

### Open-source kernels

**OpenCascade** (OCCT) is the only full-scale open-source B-rep kernel. Its topology (TopoDS_Shape hierarchy: Vertex → Edge → Wire → Face → Shell → Solid → CompSolid → Compound) uses shareable topology via reference counting. Recent v8.x modernizations replaced linked-list child storage with contiguous arrays and devirtualized ShapeType(). Boolean robustness remains its known weakness — **Fuzzy Boolean Operations** (user-specified tolerance via SetFuzzyValue) provide mitigation. OCCT backs FreeCAD, CadQuery, Build123d, and Gmsh.

**CGAL** embodies the **exact computation paradigm** with filtered predicates (interval arithmetic fast path, exact arithmetic fallback) and generic programming via geometric traits. It provides provably correct geometry but is not a CAD kernel — no NURBS sweeps, no feature modeling, no B-rep solid operations in the CAD sense.

**libfive** is an F-Rep kernel using expression trees evaluated via array, interval, and compiled evaluators. Interval arithmetic enables efficient spatial culling; octree-based meshing with QEF vertex placement produces feature-preserving meshes. Booleans are min/max on field values — mathematically infallible. **Fornjot** is an early-stage Rust B-rep kernel focused on reliability ("operations should either work correctly or return clear errors") and code-first CAD modeling — significant as an experiment in building CAD infrastructure with modern systems programming languages.

**SolveSpace** contributes an excellent constraint solver (modified Newton's method with DOF tracking and over-constraint detection) available as a standalone embeddable library (`libslvs`). Its geometry kernel is minimal — basic NURBS, limited Booleans — but the solver architecture is a reference implementation for anyone building constraint systems.

---

## The historical arc from wireframe to implicit

The field has evolved through distinct paradigms: **wireframe** (1960s–70s, lines and points, ambiguous), **surface modeling** (late 1960s–80s, Bézier/B-spline patches, no volume), **solid modeling** (1970s–80s, CSG + B-rep, Requicha's mathematical foundations), **feature-based parametric** (late 1980s–90s, Pro/ENGINEER pioneering history trees), **direct modeling** (2000s–10s, SpaceClaim, Synchronous Technology), and the current **convergent/implicit** era (2010s–present, mixed B-rep + mesh + implicit).

Key inflection points: Sutherland's Sketchpad (1963, first interactive constraint-based drawing), BUILD system (1973, Cambridge research B-rep), Romulus (late 1970s, first commercial B-rep → ancestor of Parasolid and ACIS), Requicha & Voelcker's mathematical foundations (1977–82, Rochester), Pro/ENGINEER (1988, first feature-based parametric modeler), and the emergence of implicit modeling for additive manufacturing (2012+).

---

## Conclusion

A new geometry kernel design must make three strategic decisions early. **First**, the primary representation: B-rep for manufacturing-centric workflows, implicit for additive/generative-centric workflows, or a hybrid architecture accepting the complexity of maintaining interoperation between representations. **Second**, the robustness strategy: exact arithmetic (CGAL-style, provably correct but slower), tolerance-based (ACIS/Parasolid-style, pragmatic but accumulates edge cases), or implicit-field-based (mathematically infallible Booleans but approximate geometry). **Third**, the compute architecture: CPU-centric (traditional, mature tooling) or GPU-accelerated (natural for implicit, increasingly viable for mesh Booleans via Manifold-style approaches).

The field's trajectory points toward hybrid kernels that compose B-rep precision with implicit flexibility, backed by GPU acceleration. nTopology has demonstrated that implicit-first architectures can achieve 1000× Boolean performance on complex models. Siemens' Convergent Modeling has proven that mesh and B-rep can coexist operationally. The remaining gap is bidirectional conversion with feature preservation — work like NH-Rep (neural halfspace representations preserving sharp features across 10,000+ B-rep models) suggests this is solvable. The kernel designer's task is no longer choosing one representation but architecting the composition layer that makes multiple representations interoperate seamlessly, with each doing what it does best.