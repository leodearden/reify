# Capability manifest — `run-all-pool-contention-tiering-fix`

Mechanizes G3 + G6 per leaf for `docs/prds/run-all-pool-contention-tiering-fix.md`. Each binding ties a leaf's
asserted capability to **evidence** (grep/command/file:line). Any **FAIL** binding blocks queueing until resolved.
Verified against main @ HEAD, 2026-07-06.

**Domain notes.** This PRD is **shell / verify-pipeline-tiering / concurrency-control infra** — no `.ri` syntax, no
result fields — so the reify field-population sentinel (`Value::Undef`) and grammar-fixture checks are **N/A by
construction** (recorded per binding, not silently skipped). The live G6 surface is narrow and **anti-regression**:
(a) no host-baked constant (W/N from `nproc` at runtime), (b) no new wall-clock upper bound, (c) the host-global
concurrency cap is **preserved, not re-authored** (INV-1), (d) the byte-identical output contract is **inherited**,
only its liveness augmented (INV-4). No leaf authors a new numeric assertion.

---

## L0a — role-gate full pool to merge; per-task → selective; tier drift-guard  **[priority: critical]**

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `DF_VERIFY_ROLE` is set on both merge seams (`hooks/pre-merge-commit:39`; DF merge-verify merge-bypass). `verify.sh build_plan` already branches on `INCLUDE_INFRA` (`:1318`) and `role` (`:1391` print-plan header); `select_infra_tests()` + `SELECTED_INFRA_GLOBS` (`:789-816`, `:1368`) already exist and already run when the full line does not. `--print-plan` is machine-readable (`test_run_all.sh:169-170`). |
| **Auto-wired, no cross-repo flag (anti-orphan)** | **PASS** | Gating on `role==merge` (a signal both seams already export) means no `orchestrator.yaml` / `hooks` flag edit and no "miss a seam → suite runs nowhere" hole. The drift-guard (INV-5) makes "merge plan contains run_all, task plan does not" an executed assertion, not a tabulated promise. |
| **No host-baked constant (G6 anti-#4901)** | **PASS** | No integer introduced; tiering is a pure `role` branch. |
| **Coverage-backstop non-vacuous (negative-assertion)** | **PASS (by signal)** | The drift-guard must go RED if the merge line is dropped OR if the task line reappears — both halves asserted, so a regression to "infra gated nowhere" or "full pool back on per-task" fails the pipeline. |
| Grammar-fixture / field-population | **N/A** | No `.ri` syntax, no result fields. |

## L0b — broaden the select map to high-value infra

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `verify-pipeline-infra-tests.txt` exact-match map + `select_infra_tests()` (`verify.sh:789-816`, `[ "$_f" = "$_artifact" ]` at `:802`) already drive the selective path; adding rows is additive. The `test_verify_scope.sh` VS-* scenario harness (task 4523) already asserts path→test selection. |
| **Reachability premise (G6, measured)** | **PASS — gap acknowledged, backstopped** | Today 23 artifacts reach 28/110 tests; product-source diffs select 0. This leaf broadens to the sourced verify libs + `run_all.sh` + `land.sh` + `hooks/*`. The residual (product-source can't reach infra meta-tests) is **backstopped by L0a's merge full-suite**, not left uncovered. |
| **Non-vacuous (negative-assertion)** | **PASS (by signal)** | A new VS-* row asserts a diff touching e.g. `occt-scope-lib.sh` selects `test_occt_gated_scope.sh` — RED if the map row is absent. |
| Grammar / field-population | **N/A** | Infra. |

## L1 — Phase-1 bounded worker pool + fork-EAGAIN guard

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | bash 5.2 `wait -n` (codebase baseline); Phase-1 spawn loop + per-index buffering at `run_all.sh:454-475`; discovered-order Phase-3 replay at `:508-541` is exec-order-independent (`test_run_all.sh:500-506`). |
| **INV-1 host-global semaphore preserved (G6 anti-regression, load-bearing)** | **PASS (asserted)** | The leaf keeps the `slot_acquire "$_H2_POOL_LOCK" …` call (`run_all.sh:463`) as the concurrency gate; `wait -n` caps worker **shells** only. A test asserts `slot_acquire` is still invoked. Removing it would 2N-oversubscribe the Lever-C/`land.sh` collision cases — the exact regression this binding blocks. |
| **INV-3 W≥2 in flight (anti-serial-throttle)** | **PASS (by signal)** | The existing pool-max-concurrency≥2 assertion (`test_run_all.sh` T9a, `:475`) is a real deadlock/serial guard; the throttle must satisfy it. |
| **No new wall-clock upper bound (G6)** | **PASS** | Worker-pool + EAGAIN retry use bounded small sleeps, no new wall-clock assertion; `test_no_new_wallclock_upper_bounds.sh` (T9 standing guard) stays green. |
| **fork-EAGAIN degrade non-vacuous (negative-assertion)** | **PASS (by signal)** | Fault-injected fork failure must yield a recorded single-member failure + a completed suite emitting the bare `FAILED` marker — not a 2-line `exit 1` abort (esc-3848). |
| **Output-contract preservation** | **PASS (by signal)** | Contract test (`test_run_all.sh:500-506`) + the `^FAILED\s` DF classifier stay green; Phase 2/2.5/3 untouched (INV-2). |

## L2 — single-writer Phase-1 progress heartbeat

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | run_all already emits `INFO: … N=` / `PSI backoff` on stderr (`run_all.sh:369,441`); a background printer + `done/total` counting via `.rc` files is a local addition. |
| **INV-4 marker-safe & additive (G6, cross-repo seam)** | **PASS (asserted)** | The line goes to stderr, never contains `@@REIFY_CLOCK_` (the both-stream scan `test_run_all_clock_marker_sanitize.sh:129-134` stays green), never starts with `--- Running: `/`FAILED `/`=== `, and is added alongside existing INFO lines. It does not match DF's `^FAILED\s` or the line-anchored clock parser. |
| **Non-vacuous (negative-assertion)** | **PASS (by signal)** | A slow-fixture run must emit ≥1 progress line within 2×interval; a run that stays silent fails the new assertion (guards against a printer that never fires). |
| **Not a clock marker (design invariant)** | **PASS** | Plain INFO by construction — silence in `_CS_RUNNING` is already safe (7200s budget), so no clock-stop semantics are needed or wanted (avoids the concurrent-STOP collapse). |

## L3a — host-global-lock drift-guard (+ optional merge single-flight flock)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | Pool lock resolves to `${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock` (`run_all.sh:347`); a meta-test can assert the resolved base is host-global. Optional flock: H8 Lane-X precedent (`run_all.sh:290-303`, `lib_lane_x_flock.sh`). |
| **Non-vacuous (negative-assertion)** | **PASS (by signal)** | The guard goes RED if the lock base embeds a worktree/per-lane path (the latent per-lane-TMPDIR trap → M×16 melt); green on the default per-uid host path. |
| **Anti-orphan / wired** | **PASS** | Registered in the verify-pipeline infra step; defends INV-1's premise that the semaphore is genuinely host-shared. |

## L3b — [dark-factory] dispatch-admission burst companion

| Check | Verdict | Evidence |
|---|---|---|
| **Cross-repo seam, owned not hand-waved (G4)** | **PASS (flagged)** | Owner = dark-factory scheduler (`cpu-load-admission-control.md` dispatch axis, "govern load not lanes"). Recorded as a cross-project `add_dependency` edge at decompose; **not built in this reify PRD**. Reify's L0 raises the survivable M; DF caps it. |
| Reify-local substrate | **N/A** | No reify code; flag only. |
