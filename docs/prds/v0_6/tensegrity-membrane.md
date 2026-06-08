# Tensegrity Membrane / Surface-Element Layer (Natural Force-Density + CST Membrane)

Status: contract. Authored 2026-06-08 in interactive `/prd` session under G1–G6+META gates. Promotes the §10 breadcrumb / deferred bookmark **T4** of `docs/prds/v0_6/tensegrity-structures.md` (fused-memory task **3799**) into a full B+H PRD. Approved by Leo before queueing tasks.

Greenfield extension of the v0.6 tensegrity roadmap — no gap-register cluster covers it. It is the **surface-element** sibling of the landed line-element layers: where `tensegrity-structures.md` delivered struts + cables (1-D members), this delivers **membrane patches** (2-D surface elements) that are *simultaneously a structural element and a load source* — the defining feature of a tensegrity **pavilion**.

## §0 — Purpose and scope

A tensegrity **pavilion** stretches a prestressed fabric membrane across a self-stressed strut/cable network. The membrane is not decoration: its in-plane prestress **shapes** the equilibrium form (a structural element) and it **catches** wind/snow (a load source). Reify today can form-find and load-analyse the strut/cable network (`tensegrity-structures.md`, landed) but has no surface element — a pavilion's fabric cannot be declared, form-found, or loaded.

This PRD adds the surface-element layer as four capabilities, each mirroring a **landed** strut/cable precedent so the architecture is a known-good shape rather than a new one:

1. **Representation** — a `surfaces` typed-element group + a `Membrane` constitutive structure in the `.ri` network. *Mirrors* T0a (struts/cables). The shipped `tensegrity.ri` already documents this as a **two-line additive** seam — not a schema break.
2. **Form-finding** — the **Natural Force-Density Method** (NFDM; Pauletti & Pimenta 2008): per-triangle surface-stress contributions assemble into the **same** global force-density matrix `D` the cable/strut FDM already builds, so a membrane patch and a cable net solve as **one** linear system. *Mirrors* T1a/T1b (anchored → combined). Stays in the FD family — like the rest of the roadmap.
3. **Load analysis** — a linear-about-prestress **CST membrane element** (3-DOF/node) added to `reify-solver-elastic`, reusing the shell's validated membrane stiffness block + the v0.5 buckling `K_g` geometric-stiffness stack. *Mirrors* T3a/T3b (pin-jointed bar element + load analysis).
4. **Large-displacement fallback (recorded, not built)** — FD-seeded **Dynamic Relaxation / energy-minimization** for slack/wrinkling membranes beyond linear-about-prestress. Filed as a **deferred bookmark** (the same T4-bookmark pattern this PRD itself promotes), explicitly **not** the primary method (parent §10 rejected DR as primary).

**Naming.** The surface method is the **Natural Force-Density Method**, abbreviated **NFDM** in prose only; the stdlib surface (per the resolved decisions below) **extends the existing `form_find` / `form_find_free` entry points** rather than adding a new verb. The membrane element lives alongside the bar element under `reify-solver-elastic`.

**Resolved design decisions (this session).**

| # | Decision | Rationale |
|---|---|---|
| D1 | **NFDM isotropic-first**, anisotropic warp/weft as a later extension slice. | For isotropic surface stress σ the per-triangle contribution is exactly the **cotangent-Laplacian** (discrete Laplace–Beltrami) — robust, well-understood, validates against analytic minimal surfaces. Covers the soap-film / uniform-prestress pavilion case. Anisotropic adds a per-triangle material frame + 2×2 stress tensor; isolated to its own slice. Mirrors the parent's anchored-2a → free-2b staging. |
| D2 | **Dedicated 3-DOF/node CST membrane element** for load analysis, not the full 18-DOF shell. | `accumulate_membrane_k` (the patch-test-validated CST block) exists but is private inside the 18-DOF shell builder. A flat prestressed membrane's bending term is negligible. A standalone 3-DOF element is the 2-D analogue of T3a's pin-jointed bar — no rotational-DOF singularity, smaller SPD system. The brief calls surfacing it "trivial scaffolding, not FE-kernel work." |
| D3 | **Extend the existing `form_find` / `form_find_free` targets** to accept the surface group, rather than a new `solver::form_find_surface` target. | NFDM contributions assemble into the **same** `D`; one combined system keeps the "struts + cables + membrane solve together" story whole and avoids duplicating the anchored/free split. Mirrors the parent §11-Q2 lean (extend, don't fork). |
| D4 | **DR / energy-min recorded as a deferred-bookmark task**, not built. | Explicitly a fallback for behaviour beyond FD + linear-about-prestress; reuses none of the existing infra. The T4-bookmark pattern keeps the future path a tracked (dormant) task without committing scope. |

## §1 — Foundation summary (prerequisites — all landed unless noted)

This PRD is **almost entirely a consumer of landed contracts**; its novel content is the NFDM triangle assembly, the CST membrane element, and the `surfaces` representation. It builds on:

- **Tensegrity line layers** (`docs/prds/v0_6/tensegrity-structures.md`). T0a representation (`tensegrity.ri`: `Strut`/`Cable`/`Tensegrity` + `tensegrity_wires`), T1a anchored `solver::form_find`, T1b free-standing `solver::form_find_free`, T2 stability, T3a pin-jointed bar/cable element + `K_g` — **all landed**. T3b load analysis with tension-only active-set is **pending** (the load path this PRD's membrane-load slice reuses; wired as a dependency).
- **ComputeNode contract** (`docs/prds/v0_3/compute-node-contract.md`, GR-002). Form-finding and load analysis route through this seam exactly as the line layers do — `@optimized("solver::…")` → ComputeNode → `reify-solver-elastic` trampoline (`crates/reify-eval/src/compute_targets/form_find.rs`). Cancellation, pending lifecycle, warm-state, caching come for free. **This PRD owns only the `solver::` targets, not the dispatch mechanism.**
- **Shell / CST element family** (`docs/prds/v0_4/…` shells, landed). `accumulate_membrane_k` (`crates/reify-solver-elastic/src/shell_assembly.rs:230`) is the textbook constant-strain-triangle membrane block, validated to analytical energy at **1e-9** (`shell_membrane_patch_test_uniform_in_plane_strain_matches_analytical_energy`, `shell_assembly.rs:1323`). It is **private** inside the 18-DOF shell builder; D2 surfaces a 3-DOF membrane element reusing its strain-displacement core. The curved-shell bending-accuracy caveat (tasks 4068/4069, MacNeal-Harder) is **irrelevant** to an in-plane-prestressed flat membrane and does not gate this PRD.
- **Buckling `K_g`** (`docs/prds/v0_5/buckling-eigensolver.md`, GR-024). The geometric-stiffness assembly the bar `K_g` (`crates/reify-solver-elastic/src/geometric_stiffness/bar.rs`) already reuses. This PRD adds a **membrane** `K_g` (in-plane-prestress geometric stiffness, the 2-D analogue) into the same assembly; it does not re-architect the eigensolver.
- **Structure-instance runtime** (GR-001, landed). The `Membrane` structure evaluates to `Value::StructureInstance` via the same SIR ctor-lowering path `Strut`/`Cable`/`ElasticMaterial` use.

## §2 — Consumer and user-observable surface (G1)

**Ultimate consumer: a user designing a tensegrity *pavilion* in a `.ri` file**, plus the GUI viewport that renders the fabric. Every layer terminates in something the user sees:

| Layer | Mechanism | Consumed by | What the user observes |
|---|---|---|---|
| M0 Representation | `surfaces` group + `Membrane` structure + `tensegrity_surfaces` accessor | `form_find`, viewport, membrane-load element | A `.ri` declares a membrane patch (nodes + triangle index-triples + a `Membrane`); `reify eval` dumps tagged `TensegritySurface` facets; viewport renders a filled surface |
| M0b GUI styling | member-type tag on emitted facets | viewport | Membrane renders as a shaded/translucent surface, visually distinct from heavy struts / thin cables |
| M1 Form-finding (NFDM) | extended `solver::form_find` / `solver::form_find_free` | the user's `.ri` (`let form = form_find(...)`); viewport | Fixed-boundary membrane form-finds to a minimal surface; a full pavilion (struts+cables+membrane) form-finds in **one** solve; `result.nodes` / surface stresses readable in CLI |
| M2 Load analysis | CST membrane element (`K_e` + membrane `K_g`) | `solve_elastic_static` / membrane-load target over a form-found pavilion | Membrane under wind/snow load reports nodal deflections + membrane-stress deltas; a slackening edge cable drops to zero force |
| M3 Integration | the pavilion as one artifact | end user + CI | `examples/tensegrity_pavilion.ri` form-finds **and** carries load — the membrane is both structure and load source |

No mechanism is producer-only: every §9 phase ends in a CLI-readable result field, a viewport signal, or an example `.ri` that runs in CI (`feedback_task_chain_user_observable`).

## §3 — Representation contract (layer M0)

**Grammar: confirmed against existing grammar (G3) — no grammar prerequisite.** The shipped `crates/reify-compiler/stdlib/tensegrity.ri` (lines 16–22) *already prescribes* this seam verbatim:

> Adding a future `surfaces` group (§10 / T4) is two-line additive: (i) add `param surfaces : List<List<Int>>` to Tensegrity, (ii) add a sibling `tensegrity_surfaces(t)` accessor — existing wire-consuming call sites do not change.

`surfaces : List<List<Int>>` is the **same parse shape** as the landed `struts`/`cables` fields (a list of index lists); `Membrane(...)` is the same structure-constructor shape as the landed `Strut(...)`/`Cable(...)`; `tensegrity_surfaces(t)` is the same call shape as `tensegrity_wires(t)`. **No novel syntax** (G3 N/A for grammar; `grammar_confirmed=true`).

**Shape (mirrors the shipped `Strut`/`Cable` discipline; exact field names finalized in the M0 task):**

```
// A membrane patch is a triangulated surface. Triangles reference nodes by
// index — the 2-D analogue of a strut/cable's index *pair*.
structure def Membrane {            // surface (tension) element
    param thickness : Length
    param material  : ElasticMaterial
    param prestress : Pressure = 0 * 1Pa    // isotropic surface prestress (σ); warp/weft is the aniso extension
}

structure def Tensegrity {
    param nodes    : List<Point3<Length>>   // shared node table (form-finding refines)
    param struts   : List<List<Int>>        // index pairs   (landed)
    param cables   : List<List<Int>>        // index pairs   (landed)
    param surfaces : List<List<Int>>        // index TRIPLES (triangles) — ADDITIVE, this PRD
}
```

- **Triangles only.** Index-triples are the primitive; the NFDM works on triangulated patches. Quad/polygon sugar and boundary-loop auto-triangulation are out of scope (a future ergonomic layer) — the user supplies a triangulated mesh, which is also the form-finding discretization. (`feedback_breadcrumb_design_alternatives_at_impl_site`: leave the auto-triangulation breadcrumb at the impl site.)
- **`Membrane` carries constitutive data** (thickness, material, prestress) the way `Strut`/`Cable` carry section_area/material/pretension — needed by the **load** element. Form-finding reads the **surface stress** (see §4), defaulting from `Membrane.prestress`.
- **Geometry emission.** A `Tensegrity` with surfaces realizes its membrane as a triangulated surface mesh: each triangle becomes a `TensegritySurface` facet record (three node indices + inline corner coordinates + a `kind: "membrane"` tag), sibling to the landed `TensegrityWire`. The viewport (M0b) and any export style/colour it. Do **not** add an axis triad to the model (`feedback_gui_auto_axis_triad`).
- **Open-group invariant preserved.** `surfaces` is a third typed element group; layers M1–M2 read it through the new `tensegrity_surfaces(t)` accessor exactly as M1/M2 of the line layers read `tensegrity_wires(t)`. Existing wire-consuming call sites do not change.

## §4 — NFDM form-finding contract (layer M1)

**The reduction — NFDM assembles into the SAME `D`.** For a triangle *T* = (i,j,k) carrying an **isotropic** surface stress σ (force/length = σ_Cauchy · thickness), the nodal force it exerts is *linear in the coordinates* once σ is fixed — exactly the property that makes Schek's cable FDM linear. The per-triangle contribution to the global force-density matrix is

```
D_T  =  σ · L_T,    where L_T = the triangle's cotangent-Laplacian (discrete Laplace–Beltrami) stencil
```

so the **combined** system over a strut/cable/membrane network is one matrix:

```
D x = P,    D = Cᵀ Q C  (lines, landed)  +  Σ_T σ_T L_T  (surfaces, this PRD)
```

This is the architectural win: a membrane patch and a cable net **share one linear solve**. The line and surface assemblies just both write into `D`.

**Anchored case (M1a — minimum vertical slice).** Partition nodes into free (`f`) and fixed-boundary (`n`). Per coordinate axis `D_ff x_f = P_f − D_fn x_n` — one faer linear solve per axis, exactly as the landed anchored line solve. A fixed-boundary membrane patch with isotropic prestress form-finds to a **minimal surface**. This is the first surface consumer slice and the cleanest validation target.

**Combined / free-standing case (M1b).** A full pavilion: struts (q<0) + cables (q>0) + membrane (σ>0) assemble into one `D`, solved through the landed `form_find_free` null-space / adaptive machinery. The membrane stress σ enters the same per-group ratio search the line groups already use. The membrane both shapes the form and stiffens the network.

**Anisotropic extension (M1c).** Warp/weft prestress σ_w ≠ σ_f needs a per-triangle local material frame and a 2×2 stress tensor; the triangle contribution generalizes beyond the plain cotangent-Laplacian. Isolated to its own slice (D1).

**Sign / feasibility convention (load-bearing; breadcrumb at impl site).** Membrane surface stress is tension (σ > 0). A degenerate triangle (zero area), a non-positive prestress where tension is required, or a boundary that admits no equilibrium is **infeasible input** → a clean `E_FormFindInfeasible` diagnostic, not a silent wrong answer (mirrors the landed line-solver contract).

**Inputs / outputs.** The existing `FormFindResult` (nodes / member_forces / force_densities / converged) is **extended** with a `surface_stresses : List<...>` field (per-triangle solved σ, populated only when surfaces are present; mirrors `member_forces`). The `form_find` / `form_find_free` signatures gain the surface group + per-patch surface-stress inputs as additive arguments (D3; exact arg names finalized in M1a).

**G6 — premise honesty (critical).**
- The **equilibrium residual** `‖D x − P‖` is a *linear* solve → genuinely ~**1e-9** (machine precision). Honest to assert.
- The **form-found shape** vs an analytic minimal surface is a **mesh-convergence bound, never an exact number.** The validation golden is the **catenoid** `r(z) = c · cosh(z/c)` (the closed-form minimal surface between two coaxial rings): assert the recovered shape's relative L2 error **decreases at the cotangent-Laplacian's O(h²) rate** / sits below a stated tolerance at the example mesh — *not* an exact coordinate frozen into a RED test.
- **Explicit trap flagged:** the "soap-film on a square frame with raised opposite corners ≈ hyperbolic paraboloid" intuition is **false** — that minimal surface is *not* exactly a hypar. Do **not** assert hypar-exactness (same class as the spline-natural-end-condition and buckling-bending-lock false premises in `esc-3770-1` / `esc-3453`). Use the catenoid, where the closed form is real.

## §5 — Membrane load-element contract (layer M2)

**Dedicated 3-DOF/node CST membrane element** (D2), added to `reify-solver-elastic` alongside the tet/hex/wedge/shell/bar families:

- **Elastic stiffness `K_e`** — reuse `accumulate_membrane_k`'s strain-displacement core (`shell_assembly.rs:230`), surfaced as a public 3-DOF (3 translational DOF/node) constant-strain-triangle membrane element. A flat element with no rotational DOFs → no SPD-suppression trick needed.
- **Geometric (prestress) stiffness `K_g`** — the in-plane-prestress transverse-stiffening matrix, the **2-D analogue** of the bar `K_g` (`geometric_stiffness/bar.rs`). New kernel, but assembles through the **same** GR-024 global-assembly machinery; does not re-architect the eigensolver.
- **Tension-only membranes under load** — a membrane that goes slack (compressive principal stress) drops out via the **same active-set loop** T3b builds for slack cables (wired as a dependency; not a new solver paradigm).

**Composition.** Analysing a built pavilion = take the form-found geometry + prestress, assemble membrane `K_e + K_g` (and bar/cable `K_e + K_g` for the line members), apply external loads (wind/snow as surface pressure), solve via the existing CG path, report nodal deflections + membrane-stress deltas. Routes through ComputeNode like every other solver.

**G6 — premise honesty.**
- **In-plane patch test** (uniform constant strain): **1e-9**, honest — a CST *exactly* reproduces constant strain (the `accumulate_membrane_k` patch test already proves this).
- **Membrane `K_g` element unit:** a single prestressed triangle's `K_e`/`K_g` match **hand-computed** values (exact-to-hand-computation, like the T3a bar unit).
- **Pretensioned-membrane-under-pressure golden:** a uniformly pretensioned flat membrane under uniform transverse pressure has a closed-form small-deflection solution (`σ t ∇²w = −p`, Fourier-series center deflection). Assert deflection within a **mesh-convergence bound** of that solution — *not* an exact number (CST membrane bending-like response converges O(h²)).

## §6 — Cross-PRD seam ownership (G4)

All seams are **REUSE** of landed contracts or **completion** of the parent — no new contested ownership, no reciprocal "the other owns it."

| Seam | Owner | Direction | Relationship |
|---|---|---|---|
| `surfaces` typed-element group on `Tensegrity` + accessor | **this PRD** (completes `tensegrity-structures.md` T4) | produces | The parent left the open-group seam *documented* (`tensegrity.ri:16–22`); this PRD fills it. Closes/supersedes the T4 deferred bookmark (task 3799). |
| `@optimized` dispatch / cache / cancellation / warm-state | **ComputeNode contract** (GR-002) | consumes | Owns the mechanism. This PRD registers only the surface-extended `solver::form_find` behaviour + the membrane-load target. No redefinition. |
| Structure-instance runtime (`Membrane` → `Value::StructureInstance`) | **structure-instance-runtime** (GR-001) | consumes | Prerequisite, landed. Same path `Strut`/`Cable`/`ElasticMaterial` use. |
| CST membrane stiffness block (`accumulate_membrane_k`) | **v0.4 shells** | consumes | Reuse. This PRD *surfaces* a 3-DOF membrane element from the existing private block + adds the membrane `K_g`. Does not modify the shell element. |
| Geometric-stiffness assembly + eigensolver (`K_g`) | **buckling-eigensolver** (GR-024) | consumes | Reuse. Adds the membrane-element `K_g` into the existing assembly. |
| Tension-only active-set load loop | **tensegrity-structures.md T3b** (task 3798, pending) | consumes | The membrane-load slice reuses T3b's active-set wrapper for slack surfaces; wired as a hard dependency. |
| GUI viewport membrane styling | **GUI** | produces tag / consumes render | This PRD emits the `kind: "membrane"` facet tag + styling intent (§3); the viewport owns the rendering channel (M0b touches `gui/`). |

Mild-contradiction watch (`structural-analysis-fea ↔ shells`): N/A here — this PRD only *reads* the shell membrane block, it does not touch shell ownership.

## §7 — Contract section (B + H)

The seam crosses `reify-eval` (graph + ComputeNode dispatch) and `reify-solver-elastic` (NFDM + membrane-load trampolines). The contract an implementer needs to build the producer side without further discussion:

**Representation (M0).**
- `Tensegrity` gains `param surfaces : List<List<Int>>` (triangle index-triples; each inner list length **exactly 3**, 0-based into `nodes`). Out-of-range or non-triple → `Value::Undef` on the accessor (mirrors `tensegrity_wires`' index validation).
- `structure def Membrane { thickness: Length; material: ElasticMaterial; prestress: Pressure = 0Pa }` — required thickness + material, defaulted prestress (mirrors `Cable.pretension`).
- `tensegrity_surfaces(t) -> List<TensegritySurface>` — one facet per triangle, declaration order; `TensegritySurface { kind: String, i0/i1/i2: Int, x0..z2: Length }`.

**Form-finding (M1).**
- Extend the `@optimized("solver::form_find")` / `("solver::form_find_free")` decls with additive surface arguments (surface group connectivity + per-patch surface stresses). Body stays the `{ FormFindResult() }` never-run inline fallback (landed discipline).
- Trampoline (`compute_targets/form_find.rs`): assemble `Σ_T σ_T L_T` into the same `D` as the line contributions; one combined linear solve (anchored) / null-space recovery (combined). `FormFindResult.surface_stresses` populated with real per-triangle σ on the production path (non-`Undef`; field-population invariant). Infeasible input (degenerate triangle, no equilibrium, sign violation) → `ComputeOutcome::Failed` + `E_FormFindInfeasible`, never a `converged:false` result.
- **Ordering invariant:** struts-then-cables-then-surfaces, matching the emission order, so stress-to-element indexing is unambiguous.

**Load element (M2).**
- Public 3-DOF/node CST membrane element exposing `K_e` (from `accumulate_membrane_k`'s strain-displacement core) and a membrane `K_g(σ)`. Assembles through the GR-024 global path. Slack (compressive principal stress) handled by T3b's active-set loop.
- Result fields (deflections, membrane-stress deltas) populated with real sampleable values on the production path (field-population invariant; not `Undef`, not test-only).

## §8 — Boundary-test sketch (cross-crate; facing both ways) (H)

The integration-gate task (M3 / θ) names this table as its observable signal (closing G2's loop).

### 8.1 Producer-side (`reify-eval` looking outward at the trampolines)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Combined dispatch | strut+cable+membrane `Tensegrity`, inputs Final | One ComputeNode `solver::form_find_free` inserted (no inlining); `D` contains both line and surface contributions; `FormFindResult.nodes` + `surface_stresses` populated; freshness Final |
| Surface-infeasible diagnostic | degenerate triangle / non-tension prestress | Clean `E_FormFindInfeasible`; output VCs → `Failed`; no panic |
| Warm-state reuse | re-solve, perturbed prestress | Prior factorization reused as warm start; dispatch-count instrumentation confirms cache hit (per ComputeNode contract §2) |
| Cancellation under design loop | `param … = auto` driving prestress; solve mid-iteration | Cancellation within 2× poll budget; prior cache intact |

### 8.2 Consumer-side (`reify-solver-elastic` looking inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Anchored minimal-surface golden | fixed-boundary membrane patch (catenoid rings) | Equilibrium residual ~1e-9; recovered shape within a **mesh-convergence bound** of `r(z)=c·cosh(z/c)` — the M1a signal |
| Combined pavilion golden | struts + cables + membrane in one solve | Solved coords consistent; struts compressive, cables tensile, membrane σ>0 (sign check) — the M1b signal |
| Membrane `K_e`/`K_g` unit | single prestressed triangle | Element matrices match hand-computed values; in-plane patch test 1e-9 |
| Pretensioned-membrane-under-pressure | uniform σ, uniform transverse `p` | Center deflection within a **mesh-convergence bound** of the `σ t ∇²w=−p` closed form |
| Slack membrane active-set | load case compressing one patch's principal stress | That patch drops from the active set; deflection matches a reference / DR cross-check |
| Pavilion end-to-end (the θ leaf) | `examples/tensegrity_pavilion.ri` in CI | Form-finds (NFDM, one `D`) **and** carries load (CST `K_e+K_g`); both result fields populated; viewport renders the surface |

## §9 — Vertical-slice DAG (B + H)

Decomposition **B (vertical slice) + H (contracts + two-way boundary tests)** per `preferences_implementation_chain_portfolio` — high-stakes (FEA + ComputeNode seam), 3-crate blast radius, ~8 mechanisms → B+H. Each leaf names its **user-observable signal** (G2). All tasks filed `planning_mode=True`; deps wired as real edges before the batch flips `deferred → pending`. The DR-fallback task (ι) is **excluded from the pending flip** (deferred bookmark).

### Tier M0 — Representation

- **α — `surfaces` group + `Membrane` structure + surface geometry emission.**
  - *Signal:* a pavilion `.ri` declares a `Membrane` + a `surfaces` triangle list; `reify eval` dumps tagged `TensegritySurface` facets (kind `"membrane"`); viewport receives a filled surface mesh.
  - *Prereqs:* T0a (3792, landed; the open-group seam), GR-001. *Crates:* reify-compiler/stdlib, reify-eval, reify-types.
  - *consumer_ref:* β, γ, ζ + viewport. *grammar_confirmed:* true.

- **β — GUI membrane surface styling.**
  - *Signal:* viewport renders the membrane as a shaded/translucent surface, distinct from heavy struts / thin cables; screenshot diff confirms the distinction.
  - *Prereqs:* α. *Crates:* gui/. *consumer_ref:* end user. (Leaf.)

### Tier M1 — NFDM form-finding

- **γ — Anchored isotropic NFDM form-find (extend `solver::form_find`).** Minimum vertical slice.
  - *Signal:* a fixed-boundary membrane `.ri` form-finds to a minimal surface; equilibrium residual ~1e-9 via CLI; recovered shape within a stated mesh-convergence bound of the analytic catenoid; a `solver::form_find` ComputeNode is in the graph; re-run hits cache.
  - *Prereqs:* α, T1a (`form_find` anchored, landed), ComputeNode contract (landed). *Crates:* reify-solver-elastic (NFDM triangle assembly into `D`), reify-compiler/stdlib, reify-eval.
  - *consumer_ref:* δ, θ. *grammar_confirmed:* true.

- **δ — Combined struts+cables+membrane form-find (extend `solver::form_find_free`).**
  - *Signal:* a pavilion `.ri` (struts+cables+membrane) form-finds all three groups in **one** `D`/solve; the membrane shapes the form; struts compressive, cables tensile, membrane σ>0 in `result`.
  - *Prereqs:* γ, T1b (3795, `form_find_free`, landed). *Crates:* reify-solver-elastic, reify-eval.
  - *consumer_ref:* θ, ι. *grammar_confirmed:* true.

- **ε — Anisotropic warp/weft NFDM extension.**
  - *Signal:* a warp/weft-prestressed patch (σ_w ≫ σ_f) form-finds to a shape distinct from the isotropic minimal surface, with the recovered principal-stress directions aligned to the warp axis (read off `result`); equilibrium residual ~1e-9.
  - *Prereqs:* δ. *Crates:* reify-solver-elastic. (Leaf; in-scope extension slice per D1.)

### Tier M2 — Membrane load element

- **ζ — Dedicated CST membrane element (`K_e` + membrane `K_g`).** Parallel with M1.
  - *Signal:* a single prestressed membrane triangle's `K_e`/`K_g` match hand-computed values; in-plane patch test 1e-9; a pretensioned membrane under uniform transverse pressure deflects within a mesh-convergence bound of the `σ t ∇²w=−p` closed form (via an example `.ri`).
  - *Prereqs:* α, shell membrane block (landed), GR-024 buckling `K_g` (landed). *Crates:* reify-solver-elastic.
  - *consumer_ref:* η.

- **η — Membrane load analysis (form-found pavilion under load).**
  - *Signal:* a form-found pavilion membrane under wind/snow load reports nodal deflections + membrane-stress deltas (result fields populated, non-`Undef`); a load case that slackens a patch shows its principal stress dropping out of the active set.
  - *Prereqs:* ζ, δ, T3b (3798, active-set load path, **pending** — wired). *Crates:* reify-solver-elastic, reify-eval (ComputeNode routing).
  - *consumer_ref:* θ, ι.

### Tier M3 — Integration gate (critical leaf, B+H)

- **θ — Pavilion end-to-end CI example + two-way boundary tests.**
  - *Signal:* `examples/tensegrity_pavilion.ri` — a prestressed membrane patch on a strut/cable network — form-finds (NFDM, combined `D`) **and** carries load (CST `K_e+K_g`); both result fields populated + viewport surface render; the §8 boundary-test table passes; runs in CI.
  - *Prereqs:* δ, η, β. *Crates:* reify-solver-elastic, reify-eval, examples, gui/ (smoke). *consumer_ref:* end user + CI. (Leaf — the integration gate; names §8 as its signal, closing G2.)

### Tier M4 — Large-displacement fallback (deferred bookmark; §10)

- **ι — FD-seeded Dynamic Relaxation / energy-min (DEFERRED BOOKMARK).**
  - Filed `planning_mode=True`, **excluded from the pending flip** — stays `deferred` until Leo triggers it (`preferences_bookmark_task_pattern`). Carries the §10 breadcrumb: FD form-find as the seed, DR/energy-min for slack/wrinkling beyond linear-about-prestress.
  - *Signal (when eventually done):* a slack/wrinkling membrane case that FD + linear-about-prestress cannot resolve form-finds via FD-seeded DR; deflections cross-check a reference.
  - *Prereqs (recorded, for when activated):* δ, η.

### Dependency view

```
T0a(3792) ─→ α ─┬─→ β
                ├─→ γ ─→ δ ─→ ε
                │         │
                └─→ ζ ────┼─→ η ─→ θ
                          └────────┘
T1a(form_find) ────→ γ            β,δ,η ──→ θ  (integration gate)
T1b(3795) ─────────→ δ
GR-024 K_g + shells → ζ
ComputeNode ───────→ γ, η
T3b(3798, pending) ─→ η
                           δ, η ──→ ι  (deferred bookmark)
```

## §10 — Out of scope / future

- **Dynamic Relaxation / energy-minimization as a general solver** — recorded as the deferred bookmark ι; the FD-seeded fallback for large-displacement, slack-dominated, or wrinkling membrane behaviour beyond FD + linear-about-prestress. Considered and **rejected as primary** (parent §10): reuses none of the existing FD/`K_g` infra and adds a damping/time-step tuning surface ill-suited to the deterministic-caching pipeline.
- **Quad / polygon surface patches + boundary-loop auto-triangulation.** This PRD takes a user-supplied triangulated mesh (index-triples). Ergonomic auto-meshing is future work.
- **Geometrically-nonlinear (large-displacement) membranes** — snap-through, deployable folding, finite wrinkling fields. Form-finding and load analysis here are linear-about-prestress.
- **Actuation / variable-prestress pavilions** (controlled tensioning). Out of scope.

## §11 — Open (tactical) questions

Implementation-time choices, not design blockers; none affects the architecture above.

1. **Catenoid vs alternative minimal-surface golden.** Catenoid is the recommended closed-form validation. A scaled Scherk/Enneper patch is an alternative if the ring boundary is awkward to mesh. Decide in γ against the mesh.
2. **`surface_stresses` field granularity.** Per-triangle σ on the extended `FormFindResult`, vs a per-patch summary. Lean per-triangle (mirrors `member_forces`). Decide in γ.
3. **Wind/snow load form.** Uniform transverse pressure vs a follower-pressure (deformation-tracking) load. Lean uniform transverse for linear-about-prestress; follower is a nonlinear extension. Decide in η.
4. **Anisotropic principal-direction golden.** Exact analytic shape for warp/weft is awkward; the ε signal asserts principal-direction alignment + residual rather than an exact shape. Confirm the qualitative golden during ε.
5. **Active-set guard for slack membranes.** Iteration cap + diagnostic when patch active-set cycles (mirrors T3b's cable guard). Decide in η.
