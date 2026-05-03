# PRD: Structural Analysis (Linear Elastostatic FEA)

Status: deferred — tentatively v0.3. First concrete `ComputeNode` realization in the language.
Design resolved 2026-05-03 — see "Resolved design decisions" below. Solver, mesher, licensing, BC surface, and architectural extension all settled; one minor open question (failure-mode reporting policy) is implementation-time.

## Goal

Ship a linear elastostatic FEA capability as a stdlib `@optimized` kernel binding so that a user can declare loads, supports, and a material on a realized `Body`, and obtain `displacement` and `stress` fields plus derived scalars (max von Mises, safety factor). The computation appears as a normal pure function returning `Field`-typed values; the runtime materializes it as a `ComputeNode` with warm-start support so that small parameter changes (load magnitude, fillet radius, thickness `auto` value) reuse the previous solution.

The motivating end-to-end use is closing the design loop:

```
param thickness : Length = auto
material = Steel_AISI_1045
load    on bracket.face("top")    = 5 kN downward
support on bracket.face("mount")  = fixed

minimize mass subject to max(von_mises(bracket)) < material.yield_stress
```

That fragment requires three things working together: an FEA `ComputeNode` producing a stress field, the existing `auto`/`minimize` resolver from v0.1 driving `thickness`, and a stdlib `von_mises` reduction over the field. This PRD covers the FEA node and the minimum stdlib needed to express loads/supports/materials; it does **not** revisit the solver framework.

## Background

`docs/reify-implementation-architecture.md` cites FEA as the canonical example of a `ComputeNode` throughout: §6.1 ("One FEA run = one node"), §6.2 ("FEA reads `material.youngs_modulus`"; "Stress analysis reads mesh"; "FEA safety factor feeds field value"), §13 ("an FEA solver node might be `warm_startable + progressive + committable`"), and the multi-rep stack pattern §10.5 (`B-rep -> mesh -> FEA stress field -> density field -> implicit lattice`). The architecture is FEA-shaped from the start; we have just never built one.

ComputeNode infrastructure exists in the engine (node taxonomy, cache key shape, dependency edges 6/10/12) but has no consumer. Shipping FEA is therefore both a feature and the first stress test of that path — every implicit assumption about ComputeNode bookkeeping (cache key composition over fields, warm-state handoff, content-hash significance filter, `pending` propagation gate during long runs) gets exercised for real for the first time.

FEA is the right first ComputeNode because (a) the `Body → mesh → solve → field` pipeline is already mostly in place by v0.2 (multi-kernel mesh-from-B-rep), (b) it directly feeds the auto-resolve loop that is v0.1's signature feature, and (c) there is mature open-source solver tech to wrap rather than write — risk concentrates on integration, not numerical methods.

## Why deferred

- **Mesh-from-B-rep depends on multi-kernel.** v0.1 ships OCCT-only with no exposed mesh pipeline; FEA needs a quality tetrahedral mesh with element-size control. The cleanest path is the `BRep -> Mesh` conversion that the v0.2 multi-kernel PRD makes available.
- **Field-typed outputs depend on field machinery being load-bearing.** v0.1 exercises `analytical` and `composed` field sources lightly. FEA produces `Field<Tensor<2,3,Pressure>>` and `Field<Vector3<Length>>` outputs that downstream operations (max reduction, von Mises, plotting in the GUI) need to consume cleanly. v0.2 imported-field work exercises the same machinery from the input side; FEA closes the loop.
- **Material stdlib is incomplete.** `Pressure`, `Density` exist; `YieldStress` (alias of `Pressure`), `PoissonRatio` (dimensionless with `[0, 0.5)` constraint), and a `Material` trait grouping these are not yet in stdlib. Doing this speculatively risks rework once a concrete consumer demands it; FEA is that consumer.
- **No surface syntax for boundary conditions (loads, supports).** Needs design — see open questions.
- **Solver choice is undetermined.** No telemetry, no user demand specifics, no licensing decision. Picking before v0.2 ships is premature.
- **GUI work to render fields is non-trivial and not yet scoped.** Stress contour plots, deformed-shape rendering, probe-point queries — useful but easily a milestone of their own. Decoupling this lets the headless FEA path land first.

## Sketch of approach

**Surface syntax** — no new declaration kind. FEA appears as a stdlib intrinsic registered via `@optimized`, taking the same shape as any other pure function. The signature is roughly:

```
@optimized("solver::elastic_static")
fn solve_elastic_static(
    body: Body,
    material: ElasticMaterial,
    loads: List<Load>,
    supports: List<Support>,
    options: ElasticOptions = ElasticOptions.default,
) -> ElasticResult
```

`ElasticResult` is a struct of fields and scalars:

```
struct ElasticResult {
    displacement : Field<Point3<Length>, Vector3<Length>>
    stress       : Field<Point3<Length>, Tensor<2, 3, Pressure>>
    max_von_mises: Pressure
    converged    : Bool
    iterations   : Integer
}
```

Calling `solve_elastic_static(...)` evaluates as a `ComputeNode` (cache key derived from the realization hash of `body`, value hashes of `material` / `loads` / `supports` / `options`). A second call with a perturbed input rebuilds the node with the prior solution as warm start.

**Boundary conditions as values, not statements.** `Load` and `Support` are stdlib structs whose constructors capture the geometric target (a face, edge, or point selector — interoperates with `topology-selectors.md`), the magnitude, and the direction. This avoids inventing a new statement form. Sketch:

```
load    = PressureLoad(face = bracket.face("top"),     magnitude = 5 MPa, direction = -bracket.up)
support = FixedSupport(face = bracket.face("mount"))
```

**Material.** Add `ElasticMaterial` trait to stdlib carrying `youngs_modulus : Pressure`, `poisson_ratio : Number`, `density : Density`, `yield_stress : Pressure` (optional). Provide a small starter library (`Steel_AISI_1045`, `Aluminium_6061`, `Titanium_Ti6Al4V`, `ABS_Plastic`) sourced from MMPDS / matweb-equivalent public references. Per-property provenance recorded so users can trace each constant.

**Stress reductions.** Stdlib functions `von_mises(stress: Tensor<2,3,Pressure>) -> Pressure`, `principal_stresses(...)`, `max(field: Field<_, T>) -> T` — pure functions, no kernel binding needed beyond efficient implementations.

**Mesh source.** Pipeline is two-stage:

```
B-rep (OCCT)
  → surface triangle mesh   ← existing v0.1 OCCT BRepMesh / v0.2 Manifold path
  → volume tet mesh         ← new in this PRD (Gmsh)
  → FEA assembly
```

The existing v0.1/v0.2 surface-mesh path produces boundary triangulations; FEA needs a volume tet mesh of the interior. The new stage takes the surface mesh as input and emits second-order tets. v0.2's `ReprKind = BRep | Mesh | Sdf | Voxel` enum is extended (non-breaking minor, as the multi-kernel PRD already noted) with a `VolumeMesh` variant so the cache can distinguish surface from volume realizations.

The realization request is `(body, ReprKind::VolumeMesh, tol)` where `tol` is sourced from per-purpose tolerance. Mesh-quality knobs (element-size grading, refinement around stress concentrations) live in `ElasticOptions` for v0.3; defaults pick something reasonable from `tol`. See "Resolved design decisions" for the mesher selection (Gmsh).

**Solver.** Handwritten linear-elastostatic kernel in Rust on top of `faer-rs` for the linear-algebra layer — see "Resolved design decisions" below for the trade analysis. The kernel lives in a new `reify-solver-elastic` crate, exposes assemble / apply-BCs / solve / interpolate-result primitives, and is registered as `@optimized("solver::elastic_static")`.

**Warm start.** The solver's prior iterate (and, when a direct solve is used, its symbolic factorization) is held as `OpaqueState` attached to the ComputeNode — same mechanism as constraint solvers. Engine restarts shed it; in-process re-solves with perturbed inputs reuse it. With a handwritten kernel we own this hook outright, so warm start is a first-class API surface from day one rather than something we hope a wrapped solver supports.

**Caching.** Standard ComputeNode caching: result is content-hashed; downstream consumers (max von Mises feeding a constraint) skip re-evaluation when consecutive solves produce numerically-equivalent fields up to tolerance. Significance threshold should be tied to the same per-purpose tolerance the solve was run at.

## Pre-conditions for activating

- v0.2 multi-kernel work has shipped at least the `BRep → Mesh` path (Manifold or OCCT mesh export) with an exposed tolerance knob.
- v0.2 imported-field-source has shipped (proves the field-as-output side of the ComputeNode pipeline works for OpenVDB-shape grids; FEA outputs are similar in shape).
- Per-purpose tolerance (`per-purpose-tolerance.md`) is live so the FEA node can declare its tolerance demand and the cache keys it correctly.
- Topology selectors (`topology-selectors.md`) sufficiently expressive to refer to the faces/edges that loads and supports attach to (likely already true after v0.2 task 7).
- A concrete user / example wants this. Keep deferred until then.

## Resolved design decisions (2026-05-02)

**Reify is AGPL-3.0**, so license is not a discriminator across viable candidates: MFEM (BSD-3), CalculiX (GPL-2.0+), Code_Aster (GPL-3+), FEniCSx (LGPL-3.0), and faer-rs (MIT/Apache) are all distribution-compatible. The pick reduces to technical fit:

| Candidate            | Embedding         | Warm start          | Determinism            | Build cost            | Future scope reach                          |
|----------------------|-------------------|---------------------|------------------------|-----------------------|---------------------------------------------|
| MFEM                 | in-process C++    | controllable        | serial-deterministic   | moderate (CMake)      | strong — write your own forms               |
| CalculiX             | subprocess only   | effectively no      | yes                    | separate binary       | very strong (Abaqus-clone scope in the box) |
| FEniCSx              | C++ embed of DOLFINx | controllable     | serial-deterministic   | high (Python at build) | strong via UFL                              |
| **faer-rs + handwritten** | **pure Rust**  | **we own it**       | **trivial**            | **minimal**           | **none for free**                           |

**Selected: handwritten linear-elastostatic kernel on faer-rs** for the v0.3 MVP. Reasons: (a) perfect warm start exercises the auto-resolve loop seriously from day one; (b) zero FFI / build-system pain preserves the single-command-launch story; (c) bit-for-bit determinism without effort; (d) the entire surface lives inside Reify so the ComputeNode plumbing gets validated in isolation, not entangled with FFI bookkeeping; (e) AGPL means we have no licensing pressure to prefer a permissive option, only technical pressure.

The `solve_elastic_static` signature is designed so MFEM (or CalculiX as a subprocess fallback for industry-validation parity) can slot in behind it without changing user-visible code. Migration path is staged, not foreclosed.

**Risk acknowledged — "permanent temporary."** The handwritten path silently calcifies because everyone treats it as a stop-gap. Mitigation: when a v0.4+ feature (plasticity, contact, transient) demands wrapping a real solver, treat it as the trigger to replace the linear path too, not just to add the new analysis kind alongside it.

**Element type/order.** Default to second-order tetrahedra (P2 tet); single override in `ElasticOptions` for first-order tets when speed beats accuracy. No higher-order or hex/wedge for v0.3.

**Multi-physics shape.** Sibling functions, not parameterised dispatch — `solve_elastic_static`, future `solve_modal`, future `solve_thermal_static`. Mirrors SciPy `linalg.solve` / `linalg.eigh` style. Cheap to ship, easy to specialise.

**Boundary conditions as plain stdlib structs**, not a dedicated declaration form. Joint syntax (`kinematic-constraints.md`) earned its dedicated form through usage volume; FEA hasn't. Revisit if v0.4+ usage proves it warrants its own grammar.

**GUI rendering deferred.** Headless-first. Stress contour plots, deformed-shape mode, probe-point readouts tracked as a separate GUI milestone.

**Mesher: Gmsh, library-linked, fed by the existing v0.1/v0.2 surface-mesh path.** Reasoning:

| Candidate         | License (AGPL fit) | Strengths                                                          | Weaknesses                                                |
|-------------------|--------------------|--------------------------------------------------------------------|-----------------------------------------------------------|
| TetGen            | research-only — **out** | Mature, fast, simple                                          | License incompatible with AGPL distribution               |
| **Gmsh**          | GPL-2.0 — fine     | Mature, fast, deterministic in serial, broad algorithm choice, library API | Standard CAD-mesher pain on tight features              |
| MMG3D             | LGPL — fine        | Anisotropic adaptation, quality improvement                        | Primarily a *remesher* — wants a starting tet mesh, not a from-scratch tool for v0.3 |
| fTetWild          | MPL — fine         | Robust on imperfect surface input; eats triangle soup              | Slower (minutes vs seconds for moderate models)           |
| CGAL mesh_3       | GPL — fine         | Highest output quality (Delaunay refinement)                       | Heavy build (templates, GMP/MPFR, fiddly CMake), weak Cargo binding |

Gmsh wins on the speed/maturity/build-cost balance and is deterministic in single-threaded mode. fTetWild is documented as the fallback if surface-mesh quality from OCCT BRepMesh causes Gmsh failures in practice (sliver triangles, near-coincident vertices around tight features). MMG3D becomes attractive in v0.4+ if we add adaptive refinement driven by an a-posteriori error estimator.

**`ReprKind` extension.** Adds `VolumeMesh` variant alongside the v0.2 `BRep | Mesh | Sdf | Voxel`. Non-breaking minor as the multi-kernel PRD anticipated. Surface-mesh realizations remain `Mesh`; volume tet meshes are `VolumeMesh`. Cache keys correctly distinguish.

## Open design questions

- **Failure-mode reporting policy.** What does "non-converged", "ill-conditioned K", "no supports", "load applied to interior" look like as Reify diagnostics? Need a triage policy: small fixed set of well-known failure causes mapped to actionable messages, everything else surfaces as "solver internal error" with internals attached. Decide during the diagnostic-mapping task itself rather than up-front — not a queueing blocker.

## Decomposition plan

Twenty tasks. Several depend on v0.2 work (multi-kernel mesh path, per-purpose tolerance, topology selectors); those gates are noted per task. Material/BC/reduction tasks are independent and can land any time the PRD activates.

**Stdlib surface (independent of v0.2 gates):**

1. `ElasticMaterial` trait + starter material library (`Steel_AISI_1045`, `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic`) with per-property provenance metadata. Fields: `youngs_modulus : Pressure`, `poisson_ratio : Number` (constrained `[0, 0.5)`), `density : Density`, `yield_stress : Pressure` (optional).
2. `Load` stdlib hierarchy: `PointLoad`, `PressureLoad`, `TractionLoad`, `BodyForce` / `Gravity`, all targeting topology selectors. Constructor signatures + selector validation.
3. `Support` stdlib hierarchy: `FixedSupport`, `DisplacementSupport`, `RollerSupport`. Constructor signatures + selector validation.
4. `ElasticOptions` stdlib type (element order, mesh-size override, max iterations, convergence tolerance) + `ElasticResult` struct shape (displacement field, stress field, max von Mises, converged, iterations).
5. `von_mises(stress: Tensor<2,3,Pressure>) -> Pressure` and `principal_stresses(...)` tensor reductions in stdlib.
6. Field `max` / `min` / `argmax` reductions over `Field<_, T : Ordered>` in stdlib.

**Solver kernel crate (depends on tasks 1–4):**

7. `reify-solver-elastic` crate skeleton + P2-tetrahedron reference element: shape functions, gradients, Gauss quadrature.
8. Element-level stiffness assembly under isotropic linear-elastic constitutive law (engineering strain, Voigt notation).
9. Global sparse-matrix assembly via `faer-rs` (CSR format, deterministic insertion order).
10. Dirichlet BC application (row-elimination preferred over penalty for cleaner conditioning).
11. Neumann BC application: surface-traction integrals over selector-targeted faces, body-force integrals over the volume.
12. CG solver with diagonal (Jacobi) preconditioner via faer-rs. AMG preconditioner deferred — Jacobi is enough for v0.3 first-cut.
13. Result interpolation: evaluate displacement at any point as `Field<Point3, Vector3<Length>>`; recover stress at any point as `Field<Point3, Tensor<2,3,Pressure>>` (gradient recovery from displacement, not separate solve).
14. Warm-state plumbing: prior-iterate carry-through across solves; opaque state attached to ComputeNode.

**Engine integration (depends on tasks 7–14, plus v0.2 gates):**

15. `solve_elastic_static` `@optimized` registration + ComputeNode wiring: cache-key composition over (realization hash, value hashes, options hash), dependency-edge declaration, OpaqueState attachment. **Gate:** per-purpose tolerance live.
16. Surface-to-volume tet meshing via Gmsh: extends `ReprKind` with `VolumeMesh` variant; consumes the existing v0.1/v0.2 surface-mesh path; emits second-order tet meshes. Includes a small surface-mesh-repair pre-stage (sliver collapse, near-coincident vertex merging) to keep Gmsh's failure rate manageable on OCCT BRepMesh output. **Gate:** v0.2 multi-kernel basis (task 2640) shipped so the surface-mesh path is stable.

**Validation & polish:**

17. Determinism harness — bit-stable assertion across repeated runs and across thread counts.
18. Validation suite against analytical references: cantilever beam (tip deflection), pressurised thick-walled cylinder (radial stress profile), simple shear (uniform stress), Boussinesq half-space point load. Tolerance comparisons per case.
19. Diagnostic mapping for common failure modes: under-constrained body (rigid-body modes), singular K (degenerate mesh), non-convergence, no loads, load-on-interior, BC-on-non-existent-selector. Each mapped to an actionable Reify diagnostic.
20. End-to-end example file: bracket with `param thickness : Length = auto`, `minimize mass subject to max(von_mises(bracket)) < material.yield_stress`. Closes the design loop demo from the Goal section.

## Out of scope for this PRD

- Modal / eigenvalue analysis — sibling PRD (`structural-analysis-modal.md`) once linear-static lands.
- Transient / dynamic analysis — sibling PRD; needs time-stepping ComputeNode trait extensions.
- Material non-linearity (plasticity, hyperelasticity) — separate PRD per material model.
- Contact, friction, sliding interfaces — separate PRD (large feature, often per-solver-specific).
- Large-deformation / geometric non-linearity — separate PRD.
- Thermal, fluid, electromagnetic, multi-physics coupling — separate PRDs each.
- Adaptive mesh refinement driven by error estimators — useful follow-on; ships after the static path is stable.
- GUI rendering of stress / displacement fields — tracked as a GUI milestone; headless path lands first.
- Mesh authoring UI — Reify generates meshes from B-rep; users do not hand-mesh in v0.3.
- Optimization-for-FEA (topology optimization, density-field design) — promising future direction, but a v0.4+ feature once the basics are mature.
