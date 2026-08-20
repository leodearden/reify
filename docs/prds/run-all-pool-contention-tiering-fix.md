# PRD — `run_all.sh` pool-contention tiering fix + Phase-1 robustness

**Status:** author-complete (2026-07-06). Decompose-ready — **stop here for Leo's review.** L0 is the
scheduler-resume gate: it may be pulled forward and landed **before** the rest is decomposed (§9 priority).
**Slug:** `run-all-pool-contention-tiering-fix` · **Milestone:** version-agnostic verify-pipeline infra (root `docs/prds/`).
**Corrects/extends:** `docs/prds/run-all-host-infra-partition.md` (Part A shipped the H2 concurrent pool this PRD
re-tiers and hardens) · **Companions:** `docs/prds/cpu-load-admission-control.md` (the dispatch-admission axis, §7
L3b flag), `docs/prds/verify-admission-wait-clock-stop.md` (the clock-stop seam this PRD deliberately does **not**
touch).

This PRD closes the recurring **run_all pool-semaphore over-serialization** escalation cluster
(esc-5027/5037/5029/3848/5113/5091, + pool-hang halves of 5052/5020). It is a **reify-local, independently
shippable** fix; the one cross-repo item (L3b dispatch-admission) is flagged, not owned here.

---

## 0. Scope & consumers (G1)

**Named consumers (no orphan producers):**

| Mechanism | Consumer / enforcement point |
|---|---|
| `role==merge`-gated full-suite `run_all.sh` plan line (L0a) | `scripts/verify.sh build_plan`; fired by every `DF_VERIFY_ROLE=merge` verify (`hooks/pre-merge-commit`, the DF merge-verify command) — the single-flight tier |
| Per-task selective-infra path (L0a) | `scripts/verify.sh` `select_infra_tests()` on `role==task` `--include-infra` verifies |
| Broadened select map (L0b) | `scripts/verify-pipeline-infra-tests.txt`; read by `select_infra_tests()` |
| Merge-plan/task-plan drift-guard (L0a) | a new `tests/infra/` meta-test; the verify pipeline infra step |
| Bounded worker pool + fork-EAGAIN guard (L1) | `tests/infra/run_all.sh` Phase 1 (every run_all caller) |
| Single-writer progress heartbeat (L2) | `tests/infra/run_all.sh` Phase 1 → the live verify stream (operators + DF telemetry) |
| Host-global-lock drift-guard (L3a) | a new `tests/infra/` meta-test |
| DF dispatch-admission companion (L3b) | **dark-factory** scheduler (flagged; out of reify scope) |

**User-observable surface (operator-facing, G2):**
1. Under a burst of M concurrent task lanes, **no per-task lane runs the full 103-test pool** — each runs only its
   diff-selected infra subset (0–N tests); `verify.sh --print-plan` on a `role=task` `--scope branch --include-infra`
   plan contains **no** `run_all.sh` line.
2. The **merge-role** plan (`DF_VERIFY_ROLE=merge`) contains the full `run_all.sh` line; the full infra suite runs
   once, single-flight, at the authoritative gate — closing the current gap where merge gates infra **nowhere**.
3. A `run_all.sh` Phase-1 run has a **≤W=N process footprint** (not 103+ parked subshells) and emits a live
   `INFO: run_all.sh pool progress: X/Y complete, elapsed Ns` line at least every `REIFY_RUN_ALL_PROGRESS_SECS`
   — a contended/slow pool is **attributable on the live stream**, not a silent black box.
4. A transient `fork()` EAGAIN degrades a single pool member, it does **not** abort the suite (esc-3848 class gone).
5. The byte-identical output contract (`=== Summary:`, bare `FAILED <names>`, `--- Running:`, `RESULT:`, FLAKY)
   is **unchanged** (the drift-guards and DF `^FAILED\s` classifier stay green).

---

## 1. Problem & premise (G6 record)

`scripts/verify.sh` runs the full 103-member `run_all.sh` pool as a `timeout --kill-after=60 30m` plan line
(`verify.sh:1350`) **only when `INCLUDE_INFRA=1`**, which is set **only** by the `--include-infra` flag
(`verify.sh:442`; default `0` at `:420` — never forced by `role=merge`, `scope=all`, or `MERGE_HEAD`, which forces
only `--scope`). `--include-infra` appears **only** on the per-task `test_command`/`lint_command`
(`dark-factory-orchestrator.yaml:122-123`); `hooks/pre-merge-commit` runs `verify.sh all --scope all` with **no** `--include-infra`.

**⇒ Tiering inversion:** the full suite runs on **every per-task lane (×M concurrent)** and on **no merge**. Across
M lanes this is **M × 103 test-executions draining through a 16-slot (`nproc/2`) host-global semaphore**
(`run_all.sh:347,463`). Under burst oversubscription (observed M 20–88, load 168–618 on 32 cores; ~104 subshells
parked in `slot_acquire` rather than executing, esc-5029-42), the pool cannot finish within 30m → **exit-124
SIGKILL → task BLOCKED**. Aggravators: all 103 forked at once (541-proc footprint, fork-storm → EAGAIN abort,
esc-3848); poll-heavy members auto-stretch up to 8× under load **while holding a slot** (effective-width collapse);
Phase-1 output is fully buffered (`run_all.sh:454-475,508-541`), so every timeout is a bare 2-line `exit 1` →
mis-triage / cluster accretion.

**Premise re-check — the 180s backstop is NOT this cluster's kill (do not rubber-stamp).** DF's
`verify_clock_stop_heartbeat_idle_max=180` fires **only** in the `_CS_STOPPED` state (dark_factory `verify.py`
`_run_cmd` ~2111-2227; binary flag, line-anchored match, stderr merged into stdout). In `_CS_RUNNING` silence is
safe up to `verify_command_timeout_secs=7200`. run_all reaches its infra phase in RUNNING (nothing before it in the
`test` command emits a STOP; its pool `slot_acquire` is called with **no** clock reason at `run_all.sh:463`), so the
recent cluster's kill is the **30m over-serialization timeout**, not the backstop. The 4791/4655 **361s** backstop
kills were a **different, already-fixed** class (marker pollution → STOPPED into the later silent nextest **compile**;
closed by task 4998 + DF anchoring 2106). Re-lumping them here is the "recurring-class accretes fixed bugs" trap.

---

## 2. Goal & non-goals

**Goal:** eliminate the M×103 over-serialization by placing the full suite on the single-flight merge tier and the
diff-relevant subset on the per-task tier, harden run_all's Phase-1 execution model, and make a contended pool
attributable — without touching the clock-stop wire contract or the host-global concurrency cap.

**Non-goals:** (a) the DF dispatch-admission burst cap (`max_concurrent_tasks` / PSI-gated dispatch) — flagged L3b,
DF-owned; (b) the clock-stop / heartbeat compile-phase facet — owned by the design-verify-backstop-fix session
(landed 4998 + 2106); (c) the mis-lumped non-pool defects (esc-4037/5052/5020) — split to separate tasks; (d) any
change to Phase 2/2.5/3 (serial retry, FLAKY ledger, the 4998 sanitizer) — left **verbatim**.

---

## 3. The tiering seam (the L0 core)

**Single source of truth = `DF_VERIFY_ROLE`.** Both merge entry points already export `DF_VERIFY_ROLE=merge`
(`hooks/pre-merge-commit:39`; the DF merge-verify command's merge-bypass), so gating the full-suite line on
`role==merge` **auto-wires both seams with no cross-repo flag change** and removes the "miss a seam → suite runs
nowhere" hazard that a `--include-infra`-on-merge approach carries.

**Invariant: exactly one of {full pool, selective infra} runs per verify.**
- `role==merge`: emit the full `run_all.sh` line (with `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1` — the 4 host-exclusive
  tests stay on the cold lane per `run-all-host-infra-partition.md` H9/Part-B).
- `role==task` `--include-infra`: suppress the full line; run the selective injection (flip the current
  `verify.sh:1368` `INCLUDE_INFRA==0` suppression so selective runs whenever the full line does not).
- Cheap always-on per-task checks (sync_comments, PTODO prebuild+gate, pm_standardization, event_inventory) are
  **unchanged** (kept on the per-task tier; their possible additional placement at merge is out of scope, §7).

**Coverage backstop (why this is safe):** the merge gate is the authoritative check before main advances, and it now
runs the full suite. Per-task selective is fail-fast, not the last line of defense. L0b broadens the (currently
23-artifact, 28/110-reachable) select map so per-task keeps fail-fast on the high-value infra most likely to be
edited; product-source diffs correctly select nothing (they cannot break infra meta-tests except indirectly — the
merge suite is the backstop).

---

## 4. Contracts / invariants (H — pin the dangerous ones)

- **INV-1 (host-global semaphore is load-bearing, do NOT remove).** L1's `wait -n` throttle caps spawned worker
  **shells** only; `slot_acquire` on the per-uid host-global lock (`run_all.sh:347`) **remains** the concurrency
  gate. It is the sole mechanism bounding total host concurrency across concurrent invocations (the Lever-C
  laptop-fallback and lockless-`land.sh` collision cases, §8) — removing it turns those into 2N oversubscription.
- **INV-2 (Phase-1-only rewrite).** Phases 2/2.5/3 (serial retry, `--- attempt N ---` sub-headers, FLAKY counting,
  the 4998 `@@REIFY_CLOCK_`→`@@REIFY_QUOTED_CLOCK_` sanitizer, discovered-order Phase-3 replay) are **verbatim**.
- **INV-3 (W=N ≥ 2 genuinely in flight).** The throttle must keep ≥2 workers concurrent or the pool
  max-concurrency≥2 assertion (`test_run_all.sh` T9a) deadlocks — it is a real guard against an accidentally-serial
  throttle.
- **INV-4 (progress line is additive & marker-safe).** The L2 line goes to **stderr**, never contains
  `@@REIFY_CLOCK_` (the sanitize test scans both streams), never starts with `--- Running: ` / `FAILED ` / `=== `,
  and is **added alongside** the existing `INFO: … N=` / `PSI backoff` lines. It is plain INFO, **not** a clock
  marker — silence in `_CS_RUNNING` is already safe, and a marker would risk the concurrent-STOP-collapse.
- **INV-5 (exactly-one-tier).** The merge-plan drift-guard asserts the `role==merge` plan **contains** the
  `run_all.sh` line and the `role==task` plan **does not** — making "the merge backstop exists" a tested invariant.
- **INV-5′ (per-member content-addressed skip within the merge tier).** Amends INV-5 per
  `docs/prds/merge-gate-riders.md` §4.4 (task γ / 5273). The full pool stays **merge-tier-resident** and the plan
  shape is unchanged — INV-5's tier assertions still hold, and the `role==merge` run_all.sh line now additionally
  carries `REIFY_RUN_ALL_CONTENT_SKIP=1`. **Within** the merge-tier invocation: every member runs on every merge
  whose delta (vs. that member's last green main run) touches its declared closure; every member without a declared
  closure runs on every merge; and every member runs at least once per `REIFY_RUN_ALL_SKIP_MAX_MERGES` (default 25)
  merge-tier runs and once per `REIFY_RUN_ALL_SKIP_MAX_AGE_HOURS` (default 24) regardless of deltas. Skips are
  **per-member, content-hash-based** (git tree compare against the closure declared in
  `tests/infra/run-all-skip-closures.manifest`), **individually logged**
  (`SKIP (content-clean)` / `RUN (delta|unmapped|no-baseline|backstop-due)`), and **fail-open** — an unmapped or
  no-baseline member, an own-file change, or a corrupt/absent `REIFY_RUN_ALL_SKIP_STATE` file all force a run (the
  last emits one loud line + full pool). The engine is a three-key inert no-op
  (`REIFY_RUN_ALL_CONTENT_SKIP=1` **and** inbound role `merge` **and** a non-empty state path), so it ships
  production-inert until the activation task (δ / 5276) wires the durable state path in yaml `verify_env`. Guarded by
  `tests/infra/test_run_all_content_skip.sh` plus the run_all.sh-line token asserts in
  `test_run_all_tiering.sh` / `test_verify_failfast_order.sh`.

---

## 5. Pre-conditions / substrate (G3 — verified against HEAD 2026-07-06)

| Capability | Status | Evidence |
|---|---|---|
| `DF_VERIFY_ROLE` set on both merge seams | ✅ | `hooks/pre-merge-commit:39` (`DF_VERIFY_ROLE=merge`); DF merge-verify sets it (merge-bypass everywhere) |
| `select_infra_tests()` + map + `--print-plan` | ✅ | `verify.sh:789-816`; `verify-pipeline-infra-tests.txt` (exact-match, 23 artifacts); `test_run_all.sh:169-170` reads the plan |
| `slot_acquire LOCK N WAIT` host-global | ✅ | `lib_slot_acquire.sh`; lock `${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock` (`run_all.sh:347`) |
| bash `wait -n` | ✅ | bash 5.2 baseline (codebase norm) |
| Output-contract independence of exec-order | ✅ | Phase-1 buffered by discovered-index, Phase-3 replay sorted (`run_all.sh:456-541`); `test_run_all.sh:500-506` asserts sorted-order only |
| Coarse single-flight flock precedent (L3a option) | ✅ | H8 Lane-X flock (`run_all.sh:290-303`, `lib_lane_x_flock.sh`) |

No novel substrate. G3 verdict: PASS.

---

## 6. Cross-PRD / seam ownership (G4)

| Other | Direction | Seam | Owner | Status |
|---|---|---|---|---|
| `run-all-host-infra-partition.md` (Part A) | this PRD **re-tiers + hardens** its H2 pool | role-gate the pool line; Phase-1 worker pool; host-exclusive stays on the H9/Part-B cold lane | this PRD | corrects |
| design-verify-backstop-fix session | **disjoint** | this PRD adds **no** clock markers and rewrites Phase-1 **only**, leaving 4998's Phase-3 sanitizer verbatim | that session (landed 4998+2106) | disjoint |
| `cpu-load-admission-control.md` | **companion** (L3b) | DF dispatch-admission (govern load not lanes) caps the burst M that this PRD's L0 makes survivable-but-not-bounded | dark-factory | flagged |

---

## 7. Out of scope (accepted)

- DF dispatch-admission burst cap (L3b — flagged, DF-owned).
- Moving the cheap per-task infra checks (sync_comments/PTODO/pm_std/event_inventory) to also gate at merge — a
  separate latent under-gating, noted for a follow-up, not scoped here.
- The clock-stop / compile-phase heartbeat facet (owned elsewhere; already landed).
- esc-4037 (#4997 host-infra flake — **auto-resolved** by L0: host-exclusive never runs per-task), esc-5052
  (warm-lane CoW tauri path-leak), esc-5020 (reaper false-kill) — split to separate triage tasks.

---

## 8. Invariants / do-nots

- **Never** replace `slot_acquire` with an in-process-only throttle (INV-1).
- **Never** worktree/per-lane-scope the pool lock (inverts the host-global cap → M×16 host melt; the escalations'
  own suggestion, explicitly rejected).
- **Never** emit `@@REIFY_CLOCK_*@@` from run_all's live stream (INV-4).
- Rare-concurrency reality (do not assume M=1 at merge): Lever-C K=2 laptop-unreachable fallback runs 2 verifies
  local; lockless `land.sh` can collide with the orchestrator merge. INV-1's semaphore keeps both safe; an optional
  coarse merge-run_all single-flight flock (L3a) is belt-and-braces.

---

## 9. Decomposition plan (leaf → observable signal, G2). **Priority: L0a is `critical` (resume-gate).**

- **L0a — Role-gate the full pool to merge; per-task → selective; add the tier drift-guard. [priority: critical]**
  *Modules:* `scripts/verify.sh` (`build_plan`), `tests/infra/` (new `test_run_all_tiering.sh`).
  *Signal:* `verify.sh --print-plan` with `DF_VERIFY_ROLE=merge` **contains** the `run_all.sh` line;
  `DF_VERIFY_ROLE=task --scope branch --include-infra` **omits** it and **contains** selective-infra lines; the new
  drift-guard goes RED if either half regresses (INV-5). `test_run_all.sh`/`test_verify_scope.sh` stay green.
  *This is the scheduler-resume gate — collapses M×103 → M×(selected)+1×103.*

- **L0b — Broaden the select map to high-value infra.**
  *Modules:* `scripts/verify-pipeline-infra-tests.txt` (+ its citing-test rows).
  *Signal:* a `--scope branch` diff touching a sourced verify lib (`occt-scope-lib`/`affected-crates-lib`/
  `release-scope-lib`/`cpu-admit.sh`), `run_all.sh`, `land.sh`, or `hooks/*` selects its guard test in
  `--print-plan` (new `test_verify_scope.sh` rows). *Depends: L0a (shares the selective path).*

- **L1 — Phase-1 bounded worker pool + fork-EAGAIN guard.**
  *Modules:* `tests/infra/run_all.sh` (Phase-1 only).
  *Signal:* under a fixture with 103 members and N=4, peak concurrent worker shells ≤ N (observable via a spawn
  counter) **and** ≥2 (INV-3); a fault-injected `fork()` EAGAIN degrades one member and the suite still completes
  and emits the byte-identical Summary/FAILED contract. `slot_acquire` still present (INV-1 asserted). *Depends: L0a.*

- **L2 — Single-writer Phase-1 progress heartbeat.**
  *Modules:* `tests/infra/run_all.sh`, `tests/infra/test_run_all.sh`.
  *Signal:* a slow-fixture pool run emits ≥1 `INFO: run_all.sh pool progress: X/Y complete, elapsed Ns` on stderr
  within 2×`REIFY_RUN_ALL_PROGRESS_SECS`; the clock-marker-sanitize test stays green (INV-4: no `@@REIFY_CLOCK_`);
  the output-contract test is unaffected. *Depends: L1.*

- **L3a — Host-global-lock drift-guard (+ optional merge single-flight flock).**
  *Modules:* `tests/infra/` (new meta-test), optionally `run_all.sh` (reuse `lib_lane_x_flock.sh` around merge-role).
  *Signal:* the guard goes RED if the resolved pool lock base is not host-global (embeds a worktree/per-lane path);
  green on the default per-uid host path. *Depends: L0a.*

- **L3b — [dark-factory] Dispatch-admission burst companion (FLAG, not built here).**
  *Signal (cross-repo):* the orchestrator caps concurrently-dispatched heavy verifies by PSI/load, so burst M stays
  bounded — pairing with L0 which raises but does not cap the survivable M. *Owner: dark-factory* (recorded as a
  cross-project edge at decompose; the scheduler-resume gate should require L0a landed + this flagged).

**DAG:** L0a → {L0b, L1, L3a}; L1 → L2. L0a is independently landable and is the resume-gate; L1/L2/L3a harden the
merge-tier run and diagnosability; L3b is DF-owned.

---

## 10. Open (tactical) questions

1. **W (worker count) default** — `N` (= pool concurrency = `nproc/2`) keeps a single merge-tier run at full width;
   confirm during L1. No new host-baked constant (derived from `nproc` at runtime).
2. **`REIFY_RUN_ALL_PROGRESS_SECS` default** — 30 (mirrors `REIFY_CLOCK_HEARTBEAT_SECS`); decide during L2.
3. **Merge single-flight flock (L3a option)** — build now or defer? INV-1's semaphore already prevents the melt;
   the flock only reduces contention in the rare 2-invocation case. Suggested: defer unless observed.
