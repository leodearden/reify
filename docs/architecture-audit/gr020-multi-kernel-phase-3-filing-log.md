<!-- 2026-05-14 RECOVERY AUDIT TRAIL
This filing log was authored 2026-05-12. The task IDs referenced below (3491-3594, 3503/3504/3508/3510/3512, 3563-3574, etc.) were LOST in the 2026-05-13 fused-memory SIGABRT.
The DAG was re-filed 2026-05-14 with NEW task IDs:
multi-kernel-phase-3 DAG: α=3432 (was 3526), β=3433 (was 3527), γ=3434 (was 3528), δ=3435 (was 3529), ε=3436 (was 3530), ζ=3437 (was 3531), η=3438 (was 3532), θ=3439 (was 3533), ι=3440 (was 3534), κ=3441 (was 3535), ξ=3442 (was 3536), ο=3443 (was 3537), π=3444 (was 3538), ρ=3445 (was 3539), μ=3446 (was 3540), ν=3447 (was 3541), τ=3448 (was 3542). Cross-PRD edge ξ→CN-η: 3442→3426.
The body of this log is preserved as historical record. Use docs/task-recovery-2026-05-13/id-map.json as the canonical translation table for live work.
-->

# Multi-kernel Phase 3 §8 DAG — filing log

Session: 2026-05-12 decompose-mode filing of `docs/prds/v0_3/multi-kernel-phase-3.md` (commit `e477a68d96`) into the task tracker.

Source contract: `docs/prds/v0_3/multi-kernel-phase-3.md` resolving cluster C-18 / gap GR-020. Folds in GR-034 (cluster C-32, long-chain diagnostic) and the OpenVDB consumer half of GR-003 (cluster C-17) per the 2026-05-12 contested-ownership disposition.

Cross-PRD prerequisite (for task ξ): `docs/prds/v0_3/compute-node-contract.md` task η (3497, `pending`) — first real FEA consumer needed to validate ComputeNode's transitive cache-key composition through the realization layer.

## Task IDs assigned

| Letter | Task ID | Title | Prereqs (intra-batch task IDs) |
|---|---|---|---|
| α | 3526 | RealizationNodeData.produced_repr field (per-realization ReprKind tracking) | none |
| β | 3527 | RealizationCacheKey adds options_hash for per-op option folding | none |
| γ | 3528 | DiagnosticCode entries for multi-kernel dispatch failures | none |
| δ | 3529 | OCCT Convert{BRep→Mesh} capability descriptor + TessellateOptions hash producer | α, β, γ |
| ε | 3530 | Engine.geometry_kernels multi-handle shape + execute_realization_ops dispatches per-op | δ, α |
| ζ | 3531 | Manifold execute arm for (BooleanUnion/Difference/Intersection, Mesh) integrated | ε |
| η | 3532 | OpenVDB Convert{Mesh→Voxel} capability descriptor + MeshToVoxelOptions hash | δ, ε |
| θ | 3533 | engine_eval CompiledFieldSource::Imported routes through OpenVDB ingest (GR-003) | η |
| ι | 3534 | OpenVDB Convert{Voxel→Mesh} marching cubes + MarchingCubesOptions hash | θ |
| κ | 3535 | Fidget Convert{Sdf→Mesh} iso-meshing + IsoMeshOptions hash | δ, ε |
| ξ | 3536 | Gmsh VolumeMeshOptions as options-hash producer (hex-wedge force_tet cache discipline) | β, ε (+ cross-PRD: 3497) |
| ο | 3537 | #kernel(...) pragma propagation to dispatcher prefer_kernel | ζ |
| π | 3538 | reify.toml kernel pin consumer-side enforcement at Engine::with_registered_kernels | ε |
| ρ | 3539 | long-chain diagnostic wired into execute_realization_ops (GR-034) | ε, ι |
| μ | 3540 | correct v0.2 multi-kernel.md ReprKind count (four → five with VolumeMesh) | none |
| ν | 3541 | v0.2 imported-field-source.md cross-reference to Phase 3 GR-003 wiring | none |
| τ | 3542 | verify gap-register GR-020/GR-034/GR-003 disposition pointers (audit-confirmation) | none |

All filed via `mcp__fused-memory__submit_task(planning_mode=true)`. All start in `deferred` status; flipped to `pending` via `commit_planning` at the end of this session.

## Dependency edges added (18 edges total, intra-batch only)

| From | To (depends on) | Rationale |
|---|---|---|
| 3529 (δ) | 3526 (α) | Convert edge implementation uses produced_repr tag |
| 3529 (δ) | 3527 (β) | TessellateOptions hash producer needs options_hash key |
| 3529 (δ) | 3528 (γ) | Convert edge plumbing uses NoKernelChain diagnostic |
| 3530 (ε) | 3529 (δ) | multi-handle engine needs first Convert edge to dispatch |
| 3530 (ε) | 3526 (α) | execute_realization_ops populates produced_repr |
| 3531 (ζ) | 3530 (ε) | Manifold consumer needs multi-handle engine for routing |
| 3532 (η) | 3529 (δ) | OpenVDB Mesh→Voxel chain needs OCCT BRep→Mesh upstream |
| 3532 (η) | 3530 (ε) | OpenVDB Convert routed via multi-handle dispatcher |
| 3533 (θ) | 3532 (η) | engine_eval Imported arm needs OpenVDB capability infra |
| 3534 (ι) | 3533 (θ) | Voxel→Mesh consumes Voxel grids produced by θ ingest |
| 3535 (κ) | 3529 (δ) | Fidget Convert edge follows OCCT Convert pattern |
| 3535 (κ) | 3530 (ε) | Fidget Sdf→Mesh routed via multi-handle dispatcher |
| 3536 (ξ) | 3527 (β) | force_tet cache discipline uses options_hash key |
| 3536 (ξ) | 3530 (ε) | hex-wedge fold-in uses per-op dispatch through engine |
| 3537 (ο) | 3531 (ζ) | pragma steering needs Manifold + OCCT serving same op |
| 3538 (π) | 3530 (ε) | reify.toml pin enforcement at with_registered_kernels |
| 3539 (ρ) | 3530 (ε) | long-chain diagnostic wires into execute_realization_ops |
| 3539 (ρ) | 3534 (ι) | 3-stage chain (BRep→Mesh→Voxel→Mesh) reachable after ι |

DAG view (from PRD §8):

```
α(3526) ─┐
β(3527) ─┼─→ δ(3529) ─→ ε(3530) ─┬─→ ζ(3531) ─→ ο(3537)
γ(3528) ─┘                       │
                                 ├─→ η(3532) ─→ θ(3533) ─→ ι(3534) ─┐
                                 │                                   │
                                 ├─→ κ(3535)                         │
                                 │                                   │
                                 ├─→ ξ(3536) ←── (compute-node-contract.md η: 3497)
                                 │                                   │
                                 └─→ π(3538)                         │
                                                                     │
                                                  ρ(3539) ←──────────┘
                                                  ρ(3539) ←─ ε(3530)

μ(3540), ν(3541), τ(3542) — independent doc edits / audit confirmation
```

## Cross-PRD dependencies

**Rule reversal 2026-05-12 (post-filing):** Cross-PRD deps MUST be real `add_dependency` edges, NOT metadata-only. Leo flagged the metadata-only approach immediately after this filing; edge added retroactively. See updated memory `preferences-cross-prd-deps-real-edges`.

| Task | Edge added | Rationale |
|---|---|---|
| ξ (3536) | `add_dependency(3536, depends_on=3497)` | ComputeNode contract task η (3497) — first real FEA consumer (solve_elastic_static through ComputeNode trampoline). Hex-wedge `force_tet` cache discipline slice validates ComputeNode's transitive cache-key composition through realization layer. Leo confirmed in session prompt. |

Informational (kept also as metadata for prose/audit, but the edge above is what gates the scheduler):
- ξ (3536) `metadata.cross_prd_dep`: `docs/prds/v0_3/compute-node-contract.md`
- θ (3533) `metadata.resolves_gap`: `GR-003` — folded in per 2026-05-12 contested-ownership disposition.
- ρ (3539) `metadata.resolves_gap`: `GR-034` — folded in per gap-register disposition.

## Supersessions applied

**Per memory `preferences_supersession_same_prd_only`** — same-PRD-decomp siblings only get `cancelled` status flips. Cross-PRD nominally-re-met tasks stay readable for their owning PRD to absorb.

| Task | Action | Reason |
|---|---|---|
| — | none | This is a fresh PRD with no prior same-PRD-decomp siblings. |

### Code-shape supersession (no task to flip)

Task ε (3530) supersedes the **code-shape**: `Engine.geometry_kernel: Option<Box<dyn GeometryKernel>>` + `with_registered_kernel` + `pick_lexmin_brep_kernel`. Leo confirmed this supersession in the PRD-authoring session 2026-05-12. Recorded in ε's `metadata.supersedes_code_shape` field. No same-PRD sibling task exists to flip to cancelled.

### Cross-PRD nominally-re-met (NOT cancelled — owning PRDs absorb later)

Per memory `preferences_supersession_same_prd_only`, these stay readable; this PRD does NOT flip them:

- v0.2 multi-kernel.md tasks 2566–2569 — Phase 1+2 already shipped; this PRD is Phase 3. No supersession needed.
- hex-wedge-meshing.md M-024 (force_tet shared cache slot) — nominally re-met by ξ but owning PRD (hex-wedge) absorbs in follow-up sweep.
- shells-prd M-025 (BRep→Voxel chain for mid-surface) — nominally re-met by δ+η chain, but owning PRD (shells) absorbs in follow-up.

## τ verification at filing time

PRD §8 task τ states: "performed in the 2026-05-12 PRD-authoring session alongside the PRD save; verify at decompose time that the three GR entries point at this PRD and at §8 tasks θ / ρ".

Verified via grep at filing time:

```
$ grep -n "multi-kernel-phase-3" docs/architecture-audit/gap-register.md | head -10
62:### GR-003 — OpenVDB sub-kernel dispatcher / consumer boundary
300:### GR-020 — Kernel/eval ReprKind chain coverage gaps (cluster C-18)
310:| Disposition | ... `docs/prds/v0_3/multi-kernel-phase-3.md` ... §8 task ρ ... §8 task θ ...
496:### GR-034 — Long-chain diagnostic / per-stage tolerance budget unreachable (cluster C-32)
506:| Disposition | folded into GR-020 — resolution mechanism `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task ρ
```

All three entries point at the PRD with intact §8 task letter references. τ (3542) can be closed `done` once the batch flips pending (the verification is complete; no further work required).

## Session-end procedure

1. Call `commit_planning` on `3526,3527,3528,3529,3530,3531,3532,3533,3534,3535,3536,3537,3538,3539,3540,3541,3542` with target_status=pending.
2. Write a summary memory under `observations_and_summaries` capturing the IDs + this log location.

## Hand-back summary

- **Tasks filed:** 17 (α=3526, β=3527, γ=3528, δ=3529, ε=3530, ζ=3531, η=3532, θ=3533, ι=3534, κ=3535, ξ=3536, ο=3537, π=3538, ρ=3539, μ=3540, ν=3541, τ=3542).
- **Intra-batch deps wired:** 18 edges.
- **Cross-PRD deps wired as real edges:** 1 — `add_dependency(3536, depends_on=3497)` (ξ → compute-node-contract task η). Rule reversed 2026-05-12 post-filing — metadata-only no longer acceptable.
- **Supersessions applied:** 0 same-PRD; 1 code-shape (ε supersedes single-kernel `Engine.geometry_kernel`, no task to flip).
- **`combined` results:** none — all 17 returned `status=deferred` from planning_mode submit_task (synchronous path, no curator combining).
- **Orchestrator-side note:** the orchestrator does NOT currently read `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata fields. These are substrate for the F-infra follow-up session. The decompose-skill wrote them; orchestrator-side read is a separate design+implement session pair.
