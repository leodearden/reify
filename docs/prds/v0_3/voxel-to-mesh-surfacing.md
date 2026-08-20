# PRD — `build()`-reachable OpenVDB Voxel→Mesh surfacing (`isosurface`)

**Status:** deferred · **Milestone:** v0.3 · **Authored:** 2026-07-05
**Type:** contract PRD completing `multi-kernel-phase-3.md` §8 **task ι**; supersedes the
premise-invalid, cancelled **task 4816**.
**Approach:** B + H (contract + two-way boundary tests) — touches the multi-kernel and
grammar/compiler load-bearing seams across ≥ 4 crates.

---

## 1. Goal

A `.ri` `build()` can surface a Voxel-realized geometry back to a triangle Mesh via OpenVDB
marching cubes, and a user observes the result end-to-end:

```ri
// examples/multi_kernel/voxel_to_mesh.ri
structure VoxelToMesh {
    param size : Length = 20mm
    let solid = box(size, size, size)
    let shell = isosurface(solid)          // voxelize `solid`, surface it back to a Mesh
}
```

```
$ reify build examples/multi_kernel/voxel_to_mesh.ri /tmp/shell.stl
…
Triangles: 936            # a positive triangle count, printed by the CLI
Wrote /tmp/shell.stl
```

and viewport-debug-MCP `mesh_stats` reports `vertex_count > 0` for the surfaced body.

The load-bearing property: the terminal Mesh is produced by **marching cubes on a Voxel grid**
(`realize_mesh_from_voxel_with_options`, #3440), *not* by direct `BRep→Mesh` tessellation and
*not* by the `realize_solid_sdf` `BRep→Mesh→Voxel→SampledField` recipe (which terminates in a
`SampledField` and would prove the wrong thing).

## 2. Background — why this exists

The Voxel→Mesh marching-cubes **capability is DONE and unit-tested** (task **#3440**):
`OpenVdbKernel::realize_mesh_from_voxel_with_options(handle, &MarchingCubesOptions)` at
`crates/reify-kernel-openvdb/src/kernel_real.rs:336` (calls `volume_to_mesh_ffi`), and the
`(Convert { from: Voxel }, Mesh)` **planning** entry in the descriptor at `register.rs:143`.

But the primitive is reachable **only** from tests that hand-seed `available = {Voxel}`
(`crates/reify-kernel-openvdb/tests/dispatcher_integration.rs:315-348`). **No real `.ri` `build()`
can reach it** — three independently-fatal gaps sit between a `build()` and the primitive. Task
**4816** claimed a 3-file "single-row extension" would close them and was **cancelled as
premise-invalid on 2026-07-05** (full engine-graph trace: escalations `esc-4816-3` L1 block report,
`esc-4816-6` L2 decision). The three gaps:

1. **No Voxel demand.** `compute_demanded_reprs` (`crates/reify-eval/src/engine_build.rs:2270-2302`)
   maps terminal sinks over `ExportFormat` (`Stl|Obj|ThreeMF ⇒ Mesh`, `Step ⇒ BRep`, plus a
   VolumeMesh override) and the reverse-pass only ever picks `Mesh` or `BRep` — **never
   `ReprKind::Voxel`**. Nothing in a `build()` graph can ask for a Voxel realization, so the
   marching-cubes stage has no Voxel to consume.

2. **No `(Voxel,Mesh)` conversion arm.** `v03_conversion_projection`
   (`crates/reify-eval/src/dispatcher.rs:613-622`) classifies only `(BRep,Mesh)=Tessellate` and
   `(Mesh,Voxel)=Voxelize`. A planned `(openvdb, Voxel, Mesh)` stage hits the `None` branch and
   degrades to the `"not executable in v0.3-β"` diagnostic (`engine_build.rs:6634-6641`).

3. **No DSL surfacing op** — *the design crux.* The compiler never emits `Operation::Convert`
   (`engine_build.rs:1708-1714`) and no `surface`/`isosurface`/`marching_cubes`/`contour`
   construct exists, so a `.ri` cannot even *express* "voxelize this and surface it back."

This PRD owns all three gaps as one vertical slice. It is the substrate `multi-kernel-phase-3.md`
§8 **task ι** (~line 504) and its downstream **task ρ** (§8, long-chain diagnostic — lists ι as a
prereq "for the multi-stage chain to be reachable in production") actually require.

## 3. Sketch of approach

`isosurface(g)` is a new geometry builtin. Its operand `g` is realized as a **Voxel grid**, and
the op produces a **Mesh** by marching-cubes surfacing that grid. Wiring, gap-by-gap:

- **Gap 3 (DSL + IR + compiler).** Add `Operation::Surface` and `GeometryOp::Surface { grid,
  options }` (mirror functions `geometry_op_to_operation`, `classify_op_input_reprs`,
  `op_accepts_repr`, `parent_handles_for_op` updated together — the strum-completeness test forces
  this). The compiler recognizes the `isosurface(...)` builtin (grammar unchanged — it parses as a
  function call today, verified `tree-sitter parse --quiet`, 0 ERROR nodes) and lowers it to
  `GeometryOp::Surface`. `classify_op_input_reprs(Surface) = [Voxel]`.

- **Gap 1 (demand-seeding).** Extend the `compute_demanded_reprs` reverse-pass so a realization
  consumed by a **Voxel-only-input** op (`Surface`) is demanded `ReprKind::Voxel` — the first
  production path to demand a Voxel realization from a `build()`. The operand's Voxel realization is
  produced by the **existing** `BRep→Mesh→Voxel` chain (`Tessellate` #3438-era + `Voxelize` #4422,
  both DONE).

- **Gap 2 (conversion executor).** Add `ConversionProjection::MarchingCubes`,
  `v03_conversion_projection(Voxel, Mesh) = Some(MarchingCubes)`, and a Phase-1/Phase-2 executor arm
  that runs `realize_mesh_from_voxel_with_options(grid, &opts)` — **threading the `Surface` op's
  `MarchingCubesOptions`** (not the `MarchingCubesOptions::default()` that the bare `tessellate`
  trait route hard-codes, and not the `NO_OPTIONS` cache sentinel).

The `Convert`-is-edge-only invariant (see §7) means the `Voxel→Mesh` crossing is a **conversion
edge feeding a Mesh-repr terminal op**, never a terminal kernel `execute`. The surfacing work lives
in the marching-cubes conversion stage; the `Surface` op is the Mesh-repr realization anchor that
carries `MarchingCubesOptions`.

## 4. Resolved design decisions

- **D1 — Op name: `isosurface`.** Names the intent (iso-level surface extraction),
  algorithm-agnostic (survives a future swap to dual-contouring / adaptive meshing), and does not
  collide with the existing `SurfaceNurbs` op or the `surface_finish*` examples.

- **D2 — Surface a *Voxel geometry handle*, NOT a `Field<D,C>`.** #3440's primitive takes a
  registered Voxel **grid handle** (`GeometryHandleId`) and returns a `Mesh`. θ's imported-VDB
  pipeline (`engine_eval.rs` `CompiledFieldSource::Imported` → `read_vdb_file`) yields a
  **`SampledField`** — a CPU-resident sampled value, *not* a registered grid handle. No
  `SampledField → grid-handle` path exists. Routing `isosurface` through a Field would require
  inventing that path; routing it through a Voxel handle reuses the existing `BRep→Mesh→Voxel`
  chain. **Consequence:** this PRD does **not** depend on θ. §8 task ι's stated `Prereqs: θ` is
  incorrect and is corrected by task **ζ** below.

- **D3 — Isovalue & options: `isosurface(g, iso: <Length> = 0mm, adaptive: <Bool> = false)`.**
  `iso:` → `MarchingCubesOptions::iso_level` (`0.0` = the zero level-set = the surface of an SDF /
  narrow-band level set); `adaptive:` → `MarchingCubesOptions::adaptive`. Named args parse today
  (grammar verified). Both optional; defaults reproduce the current `tessellate`-route behaviour.

- **D4 — Options threading via the op's per-op options-hash.** The bare `GeometryKernel::tessellate`
  route hard-codes `MarchingCubesOptions::default()` and its `(handle, tolerance)` signature cannot
  carry a custom iso. So the `MarchingCubesOptions` travel on the `Surface` realization and the
  marching-cubes conversion stage reads them, **replacing the `NO_OPTIONS` intermediate-cache
  sentinel** with `MarchingCubesOptions::content_hash()`. This keeps two builds with different `iso:`
  from aliasing the same cache slot (the ESC-3433-117 non-zero-domain-tag invariant already guards
  the hash).

- **D5 — Voxel→Mesh is a conversion edge, not a terminal kernel op.** Forced by the dispatcher's
  input==output invariant (`dispatcher.rs:660-666`): `Operation::Convert { from }` entries are used
  **exclusively** in the BFS expansion step, never as the final-stage op, and no non-`Convert` op
  accepts `Voxel` today. See §7 C-1.

- **D6 — Honest-signal checkpoints.** The end-to-end test asserts the realization graph **traversed
  Voxel**: the operand realization's `produced_repr == ReprKind::Voxel` and the terminal
  realization's `produced_repr == ReprKind::Mesh`. This pins that the mesh came from marching cubes,
  not from `BRep→Mesh` tessellation or the `realize_solid_sdf` SampledField recipe.

## 5. Pre-conditions for activating

- **#3440** (marching-cubes primitive + `(Convert{from:Voxel}, Mesh)` descriptor entry) — **DONE**.
- **#4422** (OpenVDB `Voxelize` dispatcher stage, `Mesh→Voxel`) and the `BRep→Mesh` `Tessellate`
  stage — **DONE** (they realize `isosurface`'s operand as a Voxel grid).
- OpenVDB kernel registered under `cfg(has_openvdb)` / `stub_register` (present in the CLI + eval
  test binaries). In stub builds the path degrades to a diagnostic, never a panic.
- No grammar work (`isosurface(...)` and its named args parse today).

## 6. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `multi-kernel-phase-3.md` §8 **task ι** | completes / supersedes | the Voxel→Mesh `build()` observable | **this PRD** | this PRD's task δ is the real ι leaf |
| `multi-kernel-phase-3.md` §8 **task ρ** | produces-for | multi-stage chain reachable in production (ρ's `LongChainRealization` fixture) | multi-kernel-phase-3 (ρ) | ρ `depends_on` δ |
| cancelled **task 4816** | supersedes | — | this PRD | 4816 stays cancelled; this PRD is its correct decomposition |
| `imported-field-source ↔ multi-kernel` (contested pair #2) | **not touched** | θ's `SampledField` is the wrong type (D2) | n/a | this PRD does **not** re-open the seam |

**Integration-seam sub-check** (`engine-integration-norm.md` §3): `isosurface` plugs into existing
seams only — **§3.3 multi-kernel dispatch** (dispatcher + conversion executor), **§3.1 op-execute**
(`GeometryOp` emission/execution), **§3.2 realization-kind dispatch** (demand-seeding). No new seam.

**G1 consumer.** The user-facing leaf (CLI triangle-count print + viewport `mesh_stats`) is itself a
named consumer; additionally **task ρ** is a concrete in-repo downstream consumer of the multi-stage
surface chain. Not an orphan.

## 7. Contract (B + H)

### Seam signatures

```rust
// reify-ir (crates/reify-ir/src/geometry.rs) — mirror the existing Operation / GeometryOp shapes.
Operation::Surface                                   // coarse classifier key; input repr = Voxel, output = Mesh
GeometryOp::Surface { grid: GeomRef, options: MarchingCubesOptions }

// reify-eval classifiers (engine_build.rs) — all four mirror fns updated in one diff (strum test):
geometry_op_to_operation(GeometryOp::Surface{..}) -> Operation::Surface
classify_op_input_reprs(Operation::Surface)        -> Some(&[ReprKind::Voxel])   // Voxel-only consumer
op_accepts_repr(Operation::Surface, Voxel)         -> true;  (…, Mesh|BRep) -> false
parent_handles_for_op(GeometryOp::Surface{grid})   -> [grid handle]

// reify-eval dispatcher (dispatcher.rs)
enum ConversionProjection { Tessellate, Voxelize, MarchingCubes }   // + new variant
v03_conversion_projection(Voxel, Mesh) -> Some(ConversionProjection::MarchingCubes)

// reify-kernel-openvdb (kernel_real.rs:336) — DONE, unchanged:
fn realize_mesh_from_voxel_with_options(&self, handle: GeometryHandleId,
    opts: &MarchingCubesOptions) -> Result<Mesh, GeometryError>
```

### Invariants

- **C-1 (edge-only Voxel→Mesh).** The `Voxel→Mesh` crossing is a `ConversionProjection` stage in a
  `DispatchPlan.conversions`, never a terminal `kernel.execute`. The `Surface` realization's terminal
  op is Mesh-repr; the marching-cubes conversion feeds it. (Forced by the dispatcher input==output
  invariant; `Convert` is BFS-expansion-only.)
- **C-2 (options fidelity).** A `MarchingCubes` stage runs `realize_mesh_from_voxel_with_options`
  with the `Surface` op's `MarchingCubesOptions`. It MUST NOT collapse to
  `MarchingCubesOptions::default()`; the intermediate-cache key uses
  `MarchingCubesOptions::content_hash()`, not `NO_OPTIONS`.
- **C-3 (Voxel demand is opt-in).** Only a realization consumed by a Voxel-only-input op is demanded
  `Voxel`. Existing `Mesh`/`BRep` demand for every other graph is unchanged (regression-guarded).
- **C-4 (graceful degradation).** Absent OpenVDB (stub build), an empty grid, or a surface that does
  not cross `iso` → an `Error`/empty-mesh diagnostic, never a panic (mirrors #3440's empty-surface
  contract and `realize_solid_sdf`'s D5 discipline).
- **C-5 (honest provenance).** Operand realization `produced_repr == Voxel`; terminal
  `produced_repr == Mesh`. The path is never silently rerouted onto `BRep→Mesh` tessellation or
  `realize_solid_sdf`.

### Boundary-test sketch (faces both sides of the seam)

| # | Scenario | Preconditions | Postconditions (asserts) | Side |
|---|---|---|---|---|
| B1 | Demand-seeding emits Voxel | template: `let s=box(..); isosurface(s)` | `compute_demanded_reprs` sets `demand[s]==Voxel`; terminal `==Mesh` | producer (gap 1) |
| B2 | `(Voxel,Mesh)` classifies + executes | synthetic registry, `available={Voxel}`, real grid | `v03_conversion_projection(Voxel,Mesh)==MarchingCubes`; executor returns Mesh, `vertices>0` | producer (gap 2) |
| B3 | Options thread, no default-collapse | `isosurface(s, iso: 0mm)` vs `iso: <band-edge>` | two distinct meshes (or one empty); cache keys differ (`content_hash != NO_OPTIONS`) | producer (gap 2/D4) |
| B4 | End-to-end build (the leaf) | `voxel_to_mesh.ri`, OpenVDB present | `reify build` exits 0, prints `Triangles: N>0`; operand `produced_repr==Voxel`, terminal `==Mesh` | consumer (δ) |
| B5 | Viewport observable | δ build loaded in GUI debug | `mesh_stats.vertex_count > 0` | consumer (δ) |
| B6 | No silent reroute | `voxel_to_mesh.ri` | terminal is NOT a `BRep→Mesh` tessellation and NOT a `SampledField` (C-5 checkpoints) | consumer (δ) |
| B7 | Degradation | stub build (no OpenVDB) | one `Error`/diagnostic, no panic, non-zero exit | both (C-4) |

## 8. Decomposition plan

B+H: foundation intermediates (α, β, γ) roped to the integration-gate **leaf δ** (C-as-integration
pattern); Phase 3 hardening (ε); companion doc correction (ζ). Greek labels; task IDs assigned at
decompose time.

### Phase 1 — foundation (intermediates)

- **α — `isosurface` builtin + `Surface` IR op + classifiers.**
  Modules: `reify-ir` (`Operation::Surface`, `GeometryOp::Surface`), `reify-compiler`
  (`geometry.rs` builtin lowering). Adds the four mirror-fn arms
  (`geometry_op_to_operation`/`classify_op_input_reprs`/`op_accepts_repr`/`parent_handles_for_op`).
  **Unlocks:** β, γ, δ.
  **Signal (intermediate):** compiler lowers `isosurface(s)` → `GeometryOp::Surface{grid, options}`
  (IR-shape unit test) and the strum-completeness + `op_accepts_repr` classifier tables pass with
  the new variant. *(Foundation task — signal is a wiring proof; it is roped to leaf δ, not a
  fake-done leaf.)*

- **β — Voxel demand-seeding.**
  Modules: `reify-eval` (`engine_build.rs` `compute_demanded_reprs` reverse-pass).
  **Prereqs:** α. **Unlocks:** δ.
  **Signal (intermediate):** unit test — a template with an `isosurface` consumer yields
  `demand[operand] == ReprKind::Voxel` and terminal `== Mesh`; a sibling non-`isosurface` template
  is unchanged (C-3 regression guard).

- **γ — `(Voxel,Mesh)` marching-cubes conversion + options threading.**
  Modules: `reify-eval` (`dispatcher.rs` `ConversionProjection`/`v03_conversion_projection`,
  `engine_build.rs` executor Phase-1/2 arm), `reify-kernel-openvdb` (options-carrying surface call if
  the trait surface needs it).
  **Prereqs:** α. **Unlocks:** δ.
  **Signal (intermediate):** extend `dispatcher_integration.rs` — a `(openvdb, Voxel, Mesh)` stage on
  a real grid **executes** (not degrades) and returns a Mesh with `vertices > 0`; cache key uses
  `MarchingCubesOptions::content_hash()` (B2/B3, producer side).

### Phase 2 — vertical slice (the integration-gate LEAF)

- **δ — `examples/multi_kernel/voxel_to_mesh.ri` + CLI triangle-count + end-to-end test.**
  Modules: `examples/multi_kernel/`, `reify-cli` (print `Triangles: N` on a Mesh-terminal build),
  `reify-eval/tests` (end-to-end).
  **Prereqs:** α, β, γ.
  **Signal (LEAF, user-observable):** `reify build examples/multi_kernel/voxel_to_mesh.ri
  <out>.stl` exits 0 and prints `Triangles: N` with **N > 0**; the end-to-end test asserts operand
  `produced_repr == Voxel`, terminal `produced_repr == Mesh`, and Mesh `vertices > 0`;
  viewport-debug-MCP `mesh_stats.vertex_count > 0` (B4/B5/B6). **This signal is the boundary-test
  sketch's consumer-side rows — closing G2/G5.**

### Phase 3 — hardening

- **ε — non-default `iso:` honestly exercised.**
  Modules: `reify-eval/tests` (or `reify-cli/tests`).
  **Prereqs:** δ.
  **Signal (LEAF):** two builds of a narrow-band fixture with **distinct** `iso:` values produce
  **measurably different** outcomes — different triangle counts, or one non-empty + one empty when
  `iso` lies outside the band (binary assertion; **no guessed numeric bound**). Proves D4's options
  path is wired, not merely declared (guards the C-10 declared-but-unexercised shape).

### Phase 4 — companion correction

- **ζ — correct `multi-kernel-phase-3.md` §8 task ι.**
  Edit: ι `Prereqs: θ` → **not θ** (D2: θ yields a `SampledField`, the wrong type); source is a
  voxelized solid via the existing `BRep→Mesh→Voxel` chain. Note the three-gap reality (the §3a.5
  "single-row extension" framing covered only the kernel/executor half, not demand-seeding or a DSL
  op). Cross-link this PRD; mark ι as owned here.
  **Prereqs:** none (doc-only). **Signal (LEAF):** PRD updated, cross-links bidirectional, doc lint
  passes.

### Dependency view

```
α ─┬─→ β ─┐
   └─→ γ ─┴─→ δ ─→ ε
ζ  (independent doc edit)
δ ──→ (multi-kernel-phase-3 §8 task ρ, out-of-batch consumer)
```

### Per-leaf premise notes (G6 / capability-manifest seed)

- **δ (end-to-end capability, branch 3).** Every capability traces to this batch or a DONE prereq:
  DSL op + demand + conversion (α/β/γ, this batch), marching-cubes primitive (#3440 DONE),
  `BRep→Mesh→Voxel` chain (Tessellate + Voxelize #4422, DONE). **No capability delegates to a task
  that depends on δ.** `vertices > 0` is achievable: a closed solid's `meshToVolume` narrow-band
  level set crosses `iso=0` at its boundary, so marching cubes yields a non-empty mesh (#3440's
  empty-surface caveat is avoided by using a solid, not a hollow/degenerate grid). Asserted as the
  **binary** `N > 0` — no guessed count (G6 branch 1 avoided).
- **ε.** Distinct-`iso` → distinct-outcome is achievable for a narrow-band level set (iso outside the
  band → empty; inside → offset surface). Binary assertion; no numeric bound.
- **ζ.** Doc-only; no substrate premise.

## 9. Out of scope

- **Fidget `Sdf→Mesh` (`κ`).** Sibling follow-on in `multi-kernel-phase-3.md` §8 Phase 5; different
  kernel + `IsoMeshOptions`. Not this PRD.
- **Surfacing an *imported* VDB grid** (θ's pipeline) directly. Requires a `SampledField →
  grid-handle` bridge that does not exist (D2). A future PRD may add it; `isosurface` on a voxelized
  solid does not need it.
- **`@optimized` / ComputeNode caching of the surface stage** beyond the existing
  intermediate-cache slot.
- **Adaptive-mesh quality tuning** beyond the boolean `adaptive:` → `adaptivity ∈ {0.0, 1.0}` map.

## 10. Open questions (tactical — deferred, not design-blocking)

1. **Terminal-anchor-op modeling for the `Surface` realization.** Given C-1 (edge-only Voxel→Mesh),
   the `Surface` realization needs a Mesh-repr anchor fed by the marching-cubes edge. Two local
   implementations keep the system coherent: (a) a thin Mesh→Mesh `Surface` execute arm that returns
   the converted handle, carrying `MarchingCubesOptions`; (b) the executor treats a trailing
   `MarchingCubes` conversion stage as the realization's terminal output directly. **Suggested:** (a)
   — it gives the options a natural home and reuses the existing "conversions then terminal op"
   executor shape. **Decide during task α** against the code; either choice leaves §7's contract and
   the δ observable unchanged.
2. **`iso:` unit.** `Length` (metric, matches SDF distance units) vs a dimensionless scalar for
   non-SDF grids. **Suggested:** `Length`, since the operand is always a distance-valued level set
   here. Decide during α.
3. **CLI triangle-count line format.** `Triangles: N` vs folding into a `--verbose` mesh-stats block.
   **Suggested:** always-on `Triangles: N` for any Mesh-terminal build (small, matches the brief's
   "CLI prints the triangle count"). Decide during δ.
