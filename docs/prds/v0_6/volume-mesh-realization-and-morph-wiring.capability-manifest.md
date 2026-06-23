# Capability manifest — volume-mesh-realization-and-morph-wiring

Mechanizes G3 + G6 per leaf (`gates.md` → Capability Manifest). Each leaf's user-observable signal is decomposed into the capabilities it asserts; each capability is bound to evidence. **Every binding PASSES** — all are either a `done`-prereq with a verified production-path site, or this task's own deliverable (upstream of the asserting leaf). No `declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `bound≤floor` / `rejection-absent` binding exists, so nothing blocks the batch.

Verification basis: HEAD `95c714ad80` (2026-06-23), corroborated by direct code reads + two Explore sweeps (engine realization path + Cargo topology). The substrate was verified by hand rather than via `scripts/prd-decompose-verify.mjs` because every asserted capability resolves to a confirmed `done` prereq (with file:line) or the leaf's own deliverable — there is no unproven premise for the workflow to probe.

Empty-value sentinel (reify): `Value::Undef` / `None` / trivial-ctor placeholder. None of these leaves sample a result *field*, so the field-population sub-check does not fire. No leaf asserts a numeric accuracy bound (the ≥10× claim lives on task 2953, dep-wired downstream, not on a leaf of this batch), so the numeric-floor check does not fire. No leaf asserts a rejection, so the rejection-mechanism check does not fire.

---

## Task α — VolumeMesh realization demand + execution (§3.2 foundation)

**Signal:** a CI e2e drives a real `.ri`-compiled body + a registered probe consumer declaring `VolumeMesh` demand; `RealizationReadHandle.volume_mesh()` returns `Some` with `tet_indices.len() % 4 == 0`, `> 0` tets, element-order tag preserved (structural assertions only).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `RealizedContent::VolumeMesh(Arc<VolumeMesh>)` type | `grep:crates/reify-eval/src/engine_compute.rs:124` — variant present (task 4507, done) | PASS (wired) |
| `RealizationReadHandle.volume_mesh()` accessor | `grep:crates/reify-eval/src/engine_compute.rs` (accessor on the handle, task 4507, done) | PASS (wired) |
| VolumeMesh projection arm (handle→`RealizedContent::VolumeMesh`) on the production read path | `grep:crates/reify-eval/src/realization_content.rs:192` (`ReprKind::VolumeMesh => kernel.volume_mesh(...)`, task 4509, done) | PASS (wired) |
| `GeometryKernel::volume_mesh()` + gmsh impl | `producer:task-4509` (done) — gmsh implements it; default impl unsupported-Err | PASS (producer upstream/landed) |
| Gmsh surface-to-volume tet mesher + `ReprKind::VolumeMesh` | `producer:task-2925` (done) | PASS |
| `dispatch_volume_mesh` tet/hex/wedge truth table | `grep:crates/reify-eval/src/engine_build.rs:7698` (`#[allow(dead_code)]`, task 2989 hex-wedge, done) | PASS (exists; α adds the call edge) |
| **VolumeMesh demand computation + `execute_realization_ops`→`dispatch_volume_mesh` call edge** | **this task α's deliverable** (`engine_build.rs`; reuses realization-read-api β `Value::GeometryHandle → realization_inputs` lowering) | PASS (own-deliverable) |

Grammar: no new `.ri` syntax (`grammar_confirmed = true`).

---

## Task β — Mesh-morph producer hook + morph-or-remesh arm

**Signal:** a CI e2e — parametric `.ri`, demand a VolumeMesh realization, tick a non-structural parameter → realization produced by morph (connectivity identical to source, same `tet_indices`); `morph_stats` Debug-MCP RPC / CLI `--verbose` reports `morphed: 1`; a topology-changing tick reports `ineligible`/`remeshed`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| α's VolumeMesh demand+execute substrate | `producer:task-α` (intra-batch, **upstream** of β) | PASS (DAG-direction ✓) |
| `morph_eligible` (Stage A/B eligibility) | `producer:task-2939/2940` (done); `grep:crates/reify-mesh-morph/src/eligibility.rs` | PASS |
| `compute_dirichlet_bcs` (BC list via Projector) | `producer:task-2942` (done); `grep:crates/reify-mesh-morph/src/boundary.rs` | PASS |
| `elasticity_morph` / `laplacian_smooth` / `quality_check` | `producer:task-2943/2944/2945/2946` (done); `grep:crates/reify-mesh-morph/src/lib.rs` (public re-exports) | PASS |
| OCCT `Projector` impl (`BRepExtrema_DistShapeShape`) | `producer:task-3535` (done); `grep:crates/reify-kernel-occt/src/projector_impl.rs` | PASS |
| Gmsh `NodeAttachment`/`BoundaryAssociation` producer | `producer:task-3591` (done, hardened 3763); `grep:crates/reify-kernel-gmsh/src/mesh_boundary.rs` | PASS |
| Morph diagnostic counters + `morph_stats` Debug-MCP RPC + `--verbose` summary | `producer:task-2948/2949` (done) | PASS (the user-observable surface) |
| Cycle-safe boundary types (`reify-types::boundary_attachment`) | `producer:task-3591` (relocated; reify-mesh-morph aliases) | PASS (resolves the Cargo cycle) |
| **`MorphProducer` hook seam + `Engine::register_morph_producer` + morph-or-remesh decision tree** | **this task β's deliverable** (reify-eval seam + reify-mesh-morph impl/registration) | PASS (own-deliverable) |

Numeric: β's signal is structural (`morphed` counter; connectivity identity) — the ≥10× wall-clock claim is **not** a β signal (it lives on task 2953, dep-wired). No floor check fires. Grammar: `grammar_confirmed = true`.

---

## Task δ — Companion prose corrections

**Signal:** `compute-node-contract.md` §6/§8-κ + `mesh-morphing.md` axis-1 note + the `dispatch_volume_mesh` G-allow comment updated to "§3.2 realization-kind dispatch, not §3.4 ComputeNode"; doc lint clean; `rg` for the stale "axis-1 = morph routes through ComputeNode" / 3429 / 2947 engine-wire references returns the corrected pointers.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Target doc sites exist | `grep:docs/prds/v0_3/compute-node-contract.md §6/§8` (axis-1 prose, task κ) ; `grep:docs/prds/v0_3/mesh-morphing.md` (axis-1 note) ; `grep:crates/reify-eval/src/engine_build.rs:7697` (G-allow comment) | PASS (all present) |
| No code capability | doc-only edit | N/A |

Grammar: N/A (`grammar_confirmed = true`).

---

## Decompose-time graph operations (not leaves)

- Rewire `2951 → β` (remove cancelled engine-wire dep 3429); `2952 → β, 4091` (remove 3429); `2953 → β, 4091` (remove 3429); `4091 → α` (remove 3429).
- Cancel `3429` **after** the rewires (rewire-before-cancel) with `reopen_reason` → this PRD.
- `4091`'s FEA-body-param prerequisite is **out of scope** (un-authored separate PRD); it gates only the full FEA user surface, not this batch's in-batch signals.
