# PRD — Coordinate-target FEA (the P4 D2 follow-up)

> **Status:** active — authored 2026-10-05 via `/prd` (Leo + Claude), at Leo's ruling on esc-7189-9
> ("A and /prd coordinate-target FEA"). Decomposed 2026-10-06: leaf task ids are in §7 and the
> decompose-time corrections are in §11; the capability manifest and its stamped sidecar sit beside this
> file.
>
> **Milestone:** v0.6. **Approach:** B + H (FEA is a load-bearing seam; §5 contract, §6 boundary tests).
>
> **Code anchors** are cited by symbol. Substrate was read on main `f704d75ccc` and on the unmerged
> branches `task/7189` (`676fde4c10`) and `task/7448`, all 2026-10-05. CLI probes ran on
> `target/debug/reify` built 2026-09-30, five days behind main; decompose re-runs them (§3).
>
> **Parent decision:** `docs/prds/naming-convergence/P4-region-ref-fea-selector-unification.md` §4.
> D1 (region targets are set-only) and D3 (a pose at a region target is rejected) are **unchanged** by
> this PRD. This PRD is the "separate PRD" that D2 names.

---

## 1. Goal and consumers (G1)

An author can locate an FEA support, force or mass **by coordinate**, on any mesh a solve runs on, and
get the same physical model whether the mesh is the synthetic box, a realized body, or a refined mesh.

Three kinds, one pattern: a distinct structure with a typed `at` field, never a `target`.

```reify
let head = PointMass(at: point3(head_x, 20mm, 20mm), mass: 7.5kg, radius: 15mm)

let supports = [
    PointSupport(at: p1, radius: 4mm),                                   // ball in cone: 3 directions
    PointSupport(at: p2, radius: 4mm, restrain: [vec3(0.0, 0.0, 1.0), vec3(0.6, 0.8, 0.0)]),  // ball in vee
    PointSupport(at: p3, radius: 4mm, restrain: [vec3(0.0, 0.0, 1.0)]),  // ball on flat
]
let loads = [ Gravity(), head, PointForce(at: p4, force: vec3(0N, 0N, -75N), radius: 10mm) ]

let static = solve_elastic_static(material, length, width, height, loads, supports, options)
let modes  = modal_analysis(material, length, width, height,
                            ModalOptions(boundary_conditions: supports, point_masses: [head], ...))
```

| Mechanism | Consumer | Where it is observed |
|---|---|---|
| `PointSupport` (on `task/7189`) plus `restrain` | The EZBed plate on three interior supports in printer_v01 (`prj/printer_v01/printer.ri`, `EZBed`). Today it is 1-D strip algebra with no FEA solve. | Leaves ε, ζ (modal plate, kinematic mount); η (static sag, staged); λ |
| `PointForce` | The gantry tube under the toolhead at a variable X (`GantryFea`). Today the head is assumed mid-span through a half-span cantilever equivalence. Named by Leo, 2026-10-05. | Leaf β; λ |
| `PointMass` | The same toolhead (`m_head_m`, 7.49 kg): weight in the static solve, inertia in the modal solve. Today it is kept out of FEA and handled by 2-DOF cell algebra. Named by Leo, 2026-10-05. | Leaf γ; λ |
| Per-mesh re-resolution | Adaptive refinement, and the realized-body overload | Leaves θ, ι |
| Shared node-patch resolver and volume weights | All three kinds, static and modal | Leaves β–ι |

Engine seam (overlay G1 sub-check): everything plugs into the existing ComputeNode dispatch
(engine-integration-norm §3.4) through the registered trampolines `solver::elastic_static`,
`solver::multi_case`, `solver::buckling` and `modal::free_vibration`. No new in-engine seam.

**The consumer evidence is evaluated fixtures, not `printer.ri`.** No test evaluates `printer.ri` (it
is compile-only in `pin_cell_id_namespace_tests.rs`). Adoption there is leaf λ.

## 2. Background: what exists

**On `task/7189` (accepted, unmerged).** `PointSupport(at, radius)` and its resolver
`compute_targets/point_support.rs`: every node within `radius` of `at`, else the single nearest node;
off-body (nearest node farther than the longest tet edge) is an Error. It serves the static and modal
solves and declines adaptive refinement at any radius. Its diagnostics carry no `DiagnosticCode`.

**On main, the load side is aggregate, not located.**

- `elastic_static.rs::extract_loads` reads only `force` and `direction` from a `PointLoad` and sums
  every one into a single tip-face vector. `PointLoad.point` is never read.
- `TractionLoad`, `BodyForce` and unknown kinds are skipped with no diagnostic.
- `buckling.rs::extract_total_load` keys on the field name `force` and sums it when it is a `Real` or
  a `Scalar`. A list with no such field falls back to a 1.0 N load with no diagnostic.
- `DiagnosticCode::FeaLoadKindUnsupported` is defined but has **no production emitter** (its only
  attachment is a test stub in `compute_persist.rs`).
- Modal has no loads input and no nodal-mass hook. `ModalOptions` is shared by `modal_analysis` and
  `mechanism_modal_analysis`.
- `ElasticResult` has no reaction field.

**The static solve is linear tets only.** `ElasticOptions.element_order` is never read by
`elastic_static.rs` (pending #7075). Modal honours P2. The linear solver is Jacobi-preconditioned CG.

**The synthetic box is one element wide.** `synthetic_grid_counts` and `modal_ops::build_beam_mesh`
both fix `ny = 1`, so a `y` coordinate snaps to an edge. Nothing structural assumes `ny = 1`; the
adaptive uniform lane already runs at `ny = 2` and `4`. The static rule keeps elements near-cubic in
the bending plane because linear tets shear-lock in proportion to (δx/δz)².

**The modal `(K, M)` assembly is cached** under `ModalCacheKey` (dimensions, material, element order),
deliberately independent of boundary conditions.

**The realized-body overload does not run from the CLI.** No gmsh kernel is registered per engine and
the solve returns a hollow result (#6660; #7417 was folded into #6660).

## 3. Substrate verification (G3)

| Assumed capability | Verdict | Evidence |
|---|---|---|
| `PointSupport` kind, resolver, static and modal wiring | **Prerequisite** | `task/7189`, pending. Every leaf depends on it. |
| The field shapes in §5 C1, including the list-of-`vec3` default and `point_masses = []` | **Exists** (no novel syntax) | Mirror structures in `/tmp/prd-gate-fixtures/coordinate-target-fea/kinds.ri`: `reify check` exit 0; `reify eval` prints every field populated, both defaults included. Tree-sitter: 0 ERROR nodes. |
| A pose at `at` is rejected | **Exists** | `neg.ri`: `error: argument 'at' has type 'Frame3' but param 'at' requires type 'Point3<Scalar[m]>'`, exit 1. |
| An unresolved kind name is an Error | **ABSENT** | `PointMass(...)` on main: `warning: unresolved function`, exit 0, value `undef`. No leaf signal asserts that an unknown kind is rejected. |
| `K` and `M` keep full 3×3 nodal blocks, so a per-node basis change preserves the sparsity pattern | **Exists** | `assembly/global.rs::emit_element_triplets` emits every entry; `modal_ops::assemble_global_matrix` uses the same path. |
| A directional (n·u = 0) constraint | **ABSENT** — leaf ε builds it | `apply_dirichlet_row_elimination` takes global DOFs only. `mpc.rs::apply_mpc_row_elimination` has no production caller and leaves `K` unsymmetric for CG. The symmetric reduction in `buckling_kernel.rs` is private and forbids two constraints sharing a node. `RollerSupport` is a `Value::Map` builtin nothing reads. |
| A nodal mass in modal `M` | **ABSENT** — leaf γ builds it | Cache-safe site: `modal_ops::eigensolve_modal`, on a copy of `assembly.m_full`, before `project_free`. |
| A production emitter for `FeaLoadKindUnsupported` | **ABSENT** — leaf β builds the first | §2. |
| A grid discriminator in `ModalCacheKey` | **ABSENT** — leaf ζ adds it | §2. Without it a warm engine would reuse an assembly from a different grid. |
| Persistent FEA cache keys cover the new inputs | **Exists** | `engine_eval.rs::persistent_cache_key` hashes every evaluated argument value. Modal is not persisted. |
| `max(result.displacement)` reads a deflection | **Exists** | `GantryFea.defl_cant` in `printer.ri`. |
| Static P2, and Jacobi-CG converging on a thin plate | **Unverifiable today** — η is staged behind it | #7075 is pending; the 800×500×12 box on linear tets does not converge in 2000 iterations on main. (amended 2026-10-08 by docs/prds/v0_6/elastic-static-solver-performance.md) The Jacobi-CG non-convergence is resolved by the tiered solve of `docs/prds/v0_6/elastic-static-solver-performance.md` (its leaves β and ν); the converging linear solve it provides is what η's probe now runs against. |
| Body overload from the CLI | **Prerequisite for ι only** | #6660 (#7417 was folded into #6660). |
| A coordinate kind solving at sub-instance scope with a non-default argument | **To probe at decompose** (λ only) | `unfold/optimized_instance_reuse.rs` reuses the template's solved value only when inputs match. |

## 4. Resolved design decisions

| # | Decision | Source |
|---|---|---|
| **D1** | **Distinct kinds with a typed `at`**: `PointSupport`, `PointForce`, `PointMass`. Target fields stay set-only; P4 D1/D3 untouched. Rejected: a point at an existing `target`/`point` field (`radius` has no home, the field needs a selector-or-point union, and survival under remeshing would depend invisibly on the argument's type). Rejected: a proximity selector `near(body, p, r)` (selectors resolve against B-rep topology; the dims overload has none). | Leo, 2026-10-05 |
| **D2** | **Loads are built now**, for the gantry head at a variable X: a force and a mass. | Leo, 2026-10-05 |
| **D3** | **One `PointMass` declaration serves both solves.** It conforms to `Load`. In a static solve it contributes weight `m·g`, with `g` the summed acceleration of the `Gravity` loads in the same list. In a modal solve it enters through `ModalOptions.point_masses`. | this session |
| **D4** | **Restraint is a list of directions**: `restrain : List<Vector3<Dimensionless>>`, default the three global axes (7189's behaviour). One to three linearly independent directions; oblique directions in the first cut; static and modal. | Leo, 2026-10-05 |
| **D5** | **Directional restraint is a per-node basis change**, not an MPC. It keeps `K` and `M` symmetric and their sparsity pattern, so the existing CG and eigensolve paths are unchanged. | this session |
| **D6** | **Coordinates re-resolve on every mesh.** Nothing coordinate-addressed is held as a node index across a mesh change (§5 C4). | Leo, 2026-10-05 |
| **D7** | **`radius = 0` always resolves, and refuses energy-norm adaptivity** with a coded Warning. A point constraint or point force on a 3-D solid has no finite-energy solution, so the estimator would spend every mark at the point. `radius > 0` refines normally. If any coordinate kind in the solve has `radius = 0`, the whole solve refuses, and the result reports the explicit declined `ConvergenceStatus` variant (#8266), never the non-adaptive defaults, which report `Converged{0.0}` (C7). The refusal lifts for a far-field quantity of interest once the goal-oriented estimator lands. | Leo, 2026-10-05 |
| **D8** | **This PRD owns `SolveBoundary`**, the solver's located-load and located-support input (§5 C4). #5313 builds on it for selector-resolved loads and adds its own variant, shaped with #8078; this PRD defines only the coordinate variant. | Leo, 2026-10-05 |
| **D9** | **One weight rule for force, weight and inertia: nodal volume share over the patch** (§5 C3). It is continuous under refinement, needs no free surface, and works on tet10 meshes. A surface-traction rule was rejected: whether a ball patch contains a boundary face is mesh-dependent, so the load would jump between rules under refinement. | this session |
| **D10** | **The dims mesh gains `y` resolution only when a coordinate kind is present** (§5 C8). The lock-avoiding through-thickness rule is kept for linear tets. Every scene without a coordinate kind is byte-identical. | Leo, 2026-10-05 (scope); rule this session |
| **D11** | **The static plate is staged.** A credible plate sag needs quadratic tets (#7075) and a linear solve that converges on a thin plate, and neither can be probed today. Leaf η is filed as a dependency-gated milestone that probes first. The modal plate needs neither and ships in ζ. | this session — **Q-K** |
| **D12** | **The body overload from the CLI is not re-filed.** Leaf ι depends on #6660 (#7417 was folded into #6660). | this session — **Q-L** |
| **D13** | **7189 merges as accepted**; leaf α extracts the shared resolver and codes its diagnostics. | Leo, 2026-10-05 |

## 5. Contract (the H half)

### C1. Stdlib kinds (`fea_multi_case.ri`, `modal_analysis.ri`)

```reify
structure def PointSupport : Support {
    param at       : Point3<Length>
    param radius   : Length = 0mm
    param restrain : List<Vector3<Dimensionless>> = [vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0)]
    constraint radius >= 0mm
}
structure def PointForce : Load {
    param at     : Point3<Length>
    param force  : Vector3<Force>
    param radius : Length = 0mm
    constraint radius >= 0mm
}
structure def PointMass : Load {
    param at     : Point3<Length>
    param mass   : Mass
    param radius : Length = 0mm
    constraint radius >= 0mm
}
// ModalOptions gains:
    param point_masses : List<PointMass> = []
```

`mass >= 0` and the `restrain` rules are checked at the Rust boundary. A template constraint on a
required param reports INDETERMINATE on every `reify check` (probed).

`restrain` directions are `Dimensionless` because only their direction is consumed; a `Length` vector
would carry a magnitude the solver silently discards. This matches `PointLoad.direction`,
`Gravity.direction` and `ModalOptions.reference_direction`.

### C2. The node-patch resolver (`compute_targets/node_patch.rs`)

```rust
pub(crate) struct PointTarget { pub at: [f64; 3], pub radius: f64, pub span: Option<SourceSpan> }
pub(crate) struct OffBody { pub at: [f64; 3], pub nearest_distance: f64, pub h_max: f64 }

pub(crate) fn patch_nodes(target: &PointTarget, coords: &[[f64; 3]]) -> Vec<usize>;
pub(crate) fn check_on_body(target: &PointTarget, coords: &[[f64; 3]], h_max: f64) -> Result<(), OffBody>;
pub(crate) fn max_tet_edge_length(coords: &[[f64; 3]], tets: &[[usize; 4]]) -> f64;
```

- `patch_nodes`: every node within `radius` (boundary inclusive), else the single nearest node; ties to
  the lowest index; ascending; never empty. This is 7189's rule, unchanged. On a tet10 mesh it runs
  over corner and midside nodes alike.
- The resolver names no kind. The caller formats `OffBody` into a diagnostic naming its own kind.
- One `Point3<Length>` reader for all three kinds: `elastic_static.rs::extract_point3_si`.
  `point_support.rs::read_length_point3` is removed.

### C3. Patch weights: nodal volume share

`patch_weights(coords, elements, patch) -> Vec<(usize, f64)>`, weights summing to 1:

- a node's volume is the sum, over the elements containing it, of the element volume divided by the
  element's node count (4 or 10);
- a patch node's weight is its volume over the patch's total volume;
- a single-node patch has weight 1.

A `PointForce` applies `force · wᵢ`. A `PointMass` applies `m · g · wᵢ` in a static solve and adds
`m · wᵢ` to the three diagonal entries of each patch node in the modal mass matrix. The model is a
uniform body force (or added density) over the part of the ball inside the body. The resultant acts at
the weighted centroid of the patch, within `radius + h` of `at`.

### C4. `SolveBoundary`: the per-mesh-resolved description

```rust
pub(crate) struct SolveBoundary {
    pub node_override: BcNodeSetOverride,        // legacy selector node sets (index-based)
    pub apply_face_clamp: bool,
    pub point_supports: Vec<PointSupportSpec>,   // PointTarget + restrained directions
    pub point_forces: Vec<PointForceSpec>,       // PointTarget + force vector
    pub point_masses: Vec<PointMassSpec>,        // PointTarget + mass
    pub gravity: [f64; 3],                       // acceleration, summed over Gravity loads
}
```

It replaces 7189's `CantileverBcs`. Gravity is carried once as an acceleration; the body force is
derived from it and the density where it is assembled.

**Invariants.** Each names a place where today's code would do the wrong thing for a coordinate kind,
so that an implementer of a lane or overload keeps the behaviour without re-deriving it.

- **I1.** `solve_cantilever_fea` resolves every `PointTarget` against the coordinate array of the mesh
  it is about to solve, on every call. *Why:* a node index dies with its mesh; a coordinate does not.
  *Lifetime:* permanent.
- **I2.** Both adaptive problems (`CantileverAdaptiveProblem`, `RealizedAdaptiveProblem`) hold the
  `SolveBoundary` and pass it on every `solve_and_estimate`. Neither holds a resolved node index for a
  coordinate kind. *Why:* both lanes rebuild the mesh and today re-derive BCs by a hard-coded root-face
  rule, which is why 7189 had to decline adaptivity. *Lifetime:* permanent.
- **I3.** `node_override` stays index-based and keeps today's behaviour (its presence refuses the
  localized lane). *Why:* #4092's path is not reworked here. *Lifetime:* transitional, until #5313 and
  #8078 replace it. **On the body overload the uniform lane is never entered** (#8248):
  `node_override`'s presence leads to #8248's per-trigger handling, never to a synthetic-box solve.
- **I4.** The restraint-rank guard and the off-body check each run on every mesh that is solved. The
  off-body check measures against the **seed** mesh's `h_max` for the whole refinement run. *Why:*
  `h_max` shrinks under refinement, so a point that snapped on the seed must not become off-body three
  iterations in. *Lifetime:* permanent.
- **I5.** The legacy summed tip force stays for `PointLoad`. *Lifetime:* transitional, until #5313.
- **I6.** `FeaNoLoads` counts point forces, and point masses under gravity, as loads. *Why:* a
  `PointForce`-only scene would otherwise warn "no loads". *Lifetime:* permanent.

### C5. Directional restraint (`reify-solver-elastic/src/boundary/nodal_basis.rs`)

```rust
pub struct NodalBasis { pub node: usize, pub q: [[f64; 3]; 3] }   // orthonormal; restrained directions first
pub fn rotate_system(k: &mut SparseRowMat<usize, f64>, f: &mut [f64], bases: &[NodalBasis]);
pub fn rotate_matrix(m: &mut SparseRowMat<usize, f64>, bases: &[NodalBasis]);
pub fn rotate_vector(v: &mut [f64], bases: &[NodalBasis]);        // global -> local
pub fn back_rotate(u: &mut [f64], bases: &[NodalBasis]);          // local -> global
```

- **Direction sets.** A support's directions are orthonormalised in list order. A zero vector, a count
  outside 1–3, or a rank below the count is `FeaRestraintDirectionsInvalid`. When several supports'
  patches share a node, that node restrains the span of all their directions, built in list order.
- **Static.** Rotate `K` and `f` after assembly; fix the first *k* local DOFs of each patch node;
  forward-rotate any warm-start vector; solve; then `back_rotate` before stress recovery, nodal
  recovery, the returned displacement and the stored warm state. Every displacement that leaves the
  solve is in the global basis.
- **Modal.** `rotate_matrix` on `K` and on the augmented `M` before `project_free`; back-rotate each
  mode shape. The participation vector is the reference direction rotated into each node's local basis
  (`Qᵀd`), not `reference_direction[g % 3]`.
- A node whose restrained directions are all global axes may skip the rotation.
- **Static guard.** A point-only model must restrain all six rigid-body modes: 6 minus the rank of the
  constraint rows (one row per patch node per restrained direction) in rigid-mode space. The guard is
  skipped when a face clamp applies (7189's rule).
- **Modal.** No new guard. An under-restrained model keeps today's behaviour: `W_ModalRigidBodyMode`
  on small models, and `E_ModalNoModesComputed` when `K_free` is singular above the 1024-DOF dense
  ceiling. A plate resting on three `z`-only points is therefore an Error in modal until in-plane
  restraint is added; BT6 pins that.

### C6. Every consumer honours each kind or says why not

| Solve | `PointSupport` | `PointForce` | `PointMass` |
|---|---|---|---|
| `solve_elastic_static`, tet path, every overload (dims, heterogeneous, body) | honoured | honoured | weight under `Gravity`; with no `Gravity`, `FeaPointMassNoGravity` Warning. The weight needs no material density. |
| `solve_elastic_static`, shell-classified body | 7189's policy: `Auto` → tet with Warning, `On` → Error | same policy | same policy |
| `solve_load_cases` | passes through | passes through | passes through |
| `solve_buckling`, `solve_buckling_load_cases` | supports are already unused there | `FeaLoadKindUnsupported` Error | `FeaLoadKindUnsupported` Error |
| `modal_analysis` | `boundary_conditions`, with `restrain` | not accepted (no loads input) | `point_masses`; `mass_matrix_norm` is recomputed on the augmented `M` |
| `mechanism_modal_analysis` | n/a | n/a | non-empty `point_masses` → `FeaPointMassNotApplicable` Warning (the mechanism path has no mesh) |

Buckling errors only on the two new kinds. Its silent treatment of `Gravity`, `PressureLoad`,
`TractionLoad`, `BodyForce` and of an empty list (the 1.0 N sentinel) is owned elsewhere: pending
#7081 (buckling honours supports, `PointLoad.point`/`direction`, `PressureLoad`, `Gravity`) and
deferred #5797 (one shared load reader for both solvers, with buckling's type-name guard).

`modal_analysis` accepts no force because linear free vibration never sees one. A preload changes
modes through stress stiffening (`K + K_g(σ)`), which is a new solve mode for every load kind, not a
coordinate matter: milestone M1 in §7.

### C7. Adaptive refinement

- Any coordinate kind with `radius = 0` and `adaptive: true` → `FeaAdaptiveDeclinedPointTarget`
  Warning; the result is the single-shot solve (7189's shape) and reports the explicit declined
  `ConvergenceStatus` variant from #8266, never the non-adaptive defaults, which report `Converged{0.0}`.
- Otherwise both lanes run with the `SolveBoundary`. A coordinate-only boundary no longer forces the
  localized lane to fall back.
- Each iteration emits an Info line with each patch's node count on that mesh.

### C8. Synthetic grid with a coordinate kind present

One function, `synthetic_grid(dims, has_coordinate_kind, element_order) -> (nx, ny, nz)`, called by
`solve_cantilever_fea`, 7189's validation mesh, `CantileverAdaptiveProblem::new` and
`modal_ops::build_beam_mesh`.

- **No coordinate kind:** exactly today's counts for each caller.
- **Linear tets:** `nx` and `nz` as today (near-cubic in the bending plane); `ny = round(width / dx)`
  with `dx = length / nx`, at least 1.
- **Quadratic tets:** a body with `height / min(length, width)` below a fixed constant (0.2) uses two
  layers through the thickness and an in-plane size of at most five times the layer height, equal in
  `x` and `y`. A thicker body uses the linear-tet rule.
- **Ceiling:** a fixed DOF ceiling. When it binds, the in-plane size is coarsened equally in `x` and
  `y`, and `FeaSyntheticGridCapped` is emitted.
- The grid counts join `ModalCacheKey`.
- Every static and modal solve is single-threaded and bit-identical across `threads` values on one
  machine, so a scene with a coordinate kind needs no `deterministic: true` and `PARALLEL_DOF_THRESHOLD`
  is gone. (amended 2026-10-08 by docs/prds/v0_6/elastic-static-solver-performance.md) Originally: a scene with a coordinate kind can exceed `PARALLEL_DOF_THRESHOLD`, above which
  results are bit-stable only for a fixed thread count, so gated fixtures set `deterministic: true`.

Until #7075 lands the static solve is linear, so a thin plate with a coordinate kind gets a large,
ill-conditioned linear-tet mesh and the existing thin-body Warning. That case is η's.

### C9. Diagnostics (every Warning and Error carries a `DiagnosticCode`)

| Code | Severity | When | Minted by |
|---|---|---|---|
| `FeaPointOffBody` | Error | nearest node farther than `h_max`; names the kind, `at`, the distance, `h_max` | α |
| `FeaPointSupportsUnderRestrained` | Error | static point-only model with rigid-body modes free; names the count | α |
| `FeaPointTargetRequiresTet` | Error | coordinate kind with `ShellForce.On` | α |
| `FeaPointTargetShellFallback` | Warning | coordinate kind, shell-classified, `Auto` | α |
| `FeaAdaptiveDeclinedPointTarget` | Warning | α: 7189's any-radius decline. θ narrows it to C7. | α |
| `FeaSyntheticGridCapped` | Warning | C8 | ζ |
| `FeaLoadKindUnsupported` (existing variant) | Error | C6, buckling | β |
| `FeaPointMassNoGravity` | Warning | C6 | γ |
| `FeaPointMassNotApplicable` | Warning | C6, mechanism modal | γ |
| `FeaRestraintDirectionsInvalid` | Error | C5 | ε |

A single-shot solve with a `radius = 0` kind emits an Info line that `max_von_mises` includes a
mesh-dependent point singularity. `point_support.rs` and `node_patch.rs` reach zero code-less sites.
The as-printed gravity Warning's text is corrected in γ: a `PointMass` weight still applies when the
material has no single density.

## 6. Boundary-test sketch (both sides of the seam)

Every row runs in an evaluating integration test in `crates/reify-eval-fea-tests/tests/`, on a `.ri`
fixture through the production eval path. `examples/` is compile-gated only, so it holds the
user-facing positive designs and carries no assertion. Every static fixture asserts
`result.converged == true`, so that no signal passes on an unconverged iterate (β depends on #8244).

| # | Scenario | Precondition | Postcondition | Leaf |
|---|---|---|---|---|
| BT1 | Stdlib → solver value shape, per kind | A `.ri` constructing the kind with defaults and with every field set | The boundary parses it; a dimensionless `force`, a negative mass, a `Real` radius each fail the solve naming the list index and field | β, γ, ε |
| BT2 | Force and mass agree | Cantilever, `[Gravity(), PointMass(m)]` versus `[Gravity(), PointForce(m·g)]`, same `at` and radius | Displacement fields equal to the CG tolerance | γ |
| BT3 | Force position | Cantilever, `L/h = 20`, `PointForce` at `a₁ = L/2` and `a₂ = L` | `max(displacement)` ratio matches `a₁²(3L−a₁) / a₂²(3L−a₂)` within 10% | β |
| BT4 | Mass position, modal | Pinned–pinned beam, `PointMass` at `L/2`, at `L/4`, at a support, and none | `f₁(L/2) < f₁(L/4) < f₁(support) ≤ f₁(none)` | γ |
| BT5 | Oblique restraint, static and modal | Block on a cone / vee / flat mount with an oblique vee direction | Static: the vee patch's displacement along each restrained direction is zero to rounding, and non-zero along the free one. Modal: a finite first frequency with no rigid-body Warning | ε |
| BT6 | Under-restraint | Three points each restraining only `z` | Static: `FeaPointSupportsUnderRestrained` naming 3 free modes. Modal: the existing no-modes Error or rigid-body Warning | ε |
| BT7 | Remesh survival | `radius > 0` supports, `adaptive: true`, dims overload | The loop runs; no decline Warning; per-iteration patch node counts are non-decreasing | θ |
| BT8 | Radius-zero refusal | Same with one `radius = 0` kind | `FeaAdaptiveDeclinedPointTarget`; single-shot result | θ |
| BT9 | Realized mesh, Rust harness | Body overload, `ensure_gmsh_kernel`, coordinate supports and force | The solve completes on the realized node set; the localized lane does not fall back; an off-body point is `FeaPointOffBody`; weights sum to 1 on an unstructured mesh | θ |
| BT10 | Legacy unchanged | Every existing fixture without a coordinate kind | Byte-identical results and grid counts | ζ |
| BT11 | Unsupported consumer | `solve_buckling` with a `PointForce` | `FeaLoadKindUnsupported`, exit non-zero | β |
| BT12 | Warm modal cache | Same dims, a `PointSupport` toggled on a warm engine | A cache miss; frequencies equal the cold solve | ζ |
| BT13 | Weights converge | A `radius > 0` force on two successive refinements | Weighted centroid within `radius + h` of `at` on both; resultant exact on both | β |
| BT14 | `at` rejects a pose | `frame3(...)` at each kind's `at` | Compile-time Error | β, γ |
| BT15 | Body overload never boxes | Body overload, coordinate kinds with `radius > 0`, `adaptive: true`, a forced `RefineError` from the remesh | #8248's handling: the last good iterate with a "stopped" status, or a coded Error when no iterate solved; never a synthetic-box result | θ |

## 7. Decomposition plan

Prerequisites outside this batch: **#7189** (all leaves), **#8244** (β), **#7075** (η), **#6660** (ι;
#7417 was folded into #6660).
#7448 landed on 2026-10-05, so `apply_patch_resultant` and its load block are on main.

| Leaf | Title | Depends on | Observable signal |
|---|---|---|---|
| **α** #8251 | Extract the shared node-patch resolver; code 7189's diagnostics | #7189 | Intermediate — unlocks every other leaf. 7189's off-body and collinear fixtures still exit 1 through `reify eval`, now with the C9 codes asserted by identity in `point_support_e2e.rs`; `reify-audit --pattern PDIAG` finds no code-less site in the two modules. |
| **ζ** #8252 | Plate-capable synthetic grid; grid in `ModalCacheKey` | α | `plate_three_point_modes.ri` (P2 modal, three interior fully restrained `PointSupport`s on a 500×800×12 plate): the e2e reads a finite first frequency and an Info line showing each support resolved within one element of its requested `y`. BT10, BT12. 7189's fixtures are re-baselined here. |
| **β** #8253 | `SolveBoundary`, volume weights, `PointForce` (vertical slice) | ζ, #8244 | `gantry_head_force.ri`: a cantilever tube with a `PointForce` at two head positions; `reify eval` prints two deflections in the BT3 ratio. BT1, BT11, BT13, BT14. `examples/fea/gantry_head_force.ri` is the user-facing copy. |
| **γ** #8255 | `PointMass`: static weight and modal inertia | β | `gantry_head_mass.ri`: static deflection equals the equivalent `PointForce` (BT2); `first_frequency` with the head at `L/2`, `L/4` and a support is ordered as BT4. `mechanism_modal_analysis` with `point_masses` warns. |
| **ε** #8256 | Directional restraint, static and modal | β | `kinematic_mount.ri`: a block on a cone / vee / flat mount evaluates in both solves (BT5); the flat-only variant fails as BT6. |
| **θ** #8257 | Adaptive refinement with coordinate kinds | ε | CLI, dims overload: BT7 and BT8. Rust harness: BT9. |
| **ι** #8258 | Coordinate kinds on a realized body from the CLI | θ, #6660 | `bracket_point_targets.ri` (a body the box cannot express): `reify eval` prints a populated `ElasticResult`; an off-body point exits non-zero with `FeaPointOffBody`. |
| **κ** #8259 | Docs: FEA chunk, exemplar, index, reference | γ, ε, ζ, θ | A new `crates/reify-mcp/src/tools/chunks/fea.md` whose fenced signatures pass the chunk fence gate; `examples/best_practices/fea_point_targets.ri` and its `INDEX.md` row; one index line in `.claude/skills/reify-design/SKILL.md`; a `std.fea` subsection in `docs/reify-stdlib-reference.md`; `docs/notes/fea-point-supports.md` generalised to the three kinds; the P4 D2 pointer updated. An author searching "support a plate at three points" or "mass at a position on a beam" finds the kind from the chunk or the index. |
| **λ** #8260 | printer_v01 adopts the kinds | γ, ε | `GantryFea` gains a `head_x` param and a `PointMass`; `EZBed` gains a modal cell on its three supports. `reify check prj/printer_v01/printer.ri` is clean and a standalone `reify eval` of each structure prints the new cells. Its dependency on #8102 is decided by the §3 sub-instance probe. |
| **η** #8265 | *Milestone, dependency-gated:* static plate on three points | ζ, ε, γ, #7075 | First step: probe a P2 static solve of the 500×800×12 plate on minimal point restraint. If CG converges, add `plate_three_point_sag.ri` (reported sag; a narrow strip on two supports against the overhanging-beam closed form) and the EZBed sag cell. If it does not, η stops and returns the solver question to Leo. |
| **ω** #8264 | PRD close | α–λ | The terminal `Status` header, per the overlay's freeze shape. η and the milestones are named there as open or landed. |

**Milestones** (dependency-gated bookmark tasks filed in the same batch; each is a `/prd` session, not
an implementation leaf):

| Milestone | Gate | Trigger to design |
|---|---|---|
| **M1** #8261 prestressed modal analysis: `ModalOptions.preload : List<Load>`, a static solve, `K_g(σ)` from `geometric_stiffness`, eigen of `K + K_g` | β | a consumer with a belt, string, membrane or column whose modes shift under preload |
| **M2** #8262 coordinate kinds on the MITC3 shell path (general shell boundary conditions) | η | η's probe: quadratic tets fail to converge on the plate, or a consumer's thickness ratio makes tets impractical |
| **M3** #8263 lift D7's `radius = 0` refusal under a far-field quantity of interest | θ, DWR leaves #7453–#7458 | the goal-oriented estimator lands |

At decompose, #5313 gained a dependency on β #8253 and a note pointing at D8.

**G6 notes on the numeric signals.**

- **BT3, 10%.** The fixture is slender (`L/h = 20`) and both load points are at or beyond mid-span. The
  ratio's error terms: shear deformation, under 0.5% at this slenderness; the resultant's offset from
  `at`, at most `radius + h` ≈ 13 mm on `a = 500 mm`, which moves `a²(3L−a)` by under 5%; and the
  linear-tet stiffness error, which scales both deflections by nearly the same factor because the
  element shape is uniform along the tube. `max(displacement)` is the tip deflection for a cantilever
  loaded at `a ≤ L`. The budget has no CG-residual term, which is why the signal asserts
  `result.converged == true` and β depends on #8244.
- **BT4** is exact in direction: adding positive mass cannot raise any eigenvalue, and a mass nearer
  an antinode lowers the first mode more.
- **BT5** is exact by construction: the restrained local DOF is eliminated.
- **η** asserts nothing until its probe has run.
- Every signal's capabilities are delivered by its leaf or its prerequisites.

**G7 (reify invariants, advisory walk).**

- `diagnostics-carry-codes`: C9.
- `declared-intent-consumed-or-diagnosed`, `declared-param-reaches-kernel`: C6 covers every solve that
  accepts a `List<Load>`, a `List<Support>` or `ModalOptions`.
- `error-severity-exits-nonzero`: every C9 Error fails the solve.
- `result-fields-populated-or-owned`: no result field is added; `mass_matrix_norm` stays truthful.
- `placeholders-owned-and-loud`: no placeholder-typed field is added.

**Gate-test registration.** Each new integration test binary carries its drift-guard registration in
the same diff.

## 8. Out of scope

- **Coordinate kinds on the MITC3 shell path.** The shell solve takes a scalar tip force and an edge
  clamp. General shell boundary conditions are milestone M2, the physically natural home for a thin
  plate.
- **Preload in modal analysis.** Milestone M1.
- **Reaction forces at supports.** `ElasticResult` has no such field.
- **Moments, rotational inertia, remote (offset) loads.** Tets have no rotational DOFs.
- **A modal body overload.** Modal stays on the synthetic mesh.
- **Honouring `TractionLoad` and `BodyForce`**, retiring the legacy tip force, and selector-resolved
  loads: #5313, #8078.
- **Selector typing of the remaining target fields:** #5312. **The pose-vs-set verifier:** #4833.
- **Boundary fidelity of the remesh** (curved faces frozen at the seed facets): #8270, the fidelity
  milestone (§9).

## 9. Cross-PRD relationship (G4)

| Other work | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #7189 `PointSupport` | consumes | `point_support.rs`, `CantileverBcs` | 7189 until it merges; then this PRD (leaf α onward) | pending |
| #7448 patch resultant | neighbour | `solve_cantilever_fea`'s load block | 7448 (landed 2026-10-05). C3 does not call `apply_patch_resultant` | landed |
| #7081 / #5797 buckling load reader | neighbour | `buckling.rs::extract_total_load` | those tasks; β adds only the two new kinds' Error | pending / deferred |
| #5313 selector consumption | produces for | `SolveBoundary` | this PRD owns the struct (β); #5313 adds its selector variant | pending |
| #8078 face triangles for selector loads | neighbour | the selector variant's representation | 8078 with 5313 | pending |
| #7075 static P2 | consumes | `element_order` on `elastic_static`; selects C8's quadratic rule | 7075 | pending |
| #6660 gmsh in CLI and engine | consumes | per-engine kernel registration | #6660 (#7417 was folded into it) | pending |
| `a-posteriori-error-estimation.md` | modifies | the two `AdaptiveProblem` structs | this PRD (θ) for the boundary they carry; the lanes stay theirs | landed |
| `goal-oriented-error-estimation.md` (#7453–#7458) | neighbour | the same structs; coordinate-addressed QoI | DWR for the estimator; milestone M3 for lifting D7's refusal | pending |
| #7890 size proxy | neighbour | `RealizedAdaptiveProblem::current_sizes` | 7890 | pending |
| Adaptive boundary fidelity (frozen-facet remesh surface) | neighbour | the surface fed to the remesh | #8270, the fidelity milestone. `FeaPointOffBody`'s `h_max` bound holds either way | G6 not established; gated on #8270 |
| #8246 refined results | neighbour | the displacement, stress and `error_indicator` an adaptive solve reports; θ edits both `AdaptiveProblem` structs | #8246 for the result fields, θ for the boundary they carry; sequence the two | — |
| #8266 `convergence_status` default | consumes | the explicit declined `ConvergenceStatus` variant that C7's refusal reports | #8266 | — |
| #7781 redispatch swallow | neighbour | the engine's post-hydration redispatch of the body overload's compute node discards `DispatchError::Failed` | #7781 | — |
| #8254 degenerate-tet threshold | neighbour | the `FeaSingularStiffness` degenerate-tet check in the shared solve, which a fine realized mesh can trip | #8254 | — |
| #8244 CG cap and options | consumes | `ElasticOptions.max_iter` and `cg_tolerance` reaching the solve; an exhausted cap is not a valid result | #8244 (β prerequisite) | — |
| #8248 body-overload cube fallback | consumes | the uniform lane is never entered on the body overload (C4 I3, BT15) | #8248 for the per-trigger handling, θ for the `SolveBoundary` on both lanes | — |
| `docs/prds/v0_6/meshing-service.md` | neighbour | the realized mesh the body overload remeshes; face identity carried across a remesh | that PRD | — |
| #8270 fidelity milestone | neighbour | measurement gating the meshing service's fidelity increments | #8270 | — |
| P4 (naming convergence) | parent | D1 / D3 | P4; this PRD updates only its D2 pointer (κ) | active |
| `flexible-modal` A2 (#7143) | neighbour | selector supports on the modal path | 7143 | pending |

## 10. Open questions (tactical)

- **The DOF ceiling in C8.** Decide in ζ from a measured solve time and the CG iteration count at the
  candidate ceiling, both recorded.
- **Whether axis-aligned restraint skips the rotation** (C5 allows either). Decide in ε.
- **Spelling of the per-iteration patch Info line** (C7). Decide in θ.
- **`PointMass` versus the existing `point_mass(m)` dynamics builtin.** Different things with similar
  names; κ's chunk says so in one line. Rename only if dogfood shows confusion.
- **Support height on a plate.** 7189's note warns that pinning off the neutral plane stiffens
  bending; the plate fixtures place supports at mid-thickness.

## 11. Decompose amendments (2026-10-06)

Recorded at decompose (Leo's rulings Q-A–Q-F, the D3 run `wf_61a00e7b-afe`, and a critic pass); the
design decisions D1–D13 stand. Each item names the section it amends. The capability manifest beside
this file carries the evidence.

- **D13 (Q-A).** #7189 mints and applies its five C9 codes (`FeaPointOffBody`,
  `FeaPointSupportsUnderRestrained`, `FeaPointTargetRequiresTet`, `FeaPointTargetShellFallback`,
  `FeaAdaptiveDeclinedPointTarget`) during the rebase it needs — measured with `reify-audit --pattern
  PDIAG`, the branch as of `676fde4c10` adds five code-less sites and reds the ratchet. α #8251 extracts
  the resolver and the shared reader.
- **§1.** `printer.ri` is compile-gated by two tests and evaluated kernel-free by
  `idler_seat_e2e.rs::check_printer`; "no test evaluates printer.ri" was too strong.
- **§3, last row (λ).** Probed: λ does not depend on #8102 (a `point3` from an overridden param is
  already correct at instance scope; #8102 mints geometry/selector values). An `@optimized` FEA cell in a
  `sub` with a non-default argument is body-inlined to its sentinel (undef, exit 0, one Warning); the
  per-instance dispatch gap is filed as #8249 and λ keeps its inputs at template scope. Q-E: no edges on
  #7383 (the `printer.ri` SIGSEGV, unrelated) or #5312 (same-file ordering; the file lock serialises).
- **§5 C1 (Q-D).** `constraint radius >= 0mm` is dropped from all three kinds (and 7189's
  `PointSupport`): a stdlib template constraint is evaluated against the template default only, never an
  instance argument (probed), so the Rust boundary is the enforcement.
- **§5 C2.** `extract_point3_si` stays the strict triple parser (exact 3, finite); the kind / list-index
  / field labelling wrapper lives in `node_patch.rs` (the AABB callers have none to pass).
- **§5 C9.** Added: `FeaPointTargetFieldInvalid` (α; missing / non-finite / negative / wrong-arity
  field, naming kind, index and field — a dimension mismatch keeps `DimensionedArgRejected`);
  `ModalRigidBodyMode` and `ModalNoModesComputed` (ε codes the two pre-existing uncoded modal sites
  BT5/BT6 assert on — Q-F). Withdrawn: `FeaPointMassNotApplicable` — γ #8255 depends on #7079 and emits
  PDROP's `W_PARAM_NOT_APPLICABLE` instead (Q-C; #7084 amended to nine `ModalOptions` params). The CLI
  prints diagnostic message text, never a code name; signals assert the message, tests the code.
- **§5 C8 / ζ.** The per-support Info carries a structured payload: ζ mints
  `FeaDiagnosticDetail::PointTargetResolved` (the enum has three variants today) and the e2e reads it on
  the first (cold) eval. Every fixture with a coordinate kind sets `deterministic: true` (the `ny` rule
  takes a slender dims solve past `PARALLEL_DOF_THRESHOLD`).
- **§6 header.** Most rows run on a `.ri` fixture; BT9 and BT13 are Rust harness tests (`ElasticResult`
  exposes no centroid or resultant; gmsh is a dev-dep of reify-eval). BT1: a `Real` radius is a
  compile-time `ArgTypeMismatch`; the solve-boundary rows are a dimensionless force, a negative radius or
  mass, and an omitted field. BT3: `radius > 0`, on the solid dims box. BT4: two supports per end across
  the width (a single point per end leaves torsion free in modal). BT6: one pinned outcome per fixture.
- **§6 / ι.** ι's fixture and e2e are a reify-cli subprocess test under `crates/reify-cli/tests/`.
- **§8 / C6 owner cites.** The loud rejection of `TractionLoad`/`BodyForce` is #5802's, the wire-up
  #5800's; #5313/#8078 own the selector variant. β's buckling `FeaLoadKindUnsupported` arm is built in
  #5802's shape and cites #7081 as the honour owner (Q-C: #7081 amended to honour `PointForce`/`PointMass`
  and `PointSupport` with `restrain` on buckling; #7142 and #7166 amended to honour or declare
  `point_masses` and `restrain`; #7088 co-owns the FEA doc chunk with κ #8259).
- **§9.** Additional seams: #5802/#5800 (load-kind rejection), #7079/#7084/#7085 (PDROP), #7142/#7166
  (other `ModalOptions` consumers), #7088 (FEA chunk), #7383 (`printer.ri` crash), #5312 (same-file
  ordering), #8248 (adaptive `bc_override` defect, filed on Q-B).
- **Probes.** §3 was re-run on the 2026-10-05 binary (`e5f638d029`); every row confirmed.

### 2026-10-06 — adaptive-boundary-fidelity rulings (Q-m, Q-n, Q-o, Q-u, Q-z)

Recorded after the adaptive-boundary-fidelity investigation. Each item names the section it amends; the
normative text in §4–§10 is edited in place.

- **§5 C7 / §4 D7.** The `radius = 0` refusal reports the explicit declined `ConvergenceStatus` variant
  from #8266, not the non-adaptive defaults, which report `Converged{0.0}`.
- **§5 C4 I3.** On the body overload the uniform lane is never entered (#8248); `node_override`'s presence
  leads to #8248's per-trigger handling, never to a synthetic-box solve.
- **§5 C4 I4.** The off-body check runs on every mesh solved, against the seed mesh's `h_max`.
- **§6.** New BT15: body overload, coordinate kinds with `radius > 0`, `adaptive: true`, a forced
  `RefineError` gives #8248's handling (last good iterate with a "stopped" status, or a coded Error),
  never a box result. Leaf θ.
- **§6 / §7 / §10.** Every static fixture asserts `result.converged == true`. β #8253 depends on #8244
  (CG cap and options honesty); the BT3 budget note says why. ζ's DOF ceiling records the CG iteration
  count as well as the solve time.
- **§7 / §2 / §3 / §4 D12 / §9.** ι #8258 depends on #6660 only: #7417 was folded into #6660.
- **§8 / §9.** Neighbour rows for #8246 (refined results), #8266 (status default), #7781 (redispatch
  swallow), #8254 (degenerate threshold), #8244 (CG), #8248 (cube fallback), the meshing-service PRD
  (`docs/prds/v0_6/meshing-service.md`) and #8270 (fidelity milestone). The adaptive-boundary-fidelity
  row now records the outcome: G6 not established; gated on #8270. The §8 fidelity bullet cites #8270.
  The Status cells of the new rows are left as "—", because task status is not cited here.
