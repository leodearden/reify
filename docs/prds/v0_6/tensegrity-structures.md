# Tensegrity Structures (Force-Density + Pin-Jointed Bar)

Status: contract. Authored 2026-05-26 in interactive `/prd` session under G1–G5+META gates. Staged umbrella PRD spanning four capability layers as one vertical-slice DAG. Approved by Leo before queueing tasks.

Greenfield — no gap-register cluster covers tensegrity; this is feature-driven work, not an audit-cluster resolution. It slots onto the structural-analysis roadmap as the natural successor to v0.5 buckling (v0_2 multi-kernel → v0_3 elastostatics → v0_4 shells → v0_5 buckling → **v0_6 tensegrity / form-finding**).

## §0 — Purpose and scope

A **tensegrity** is a self-stressed network of isolated compression members (**struts**) suspended in a continuous network of tension members (**cables**); the struts touch nothing but cables, and the whole assembly is rigidized only by its internal prestress. Its defining design problem is **form-finding**: the equilibrium geometry *emerges* from the force balance under prestress — you do not type the node coordinates, you solve for them.

Reify today can model the *geometry* of a tensegrity (points + wires) but has none of the structural machinery: no typed strut/cable members, no form-finding solver, no self-stress/stability analysis, no pin-jointed axial element. This PRD adds all four as a staged roadmap.

**Naming.** The method is **Force-Density** (abbreviated **FD**). It is *not* abbreviated "FDM" — that collides with Fused Deposition Modelling. The stdlib entry point is `form_find(...)`; the ComputeNode dispatch target is `solver::form_find`.

**The four layers** (each a phase tier in the §9 DAG; later tiers may land as deferred bookmarks):

1. **Representation** — a typed-element structural network in the `.ri` DSL (nodes + struts + cables, extensible to surfaces). Prerequisite for everything below.
2. **Form-finding** — Force-Density solver: from topology + prestress, compute equilibrium node coordinates. The core.
3. **Self-stress & prestress-stability** — does the found form actually stand up? Shares the stress-matrix linear-algebra core with layer 2.
4. **Load analysis** — a linear pin-jointed bar/cable element added to `reify-solver-elastic`, reusing the v0.5 buckling `K_g` geometric-stiffness stack, with a tension-only active-set wrapper for slack cables.

Layers 1⊂2⊂3 nest (3 reads the matrices 2 builds); layer 4 is the analysis arm that reuses the existing FEA stack.

## §1 — Foundation summary (prerequisites)

This PRD builds on three landed/in-flight pieces and assumes they hold before the dependent slices:

- **ComputeNode contract** (`docs/prds/v0_3/compute-node-contract.md`, GR-002). Form-finding routes through this seam exactly like `solve_elastic_static`: `form_find` is a stdlib `fn` annotated `@optimized("solver::form_find")`, lowered to a ComputeNode, dispatched to a Rust trampoline in `reify-solver-elastic`, returning a graph-participant `Value` (solved coordinates + member forces). The hard "iterative solve flowing through a one-way eval pipeline" problem is **already solved** by this contract — this PRD owns only the `solver::form_find` target, not the dispatch mechanism. Cancellation, pending lifecycle, warm-state, and caching come for free.
- **Structure-instance runtime** (`docs/prds/v0_3/structure-instance-runtime.md`, GR-001). The `Strut` / `Cable` / network structures evaluate to `Value::StructureInstance`, cracked open by the trampoline — same path the FEA `ElasticMaterial` already uses. Gates the first solver slice.
- **Buckling `K_g`** (`docs/prds/v0_5/buckling-eigensolver.md`, GR-024). The geometric-stiffness assembly + eigensolver path (currently P1-tet) is the operator layer 4's prestressed bar element and layer 3's stability check reuse. This PRD adds the bar-element `K_g`; it does not re-architect the eigensolver.

## §2 — Consumer and user-observable surface (G1)

**Ultimate consumer: a user designing a tensegrity in a `.ri` file**, plus the GUI viewport that renders it. Concretely, the value chain at each layer terminates in something the user sees:

| Layer | Mechanism | Consumed by | What the user observes |
|---|---|---|---|
| 1 Representation | typed-element network | `form_find`, viewport, load solver | A `.ri` file declares struts + cables; viewport renders them visually distinct (struts heavy, cables thin) |
| 2 Form-finding | `solver::form_find` | the user's `.ri` (`let form = form_find(...)`); viewport shows solved shape | Typed topology + prestress → solved equilibrium geometry in the viewport; `result.nodes` / `result.member_forces` readable in CLI |
| 3 Stability | stress-matrix null-space + tangent-stiffness check | the user's `.ri` (`constraint form.stable`); design feedback | `result.self_stress_states`, `result.mechanisms`, `result.stable` — a floppy topology is flagged, a valid one passes |
| 4 Load analysis | pin-jointed bar/cable element | `solve_elastic_static` over a tensegrity | Deflections + member-force changes under load; a slackening cable drops to zero force |

No mechanism in this PRD is producer-only: every phase in §9 ends in a CLI-readable result field, a viewport signal, or an example `.ri` that exercises it (`feedback_task_chain_user_observable`).

## §3 — Representation contract (layer 1)

**Grammar: confirmed against existing grammar (G3).** Tree-sitter-parsed 2026-05-26 — nodes as a list of `point3(...)`, struts/cables as nested index-pair lists `[[0,4],[1,5],...]`, `form_find(...)` named-argument calls, `result.field` member access, and `Strut(...)`/`Cable(...)` structure constructors all parse with no grammar work. (Caveat for authors: `²` superscripts are comment-only in the codebase; area literals are products — `area: a * a`, not `mm²`.) **No grammar prerequisite task.**

**Shape (recommended; exact field names finalized in the layer-1 task):**

```
// A node is just a point; members reference nodes by index.
structure def Strut {            // compression member
    param section_area : Area
    param material : ElasticMaterial
}
structure def Cable {            // tension member
    param section_area : Area
    param material : ElasticMaterial
    param pretension : Force = 0N    // optional explicit prestress
}

structure def Tensegrity {
    param nodes  : List<Point3<Length>>   // initial guess (form-finding refines)
    param struts : List<List<Int>>        // index pairs into `nodes`
    param cables : List<List<Int>>        // index pairs into `nodes`
    // FUTURE (membrane extension, §10): surfaces : List<List<Int>>  -- index triples/quads
}
```

**Open typed-element network — the membrane hook.** The network is modelled as a collection of *typed element groups*, not a hardcoded `{struts, cables}` pair. Adding a `surfaces` group later (§10) is an additive field, not a schema break. The layer-1 task must structure the internal `Value` so a third element class slots in without touching layers 2–4's existing call sites.

**Geometry emission.** A `Tensegrity` realizes to a wireframe: each strut/cable becomes a `line_segment` wire between its two node positions, tagged with its member type so the viewport (layer-1b) and any export can style/colour them. Struts and cables are visually distinct in the viewport (per `feedback_gui_auto_axis_triad`: do not add an axis triad to the model — the viewport supplies one).

## §4 — Force-Density form-finding contract (layer 2)

**The reduction.** For a member *i* joining nodes *j*–*k* with axial force `N_i` and length `L_i`, define the **force density** `q_i = N_i / L_i`. The force member *i* exerts on node *j* is `q_i · (x_k − x_j)`, which is *linear in the coordinates once `q` is fixed*. With branch-node connectivity matrix `C` (m×N: `+1` at *j*, `−1` at *k*) and `Q = diag(q)`, nodal equilibrium is:

```
D x = P,    where  D = Cᵀ Q C   (the N×N stress / force-density matrix)
```

**Anchored case (layer-2a, the minimum vertical slice).** Partition nodes into free (`f`) and fixed/anchored (`n`). Per coordinate axis:

```
D_ff x_f = P_f − D_fn x_n
```

One linear solve (faer) per axis gives the equilibrium geometry. This is the simplest end-to-end path through ComputeNode and is the first real consumer slice.

**Free-standing case (layer-2b).** With no anchors, equilibrium is `D x = 0`, which needs `D` rank-deficient: for a non-degenerate `d`-dimensional form, `rank(D) = N − (d+1)`, i.e. nullity `d+1`, so `null(D)` spans the all-ones vector plus the `d` coordinate directions. Arbitrary `q` does **not** yield a valid 3-D form — you must *find* an admissible `q` (sign-constrained: cables `q > 0`, struts `q < 0`) that produces the required rank deficiency, then recover coordinates from `null(D)`. This is the eigenvalue / adaptive force-density extension (e.g. drive the `(d+1)`-th smallest eigenvalue of `D` to zero, or solve for `q` in the null space of the equilibrium matrix), all faer eigendecomposition.

**Sign convention (load-bearing, breadcrumb at impl site per `feedback_breadcrumb_design_alternatives_at_impl_site`).** Cables carry tension (`q > 0`), struts carry compression (`q < 0`). The solver enforces these as hard sign constraints on `q`; a topology that cannot satisfy them is a diagnostic, not a silent wrong answer.

**Inputs / outputs.** `form_find(nodes, struts, cables, force_densities | force_density_ratios, anchors?)` → a `FormFindResult { nodes: List<Point3>, member_forces: List<Force>, force_densities: List<...>, converged: Bool, diagnostics }`. The user supplies either explicit per-member force densities, or relative ratios per member group (cables vs struts) with the eigenvalue extension finding the admissible scaling.

## §5 — Self-stress & prestress-stability contract (layer 3)

**Shares layer 2's core** — this is the same matrices viewed for a different question, which is why FD won the method choice over Dynamic Relaxation.

**Self-stress & mechanisms.** Build the equilibrium matrix `A` (`d·N_free × m`) relating member forces `s` to nodal loads (`A s = f`):

- **Self-stress states** = `null(A)` → count `s = m − rank(A)`. A valid tensegrity needs `s ≥ 1` (at least one self-equilibrated prestress).
- **Infinitesimal mechanisms** = `null(Aᵀ)` minus rigid-body modes → count `m_count`.
- **Generalized Maxwell rule** (Calladine): `m − d·N_free = s − m_count`. Reported for sanity.

**Prestress stability.** The tangent stiffness is `K_T = K_E + K_G` — elastic (material) plus geometric (stress). A framework with first-order mechanisms is stabilized *only if the prestress stiffens them*: the structure is **prestress-stable** iff `K_G` (equivalently the stress matrix `D ⊗ I_d`) is positive definite on the mechanism subspace. The stronger **super-stability** test (Connelly): `D` positive semidefinite with rank exactly `N − d − 1` and member directions not lying on a conic at infinity → stable independent of material stiffness and prestress level. The stability task computes the mechanism-subspace eigenvalues of `K_G` (reusing the buckling eigensolver path) and reports a verdict.

**Output fields** (on `FormFindResult` or a sibling `StabilityResult`): `self_stress_states: Int`, `mechanisms: Int`, `maxwell: Int`, `stable: Bool`, `super_stable: Bool`. Consumable as `constraint form.stable` in `.ri`.

## §6 — Load-analysis contract (layer 4)

**Linear pin-jointed bar/cable element** added to `reify-solver-elastic` alongside the existing tet/hex/wedge/shell families:

- **Elastic stiffness** `K_e = (EA/L) · [[c cᵀ, −c cᵀ], [−c cᵀ, c cᵀ]]`, where `c` is the unit direction vector (direction cosines), 3×3 blocks per node pair.
- **Geometric (prestress) stiffness** `K_g` for an axially-loaded bar — the transverse stiffening from member force `N`. This is the *bar* analogue of the P1-tet `K_g` the v0.5 buckling solver already assembles; it reuses the same global-assembly and eigensolver machinery.
- **Tension-only cables under load** via an **active-set loop**: solve, drop any cable whose force went compressive (slack), re-solve, repeat to a fixed point. A modest nonlinear wrapper around the existing linear `solve_cg`, not a new solver paradigm.

**Composition.** Analysing a built tensegrity = take a form-found geometry + prestress, assemble bar/cable `K_e + K_g`, apply external loads, solve via the existing CG path, report nodal deflections and member-force deltas. Routes through ComputeNode like every other solver (`solver::elastic_static` extended, or a dedicated `solver::tensegrity_load` — decided in the layer-4 task).

## §7 — Cross-PRD seam ownership (G4)

| Seam | Owner | Relationship |
|---|---|---|
| `@optimized` dispatch / cache / cancellation / warm-state | **ComputeNode contract** (GR-002) | Owns the mechanism. This PRD owns only the `solver::form_find` (and `solver::tensegrity_load`) *targets* registered via `register_compute_fns`. No redefinition. |
| Structure-instance runtime (`Strut`/`Cable`/`Tensegrity` → `Value::StructureInstance`) | **structure-instance-runtime** (GR-001) | Prerequisite. Same path FEA materials use. Gates layer-2a. |
| Geometric-stiffness assembly + eigensolver (`K_g`) | **buckling-eigensolver** (GR-024) | Reuse. This PRD *adds* the bar-element `K_g` kernel and a mechanism-subspace eigenvalue use; does not re-architect the eigensolver. |
| GUI viewport member-type styling | **GUI** | This PRD specifies the member-type tag on emitted wires (§3) and the styling intent; the viewport owns the rendering channel. Layer-1b task touches `gui/`. |
| Shell / CST element family (future membrane load element) | **v0_4 shells** | Future (§10). A membrane is a bending-dropped shell; reuses that element family. No work now. |

No reciprocal "the other owns it" deadlocks: every seam has exactly one owner, and this PRD is a consumer of three existing contracts + an extender of one element library.

## §8 — Boundary-test sketch (cross-crate; facing both ways) (H)

The seam is between `reify-eval` (graph + ComputeNode dispatch) and `reify-solver-elastic` (the form-finding + load trampolines). Tests cross it from both sides.

### 8.1 Producer-side (`reify-eval` looking outward at the `solver::form_find` trampoline)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Round-trip form-find dispatch | `solver::form_find` registered; inputs Final | A ComputeNode with target `solver::form_find` is inserted (no inlining); `FormFindResult` populates output VCs; freshness Final |
| Sign-constraint violation diagnostic | topology where no admissible `q` exists | Clean `Diagnostic` (e.g. `E_FormFindInfeasible`), output VCs → `Failed`; no panic |
| Warm-state reuse | re-solve with same topology, perturbed initial guess | Prior `q`/factorization donated to cache reused as warm start; dispatch-count instrumentation confirms cache hit |
| Cancellation under design loop | `param … = auto` driving a prestress ratio; solve mid-iteration | Cancellation observed within 2× poll budget; prior cache intact (per ComputeNode contract §2) |

### 8.2 Consumer-side (`reify-solver-elastic` looking inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Anchored cable-net golden | fixed-corner net `.ri` | `result.nodes` matches the analytic shape within tol (the layer-2a signal) |
| Free-standing T-prism golden | 3-strut prism topology + relative `q` signs | Solved coords match published equilibrium within tol; struts in compression, cables in tension (sign check) — the layer-2b signal |
| Stability classification | T-prism vs a deliberately floppy topology | T-prism: `self_stress_states == 1`, `stable == true`; floppy topology: `stable == false` with a mechanism reported |
| Bar element `K_e`/`K_g` unit | single prestressed bar | Element matrices match hand-computed values; tip-loaded truss deflection matches analytic axial solution |
| Tension-only active set | load case that slackens one cable | That cable's force → 0 (dropped from active set); deflection matches a reference / DR cross-check |

## §9 — Vertical-slice DAG (B + H)

Decomposition style **B (vertical slice) + H (design-first / contracts / boundary tests)** per `preferences_implementation_chain_portfolio` — this is a high-stakes, multi-crate, architecturally-complex PRD, so G5 resolves to B+H rather than bare B. Each leaf names its **user-observable signal** (G2). All tasks filed `planning_mode=True`; all deps wired as real edges before the batch flips `deferred → pending`.

### Tier 0 — Representation

- **T0a — Typed-element network + geometry emission.**
  - *Signal:* `examples/tensegrity_t_prism.ri` declaring the 3-strut prism (nodes + struts + cables) evaluates; CLI dumps the wireframe (6 struts/cables as tagged wires); viewport renders the wireframe.
  - *Prereqs:* GR-001 (structure-instance runtime). *Crates:* reify-compiler/stdlib, reify-types, reify-eval.
  - *consumer_ref:* layers 2/4 + viewport. *grammar_confirmed:* true.

- **T0b — GUI member-type styling.**
  - *Signal:* viewport renders struts heavy/one colour, cables thin/another; screenshot diff confirms distinction.
  - *Prereqs:* T0a. *Crates:* gui/.

### Tier 1 — Force-Density form-finding

- **T1a — `solver::form_find`, anchored case (minimum vertical slice).**
  - *Signal:* an anchored cable-net `.ri` form-finds to its analytic shape; `result.nodes` within tol via CLI; a ComputeNode with target `solver::form_find` is in the graph; re-run hits cache.
  - *Prereqs:* T0a, ComputeNode contract landed, GR-001. *Crates:* reify-solver-elastic (FD linear solve), reify-compiler/stdlib (`form_find` `@optimized` decl), reify-eval.
  - *consumer_ref:* user `.ri` + stability/load layers. *grammar_confirmed:* true.

- **T1b — Free-standing eigenvalue extension.**
  - *Signal:* `tensegrity_t_prism.ri` form-finds from topology + relative `q` signs to the published prism equilibrium within tol; member forces show struts compressive, cables tensile.
  - *Prereqs:* T1a. *Crates:* reify-solver-elastic (eigenvalue/null-space `q` search via faer).

### Tier 2 — Self-stress & stability

- **T2 — Stress-matrix null-space + tangent-stiffness stability.**
  - *Signal:* T-prism reports `self_stress_states == 1`, correct mechanism count, `stable == true`; a floppy topology reports `stable == false`. Exercised via `constraint form.stable` in an example `.ri`.
  - *Prereqs:* T1b (shares the stress matrix). *Crates:* reify-solver-elastic (reuses buckling eigensolver path).

### Tier 3 — Load analysis

- **T3a — Pin-jointed bar/cable element + `K_g`.**
  - *Signal:* single prestressed bar / simple truss `.ri` under tip load gives deflection matching the analytic axial-stiffness solution; element `K_e`/`K_g` unit tests pin hand-computed values.
  - *Prereqs:* T0a, buckling `K_g` (GR-024). *Crates:* reify-solver-elastic. (Parallel with Tier 1–2.)

- **T3b — Tensegrity load analysis with tension-only active set.**
  - *Signal:* a form-found tensegrity under external load reports nodal deflections + member-force deltas; a load case that slackens a cable shows that cable dropping to zero force; deflection cross-checks a reference.
  - *Prereqs:* T3a, T1b. *Crates:* reify-solver-elastic, reify-eval (ComputeNode routing).

### Tier 4 — Membrane bookmark (deferred; §10)

- **T4 — Membrane / surface element class (DEFERRED BOOKMARK).**
  - Filed `planning_mode=True`, **excluded from the pending flip** — stays `deferred` until Leo triggers it (`preferences_bookmark_task_pattern`). Carries the §10 breadcrumb (Natural Force Density for form-finding; shell/CST reuse for load; FD-seed → DR/energy-min as the large-displacement fallback).
  - *Signal (when eventually done):* a tensegrity-pavilion `.ri` with a prestressed membrane patch form-finds and carries load as both structure and load source.

### Dependency view

```
GR-001 ─→ T0a ─┬─→ T0b
               ├─→ T1a ─→ T1b ─→ T2
               │                 │
               └─→ T3a ──────────┴─→ T3b
buckling K_g ──────→ T3a                     T4 (deferred bookmark)
ComputeNode contract ─→ T1a, T3b
```

## §10 — Out of scope / future

- **Membranes / fabric surfaces (planned next, breadcrumbed in T4).** A tensegrity *pavilion* uses membrane that is simultaneously a load source and a structural element. The representation (§3) is left open to a `surfaces` element group precisely so this is additive. Form-finding stays in the FD family via the **Natural Force Density Method** (surface-stress-density on triangulated patches, Pauletti & Pimenta); load analysis reuses the v0.4 shell/CST element family (a membrane = bending-dropped shell). The **Dynamic Relaxation / energy-minimization** path (seeded by an FD approximation) is the recorded fallback for large-displacement, slack-dominated, or wrinkling membrane behaviour beyond what FD + linear-about-prestress covers.
- **Dynamic Relaxation as a general solver.** Considered and rejected as the *primary* method (it reuses none of the existing FEA/`K_g` infra and adds a damping/time-step tuning surface ill-suited to the deterministic-caching pipeline). Retained only as the membrane large-displacement fallback above.
- **Nonlinear (large-displacement) tensegrity.** Form-finding and load analysis here are linear-about-prestress. Geometrically-nonlinear tensegrity (snap-through, deployable folding) is future work.
- **Actuation / deployable tensegrity** (variable-length members, control). Out of scope.

## §11 — Open (tactical) questions

These are implementation-time choices, not design blockers; none affects the architecture above.

1. **Force-density input form.** Explicit per-member `q` vs per-group relative ratios + eigenvalue auto-scaling vs both. Lean: support both, ratios as the ergonomic default. Decide in T1b.
2. **`solver::tensegrity_load` vs extending `solver::elastic_static`.** Whether load analysis is a distinct target or the existing FEA target taught about bar/cable elements. Decide in T3b once the element family lands.
3. **Result type granularity.** One `FormFindResult` carrying stability fields, vs a separate `StabilityResult`. Lean: one result with optional stability block populated when requested. Decide in T2.
4. **`q`-search algorithm for the free-standing case.** Eigenvalue minimization vs direct null-space-of-equilibrium-matrix solve vs an existing adaptive scheme. Bounded faer work either way; pick during T1b against the T-prism golden.
5. **Active-set convergence guard.** Iteration cap + diagnostic when slack-cable active-set cycles. Decide in T3b.
