# Infra-test wall-clock de-flake — load-independent assertions for timing-sensitive merge-gate tests

**Status:** draft (authored 2026-06-25)
**Consumer:** the merge gate / post-merge verify pipeline (`tests/infra/run_all.sh` run under `DF_VERIFY_ROLE=merge` by `verify.sh --scope all`), and transitively every task whose merge passes through that gate.
**Provenance:** `/deb` root-cause session 2026-06-25 (esc-4782-109/110); empirical merge-ambush forensics over `data/escalations/`.

## 1. Problem & user-observable surface

A class of `tests/infra/*.sh` tests gate the merge by asserting **absolute wall-clock upper bounds** on the elapsed time of a spawned subprocess (e.g. `MERGE_S < 4`, `elapsed <= 8s`, `MS <= 20000ms`). The measured quantity is dominated by **process-spawn + scheduling latency** — a property of the box at that instant, not of the code under test. Under normal merge-queue load these bounds trip RED and fail the *whole* merge gate, blocking merges whose diffs have **no causal path** to the test (an "ambush").

Empirical impact (escalation forensics, `data/escalations/`):

| Test | merge-gate escalations | distinct tasks ambushed | window |
|---|---|---|---|
| `test_verify_semaphore_e2e.sh` | **72** | **37** | 06-15 → 06-25 |
| warm-lane trio (`preflight`/`pool`/`refresh`) | 61 + 27 + 5 | 36 / 17 / 3 | 06-18 → 06-25 |
| `test_test_run_semaphore.sh` | 14 | 10 | 06-10 → 06-25 |
| `test_cpu_load_governance.sh` (all-ctx) | 13 | 6 | 06-17 → 06-22 |
| `test_occt_flock_gate.sh` (all-ctx) | 10 | 3+ | 05-30 → 06-11 |

The canonical member, `test_verify_semaphore_e2e.sh`, failed the day **after** task 4799's load-tolerance scaling landed (6-23: 14, 6-24: 15 ambushes — the two worst days in the series), proving the prior fix attempt did not address the root cause.

**Root cause (established in `/deb`):** the tests assert absolute wall-clock ceilings. Task 4799's `_LOAD_FACTOR = clamp(ceil(loadavg₁ₘᵢₙ / nproc), 1, 8)` scaling is the wrong remedy: `loadavg` diverges from the PSI scheduling-stall that actually inflates the preamble (measured live: loadavg 51–72 while PSI cpu `full=0%`, `some~36%`), it is sampled **once** at test start and lags (1-min average), and at `factor=1` (whenever `loadavg ≤ nproc` at the sampling instant) every bound reverts to the original fragile literal. The bare merge preamble is **0.33s** in isolation but the test budgets 4s and still flakes — the timed quantity is fork/exec latency, not work.

**User-observable surface when done:** the merge gate stops escalating on these tests (no more ambushed merges); each test still proves the **same wiring** it proved before, via load-independent assertions; the tests stay green at idle *and* under induced contention (PSI `some~50%`), and still go **RED** when the underlying mechanism is actually broken (non-vacuous).

## 2. Sketch of approach — the S/R/T/C toolkit

Replace each absolute-wall-clock **upper-bound** assertion with a load-independent proof of the same property. Four techniques; pick per assertion:

- **S — structural marker.** Assert a deterministic stderr/stdout marker or exit code that proves the code path was taken. Substrate (G3-verified to exist): `lib_test_semaphore.sh` / `cpu-admit.sh` emit `bypass (role=merge) — no slot acquired`; the disable path emits `disabled (REIFY_TEST_SEMAPHORE_DISABLE=1)`; `verify.sh` emits `FAILED (exit 75): test-run semaphore acquire`; `cpu-admit.sh` emits the fail-open `WARNING — … kernel lacks …`; `test_agent_cargo_shim.sh`'s stub already echoes a `STUB_CARGO <args>` sentinel captured in `SHIM_STDOUT`.
- **R — relative/causal ordering.** For serialization/concurrency proofs, assert event-timestamp ordering (acquire-B after release-A) instead of a duration. Requires a new opt-in event-log substrate (see §3 / decomposition T1).
- **T — anti-hang timeout only.** Keep a `timeout`/ceiling but make it generous enough that it *never* discriminates — only guards against a true hang. The real discriminator becomes the exit code / marker (e.g. `test_occt_flock_gate.sh` Tests 14/22: exit 75 is the proof; elapsed is just a guard).
- **C — drop ceiling.** Remove an absolute upper bound outright, keeping only a meaningful **lower-bound** discriminator (which can only false-*green*, never RED, under load) or a structural check.

**Lower bounds are not in this class** and are retained: `MS >= 3000` etc. can only weaken to false-green under load, never RED-flake.

In-repo precedent for the migration: `crates/reify-eval/tests/cancellation_compute_dispatch.rs` already replaced a wall-clock SLA with a load-independent **iteration-count** assertion (documented at its line ~132) — the same move, blessed here before.

**Non-vacuous mandate (the H / two-way boundary test).** Replacing a timing assertion risks making the test vacuously green (exactly what the temporary disable did). Every rewritten assertion MUST be shown to still go **RED under a deliberately broken/mutated mechanism** (e.g. Section B's bypass marker is absent if the merge-exemption is removed). This negative proof is a required part of each fix task's signal.

## 3. Pre-conditions (G3 substrate)

- **Exists (verified 2026-06-25):** all **S** markers above; `tests/infra/cpu_load_fixture.sh` (synthetic CPU-burn, task 4634) for the induced-contention acceptance check; `test_test_run_semaphore.sh` already has a structural bypass assert (Test 9) and exit-75 assert (Test 11).
- **New substrate (queued as T1):** the **R** technique needs an opt-in event-log emitting timestamped `ACQUIRE`/`RELEASE` events from the slot-acquire core. Per the CLAUDE.md mirror contract (`lib_test_semaphore.sh` ⇄ `cargo-test-occt-gated.sh` share the shuffle-acquire/deadline/FD-9 logic by **duplication**, "a fix here MUST also be applied there"), T1 extracts that shared logic into **one common sourced implementation** and houses the event-log there once — eliminating the mirror-maintenance hazard rather than duplicating the hook. Event format: `<epoch_ns> <pid> ACQUIRE slot-N` / `<epoch_ns> <pid> RELEASE`, single `printf` ≤ PIPE_BUF (atomic concurrent append), nanosecond resolution for ordering, **no-op when the env var is unset** (production path byte-for-byte unchanged).

## 4. Resolved design decisions

1. **Structural + relative-ordering direction** (not "fix the scaling signal"). Absolute wall-clock gates are abandoned for this class, not re-tuned. (Leo, this session.)
2. **Shared common core for the acquire logic + event-log** (not duplicated into both mirrored scripts). "Both tests should share a common implementation: reuse, don't duplicate logic." (Leo.) → T1 is a behavior-preserving refactor of two load-bearing verify-pipeline files.
3. **Warm-lane trio: audit-then-fix in this PRD** (T8) — confirm the wall-clock pattern before applying S/R/T; hand back findings for any out-of-class (reflink/disk) residue.
4. **Regression guard included** (T9) — a detector for NEW absolute-wall-clock upper bounds in `tests/infra`, lands after the suite is clean.
5. **`load_tolerance_lib.sh` stays** (benign poll-count consumers: `test_portable_timeout.sh`); only its use **as a prop for an absolute upper bound** is removed (e.g. `_LOAD_FACTOR` / Section E in the e2e test).

## 5. Out of scope

- **Deterministic plan/scope-drift gates** — `test_verify_throughput.sh`, `test_verify_scope.sh`, `test_verify_failfast_order.sh`. These ambush merges too, but the cause is plan-count / `DF_VERIFY_ROLE=merge` C2 role-contract drift (the #4618/#4624→#4288 class), not load timing. Different fix; separate track.
- **Production semaphore capacity** — the `N=2 → N=3` bump (commit `52f9075cb2`, esc-4800) already landed; the exit-75 *capacity* exhaustion is a distinct issue, not a test flake.
- **The temporary disable of `test_verify_semaphore_e2e.sh`** (commit `0b95e80b0c`) — already landed via the merge queue to protect the queue immediately. T2 **removes** the early-exit and restores coverage; until T2 lands the disable stands.

## 6. Cross-PRD relationship + seam owners

| Seam | Owner | Note |
|---|---|---|
| `test-run-concurrency-semaphore.md` (the semaphore itself) | this PRD touches only its **tests**, not its design | the N=3 capacity bump is theirs, not ours |
| `lib_test_semaphore.sh` ⇄ `cargo-test-occt-gated.sh` mirror | **T1** (this PRD) | refactor to a shared core resolves the duplication; both are load-bearing → full `--scope all` gate, not config fast-path |
| `cpu-load-admission-control.md` (`cpu-admit.sh`, shim) | this PRD touches only `test_cpu_admit.sh` / `test_agent_cargo_shim.sh` | structural markers already emitted by the impl |
| `warm-lane-pool-cow-seeding.md` (warm-lane scripts) | **T8 audit** decides | if the trio's flake is a distinct reflink/disk mechanism, it hands back to a warm-lane-owned follow-up |

No engine-integration seam (G1 engine sub-check N/A — this is test infrastructure).

## 7. Decomposition plan (one bullet per task → observable signal)

- **T1 — shared slot-acquire core + opt-in event-log** *(foundation; G5 contract)*. Extract the duplicated shuffle-acquire/deadline/FD-9 logic from `lib_test_semaphore.sh` and `cargo-test-occt-gated.sh` into one common sourced file; add the no-op-by-default timestamped event-log. **Signal:** the existing `test_test_run_semaphore.sh` *and* `test_occt_flock_gate.sh` stay green (behavior-preserving two-way boundary); a new unit test shows ordered `ACQUIRE`/`RELEASE` events emitted only when the env is set, and zero diff to stderr/exit when unset; green under `cpu_load_fixture` load. *(load-bearing → full gate)*
- **T2 — rewrite `test_verify_semaphore_e2e.sh`** *(canonical, #1 impact; depends on T1)*. Section A → **R** (causal acquire/release ordering, drop `A_UPPER`); Section B → **S** (assert `bypass (role=merge)`, drop `MERGE_S < EXEMPT_BOUND`); Section C → keep `RC==75` + stderr grep, **C/T** drop `C_S` budget + generous anti-hang timeout; delete `_LOAD_FACTOR` + Section E; **remove the disable early-exit**. **Signal:** green at idle AND under `cpu_load_fixture`-induced PSI `some~50%`; still RED under a mutated semaphore (e.g. merge-bypass removed ⇒ Section B marker absent); `run_all.sh` reports it discovered + passing non-vacuously.
- **T3 — `test_occt_flock_gate.sh`** *(depends on T1 for R)*. Tests 14/22 → **T** (generous deadline guard; exit-75 is the discriminator); Test 15 → **T** (upper side of the `[4,8]s` band); Tests 19/21A → **R** (shared event-log) or **T** sanity ceiling. **Signal:** green under load; exit-code/stderr discriminators preserved; serialization proven causally.
- **T4 — `test_test_run_semaphore.sh`** *(no T1 dep)*. Drop redundant wall-clock Tests 8/12 (Test 9 bypass-stderr + Test 11 exit-75-stderr already prove the paths structurally); Test 10 → **T** generous deadline. **Signal:** green under load; structural Tests 9/11 retained; no duration discriminator remains; still RED if the bypass path breaks.
- **T5 — `test_cpu_admit.sh`** *(no T1 dep)*. 6 cycles (A/D/F variants) → **S** (assert `bypass (role=merge)` / fail-open `WARNING` markers; drop the `< 2s` bounds). **Signal:** green under load; each cycle asserts its structural marker; still RED if the bypass/fail-open path breaks.
- **T6 — `test_agent_cargo_shim.sh`** *(no T1 dep)*. 6 cycles → **S** (assert the `STUB_CARGO` sentinel in `SHIM_STDOUT` / bypass markers; drop the `< 4–5s` bounds). **Signal:** green under load; sentinel/marker asserted per cycle.
- **T7 — `test_cpu_load_governance.sh:108`** *(no T1 dep; lower priority)*. `_LIVE_BUDGET_S` → **T** generous anti-hang or convert to a `quiet_box_met`-style skip-gate. **Signal:** green under load; no wall-clock discriminator gates the verdict.
- **T8 — audit + fix the warm-lane trio** *(audit-gated)*. Confirm whether `test_warm_lane_preflight.sh` / `test_warm_lane_pool.sh` / `test_refresh_warm_base.sh` assert absolute wall-clock bounds; apply S/R/T to those that are in-class. **Signal:** each in-class warm-lane test green under load with structural/relative assertions; a written findings note for any out-of-class (reflink/disk) flake handed to the warm-lane owner.
- **T9 — regression guard** *(depends on T2–T8; lands last)*. A detector (grep-based, wired into the infra suite or a `reify-audit` pattern) flagging NEW absolute-wall-clock **upper-bound** assertions in `tests/infra`, allowlisting lower-bound discriminators and generous anti-hang timeouts. **Signal:** the guard goes RED on a planted violation and GREEN on the cleaned suite; wired so a future reintroduction fails CI.

Dependency edges: T2→T1; T3→T1; T9→{T2,T3,T4,T5,T6,T7,T8}. T4–T8 are independent of T1 and of each other (parallelizable).

## 8. Open (tactical) questions

- T1: new common file name/location (`scripts/lib_slot_acquire.sh`?) and the exact env-var name(s) for the event-log (`REIFY_SLOT_EVENT_LOG`?) — implementation-time.
- T3: whether Tests 19/21A are worth converting to R or are fine as generous-ceiling **T** (cheaper) — decide at implementation from how tight the current proof needs to be.
- T9: host in the infra suite (a `test_*.sh`) vs as a `reify-audit` PDEAD/PTODO-style pattern — pick whichever the guard's allowlist is cleaner to express in.
- T8: if the warm-lane flake is reflink/disk-timing (not process-latency), the fix may need FS-layer mocking rather than S/R/T — that residue is explicitly handed off, not forced into this PRD.
