# FEA Result-Model + GUI-FEA Integration

**Milestone:** v0.4 · **Status:** deferred (contract resolving the FEA false-premise hot zone) · **Date:** 2026-05-30
**Approach:** B + H (vertical slice + contract section + two-way boundary tests). FEA is a load-bearing seam (overlay G5).

---

## 0. Why this PRD exists (supersession + provenance)

The 2026-05-30 premise-validation review (`.orchestrator-scratch/v0_6-premise-review-report-2026-05-30.md`) found the systemic false-premise gap is **concentrated in one root cause**: *FEA result Values never populate their sampleable fields.* `ElasticResult.{displacement,stress,frame}` are all `Value::Undef` (`crates/reify-eval/src/compute_targets/elastic_static.rs:196-198`; `buckling.rs:260-262`). Every downstream task that samples or reduces those fields inherits a false premise — which is why the review found a *cluster* of parked tasks (2962, 2930, 3015, 3018, 3026, 2966, 2968) and re-home candidates (3005, 2929) all blocked on the same missing capability.

This PRD is the coherent re-plan the review calls for (§5 option A, §8 decision #1 — *"highest leverage in the whole review"*). It is the **result-model seam** in a three-PRD chain:

```
structural-analysis-fea.md   (PRODUCER half)   Body → realized VolumeMesh → solve → nodal results
        │  done: mesher 2925/2917, ComputeNode integration 2924
        │  remaining (gates 2930): trampoline-consumes-realized-mesh (P1), face-selector BCs (P2), 3429 realization edge
        ▼
fea-result-model.md  ← THIS PRD (the SEAM)      nodal results → Sampled Value::Field → field reductions → GUI surface-vertex sampling + GUI dispatch wiring
        │  ships the prismatic vertical slice end-to-end NOW
        ▼
fea-gui-rendering.md         (RENDERER half)    surface scalars → contour / deformation / probe / overlay  (mostly WIRED, fed by fixtures, awaiting a real producer)
```

The seam is **correspondence-agnostic**: it works on the synthetic cantilever solve today (prismatic-exact) and on a realized arbitrary mesh the moment `structural-analysis-fea.md` finishes its producer half. We deliberately do **not** pull the mesher/BC seam into this PRD — the Gmsh mesher (2925) and ComputeNode integration (2924) are already done, and the face-selector BC seam is contested territory owned by `structural-analysis-fea` + the topology-selectors seam (overlay G4). Owning it here would bury the result-model fix behind a large kernel effort and duplicate seam ownership.

Audit cross-references: `docs/architecture-audit/findings/fea-gui-rendering.md` **M-006** (per-vertex `scalar_channels` IPC — schema present, never populated), **M-010** (stress-contour end-to-end wiring — FICTION), **M-019** (ElasticResult kernel→engine integration — now done as 2924).

---

## 1. Goal — what a user observes when this lands

1. Load `examples/fea_cantilever_smoke.ri` in the GUI, trigger a solve → **a real von-Mises stress contour renders on the beam surface** and a **max-von-Mises readout** appears in the FEA toolbar (today: nothing — the ElasticResult is `Undef` because the GUI engine never registers the FEA trampoline). Drag the warp slider → **the deformed shape animates**.
2. In a `.ri` design: `let peak = max(von_mises(fea.stress))` evaluates to a real `Scalar<Pressure>` (today: `Value::Undef`), usable in `constraint peak < material.yield_stress` — the **design-loop predicate becomes callable**.
3. `linear_combine` / `envelope_von_mises` over multi-case results produce **non-vacuous** fields (today they reduce over `Undef` and silently return `Undef`).
4. An under-constrained body produces a diagnostic that **points at the offending `FixedSupport`'s source location**, and the GUI **diagnostic overlay** renders rigid-body-mode arrows.

---

## 2. Background — the verified substrate (first-hand, 2026-05-30)

| Fact | Evidence |
|---|---|
| `ElasticResult` populates only `max_von_mises`/`converged`/`iterations`; `displacement`/`stress`/`frame` = `Value::Undef` | `elastic_static.rs:196-198`; `buckling.rs:260-262` |
| The P1 interpolation/recovery primitives exist and are deliverable, but are wired into **no** production `ElasticResult` (only `error_estimator.rs` + tests) | `recover_nodal_stress_p1` (`reify-solver-elastic/src/result.rs:356`), `interpolate_p1_at_point`/`locate_element_p1`/`barycentric_p1` (`interpolation.rs:144/189/51`); task **2920 done** |
| `Value::Field { source: Sampled, lambda: SampledField }` is a **regular grid** (Regular1D/2D/3D); every consumer (`envelope_*`, `linear_combine`) enforces `grids_equal` | `reify-ir/src/value.rs:89,316,486`; `reify-stdlib/src/fea.rs:1033-1063` |
| `von_mises(Field)→Field` (VonMises-derived) **exists & wired** | `reify-expr/src/analysis.rs:157` `compute_von_mises`; dispatch `lib.rs:356` |
| single-field `max(Field<_,T:Ordered>)→Scalar` **exists & wired**, BUT reduces **only** `Sampled` sources — a `VonMises`-**derived** field returns `Undef` (deferred) | `reify-expr/src/field_reductions.rs:93-115`; doc at `:405` literally cites `max(von_mises(stress)) < yield_stress` |
| multi-case `envelope_von_mises`/`envelope_max_principal`/`envelope_displacement_magnitude`/`linear_combine` **exist & wired** over Sampled fields | `reify-stdlib/src/fea.rs:47-49,369,383,420,109` |
| GUI emits every `MeshData` with `scalar_channels: HashMap::new()` / `displaced_positions: None` | `gui/src-tauri/src/engine.rs:1921-1922` |
| GUI engine **never registers** the FEA trampoline (`register_compute_fns` has zero GUI call sites — test-only) → solve body-inlines to the `{ ElasticResult() }` stub → `Undef` even for `max_von_mises` | `reify-eval/src/compute_targets/mod.rs:29` (def); call sites only under `reify-eval/tests/` |
| `pending_solve_cancel` has a **consumer** (`cancel_solve_impl`) but **no producer** (always `None`) | `gui/src-tauri/src/commands.rs:59,321-333`; `main.rs:655` |
| `examples/fea_cantilever_smoke.ri` has **no `body =` realization** | file inspection (params + solve only) |
| solve uses a synthetic `nx×1×6` Freudenthal tet box from `(length,width,height)` scalars; `solve_elastic_static_trampoline` **ignores `_realization_inputs`** | `elastic_static.rs:144,252-312`; `solver_elastic.ri:489` (scalar signature, body `{ ElasticResult() }`) |
| Gmsh surface→volume mesher (`ReprKind::VolumeMesh`) and `@optimized` ComputeNode integration are **done** | tasks **2925/2917 done**, **2924 done**; `reify-kernel-gmsh/src/register.rs:92` |

---

## 3. Sketch of approach

The unstructured tet solve stays as-is. We add a **resample-to-regular-grid** step and a **sample-at-surface-vertices** step, which together resolve the FEA-node↔OCCT-surface correspondence without touching the solver core.

```
FEA tet solve (synthetic box now / realized mesh under structural-analysis-fea)
   │  recover_nodal_stress_p1 (2920) → nodal stress; nodal displacement from solve
   ▼  (α) resample onto a Regular3D grid spanning the body bounds:
   │      for each grid point: locate_element_p1 → barycentric interpolate_p1_at_point
   ▼
ElasticResult.stress       : Field<Point3, Matrix3x3<Pressure>>  source=Sampled  (was Undef)
ElasticResult.displacement : Field<Point3, Vector3<Length>>      source=Sampled  (was Undef)
   │
   ├─ (β) max(von_mises(stress)) : Scalar<Pressure>   [von_mises field-arm exists; extend max to reduce VonMises-derived]
   ├─ multi-load-case-fea: envelope_* / linear_combine  [already wired — now fed real fields]
   │
   ▼  (γ) GUI registers register_compute_fns → solve dispatches → real ElasticResult reaches GUI
   ▼  (δ) sample stress/displacement at each OCCT surface vertex (body-local):
   │      scalar_channels["vonMises"][i] = vm(sample(stress, v_i));  displaced_positions = v_i + warp·sample(displacement, v_i)
   ▼
GUI contour + deformed shape render  (ε / fea-gui-rendering renderer half)
```

**Why a regular grid is the field domain (forced, not chosen).** `SampledField` is `Regular{1,2,3}D`; the multi-case `envelope_*`/`linear_combine` reductions require `grids_equal` across cases. A node-indexed cloud would break every existing consumer. So `stress`/`displacement` must be a **Regular3D Sampled field**, produced by resampling the unstructured solve via the 2920 primitives. Grid points outside the solid carry the `f64::NAN` sentinel (skipped uniformly by the reductions' `is_finite()` discipline — see `field_reductions.rs:196`).

**Why this is honest for prismatic geometry now.** The synthetic cantilever solve *is* a box of `(length,width,height)`. A `box(length,width,height)` `.ri` fixture realizes the identical geometry, so its OCCT surface vertices fall exactly inside the resample grid's bounds → sampling is exact. Arbitrary geometry (the bracket) requires the producer half to solve on the *realized* mesh — gated on `structural-analysis-fea` (§7).

**Correction to the original esc-2962-33 capability list.** Capability (iv) *"a field `von_mises` plus a field-`max` reduction"* is **largely already shipped** — `compute_von_mises` (field arm) and `compute_max` (single-field reduction) are both wired. The *only* gap is that `compute_max`/`compute_argmax` reduce `Sampled` sources but return `Undef` for the `VonMises`-**derived** wrapper that `von_mises(stress)` yields (`field_reductions.rs:101-115`, deferred to structural-analysis-fea task #6). So the design predicate `max(von_mises(stress))` breaks at the *reduction* step even once `stress` is Sampled. Task β closes exactly that gap (project the backing Sampled tensor field per-point, reuse `analysis::compute_von_mises_3x3`).

---

## 4. Contract section (H) — seam signatures + invariants

An architect implementing the producer side should need nothing beyond this section.

### 4.1 ElasticResult field contract (producer: α)

`solve_elastic_static` / `solve_buckling` write, on the production path:

| Field | Value shape | Invariant |
|---|---|---|
| `displacement` | `Value::Field { source: Sampled, domain_type: Point3, codomain_type: Vector3<Length>, lambda: SampledField }` | `sf.data.len() == grid_count · 3` (xyz row-major); `kind == Regular3D`; finite at grid points inside the solid, `NaN` outside |
| `stress` | `Value::Field { source: Sampled, …, codomain_type: Matrix3x3<Pressure>, … }` | `sf.data.len() == grid_count · 9` (row-major σ_xx…σ_zz); same grid metadata as `displacement` |
| `frame` | `Value::Undef` (unchanged — tet-elastic convention, `solver_elastic.ri:282-289`) | — |
| `max_von_mises` / `converged` / `iterations` | unchanged (`Scalar<Pressure>` / `Bool` / `Int`) | `max_von_mises` MUST remain consistent with `max(von_mises(stress))` within solver tolerance |

**Grid-metadata invariant (load-bearing for multi-case):** for a fixed `(body, options.element_order, options.mesh_size)`, the resample grid (`bounds_min/max`, `spacing`, `axis_grids`) is **identical** across solves — so two `ElasticResult`s for the same geometry satisfy `grids_equal` and feed `envelope_*`/`linear_combine` without `Undef`. The grid resolution is derived deterministically from the solve mesh (document the rule in α; default: one grid axis-count per solve `nx/ny/nz`).

### 4.2 Resample contract (producer: α)

`resample_nodal_to_grid(nodes: &[[f64;3]], elems: &[[usize;4]], nodal_values: &[f64], stride: usize, grid: &GridSpec) -> SampledField`:
- For each grid point `p`: `locate_element_p1(elems, p, tol)`; on hit, `interpolate_p1_at_point` (component-wise over `stride`); on miss (outside solid), write `NaN`.
- Nodal stress recovered first via `recover_nodal_stress_p1` (averages per-element constant stress to nodes); displacement is nodal already.

### 4.3 Field-reduction contract (producer: β)

- `von_mises(f: Field<D, Matrix3x3<Q>>) -> Field<D, Scalar<Q>>` — `source = VonMises`, backing field in `lambda` (**exists**, `compute_von_mises`).
- `max(f: Field<D, T:Ordered>) -> Scalar<T>` / `min` / `argmax` / `argmin` — MUST reduce **both** `Sampled` **and** `VonMises`-derived sources. For a `VonMises` source: project the backing Sampled tensor field per 9-float window via `analysis::compute_von_mises_3x3`, then reduce. Other derived sources (`MaxShear`, `PrincipalStresses`, …) MAY stay deferred (return `Undef`) — out of scope; document which are covered.
- **Invariant:** `max(von_mises(stress))` is a `Scalar<Pressure>`, dimensionally comparable to `material.yield_stress` (`field_reductions.rs:983-988` warns against rewrapping to `Real`).

### 4.4 GUI surface-vertex sampling contract (producer: δ)

For an `ElasticResult` associated with a rendered entity, for each OCCT surface vertex `v_i` (body-local coords, `i ∈ [0, vertex_count)`):
- `scalar_channels["vonMises"][i] = compute_von_mises_3x3(sample(stress, v_i))` — `len == vertex_count` (the `types.rs` IPC contract).
- `displaced_positions[3i..3i+3] = v_i + warp · sample(displacement, v_i)` — `len == vertices.len()`. Warp factor is applied GUI-side per the existing slider; the channel carries `warp = 1` positions, UI scales.
- Vertices outside the field bounds: `vonMises = NaN`-skip → rendered as the colormap's out-of-range sentinel; `displaced = v_i` (no displacement). Document the OOB policy.

### 4.5 GUI dispatch contract (producer: γ)

- `Engine::new` **and** `from_engine` (`gui/src-tauri/src/engine.rs:1367,893`) MUST call `reify_eval::compute_targets::register_compute_fns(&mut engine)` — else `solve_elastic_static` body-inlines to the `{ ElasticResult() }` stub and every field (incl. `max_von_mises`) is `Undef`. (This is the esc-2962-66 root cause.)
- The solve command (`commands.rs`) MUST set `pending_solve_cancel = Some(handle)` when a solve starts (the producer the existing `cancel_solve_impl` consumer needs) and clear it on completion.
- `examples/fea_cantilever_smoke.ri` MUST gain `let body = box(length, width, height)` so a realization exists for the GUI to render the contour onto.

### 4.6 Structured-diagnostic IPC channel (producer: R3b)

R3 (task 4090) shipped the typed `FeaDiagnosticDetail` structs + the
`fea_structured_detail()` classifier, but **no emission**: `structured_detail()`
has zero production callers, every `FeaFailure` is flattened to a structureless
`reify_core::Diagnostic` at `elastic_static.rs` (`:407/416/431/710/727`) and
discarded, and `ComputeOutcome` carries no slot for the typed detail. So the
structs never reach the GUI overlay consumer (ι/2966). This section is the
producer contract for the missing emission + IPC plumbing (R3b). It was split out
of R3 because it is a separable **core-eval-result-model + GUI-IPC** change, not a
GUI-overlay concern — ι/2966 cannot legitimately reach into `reify-eval` internals.

**Channel type (neutral, `reify-eval`-owned).** A new enum in `reify-eval`
(`engine_compute.rs`) wraps solver-specific structured detail so the generic
compute-node contract never names a specific solver crate's type:

```rust
pub enum StructuredComputeDetail {
    Fea(reify_solver_elastic::FeaDiagnosticDetail),
}
```

Both `ComputeOutcome::Completed` **and** `ComputeOutcome::Failed` gain
`structured_detail: Vec<StructuredComputeDetail>` (legal because
`reify-eval` already depends on `reify-solver-elastic`; `reify_core` is untouched,
so the `reify-solver-elastic/src/diagnostics.rs:3-9` neutral-types boundary holds).
This extends the `compute-node-contract.md` §4/§5 `ComputeOutcome` shape — owned
here, and no *consumer of the existing `diagnostics` field* changes its semantics.

**Scope reality (this is NOT a 4-file change — corrected 2026-06-24, task 4802 re-scope).**
"Additive" above describes the *type contract* (no existing field's meaning changes),
NOT the edit blast radius. Because Rust enum struct-variants have no defaulted fields,
adding the **required** `structured_detail` field breaks every existing constructor
with E0063, so it must be added (`structured_detail: vec![]`) at **all ~177
`ComputeOutcome::{Completed,Failed}` construction sites across ~48 files** (~119
`Completed` / ~58 `Failed`, including ~22 test files and the contended `reify-eval`
core: `engine_compute.rs`, every `compute_targets/*.rs`, plus
`dynamics_ops`/`modal_ops`/`trajectory_ops`/`shell_extract_compute`/`engine_admin`/`compute_persist`).
These edits are mechanical and low-correctness-risk, but the producer task's
declared `files`/lock scope MUST list all ~48 files — they are not optional. The
chosen design (symmetric `ComputeOutcome` field) was ruled in over a lower-churn
asymmetric alternative because the Failed path has no result `Value` to ride and
`reify_core::Diagnostic` cannot carry the solver-typed detail (the neutral boundary),
so a `reify-eval`-layer channel is unavoidable regardless; the symmetric form keeps
both paths uniform and reusable for future solvers.

**Variant → outcome mapping (load-bearing — detail rides BOTH paths).** Verified
on main @ 959bf42094:

| `FeaFailure` source | `FeaDiagnosticDetail` | Rides | Production site |
|---|---|---|---|
| `UnderConstrained{support_count:0}` | `Unconstrained{rigid_body_modes:[…6 DOF]}` | `Completed` (auto-clamp **warning**) | `elastic_static.rs:416` — **wired source** |
| `SingularStiffness{element_id}` | `ProblemElements{ids:[ElementId]}` | `Failed` | `elastic_static.rs:708` (`classify_degenerate`, near-degenerate marginal case) — **wired source** |
| `SelectorNoMatch{selector}` | `UnresolvedSelector{selector_path}` | `Failed` | **no production source today** — `SelectorNoMatch` is never constructed in the solve path; gated on selector-BC failure emission (P2/4092, `structural-analysis-fea`). Channel-ready, data-deferred. |

The headline rigid-body-arrows overlay rides **Completed-with-warning**, NOT
`Failed` — a `Failed`-only channel would silently drop it. R3b wires
`structured_detail` by calling `failure.structured_detail()` at the `:416` and
`:708` sites and pushing `StructuredComputeDetail::Fea(_)` onto the same outcome
the message rides. The `UnresolvedSelector` arm is plumbed end-to-channel but
emits no data until selector-BC failures are constructed downstream (do not claim
a ghost-selector overlay until then; note B11 already scopes to arrows+outlines).

**GUI IPC mirror (consumer-side serde, neutral boundary preserved).** A
serde-serializable mirror of `FeaDiagnosticDetail` lives GUI-side
(`gui/src-tauri/src/types.rs`, modeled on the existing `reify_core::DiagnosticInfo`
mirror) — the neutral kernel enum gains **no** serde derive (its header already
assigns IPC serialization to the consumer). `GuiState` gains a
`fea_diagnostics: Vec<FeaDiagnosticInfo>` field (`#[serde(default)]` for
forward-compat). `gui/src-tauri/src/engine.rs` propagates from the `ComputeOutcome`
`structured_detail` channel into that field on both the success-with-warning and
failed-solve build paths. **Threading note (part of the ~48-file scope above).** The
GUI never sees `ComputeOutcome` directly — it reads solve data only from
`CheckResult`. So the channel must be threaded: `run_compute_dispatch` return (+ the
`DispatchError::Failed` payload) → `engine_eval.rs` call sites/matchers →
`EvalResult`/`CheckResult` new fields (`lib.rs`, propagated via
`engine_constraints.rs::check()`) → the `GuiState` literals in `engine.rs`.

**Failed-solve viewport state (per §6.8).** On a failed solve, `engine.rs` clears
`scalar_channels`/`displaced_positions` (no result to contour) but **populates**
`fea_diagnostics` so the overlay stays visible — matching the task-2966
"clear scalar channels, keep diagnostic overlay visible" requirement.

---

## 5. Boundary-test sketch (H) — facing both sides

The integration-gate task (ε) names this table as its observable signal.

| # | Scenario | Preconditions | Postconditions (asserts) | Side / task |
|---|---|---|---|---|
| B1 | stress/displacement populated | cantilever solve via registered trampoline | `result.stress` is `Field{Sampled, Matrix3x3<Pressure>}`, `data.len()==grid·9`, finite interior; `result.displacement` `…Vector3<Length>`, `data.len()==grid·3` | producer (α) |
| B2 | `max_von_mises` consistency | populated fields | `max(von_mises(stress))` ≈ `result.max_von_mises` within solver tol; existing `solve_elastic_static_e2e.rs` ±50%-of-6 MPa still green | producer (α) |
| B3 | design predicate callable | Sampled stress | `max(von_mises(stress))` is `Scalar<Pressure>` ≈ analytical σ_max, **not** `Undef`; `min`/`argmax` likewise | consumer (β) |
| B4 | GUI dispatch non-Undef | GUI loads fixture with `body` | a non-`Undef` `ElasticResult` with real `max_von_mises` (~6 MPa) reaches the GUI (debug-MCP `store_state`) | producer (γ) |
| B5 | contour renders | B1 + B4 | `scalar_channels["vonMises"].len()==vertex_count`; contour visible (mesh/material-swap state via debug MCP) | consumer (δ/ε) |
| B6 | deformed shape | displacement field | `displaced_positions.len()==vertices.len()`; warp slider moves vertices | consumer (δ/ε) |
| B7 | cancel handle wired | solve in flight | `pending_solve_cancel` is `Some` during solve, cleared after; rapid retick cancels prior solve (no orphaned threads) | producer (γ) |
| B8 | superposition non-vacuous | two case results, Sampled fields, shared grid | `linear_combine(A,B,{α,β}).stress` ≈ direct `α·A+β·B` solve within `Σ|w|·cg_tol·C` | consumer (ζ) |
| B9 | multi-case cache reuse | `solve_load_cases` 2 cases shared `(body,material,order,mesh_size)` | volume-mesh realization-cache increments **once** across both solves | producer (R1) |
| B10 | Support source-span diagnostic | under-constrained body | the emitted diagnostic references the `FixedSupport`'s `.ri` source span (today `None`) | producer (R2 → 2929) |
| B11 | typed diagnostics → overlay | unconstrained body | overlay renders rigid-body arrows from `Vec<DofDirection>`; problem-element outlines from `Vec<ElementId>` | consumer (R3b → ι) |
| B12 | structured detail rides outcome | unconstrained-body solve (Completed-warning) + near-degenerate solve (Failed) | `ComputeOutcome.structured_detail` carries `Fea(Unconstrained{rigid_body_modes:[…6]})` on the warned Completed; `Fea(ProblemElements{ids:[…]})` on the Failed (Rust engine test) | producer (R3b) |
| B13 | detail reaches GuiState IPC | GUI loads unconstrained-body fixture, solves | `GuiState.fea_diagnostics` is non-empty with the `Unconstrained` payload (debug-MCP `store_state`); failed solve clears `scalar_channels` but keeps `fea_diagnostics` | producer (R3b) / consumer-boundary |

---

## 6. Resolved design decisions

1. **Field domain = Regular3D Sampled grid, resampled from the unstructured solve.** Forced by the `grids_equal` requirement of every existing field consumer. (§3, §4.1)
2. **Correspondence = resample-to-grid + sample-at-OCCT-surface-vertices.** Prismatic-exact today; arbitrary geometry deferred to the producer half. The "solve-on-realization-mesh vs map-onto-vertices" fork (esc-2962-33) resolves to *both are the same grid intermediate; only the solve mesh differs*, and the solve mesh is `structural-analysis-fea`'s to change. (User-confirmed 2026-05-30.)
3. **This PRD is the result-model seam, not the mesher seam.** Arbitrary-geometry producer-completion (P1 trampoline-consumes-realized-mesh, P2 face-selector BCs, existing 3429) stays owned by `structural-analysis-fea`; 2930's bracket gates on it cross-PRD. (User-confirmed.)
4. **Capability (iv) is mostly pre-existing.** Only the `VonMises`-derived `max`/`argmax` reduction is net-new (β). The original "only scalar `max_von_mises` exists" premise was inaccurate.
5. **2930 stays a bracket with the full field-reduction design loop**, rewritten to honest grammar (`minimize mass(body) where max(von_mises(fea.stress)) < material.yield_stress`; free-fn `face(body,"top")` per GR-040 — both parse, G3 verified), gated on producer-completion. No honest-scalar interim (Leo-ratified in 2930's parked note).
6. **2962 becomes a C-as-integration-gate leaf**: max-von-Mises readout + per-vertex contour + the pure-frontend Lock Current handler, with the contour as the integration gate over α/γ/δ.
7. **Modal Φ (`ModalResult.shape`) is OUT of scope** — the report's §3-C modal twin is owned by task 3823 / a separate Φ-serialization decision, not this PRD.
8. **R3 is split: classifier (4090, done) vs emission/plumbing (R3b, new).** Task 4090 delivered only the typed structs + `fea_structured_detail()` classifier with **zero production callers** — twice blocking ι/2966 (parks 2026-05-30, 2026-06-24) on a dependency-capability gap: the structs never reach the GUI IPC boundary. R3b is the missing emission half (§4.6): a neutral `reify-eval`-owned `StructuredComputeDetail::Fea(_)` wrapper on **both** `ComputeOutcome::{Completed,Failed}` (the `Unconstrained` rigid-body-arrows detail rides Completed-with-**warning**, not Failed) → a serde IPC mirror + `GuiState.fea_diagnostics` field. The neutral kernel enum gains no serde derive (consumer owns IPC serialization, per its header). `UnresolvedSelector` is channel-plumbed but data-deferred (no production `SelectorNoMatch` source until selector-BC emission, P2/4092). Channel-shape (wrapper enum vs raw `FeaDiagnosticDetail` field vs per-diagnostic pairing) and the both-paths finding ratified 2026-06-24 (/unblock 2966 → /prd). ι/2966 re-deps `[2924,2929,2961,4090]` → `[R3b, 2961]`.

---

## 7. Pre-conditions for activating

- **Done & relied upon:** 2920 (interpolation primitives), 2911 (ElasticResult type), 2924 (`@optimized` ComputeNode integration), 2925/2917 (Gmsh mesher), 3426 (stdlib `solve_elastic_static` decl), 3005 (relaxed `solve_load_cases` shape), the multi-case reduction layer (`reify-stdlib/src/fea.rs`).
- **Cross-PRD prerequisites for the gated bracket (2930) only** — owned by `structural-analysis-fea.md`. **Filed + wired + activated at decompose (2026-05-30)** as the complete arbitrary-geometry producer-half / typed-Load-Support consumer chain:
  - **2881 / 2882** (now `pending`) — typed `Load` / `Support` stdlib hierarchy. *G3 caveat:* spec assumes typed `FaceSelector`/`BodySelector` value types, but the codebase models selectors as runtime query fns + tag strings (`topology_selectors.rs`) — re-check the selector value-model before building.
  - **A = 4093** — tighten `solve_elastic_static` signature `List<Real>` → `List<Load>`/`List<Support>` + move Load/Support trait decls earlier in stdlib load order + retire string targets (`TODO(load-trait)`, `solver_elastic.ri:476`). Deps 2881/2882.
  - **P1 = 4091** — `solve_elastic_static_trampoline` consumes the realized `VolumeMesh` (reads `_realization_inputs`; today hardcoded to the synthetic cantilever `extract_tip_force`). Deps α + **3429**.
  - **P2 = 4092** — face-selector BCs: typed `Load`/`Support` → node sets on the realized mesh + general BC assembly (replaces the cantilever-only model). Deps 2881/2882 + **P1**.
  - **C = 4094** — migrate FEA callers/fixtures to the typed form (drop the `List<Real>`/`ConstitutiveLawInput` workarounds). Deps A.
  - **3429** (pending) — realization-op execution edge at `VolumeMesh` dispatch.
  - Full bracket chain: `2881/2882 → A → C` (type/idiom) + `3429 → P1 → P2` (functional), converging on **2930** (`α, β, A, P1, P2`). All `pending`, schedulable in dependency order now that 2881/2882 are activated.
- **No novel substrate for the prismatic slice** — α/β/γ/δ/ε use only existing grammar and existing primitives (G3 fixtures `fea-result-model-1.ri`, `-2.ri` parse with 0 ERROR nodes).

---

## 8. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/structural-analysis-fea.md` | this **consumes** | realized-mesh solve + face-selector BCs — the arbitrary-geometry producer half: 2881/2882 (typed Load/Support) → A=4093 → C=4094, and 3429 → P1=4091 → P2=4092 | **structural-analysis-fea** | filed + wired + activated 2026-05-30 (gates 2930 only) |
| `docs/prds/v0_3/structural-analysis-fea.md` | this **produces** | `ElasticResult.{stress,displacement}` as Sampled fields (α) | **this PRD** | this PRD |
| `docs/prds/v0_3/multi-load-case-fea.md` | this **produces for** | `envelope_*`/`linear_combine` consume the now-real Sampled stress/displacement fields | this PRD populates; multi-load-case consumes | wired (3015/3018 re-homed here) |
| `docs/prds/v0_3/fea-gui-rendering.md` | this **produces for** | `scalar_channels`/`displaced_positions` populated (δ); `register_compute_fns` dispatch (γ) | **this PRD** | this PRD (2962/2966/2968 re-homed here) |
| `docs/prds/v0_3/compute-node-contract.md` | this **consumes** | `@optimized` ComputeNode dispatch (2924) + 3429 realization edge | compute-node-contract | shipped / 3429 pending |

**No new contested-ownership seam introduced.** This PRD explicitly declines ownership of the topology-selectors BC seam (overlay G4 contested pair `topology-selectors ↔ persistent-naming-v2`) — it stays with `structural-analysis-fea`. The mild `structural-analysis-fea ↔ structural-analysis-shells` contradiction is untouched (shells are out of scope).

---

## 9. Decomposition plan

Greek labels are PRD-local; task IDs are assigned/reused at decompose. Re-homed parked tasks keep their IDs with corrected dependencies.

**Phase 1 — Result-model foundation (the seam).**

- **α — Populate `ElasticResult.{stress,displacement}` as Sampled fields.** *Modules:* `reify-eval/src/compute_targets/{elastic_static,buckling}.rs`, `reify-solver-elastic`. *Signal (intermediate → unlocks β,γ,δ,ζ,R1):* e2e test asserts B1+B2 (Sampled fields, correct strides, `max_von_mises` consistency; `solve_elastic_static_e2e.rs` still green). *Prereqs:* 2920, 2924 (done). NEW task.
- **β — Reduce `VonMises`-derived fields in `max`/`min`/`argmax`/`argmin`.** *Modules:* `reify-expr/src/field_reductions.rs`. *Signal (leaf):* a `.ri` fixture where `reify build` evaluates `max(von_mises(stress))` to a non-`Undef` `Scalar<Pressure>` (B3). *Prereqs:* α. NEW task.

**Phase 2 — GUI vertical slice (integration gate).**

- **γ — GUI engine FEA dispatch wiring (cap v / esc-2962-66).** *Modules:* `gui/src-tauri/src/engine.rs` (`Engine::new`, `from_engine`), `commands.rs`, `examples/fea_cantilever_smoke.ri`. *Signal (intermediate → unlocks δ):* B4+B7 (non-`Undef` ElasticResult reaches GUI; `pending_solve_cancel` produced). *Prereqs:* α. NEW task.
- **δ — GUI `ElasticResult`→`scalar_channels`/`displaced_positions` path (cap iii, M-006/M-010).** *Modules:* `gui/src-tauri/src/engine.rs` (replace `:1921-1922`), `types.rs`. *Signal (intermediate → unlocks ε,θ):* B5+B6 channel-population asserts. *Prereqs:* α, γ. NEW task.
- **ε — 2962 (re-homed): max-von-Mises readout + per-vertex contour + Lock Current handler.** *Modules:* `gui/src/viewport/FeaModeToolbar.tsx`, `Viewport.tsx`, stores, `__tests__`. *Signal (LEAF, C-as-integration-gate — names the §5 boundary table):* contour renders + readout shows + Lock Current stores/persists range across a re-solve. *Prereqs:* δ, 2961 (done). Re-dep 2962: `[2920,2924,2961]` → `[δ, 2961]`.

**Phase 3 — Multi-case consumers (re-queued).**

- **R1 — `solve_load_cases` real ComputeNode multi-case lowering + cache-reuse verification (re-home from 3005).** *Modules:* new `reify-eval/src/compute_targets/multi_case.rs`, `compute_targets/mod.rs`, `reify-stdlib`. Make `solve_load_cases` `@optimized`; iterate cases invoking the elastic trampoline; produce a `MultiCaseResult` of real Sampled-field ElasticResults; verify B9 (realization-cache hits once). *Signal (intermediate+leaf):* B9 cache-reuse assert. *Prereqs:* α, 3004 (LoadCase/MultiCaseResult types). NEW task (the explicit re-home target 3005 was relaxed into).
- **ζ — 3015 superposition validation suite.** *Signal (leaf):* B8 (`linear_combine` ≈ direct combined solve within bound), P1+P2 element orders. *Prereqs:* α (real fields), 3011 (linear_combine, done). Re-dep 3015: `[2928,3011]` → `[α, 2928, 3011]`.
- **η — 3018 `multi_load_bracket` example.** *Signal (leaf):* example parses/types/runs; envelope < yield globally. *Prereqs:* R1, α, β. **Gated cross-PRD** on P1/P2 (bracket geometry, like 2930) — author the example over the prismatic multi-case path now; the bracket-geometry variant gates on producer-completion. Re-dep 3018: `[2929,3005,3007]` → `[R1, α, β, 3007]` (+ note bracket-geometry gate).
- **θ — 3026 GUI case-picker dropdown.** *Signal (leaf):* selecting a case re-sources the contour; per-case visual baselines. *Prereqs:* δ, ε, R1, 2961. Re-dep 3026 accordingly (drop the `screenshot_window`-dependent assertions; see κ note).

**Phase 4 — Diagnostics re-homes + overlay.**

- **R2 — per-Support/per-Load source-span provenance (re-home from 2929).** *Modules:* `reify-eval` value model (span on `Value::StructureInstance` for `PointLoad`/`FixedSupport`), ComputeFn-signature span threading into `solve_elastic_static_trampoline`, `reify-stdlib` FEA trampoline. *Signal (leaf):* B10 (diagnostic references the Support's source span; today `None`). *Prereqs:* α/γ (real solve path). NEW task; consumer is 2929's relaxed diagnostic.
- **R3 — typed structured FEA diagnostics (`Vec<DofDirection>`/`Vec<ElementId>`/`UnresolvedSelector`).** *Modules:* `reify-solver-elastic/src/diagnostics.rs`, `reify-eval`. *Signal (intermediate → unlocks ι):* solver emits the typed structs 2966 consumes. *Prereqs:* 2929 (messages+code, pending). **DONE = task 4090** — but delivered only the type defs + `fea_structured_detail()` classifier with zero production callers (the emission half is R3b).
- **R3b — emit structured FEA diagnostics → `ComputeOutcome` → `GuiState` (the missing emission/plumbing half of R3).** *Modules:* `reify-eval/src/engine_compute.rs` (new `StructuredComputeDetail` wrapper enum + `structured_detail` field on `ComputeOutcome::{Completed,Failed}`), `reify-eval/src/compute_targets/elastic_static.rs` (call `failure.structured_detail()` at the `:416` Completed-warning + `:708` Failed sites), `gui/src-tauri/src/types.rs` (serde IPC mirror `FeaDiagnosticInfo` + `GuiState.fea_diagnostics` field), `gui/src-tauri/src/engine.rs` (propagate the channel; failed-solve clears scalar channels, keeps `fea_diagnostics`). **Scope: ~48 files / ~177 mechanical constructor edits** (see §4.6 "Scope reality") — every `ComputeOutcome::{Completed,Failed}` literal gains `structured_detail: vec![]`, plus the `run_compute_dispatch→EvalResult→CheckResult→GuiState` thread; this is NOT a 4-file change. *Signal (intermediate → unlocks ι):* B12 (Rust engine test — `structured_detail` carries `Unconstrained` on the warned Completed + `ProblemElements` on the Failed) **and** B13 (debug-MCP `store_state` — `GuiState.fea_diagnostics` non-empty with the `Unconstrained` payload after an unconstrained-body solve). Wires up the 4090 orphan; `UnresolvedSelector` arm channel-plumbed, data-deferred to P2/4092. *Prereqs:* 4090 (R3 classifier, done), γ/2962-line GUI dispatch wiring (4086, done — `register_compute_fns` reaches the GUI). Contract: §4.6. NEW task.
- **ι — 2966 diagnostic overlay.** *Signal (leaf):* B11 (rigid-body arrows + problem-element outlines render). *Prereqs:* R3b, 2961. Re-dep 2966: `[2924,2929,2961,4090]` → `[R3b, 2961]`.

**Phase 5 — Baselines.**

- **κ — 2968 FEA visual-regression baselines (re-scoped).** *Signal (leaf):* the **cantilever contour + deformed** scene baselines pass (the scenes this PRD enables on prismatic geometry). *Prereqs:* ε. **Scope note:** the pressurised-cylinder/bracket-auto-resolve scenes gate on producer-completion (arbitrary geometry) and on the auto-resolve panel producer (M-015) — split those out; do **not** silently fold them in. Also flag the `screenshot_window` FICTION (M-001/2954) for full-window scenes — viewport WebGL capture is fine for contour/deformed; full-window probe/overlay scenes need the harness fix. Re-dep 2968 to `[ε]` for the prismatic scenes.

**Gated cross-PRD (kept, not shipped in the prismatic batch).**

- **2930 — bracket auto-thickness, minimize-mass, end-to-end (kept a bracket).** Rewrite to honest grammar (decision #5). *Signal (leaf):* `reify build` of the bracket example converges a thickness and the design loop holds. *Prereqs:* α, β, **A=4093, P1=4091, P2=4092** (producer-completion, cross-PRD; P2→P1, A→2881/2882, 3429 transitive via P1). Re-dep 2930 (applied): `[2924,2926,2928,3092]` → `[α(4084), β(4085), A(4093), P1(4091), P2(4092), 2926, 2928, 3092]`.

---

## 10. Out of scope

- The mesher / realized-mesh solve / face-selector BC seam (P1/P2) — owned by `structural-analysis-fea`.
- Modal `ModalResult.shape` (Φ) serialization — separate decision (report §3-C, task 3823).
- Envelope-view rendering, side-by-side DualViewport comparison, multi-case probe popups (v0.4+ per `fea-gui-rendering.md`).
- Deferred derived-source reductions other than `VonMises` (`MaxShear`/`PrincipalStresses`/`Gradient`/…) in `max`/`min` — β covers `VonMises` only; others stay `Undef` until needed.
- The `screenshot_window` full-window-capture harness fix (M-001) — needed only for full-window probe/overlay baselines, not contour/deformed.

---

## 11. Open questions (tactical — decide at impl time)

1. **Resample grid resolution rule.** Default to the solve mesh's `nx/ny/nz`? Or a fixed/he-derived resolution decoupled from solve density? *Suggested:* mirror solve mesh counts for exactness; revisit if GUI sampling looks coarse. Decide in α.
2. **OOB surface-vertex policy.** Sample at a vertex marginally outside the grid (tessellation vs grid-bound rounding): clamp to nearest in-bounds grid cell, or emit NaN? *Suggested:* small `tol` expansion on `locate_element_p1`, NaN beyond. Decide in δ.
3. **Whether β also covers `SafetyFactor`/`PrincipalStresses` reductions** opportunistically (cheap once the VonMises projection pattern exists) or strictly `VonMises`. *Suggested:* `VonMises` only; file a follow-up if a consumer needs the others. Decide in β.
4. **P1/P2 filing.** ~~File P1/P2 as new tasks under `structural-analysis-fea`?~~ **RESOLVED at decompose (2026-05-30):** filed P1=4091, P2=4092, plus A=4093 (signature tightening) and C=4094 (caller migration) — the full typed-Load/Support consumer chain, since 2881/2882 alone were a 4-piece-short orphan. Wired `P2→P1`, `A→2881/2882`, `C→A`, `2930→{α,β,A,P1,P2}`. Activated 2881/2882 → pending so the whole chain is schedulable in dependency order.
