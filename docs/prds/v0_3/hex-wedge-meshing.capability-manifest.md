# Capability manifest — hex-wedge-meshing Addendum (2026-07-04)

Mechanizes G3 + G6 for the addendum's 2-task batch (see `hex-wedge-meshing.md` → "Addendum (2026-07-04)"). One block per leaf; intermediate task 4986 is listed for its provided-substrate bindings. Any FAIL binding blocks the batch until resolved — **none FAIL** (see verdict).

Batch:
- **4986** (intermediate) — hex/wedge storage + read-back substrate (contract C-1/C-2/C-3). Deps: 4091, 4509, 4743 (all done). Unlocks 4746.
- **4746** (leaf, re-scoped) — production-edge element-type selection (contract C-4/C-5). Deps: 4091, **4986**.

## Verification method (transparency note)

Bindings are hand-derived from a three-agent in-code investigation (2026-07-04) with file:line citations, not from the deterministic `scripts/prd-decompose-verify.mjs` α-probe. Rationale: (a) this is a 2-task **corrective** re-decomposition, not a fresh multi-task batch; (b) the leaf's novel storage/projection/consume premises are deliberately **provided by upstream task 4986 filed in the same batch** — an α-probe against *main* would report them `producer-absent` (the expected `producer:task-4986` case), not a true FAIL; (c) the on-main capabilities (swept_kind_table wiring, the live `dispatch_volume_mesh` tet caller, `realization_indices_where`) were grep-confirmed with citations during the investigation. If a full D3 adversarial pass is wanted, it can be run against the branch once 4986 lands.

## 4986 (intermediate) — provides substrate for 4746

| Capability asserted | Binding | Verdict |
|---|---|---|
| `SweptMesh3d` / `SweptConnectivity::{Hex,Wedge}` source exists | `grep:crates/reify-solver-elastic/src/sweep.rs:184` (struct) + `:200` (enum) | PASS (wired-on-main) |
| `reify_ir::VolumeMesh` is the read-back type to extend | `grep:crates/reify-ir/src/geometry.rs:2861` (tet-only today; this task adds `VolumeConnectivity`) | PASS (extend-in-place; no new `ReprKind`) |
| `volume_mesh()` projection to extend | `grep:crates/reify-eval/src/realization_content.rs:192` (tet arm, from task 4509) | PASS (wired-on-main) |
| P1 hex / P1 wedge stiffness assembly exists to route to | Phase-A tasks 2983–2986 (`done`) — `producer:task-2983..2986` upstream | PASS (producer upstream, done) |
| Elastic consumer of realized VolumeMesh to extend | `producer:task-4091` (`done`) — solver consumes realized VolumeMesh | PASS (producer upstream, done) |

DAG-direction: all producers upstream or self. Field-population: N/A (this task *is* the producer of hex/wedge connectivity; the boundary rows P2/C1/C2 assert a real sampleable mesh assembles+solves, not a sentinel). Grammar: no novel syntax. Numeric floor: N/A.

## 4746 (leaf) — E1/E2/E3 (addendum boundary rows)

| # | Capability the leaf signal asserts | Binding | Verdict |
|---|---|---|---|
| 1 | Hex/wedge storable in `VolumeMesh` (`VolumeConnectivity`) | `producer:task-4986` — **upstream** dep; deliverable covers exactly C-1 | PASS |
| 2 | `volume_mesh()` projection emits Hex/Wedge `RealizedContent::VolumeMesh` | `producer:task-4986` — upstream; covers C-2 | PASS |
| 3 | Elastic solver assembles a realized hex/wedge VolumeMesh | `producer:task-4986` — upstream; covers C-3 | PASS |
| 4 | `swept_kind_table` populated + readable at the production edge | `grep:crates/reify-eval/src/engine_build.rs:7216` (`.record`) + `Engine::swept_kind_table()` accessor | PASS (wired-on-main) |
| 5 | `dispatch_volume_mesh` gate w/ hex/wedge truth-table arms + live caller edge | `grep:crates/reify-eval/src/engine_build.rs:10200` (defn) + `:7598` (live tet caller, task 4743); this task adds the swept-path args | PASS (wired-on-main; self extends) |
| 6 | `sweep_2d_mesh_to_3d` swept-mesh producer | `grep:crates/reify-solver-elastic/src/sweep.rs:359` — exists (zero non-test callers today); **`producer:task-4746`** gives it its first live caller (the orphan-allowlist removal is the done-signal) | PASS (self wires) |
| 7 | Demand carries `ElementTypePref` via static read of `force_tet`/`require_hex_wedge` | `producer:task-4746` (self). Substrate it reads: `realization_indices_where` `grep:crates/reify-eval/src/engine_build.rs:2021` (wired) + `force_tet`/`require_hex_wedge` params `grep:crates/reify-compiler/stdlib/solver_elastic.ri:295-296` (exist). Exact nested-ctor `CompiledExpr` traversal is an impl-time probe (Open Q1) with a defined fallback → not a premise risk | PASS (self; substrate on-main; fallback-guarded) |
| 8 | **Rejection** (G6 branch 4): `require_hex_wedge=true` on a non-swept body → hard error | `producer:task-4746` (builds it) + `rejection-check:E2` — the leaf RED test authors a non-swept body with `require_hex_wedge=true` and **observes the hard error fires**. Not declared-only: observation is the done-condition | PASS (build-and-observe) |

**G6 branch 3 (end-to-end capability, E1):** every capability the "swept body realizes hex/wedge" signal needs (1–7) traces to this task or an **upstream** prerequisite (4986/4091 upstream; 4/5 on-main; 6/7 self) — none owned by a task that depends on 4746. This is exactly the fix for the original block: the previously-unreachable premise now has its substrate upstream.
**G6 branch 1/2 (numeric/exactness):** none — E1/E2/E3 assert connectivity-kind + diagnostic + error, no numeric bound. (The convergence-vs-DOF numeric claim is PRD task 12, `done`, out of this batch.)

## Verdict

**No FAIL bindings — batch is clear to activate.** The single residual uncertainty (binding 7's exact compiled-expr traversal) is fallback-guarded (`HexPreferred`+diagnostic), so it cannot make the leaf unachievable; it is filed as tactical Open Question 1, to be probed via `reify eval` before any RED test freezes an expr-shape assumption.
