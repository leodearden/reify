# PRD — Warm-lane pool sizing, lifecycle-accretion audit & proactive dispatch admission

**Status:** active · version-agnostic infrastructure foundation · authored 2026-07-11 · durable-fix layer for the recurring warm-lane ENOSPC wedge (2026-07-10 esc-5078-2). Complements — does **not** duplicate — the in-flight acute fixes **task 5167** (terminal-task rebase-orphan lane reclaim) and **task 5168** (GC free-before-reseed ENOSPC deadlock), and the landed `warm-lane-pool-space-safety.md` PRD.
**Source:** live lane/GC telemetry audit (2026-07-11, this PRD's G6 investigation) + spawn brief `warm-lane-pool-sizing-audit.md`. Memory: `project_warm_lane_accretion_falsepreserve_rca`.
**Scope guard (inherited, load-bearing):** warmth never narrows the gate (verify-scope-contract C2). This PRD bounds the pool's *disk footprint* and makes accretion *observable + admission-gated*; it never trades verify coverage.

---

## 1. Goal — the warm-lane pool stops accreting to ENOSPC, by construction and observably

The XFS-reflink warm-lane pool (`/dev/loop29`, 6.0 TB, mounted `/home/leo/src/warm-lanes`) repeatedly fills to ~100% and wedges verify/merge for the whole fleet. The acute reclaim gaps are being closed point-wise (5167/5168). This PRD ships the **durable structural layer** so the pool's *steady-state* resident-divergent footprint tracks the *working set* (~12–15 lanes), not the *configured cap* (56), and so any drift toward the ceiling is visible long before ENOSPC.

**User-observable end state (consumers: the orchestrator dispatch/acquire path · the operator · every human/agent waiting on a build that would otherwise wedge at ENOSPC):**

| Axis | Today | With this PRD |
|---|---|---|
| Resident divergent lanes | grows to the count cap (47/56 resident, ~122 GB each ⇒ 93% full) | ≈ the ASSIGNED working set (~12–15); FREE lanes hold no divergent target |
| Accretion visibility | none until the disk-guard trips at the 50 GiB cliff (which *is* the wedge) | `warm-lane-audit.sh` reports resident/free/reclaimable/leaked/stale + projected headroom on demand and on a timer |
| Dispatch under disk pressure | allocates new divergent lanes straight into the hard floor → ENOSPC | throttles new-lane allocation at a **soft floor** (prefers reclaim/reuse) before the hard floor is reached |
| Pool capacity | fixed 6.0 TB image, hand-grown in incidents | budget-derived sizing + a supported online-grow operation (insurance, not the primary lever) |

All disk/footprint figures below are **measured, never frozen** into RED-test thresholds (G6, inheriting the parent PRDs' §9 convention): the sizing budget is recomputed from the *live* mean divergent footprint, and assertions test a *relation/direction* (resident-divergent ≤ budget; free recovers after reclaim), never a guessed GB constant.

## 2. Background — what the 2026-07-11 telemetry audit established

A live audit of the pool (`lslocks`, per-lane git state, GC journal history, `df`) established the accretion mechanism precisely — and corrected two premises in the brief:

- **The "N=24" baseline is stale.** `orchestrator.yaml` is now `max_concurrent_tasks: 48` + `spare_warm_lanes: 8` ⇒ **effective N = 56** task lanes + K merge-spec. Resident = 47 `_lane-*` (< 56) — the pool is **not over the count cap**. The binding constraint is **disk**: `56 × ~122 GB mean-divergent ≈ 6.8 TB > 6.0 TB`. 6 TB is *ample* for the ~15-lane working set (~1.8 TB + a ~108 GB base); the leak is that FREE lanes keep their divergent targets.
- **Only ~1 of 47 lanes was actually building** (a single live `cargo` lock, on `_lane-22`). The working set (mtime < 60 min) was ~12; the other ~35 lanes were **FREE-but-pinned**.
- **GC preserves released lanes as if they were live.** `scripts/warm-lane-gc.sh` `_is_reclaimable` preserves on `flock-held ∨ dirty-tracked ∨ ahead-of-*local*-main`, and **never consults dark-factory's FREE/ASSIGNED state** — so a *released* dirty/ahead lane is indistinguishable from a live one. GC resets clean-FREE lanes fine (`reset=8–17`/run historically) but `removed=0` **always** and `preserved` never shrinks (latest run: `reset=0 removed=0 preserved=53`). The preserved set *is* the accretion.

Three preserve failure modes, and where each is (or is not) already handled:

| Leak class | Evidence (2026-07-11) | Handled by |
|---|---|---|
| Terminal-task (done/cancelled) lane, tip a rebase-orphan ahead-of-main SHA | `_lane-40` task/4827 done = 768 GB; `_lane-42` task/5033 done = 404 GB (per task 5167) | **task 5167** (in-flight) |
| GC cannot reclaim at *true* ENOSPC (stages a reflink reseed before freeing) | `cp: … No space left` → `reset=N removed=0` | **task 5168** (in-flight; kept separate per brief) |
| `ahead=0 + dirty` lane, only dirty content is `data/queue/write_queue.db*`, backing task **pending** | `_lane-0`→5163, `_lane-1`→4152, `_lane-2`→4958, `_lane-8`→4935, `_lane-13`→4996, `_lane-23`→5162, `_lane-24`→5043, `_lane-31`→5166 — all **pending** | **this PRD** (Pillar C, filed as a fast-fix) — 5167 will **not** free these (non-terminal) |
| FREE lane retains a 122 GB divergent target between release and a re-acquire that never comes (working set 12 ≪ pool 56) | ~35 FREE-but-pinned lanes | **this PRD** (Pillar C eager release-thin) |

`data/queue/write_queue.db{,-shm,-wal}` is the fused-memory `DurableWriteQueue` runtime SQLite DB and is **tracked in git by mistake**: `.gitignore` ignores `.orchestrator/queue/*.db*` and `data/.orchestrator/queue/*.db*` but not the live path `data/queue/*.db*`. The orchestrator's auto `chore: save WIP before requeue rebase` commits swept it onto main; every agent re-dirties it at runtime. It false-triggers GC's dirty-WIP preserve on every lane it touches.

## 3. Sketch of approach — four pillars on the established reify-ships-primitives / dark-factory-wires-consumers seam

```
  A. AUDIT/OBSERVABILITY   warm-lane-audit.sh  ── assigned/free · terminal-status · recoverable · footprint · staleness · headroom
        (reify α)                │  feeds ▼                         ▲ consumed by operator + health timer
  B. SIZING                disk-budget formula + online-grow  ──────┤  (resident-divergent budget vs effective N)
        (reify β)                │                                  │
  C. LIFECYCLE      γ untrack write_queue.db (fast-fix, filed)   δ thin-warm-lane.sh (free-first)  ─┐
        (reify γ/δ + DF η)       │                                  η DF release_lane calls δ  ◀─────┘  resident-divergent ≈ working set
  D. ADMISSION      ε soft-floor headroom oracle + yaml knobs   ── θ DF dispatch throttles new-lane alloc at soft floor
        (reify ε + DF θ)                                             (before the 50 GiB hard floor / ENOSPC)
  ─ ζ END-TO-END INTEGRATION GATE (reify, B+H) proves: release-thin bounds resident-divergent; soft-floor throttles before hard floor
```

**Split (G4):**

| Side | Deliverables |
|---|---|
| **reify** (this batch) | α audit/telemetry script · β sizing-budget formula + `provision-warm-lane-fs.sh` online-grow · γ `write_queue.db` untrack (fast-fix, filed ahead of batch) · δ `thin-warm-lane.sh` free-first target-reclaim primitive · ε soft-floor headroom oracle + `orchestrator.yaml warm_lane_pool` admission knobs · ζ the integration gate · ι doc corrections |
| **dark-factory** (`dark_factory:` tasks, cross-project edges to reify's ε/δ contract) | η wire `thin-warm-lane.sh` into `release_lane` (eager release-thin) · θ consult ε's soft-floor oracle in the dispatch/acquire loop and throttle new-lane allocation (prefer reclaim/reuse) before the hard floor |

## 4. Resolved design decisions

- **D1 — write_queue.db untrack is a fast-fix, landed ahead of the batch, documented here** *(Leo, 2026-07-11)*. `git rm --cached data/queue/write_queue.db*` + gitignore `data/queue/*.db*`. Load-bearing, not hygiene-only: it is the sole pin on the ~12 `ahead=0`+residue-dirty **pending**-task lanes that 5167 (terminal-only) cannot free. Safe: `DurableWriteQueue.initialize()` does `self._data_dir.mkdir(parents=True, exist_ok=True)` (`durable_queue.py:143`) then SQLite creates the DB on open, so a fresh worktree self-provisions — no `.gitkeep`. Filed as a standalone high-priority task (γ) so relief does not wait on the batch.
- **D2 — Sizing is lifecycle-bound; growth is insurance, not the lever** *(Leo, 2026-07-11)*. 6.0 TB fits the working set (~15 × ~122 GB ≈ 1.8 TB) with large headroom **iff** FREE lanes do not retain divergent targets. The primary fix is therefore lifecycle (C) + admission (D); β grows `/dev/loop29` modestly (→ ~8 TB; host `data_lv` has 4.5 TB free) only as headroom insurance against the heavy tail (768 GB lanes) and the 2-generation base flip. Growing-first is rejected as the primary lever because it *masks* accretion (brief §"candidate fixes").
- **D3 — Eager thin at release, free-first** *(Leo, 2026-07-11)*. `release_lane` reclaims the divergent **target** immediately (η calls δ), rather than waiting for the next acquire's re-seed (which may never come). Always safe: acquire ALWAYS re-seeds from base (cow-seeding D10), so a FREE lane needs no warm target; the source-tree WIP lives on the task branch, not in `target/`. δ frees **before** staging any clone (free-first), composing with 5168's ordering fix so it never inherits the ENOSPC-deadlock. Net warmth is unchanged (acquire re-seeds regardless); only the idle-hold is eliminated.
- **D4 — Proactive soft-floor admission, above the existing hard floor** *(Leo, 2026-07-11)*. ε adds a *soft* free-space/inode threshold (> the 50 GiB hard floor) emitting a distinct backpressure signal; θ (DF) throttles NEW-lane allocation (prefers reclaim/reuse of a FREE lane) when free < soft-floor, so the pool never *approaches* the hard floor under normal load. The hard-floor `check → reclaim → requeue` path (space-safety ε) remains the last-ditch backstop. Modelled on `fleet-load-detector.sh` (reify signal → DF admission loop) and `warm-lane-disk-guard.sh` (the existing hard guard).
- **D5 — GC's preserve invariant for ASSIGNED / unrecoverable-WIP lanes is unchanged** *(analysis)*. This PRD does **not** loosen `_is_reclaimable` to reclaim live or genuinely-unrecoverable-WIP lanes. It makes the preserve invariant *correct by construction*: remove the residue that false-triggers it (γ), thin FREE lanes' targets at release before they linger (δ/η), and leave terminal-task ahead-of-main reclaim to 5167. What remains preserved is then only genuinely-live or genuinely-at-risk work.
- **D6 — Audit reuses the 4749 status-lookup seam** *(analysis)*. α resolves a lane's backing-task terminal status via the same `REIFY_LANE_LEAK_STATUS_CMD` seam task 4749 wired into `warm-lane-preflight.sh`'s leak detector — no new status-lookup plumbing; α is the richer, standalone, timer-friendly report the detector's one-line alarm could not be.
- **D7 — B+H: full contract (§9) + two-way boundary tests (§10)** *(G5)*. Load-bearing warm-lane infra crossing the reify↔DF seam, with a safety-critical action (reclaiming a lane's target must never destroy unrecoverable WIP). Signatures + boundary scenarios are specified up front so the integration gate (ζ) lands as a first-class task, not a starved afterthought.
- **D8 — No frozen numeric threshold** *(G6, inherited)*. The mean-divergent footprint (~122 GB today) and the 56-vs-6 TB arithmetic are *measured context*, not test constants. β's budget is recomputed from live `df`/`du`; ζ asserts *relations* (resident-divergent ≤ budget; free recovers post-reclaim; throttle fires below soft-floor), never a guessed GB/minute number.

## 5. Pre-conditions for activating

- **The pool is live** (`/dev/loop29` mounted, warm base at `base/target.gen.N`). ✔ (verified 2026-07-11).
- **4749 status-lookup seam present** (`REIFY_LANE_LEAK_STATUS_CMD` in `warm-lane-preflight.sh`). ✔ (task 4749 done) — α depends on it.
- **δ free-first ordering coordinates with 5168.** δ must free the target's bytes before staging any reseed; where 5168's `warm-lane-gc.sh` free-before-reseed fix lands the same primitive, δ reuses it rather than re-implementing. Soft ordering, not a hard block.
- **η/θ (dark-factory) depend cross-project on reify's ε/δ contract** (§9) — filed but gated on the reify primitives landing.
- No `.ri` substrate or grammar surface (§6).

## 6. Substrate verification (G3) — shell / XFS / systemd / git / dark-factory-Python; grammar gate N/A

Same as the parent `warm-lane-pool-cow-seeding.md` / `…-space-safety.md`: no `.ri` grammar surface, so the grammar gate / `prd-decompose-verify.mjs` workflow is **N/A**. G3 is discharged by direct host checks, all verified live 2026-07-11:

| Capability | Leaf | Evidence |
|---|---|---|
| `lslocks` / `flock -n` distinguishes ASSIGNED (live cargo/lane lock) from FREE | α | verified: only `_lane-22` held a live `cargo` lock among 47 lanes |
| backing-task status resolvable per lane (branch `task/NNNN` → status) | α | verified: `get_statuses` mapped every resident lane's branch to a status; 4749 seam (`REIFY_LANE_LEAK_STATUS_CMD`) present |
| recoverable-on-origin classification (`merge-base --is-ancestor` landed / `origin/<branch>` pushed) | α | verified: the live audit classified LANDED/PUSHED/NO per lane |
| per-lane divergent footprint measurable (`du` / `df`-delta) | α/β | verified: `du -sh` per lane (108–208 GB), `df` 5.6 TB used / 462 GB free |
| `data/queue/write_queue.db*` is tracked; queue self-creates its dir | γ | verified: `git ls-files data/queue/` = the 3 db files; `durable_queue.py:143` `mkdir(parents=True, exist_ok=True)` |
| free-first target removal frees bytes without staging a clone | δ | `rm -rf <lane>/target` frees the divergent extents directly; acquire re-seeds (cow-seeding D10) |
| loopback XFS online-grow (`fallocate` extend → `losetup -c` → `xfs_growfs`) | β | standard on the 6.x kernel; `xfsprogs` installed; host `data_lv` 4.5 TB free |
| `warm-lane-disk-guard.sh` extensible with a soft threshold + sentinel | ε | verified: the guard reads `df -B1 --output=avail,iavail`; `fleet-load-detector.sh` sentinel pattern is the template |
| DF `release_lane` / dispatch-acquire seam exists to wire | η/θ | space-safety β/ε wired `acquire_warm_lane`/`create_worktree`; `release_lane` in the same pool lifecycle |

No FAIL bindings. The capability manifest (`warm-lane-pool-sizing-lifecycle.capability-manifest.md`) records each leaf's binding to these host-check evidences.

## 7. Cross-PRD / cross-repo relationship (G4)

The genuine seam is reify-primitives ↔ dark-factory-wiring (no reciprocal "the other owns it"). This PRD *completes and corrects* the warm-lane series rather than introducing a new mechanism family.

| Other | Direction | Seam / relationship | Owner | Status |
|---|---|---|---|---|
| `warm-lane-pool-cow-seeding.md` (D9 sizing, §9.5 lifecycle) | this PRD **amends** it | corrects the stale "N=24" (now 48+8=56); adds eager-release-thin to the §9.5 release lifecycle (inv beyond "release retains nothing load-bearing"): release now *actively thins* | ι amends the record | amending |
| `warm-lane-pool-space-safety.md` (§11 capacity out-of-scope; §12 release-thin open-Q; ε admission) | this PRD **completes** it | brings the deferred capacity/sizing pillar in-scope (β); resolves its §12 "thin at release vs acquire" open question (D3: thin at release); extends its hard-floor admission (ε) with a proactive soft floor | this PRD; ι cross-links | completing |
| **task 5167** (terminal-task rebase-orphan reclaim) | this PRD **composes with** it | 5167 reclaims *terminal*-task ahead-of-main lanes; this PRD handles *non-terminal* residue-dirty lanes (γ) + eager release-thin (δ/η) + sizing + admission — disjoint domains | 5167 (in-flight) | composed-with |
| **task 5168** (GC free-before-reseed ENOSPC deadlock) | this PRD **references, does not fold** | δ's free-first target reclaim uses the same free-before-stage ordering; where 5168 lands it as a shared primitive, δ reuses it | 5168 (in-flight) | referenced |
| dark-factory `release_lane` / dispatch-acquire | DF **consumes** reify's δ/ε primitives | η calls `thin-warm-lane.sh` on release; θ consults the soft-floor oracle before allocation | **η/θ (dark_factory)**; depend cross-project on ε/δ | queued (this batch) |
| `fleet-load-detector.sh` (host-oversubscription detector) | ε **mirrors** its signal→DF-loop pattern | reify emits a headroom sentinel; DF's admission loop (θ) consumes it | prior art | pattern-reuse |

## 8. (no "why deferred" — this PRD is active)

Every reify leaf is shippable now. η/θ are DF-side, gated on the reify ε/δ contract. The §5 dependency set is the only sequencing.

## 9. Contract — seam signatures + invariants (the B+H §, D7)

All scripts follow the repo's stdout-contract convention (resolved value on stdout, diagnostics on stderr; mirror `warm-lane-disk-guard.sh` / `setup-worktree-debug-port.sh`).

### 9.1 Audit/telemetry — `scripts/warm-lane-audit.sh` (α)

```
warm-lane-audit.sh [--mount <worktrees_dir>] [--format table|json] [--status-cmd <cmd>]
  For each resident worktree under <mount> (lanes _lane-*/_spec-* and orphan dirs), emit:
    lane · role · ASSIGNED|FREE (flock -n -x <dir>.lock) · branch · backing-task-status
      (terminal|non-terminal|unknown, via REIFY_LANE_LEAK_STATUS_CMD — 4749 seam) ·
      recoverable (LANDED | PUSHED | ORPHAN) · divergent_gib (du or df-delta) · age_min (mtime)
    classification: LIVE | RECLAIMABLE (free ∧ (terminal ∨ recoverable ∨ residue-only-dirty)) |
                    LEAKED (free ∧ non-terminal ∧ unrecoverable-WIP ∧ stale) | PRESERVED-OK
  STDOUT: the table/json. Trailing one-line HEADROOM summary:
    "resident=N assigned=A free=F reclaimable=R leaked=L divergent_gib=D free_gib=G budget_gib=B"
  Exit 0 always (advisory/observability — NEVER fail-closed; it must not gate anything).
```
*Invariant A1:* read-only — `warm-lane-audit.sh` never mutates a lane (no reset/rm/reclaim); it observes. *Invariant A2:* `flock -n -x` probe is non-blocking and released immediately (never holds a lane lock across the walk). *Invariant A3:* a status-lookup failure degrades that lane to `unknown` (never aborts the report, never reclassifies as reclaimable).

### 9.2 Sizing budget + online-grow — `scripts/provision-warm-lane-fs.sh --grow` (β) + budget formula

```
provision-warm-lane-fs.sh --grow --size-gib <N> [--img <path>] [--mount <dir>]
  ONLINE grow of the mounted loopback XFS to <N> GiB (never shrink):
    fallocate -l <N>GiB <img>  →  losetup -c <loopdev>  →  xfs_growfs <mount>
  IDEMPOTENT: if the FS is already ≥ <N> GiB → print <mount>, exit 0 (no-op).
  Guards: refuse if <N> < current size (P3 no-shrink); refuse if <img> not the mounted backing file.
  STDOUT: the resolved mount dir. STDERR: diagnostics.
budget formula (documented in orchestrator.yaml + β doc, recomputed from LIVE measurement — NOT frozen):
  resident_divergent_budget_gib = floor( (image_free_gib − base_gib − 2×base_gib_flip_reserve) / safety )
  effective_N (task) = max_concurrent_tasks + spare_warm_lanes ; K = _MERGE_AHEAD_BOUND
  ADVISORY relation the audit/admission assert: measured_resident_divergent_gib ≤ resident_divergent_budget_gib
```
*Invariant P3:* grow is monotone (never shrinks a populated image). *Invariant P4:* the budget is a *derived, recomputed* quantity, never a hardcoded lane-count or GB constant frozen into a test (G6).

### 9.3 Free-first target-reclaim primitive — `scripts/thin-warm-lane.sh` (δ)

```
thin-warm-lane.sh <lane_dir>
  PRECONDITION (caller-asserted): lane_dir is FREE (no live consumer) and under the warm-lane mount, ≠ base.
  FREE-FIRST: rm -rf <lane_dir>/target   (frees the divergent extents DIRECTLY — no reseed staged,
    so it makes progress even at true ENOSPC; acquire_lane always re-seeds from base — cow-seeding D10).
  Optional --reseed: after freeing, seed a thin base clone (seed-warm-lane.sh --fresh-checkout) —
    default OFF (leave empty; acquire re-seeds on next assignment).
  STDOUT: nothing on success (or lane_dir). STDERR: diagnostics. Exit 0 on freed.
```
*Invariant T1:* only `target/` is removed — the source tree (branch, uncommitted WIP) is **never** touched; unlanded source WIP survives on the branch and is restored by `reset_lane` on re-acquire. *Invariant T2:* free-first — bytes are freed before any clone is staged (never inherits 5168's ENOSPC deadlock). *Invariant T3:* caller holds the lane's `flock -x` across the call (inv.2 one-consumer-per-lane); δ refuses if it cannot acquire it.

### 9.4 Soft-floor headroom oracle + knobs — `scripts/warm-lane-disk-guard.sh check --soft` (ε)

```
warm-lane-disk-guard.sh check [--soft]           (extends the existing hard-floor guard)
  hard floor (existing): free_bytes < min_free_gib OR free_inodes < min_free_inodes → exit 75 (EX_TEMPFAIL)
  --soft: additionally, min_free_gib ≤ free < soft_free_gib (or inode analogue) →
    exit <SOFT_CODE> + stdout sentinel "@@REIFY_WARM_LANE_SOFT_PRESSURE@@ free_gib=<G> budget_gib=<B>"
    (distinct from the hard 75 so DF can throttle-not-requeue)
orchestrator.yaml warm_lane_pool: (DECLARE the currently-missing knobs; config-test asserts presence)
  min_free_gib, min_free_inodes            (hard floor — today only defaulted in the script)
  soft_free_gib, soft_free_inodes          (NEW — the proactive throttle floor; soft > hard)
```
*Invariant E1:* soft > hard (a soft floor below the hard floor is a config error → exit 2, loud). *Invariant E2:* the hard-floor exit-75 contract is unchanged (space-safety ε still requeues at the cliff); `--soft` only *adds* a distinct earlier signal. *Invariant E3:* fail-closed measurement (df error) maps to the hard path (75), never a false "healthy".

### 9.5 Pool lifecycle additions (consumed by dark-factory η/θ)

Extends `warm-lane-pool-cow-seeding.md` §9.5. The DF wiring implements:

```
release_lane(lane_dir)                                   (η — amends the D10 release)
    ASSIGNED → FREE; THEN thin-warm-lane.sh <lane_dir>   (eager free-first target reclaim; D3)
    (was: "retains NOTHING load-bearing" — now also actively frees the divergent target)
dispatch/acquire admission                               (θ — new soft-floor throttle)
    before allocating a NEW lane: warm-lane-disk-guard.sh check --soft
      soft-pressure → prefer reclaiming/reusing a FREE lane; if none, apply backpressure
        (defer the dispatch) rather than growing resident-divergent toward the hard floor
    hard floor unchanged: space-safety ε check → reclaim → requeue exit-75
```
**Invariants (additive to cow-seeding §9.5 inv.1–9):**
10. **Release-thin safety** — `release_lane`'s thin removes only `target/`; the lane's branch + uncommitted source WIP are untouched and recoverable (T1). A lane is never thinned while ASSIGNED (T3).
11. **Soft-floor precedence** — soft-floor throttling is *backpressure* (defer dispatch), never an escalation or a fault; only the *hard* floor requeues (exit-75) and only genuine seed/worktree faults escalate (space-safety D2/D3 unchanged).
12. **Observability is non-gating** — the audit (α) never blocks dispatch, reclaim, or merge; it informs. Only the guard (ε) gates.

## 10. Boundary-test sketch (two-way; the B+H §, closes G2 for ζ/η/θ)

| # | Scenario | Preconditions | Postconditions (asserted) | Faces |
|---|---|---|---|---|
| B1 | Audit classifies correctly | fixture pool: one live-flock lane, one terminal-task ahead-of-main lane, one pending residue-only-dirty lane, one leaked stale-detached lane | audit labels them LIVE / RECLAIMABLE / RECLAIMABLE / LEAKED; headroom line counts match | reify (α) |
| B2 | Audit is read-only | any pool state | after `warm-lane-audit.sh`, every lane's `target/` + git state is byte-identical (no mutation) | reify (α) |
| B3 | Online grow is monotone + idempotent | mounted 6 TB image | `--grow --size-gib 8192` grows (`df` shows ~8 TB); a 2nd run no-ops; `--size-gib 4096` refuses (no-shrink) | reify (β) |
| B4 | **Free-first thin frees a divergent lane** | FREE lane with a large divergent `target/` | `thin-warm-lane.sh` frees the divergent extents (`df` recovers ≈ the lane's footprint); source tree + branch untouched; **no reseed staged** (progresses even with a near-full fixture) | reify (δ) |
| B5 | Thin never touches assigned/source | ASSIGNED lane (flock held) OR a lane with uncommitted source WIP | δ refuses the assigned lane (T3); on a FREE lane it removes only `target/`, leaving uncommitted source WIP intact (T1) | reify (δ) |
| B6 | Soft floor fires above hard floor | free between soft and hard floors | `check --soft` emits the soft sentinel + distinct exit; `check` (hard) still exits 0; below hard floor both trip 75 | reify (ε) |
| B7 | Config knobs declared | orchestrator.yaml | `test_warm_lane_pool_config.sh` asserts `min_free_gib/inodes` + `soft_free_gib/inodes` present + soft>hard | reify (ε) |
| B8 | **Integration: release-thin bounds resident-divergent** | seed 3 lanes divergent, release 2 | after release, resident-divergent ≈ the 1 still-assigned (freed ≈ 2× footprint); audit headroom reflects it | reify (ζ) |
| B9 | Eager release-thin in the pool | DF releases a lane after a dispatched agent finishes | orchestrator journal shows the released lane's target thinned promptly; next acquire re-seeds warm (delta vs cold) | dark-factory (η) |
| B10 | Soft-floor dispatch throttle | pool free between soft and hard floors, a new dispatch arrives | DF prefers a FREE-lane reclaim/reuse or defers; it does **not** allocate a new divergent lane toward the hard floor; no ENOSPC | dark-factory (θ) |

ζ (reify integration gate) realizes B1–B8; η realizes B9; θ realizes B10.

## 11. Decomposition plan — task DAG with observable signals (G2)

Greek labels; task IDs assigned at decompose. All disk/footprint signals are *measured direction + recorded delta*, never a frozen constant (G6, D8).

- **γ — reify · untrack `data/queue/write_queue.db*` (the fast-fix).** ALREADY FILED as a standalone high-priority task (ticket `tkt_0RR4NPSDRP7X9CFWQY7DRY8091`, 2026-07-11) so relief lands ahead of the batch. **Signal:** `git ls-files data/queue/` empty; `git check-ignore data/queue/write_queue.db` prints the path; a lane `git status` no longer shows the DB triple as dirty tracked → a subsequent `warm-lane-gc.sh reclaim` resets the previously-pinned pending-task landed lanes (reset↑, preserved↓). *Referenced here; not re-filed in this batch.*
- **α — reify · `scripts/warm-lane-audit.sh` (audit & telemetry).** Per §9.1, reusing the 4749 status seam (D6). *Intermediate.* **Signal:** on the live pool prints the assigned/free/reclaimable/leaked/stale table + headroom line; `tests/infra/test_warm_lane_audit.sh` asserts correct classification of a seeded fixture (B1/B2). *Modules:* `scripts/`, `tests/infra/`.
- **β — reify · sizing-budget formula + `provision-warm-lane-fs.sh --grow`.** Per §9.2. *Intermediate.* **Signal:** `--grow --size-gib 8192` grows the mounted image online (`df` shows the new ceiling), idempotent + no-shrink (B3); the budget formula + knobs documented (`orchestrator.yaml`, β doc). *Modules:* `scripts/`, `orchestrator.yaml`, `docs/`.
- **δ — reify · `scripts/thin-warm-lane.sh` (free-first target reclaim primitive).** Per §9.3. *Intermediate* (unlocks DF η). **Signal:** `tests/infra/test_thin_warm_lane.sh` — frees a divergent FREE lane's target (`df` recovers ≈ its footprint) with no reseed staged; refuses an assigned lane; leaves source WIP intact (B4/B5). *Modules:* `scripts/`, `tests/infra/`.
- **ε — reify · soft-floor headroom oracle + `orchestrator.yaml` admission knobs.** Per §9.4. *Intermediate* (the DF-facing contract; unlocks DF θ). **Signal:** `check --soft` emits the soft sentinel between the floors while hard `check` stays green (B6); `test_warm_lane_pool_config.sh` asserts the declared `min_free_*`/`soft_free_*` knobs + soft>hard (B7). *Modules:* `scripts/`, `orchestrator.yaml`, `tests/infra/`.
- **ζ — reify · END-TO-END INTEGRATION GATE (the C-as-integration-gate leaf).** `tests/infra/test_warm_lane_sizing_lifecycle.sh`: seed divergent lanes → thin-on-release → assert resident-divergent tracks the assigned set + audit headroom reflects it (B8), and the soft floor fires before the hard floor on a shrinking-free fixture (B6 end-to-end). *Leaf.* *(depends_on α, β, δ, ε; soft-dep on γ for the residue-clean baseline.)* **Signal:** the harness runs green in CI and records the measured free-recovered-on-release delta. *Modules:* `tests/infra/`, `scripts/`.
- **ι — reify · companion doc corrections.** Amend `warm-lane-pool-cow-seeding.md` (N=24→56; §9.5 release now thins) + `warm-lane-pool-space-safety.md` (§11 capacity in-scope here; §12 release-thin resolved; ε soft-floor extension) + CLAUDE.md warm-lane invariants (audit + soft-floor admission) + a `docs/notes` audit runbook. *Leaf.* *(depends_on ζ — prove before recording.)* **Signal:** the record reflects the landed pillars; the "N=24" references are corrected. *Modules:* `docs/`, `CLAUDE.md`.
- **η — dark-factory · wire eager release-thin into `release_lane`.** Call `thin-warm-lane.sh` on release (free-first), per §9.5. *Leaf (DF-side).* *(depends_on δ + ε contract, cross-project.)* **Signal:** orchestrator journal shows a released lane's target thinned promptly + resident-divergent bounded to the assigned set; next acquire re-seeds warm (B9). *Repo:* dark-factory.
- **θ — dark-factory · proactive soft-floor dispatch throttle.** Consult ε's `check --soft` in the dispatch/acquire loop; on soft-pressure prefer FREE-lane reclaim/reuse or defer, before the hard floor. *Leaf (DF-side).* *(depends_on ε + α, cross-project.)* **Signal:** under a soft-pressure fixture the journal shows dispatch deferring/reclaiming rather than allocating a new divergent lane; the pool never reaches the hard floor (B10). *Repo:* dark-factory.

**DAG:** γ (filed, ahead) ; {α, β, δ, ε} independent ; {α, β, δ, ε} → ζ → ι ; δ + ε → η ; ε + α → θ. η and θ are `dark_factory:` tasks with cross-project edges to δ/ε.

## 12. Out of scope

- **The terminal-task rebase-orphan reclaim (task 5167)** and **the GC free-before-reseed ENOSPC deadlock (task 5168)** — in-flight point-fixes; this PRD composes with them, does not re-implement (brief §"Related — do NOT fold in").
- **Loosening GC's preserve invariant for ASSIGNED or genuinely-unrecoverable-WIP lanes** — forbidden (D5). This PRD removes false-triggers and thins FREE lanes, never reclaims live/at-risk work.
- **Narrowing the merge-gate scope** — forbidden (C2 / §1).
- **Carved-LV / btrfs substrate** — deferred (cow-seeding D2); loopback XFS-reflink stays; β only grows it.
- **Lowering `max_concurrent_tasks`** as the sizing lever — rejected (D2: lifecycle-bound, not throughput-throttled); it remains an operator knob if the working set itself outgrows disk.
- **The in-engine warm-state compute-node eviction pool** (`warm-state-eviction.md`) — a different pool.

## 13. Open questions (tactical — surfaced, not blocking)

1. **Soft-floor threshold value** (`soft_free_gib`). Start generous (e.g. a few × the mean divergent footprint above the 50 GiB hard floor) so throttling engages with room to reclaim; tune post-deploy from audit headroom history. Decide during ε.
2. **Grow target size** for β's insurance bump (→ ~7–8 TB?). Validate against the heavy-tail (768 GB) lanes + the 2-generation base-flip reserve; size conservatively from live `du`. Decide during β / with the operator.
3. **Audit timer cadence** — run `warm-lane-audit.sh` on a systemd timer (like `reify-warm-base-health`) and/or emit its headroom line into the GC sweep log for trend history. Decide during α/ι.
4. **`--reseed` default for δ** — leave targets empty after release-thin (acquire re-seeds) vs eagerly re-seed a thin base clone so a re-acquire is instant. Default empty (§9.3); revisit if re-acquire latency measurably bites. Decide during η.
5. **Does θ's throttle need a fleet-load coupling** — should soft-floor disk pressure compose with `fleet-load-detector.sh`'s CPU/PSI admission into one dispatch-admission decision, or stay an independent axis? Decide during θ (dark-factory).
