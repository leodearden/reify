# PRD — Meshing service: face identity on every mesh, gmsh in a worker

> **Status:** active — authored 2026-10-06 via `/prd` (Leo + Claude, agent team), from the
> adaptive-boundary-fidelity investigation (2026-10-05/06) and a stringent design review whose
> decisions Leo ruled the same day (D-A–D-J, Q-x, Q-y), plus five rulings on this draft (Q-a–Q-e,
> 2026-10-07; §4). An adversarial review of the draft is folded in.
>
> **Milestone:** v0.6. **Approach:** B + H (FEA, multi-kernel and the realization seam are
> load-bearing; eight crates change). Contract in §5, boundary tests in §6.
>
> **Code anchors** are cited by symbol. Substrate was read on main `fca4a9ad5f`–`dcd3c5a90a`
> (2026-10-06). The measurements in §3 (m4, m5) ran on libgmsh 4.15.2 from `/opt/reify-deps`; their
> scripts and raw output are parked at commit `8ce250b5a9` on the throwaway branch
> `task/adaptive-boundary-fidelity-probe` (`probe/m4/`, `probe/m5/`; never merged).

---

## 1. Goal and consumers (G1)

Every volume mesh reify produces from a B-rep knows which B-rep face, edge and vertex each boundary
node and boundary triangle came from: on the seed mesh, on every adaptive refine, and on every body
the CLI or GUI solves. gmsh runs in a separate worker process, so a gmsh crash, hang or OCCT clash
becomes a coded error instead of a dead GUI.

The user-facing payoff is a face-selector boundary condition on a realized body that clamps the
whole face, rims included, before and after adaptive refinement. Today no production binary meshes
a body, a face selector on a body is coded-refused (Q-x), refinement output carries no face
identity, and the attributed producer (tests only) clamps a face without its rims. Selector-typed
FEA targets are #5312's and their solver consumption #5313's; this program supplies the identity
they resolve against, and its integration-gate leaves λ and μ (§7) observe the clamp end to end.

| Mechanism | Consumer | Observed by |
|---|---|---|
| `BoundaryTopology` from OCCT tessellation (§5 C1–C2) | The meshing service (C4); #8078 (face triangles for selector loads) | Leaves α, ε |
| `reify-meshing` facade, one gmsh sequence, typed `MeshError` (C3–C4) | The VolumeMesh realization edge (engine-integration-norm §3.2), the realized adaptive lane, the hex/wedge sweep's 2D profile mesher | Leaves β, γ |
| Entity-per-face identity through gmsh; extended `BoundaryAssociation`; `face_closure_nodes` (C5–C6) | Face-selector BCs on realized bodies (#5313, the #4092 path in `bc_resolve`); `reify-mesh-morph`'s `compute_dirichlet_bcs` (already reads `OnEdge`/`OnVertex`); #8078 | Leaves δ, ε, λ |
| `RemeshFactory` on the read handle; `MeshSession` (C7–C8) | `RealizedAdaptiveProblem`; #8248's selector trigger is deleted; coordinate-target-fea θ #8257 and ι #8258 | Leaves ζ, μ |
| Per-options VolumeMesh variants realized at dispatch; meshing options in the cache key (C10–C11) | `ElasticOptions.mesh_size` (declared, zero production readers today); later `element_order` (#7075); #2953's mesh-size lever | Leaf η |
| Morph as a second seed producer, with the body's remesh capability (C7) | `reify-mesh-morph` in the CLI and, once #2953 re-enables it, the GUI; FEA warm start across edits (#2952) | Leaves ε, η |
| Default sizing policy and element budget (C10) | Every body solve without `mesh_size` | Leaf θ |
| The worker process (C9) | The CLI and GUI (crash containment); #7440/#8269 (one OCCT per process); #4289 (its XCAF hazard ends) | Leaves ι, κ |

**Engine seam (overlay G1 sub-check).** The service plugs into engine-integration-norm
(`docs/prds/v0_3/engine-integration-norm.md`) §3.2 (realization-kind dispatch) at
`Engine::execute_realization_ops`'s VolumeMesh edge. The topology side channel is a §3.1
`GeometryKernel` method on the OCCT kernel. gmsh stops being a `GeometryKernel` (§3.1): it is
reached only through the service. No new seam; leaf ν updates the norm's §3.1/§3.2 rows.

## 2. Background: what exists (main, 2026-10-06)

**Four gmsh producers, three of them near-copies.**

- PLAIN — `GeometryKernel::mesh_surface_to_volume` → `GmshKernel::mesh_to_volume`: always welds,
  sizes uniformly at the minimum OCCT triangle edge (`auto_size::auto_mesh_size_from_features`),
  `NumThreads` = nproc (so it drifts run to run), arms `LogCapture`, reports fill and discards it.
- ATTRIBUTED — `mesh_surface_to_volume_attributed`, compiled only under reify-kernel-gmsh's
  `mesh-morph` cargo feature, which only dev-dependencies enable: `mesh_boundary.rs::
  run_meshing_with_entity_queries` "deliberately mirrors" the plain sequence, writes no size option
  (gmsh defaults), runs one thread, and attributes classify patches to B-rep faces by nearest centroid
  within `0.3 × min bbox extent`. Edge and vertex anchors are passed empty and per-entity node
  queries exclude boundary nodes, so rim and corner nodes get no attachment (#7277).
- REFINE — `refine_volume.rs::refine_volume_with_size_field`, called directly from
  `reify-solver-elastic`'s `refine_marked_elements`: a third copy, `classify(π/12)`, a PostView
  background field (one thread only, or it deadlocks: `docs/notes/gmsh-postview-background-field-
  threading.md`), output `boundary: None`.
- 2D PROFILE — `mesh_profile_2d.rs::mesh_plane_2d`, called directly from `reify-solver-elastic`'s
  `mesh_swept_profile_2d` on the production hex/wedge sweep path (`sweep_classifier`).

The seed classify angles live in two named constants (π/4); refine keeps its own π/12. The 2D and
refine stubs signal "gmsh absent" with `STUB_UNAVAILABLE_MARKER`, which two consumers detect with
`msg.contains` (the two `map_geometry_error` functions in `mesher.rs` and `volume_refine.rs`).

**Face identity is produced and then dropped.** `occt_wrapper.cpp::tessellate_shape` meshes face by
face (`TopExp_Explorer`), but `TessResult` and `reify_ir::Mesh` carry only vertices, indices and
normals. Nodes are not shared across faces, degenerate triangles are dropped, and a face with no
triangulation is skipped silently. No code reads `BRep_Tool::PolygonOnTriangulation`. A face handle
(`GeometryHandleId`) is a kernel-local counter minted by `extract_faces`; the stable positional
identity is the `TopExp::MapShapes` ordinal, which `extract_faces/edges/vertices` all follow (the
`OcctShape` map caches).

**`BoundaryAssociation`** (`reify-ir/src/boundary_attachment.rs`) maps each node to ONE
`NodeAttachment::{OnFace, OnEdge, OnVertex}`. Every face-selector reader (`bc_resolve::
boundary_node_set`, `topology_selectors::nodes_for_faces`) keeps `OnFace` only, so even with edge
attachments a face clamp would miss its rim. `reify-mesh-morph`'s `compute_dirichlet_bcs` is the one
reader of all three variants.

**The engine edge.** `execute_realization_ops` tessellates the terminal B-rep into a local surface
`Mesh`, tries the attributed producer only when the target demands a boundary (`elastic_static`
alone does) and face anchors exist, degrades to plain on any failure, and stores the result through
`gmsh.store_volume_mesh`: the terminal is a `KernelHandle { kernel: KernelId::Gmsh, .. }`, cached as
`RealizationCache<KernelHandle>` under `NO_OPTIONS` and only when a tolerance is demanded, and
projected back through `resolve_realization_kernel` → `GeometryKernel::volume_mesh`. The morph arm
stores through the same path. `reify_kernel_gmsh::volume_mesh_cache_key` and
`VolumeMeshOptions::content_hash` have no production caller.

**Reachability.** `reify` and `reify-gui` already link libgmsh (reify-eval → reify-solver-elastic →
reify-kernel-gmsh) and load OCCT 7.8 directly and 7.9 through libgmsh, with unversioned symbols
(#7440). But `Engine::with_registered_kernel` registers only the lex-min B-rep kernel, so no body
solve meshes in production. #6660 (depends on #7052) registers gmsh in-process on the plain producer
and coded-refuses selector BCs on bodies (Q-x). Body solves are redispatched with
`options = Value::Undef` and a constant persistent-cache key; #7052 makes the options value present.

**The adaptive realized lane** (`RealizedAdaptiveProblem`, `elastic_static.rs`, 14,463 lines)
extracts the boundary of the seed volume mesh once (`boundary_surface_mesh`) and remeshes from that
frozen surface on every refine. `ComputeFn` has no kernel or service parameter; the read handle
carries `{Sdf | SurfaceMesh | VolumeMesh}` plus `boundary()`.

**Morph is registered and dormant.** The CLI and the shipping GUI (`gui` feature) both register
`reify_mesh_morph::register_morph_producer` (`MorphRegistration::Enabled`); only the non-`gui` lib
build passes `Unavailable`. Morph engages only when a prior source mesh of the same realization in the
same engine carries a non-empty `BoundaryAssociation`, which production never produces today; the CLI
builds once per command, so it never has a prior source. Stage A eligibility is fixed (#6635, #6643).
`Engine::on_refine_trigger` (#4945; semantics #3000), which should force a fresh remesh at settled
moments, has no production caller. After #7834, morph took 5.6–6.2 s against gmsh's 1.6–1.8 s on a
108,756-tet bracket and was ~4× faster at 10k tets (`docs/notes/morph-vs-remesh-scale-
characterisation.md`, "After the Dirichlet fix"); its payoff is FEA warm start (connectivity, and so
the DOF count, is preserved), which #2952 measures and which no test has yet observed through the
engine.

**FEA targets are strings.** `FixedSupport.target` is `String` in `fea_multi_case.ri`; typing it as a
selector is #5312 (blocked on #8102), and consuming selectors in the solver is #5313.

## 3. Substrate verification (G3) and measurements

| Assumed capability | Verdict | Evidence |
|---|---|---|
| OCCT edge polygons on a face triangulation (`BRep_Tool::PolygonOnTriangulation`) | **Exists** in OCCT 7.8.1; **unread** in reify — α reads it | `BRep_Tool.hxx`, `Poly_PolygonOnTriangulation.hxx` (1-based nodes of the face triangulation) |
| `MapShapes` ordinal as the shared face/edge/vertex order | **Exists** | `OcctShape` map caches; C++ `get_vertices`; the `extract_*` handles follow it |
| gmsh discrete entities with boundary arrays, nodes per dim, elements per entity, `classifySurfaces`, `createGeometry`, `getEntities` | **Exists** (FFI bound); wrappers hard-code dim 2 / tag −1 — β generalises them | `ffi.rs` externs; `add_nodes_2d`, `get_elements_by_type` |
| Entity-per-face identity survives gmsh meshing | **Exists, by a different mechanism than the review assumed** — m5 | probe/m5 |
| A worker round trip is cheap next to meshing and solving | **Exists** — m4 | probe/m4 |
| A versioned binary wire format | **Exists** | workspace `bincode = "=1.3"` (pinned under `ELASTIC_RESULT_FORMAT_VERSION`'s discipline), `serde` |
| Shipping an extra binary in the GUI bundle | **Partial** — `bundle.externalBin` ships the Node `reify-sidecar` (built by `gui/sidecar`, stubbed by `scripts/ensure-gui-sidecar-placeholder.sh`); nothing copies a cargo binary there today — ι adds it | `tauri.conf.json` |
| tbb pin and RUNPATH for a new gmsh-linking binary | **Exists** (pattern) | `emit_tbb_pin_for_bins` before `emit_rpath_for_bins(NativeDep::Gmsh)`, as in `reify-cli/build.rs`; no new `NativeDep` arm |
| The solve's evaluated options at dispatch | **ABSENT on the body path** — #7052 makes them present; η consumes them | `redispatch_geometry_consuming_compute_nodes` passes `Value::Undef` options |
| Per-options variants of one realization | **ABSENT** — η builds them | one VolumeMesh realization per entity; the cache key (entity, repr, tol, options_hash) can hold several |
| An engine-owned VolumeMesh value in the realization store | **ABSENT** — γ builds it | the terminal is a gmsh `KernelHandle` today |
| Selector-typed FEA targets in `.ri` | **ABSENT** — #5312, #5313 | `FixedSupport.target : String` |

**m5 — entity-per-face through gmsh** (cylinder; one-edge and all-edge filleted blocks; plate with a
hole; sphere; torus; gmsh's own OCC tessellation stood in for the side channel, so the input was
conforming by construction):

- `createGeometry` never split a face. It parametrises the whole face (planar annuli, fillet strips,
  the cylinder wall once the seam is not a boundary curve) or **fails** on closed faces (sphere,
  torus: "Invalid exterior boundary mesh for parametrization"). A seam passed as a twice-used surface
  boundary curve makes the 2D remesh fail when no classify step runs.
- **Element tags do not survive.** `generate(3)` remeshes 1D and 2D; HXT re-creates even untouched
  boundary triangles with new tags; with the default `Mesh.Renumber=1` most new tags land back in the
  input range, so a tag-based check passes falsely (cylinder: 1,140 of 1,318 in range, 784 at the
  right face). `gmshModelGetParent` returns (−1, −1).
- **What works on all six bodies (`epfcls`, seams and degenerate edges kept as input curves):**
  entity tags equal to the B-rep ordinals; `classifySurfaces(π, 1, 1, π, 0)` on the entity-per-face
  input — it splits only where parametrisation needs it (sphere, torus), never merges input faces,
  and removes seam and zero-length curves itself; then `createGeometry`; the child-surface →
  parent-face map read from the input triangle tags **before** `generate`; curves mapped to B-rep
  edges by the pair of parent faces on either side (same parent on both sides marks a synthetic cut);
  points mapped by the input node they hold. Every child surface was pure to one face; no node was
  classified on two entities; every rim node lay on its B-rep edge polyline (distance 0); tangent
  fillet edges were kept.
- The same input under the classify pipeline lost tangent faces (all-edge filleted block: 26 faces →
  2 surfaces) and over-split (plate: 7 → 20).
- Without a discrete volume in `createGeometry`, `generate(3)` returns zero tets with no error.
- `Mesh.MeshSizeFromCurvature` acts on the discrete parametrisation and can multiply counts many
  times (torus 352 → 30,054 tets at 12 per 2π). A PostView-style background field refines identity-
  bearing meshes the same way (cylinder 4,243 → 29,119 tets).
- `NumThreads=1` with HXT: node coordinates and tets bit-identical across three processes on one
  host, all bodies (cross-host identity not measured).
- The plate (8.5k input triangles) meshes in 0.84 s.

**m4 — worker round trip** (host load 190–490 on 32 cores; absolute numbers inflated): cold start
(dlopen + `gmshInitialize`) 0.16 s median; warm per-request overhead 0.9 ms; reading a 0.9M-tet mesh
out of gmsh into flat arrays 0.5–1.5 s through Python ctypes (an upper bound for a native worker);
moving it (18 MB) over a pipe 15–30 ms. A crashing child shows as pipe EOF plus exit signal −11,
1.2 s after the fault on this host (apport's pipe `core_pattern` holds it); a hung child is caught by
a parent deadline and SIGKILL, reaped in 20–38 ms. Against meshing (≈5 s per 0.9M tets) the worker
adds ~1 s at that size; against the Jacobi-CG solve (10–20 min) it is under 0.2%.

**G6 premise status.** Identity: established (live consumers; classify measured to over-split and to
lose tangent faces). Robustness: established (#4876's SIGSEGV, the 25-minute HXT spin recorded in
#7584, the PostView deadlock). Fidelity of CAD mode, P2 midside placement or a finer frozen surface:
**not established** — gated on #8270. Meshing performance: **not a lever** — the solve is (#8271).

## 4. Resolved design decisions

**Ruled by Leo, 2026-10-06** (from the design review; restated, not relitigated):

| # | Decision |
|---|---|
| **D-A** | gmsh runs in a **worker subprocess**: a separate binary that links libgmsh and not reify-kernel-occt; a versioned request/response protocol, a timeout and a crash boundary; a `MeshSession` kept alive across adaptive refines; `GMSH_LOCK`/`GMSH_DEAD` leave reify-eval. This, not "single OCCT in process", is the prerequisite for CAD mode, and it removes the dual OCCT load from the reify process. |
| **D-B** | **Order:** the topology side channel first, then ONE producer (plain, attributed and refine collapse into one gmsh sequence). Attribution is never turned always-on from classify patches. |
| **D-C** | The mode predicate is **capability + budget**, never provenance. CAD mode only if the B-rep is present, its faces analytic/NURBS, the face count within budget, and import + heal succeed within the timeout; otherwise discrete. A per-solve `MeshMode {Auto, Cad, Discrete}` override and a coded Info naming the mode used. |
| **D-D** | **Sizing:** the user's `mesh_size` always wins. Default = min(bbox-relative cap, thickness/2, curvature term), with a floor and an element budget and a coded Warning when the budget binds. The min-edge term stays a floor contributor until a measurement retires it. |
| **D-E** | Face identity lives in a **separate `BoundaryTopology` value** paired with `Mesh` (`TessellatedBody { mesh, topology }`), never in new fields on `Mesh`, and is carried on `VolumeMesh` as `BoundaryAssociation` extended with edges and vertices. |
| **D-F** | Fidelity increments (CAD mode, P2 with geometry-placed midside nodes, a fine frozen surface) are **gated on #8270**. Identity and robustness proceed now. |
| **D-G** | Solver performance is the wall-clock lever and is **out of scope** (#8271); it is why parallel meshing is not pursued. |
| **D-H** | Assemblies and multi-solid bodies: one session per body, **non-conformal between bodies**, v1. |
| **D-I** | SDF / voxel / STL bodies keep **classify as the explicit no-topology fallback**, with coded diagnostics; selector BCs on such bodies are a coded Error. |
| **D-J** | Per-entity parallel meshing is **dropped**. Determinism stays at `NumThreads=1`, with no `RandomSeed`. |
| **Q-x** | #6660 ships gmsh registration on the plain producer only; selector BCs on bodies are coded-refused until this program's identity lands. The unified producer deletes the `mesh-morph` gate. |
| **Q-y** | #6660 depends on #7052. |

**Corrected by measurement (G6).** D-E's sub-claim that identity "rides on ELEMENT tags, because
createGeometry splits non-disk faces" is false on both counts (m5). Identity rides on **entity**
tags and a child → parent map taken before generation (C4–C5). D-E's ruling — a separate value,
carried through `BoundaryAssociation` — is unchanged.

**Ruled by Leo on this draft (2026-10-07):**

| # | Question | Ruling |
|---|---|---|
| **Q-a** | Morph becomes eligible once meshes carry identity | **Morph is a second seed producer.** The edge computes the body's topology once, seeds by morphing a prior mesh of the same options or by meshing, validates a morphed mesh's re-keyed association against the new body with the same invariants as a fresh one, and stores either seed with the body's `RemeshFactory`. The morph source is keyed by `options_hash`. The CLI registration stays `Enabled` (it builds once per command, so it never morphs). |
| **Q-b** | D-D's min-edge floor equals the thickness on any prismatic slab, so thickness/2 can never bind | **The floor is an absolute floor only**; the min OCCT edge is reported in `MeshModeUsed`, not used. This is the measurement D-D named. |
| **Q-c** | Where `mesh_size` (and later `element_order`) reaches the mesher | **The engine realizes per-options VolumeMesh variants at dispatch**, where the consuming node's evaluated options exist (#7052), cached by `options_hash` (C7, C10, C11). |
| **Q-d** | D-C's `MeshMode` override when only one mode exists | **The coded Info ships now; the override arrives with the CAD-mode PRD.** A knob with one effective value would be declared and inert. |
| **Q-e** | GUI morph wakes up the moment identity lands (ε), before settled-moment remeshing or any warm-start evidence | **ε declares the GUI's dormancy**: `MorphRegistration::Unavailable { reason }` naming #2953. A new task wires the GUI's settled-moment `on_refine_trigger` call; #2953 owns flipping the GUI back to `Enabled`, after that task and #2952. #7836 stays the morph-performance decision on #2952's numbers. |

**Decided this session** (design within the rulings):

| # | Decision | Why |
|---|---|---|
| **S1** | **One PRD, two phases**: identity (α–θ, λ, μ) and isolation (ι, κ). | The leaves share one contract (§5); two PRDs would split the facade's owner from its first consumer. |
| **S2** | **Entity-per-face on every B-rep-sourced mesh**, not only where a target demands a boundary; the `demanded_boundary` plumbing is deleted. | Identity is exact and cheap (one extra `extract_*` round trip); an identity-free second path would be a second producer (D-B) and an axis with no consumer (heuristic 3). Every realized mesh's node order and boundary triangulation change once; recorded figures that move stay inside their ruled tolerances, and one that leaves its bound is escalated, never widened. |
| **S3** | **gmsh stops being a `GeometryKernel`.** Its meshing trait methods, its `inventory` registration and `KernelId::Gmsh` retire; VolumeMesh terminals are engine-owned values. | A worker cannot sit behind an in-process trait object; gmsh implements no geometry operation (heuristic 6). |
| **S4** | **Remeshing reaches the trampoline through the read handle**, as a narrow `RemeshFactory` capability; `ComputeFn` is unchanged. | The refine geometry is the seed's own input (heuristic 11); no global registry and no CN-contract signature change (heuristic 7); the handle carries only `open_session`, not the whole service (heuristic 9). Producers still resolve at §3.2 (seeds, including per-options variants, are realized by the engine); the factory only re-runs the producer the edge chose. |
| **S5** | **Refine remeshes from the seed's input tessellation**, not from the seed mesh's boundary. | It is the only surface that carries topology. It is not the gated "fine frozen surface": no re-tessellation, the seed's OCCT tolerance. |
| **S6** | **`BoundaryAssociation` keeps one attachment per node** (the lowest-dimension entity, gmsh's own node classification), and gains the face → edge → vertex adjacency and per-face boundary triangles; one resolver, `face_closure_nodes`, gives a face's interior + rim + corner nodes. | Multi-attachment would copy the adjacency into every node; consumers needing a rim ask the one resolver (heuristic 11). #8078 gets exact face triangles instead of re-deriving them from node sets. |
| **S7** | **The 2D profile mesher moves behind the facade too** (a second request kind) with its own gmsh sequence. | The worker must carry every gmsh call or `reify` keeps linking libgmsh; a 2D `.geo`-API profile shares nothing with the 3D discrete sequence. |
| **S8** | **The no-topology classify angles keep today's values** — π/4 uniform, π/12 size field — in one policy table. | Preserves the fallback's behaviour; one home for both (heuristic 11). Topology-bearing input classifies at π only to split. |
| **S9** | **Identity and the worker are independent leaves.** ε does not wait for ι. | #6660 already runs gmsh in-process; entity-per-face uses the same gmsh stages as the plain producer. |
| **S10** | **The face clamp is observed by two integration-gate leaves** (λ single-shot, μ adaptive) that depend on #5313; Q-x's refusal is removed by λ and #8248's selector trigger by μ. | A selector target cannot be written in `.ri` before #5312 nor consumed before #5313; removing a refusal before consumption is real would trade a coded Error for a wrong clamp. |
| **S11** | **The remesh capability belongs to the body, not to how its seed was made** (Q-a). | One realization function seeds by morph or by mesh and stores the same `{seed, factory}` shape either way; no second-class seed and no special refusal (heuristics 3, 6). |

## 5. Contract (the H half)

### C1. `BoundaryTopology` and `TessellatedBody` (reify-ir)

```rust
pub struct TessellatedBody { pub mesh: Mesh, pub topology: BoundaryTopology }

pub struct BoundaryTopology {
    pub face_of_triangle: Vec<u32>,          // face ordinal per triangle of `mesh`
    pub edges: Vec<EdgePath>,                // one per edge ordinal
    pub vertices: Vec<u32>,                  // node index per vertex ordinal
    pub solids: Vec<Vec<u32>>,               // face ordinals bounding each solid ordinal
}
pub struct EdgePath {
    pub nodes: Vec<u32>,                     // ordered, first..last vertex; one node if degenerate
    pub faces: Vec<u32>,                     // incident face ordinals (1 or 2)
    pub kind: EdgeKind,                      // Regular | Seam | Degenerate
}
```

- Ordinals are `TopExp::MapShapes` positions (0-based) for faces, edges, vertices and solids — the
  order `extract_faces/edges/vertices` already follow. Handles are not stored here; the engine maps
  ordinals to handles (C7).
- `mesh` is **welded topologically**: nodes on an edge are shared by its faces through that edge's
  polygon on each face triangulation, not by a distance tolerance. `mesh.normals` is `None`.
- **Invariants** — one function, `BoundaryTopology::validate(&Mesh) -> Result<(), TopologyInvariant>`,
  called where the value is built and again where the service accepts it (heuristic 10):
  `face_of_triangle.len() == indices.len() / 3`; every face ordinal owns ≥ 1 triangle; every regular
  edge's nodes are shared by exactly the triangles of its listed faces; every vertex node ends each
  edge path that meets it; every solid lists ≥ 1 face. A violation is a typed `TopologyInvariant`
  naming the invariant and the offending ordinals and counts.
- `Mesh` gains no field (D-E).

### C2. The OCCT side channel (reify-kernel-occt)

- `GeometryKernel::tessellate_with_topology(handle, tol) -> Result<TessellatedBody, GeometryError>`,
  default `Err` (the additive trait-method pattern); the OCCT kernel implements it through a new
  `OcctRequest` variant on `OcctKernelHandle`.
- One C++ core shared with `tessellate_shape`: `BRepMesh_IncrementalMesh` once, then faces in the
  `MapShapes(FACE)` order, edge polygons with `BRep_Tool::PolygonOnTriangulation(edge, triangulation,
  location)` per incident face, vertices from `MapShapes(VERTEX)`, solids from `MapShapes(SOLID)`.
  Edge polygons are read from the same triangulations as the face blocks, in the same call.
- A face whose triangulation is null, or an edge whose polygon on an incident face is null, is an
  **Error** naming the ordinal (today a null face is skipped silently). Degenerate triangles are
  dropped as today; `face_of_triangle` counts emitted triangles, not `NbTriangles()`.
- Seam edges (`BRep_Tool::IsClosed(edge, face)`) are `EdgeKind::Seam`; zero-length edges (sphere
  poles) are `Degenerate`. Both are passed to gmsh like any edge; classify removes them (m5) and their
  nodes attach to the face (C5).
- Consumers outside meshing keep `tessellate`; nothing else changes.

### C3. The facade (`reify-meshing`, a new crate)

```rust
pub enum GeometrySource { Topological(Arc<TessellatedBody>), Surface(Arc<Mesh>) }   // Surface: D-I's fallback
pub enum Sizing { Uniform { h: f64, floor: f64, curvature_per_2pi: u32 }, Field(Arc<BackgroundSizeField>) }
pub struct MeshRequest { pub geometry: GeometrySource, pub sizing: Sizing, pub order: ElementOrderTag,
                         pub budget: u64 }
pub struct MeshReport  { pub volume: VolumeMesh, pub identity: Option<MeshIdentity>,   // ordinal-keyed, C5
                         pub mode_used: MeshModeUsed, pub provenance: MeshProvenance,
                         pub diagnostics: Vec<MeshDiagnostic> }
pub enum MeshModeUsed { EntityPerFace, Classified }          // Cad joins with the CAD-mode PRD
pub struct MeshProvenance { pub gmsh_version: String, pub protocol_version: u32,
                            pub options_hash: ContentHash, pub binding_term: SizingTerm, pub h: f64 }
pub enum MeshError {
    BackendUnavailable { reason: String },                   // no worker binary, version mismatch, no service
    Timeout { after: Duration },
    BackendCrashed { signal: Option<i32>, exit: Option<i32> },
    Geometry { stage: MeshStage, gmsh_log: Vec<String> },    // gmsh refused, at a named stage
    TopologyInvalid(TopologyInvariant),
    EmptyResult { stage: MeshStage },                        // zero tets / zero triangles
    BudgetExceeded { estimated: u64, budget: u64 },
}
pub trait MeshService: Send + Sync {
    fn mesh(&self, req: &MeshRequest) -> Result<MeshReport, MeshError>;
    fn mesh_profile_2d(&self, req: &ProfileRequest) -> Result<ProfileMesh, MeshError>;
    fn open_session(&self, geometry: GeometrySource, order: ElementOrderTag, budget: u64)
        -> Result<Box<dyn MeshSession>, MeshError>;
}
pub trait MeshSession: Send { fn remesh(&mut self, sizing: &Sizing) -> Result<MeshReport, MeshError>; }
```

- **Homes.** `reify-meshing` depends on reify-ir, reify-core, serde and bincode only. β **moves** into
  it from reify-kernel-gmsh: `BackgroundSizeField` (with its validation), the classify-angle policy
  table (S8) and the sizing types. `MeshStage`, `SizingTerm`, `MeshDiagnostic`, `ProfileRequest`,
  `ProfileMesh` and `TopologyHandles` are defined here. `MeshingOptions`' thread and determinism fields
  are deleted (D-J); its element order becomes the request's `order`.
- Every request is self-contained (heuristic 7); a session is the one piece of owned durable state,
  scoped to one adaptive loop.
- Threads are not a field: gmsh always runs at `NumThreads=1` (D-J). No determinism enum, no seed.
  The deadline is the worker client's configuration (C9), not a request field, so the in-process
  service declares nothing it cannot enforce.
- `MeshError` is the only failure channel. Nothing downstream matches message text; the
  `STUB_UNAVAILABLE_MARKER` constant, both `msg.contains` sites and `GMSH_AVAILABLE` are deleted
  (INV-SF-6, heuristic 12).
- `MeshModeUsed` declares only modes that exist (Q-d).

### C4. One gmsh sequence (reify-kernel-gmsh)

`reify_kernel_gmsh::run_mesh(&MeshRequest) -> Result<MeshReport, MeshError>` replaces the plain,
attributed and refine producers. In order:

1. **Scope.** `init::lock`, `MeshSizeScope`, `clear`, `General.Terminal=0`, `LogCapture` armed,
   `General.NumThreads=1`, `Mesh.Algorithm3D=10`, `Mesh.ElementOrder`.
2. **Discrete model.** `Topological`: one discrete point per vertex ordinal, one discrete curve per
   edge ordinal of every kind (boundary = its end points), one discrete surface per face ordinal
   (boundary = its edges), one discrete volume per solid (boundary = its faces); each entity's tag is
   its ordinal + 1. Nodes go on the lowest-dimension entity holding them (a vertex node on its point,
   edge-interior nodes on the curve once, the rest on the surface); triangles go on their face's
   surface with element tags `1..N` recorded against `face_of_triangle`. This is m5's measured
   `epfcls` input shape. `Surface`: one discrete surface and one volume, as today.
3. **Classify / split.** `Topological`: `classifySurfaces(π, 1, 1, π, 0)` — splits closed faces into
   parametrisable children, never merges faces, removes seam and degenerate curves. `Surface`:
   `classifySurfaces` at the policy angle for the sizing kind (π/4 uniform, π/12 field; S8).
4. **Identity map, before generation.** For each child surface, read its input triangle tags and map
   it to the single parent face they belong to; a child holding triangles of two faces is
   `TopologyInvalid`. Map each curve to a B-rep edge by the unordered pair of parent faces of its
   adjacent child surfaces; a curve with the same parent on both sides is a synthetic cut and maps to
   that face; a face pair shared by two edges is tie-broken by coincidence with the edges' node paths.
   Map each point by the input node it holds: a topology vertex node maps to that vertex; any other
   node makes the point synthetic, mapping to its curve's edge or face. Element tags are never read
   after step 6.
5. **`createGeometry`**, including the volumes (without them `generate(3)` returns zero tets silently).
   **Sizing:** `Uniform` writes `MeshSizeMin = floor`, `MeshSizeMax = h`, `MeshSizeFromCurvature =
   curvature_per_2pi`; `Field` installs the PostView field through `BackgroundFieldGuard` (with its
   `view_probe` warm-up) and zeroes the other size sources.
6. **Generate** in two calls: `generate(2)`, then the **budget check** (C10) on the real surface mesh,
   then `generate(3)`, each through `mesh_generate_with_recovery`.
7. **Read back.** Nodes, tets (sorted by tag and remapped by one helper), and the boundary triangles of
   each child surface. Attach each boundary node by the entity gmsh classified it on, translated
   through the step-4 map. Zero tets is `EmptyResult`; a face ordinal with no boundary triangle is
   `TopologyInvalid`; today's plain-path stride and fill checks run for every request; the fill report
   joins `diagnostics`.
8. `clear`; scopes restore options and stop the logger.

The 2D profile request keeps `mesh_plane_2d`'s `.geo`-API sequence (S7), behind the same scope and
`MeshError`.

### C5. Identity through gmsh — what the report carries

`MeshIdentity` (ordinal-keyed; the engine converts it to handles, C7):

- `node_entity: BTreeMap<u32, EntityRef>` — exactly one entry per boundary node,
  `EntityRef::{Face(u32), Edge(u32), Vertex(u32)}`, the lowest-dimension B-rep entity holding it. Seam
  and synthetic-cut nodes are `Face`.
- `face_triangles: Vec<Vec<[u32; 3]>>` — the volume mesh's boundary triangles per face ordinal.
- Adjacency is not repeated here; it is the topology's (C1).

Invariants (checked in `run_mesh` and again where the engine builds the association): every boundary
node has exactly one entry; every boundary triangle belongs to exactly one face; every face ordinal
has ≥ 1 triangle; a `Vertex(v)` node coincides with topology vertex `v`'s input node.

### C6. `BoundaryAssociation`, extended (reify-ir)

```rust
pub struct BoundaryAssociation {
    nodes: BTreeMap<u32, NodeAttachment>,                       // unchanged
    adjacency: BoundaryAdjacency,                               // face → edges, edge → vertices (handles)
    face_triangles: BTreeMap<GeometryHandleId, Vec<[u32; 3]>>,  // per face, volume-mesh boundary
}
impl BoundaryAssociation {
    pub fn face_closure_nodes(&self, faces: &BTreeSet<GeometryHandleId>) -> Vec<u32>; // interior + rim + corners
    pub fn face_triangles(&self, face: GeometryHandleId) -> &[[u32; 3]];
}
```

- `bc_resolve::boundary_node_set` and `topology_selectors::nodes_for_faces` both call
  `face_closure_nodes` (heuristic 11): a face clamp includes its rims and corners.
- `reify-mesh-morph`'s `rekey_boundary_association` re-keys the adjacency and the face triangles,
  fail-closed as today; `CarriedTopology`'s value encoding round-trips the extension.
- Vertex- and edge-targeted selectors (#5313) read `nodes` and `adjacency`; this PRD supplies the
  data, #5313 the solver-side consumption.

### C7. The engine seam (reify-eval)

- **Registration.** `Engine::with_mesh_service(Arc<dyn MeshService>)`, installed by the CLI and GUI
  constructors where #6660 installs the gmsh kernel. With no service, a body solve gets
  `MeshBackendUnavailable` (Error), never a hollow result. Engine tests install the in-process service
  (reify-kernel-gmsh as a reify-eval dev-dependency); `ensure_gmsh_kernel` is deleted.
- **The meshable body.** In the build pass, the VolumeMesh edge (`execute_realization_ops`) realizes
  each VolumeMesh-demanded body's meshable form once. For a B-rep terminal: `tessellate_with_topology`
  plus the ordinal → handle map from `extract_faces/edges/vertices` (counts must equal the topology's,
  else `MeshTopologyInvalid` naming both). For a non-B-rep terminal (SDF, voxel, manifold mesh, STL):
  the surface `Mesh` (`GeometrySource::Surface`, no association). The body's `RemeshFactory` is built
  here; it belongs to the body (S11).
- **One realization function.** `realize_volume_mesh(body, request, morph_source) -> StoredMesh` seeds
  by morphing the prior mesh of the same realization and `options_hash` when a morph producer is
  registered and the edit is eligible, and otherwise by `MeshService::mesh`. It converts the
  `MeshIdentity` (or the morph's re-keyed association) to a `BoundaryAssociation` validated against
  the body's topology with C5's invariants, pushes `MeshModeUsed` naming the seed producer (mesh, or
  morph with the source mesh's hash), records the result as the morph source for (realization,
  `options_hash`), and returns `StoredMesh { volume, remesh: the body's factory }`.
- **Per-options variants (Q-c).** The build pass realizes the default variant (default options). At
  a consuming node's (re)dispatch — the site #7052 gives the evaluated options — the engine resolves
  the node's mesh options through the target's registered extractor
  (`register_volume_mesh_options(target, extractor)`, the sibling of
  `register_volume_mesh_boundary_demand`; `elastic_static` declares `mesh_size`, and `element_order`
  once #7075 lands); a target without one gets the default. A differing `options_hash` realizes that
  variant through the same function, caches it (C11), and hands the trampoline that variant's read
  handle. The realization node holds its variants keyed by `options_hash`; readers outside compute
  dispatch read the default variant.
- **The store.** A VolumeMesh is an engine-owned value, `Arc<StoredMesh { volume: VolumeMesh, remesh:
  Option<Arc<dyn RemeshFactory>> }>`, not a kernel handle: the realization cache holds it under the
  VolumeMesh repr and its `options_hash`, the read-handle projection reads it directly, and
  `resolve_realization_kernel` no longer serves VolumeMesh. Morphed and meshed seeds are stored alike.
- **Morph registration.** The CLI keeps `MorphRegistration::Enabled` (Q-a). The shipping GUI passes
  `MorphRegistration::Unavailable { reason }` naming #2953, which owns flipping it back once the GUI's
  settled-moment remesh and #2952's warm-start measurement exist (Q-e).
- **Mode selection lives here and only here.** Trampolines never see a mode (heuristic 14:
  `elastic_static.rs` gains no branch).
- **`RealizationReadHandle::remesh() -> Option<&Arc<dyn RemeshFactory>>`** (reify-compute-contract),
  projected from the same store entry as the content. `RemeshFactory: Send + Sync + Debug` exposes
  only `open_session(order) -> Result<Box<dyn MeshSession>, MeshError>`; the engine's implementation
  holds the `GeometrySource`, the `TopologyHandles` and the service, and converts identity to handles
  exactly as the realization function does. The contract crate gains a dependency on reify-meshing
  (OCCT-free, gmsh-free).
- **One `MeshError` → `DiagnosticCode` mapping** (C12), used by the realization function, the
  trampoline paths and the sweep.

### C8. Refinement through a session

- `RealizedAdaptiveProblem` holds a `Box<dyn MeshSession>` opened from the seed handle's `remesh()`,
  instead of the seed boundary `Mesh`. `refine_marked_elements(surface, …)` becomes
  `refine_marked_elements(session, volume_mesh, marked, current_sizes)`; `boundary_surface_mesh`
  leaves the refine path.
- Each refined `VolumeMesh` carries a `BoundaryAssociation` built as the seed's (C5–C6). A morphed
  seed refines the same way: the session remeshes the new body.
- A body handle whose `remesh()` is `None` (a service-less engine) refuses the
  adaptive lane with `MeshBackendUnavailable` naming the reason, under #8248's handling — never the
  uniform lane.
- A `MeshError` mid-loop is #8248's RefineError trigger: the last solved iterate, #8266's "stopped"
  status, a coded Warning naming the mesh error.
- In-process, a session re-runs `run_mesh` per remesh from its stored request. Worker-backed (κ), a
  session pins one worker that keeps the parametrised model and, per remesh, clears the mesh,
  installs the new field and regenerates.

### C9. The worker (`reify-mesh-worker`, a new binary crate)

- Links reify-kernel-gmsh and reify-meshing; **never** reify-kernel-occt. Its own `build.rs` emits
  `emit_tbb_pin_for_bins()` before `emit_rpath_for_bins(NativeDep::Gmsh)` (mechanism A″).
- **Protocol** (`reify_meshing::protocol`, one file, readable alone — heuristic 13): frames of a `u32`
  little-endian length plus a bincode 1.3 payload; `PROTOCOL_VERSION`; a `Hello` → `Ready {
  protocol_version, gmsh_version, occt_version }` handshake; requests `Mesh`, `Profile2d`,
  `OpenSession`, `Remesh`, `CloseSession`; replies `Report`, `Profile`, `Error(MeshErrorWire)`. Wire
  structs are the protocol's own flat arrays, converted at both ends; reify-ir types gain no serde
  dependency. A version mismatch is `BackendUnavailable` naming both versions.
- **Client** (`reify_meshing::WorkerMeshService`): spawns lazily; one request in flight per worker; a
  per-request deadline from its configuration (SIGKILL → `Timeout`); pipe EOF or a signal exit →
  `BackendCrashed`; the next request respawns. A session's worker dying fails that session's `remesh`.
- **Discovery:** `REIFY_MESH_WORKER` if set, else a sibling of `current_exe()`; otherwise
  `BackendUnavailable` listing the paths tried.
- **Building and shipping it.** The worker crate's own integration tests run protocol, crash and hang
  cases against `CARGO_BIN_EXE_reify-mesh-worker`. CLI-level tests find the worker as the sibling of
  the `reify` binary, so `scripts/verify.sh`'s prebuild builds `reify-mesh-worker` next to `reify`
  (a verify-pipeline edit: full gate). The GUI bundle step copies the worker into `bundle.externalBin`
  under its target-triple name, and `scripts/run-gui.sh` / `scripts/run-gui-dev.sh` build it before
  launching.
- **Fault injection** (`REIFY_MESH_WORKER_FAULT=segv|hang|exit`) is compiled into the worker only under
  its `fault-injection` cargo feature, which the gate's worker prebuild enables; release packaging
  builds without it, so a shipped worker ignores the variable.
- `GMSH_LOCK`, `GMSH_DEAD` and the finalize/initialize recovery live only in the worker. A worker whose
  gmsh is dead exits; the client respawns.

### C10. Sizing (D-D, Q-b, Q-c)

- **Who builds the request.** One function, `SizePolicy::resolve(options, body) -> Sizing`, in
  reify-meshing, called by the engine for every variant: default options for the build-pass variant,
  the consuming node's options for a dispatch-time variant (C7, Q-c).
- `mesh_size` given → `Uniform { h: mesh_size, floor: mesh_size, curvature_per_2pi: 0 }`: the user
  wins outright.
- Default → `h = max(floor, min(cap, t / 2))` with `curvature_per_2pi` applied by gmsh as a local size
  source, where `cap` is a fraction of the bounding-box diagonal, `t` is the body's thickness, and
  `floor` is an absolute floor (Q-b). `t` is a low percentile of inward ray-cast distances from the
  boundary triangles of the `TessellatedBody` (or the surface mesh, D-I) to the opposite boundary; the
  global `h` follows it, and local refinement is left to curvature and adaptivity.
- **Budget.** After `generate(2)` (C4 step 6) the tet count is estimated from the real surface mesh
  and the volume at `h`, curvature's effect included. If it exceeds the budget, `h` rises and the
  surface regenerates (at most twice); `MeshSizeBudgetBound` (Warning) names the estimate and the
  budget. A third overrun is `BudgetExceeded`.
- The `MeshModeUsed` Info names `h`, the term that set it (user, cap, thickness, floor, budget) and
  the minimum OCCT edge (reported, not used — Q-b).

### C11. Cache identity

- Every VolumeMesh variant is cached whenever it is produced (no longer only when a tolerance is
  demanded), under its tessellation tolerance and `options_hash` = hash(the request minus its
  geometry, `PROTOCOL_VERSION`, the gmsh version, the OCCT version of the tessellation). Threads are not
  in it (always 1).
- A morphed seed is path-dependent and stays in-memory only, never persisted (mesh-morphing's D6). The
  persistent FEA cache keys the solve (#7052, #8140).
- `reify_kernel_gmsh::volume_mesh_cache_key` and `VolumeMeshOptions::content_hash` are deleted.

### C12. Diagnostics (every Warning and Error carries a `DiagnosticCode`)

| Code | Severity | Emitted by | When |
|---|---|---|---|
| `MeshModeUsed` | Info | the realization function; a session | every mesh: mode, seed producer (mesh, or morph and its source's hash), node and tet counts, per-kind entity counts, `h`, its binding term, the min OCCT edge, a short content hash |
| `MeshBackendUnavailable` | Error | the edge, the trampoline paths, the sweep | no service, no worker binary, protocol mismatch, a body handle with no remesh capability |
| `MeshBackendCrashed` | Error | same | the worker died |
| `MeshTimeout` | Error | same | the deadline passed |
| `MeshGeometryFailed` | Error | same | gmsh refused the geometry; the stage and up to 40 captured log lines |
| `MeshTopologyInvalid` | Error | same | a C1/C5/C7 invariant failed; the invariant and the values |
| `MeshEmptyResult` | Error | same | zero tets or zero triangles |
| `MeshSizeBudgetBound` | Warning | the realization function; a session | the element budget raised `h` |
| `FeaSelectorResolved` | Info | the elastic trampoline, per solved mesh | a selector target's face, edge and vertex counts and its node count (λ, μ) |

λ deletes the selector refusal #6660 adds (Q-x); μ deletes #8248's selector trigger. A face selector on
a topology-free body (D-I) fails with whichever coded Error resolution raises first; λ probes which
and codes it if it is code-less.

### C13. Crate graph after the program

- `reify-meshing` (new): types, `MeshService`/`MeshSession`/`RemeshFactory`, `SizePolicy`, protocol,
  worker client.
- `reify-kernel-gmsh`: depends on reify-meshing; implements `run_mesh`, the 2D sequence and the
  in-process service; loses its `GeometryKernel` impl, its `inventory` registration and its
  `mesh-morph` feature.
- `reify-mesh-worker` (new binary): reify-meshing + reify-kernel-gmsh.
- `reify-solver-elastic`, `reify-eval`, `reify-compute-contract`: depend on reify-meshing, **not** on
  reify-kernel-gmsh (from β; reify-eval keeps it as a dev-dependency). Between β and ι the CLI and GUI
  construct the in-process service and so still link gmsh; ι replaces it with the worker client.
- `scripts/occt-touching-crates.txt` is unchanged (the worker is gmsh-only).

## 6. Boundary-test sketch (both sides of the seam)

CLI rows run `reify eval` on a fixture from a reify-cli integration test; the rest are Rust
integration tests in the crate named. No row pins a mesh hash as a committed constant: hashes are
compared within one test run.

| # | Scenario | Precondition | Postcondition | Leaf |
|---|---|---|---|---|
| BT1 | Side channel counts | All-edge filleted block; plate with a hole; cylinder (seam) — reify-kernel-occt | Face / edge / vertex / solid counts equal `extract_*`'s; every triangle has a face; each regular edge's path nodes are shared by exactly its faces; the seam is `Seam`; `validate` passes | α |
| BT2 | Invariant is named | A `TessellatedBody` with one triangle's face ordinal out of range | `TopologyInvariant` naming the invariant, the ordinal and the face count | α |
| BT3 | One sequence is deterministic (CLI) | `fea_body_cantilever.ri`, two `reify eval` runs | Identical printed displacement maxima (the plain path no longer drifts with thread count) | β |
| BT4 | Typed unavailability | A fake `MeshService` returning `BackendUnavailable` to the refine path and the 2D profile path | Both callers surface `MeshBackendUnavailable`; no `msg.contains` remains (grep) | β |
| BT5 | Zero tets is an error | A request whose volume is omitted (test hook in `run_mesh`) | `MeshEmptyResult` | β |
| BT6 | Engine-owned store (CLI) | The body fixture | `MeshModeUsed` printed (`Classified` until ε); the in-process service and the `reify eval` subprocess, run from one reify-cli test, print the same content hash | γ |
| BT7 | Tangent faces keep identity | All-edge filleted block through the service | Every face ordinal has ≥ 1 boundary triangle; a fillet face's `face_closure_nodes` excludes the adjacent planes' interior nodes | δ |
| BT8 | Identity per entity | Each body of BT1 through the service | No node has two attachments; every `Vertex` node sits on its topology vertex; every child surface maps to one face | δ |
| BT9 | Identity on every mesh (CLI) | Plate with a hole | `MeshModeUsed` says `EntityPerFace` with face/edge/vertex counts equal to the body's | ε |
| BT10 | Face closure on a realized body | Plate with a hole, a handle-valued `FixedSupport` target on the hole face (the #4092 path) | The resolved node set equals `face_closure_nodes(hole)` and contains every node of both hole rims | ε |
| BT11 | A morphed seed keeps identity | One engine with the morph producer registered: build the plate with a hole, edit the hole radius, build again | The second seed is a morph (`MeshModeUsed` names the source hash); its association passes C5's invariants against the new body; its handle's `remesh()` is present | ε |
| BT11b | GUI morph is declared dormant | The shipping GUI engine constructor | `MorphRegistration::Unavailable` with a reason naming #2953 (Q-e) | ε |
| BT12 | Identity survives refinement | BT10 with `adaptive: true` | Each refined mesh's `face_closure_nodes(hole)` contains both rims; `MeshModeUsed` per refine says `EntityPerFace` | ζ |
| BT13 | Remesh failure keeps the last iterate | BT12 with a forced `MeshError` on the second remesh | #8248's handling: last solved iterate, "stopped" status, coded Warning naming the mesh error | ζ |
| BT13b | Refining a morphed seed | BT11's morphed seed, `adaptive: true` | The session remeshes the new body; each refined mesh carries an association that passes C5's invariants | ζ |
| BT14 | `mesh_size` is honoured (CLI) | Plate at `mesh_size` 2 mm and 4 mm | `MeshModeUsed` tet counts differ; binding term `user` | η |
| BT15 | Variants and cache hits | One body consumed by two solves, `mesh_size` 2 mm and default; then the same engine re-evaluated | Two variants realized, each handed to its own solve; the second evaluation calls the service zero times (counting service) | η |
| BT15b | Morph sources are per options | BT11's edit with a 2 mm and a default consumer | Each variant morphs from the prior variant with its own `options_hash`, never across sizes | η |
| BT16 | Thin plate default (CLI) | 200 × 50 × 2 mm plate, no `mesh_size` | `MeshModeUsed` reports `h ≤ 1 mm`, binding term `thickness` (per Q-b) | θ |
| BT17 | Budget binds (CLI) | A 1 m block with a 0.2 mm fillet, low budget | `MeshSizeBudgetBound` names both counts; the solve proceeds | θ |
| BT18 | Worker crash contained (CLI) | `REIFY_MESH_WORKER_FAULT=segv` | `MeshBackendCrashed`, exit non-zero, printed by the surviving `reify` process | ι |
| BT19 | Worker hang contained (CLI) | `hang`, short deadline | `MeshTimeout`; the next request in the same engine succeeds on a fresh worker | ι |
| BT20 | No gmsh in the reify process | Prebuilt `reify` and `reify-mesh-worker` | `reify`'s NEEDED closure has no `libgmsh` and no `libTKernel.so.7.9`; the worker's has both and the tbb pin first in RUNPATH; `cargo tree -p reify-gui --features gui -e normal` has no reify-kernel-gmsh (Python infra test + `.sh` wrapper) | ι |
| BT21 | HXT spin is survivable | #7584's spinning fixture through the worker | `MeshTimeout` within the deadline | ι |
| BT22 | One worker per loop | BT12 through the worker | One worker spawn for the loop; refined meshes identical to a replay in a fresh session within the same test | κ |
| BT23 | Face clamp from `.ri` (CLI) | Plate with a hole, `FixedSupport(target: <hole-face selector>)`, single-shot | Exit 0; `FeaSelectorResolved` names one face and its node count equals `face_closure_nodes(hole)` (Rust twin); no Q-x refusal | λ |
| BT24 | No topology, coded refusal (CLI) | An SDF body with a face-selector support | A coded Error, exit non-zero; `MeshModeUsed` says `Classified` | λ |
| BT25 | Face clamp survives refinement (CLI) | BT23 with `adaptive: true` | ≥ 1 refine; each iteration's `FeaSelectorResolved` matches that mesh's face closure; no selector Error | μ |

## 7. Decomposition plan

Prerequisites outside this batch: **#6660** (β; it brings #7052), **#7052** (η), **#8248** (ζ),
**#8254** (θ), **#5313** (λ), **#8257** (μ). `elastic_static.rs` is in the declared files of #8248,
#8266, #8246, #8254, #7781, #8257, #5313, #8078 and the DWR leaves; ε, ζ, θ, λ and μ serialise on it
with them.

| Leaf | Title | Depends on | Observable signal |
|---|---|---|---|
| **α** | OCCT topology side channel: `BoundaryTopology`, `TessellatedBody`, `tessellate_with_topology` | — | Intermediate — unlocks δ, ε. BT1, BT2 against real OCCT shapes. |
| **β** | `reify-meshing` facade; `run_mesh` (`Surface` arm); typed `MeshError`; in-process service; refine and the 2D profile mesher through the service; reify-solver-elastic drops gmsh | #6660 | BT3 (CLI), BT4, BT5. `STUB_UNAVAILABLE_MARKER` and `GMSH_AVAILABLE` gone; the plain path runs one thread. Absorbs #7970 (the refine `MeshSizeMax` write; its figure list re-baselines here), #7481 (one zero-result guard) and #7409 part D (determinism, no seed). |
| **γ** | Engine-owned VolumeMesh store; `with_mesh_service`; gmsh leaves `GeometryKernel` | β | BT6 (CLI). `KernelId::Gmsh`, gmsh's `inventory` registration and `ensure_gmsh_kernel` are gone; the morph arm stores into the engine store. |
| **δ** | Identity through gmsh: the `Topological` arm, `MeshIdentity`, extended `BoundaryAssociation`, `face_closure_nodes`; morph rekey and `CarriedTopology` round trip | α, β | Intermediate — unlocks ε. BT7, BT8. |
| **ε** | Identity on every B-rep mesh: the meshable body and the one realization function (mesh or morph seed, the body's `RemeshFactory`); delete the attributed producer, the `mesh-morph` gate, `build_face_anchors` and `demanded_boundary`; readers use `face_closure_nodes`; GUI morph declared dormant | γ, δ | BT9 (CLI), BT10, BT11, BT11b. Supersedes #7277, #8113. #4092's test retargets to BT10. |
| **ζ** | Refinement through `MeshSession`; `RemeshFactory` on the read handle | ε, #8248 | BT12, BT13, BT13b; `MeshModeUsed` per refine from the CLI. |
| **η** | Per-options VolumeMesh variants realized at dispatch; the options extractor; meshing options in the cache key; morph sources per options; `mesh_size` honoured | ε, #7052 | BT14 (CLI), BT15, BT15b. Absorbs #7409 parts A–C. |
| **θ** | Default sizing policy (D-D per Q-b) and the post-surface element budget | η, #8254 | BT16, BT17 (CLI). |
| **ι** | The mesh worker: binary, protocol, client, discovery, deadline, crash boundary, fault injection, gate prebuild, GUI bundling; reify and reify-gui stop linking gmsh | γ | BT18, BT19 (CLI), BT20, BT21. Updates #7440's closure guard and #4289's check-manifold-deps arm to the one-OCCT split. Absorbs #7584 (its spinning fixture becomes BT21). |
| **κ** | Worker-backed sessions kept alive across refines | ζ, ι | BT22. |
| **λ** | Integration gate: a face clamp on a realized body from `.ri`; Q-x's refusal removed; `FeaSelectorResolved`; the body-face-supports exemplar | ε, #5313 | BT23, BT24 (CLI). `examples/best_practices/fea_body_face_supports.ri` and its `INDEX.md` row; one line in `.claude/skills/reify-design/SKILL.md`; the FEA chunk's support section says face selectors work on bodies. |
| **μ** | Integration gate: the face clamp across adaptive refinement; #8248's selector trigger removed | λ, ζ, #8257 | BT25 (CLI). |
| **ν** | Docs for the meshing surface: the FEA chunk's mesh diagnostics, `mesh_size` and worker sections; a meshing note; engine-integration-norm §3.1/§3.2 | ε, ζ, η, θ, ι | The chunk's fenced signatures pass the chunk gate; an author searching "why did my mesh fail" or "how fine is my mesh" finds `MeshModeUsed` and the error codes. `chunks/fea.md` is extended if #8259 or #7088 created it, else created (their rule). |
| **ω** | PRD close | α–ν | The terminal `Status` header, per the overlay's freeze shape. |

**External edges at decompose:** #5313 → ε (it reads vertex and edge attachments); #2953 → η
(re-pointed from #7409). **New task (Q-e), filed in this batch without a leaf label:** the GUI calls
`Engine::on_refine_trigger` at settled moments (auto-resolve accept, refine-now, user pause), so a GUI
session re-establishes a fresh mesh where #3000 says it should; #2953 depends on it and on #2952.
**Amendments:** #2953 (owns flipping the GUI back to `MorphRegistration::Enabled` once that task and
#2952 land; #7836 decides morph performance on #2952's numbers), #6660 (β replaces its registration,
λ removes its refusal), #8248 (μ removes its selector trigger), #8257 (its BT9 harness moves to
`with_mesh_service` after γ), #7440 and #4289 (ι updates their guards). **Superseded** (content ported first): #7409 → η and β;
#7277, #8113 → ε; #7481, #7970 → β; #7584 → ι.

**G6 notes.** BT3: one host, two processes, `NumThreads=1` — measured bit-identical (m5; the 12-run
measurement in `docs/notes/adaptive-e2e-seed-mesh-drift-measurement.md`). BT6, BT22: compared within
one test run, never against a committed constant (cross-host identity is not measured). BT16: with
Q-b's floor, `h ≤ t/2` holds by construction when the thickness term binds; `cap` on a 206 mm diagonal
is far larger. BT20: follows from C13's crate graph; `ldd` and `cargo tree` measure it. BT10, BT12,
BT23: exact by construction (C5) and measured in m5 (rim nodes on their edges at distance 0). No
signal asserts an accuracy number.

**G7 (reify invariants).** `diagnostics-carry-codes`: C12, and substring routing is deleted.
`error-severity-exits-nonzero`: every mesh Error fails the solve. `declared-intent-consumed-or-
diagnosed`: a face selector on a topology-free body is a coded Error (λ); `mesh_size` is honoured (η).
`declared-param-reaches-kernel`: `mesh_size` moves to honoured; `element_order` stays #7075's.
`placeholders-owned-and-loud`: Q-x's refusal and #8248's selector trigger are deleted by the gates that
make them unnecessary. Umbrella: no `MeshMode`, `Cad`, determinism enum or in-process deadline is
declared before it does anything.

**Gate-test registration.** Each new integration-test binary and BT20's infra test carry their
drift-guard registrations (nextest partition, `run-all-classification.manifest` row) in the same diff.

## 8. Out of scope

- **CAD mode** (gmsh's OCC kernel in the worker, D-C's predicate and the `MeshMode` override), **P2
  with geometry-placed midside nodes**, and a **finer frozen surface**: #8270 decides whether to pursue
  them.
- **Solver performance**: #8271. **Parallel meshing**: dropped (D-J).
- **Conformal multi-body meshing**: v1 is one session per body, non-conformal (D-H).
- **Identity for hex/wedge swept meshes** (`sweep.rs` emits `boundary: None`); `elastic_static` meshes
  tets only.
- **Topology for manifold-kernel meshes** (`faceID` runs): classify fallback (D-I).
- **Morph in the GUI**: dormant by declaration until #2953 re-enables it (Q-e). **Morph performance**:
  #7836.
- **The persistent FEA cache key**: #7052, #8140. **The OCCT version itself**: #7440 → #8269 → #8272.

## 9. Cross-PRD relationship (G4)

| Other work | Direction | Seam mechanism | Owner |
|---|---|---|---|
| #6660 gmsh registration (plain, Q-x) | consumes | engine construction; the VolumeMesh edge | #6660 lands first; β/γ replace its kernel registration with `with_mesh_service`; λ removes its refusal |
| #7052 options and persistent key | prerequisite | the options value present at dispatch | #7052 (before η) |
| #8140 recipe-keyed persistent inputs | neighbour | gmsh/OCCT versions in the persistent key | #8140 |
| #7409 mesh options and determinism | absorbed | C10, C11; D-J | η (A–C), β (D) |
| #5312 selector-typed targets | prerequisite of #5313 | `FixedSupport.target : Selector` | #5312 |
| #5313 selector consumption | produces for / gates | `face_closure_nodes`, adjacency, vertex and edge nodes | this PRD the data (δ, ε); #5313 the solver side; λ, μ observe the join |
| #8078 face triangles for selector loads | produces for | `BoundaryAssociation::face_triangles` | this PRD the data (δ); #8078 the consumption |
| #8248 body-overload fallback | modifies | its selector trigger; `MeshError` as its RefineError | #8248 first; μ deletes the selector trigger |
| #8266 status variants | consumes | "stopped" on a remesh failure | #8266 (via #8248) |
| #8246 refined results | neighbour | `RealizedAdaptiveProblem` | #8246; ζ swaps the frozen surface for a session |
| coordinate-target-fea θ #8257, ι #8258 | neighbour / prerequisite of μ | the same struct; the CLI body path; θ's BT9 calls `ensure_gmsh_kernel`, which γ deletes | that PRD; whichever lands second moves BT9 to `with_mesh_service` |
| `goal-oriented-error-estimation.md` (#7453–#7458) | neighbour | `RealizedAdaptiveProblem::refine` | DWR; it sees a session instead of a surface |
| #7781 redispatch swallow | consumes | a `MeshError` on a redispatch path is swallowed until it lands | #7781 |
| #8254 degenerate threshold | prerequisite | a finer default trips the absolute threshold | #8254 (before θ) |
| #7440 → #8269 → #8272 OCCT | neighbour | after ι the reify process loads one OCCT; the worker's OCCT can move independently | those tasks; ι updates #7440's closure guard |
| #4289 STEP assembly import | neighbour | its in-process TKXCAF 7.9 hazard and check-manifold-deps arm | #4289; ι updates the arm |
| `docs/prds/v0_3/mesh-morphing.md` / #2953 / #2952 / #7836 | modifies | morph becomes a second seed producer once meshes carry identity; the GUI registration | this PRD the seed producer and the GUI's declared dormancy (ε); #2953 the GUI re-enable, after the settled-moment task and #2952; #7836 morph performance |
| `docs/prds/v0_6/volume-mesh-realization-and-morph-wiring.md` | modifies | the §3.2 call edge it owns | this PRD rewires the edge's producer and store; the edge stays |
| `docs/prds/v0_3/engine-integration-norm.md` §3.1/§3.2 | modifies | gmsh leaves §3.1; §3.2's plug-in is the service | ν |
| #8259 / #7088 FEA chunk | neighbour | `chunks/fea.md` | extend-or-create, whichever lands first |
| #8270 fidelity milestone | gates | CAD mode, P2 midside, finer surface | #8270 |
| #8271 solver performance | neighbour | the wall-clock lever | #8271 |
| `docs/prds/v0_6/coordinate-target-fea.md` | neighbour | amended in the same landing (§9 rows, #8270) | that PRD |

## 10. Open questions (tactical)

- **Default constants** (C10): the `cap` fraction, the absolute floor, `curvature_per_2pi`, the element
  budget, the thickness percentile. Decide in θ from the fixtures; `MeshModeUsed` records them.
- **The worker deadline**: default value and whether it scales with the element estimate. Decide in ι.
- **Crash-detection latency** (m4: 1.2 s behind apport): whether the worker sets
  `prctl(PR_SET_DUMPABLE, 0)`. Decide in ι.
- **Idle worker count** in the client (default 1). Decide in ι.
- **`FeaSelectorResolved`'s payload spelling**. Decide in λ, matching coordinate-target-fea's per-patch
  line.
- **Whether a model-reusing session is bit-identical to a fresh one** (BT22). If not, κ re-runs the
  whole sequence per remesh in the pinned worker and records why.
- **Whether β keeps `MeshingOptions` as a deprecated alias for one leaf** while reify-eval's tests move
  to `MeshRequest`. Decide in β.
- **Skipping the build-pass default variant** when every consumer of a body declares non-default mesh
  options (it is meshed and never read; seconds per body). Decide in η.
- **Where in the dispatch path the variant is realized** (before the trampoline's read-handle
  projection, or inside it). Decide in η against #7052's landed shape.
