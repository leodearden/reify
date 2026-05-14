# GR-016 gui-event-channel-inventory PRD §9 DAG — filing log

Session: 2026-05-14 re-decompose pass over `docs/prds/v0_3/gui-event-channel-inventory.md` as part of the post-SIGABRT task recovery operation (see `docs/task-recovery-2026-05-13/investigation.md`).

Source PRD: `docs/prds/v0_3/gui-event-channel-inventory.md` — resolves GR-016 / cluster C-13.
Skill: `.claude/skills/prd/` decompose mode.
Recovery context: prior decomposition's task IDs (α=3563, β=3564, γ=3565, λ=3573, μ=3574, plus Phase 2/3/4 tasks ~3566-3572) lost in the 2026-05-13 14:25 BST fused-memory watchdog SIGABRT. This is a re-filing under fresh permanent IDs; original IDs (where determinable) preserved in `metadata.original_task_id`.

## Gate re-walk summary

| Gate | Result | Notes |
|---|---|---|
| G1 (consumer named) | PASS | All channels in §2.1/§2.2/§2.3 have named consumers (existing bridge.ts wrappers, new debug/feature panels, MCP tools). Each task's `consumer_ref` recorded in metadata. |
| G2 (user-observable leaf) | PASS | All Phase 2/3 emitter tasks (δ, ε, ζ, η, θ) declare CLI / viewport / debug-MCP signals. Foundation tasks (α, β, γ) name downstream consumers. Phase 4/5 doc tasks (ι, κ, μ) name git-diff signals. |
| G3 (grammar verified) | N/A | PRD covers IPC/event channels; no novel `.ri` grammar introduced. |
| G4 (cross-PRD seam ownership) | PASS | PRD §1 explicitly excludes PNv2 M-018 from inventory (handed to GR-004); §2.4/§2.5 explicitly delegate payload extensions + frontend state to citing PRDs. GR-024 (buckling-eigensolver) owns the buckling slice; this PRD owns the channel convention — clean split (see `gr024-buckling-eigensolver-filing-log.md` cross-PRD edges from 3458 → 3536/3537). |
| G5 (B + H for high-stakes) | PASS | PRD §8 boundary-test sketch faces both ways (producer + consumer); §9 task β is the C-as-integration-gate paired leaf (`convention_smoke` tests prove the convention works before any production channel rides it); §9 task δ is the Phase 2 vertical-slice proof. |
| META | PASS | §11 lists 7 tactical open questions — each annotated with suggested resolution and deciding-task pointer. No design-level open questions remain. |

## Task IDs assigned

| Letter | Task ID | Title | Prereqs (task IDs) | Original ID |
|---|---|---|---|---|
| α | 3536 | canonical GUI event channel inventory at docs/gui-event-channels.md | (none) | 3563 |
| β | 3537 | convention helpers (emit_typed + validatePayload + mockTauriEvent) | α (3536) | 3564 |
| γ | 3538 | per-channel spec template at docs/gui-event-channels/_template.md | α (3536) | 3565 |
| δ | 3539 | auto-resolve trio emitters + per-channel specs | α, β, γ | — |
| ε | 3541 | warm-pool-event emitter + WarmPoolDebugPanel | α, β, γ | — |
| ζ | 3543 | solver-progress emitter + SolverProgressOverlay | α, β, γ | — |
| η | 3545 | fea-case-changed emitter + FeaCasePickerDropdown | α, β, γ | — |
| θ | 3547 | morph_stats debug-MCP RPC | α, β, γ | — |
| ι | 3548 | strike PNv2 M-018 from gap-register GR-016 evidence row | (none) | — |
| κ | 3550 | update 6 citing PRDs' cross-PRD section | α (3536) | — |
| λ | 3551 | mode-shape-frame channel + producer (DEFERRED bookmark, **CANCELLED at filing time**) | (none) | 3573 |
| μ | 3552 | contributor doc note + optional new-emitter lint script | α (3536) | 3574 |

All non-cancelled tasks filed via `mcp__fused-memory__submit_task(planning_mode=true)` as `deferred`; bulk-flipped to `pending` via `mcp__fused-memory__commit_planning` after dependency wiring.

## Intra-batch dependency edges wired (19 total)

| From | To (depends on) | Rationale |
|---|---|---|
| 3537 (β) | 3536 (α) | inventory is the convention's spec |
| 3538 (γ) | 3536 (α) | template format coordinates with inventory rows |
| 3539 (δ) | 3536 (α) | auto-resolve channel rows named in inventory |
| 3539 (δ) | 3537 (β) | δ uses emit_typed + validatePayload helpers |
| 3539 (δ) | 3538 (γ) | δ writes per-channel specs from template |
| 3541 (ε) | 3536 (α) | warm-pool-event row in inventory |
| 3541 (ε) | 3537 (β) | uses convention helpers |
| 3541 (ε) | 3538 (γ) | per-channel spec instantiates template |
| 3543 (ζ) | 3536 (α) | solver-progress row in inventory |
| 3543 (ζ) | 3537 (β) | uses convention helpers |
| 3543 (ζ) | 3538 (γ) | per-channel spec instantiates template |
| 3545 (η) | 3536 (α) | fea-case-changed row in inventory |
| 3545 (η) | 3537 (β) | uses convention helpers |
| 3545 (η) | 3538 (γ) | per-channel spec instantiates template |
| 3547 (θ) | 3536 (α) | morph_stats RPC row in inventory |
| 3547 (θ) | 3537 (β) | uses convention helpers (RPC variant) |
| 3547 (θ) | 3538 (γ) | per-channel spec instantiates template (RPC variant) |
| 3550 (κ) | 3536 (α) | inventory path must exist before citing PRDs reference it |
| 3552 (μ) | 3536 (α) | contributor doc references inventory as source of truth |

## Cross-PRD edges to wire later (recorded as `metadata.cross_prd_dep_pending`, NOT wired in this session)

Per task-recovery brief: cross-PRD edges into `reify` tasks from this batch and the GR-024 → GR-016 back-edges are wired by the parent session's Phase 1.7. Below is the full set this batch surfaced:

| From (this batch) | To (other batch / existing) | Rationale |
|---|---|---|
| 3536 (α) | 3461 (GR-024 ν) | prose-staleness ordering: ν updates PRD §2.2 to name task ι (3458) as mode-shape-frame owner; α then reads §2.2 to populate inventory — mirror of the gr024 filing log's 3563→3593 edge (now 3536→3461). |
| 3458 (GR-024 ι) | 3536 (α) | buckling animator consumes canonical inventory |
| 3458 (GR-024 ι) | 3537 (β) | buckling animator consumes convention helpers |
| 3539 (δ) | C-05 fix-now (param-x-auto compile-pipeline wire) | δ's auto-resolve emitters require the orchestrator be wired into the compile pipeline (phase-3-files-synthesis.md fix-now list). |
| 3541 (ε) | warm-state-eviction M-010 | M-010's drain-translator is subsumed into ε per PRD §9 task ε recommendation. |
| 3543 (ζ) | 2965 | overlay component (pending; 2923 progressive framework is DONE) |
| 3545 (η) | 3026 | multi-load case GUI case-picker (multi-load-case-fea PRD) |
| 3545 (η) | multi-load-case-fea M-016 | upstream MultiCaseResult IPC type |
| 3547 (θ) | 2949 | mesh-morphing debug RPC chain (depends on 2948 → 2947) |

## DAG view (intra-batch + queued cross-PRD)

```
α(3536) ──┬──→ β(3537) ──┬──→ δ(3539)  [+ C-05 fix-now upstream]
          │              ├──→ ε(3541)  [+ warm-state M-010 subsumed]
          │              ├──→ ζ(3543)  [+ 2965 overlay]
          │              ├──→ η(3545)  [+ 3026, M-016]
          │              └──→ θ(3547)  [+ 2949 → 2948 → 2947]
          │
          ├──→ γ(3538) ──→ (consumed by δ/ε/ζ/η/θ via γ edge)
          ├──→ κ(3550)
          └──→ μ(3552)

ι(3548) — independent doc edit (strike PNv2 M-018 from gap-register)
λ(3551) — CANCELLED at filing time (superseded by GR-024 ι at 3458)

[upstream from GR-024 batch]
3461 (GR-024 ν) ──→ 3536 (α)   [prose-staleness ordering, pending Phase 1.7 wire]

[downstream into GR-024 batch]
3536 (α) ──→ 3458 (GR-024 ι)   [pending Phase 1.7 wire]
3537 (β) ──→ 3458 (GR-024 ι)   [pending Phase 1.7 wire]
```

## Supersession + side effects

- **GR-016 λ-equivalent (re-filed as task 3551)** — superseded at filing time by GR-024 task ι (3458 — "GUI mode-shape-frame emitter + BucklingPanel animator").
  - **Disposition (applied 2026-05-14):** `set_task_status(3551, cancelled, reopen_reason="Superseded at filing time by new GR-024 task ι (3458); see gr024-buckling-eigensolver-filing-log.md retroactive cleanup. Re-filed 2026-05-14 post-SIGABRT.")`.
  - This mirrors the original 2026-05-13 INV-2 retroactive cleanup pattern in `gr024-buckling-eigensolver-filing-log.md` lines 99-103. Per the revised `preferences_supersession_same_prd_only` INV-2 (one owner per piece, applied at filing time), the cancellation is done immediately rather than deferred.
  - 3458's `metadata.supersedes: [3573]` from the original GR-024 filing remains correct as audit trail (original GR-016 λ id was 3573); 3551 is the post-SIGABRT re-filing of the same bookmark.

## Cross-PRD edge audit (for procedural_runs_db_forensics integrity)

All cross-PRD reference task IDs verified to exist at filing time:

- `3458` (GR-024 ι) — verified via `get_task`. Status: `pending`. Title: "GUI mode-shape-frame emitter + BucklingPanel animator". Will edge into 3536 (α) + 3537 (β) once parent Phase 1.7 wires it.
- `3461` (GR-024 ν) — verified. Status: `pending`. Title: "gui-event-channel-inventory.md prose update (mode-shape-frame owner)". 3536 (α) will edge into 3461 once Phase 1.7 wires it (prose-staleness ordering).
- `2923` (FEA progressive-solve framework) — verified. Status: `done`. ζ (3543) prereq.
- `2965` (FEA solver progress overlay component) — verified. Status: `pending`. ζ (3543) prereq.
- `3026` (multi-load-case FEA #9 case-picker) — verified. Status: `pending`. η (3545) prereq.
- `2947` / `2948` / `2949` (mesh-morphing wiring chain) — all verified, all `pending`. θ (3547) prereq.

## Hand-back

- **12 tasks filed** (α, β, γ, δ, ε, ζ, η, θ, ι, κ, λ, μ); **11 at `pending`**, 1 at `cancelled` (λ=3551, superseded by GR-024 ι at filing time).
- **19 intra-batch dependency edges wired** via `add_dependency`.
- **9 cross-PRD edges queued** as `metadata.cross_prd_dep_pending` strings — to be wired by parent session's Phase 1.7. Notable: 3458→{3536,3537} and 3536→3461 (the GR-016/GR-024 mutual back-edges plus the prose-staleness order).
- **ID drift from original filing:** α 3563→3536, β 3564→3537, γ 3565→3538, λ 3573→3551 (cancelled), μ 3574→3552. δ/ε/ζ/η/θ/ι/κ are at new IDs only (originals lost without records).
- Orchestrator-side does **not** currently read `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata — substrate for the F-infra follow-up. Surface in resume-triage if relevant.
- Filing session log: this file.
