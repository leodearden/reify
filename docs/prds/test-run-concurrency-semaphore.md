# PRD — Test-run concurrency semaphore (held-slot, merge-exempt) + occt-cap raise

**Status:** authored 2026-06-10. Version-agnostic infrastructure foundation (root-level `docs/prds/`).
**Approach:** B + H (contract + two-way boundary tests) — the verify pipeline every task and every merge depends on this; failure modes (merge starvation, queue collapse, FD-leak wedge) are subtle and host-wide.

## 1. Consumer & user-observable surface

**Consumer:** `scripts/verify.sh`'s test-execution phase, which the dark-factory orchestrator already invokes (`test_command: ./scripts/verify.sh test …`, orchestrator.yaml:43) for every per-task verify, and which `hooks/pre-merge-commit` invokes (`verify.sh all --profile both --scope all`, pre-merge-commit:37) for every merge gate. The semaphore is not a new free-standing tool needing a future caller — its caller is the verify pipeline that runs on **every** task and merge today.

**User-observable surface** (operator / orchestrator):
- Concurrent **task** test runs across worktrees serialize to a hard bound N (default 1), observable as wall-clock serialization in `tests/infra/` behavioral tests (the established pattern: `test_occt_flock_gate.sh` 23 timing/FD/exit tests; `scripts/test_psi_gate.sh`).
- The **merge** gate is exempt (never waits behind a task), observable as a `DF_VERIFY_ROLE=merge` run proceeding while a task slot is held.
- Under contention beyond the wait budget the test phase exits **75 (EX_TEMPFAIL)** → orchestrator **requeues** (no spurious task-fail), reusing the exact contract the PSI gate already relies on (verify.sh:228).
- `cargo nextest run`'s `occt` test-group runs at **24** concurrent (was 4), observable in `.config/nextest.toml` and in `verify.sh test --print-plan`.

## 2. Premise correction (what actually exists)

The flock semaphore (`scripts/cargo-test-occt-gated.sh`) was **not silently dropped** by task 4451 — its cross-run role was replaced by **`psi_gate()`** (verify.sh:146), wired as the first test-phase step. The PSI gate already provides:
- merge exemption via `DF_VERIFY_ROLE=merge` (verify.sh:161),
- exit-75-on-`MAX_WAIT` → orchestrator retry (verify.sh:228),
- tunable knobs + break-glass disable + cross-worktree coordination (flock'd `/tmp` timestamp).

**But** the PSI gate is an *admission* gate (`avg10 < THRESHOLD` **and** `≥WINDOW` since last dispatch), not a *held-slot* bound: it releases its lock the instant a run starts, so it does **not** cap concurrent runs. Under our startup-dominated suite, several runs admit during the low-pressure init window (pressure lags), then collectively overshoot — the load-65 we measured 2026-06-10. This PRD adds the hard concurrency bound the PSI gate structurally cannot give, **layered on** (not replacing) the PSI gate so its pressure-reactivity (which also backs off under *compile* load, since it reads total `/proc/pressure/cpu`) is retained.

## 3. Sketch of approach

Two coupled changes:

**(A) Held-slot test-run counting semaphore, composed with the PSI gate.** A new sourceable lib (`scripts/lib_test_semaphore.sh`) provides acquire/hold/release of an N-slot semaphore using the proven mechanics of `cargo-test-occt-gated.sh`: shuffle-order acquire over `${LOCK}.slot-1..N`, slot held on an FD for the run's whole duration, child exec'd with the FD closed (`9<&-`) so no descendant (sccache, rustc) leaks the slot, deadline-checked-before-sleep, exit 75 on `LOCK_WAIT`. `verify.sh` runs `psi_gate()` first (pressure headroom), **then** acquires the slot, **then** runs the test passes with the slot held, releasing after the last pass. Gated region = the **test-execution passes only** — never `psi-gate` itself, never compile/check/clippy (compiles are already host-bounded by the shared jobserver). Merge-exempt: when `DF_VERIFY_ROLE=merge`, skip acquisition entirely (mirrors the PSI bypass).

**(B) Raise the `occt` nextest test-group cap 4 → 24, env-driven.** With concurrent runs now hard-bounded to ≤2 (1 task + 1 exempt merge), the intra-run OCCT RSS budget can rise: worst case 2 runs × 24 OCCT × ~2 GiB = 96 GiB < 125 GiB host — guaranteed no swap, while the occt group stays a *real* (non-inert) RSS backstop below the nproc=32 global. `.config/nextest.toml` literal → 24; `verify.sh` passes `--config 'test-groups.occt.max-threads=${REIFY_OCCT_NEXTEST_MAX_THREADS:-24}'` so it's dialable without editing tracked config.

**Coupling (the load-bearing ordering):** (B) is safe **only** once (A) is live in `verify.sh`. Raising the per-run OCCT cap without the hard cross-run bound would let `(24+1)` runs each spawn up to 24 OCCT processes — worse than today's cap=4 floor. The decomposition encodes this as a dependency edge (γ depends on β), not a comment.

## 4. Pre-conditions (substrate — all verified present 2026-06-10)

| Capability | Status | Evidence |
|---|---|---|
| `flock`, `timeout` on PATH | present | `cargo-test-occt-gated.sh:100-109` preflight already requires them |
| `DF_VERIFY_ROLE=task\|merge` lane signal | present | verify.sh:288/302; orchestrator merge queue injects `=merge` (orchestrator.yaml:35) |
| exit-75 → orchestrator requeue | present | PSI gate already returns 75 and is requeued (verify.sh:228; orchestrator honors it today) |
| `nextest --config 'test-groups.occt.max-threads=N'` override | present | accepted by cargo-nextest 0.9.136 (verified empirically 2026-06-10) |
| `tests/infra/run_all.sh` auto-discovery of `test_*.sh` | present | run_all.sh:2 discovers all `test_*.sh`; verify.sh runs it as a plan line |
| `/proc/pressure/cpu` (PSI gate dependency) | present | psi_gate reads it (verify.sh:151); fail-open on absence |

No novel `.ri` grammar — the grammar gate (G3) is N/A for this PRD (shell + nextest-config only).

## 5. Resolved design decisions

- **D1 — Augment, don't replace (user, 2026-06-10).** Held-slot semaphore is *layered on* `psi_gate()`. Semaphore = hard test×test bound; PSI gate = pressure-reactive backoff that also covers test×compile. Both kept.
- **D2 — Compose order: PSI-wait → acquire-slot → run.** Acquire *after* PSI passes so the scarce slot is never held idle during a pressure wait; two tasks may both clear PSI then contend for the slot (the loser waits holding nothing — fine).
- **D3 — N default 1, env-tunable** (`REIFY_TEST_SEMAPHORE_CONCURRENCY`). N=1 is the throughput-optimal point for a startup-dominated suite (wide single runs amortize per-test init best) and the conservative host-load point (1 task ≈ 1× nproc, +merge ≈ 2×). Tunable up if realized queue depth warrants.
- **D4 — occt cap default 24, env-driven** (`REIFY_OCCT_NEXTEST_MAX_THREADS`), via `--config` override + static literal. Keeps a guaranteed no-swap RSS backstop under the 2-run worst case.
- **D5 — Merge exemption made uniform.** The orchestrator merge-queue path already sets `DF_VERIFY_ROLE=merge`; the **local** path (`hooks/pre-merge-commit`, `land.sh`) does **not** (defaults to `task`) — β sets it, so the held-slot (and PSI) exemption covers both the queue path and manual `land.sh`. Without this, a manual land could queue behind a task slot (the merge-starvation/livelock risk this whole design exists to avoid).
- **D6 — Gate the test-execution phase only.** Compiles overlap freely (jobserver-bounded host-wide); the semaphore wraps only the nextest/cargo-test passes. This is why the held slot can't be a fire-and-return plan line like `psi-gate` — it spans multiple pass executions, so `verify.sh` holds the FD in its own process and `--print-plan` marks the gated region with a comment, not an executable line.

## 6. Out of scope / accepted limitations

- **test×compile residual.** The semaphore bounds test×test; the jobserver bounds compile×compile; their *sum* is still unbounded (the load-65 cause). The PSI gate mitigates it (delays the test run while compile pressure is high) but doesn't eliminate it. Fully unifying the two budgets is out of scope; `nice` (existing orchestrator spawn policy) handles residual merge-vs-task CPU priority.
- **No dark-factory change.** The cross-repo seam dissolves: the orchestrator already injects `DF_VERIFY_ROLE=merge` (queue path) and already requeues on exit 75. This PRD ships entirely reify-side.
- **Idle-host cap-4-vs-24 benchmark.** Deferred — the 2026-06-10 measurement was confounded by load 65 (>200 tasks queued, no predictable idle window). We proceed on the strong conjecture that cap raise sometimes helps; no leaf asserts a speedup factor (G6), so no false premise is frozen into a test.
- `cargo-test-occt-gated.sh` itself stays as the standalone/manual OCCT runner (unchanged); its mechanics are the template α copies, not a file α edits.

## 7. Cross-PRD / cross-repo relationship & seam ownership

| Seam | Owner | Resolution |
|---|---|---|
| Lane signal (task vs merge) | **existing** (`DF_VERIFY_ROLE`) | orchestrator already sets it on the queue merge path; β extends it to the local merge path |
| exit-75 → requeue | **existing** (PSI-gate contract) | orchestrator already requeues; semaphore reuses verbatim |
| merge-vs-task CPU priority | **existing** (orchestrator `nice`/`ionice` spawn) | unchanged; semaphore exempts merge so it's a fairness nicety, not a correctness req |
| occt-touching crate set / drift | `scripts/occt-touching-crates.txt` + `test_occt_gated_scope.sh` | γ only changes the cap value, not the set — drift-catcher unaffected |

No new contested-ownership pair introduced; no reciprocal "the other owns it."

**Subsequent refactor (cpu-load-admission-control PRD, tasks α–γ):** the shared PSI-admission core from `psi_gate()` and `compile_gate()` was subsequently extracted into `scripts/cpu-admit.sh` (task α). The `verify.sh` wrappers (`psi_gate` / `compile_gate`, verify.sh:210-272) now map `REIFY_PSI_GATE_*` / `REIFY_COMPILE_GATE_*` knobs onto the `_ca_*` contract and delegate to `cpu_admit requeue` / `cpu_admit admit`; the held-slot semaphore (`scripts/lib_test_semaphore.sh`) is unchanged and composes below cpu-admit. The cpu-load PRD adds an orthogonal agent-spawn axis (cgroup cpu.weight γ + per-command PSI admission β) that does not interact with this PRD's semaphore. See `docs/prds/cpu-load-admission-control.md`.

## 8. Boundary-test sketch (H — facing both ways)

- **Mechanism face (α):** `tests/infra/test_test_run_semaphore.sh` drives the lib directly — N=1 held-slot serialization (slot held for the *command's full duration*, distinguishing it from PSI admission-spacing); `role=merge` exemption; exit-75 on acquisition deadline; FD-not-leaked-to-surviving-daemon (the 2026-04-20 regression class).
- **Integration face (ε):** an e2e test drives real `scripts/verify.sh` the way the orchestrator does — two concurrent `verify.sh test` (`role=task`) hold-serialize; a `role=merge` run is exempt; exit 75 propagates out of `verify.sh`; the emitted plan shows occt cap=24 and the compile/check passes *outside* the gated region.

## 9. Decomposition plan (one bullet per leaf → its observable signal)

- **α — held-slot test-run semaphore lib** (`scripts/lib_test_semaphore.sh`). Keystone mechanism; reuses `cargo-test-occt-gated.sh` held-FD/`9<&-`/deadline/exit-75 mechanics; merge-exempt on `DF_VERIFY_ROLE=merge`; knobs `REIFY_TEST_SEMAPHORE_{CONCURRENCY,LOCK,WAIT,DISABLE}`. **Signal:** new `tests/infra/test_test_run_semaphore.sh` (auto-run by run_all.sh) — held-slot serialization, merge-exempt, exit-75, FD-non-leak. deps: none.
- **β — wire α into `verify.sh` test phase + uniform merge exemption.** PSI-wait → acquire → run-passes-held → release, around the test-execution passes only; propagate exit 75; `--print-plan` marks the gated region; set `DF_VERIFY_ROLE=merge` in `hooks/pre-merge-commit` (and `land.sh`). **Signal:** `verify.sh test --print-plan` shows the gate wrapping nextest passes and NOT compile/check/clippy; execute-mode: two concurrent `verify.sh test` serialize, `role=merge` exempt; pre-merge-commit run reports role=merge. deps: α.
- **γ — raise occt cap 4→24, env-driven; refresh headroom basis.** `.config/nextest.toml` → 24; verify.sh `--config 'test-groups.occt.max-threads=${REIFY_OCCT_NEXTEST_MAX_THREADS:-24}'`; update headroom comment (125 GiB; 2×24×2=96 GiB) + `docs/notes/multi-process-occt-bench.md`. **Signal:** `.config/nextest.toml` shows 24; infra assertion that the override appears in `verify.sh test --print-plan`; bench doc updated. **deps: β** (coupling — cap raise safe only once the hard run-bound is live).
- **δ — surface the contract** in `orchestrator.yaml` + `CLAUDE.md`. Document knobs, `DF_VERIFY_ROLE` exemption, exit-75 reuse (no dark-factory change); add a "Test concurrency" subsection near "Landing on main". **Signal:** orchestrator.yaml + CLAUDE.md updated with the contract + verify.sh:161/228 citations. deps: β.
- **ε — integration gate (critical leaf).** E2e infra test through real `verify.sh`: N concurrent `role=task` runs hold-serialize; `role=merge` exempt; exit-75 propagates; plan shows cap=24 and compiles outside the gated region. **Signal:** the e2e test passes in `tests/infra/run_all.sh`. deps: β, γ, δ.

DAG: α → β → {γ, δ} → ε.

## 10. Open (tactical) questions

- **Queue stability at N=1.** If aggregate task test-execution demand exceeds one run's capacity, the single slot backs up and tasks tail into `LOCK_WAIT`→75→requeue (backpressure, not failure — agents may idle-wait holding a worktree slot). Mitigation: generous `REIFY_TEST_SEMAPHORE_WAIT`; monitor realized queue depth; raise N if it bites. Not a design blocker — N is a knob.
- **Acquire granularity:** once around all test passes (chosen — simpler) vs per-pass (releases between passes). Tactical; revisit only if inter-pass gaps prove to waste the slot.
- **Should `verify.sh lint`/`typecheck` phases also be gated?** No for now (compile-bound, jobserver-covered); only the test-execution phase holds the slot.
- **PSI `WINDOW` redundancy under a hard cap.** With N=1, the PSI `WINDOW` spacing is partly redundant for test×test; its `THRESHOLD` (compile-pressure backoff) stays valuable. Consider simplifying `WINDOW` later.
