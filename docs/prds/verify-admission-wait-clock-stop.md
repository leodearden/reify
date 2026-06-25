# PRD — Verify admission-wait "stop the clock" (uniform clock-stop seam) — reify task 4837

**Status:** authored 2026-06-25. Version-agnostic infrastructure foundation (root-level `docs/prds/`).
**Approach:** B + H (contract + two-way boundary tests). This is the verify pipeline every task and every merge depends on; the failure mode (code-complete tasks landing `blocked` on transient resource contention) is silent and expensive to recover from, and the fix spans a **cross-repo seam** (reify ↔ dark-factory).
**Supersedes (corrections, not deletions):** `docs/prds/test-run-concurrency-semaphore.md` §4/§6/§7 — those sections assert "the orchestrator already requeues exit 75 (EX_TEMPFAIL) as retry-capped transient infra." **That is false** (see §2). This PRD corrects the premise and defines the real seam.

---

## 1. Consumer & user-observable surface

**Consumer:** `scripts/verify.sh`'s **admission waits** — the held-slot test-run semaphore acquire (`scripts/lib_test_semaphore.sh` → `scripts/lib_slot_acquire.sh`) and the PSI admission gate (`psi_gate()` → `scripts/cpu-admit.sh`, `requeue` mode). Both run inside the `./scripts/verify.sh test …` subprocess the dark-factory orchestrator spawns for **every** per-task verify (orchestrator.yaml `test_command`). The second consumer is the **orchestrator's verify-subprocess timeout** itself (`verify_command_timeout_secs`), which today wall-clock-kills any verify that waits too long.

**User-observable surface (operator / orchestrator):**
- A task verify that cannot immediately get a test slot (or PSI headroom) **waits in place, holding its file locks and warm lane**, until the resource frees — then runs and completes. It does **not** exit-75, does **not** requeue, and does **not** land `blocked`. Observable as: a `verify.sh test` process blocked at the acquire/PSI step emitting periodic `@@REIFY_CLOCK_HEARTBEAT@@` lines, then proceeding when a slot frees (infra/e2e tests).
- The wait time **does not count against** `verify_command_timeout_secs`: a verify that waits an hour for a slot then runs a 30-min test does not get killed at the 2-hr wall-clock. Observable as: a clock-stop-aware DF timeout test (a stub verify that heartbeats past the normal budget, then completes, is NOT killed).
- A **genuine hang is still caught**: a verify that stops heartbeating during a wait (wedged wait loop) is killed after the heartbeat-idle backstop; a hang in the actual test (clock running) is killed by the normal wall-clock timeout. Observable as: a stub that goes silent during a clock-stop span IS killed + classified infra.

## 2. Premise correction (what actually exists) — load-bearing

The whole semaphore/PSI "backpressure" story rested on a **false premise**, verified false directly in the dark-factory source on 2026-06-25:

> dark-factory does **NOT** requeue a verify-command exit code of 75.

`orchestrator/src/orchestrator/verify.py` `_classify_failure(output, rc, timed_out)` (lines 399-438) special-cases **only** `rc == 0` → `passed` and `timed_out` → `infra_timeout`. Every other non-zero `rc` — **including 75** — with no matching output pattern falls through to `unknown_test_failure`. `_CLASSIFY_PATTERNS` is **output-text** based (compile_error / test_failure / flock_error / …); there is **no exit-code-75 branch and no EX_TEMPFAIL handling at all** for the verify command. Consequences:

- A slot-starved (or PSI-starved) task verify that exits 75 is treated as a **genuine RED test failure** → the verify-debugfix loop spawns a **debugger agent** to "fix" a non-bug, re-probes main for a preexisting break, and after `max_failure_signature_repeat`=3 / `max_verify_attempts`=5 → **BLOCKED + L1 escalation**. This is the true mechanism behind task **4800** and the **esc-3891-45 / esc-4673-31 / esc-4552** cluster — *not* "requeue retry-cap exhaustion" as previously documented.
- The only real exit-75 → `REQUEUED` contract in dark-factory is the warm-lane **seed / disk-guard** path (`git_ops.py`, `WarmLaneRequeue`) — it fires during lane provisioning **before** verify runs, and is unrelated to the verify subprocess's exit code. Even ENOSPC during verify (`VerifyInfraError`, the in-process warm-marker write, verify.py:1832) ends **BLOCKED**, not requeued.
- There is **no uncapped / penalty-free requeue class** for verify, **no exit-code transient class** (OCCT-slot / ENOSPC are not exit-code based), and **no heartbeat / liveness** signal — the only hang detection is the per-command wall-clock `asyncio.wait_for` from spawn (verify.py:1736/1758). A verify blocking on a semaphore is **indistinguishable from a hang** today.

So the fix **cannot** be reify-side alone, and it **cannot** route through a requeue: it needs a dark-factory change. The corrected docs (CLAUDE.md, orchestrator.yaml, the existing semaphore PRD) are scoped into this work (§9 leaf γ).

## 3. Sketch of approach — chosen split + rejected alternatives

**Chosen: continuous in-process blocking wait + a uniform "stop the clock" seam (option `c`, generalized).**

Reify's admission waits (test-slot acquire **and** PSI gate) become **continuous** in-process blocking waits — the verify subprocess sleeps in place until the resource frees, **holding the task IN-PROGRESS (file locks + warm lane) start-to-finish**. While waiting, reify emits a uniform **clock-stop marker protocol** to the verify output stream. Dark-factory **excludes the marked wait span from `verify_command_timeout_secs`** ("stops the clock") and applies a **heartbeat-idle backstop** to that span so a genuinely-wedged wait is still caught. No requeue, no exit-75, no retry-cap interaction.

### Why continuous-wait-and-hold, not exit→requeue (the decisive rejection of option `b`)

The orchestrator's **narrow file locks are the dominant scheduling limitation.** A task acquires its file locks on going in-progress and may wait **hours or days** to win them under contention. An `in-progress → pending → in-progress` requeue cycle **releases those locks**; by re-acquisition, sibling tasks will have churned the same files, so the code-complete branch faces main-drift and rebase conflicts. **Requeue actively damages a finished task.** A blocking wait that holds the locks is strictly better — and the "the waiter occupies a warm lane while blocked" property is *desired* (the lane is its workspace; the test slot is the real bottleneck). This is why option (b) (reify quick-fail + DF uncapped requeue), and the related option (a) (raise the timeout), are rejected even though their DF change is smaller.

### Rejected alternatives (so implementers don't relitigate)

- **(a) Raise / remove `verify_command_timeout_secs`.** Crude: weakens hang detection for *all* verifies, and a finite-but-huge timeout still kills the wait eventually (exit-124, a hard fail). Doesn't hold-and-wait fairly; doesn't distinguish wait from hang. Rejected.
- **(b) reify quick-fail exit-75 + DF uncapped/penalty-free requeue (± priority-aging).** Smallest DF change and avoids a long-lived process, BUT **releases the task's file locks on every requeue cycle** (the decisive harm above), churns the verify preamble + agent respawn, and — because the semaphore is a **non-FIFO lottery** (`flock -xn` + `shuf` + `sleep 0.5` in `lib_slot_acquire.sh`, *not* an ordered queue) — a requeued task that leaves the lottery during the re-dispatch dead-window can be relatively starved. Even with scheduler priority-aging to bound starvation, the file-lock release alone disqualifies it. Rejected (the dead `60baafc2` branch's *active* config was a variant of this; see §10).
- **The "compile-outside-slot" structural relief is already landed (task 4839 / esc-4837-6) — it is a precondition, not this fix.** `verify.sh` already emits the `cargo nextest run --no-run` test-binary compile passes *outside* the held slot, so the slot now wraps test **execution** only and rotates fast. That makes the residual wait *rare and short in the common case* — which is exactly why a *continuous* wait is cheap here (it almost never blocks long) and why losing the file locks to a requeue would be a disproportionate price for a rare event. 4839 shrinks the problem; this PRD removes the `blocked` landmine that remains.

### The uniform "stop the clock" mechanism (details — the user delegated these 2026-06-25; "cover both, uniform")

A single marker protocol + a single shared reify emitter + a single dark-factory consumer, used by **both** the test-slot acquire loop and the PSI gate wait loop (and any future admission gate):

**Markers (reify → dark-factory, on the verify output stream, stderr):**
- `@@REIFY_CLOCK_STOP@@ reason=<test_slot_starvation|psi_pressure> pid=<pid>` — emitted once, on entering the wait (after the first failed immediate acquire / first pressure-fail). Tells DF: *stop counting wall-clock against the verify budget from here.*
- `@@REIFY_CLOCK_HEARTBEAT@@ reason=<…> waited=<secs>` — emitted every `H` seconds (knob, default 30s) while still waiting. Liveness proof, emitted **from inside the poll loop** so a wedged loop stops emitting.
- `@@REIFY_CLOCK_START@@ reason=<…> waited=<secs>` — emitted on exit (slot acquired / pressure cleared), immediately before proceeding. Tells DF: *resume the wall-clock budget (excluding the stopped span).*

**Dark-factory behavior (generic capability; reify is first consumer):**
- While streaming verify output, recognize the (configurable) markers and maintain a *clock-stopped* state.
- **Stopped:** do not accrue elapsed wall-clock toward `verify_command_timeout_secs`. Instead enforce a **heartbeat-idle deadline**: if no `@@REIFY_CLOCK_HEARTBEAT@@` (or any clock marker) arrives within `verify_clock_stop_heartbeat_idle_max` (knob, default 180s ≫ `H`), the wait is wedged → kill the tree + classify `infra_timeout` (a genuine hang).
- **Resume** on `@@REIFY_CLOCK_START@@`: continue the normal wall-clock budget with the stopped span excluded.
- **No change** to `_classify_failure` outcome routing, the debugfix loop, requeue, or the retry-cap: a clock-stopped verify that eventually completes simply passes/fails on its actual merits.
- Optional hard backstop knob `verify_clock_stop_max_total_secs` (cumulative stopped time; default 0 = unlimited) for defense-in-depth.

This is a generic orchestrator feature (any project's verify can emit the markers); reify opts in by emitting them and setting the knobs.

## 4. Pre-conditions (substrate)

| Capability | Status | Evidence |
|---|---|---|
| Compile runs OUTSIDE the held slot (slot = execution only) | **present** | task 4839 / esc-4837-6 landed; `verify.sh` `_emit_profile_passes "compile"` before `@@SEMAPHORE_ACQUIRE@@` |
| Shared slot-acquire core `lib_slot_acquire.sh` (the wait loop to extend) | **present** | task 4840 landed; `lib_test_semaphore.sh` + `cargo-test-occt-gated.sh` source it |
| `DF_VERIFY_ROLE=task\|merge` lane signal (merge stays exempt) | **present** | `verify.py` `_resolve_verify_env`; reify keys the semaphore/PSI bypass off it |
| DF streams verify stdout/stderr line-by-line (to add marker parsing) | **present** | `verify.py:1745-1758` streamed-log path |
| DF clock-stop-aware timeout (pause/resume + heartbeat-idle backstop) | **ABSENT → queued as prerequisite** | verified: `verify.py` uses a fixed `asyncio.wait_for` from spawn, no marker awareness (§2). Owned by dark-factory (§7, leaf DF-δ). |
| reify continuous (unlimited) wait + heartbeat capability | **partial (dead branch)** | `60baafc2` shipped an `unlimited` mode + 60s heartbeat in the *pre-4840 inline* loop; re-express + extend (§10) |

No novel `.ri` grammar — the grammar gate (G3) is N/A (shell + dark-factory Python only). The one assumed substrate that does not exist (the DF clock-stop timeout) is queued as the explicit cross-repo prerequisite DF-δ, satisfying G3.

## 5. Resolved design decisions

- **D1 — Hold locks, wait in place; never requeue for a transient wait (user, 2026-06-25).** The decisive constraint: requeue releases the narrow file locks the task won over hours/days → main-drift rebase conflicts on a finished branch. Blocking-and-holding is strictly better. (§3.)
- **D2 — Uniform mechanism across semaphore + PSI gate (user, 2026-06-25).** One marker protocol, one shared emitter, one DF consumer. The PSI gate (`cpu_admit requeue`) has the *identical* latent bug (its MAX_WAIT exit-75 also → `blocked`); both adopt the clock-stop wait. The `compile_gate` (`cpu_admit admit`) already admits-on-timeout (never exit-75, bounded 300s) so it is **not** a starvation source and is out of scope for clock-stop (may adopt later, no urgency).
- **D3 — Stop-the-clock via explicit STOP/HEARTBEAT/START markers, not a blanket idle-timeout.** Explicit markers keep a *tight* wall-clock budget on the actual work (compile/test — where hangs matter most) and relax only during *declared* waits. Considered alternative: a pure inactivity watchdog (reset the deadline on any output). Rejected as the primary because it relaxes hang detection everywhere and would false-trip on legitimately-quiet work phases (slow link, quiet long test); the explicit protocol is more precise. (The watchdog remains a viable simpler fallback if DF prefers it — noted for DF-δ.)
- **D4 — Heartbeat from inside the poll loop = liveness.** The heartbeat must reflect loop progress (emitted each poll iteration when `H` elapsed), so a wedged loop / SIGSTOP stops heartbeating and the idle backstop fires. A dumb wall-clock timer would mask a wedge. (§3, §6 hang-vs-wait.)
- **D5 — Continuous wait is gated until the DF seam ships (graceful degradation).** Reify ships the marker emission + the unlimited-wait *capability* but keeps `REIFY_TEST_SEMAPHORE_WAIT` **finite** (3600, < `verify_command_timeout_secs`=7200) in `orchestrator.yaml` until DF-δ is deployed. Pre-seam, a finite wait exits cleanly (status-quo `blocked`, with the N=3/4839 relief) rather than blocking past the wall-clock into exit-124. The flip to `unlimited` + enabling the DF knobs happens together at deploy (leaf ε / task 4838). This mirrors the dead branch's gating discipline ("activating unlimited before the seam is strictly worse").
- **D6 — Merge stays exempt; the gated set is unchanged.** `DF_VERIFY_ROLE=merge` continues to bypass both the semaphore and the PSI gate (the merge gate never waits behind a task). The clock-stop markers are only emitted on the `task` path.
- **D7 — No new exit-code contract.** Because DF does not honor exit-75 for verify (§2) and the wait no longer terminates in failure, the design carries **no** exit-code semantics into the seam — the discriminant is the marker stream, not the exit code. (Removes the false "75 is honored" assumption rather than entrenching it.)

## 6. Out of scope / accepted limitations

- **Waiters occupy warm lanes.** Under sustained saturation, many task verifies block in-process, each holding a warm lane. This is *intended* (D1) — the lane is the waiter's workspace. Implication: the warm-lane pool size and `max_concurrent_tasks` should be understood as "tasks that may be simultaneously dispatched **including those parked on the test slot**," not "tasks actively running tests." If lane exhaustion under deep slot queues becomes a problem, the lever is **dispatch admission** (don't dispatch more tasks than the slot throughput can drain) — a separate scheduler concern, not this PRD.
- **No FIFO fairness guarantee.** The underlying acquire is a randomized lottery (`flock -xn` + `shuf`); continuous waiting is statistically fair but not ordered. A true FIFO ticket-lock is a possible future enhancement (orthogonal to clock-stop) — out of scope here. Indefinite starvation under *permanent* saturation is a capacity problem no verify-layer scheme solves; the lever is dispatch admission.
- **`compile_gate` clock-stop.** Out of scope (D2) — it admits-on-timeout and is bounded, not a starvation source.
- **Pre-seam window.** Until DF-δ deploys, slot-starved tasks still land `blocked` (status quo). N=3 + 4839 keep this rare; this PRD does not regress it.

## 7. Cross-repo seam ownership (G4 — the centerpiece)

| Seam | Owner | Resolution |
|---|---|---|
| **Clock-stop-aware verify timeout** (pause/resume + heartbeat-idle backstop + knobs) | **dark-factory** | New generic capability in `orchestrator/src/orchestrator/verify.py` `_run_cmd`. Reify ships markers + config; DF ships the mechanism. Tracked as a **dark-factory task** (leaf DF-δ); reify's deploy milestone (4838) and the re-scoped 4837 depend on it via an external `dark_factory:<id>` edge. **Not** filed as dismissable `escalate_info` (the dead branch's mistake — esc-4837-5/6 were closed without landing). |
| Clock-stop marker protocol (string grammar, reason vocabulary) | **reify** (proposes) → **shared** | reify owns the emitter + reason subclasses (`test_slot_starvation`, `psi_pressure`, future); DF matches the `@@REIFY_CLOCK_*@@` family. Marker strings are a DF config value (generic), defaulted to this family. |
| `verify_command_timeout_secs` / `REIFY_TEST_SEMAPHORE_WAIT` co-tuning + the `unlimited` flip | **reify** | `orchestrator.yaml` (reify-owned). WAIT stays finite < timeout pre-seam; flips to `unlimited` at deploy. |
| Lane signal (`DF_VERIFY_ROLE`), merge exemption | **existing** | unchanged; markers emitted on `task` path only. |

**No reciprocal "the other owns it":** reify owns emission + config + the wait loop; dark-factory owns the timeout mechanism. The dependency direction is the allowed reify → dark_factory external edge (mirrors `setup-worktree-debug-port.sh` G4, cpu-governance α/β/γ↔ζ, warm-lane D8).

## 8. Boundary-test sketch (H — facing both ways)

- **Reify mechanism face:** `tests/infra/test_test_run_semaphore.sh` + a PSI-gate test drive the libs directly — entering a wait emits exactly one `@@REIFY_CLOCK_STOP@@`, a heartbeat every `H` seconds, and a `@@REIFY_CLOCK_START@@` on acquire; unlimited mode blocks-then-runs (never exits 75); markers carry the correct `reason=`. Same assertions for both the semaphore acquire and the psi_gate wait (proving the *uniform* emitter).
- **Reify integration face:** `tests/infra/test_verify_semaphore_e2e.sh` drives real `scripts/verify.sh test` against an externally-held slot — the verify emits STOP + heartbeats to stderr and proceeds on release; `--print-plan` marks the clock-stop region. (Re-expresses the dead branch's Section F onto the markers.)
- **Dark-factory face:** a DF integration test with a stub verify command — (1) a command that emits `CLOCK_STOP`, heartbeats *past* the normal `verify_command_timeout_secs`, then `CLOCK_START` + exits 0 → is **not** killed and records `passed`; (2) a command that emits `CLOCK_STOP` then goes silent past `heartbeat_idle_max` → **is** killed + classified `infra_timeout`; (3) the stopped span is excluded from the budget (a short post-resume hang is still killed by the resumed wall-clock).

These two-way tests are the H contract: the marker grammar is exercised from the emit side (reify) and the consume side (dark-factory) independently, so neither half can drift silently.

## 9. Decomposition plan (one bullet per leaf → observable signal)

- **α (reify) — uniform clock-stop emitter + continuous-wait capability.** Add the shared `@@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@` emitter + the unlimited/continuous wait mode to `scripts/lib_slot_acquire.sh` (re-expressed onto the post-4840 shape), and wire it into BOTH `lib_test_semaphore.sh`'s acquire AND `cpu-admit.sh`'s `psi_gate` (requeue-mode) wait. Knobs: heartbeat interval `H`; `unlimited` sentinel for the wait. **Signal:** `tests/infra/test_test_run_semaphore.sh` + a PSI-gate infra test assert the STOP/HEARTBEAT/START sequence + `reason=` on both gates, and that unlimited mode blocks-then-runs (no exit-75). **deps:** none.
- **β (reify) — verify.sh integration + print-plan oracle.** Ensure markers surface on the real verify path at the semaphore-acquire region and the psi-gate; `--print-plan` annotates the clock-stop region. **Signal:** `tests/infra/test_verify_semaphore_e2e.sh` — real `verify.sh test` blocked on a held slot emits STOP + heartbeat and proceeds on release; print-plan marks the region. **deps:** α.
- **γ (reify) — docs correction + contract surfacing.** Correct the false exit-75→requeue premise in `CLAUDE.md` ("Test concurrency"), `orchestrator.yaml` (:80-82 + the WAIT knob block), and `docs/prds/test-run-concurrency-semaphore.md` §4/§6/§7; document the clock-stop marker contract + the dark-factory dependency; co-tune the WAIT<timeout comments. **Signal:** grep shows the corrected text; orchestrator.yaml cites the marker contract + the DF task; the existing PRD §6 "No dark-factory change" is replaced with the seam pointer. **deps:** α (so the doc matches the shipped markers).
- **DF-δ (dark-factory, cross-repo) — generic clock-stop-aware verify timeout.** In `orchestrator/src/orchestrator/verify.py` `_run_cmd`: parse the configurable clock markers from the streamed output; pause/resume wall-clock accounting against `verify_command_timeout_secs`; enforce `verify_clock_stop_heartbeat_idle_max` during stopped spans; optional `verify_clock_stop_max_total_secs` backstop. Config schema + defaults. No change to classification/requeue/cap. **Signal:** DF integration test (§8 DF face): heartbeating-past-budget completes; silent-during-stop is killed+infra; resumed wall-clock still catches a post-acquire hang. **Owner:** dark-factory. **deps:** the marker grammar from α (contract only — independently implementable against the documented grammar).
- **ε (reify) — deploy milestone (existing task 4838).** Gated on α/β/γ landed on main AND DF-δ deployed (orchestrator running the new code). Flip `orchestrator.yaml REIFY_TEST_SEMAPHORE_WAIT` → `unlimited` + enable the DF clock-stop knobs; restart via `scripts/orchestrator-redeploy-restart.sh` (human-coordinated, quiet window). **Signal:** post-restart, a slot-starved task verify *waits and completes* instead of landing `blocked`; the esc cluster does not recur. **deps:** α, β, γ, DF-δ.

DAG: α → β → {γ, DF-δ} → ε. DF-δ is the cross-repo external prerequisite of ε.

## 10. Reconciliation with the `60baafc2` branch (reuse / supersede / amend)

The dead branch (task 4837, base `6b2ca0d6`, **do not delete**) had the **right instinct for option (c)** — its core was an `unlimited`-wait mode + a 60s heartbeat + a `TEST_SLOT_STARVATION` token + correct gating ("don't enable unlimited before the seam — exit-124 is strictly worse"). This PRD **blesses that instinct** and supersedes the specifics:

- **Bless + re-express:** the `unlimited` continuous-wait mode and the heartbeat become α's continuous wait + the formal `@@REIFY_CLOCK_HEARTBEAT@@`. Re-express against the **post-4840** `lib_slot_acquire.sh` (the branch edited the pre-extraction inline loop; that file was refactored out from under it — a refactor-conflict, not a clean rebase). Reuse the branch's test scaffolding (T18/T19/T20, Section F) re-pointed at the markers.
- **Amend:** the branch had a *bare* heartbeat with **no STOP/START framing** for DF to consume, and was **semaphore-only**. This PRD adds the explicit STOP/START contract (so DF knows exactly which spans to exclude) and applies the uniform emitter to the **PSI gate** too (D2).
- **Supersede:** the branch's *active* config was effectively option (b) (finite WAIT + exit-75 + `TEST_SLOT_STARVATION` token, asking DF to *requeue*). Option (b) is rejected (§3, file-lock release). The token is repurposed as the `reason=` field, not a requeue trigger.
- **Supersede the seam filing:** the branch filed `escalate_info` (esc-4837-5/6) that were **dismissed/closed without landing** — leaving the DF half untracked. This PRD files DF-δ as a real, tracked dark-factory task with an external dependency edge (§7).

## 11. Open (tactical) questions

- **Heartbeat interval `H` and `heartbeat_idle_max`.** `H`=30s, idle_max=180s are starting points; tune from observed wait durations. Not a design blocker (knobs).
- **DF marker-parse placement.** Inline in `_run_cmd`'s stream loop vs a small wrapper around it — DF's call, against the documented grammar.
- **PSI-gate `WINDOW` redundancy.** With a continuous clock-stopped wait, the PSI gate's spacing `WINDOW` may be partly redundant; revisit (tactical, not blocking).
- **Pure-watchdog fallback.** If dark-factory prefers an inactivity watchdog (reset deadline on any output) over the explicit STOP/START state machine (D3), reify can heartbeat during quiet work phases too; the marker emission is forward-compatible with either. DF's call at DF-δ.
