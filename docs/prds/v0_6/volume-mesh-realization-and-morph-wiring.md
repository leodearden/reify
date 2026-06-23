# VolumeMesh realization production + mesh-morph engine wiring

**Status:** authored 2026-06-23 (interactive `/prd` session with Leo). Re-homed from task **3429** / esc-3429-13 (the architect declared 3429 unactionable: an undefined realization-producing seam + a missing demand path + a Cargo cycle inside a 2-file task — a multi-crate epic, not a fixable failure). Leo chose "re-home as a human-led /prd epic."

**Shape:** **B + H** (contract + two-way boundary tests). High-stakes: lands the §3.2 realization-kind-dispatch seam for `VolumeMesh`, a cross-PRD FEA consumer, and a multi-crate wiring (reify-eval + reify-mesh-morph + reify-kernel-{occt,gmsh}).

**Owner surface:** `crates/reify-eval` (`engine_build.rs` realization demand + execution, the morph-producer hook seam), `crates/reify-mesh-morph` (the morph-producer trampoline + registration). Consumes the already-landed realization-read-api (`realization_content.rs`, `RealizationReadHandle.volume_mesh()`).

---

## §1 — Goal and user-observable surface (G1)

Two layered capabilities ship:

1. **VolumeMesh realizations become first-class** — a parametric model can *demand* a `ReprKind::VolumeMesh` realization, the engine *executes* it through the §3.2 realization-kind dispatcher (`dispatch_volume_mesh`, today orphaned), and the result is *readable* via the existing realization-read projection (`RealizationReadHandle.volume_mesh()`). This is the foundation the FEA-on-arbitrary-geometry work (task **4091**) has been blocked on, and it also un-orphans the already-landed tet/hex/wedge dispatch arms.

2. **Mesh-morph replaces remesh on topology-preserving parameter changes** — once VolumeMesh realizations are real, the engine, on a re-realization where Stage-A/Stage-B eligibility holds and a most-recent in-memory mesh exists, **morphs instead of remeshing**. Observable through the already-landed morph diagnostics: CLI `--verbose` prints `mesh updates: N morphed, M remeshed, K ineligible`, and the `morph_stats` Debug-MCP RPC (`REIFY_DEBUG=1`) reports the live counter snapshot.

**Named consumers (in-system seam per `engine-integration-norm.md`):**

| Consumer | Seam | What it consumes | Status |
|---|---|---|---|
| Foundation in-batch e2e (this PRD, task α) | §3.2 realization-kind dispatch | a real Gmsh-built `VolumeMesh` realization, read via `volume_mesh()` | in batch |
| Morph in-batch e2e (this PRD, task β) | §3.2 (morph arm) | a morphed `VolumeMesh` realization + the morph diagnostic counter | in batch |
| **Task 4091** (structural-analysis-fea P1) | §3.2 → FEA ComputeNode | realized `VolumeMesh` for the elastic solve, replacing the synthetic Freudenthal box | **pending; rewired onto this batch (was gated on 3429)** |
| Tasks 2951 / 2952 / 2953 (mesh-morph validation) | §3.2 (morph arm) | the morph path, for chain-degradation / warm-start / slider-benchmark assertions | **pending; rewired onto task β (were gated on 3429)** |

The full *user-level* FEA surface (FEA solving on arbitrary realized geometry; the ≥10× slider benchmark; warm-start preservation) arrives when 4091 lands — its dep edge onto this batch is wired at decompose so the chain cannot strand. **4091 itself presupposes FEA gaining a body/geometry parameter, which is a separate un-authored prerequisite** (see §5 / §7). This PRD does not block on it: the in-batch signals exercise the production demand→execute→read path with a registered probe consumer (the realization-read-api η-leaf 4513 precedent), so the substrate is proven here even before FEA grows a body argument.

---

## §2 — Background

`docs/prds/v0_3/mesh-morphing.md` and its phase-2 sibling decomposed and **fully built the morph algorithm** — `reify-mesh-morph` ships `morph_eligible`, `compute_dirichlet_bcs`, `elasticity_morph`, `laplacian_smooth`, `quality_check` (tasks 2939–2946, done), the OCCT `Projector` impl (task 3535, done), the Gmsh `BoundaryAssociation`/`NodeAttachment` producer (tasks 3591/3763, done), quality calibration (2950, done), and the diagnostic counters + Debug-MCP RPC (2948/2949, done). **None of it is reachable from the engine.** The crate has been a producer-orphan for over a month — the canonical cluster-C-14 "library callable in isolation, no engine consumer" failure.

Two engine-wiring attempts both dead-ended:

- **Task 2947** ("VolumeMesh realization wiring — morph-or-remesh") was **cancelled** (2026-05-30): it specified `reify-eval`'s `engine_build.rs` *calling into* `reify-mesh-morph`, which is a Cargo cycle (`reify-mesh-morph` already depends on `reify-eval`). Marked superseded by 3429.
- **Task 3429** (CN-contract §8 task κ, "mesh-morph engine wiring via ComputeNode at VolumeMesh realization dispatch") was found **unactionable / blocked**: it assumed a *realization-producing ComputeNode* — a trampoline depositing a `RealizedContent::VolumeMesh` — for which `compute-node-contract.md` §4 explicitly left the realization-I/O seam OPEN. Plus a missing demand path and the same Cargo cycle, inside a declared 2-file scope.

**The substrate has since moved decisively** (verified at HEAD `95c714ad80`, 2026-06-23):

- The **realization-read-api** PRD (`docs/prds/v0_6/realization-read-api.md`, tasks 4507–4513, done) built the realization **read** side: `RealizedContent {Sdf, SurfaceMesh, VolumeMesh(Arc<VolumeMesh>)}` and `RealizationReadHandle.volume_mesh()` (`engine_compute.rs`), the Engine-owned `RealizationProjectionStore` (`realization_content.rs`), and `GeometryKernel::volume_mesh()` (gmsh impl, task 4509). Crucially it made **`ComputeFn` purity a HARD invariant** (§3.2-1: "The `ComputeFn` signature does not change"): trampolines *read* realizations, they never *produce* them. `ComputeOutcome` has only `Completed { result: Value }` / `Cancelled` / `Failed` — no realization-output variant, by deliberate design.
- `engine-integration-norm.md` §7 (the canonical mesh-morph worked example) already classifies the morph branch as **§3.2 realization-kind dispatch** — a morph arm in `dispatch_volume_mesh`, called from `execute_realization_ops`, producing a `VolumeMesh` realization the same way the Gmsh tet/hex/wedge arms do.
- The boundary types were already relocated to `reify-types::boundary_attachment` (task 3591) so adapter crates emit them without a transitive `reify-eval` dep.

So the brief's "realization-producing ComputeNode" framing is now **the wrong architecture** — it would re-open a seam the project deliberately closed by making `ComputeFn` pure. This PRD adopts the §3.2 framing instead (see §3 D1).

---

## §3 — Resolved design decisions

### D1 — Morph is a §3.2 realization-op producer, NOT a §3.4 realization-producing ComputeNode *(confirmed by Leo, 2026-06-23)*

The morph runs as a **realization-kind-dispatch arm** for `ReprKind::VolumeMesh` inside the realization execution path (`execute_realization_ops → dispatch_volume_mesh`), producing a `KernelHandle`/`VolumeMesh` realization exactly like the Gmsh mesher arm. It does **not** route through a ComputeNode, and **no `ComputeOutcome::Realized` variant is added** — `ComputeFn` purity (realization-read-api §3.2-1) is preserved.

The cache / warm-state benefits the brief's §3.4 wrap was meant to provide are already delivered by the existing realization machinery: the realization cache (input-hash keyed) memoizes realizations, and the `RealizationProjectionStore` memoizes the readable content. The "FEA warm-start preserved across a morph" property (task 2952) is a consequence of *element-connectivity preservation* (a property of the morph algorithm) and lives on the FEA solve ComputeNode's `OpaqueState` — independent of whether morph is §3.2 or §3.4.

**Superseded prose** (companion correction task δ): `compute-node-contract.md` §6 "axis-1 = morph routes through ComputeNode" and §8 task κ's "ComputeNode-wrapped morph" are corrected to "§3.2 realization-kind dispatch"; the `dispatch_volume_mesh` G-allow comment and the `mesh-morphing.md` axis-1 note are repointed at this PRD. (Per `feedback_breadcrumb_design_alternatives_at_impl_site`: the §3.4 alternative and the reason it was rejected are recorded at the seam.)

### D2 — Split into a §3.2 foundation + the morph arm

The real, long-standing gap is that **nothing demands or executes a `VolumeMesh` realization in production** — `demanded_reprs_for_template` yields only `{BRep, Mesh}`, and `execute_realization_ops` has no call edge to `dispatch_volume_mesh` (so even the landed tet/hex/wedge arms are orphaned). This is logically prior to, and independent of, morph. The epic therefore decomposes as:

- **Phase 1 (task α):** make `VolumeMesh` a demandable + executable + readable realization kind (the §3.2 foundation). Unblocks FEA-on-real-geometry (4091).
- **Phase 2 (task β):** add the morph-or-remesh arm on top.

This ordering means the foundation has independent value (FEA-on-arbitrary-geometry) even if the morph optimization were never built.

### D3 — Cargo cycle resolved by a reify-mesh-morph-hosted registration hook

`reify-eval` must not depend on `reify-mesh-morph` (cycle). The morph-producer is invoked through a **hook seam** owned by `reify-eval` — a registered function pointer / `dyn` trait object (`MorphProducer`) stored on the `Engine`, called from the VolumeMesh dispatch path when present. The **implementation + registration live in `reify-mesh-morph`** (`pub fn register_morph_producer(engine: &mut reify_eval::Engine)`), mirroring how `reify_eval::compute_targets::register_compute_fns` is called at Engine construction from the binary/test setup. `reify-mesh-morph` already depends on `reify-eval`, so it can name `Engine`, `RealizedContent`, etc. directly. Boundary types (`NodeAttachment`, `BoundaryAssociation`) are already in `reify-types::boundary_attachment` (task 3591). No new Cargo edge into `reify-eval`.

### D4 — In-batch demand trigger: a registered VolumeMesh-demanding consumer, exercised by a production e2e (not a new exporter, not new grammar)

Today the only candidate production demanders each require out-of-scope work: FEA needs a body param (un-authored PRD), an export sink needs a volume-mesh serializer (owned by `io-export-import-completion.md`; `ExportFormat = {Step, Stl, Obj, ThreeMF}` today — no volume-mesh format), and a stdlib `volume_mesh(body)` op needs grammar/parser/lowering. **This PRD takes none of those.** Instead task α adds the *mechanism* for a consumer to declare `ReprKind::VolumeMesh` demand for a geometry input (extending the demand computation + the realization-read-api `Value::GeometryHandle → realization_inputs` lowering), and the in-batch e2e exercises the full production demand→execute→read path with a **test-registered probe consumer** that declares VolumeMesh demand — exactly the realization-read-api η-leaf (4513) pattern (real `.ri`-compiled body, real Gmsh execution, real projection; only the final consumer is a probe). The real production consumer is FEA (4091), dep-wired.

### D5 — FEA-on-real-geometry (4091) is the strategic consumer, dep-wired; the FEA body-param is an acknowledged separate prerequisite

This PRD owns the §3.2 `VolumeMesh` substrate and the morph arm. 4091 (FEA P1 — "elastic solve consumes realized VolumeMesh") is rewired off the now-cancelled 3429 onto task α. 4091 in turn presupposes FEA's authoring surface gaining a body/geometry parameter (today `solve_elastic_static(material, length, width, height, loads, supports, options)` is all scalars; `typed-fea-authoring-surface.md` does not add it). That body-param work is **out of scope** here and named as a cross-PRD gate (§7) — it gates only the *full FEA user surface*, not this PRD's in-batch signals.

### D6 — Morph result is in-memory-realization-cache only, never persistent *(inherited from `mesh-morphing.md`)*

The morph is path-dependent; the persistent cache key is path-independent. Morph results populate the in-memory realization cache only. The morph-source policy ("from-most-recent-in-memory only; the quality check is the sole chain-degradation safeguard") is inherited unchanged from the parent PRD.

---

## §4 — Contract (H)

### 4.1 The §3.2 VolumeMesh realization-kind dispatch seam

- **Demand.** `demanded_reprs_for_template` (`engine_build.rs`) gains the ability to yield `ReprKind::VolumeMesh` for a realization whose consumer declares VolumeMesh demand. A geometry-handle argument of a VolumeMesh-demanding consumer (routed into `realization_inputs` by the realization-read-api β lowering) propagates `VolumeMesh` demand to its producing realization. (Exact propagation rule — consumer-op marker vs. per-input demanded-repr field — is tactical, §10 OQ-1.)
- **Execution call edge.** `execute_realization_ops` gains a call edge to `dispatch_volume_mesh` when an op's demanded output repr is `VolumeMesh`. This lights up the existing 8-arm tet/hex/wedge truth table (`dispatch_volume_mesh`, currently `#[allow(dead_code)]`) and provides the slot for the morph arm. The produced realization is written so the realization-read projection (`realization_content.rs`, `volume_mesh()` arm) can read it — no change to the read side.
- **Conversion reach.** Gmsh advertises `(Convert { from: Mesh }, VolumeMesh)`; the dispatcher's existing conversion machinery reaches `VolumeMesh` from a `BRep→Mesh→VolumeMesh` chain once `VolumeMesh` is demanded.

### 4.2 The morph-producer hook (`reify-eval`-owned seam; `reify-mesh-morph`-impl)

```rust
// reify-eval (e.g. engine_compute.rs / engine_admin.rs) — the seam
pub trait MorphProducer: Send + Sync {
    /// Attempt to morph `source` (the most-recent in-memory VolumeMesh for this
    /// realization) toward `new_brep`. Returns the morphed mesh, or a structured
    /// reason on ineligibility / quality-reject so the caller falls back to remesh.
    fn try_morph(&self, ctx: MorphRequest<'_>) -> MorphResult;
}
impl Engine {
    pub fn register_morph_producer(&mut self, p: Box<dyn MorphProducer>);
    pub(crate) fn morph_producer(&self) -> Option<&dyn MorphProducer>;
}
```

```rust
// reify-mesh-morph — the impl + registration
pub fn register_morph_producer(engine: &mut reify_eval::Engine) { /* installs the impl */ }
```

`MorphRequest` carries the source `VolumeMesh`, old/new `BRep` handles, the OCCT `Projector` (from the engine's kernel), and the Gmsh `BoundaryAssociation` for the source mesh. The impl composes the landed primitives: `morph_eligible` → `compute_dirichlet_bcs` (via the OCCT `Projector`) → `laplacian_smooth` or `elasticity_morph` (by displacement-magnitude rule) → `quality_check`, recording the diagnostic counters on every outcome.

### 4.3 Morph-or-remesh decision tree (at the VolumeMesh dispatch point)

On a VolumeMesh-demanded realization:
1. If no `morph_producer` is registered, or no most-recent in-memory mesh exists for this realization → **remesh** (Gmsh tet/hex/wedge via `dispatch_volume_mesh`).
2. Else call `try_morph`. On `Ok(mesh)` (eligibility + quality pass) → use the morphed mesh; record `morphed`. On any ineligibility / quality-reject / solver error → record the matching counter, log info-level on quality-reject ("why was that slider tick slow?"), and **remesh**.

### 4.4 Invariants

1. **Purity preserved.** No `ComputeOutcome` variant is added; trampolines remain pure. The morph hook is a realization-execution-path callback, not a ComputeNode.
2. **No cycle.** `reify-eval` gains no dependency on `reify-mesh-morph`; the hook is installed at construction from the algorithm crate.
3. **Honest fallback.** Every non-success morph outcome falls back to a real Gmsh remesh; the morph is never the sole producer of a realization that could otherwise be meshed.
4. **Read side unchanged.** The realization-read projection (`volume_mesh()`) and `ComputeFn` signature are untouched.

---

## §5 — Pre-conditions for activation

| Pre-condition | State | Bearing |
|---|---|---|
| Morph algorithm (`morph_eligible`, `elasticity_morph`, `laplacian_smooth`, `quality_check`, `compute_dirichlet_bcs`) | done (2939–2946) | β composes them |
| OCCT `Projector` impl (`BRepExtrema_DistShapeShape`) | done (3535) | β's BC projection |
| Gmsh `NodeAttachment`/`BoundaryAssociation` producer | done (3591, hardened 3763) | β's source-mesh boundary |
| Morph diagnostic counters + `morph_stats` Debug-MCP RPC | done (2948/2949) | β's user-observable |
| `ReprKind::VolumeMesh` + Gmsh tet mesher + input-hash cache | done (2925) | α's execution |
| `dispatch_volume_mesh` 8-arm truth table | done (hex-wedge 2989) | α's execution (call edge added) |
| realization-read-api: `RealizedContent::VolumeMesh`, `RealizationReadHandle.volume_mesh()`, projection store, `GeometryKernel::volume_mesh()` | done (4507–4509) | α's read-back; no change to it |
| `reify-types::boundary_attachment` (cycle-safe boundary types) | done (3591) | D3 |
| **FEA gains a body/geometry parameter** | **not authored** | gates the *full FEA user surface* (4091); NOT this PRD's in-batch signals |

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/compute-node-contract.md` | corrects | §6 axis-1 / §8 task κ prose ("morph is §3.2, not a §3.4 ComputeNode") | **this PRD** (task δ) | queued |
| `docs/prds/v0_3/engine-integration-norm.md` | instance-owns | §3.2 `VolumeMesh` dispatcher call edge + morph arm (norm §7 says "this PRD owns the seam shape; the dispatcher implementer owns the instance") | **this PRD** | queued |
| `docs/prds/v0_6/realization-read-api.md` | consumes | `RealizationReadHandle.volume_mesh()` / projection store (read side) | realization-read-api (done) | wired |
| `docs/prds/v0_3/structural-analysis-fea.md` (task 4091) | produces-for | realized `VolumeMesh` for the elastic solve | this PRD (substrate); 4091 (consumer) | dep edge 4091→α wired at decompose |
| FEA body/geometry authoring param | gated-by | `solve_elastic_static` gaining a body arg | **un-authored — separate PRD** (typed-fea axis) | named gate; not in this batch |
| `docs/prds/v0_6/io-export-import-completion.md` | not-taken | volume-mesh export format (`.msh`/`.vtk`) explicitly NOT this PRD's demander | io-export PRD | out of scope |
| `docs/prds/v0_3/mesh-morphing.md` + `mesh-morphing-phase-2.md` | completes | the engine-wire their algorithm/validation tasks were stranded behind | this PRD | 2951/2952/2953 rewired onto β; 4091 onto α; 3429 cancelled |

No reciprocal-ownership ambiguity: the §3.2 seam has no upstream owner (norm §3.2), and this PRD is the dispatcher-instance owner. The §3.4 alternative is explicitly declined (D1).

---

## §7 — Boundary-test sketch (H; two-way)

**Producer side (the engine produces a VolumeMesh realization):**

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Demand → execute → read | A `.ri`-compiled body; a registered probe consumer declaring `VolumeMesh` demand on its geometry arg. | `execute_realization_ops` calls `dispatch_volume_mesh`; a Gmsh tet mesh is produced; `RealizationReadHandle.volume_mesh()` returns `Some` with `tet_indices.len() % 4 == 0`, `> 0` tets, element-order tag preserved. (Structural assertions only — no numeric-accuracy bound.) |
| Morph arm — eligible tick | A registered `MorphProducer`; a prior in-memory mesh; a non-structural parameter tick (Stage-A + Stage-B eligible). | The realization is produced by morph (connectivity identical to source: same `tet_indices`); `morph_stats` reports `morphed += 1`; `--verbose` shows `morphed`. |
| Morph arm — ineligible / quality-reject tick | Structural change, or a deformation that fails the quality check. | Falls back to a Gmsh remesh; `morph_stats` reports `ineligible` / `remeshed`; info-level log on quality-reject; the produced realization is still valid. |
| No producer registered | Engine without `register_morph_producer`. | VolumeMesh demand still produces a Gmsh remesh (foundation works standalone). |

**Consumer side (downstream reads the produced realization):**

| Scenario | Preconditions | Postconditions |
|---|---|---|
| FEA reads realized mesh (dep-wired) | 4091 landed + FEA body param. | `solve_elastic_static` solves on the realized tet mesh (node count matches the realization, not the synthetic `nx×1×6` box). *(Out of this batch; the dep edge guarantees the chain.)* |
| Warm-start preserved across morph | Tasks 2952/2953 (rewired onto β) + FEA chain. | Element-to-DOF mapping survives the morph; per-tick CG iterations materially lower; ≥10× slider wall-clock at 100K elements. *(Dep-wired; gated on the FEA surface.)* |

The morph in-batch signal (counter + connectivity) does **not** depend on FEA; the FEA-specific validations are the dep-wired downstream surface.

---

## §8 — Decomposition plan

Three new tasks; plus dep-rewiring of existing tasks and the cancellation of 3429 (done as decompose-time graph operations, not new tasks).

### Task α — VolumeMesh realization demand + execution (§3.2 foundation)
- Extend the demand computation so a consumer can declare `ReprKind::VolumeMesh` demand on a geometry input; add the `execute_realization_ops → dispatch_volume_mesh` call edge for VolumeMesh-demanded ops; ensure the produced realization is written where the realization-read projection reads it.
- **User-observable signal:** a CI e2e (`crates/reify-eval/tests/…`) drives a real `.ri`-compiled body + a registered probe consumer declaring VolumeMesh demand; asserts `RealizationReadHandle.volume_mesh()` returns `Some` with `tet_indices.len() % 4 == 0`, `> 0` tets, element-order tag preserved (realization-read-api η-leaf shape). No numeric accuracy bound.
- **Consumer:** task β (morph arm), task 4091 (FEA, dep-wired).
- **Modules:** `crates/reify-eval` (`engine_build.rs`). `grammar_confirmed = true` (no new `.ri` syntax).
- **Prereqs:** none new (all substrate done).

### Task β — Mesh-morph producer hook + morph-or-remesh arm
- Define the `MorphProducer` hook seam + `Engine::register_morph_producer` in `reify-eval`; implement + register it from `reify-mesh-morph` (`register_morph_producer`), composing the landed morph primitives with the OCCT `Projector` and the Gmsh `BoundaryAssociation`; add the morph-or-remesh decision tree at the VolumeMesh dispatch point.
- **User-observable signal:** a CI e2e — parametric `.ri`, demand a VolumeMesh realization, tick a non-structural parameter → the realization is produced by morph (connectivity identical to source), and the `morph_stats` Debug-MCP RPC / CLI `--verbose` reports `morphed: 1`; a topology-changing tick reports `ineligible`/`remeshed`.
- **Consumer:** tasks 2951/2952/2953 (validation, dep-wired); the morph counter surface (CLI/Debug-MCP).
- **Modules:** `crates/reify-eval`, `crates/reify-mesh-morph`. `grammar_confirmed = true`.
- **Prereqs:** α; (2939–2946, 3535, 3591, 2948/2949 — all done).

### Task δ — Companion prose corrections
- Correct `compute-node-contract.md` §6 (axis-1) and §8 task κ to "§3.2 realization-kind dispatch, not a §3.4 ComputeNode"; repoint the `dispatch_volume_mesh` G-allow comment and `mesh-morphing.md`'s axis-1 note at this PRD; record the rejected §3.4 alternative + rationale at the seam.
- **User-observable signal:** docs updated; doc lint clean; `rg` for the stale "axis-1 = morph routes through ComputeNode" / 3429 / 2947 engine-wire references returns the corrected pointers.
- **Modules:** docs only. Independent (no code dep). `grammar_confirmed = true`.

### Decompose-time graph operations (not new tasks)
- **Rewire** 2951 → β; 2952 → β + 4091; 2953 → β + 4091 (off 3429). **Rewire** 4091 → α (off 3429).
- **Cancel** 3429 *after* the rewires (rewire-before-cancel) with `reopen_reason` pointing at this PRD.

### DAG
```
α (VolumeMesh demand + execute) ──▶ β (morph hook + morph-or-remesh arm)
α ──▶ 4091 (FEA consumes realized VolumeMesh; rewired off 3429; gated also on FEA body-param PRD)
β ──▶ 2951 (chain-degradation)
β ──▶ 2952 (warm-start regression)   ─┐ also depend on 4091 (FEA surface)
β ──▶ 2953 (slider ≥10× benchmark)   ─┘
δ (prose corrections) — independent
```

---

## §9 — Out of scope

- **Volume-mesh export format** (`.msh`/`.vtk`) — owned by `io-export-import-completion.md`. Not this PRD's demander (D4).
- **FEA body/geometry authoring parameter** — separate un-authored PRD (typed-fea axis); gates only the full FEA surface (D5/§7).
- **The morph algorithm itself** (Laplacian, elasticity, stiffness, quality, calibration) — done in the parent PRDs; inherited unchanged.
- **Persistent caching of morph results** — never (D6).
- **Nearest-cached morph for cross-session cold start** — deferred to the parent PRD's v0.4 follow-on (`mesh-morph-nearest-cached.md`).
- **New `.ri` surface syntax for volume meshing** — not introduced; demand is a consumer-declaration mechanism, not a language surface.

---

## §10 — Open questions (tactical; decided at implementation time)

1. **Demand-propagation rule.** Whether VolumeMesh demand is declared via a consumer-op marker, a per-`realization_input` demanded-repr field, or a small extension to `demanded_reprs_for_template`'s consumer-acceptance logic. All reach the same place; the architect picks the lightest at α's implementation, reusing the realization-read-api `Value::GeometryHandle → realization_inputs` lowering.
2. **`MorphProducer` request shape.** Whether `MorphRequest` borrows the source mesh / projector / boundary association or takes owned/`Arc` copies (the engine holds the kernel for the realization lifetime, which exceeds the morph call — mirrors mesh-morphing-phase-2 §9 Q-9-3). Decided at β.
3. **Most-recent-in-memory-mesh lookup.** Exactly how the dispatch point locates the prior in-memory `VolumeMesh` for a realization (realization-cache probe keyed on the realization node identity). Tactical to β; the realization cache already memoizes by input hash.
4. **Splitting β by crate.** β spans `reify-eval` (hook seam) and `reify-mesh-morph` (impl); if lock contention warrants, the architect may split the seam definition from the impl, keeping the e2e on the impl side as the integration gate. Either way the vertical slice (hook + impl + e2e) lands together.
