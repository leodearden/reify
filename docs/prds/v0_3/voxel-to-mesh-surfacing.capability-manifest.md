# Capability manifest — `voxel-to-mesh-surfacing` (isosurface)

Mechanizes G3 + G6 per leaf for `docs/prds/v0_3/voxel-to-mesh-surfacing.md`. Binds each
capability a leaf signal asserts to evidence. **Any FAIL binding blocks the batch.** Result:
**all bindings PASS — batch not blocked.**

Verification method: deterministic grep/producer/grammar-gate bindings against main
@ `5306985849` (the PRD commit). The stochastic D3 probe workflow
(`scripts/prd-decompose-verify.mjs`) was **not** run: its probes (`reify check`/`eval`/`tree-sitter`)
test whether a capability exists *now*, but the δ/ε leaf signals assert the *new* capability this
batch builds — probing them now would false-block on "isosurface not yet wired" (the deliverable,
not a substrate gap). The one genuinely-probeable substrate premise (grammar) was verified
deterministically (`tree-sitter parse --quiet`, 0 ERROR nodes) and is bound below.

Sentinel/evidence conventions (overlay): empty-value sentinel = `Value::Undef`; production entry
paths = reify-eval dispatch tables + `engine_build.rs`/`engine_eval.rs` walks + the conversion
executor; grammar gate = `tree-sitter parse --quiet`.

Task labels α…ζ map to the filed task IDs in the batch (see the PRD §8 decomposition and the
decompose hand-back). "upstream" = in the transitive dependency closure of the leaf.

---

## Leaf δ — `voxel_to_mesh.ri` build → CLI `Triangles: N>0` + viewport `mesh_stats.vertex_count>0`

| # | Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|---|
| δ1 | `isosurface(...)` resolves + lowers to `GeometryOp::Surface` | capability→producer / DAG-direction | `producer:task-α` (this batch, **upstream** of δ) — α adds the `reify-compiler` builtin + `reify-ir` op | **PASS** |
| δ2 | operand demanded `ReprKind::Voxel` | capability→producer / DAG-direction | `producer:task-β` (**upstream**) — β extends `compute_demanded_reprs` reverse-pass (`engine_build.rs:2260-2304`) | **PASS** |
| δ3 | `(Voxel,Mesh)` marching-cubes conversion executes | capability→producer / DAG-direction | `producer:task-γ` (**upstream**) — γ adds `ConversionProjection::MarchingCubes` + executor arm | **PASS** |
| δ4 | marching-cubes primitive `realize_mesh_from_voxel_with_options(handle,&opts) -> Mesh` | wired-on-main (anti-orphan) | `grep:crates/reify-kernel-openvdb/src/kernel_real.rs:336` — DONE (#3440); **wired** into the `GeometryKernel::tessellate` production route at `kernel_real.rs:592` (calls it) | **PASS** |
| δ5 | operand realized as Voxel via existing `BRep→Mesh→Voxel` chain | capability→producer (done, upstream) | `producer:task-3438`(η, Mesh→Voxel descriptor, DONE) + `producer:task-4422`(Voxelize dispatcher stage, DONE) + `producer:task-4050`(φ in-realization conversion executor, DONE, `engine_build.rs`); Tessellate BRep→Mesh at `dispatcher.rs:618` (DONE) | **PASS** |
| δ6 | conversion executor exists to host the new arm | wired-on-main | `grep:crates/reify-eval/src/engine_build.rs:6611-6812` — φ executor walks `plan.conversions` (Tessellate/Voxelize arms live); γ adds the MarchingCubes arm here | **PASS** |
| δ7 | `RealizationNodeData.produced_repr` exists (honest-provenance checkpoint D6) | wired-on-main | `producer:task-3432` (DONE) — per-realization `produced_repr` field; the operand node writes `Voxel`, terminal writes `Mesh` (non-sentinel, set by β/γ on the production path) | **PASS** |
| δ8 | grammar: `isosurface(g)` / `isosurface(g, iso: 0mm, adaptive: false)` parses | grammar-fixture (anti-mismatch) | **no novel grammar production** — parses as an existing `function_call` with `named_argument_list`; `tree-sitter parse --quiet` → exit 0, **0 ERROR nodes** (verified 2026-07-05). No grammar-producer task needed. | **PASS** |
| δ9 | `Triangles: N` with **N > 0** | numeric floor (anti-floor) | **binary** `N > 0`, not a guessed count (G6 branch-1 avoided). Achievable: a closed solid's `meshToVolume` narrow-band level set crosses `iso=0` at its boundary ⇒ marching cubes yields a non-empty mesh (#3440 empty-surface caveat avoided by using a solid). Floor = 0; assert `> floor`. | **PASS** |
| δ10 | end-to-end capability closure | DAG-direction (anti-inversion) | every δ capability is delivered by δ itself or an **upstream** task (α/β/γ in-batch; 3440/3438/4422/4050/3432 DONE). **No capability delegates to a task that depends on δ.** | **PASS** |

**Honest-signal note (brief mandate).** δ's end-to-end test asserts the operand realization
`produced_repr == Voxel` and the terminal `produced_repr == Mesh` (PRD D6) — pinning that the Mesh
came from marching cubes, **not** from `BRep→Mesh` tessellation and **not** from the
`realize_solid_sdf` `SampledField` recipe. The signal cannot be satisfied by re-routing onto the
already-buildable path.

## Leaf ε — non-default `iso:` changes the surfaced mesh (options-threading honest test)

| # | Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|---|
| ε1 | two distinct `iso:` values → measurably different outcome | capability→producer / DAG-direction | `producer:task-δ`+`task-γ` (options threading via PRD D4 — `MarchingCubesOptions::content_hash()` replaces `NO_OPTIONS`; both **upstream** of ε) | **PASS** |
| ε2 | distinct-iso → distinct-outcome is achievable | numeric floor | **binary** (distinct triangle counts, or one non-empty + one empty when `iso` is outside the narrow band). No numeric bound. Achievable for a narrow-band level set. | **PASS** |
| ε3 | options-hash non-collision | field-population / anti-collision | `grep:crates/reify-kernel-openvdb/src/marching_cubes_options.rs:76` — `content_hash()` seeds a non-zero domain tag (ESC-3433-117), so a non-default `iso` cannot alias the `NO_OPTIONS` sentinel (`ContentHash(0)`). | **PASS** |

## Leaf ζ — doc correction of `multi-kernel-phase-3.md` §8 task ι

Doc-only; no substrate premise. Signal = PRD updated + cross-links bidirectional + doc lint passes.
**N/A** (no capability binding).

---

## Intermediates (not leaves; roped to δ via C-as-integration-gate)

- **α** (`isosurface` builtin + `Surface` IR op + classifiers) → unlocks β, γ, δ.
- **β** (Voxel demand-seeding) → unlocks δ.
- **γ** (`(Voxel,Mesh)` marching-cubes conversion + options threading) → unlocks δ.

Each is a wiring proof (strum-completeness / classifier-table / demand-arm unit test), acceptable
as an **intermediate** signal because it names its downstream consumer (δ) and is not offered as a
fake-done leaf.

## Blocking summary

No binding resolves to `declared-only | test-only | producer-absent | producer-downstream |
producer-extent-short | fixture-ERROR | bound≤floor | rejection-absent`. **Batch clears the
manifest gate.**
