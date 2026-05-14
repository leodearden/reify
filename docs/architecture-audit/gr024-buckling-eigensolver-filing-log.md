<!-- 2026-05-14 RECOVERY AUDIT TRAIL
This filing log was authored 2026-05-13. The task IDs referenced below (3491-3594, 3503/3504/3508/3510/3512, 3563-3574, etc.) were LOST in the 2026-05-13 fused-memory SIGABRT.
The DAG was re-filed 2026-05-14 with NEW task IDs:
GR-024 buckling-eigensolver DAG: π=3449 (was 3576), α=3450 (was 3577), β=3451 (was 3582), γ=3452 (was 3583), δ=3453 (was 3585), ε=3454 (was 3586), ζ=3455 (was 3587), η=3456 (was 3588), θ=3457 (was 3589), ι=3458 (was 3590), κ=3459 (was 3591), μ=3460 (was 3592), ν=3461 (was 3593), ξ=3462 (was 3594). Cross-PRD edges (Phase 1.7): ε→CN-η (3454→3426), κ→CN-ι (3459→3428), η→3005, α→SIR-α (3450→3540), ι→GR-016 α/β (3458→3536/3537), GR-016 α→GR-024 ν (3536→3461). INV-1 back-edges: 3005→π (3005→3449), CN-η→π (3426→3449).
The body of this log is preserved as historical record. Use docs/task-recovery-2026-05-13/id-map.json as the canonical translation table for live work.
-->

# GR-024 buckling-eigensolver PRD §13 DAG — filing log

Session: 2026-05-13 decompose-mode pass over `docs/prds/v0_5/buckling-eigensolver.md` (commit `8059aa59ba`).

Source PRD: `docs/prds/v0_5/buckling-eigensolver.md` — resolves GR-024 / cluster C-22.
Skill: `.claude/skills/prd/` decompose mode.

## Gate re-walk summary

| Gate | Result | Notes |
|---|---|---|
| G1 (consumer named) | PASS | All 11 introduced mechanisms have named consumers (user .ri code, BucklingPanel, stdlib helpers, ComputeNode dispatch, downstream PRDs). |
| G2 (user-observable leaf) | PASS | All leaves (ε, ζ, η, θ, ι, κ, μ, ν, ξ) declare CLI / viewport / git-diff signals. Intermediates (α, β, γ, δ) each name a downstream consumer. |
| G3 (grammar verified) | PASS (with prereq filed) | Default values in `fn` parameters (`options : BucklingOptions = BucklingOptions.default`) do NOT parse in current grammar. Filed as task π (grammar prereq). Other novel-looking forms verified to parse via tree-sitter fixtures at `/tmp/prd-gate-fixtures/buckling-{1b,2b,3b,5,7,10}-*.ri`. |
| G4 (cross-PRD seam ownership) | PASS | PRD §10 explicitly states "no reciprocal 'the other owns it' pairs surfaced." GR-016 owns the channel contract; this PRD owns the implementation slice (clean split). |
| G5 (B + H for high-stakes) | PASS | PRD §13 declared B+H. Cross-crate blast radius = 6, mechanism count ~12, high-stakes seams (FEA + ComputeNode + GUI event channel + GR-001). §9 boundary-test sketch faces both ways per H. Integration-gate signals at each phase (δ Euler analytic; ε CLI cache; ι Playwright). |
| META | PASS | No design-level open questions remain. §14 lists tactical open questions only (Cholesky factor reuse, Lanczos restart strategy, animation phase shape, etc.) — each annotated with a suggested resolution and a deciding-task pointer. |

## Task IDs assigned

| Letter | Task ID | Title | Prereqs (task IDs) |
|---|---|---|---|
| π | 3576 | Grammar: fn_param default values + lowering wire | (none) |
| α | 3577 | Stdlib structure_defs (BucklingOptions/Mode/Result/MCBR) | π (3576), SIR-α (3503) |
| β | 3582 | eigensolve.rs shift-invert Lanczos + dense fallback | (none) |
| γ | 3583 | P1-tet K_g element kernel + global assembly + shell/hex/wedge stubs | β (3582) |
| δ | 3585 | solve_buckling_kernel — pre-stress → K_g → eigensolve → mode-shape | β (3582), γ (3583) |
| ε | 3586 | fn solve_buckling stdlib + @optimized trampoline + helpers + CLI smoke | α (3577), δ (3585), π (3576), CN-η (3497) |
| ζ | 3587 | Shell + hex/wedge stub diagnostics surfaced through trampoline | ε (3586) |
| η | 3588 | MultiCaseBucklingResult + solve_buckling_load_cases + envelope helpers | ε (3586), π (3576), 3005 (solve_load_cases) |
| θ | 3589 | Significance filter integration at solver::buckling boundary | ε (3586) |
| ι | 3590 | GUI mode-shape-frame emitter + BucklingPanel animator | ε (3586), GR-016 α (3563), GR-016 β (3564) |
| κ | 3591 | Persistent-cache hookup for buckling | ε (3586), CN-ι (3499) |
| μ | 3592 | parent structural-stability-buckling.md prose update | (none) |
| ν | 3593 | gui-event-channel-inventory.md prose update + cancel deferred task 3573 | (none) |
| ξ | 3594 | gap-register.md GR-024 cross-link | (none) |

All filed via `mcp__fused-memory__submit_task(planning_mode=true)`. All filed as `deferred`; bulk-flipped to `pending` in a single `set_task_status` call after dependency wiring.

## Dependency edges added (22 total — includes 2026-05-13 retroactive cleanup)

**Intra-batch (13):**

| From | To (depends on) | Rationale |
|---|---|---|
| 3577 (α) | 3576 (π) | structure_defs need grammar work landed for downstream fn-param defaults |
| 3583 (γ) | 3582 (β) | Euler-sanity test in γ exercises eigensolve |
| 3585 (δ) | 3582 (β) | kernel slice wraps eigensolve |
| 3585 (δ) | 3583 (γ) | kernel slice consumes K_g assembly |
| 3586 (ε) | 3577 (α) | stdlib fn declared against structure_defs |
| 3586 (ε) | 3585 (δ) | trampoline wraps solve_buckling_kernel |
| 3586 (ε) | 3576 (π) | fn signature has default param value |
| 3587 (ζ) | 3586 (ε) | extends trampoline error path |
| 3588 (η) | 3586 (ε) | multi-case loops single-case dispatch |
| 3588 (η) | 3576 (π) | multi-case fn signature has default param value |
| 3589 (θ) | 3586 (ε) | filter applies at trampoline boundary |
| 3590 (ι) | 3586 (ε) | GUI animator triggers on BucklingResult |
| 3591 (κ) | 3586 (ε) | cache hooks ComputeNode dispatch |

**Cross-PRD (6 at initial filing + 3 retroactive = 9):**

| From | To (depends on) | Rationale |
|---|---|---|
| 3577 (α) | 3503 (SIR-α) | structure_def runtime ctors via Value::StructureInstance |
| 3586 (ε) | 3497 (CN-η) | first @optimized stdlib trampoline; solver::buckling sibling registration |
| 3588 (η) | 3005 (solve_load_cases) | per-case dispatch pattern precedent |
| 3590 (ι) | 3563 (GR-016 α) | canonical inventory at docs/gui-event-channels.md |
| 3590 (ι) | 3564 (GR-016 β) | convention helpers (emit_typed, validatePayload, mockTauriEvent) |
| 3591 (κ) | 3499 (CN-ι) | persistent-cache surface for ComputeNode dispatch |
| **3005** | **3576 (π)** | **fn-param-defaults grammar work; 3005's pseudo-code (`options : ElasticOptions = .default`) needs this to lower (mirror INV-1 back-edge added 2026-05-13)** |
| **3497** | **3576 (π)** | **same — 3497's stdlib decl needs fn-param defaults (mirror INV-1 back-edge added 2026-05-13)** |
| **3563 (GR-016 α)** | **3593 (ν)** | **GR-016's canonical inventory file `docs/gui-event-channels.md` reads PRD §2.2 to determine owning slice tasks per channel. PRD §2.2 currently names task λ (mapped to now-cancelled 3573) as owner of `mode-shape-frame`. ν's prose update points the row at this PRD's task ι (3590) instead — must land before 3563 reads §2.2 (added 2026-05-13)** |

Cross-PRD edges set per [[preferences_cross_prd_deps_real_edges]] (2026-05-12 reversal) + [[preferences_supersession_same_prd_only]] INV-1 mirror direction (2026-05-13 revision: every consumer of a foundation task edges into it, regardless of where the consumer was filed).

## DAG view

```
π(3576) ──┐
           ├─→ α(3577) ───┐
SIR-α(3503)─┘             │
                          │
β(3582) ──┬─→ δ(3585) ────┤
γ(3583) ──┘               │
                          │
CN-η(3497) ───────────────┤
                          │
                          ├─→ ε(3586) ─┬─→ ζ(3587)
                                       ├─→ η(3588) ←── 3005, π(3576)
                                       ├─→ θ(3589)
                                       ├─→ ι(3590) ←── 3563, 3564 (GR-016 α/β)
                                       └─→ κ(3591) ←── CN-ι(3499)

μ(3592), ν(3593), ξ(3594) — independent doc edits
```

## Supersession + side effects (updated 2026-05-13 retroactive cleanup)

- **GR-016 task λ (task 3573, deferred bookmark "mode-shape-frame channel + producer")** — superseded by this PRD's task ι (3590).
  - **Disposition (applied 2026-05-13):** `set_task_status(3573, cancelled, reopen_reason="Superseded by buckling-eigensolver task ι (3590) …")`. The bookmark's activation trigger ("when buckling lands in v0.5+") fired during this session.
  - Per the revised [[preferences_supersession_same_prd_only]] INV-2 (one owner per piece, applied at filing time), the cancellation was done at filing time rather than deferred to ν task acceptance.
  - Task ν (3593) is now purely the GR-016 PRD prose update; its description was updated 2026-05-13 to drop the side-effect-cancel obligation.
  - 3590's `metadata.supersedes: [3573]` is retained as audit-trail.

## Cross-PRD edge audit (for [[procedural_runs_db_forensics]] integrity)

All cross-PRD prereq task IDs verified to exist at filing time:

- `3497` (CN-η) — verified via `get_task`. Status: `pending`. Description references this PRD's solver::buckling as a sibling consumer.
- `3499` (CN-ι) — verified. Status: `pending`. Description includes the persistent-cache surface API this batch's κ task consumes.
- `3503` (SIR-α) — verified. Status: `in-progress`. Description references compute-node-contract.md §8 task η (3497) and this PRD's α as downstream consumers.
- `3005` (solve_load_cases) — verified. Status: `pending`. Pre-existing multi-load-case-fea PRD task.
- `3563` (GR-016 α) — verified via `get_task`. Status: `pending`. Canonical inventory document.
- `3564` (GR-016 β) — verified. Status: `pending`. Convention helpers (emit_typed, validatePayload, mockTauriEvent).

## Grammar fixtures committed

Verification fixtures used in the G3 walk live at `/tmp/prd-gate-fixtures/buckling-*.ri` (session-scratch; not committed). Outcomes:

| Fixture | Form | Result |
|---|---|---|
| buckling-1b-options.ri | `structure def BucklingOptions { param x : T = v }` | PASS |
| buckling-2b-result.ri | `structure def Mode { param eigenvalue : Real }` + `structure def BucklingResult { param modes : List<Mode> }` | PASS |
| buckling-3b-mcbr.ri | `structure def MultiCaseBucklingResult { param cases : Map<String, BucklingResult> }` | PASS |
| buckling-5-annot.ri | `@optimized("...") fn name(...) -> T { body }` (prefix-position annotation) | PASS |
| buckling-7-helpers.ri | bare `fn` helpers with `List<Mode>` field access + indexing | PASS |
| buckling-10-preannot.ri | `@optimized("...") fn name(...) -> T { body }` (full shape) | PASS |
| buckling-6-default.ri | `fn name(options : T = default)` (default value on fn param) | **FAIL** → filed as task π (3576) |
| buckling-8-nobody.ri | body-less `fn` declaration | FAIL (PRD-prose informality; actual stdlib `.ri` ships with body via task α/ε) |
| buckling-9-postannot.ri | `fn name(...) -> T @optimized(...) { body }` (post-position annotation) | FAIL (not used — PRD §4 prose ambiguity; canonical form is prefix annotation per existing compiler annotations.rs convention) |

The only blocking failure was buckling-6-default.ri → task π. Task ε (3586) is explicitly gated on π landing before its `.ri` source can be authored with the default-value form.

## Hand-back

- 14 tasks filed (π, α, β, γ, δ, ε, ζ, η, θ, ι, κ, μ, ν, ξ).
- 22 dependency edges wired (13 intra-batch, 9 cross-PRD including 3 retroactive edges: 3005→3576 + 3497→3576 INV-1 mirror, and 3563→3593 to defuse the §2.2 prose-staleness hazard).
- All 14 batch tasks at `pending`.
- Task 3573 (GR-016 λ bookmark) cancelled at filing time per revised INV-2; ν task (3593) is now purely a prose update.
- Filing session log: this file.
- Orchestrator-side does **not** currently read `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata — substrate for the F-infra follow-up (tasks 3519/3520/3521/3522/3523/3524/3525).

## Retroactive cleanup applied 2026-05-13

After this session's initial filing, [[preferences_supersession_same_prd_only]] was revised to add three pending-task invariants (INV-1 deps explicit / INV-2 single owner / INV-3 chain to user-visible). Two violations in the original filing were identified and fixed:

| Violation | Original state | Fix applied |
|---|---|---|
| INV-1 mirror | 3576 (π, grammar) gates downstream 3005 + 3497 (their pseudo-code uses fn-param defaults), but the back-edges were missing | `add_dependency(3005, 3576)` + `add_dependency(3497, 3576)` |
| INV-2 overlap | 3573 (GR-016 λ bookmark) still nominally owned mode-shape-frame channel + producer; 3590 (ι) also owned it; cancellation deferred to ν acceptance | `set_task_status(3573, cancelled, reopen_reason=…)` at filing time; 3593 (ν) description updated to drop side-effect-cancel obligation |
| INV-1 prose-staleness | 3563 (GR-016 α — inventory file creation) reads GR-016 PRD §2.2 to populate `docs/gui-event-channels.md`; §2.2 names task λ (mapped to cancelled 3573) as `mode-shape-frame` owner. Without ordering, 3563 could write a stale row | `add_dependency(3563, 3593)` — ν's PRD prose update lands first, points §2.2 at task ι (3590); 3563 then reads the corrected prose |
