# Verify-pipeline admission control & CPU governance — operational reference

Moved from `CLAUDE.md` 2026-07-02 (radical trim). This note is the **operational digest**: gate semantics, compose order, knob tables, thresholds, and the clock-stop marker contract. Design authority remains the PRDs:

- `docs/prds/verify-admission-wait-clock-stop.md` — **authoritative**; §2 corrects the exit-75-requeue premise
- `docs/prds/cpu-load-admission-control.md` — agent-spawn CPU axis (§5 design, §9 deploy/seam table, §10 out-of-scope)
- `docs/prds/test-run-concurrency-semaphore.md` — historical; §4/§6/§7 superseded

Prefer stable function names (`compile_gate`, `psi_gate`, `test_semaphore_acquire`, `@@SEMAPHORE_ACQUIRE@@`/`@@SEMAPHORE_RELEASE@@`) over line numbers for durable code links.

## The three verify-pipeline admission controls

The verify pipeline is governed by three admission controls that layer in order: **`compile_gate()`** (compile-phase PSI backpressure, task 4618) → **`psi_gate()`** (test-phase PSI backoff) → **held-slot semaphore** (hard test×test cap) → run passes.

- **`compile_gate()`** (`scripts/verify.sh`, task 4618, extended task 4853 + 4861): soft PSI admission backstop for the **clippy/check/compile** phases (lint/typecheck/all actions) and the **nextest test-binary `--no-run` link** (test/all actions). Wired via `verify.sh compile-gate` as a plan line: (a) immediately before cargo check/clippy on the lint/typecheck side (`build_plan()`, after tree-sitter prereq); and (b) immediately before the nextest `--no-run` test-binary compile on the test path (`add_test_passes()`, after `psi-gate`, before `@@SEMAPHORE_ACQUIRE@@`) — the PSI/RSS backstop for the heavy nextest link that task 4839 moved outside the held slot (task 4853). On action=all **both** fire deliberately: the early one staggers clippy/check; the late one re-checks PSI before the heaviest test-binary link wave (PSI can change materially across the long clippy/check phase). Reads **two PSI dimensions**: `/proc/pressure/cpu` avg10 (backs off when `cpu_avg10 >= 85 %`) **AND `/proc/pressure/memory` memfull avg10 (second dimension, default-ON, backs off when `memfull_avg10 >= 10 %`)**. Both dimensions must drop below their ceilings to admit. **Continuous HOLD until PSI drops (task 4920 — admit-on-timeout removed); NEVER exits 75.** compile admission is soft backpressure — it can delay/stagger a compile start but can **never requeue a task** — structurally storm-proof. Once a wait is entered it emits `@@REIFY_CLOCK_STOP@@` → `@@REIFY_CLOCK_HEARTBEAT@@` → `@@REIFY_CLOCK_START@@` (`reason=psi_pressure`), reusing the same reason token as `psi_gate`'s requeue path — compile_gate is now **IN clock-stop scope** (PRD D2 reversed; see "Exit-75 premise correction & clock-stop seam" below). Under *permanent* host saturation the hold is indefinite by design (no admit-on-timeout floor left), but it heartbeats rather than hangs (the wait span stays clock-stop-excluded from `verify_command_timeout_secs`) — the same accepted limitation as the semaphore: no verify-layer scheme fixes starvation under permanent saturation, the lever is dispatch admission. The two dimensions are NOT equally likely to trigger this: the memory ceiling (`memfull` avg10 >= 10%, default) is far more conservative than the CPU ceiling (avg10 >= 85%), so ambient host memory pressure alone — unrelated to the verify itself — is the practically more likely indefinite-hold trigger of the two on a busy multi-tenant box. No WINDOW/dispatch-file/flock (compiles run concurrently under the jobserver). `DF_VERIFY_ROLE=merge` → immediate bypass (CAVEAT 1: merge never waits). Introduces **zero host-baked constants**: only PSI %s + durations — host-portable by kernel normalization (no nproc-derived count). Reacts to **both** dimensions with the same firm hold — task 4920 folded the previously-deferred "firmer hold under sustained memfull" follow-up into this change.
- **`psi_gate()`** (`scripts/verify.sh`, task 4861): pressure-reactive admission backoff for the **test-execution** phase. Reads **two PSI dimensions**: `/proc/pressure/cpu` avg10 (blocks until CPU avg10 drops below 50 %) **AND `/proc/pressure/memory` memfull avg10 (second dimension, default-ON, blocks until memfull avg10 drops below 10 %)**, with a spacing window (default 20 s). Both conditions must be met simultaneously. Guards **test × compile** contention — any concurrent verify phase counts, not just test passes. `DF_VERIFY_ROLE=merge` exempts both dimensions. v1 = staggering only (a firmer hold = follow-up).
- **Held-slot semaphore** (`scripts/lib_test_semaphore.sh`): hard **test × test** concurrency cap. Holds an exclusive flock on FD 9 across all test passes so at most **N** verifies run their test-execution phase simultaneously (default `N=1`). Compile, check, clippy, infra steps, and `psi_gate()` itself are **outside** the gated region.

**Why the compile-gate threshold is 85 (not 50):** The dual-pool jobserver is merge-favored — `task_baseline = max(1, nproc//4)` of tokens are reserved for task lanes (e.g. 8 task / 24 merge at nproc=32; scales with the host). During a healthy EXEMPT merge, the box legitimately runs hot. A lone merge holding its reserved core fraction does NOT by itself drive avg10 to 85 (PSI measures runnable-task stall, not utilization); only sustained multi-lane oversubscription does. The jobserver-balancer already holds task pools at avg10 ≥ 50 (mirroring `psi_gate`'s threshold); the compile-gate at 85 is a deliberately coarser verify.sh-layer backstop for when the hold + jobserver cap are insufficient (implicit-token leak + non-cargo load). The threshold is a tunable knob — no empirical level is frozen into any test.

**Compose order:** `compile-gate` (lint/typecheck/all: before clippy/check) → `psi-wait` (test/all: before nextest) → `acquire-slot` → `run-test-passes-with-slot-held` → `release-slot`. The `@@SEMAPHORE_ACQUIRE@@` sentinel is emitted by `add_test_passes()` (`verify.sh`) AFTER the `psi_gate()` entry, so the slot is not occupied during a pressure wait. `@@SEMAPHORE_RELEASE@@` marks the end of the gated region. Both sentinels are handled in the executor and annotated by `--print-plan`.

## Knobs

**Knobs — compile-gate** (`scripts/verify.sh compile_gate()`):
- **`REIFY_COMPILE_GATE_THRESHOLD`** — CPU avg10 % ceiling (default `85`; host-portable PSI %)
- **`REIFY_COMPILE_GATE_MAX_WAIT`** — inoperative for the hold (task 4920: compile_gate now holds until PSI drops rather than admitting on a timeout); retained only as the fallback value for `cpu_admit`'s defensive reason-less-admit guard, a path compile_gate itself never takes (default `300`; never exit 75)
- **`REIFY_COMPILE_GATE_POLL`** — recheck interval in seconds (default `5`)
- **`REIFY_COMPILE_GATE_PROC_PATH`** — CPU PSI source (default `/proc/pressure/cpu`; testability knob)
- **`REIFY_COMPILE_GATE_DISABLE`** — set to `1` for total bypass (break-glass)
- **`REIFY_COMPILE_GATE_MEM_PROC_PATH`** — memory PSI source (default `/proc/pressure/memory`; testability knob)
- **`REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD`** — memfull avg10 % ceiling (default `10`; conservative; healthy hosts sit ~0% memfull; tunable independently from CPU threshold; empty = memfull dimension OFF; markedly lower than the 85% CPU ceiling, making it the more easily-tripped indefinite-hold trigger under the continuous-hold semantics — see the `compile_gate()` description above)
- **`REIFY_COMPILE_GATE_MEM_SOME_THRESHOLD`** — memsome avg10 % ceiling (default empty = OFF; opt-in early-warning)

**Knobs — psi-gate** (`scripts/verify.sh psi_gate()`):
- **`REIFY_PSI_GATE_THRESHOLD`** — CPU avg10 % ceiling (default `50`)
- **`REIFY_PSI_GATE_WINDOW`** — minimum inter-dispatch spacing in seconds (default `20`)
- **`REIFY_PSI_GATE_MAX_WAIT`** — give-up timeout (default `1800`; exits 75 on timeout)
- **`REIFY_PSI_GATE_POLL`** — recheck interval in seconds (default `5`)
- **`REIFY_PSI_GATE_PROC_PATH`** — CPU PSI source (default `/proc/pressure/cpu`; testability knob)
- **`REIFY_PSI_GATE_DISPATCH_FILE`** — coordination timestamp file (default `/tmp/reify-verify-last-dispatch`)
- **`REIFY_PSI_GATE_DISABLE`** — set to `1` for total bypass (break-glass)
- **`REIFY_PSI_GATE_MEM_PROC_PATH`** — memory PSI source (default `/proc/pressure/memory`; testability knob)
- **`REIFY_PSI_GATE_MEM_FULL_THRESHOLD`** — memfull avg10 % ceiling (default `10`; same conservative reasoning as compile-gate; tunable; empty = memfull dimension OFF)
- **`REIFY_PSI_GATE_MEM_SOME_THRESHOLD`** — memsome avg10 % ceiling (default empty = OFF; opt-in early-warning)

**Knobs — test semaphore** (`scripts/lib_test_semaphore.sh`):
- **`REIFY_TEST_SEMAPHORE_CONCURRENCY`** — slot count N (default `1`)
- **`REIFY_TEST_SEMAPHORE_WAIT`** — max seconds to wait for a slot (default `1800`), OR the sentinel `"unlimited"` (case-insensitive) for a continuous blocking wait with no deadline (clock-stop mode). **ACTIVATED 2026-06-27 (task 4838):** continuous wait live; `dark_factory:1916` deployed; `WAIT=unlimited` in `orchestrator.yaml`; `@@REIFY_CLOCK_*@@` span excluded from `verify_command_timeout_secs`.
- **`REIFY_TEST_SEMAPHORE_LOCK`** — base path for slot files (default `/tmp/reify-test-semaphore-$(id -u).lock`; fixed host-global path, independent of TMPDIR — task 5145)
- **`REIFY_TEST_SEMAPHORE_DISABLE`** — set to `1` for a total bypass (no slot acquired)
- **`REIFY_CLOCK_HEARTBEAT_SECS`** — interval (s) between `@@REIFY_CLOCK_HEARTBEAT@@` emissions in the semaphore + PSI poll loops (default `30`; reduce in tests for faster runs)

**`DF_VERIFY_ROLE=merge` exemption:** all three admission controls (`compile_gate`, `psi_gate`, `test_semaphore_acquire`) skip acquisition when `DF_VERIFY_ROLE=merge`. The merge gate **never waits behind a task slot**. This exemption fires on both paths: the orchestrator queue merge path (orchestrator injects `DF_VERIFY_ROLE=merge`) and the local `land.sh`/`pre-merge-commit` path.

## Exit-75 premise correction & clock-stop seam

**Premise correction — DF does NOT requeue verify exit-75 (PRD verify-admission-wait-clock-stop §2):** the earlier claim that "the orchestrator treats exit 75 as retry-capped transient infra and requeues the task" is **FALSE**. Verified in DF source: `verify.py _classify_failure` falls exit-75 through to `unknown_test_failure` → debugfix loop → **BLOCKED**. This is the true mechanism behind task 4800 and the esc-3891-45/esc-4673-31/esc-4552 cluster. `docs/prds/test-run-concurrency-semaphore.md` §4/§6/§7 which stated the requeue premise are superseded by `docs/prds/verify-admission-wait-clock-stop.md`.

**Clock-stop seam — the real fix (task 4837/4838):** instead of requeue, both admission gates use a **continuous in-process blocking wait** (holding file locks + warm lane start-to-finish), emitting uniform `@@REIFY_CLOCK_*@@` markers to stderr so `dark_factory:1916` can exclude the wait span from `verify_command_timeout_secs`:

- **`@@REIFY_CLOCK_STOP@@`** `reason=<reason> pid=<pid>` — emitted ONCE on entering the wait (first immediate acquire fails)
- **`@@REIFY_CLOCK_HEARTBEAT@@`** `reason=<reason> waited=<secs>` — emitted every `REIFY_CLOCK_HEARTBEAT_SECS` from INSIDE the poll loop (liveness — a wedged loop stops heartbeating)
- **`@@REIFY_CLOCK_START@@`** `reason=<reason> waited=<secs>` — emitted ONCE on successful acquire (STOP/START are balanced; uncontended fast-path emits nothing)
- Reason vocabulary: `test_slot_starvation` (semaphore path), `psi_pressure` (PSI gate path)
- `REIFY_TEST_SEMAPHORE_WAIT=unlimited` / `REIFY_PSI_GATE_MAX_WAIT=unlimited` activate the continuous wait (no deadline, never exit-75); `REIFY_CLOCK_HEARTBEAT_SECS` tunes the heartbeat interval (default 30s)
- **ACTIVATED 2026-06-27 (task 4838, PRD §5 D5):** `dark_factory:1916` deployed; WAIT knobs now `"unlimited"` in `orchestrator.yaml`; the `@@REIFY_CLOCK_*@@` span is excluded from `verify_command_timeout_secs` by DF:1916. A genuinely-wedged wait (no heartbeat within `verify_clock_stop_heartbeat_idle_max=600s`, raised 2026-07-08 from 180s per task 5156 — the 180s value false-killed verifies whose clock-stop span was followed by a long silent native-crate compile/link) is still killed by the orchestrator.
- **The compile-gate still NEVER exits 75** — it now HOLDS until PSI drops on either dimension rather than admitting on a timeout, and it never requeues; task 4920 moved it INTO clock-stop scope (PRD D2 reversed), reusing the `psi_pressure` reason token alongside `psi_gate`'s requeue path. Under permanent host saturation the hold is indefinite by design and heartbeats rather than hangs — the same accepted limitation as the semaphore: forward progress under sustained pressure is a dispatch-admission concern, not a verify-layer one.

## Agent-spawn CPU axis (orthogonal to the verify pipeline)

The three controls above govern the **verify pipeline** (compile + test phases inside `verify.sh`). A separate, orthogonal **agent-spawn CPU axis** governs CPU-time allocation and PSI admission for **agent processes themselves** (distinct from the commands those agents run). The two axes compose: an agent's verify invocations pass through the pipeline controls above; the agent process itself is governed by the axis below.

**Full agent-spawn compose order** (applied by dark-factory ζ at agent launch, referencing `orchestrator.yaml cpu_governance:`):

1. **`scripts/cpu-governed-exec.sh --role <task|merge>`** (γ, task 4632) — applied **once per agent at spawn**, places the agent's entire process tree in a cgroup-v2 scope with `cpu.weight` set by role (`W_task=100` / `W_merge=300`; mirrors the jobserver's ≈3:1 merge:task baseline). Work-conserving: a lone agent absorbs the full box; throttle only fires under true contention. Fail-open (C-G4): when cgroup governance is unsupported or `REIFY_CPU_GOVERN_DISABLE=1`, emits a warning and `exec`s the agent directly (with `nice` de-prioritization if available). Never blocks. Knobs: `REIFY_CPU_GOVERN_W_TASK`, `REIFY_CPU_GOVERN_W_MERGE`, `REIFY_CPU_GOVERN_DISABLE`.
2. **`scripts/agent-bin/cargo`** → **`scripts/cpu-admit.sh admit`** (β over α, tasks 4631/4630) — the PSI-admission shim intercepts **heavy `cargo` subcommands** (`build`, `test`, `check`, `clippy`, `nextest`) **per command** inside the agent, admitting only when avg10 < `REIFY_CPU_ADMIT_AGENT_THRESHOLD` (default 50 %) **AND memfull avg10 < `REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD`** (memory dimension, **default-ON at 10 % as of task 4911** — the shim sets no memory env of its own, so this default is inherited entirely from `cpu-admit.sh`'s direct-exec default, matching the verify-pipeline wrappers' own `:-10` memfull defaults; memsome stays OFF/empty; set `REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=""` to disable the memory dimension entirely on this axis). Admit-mode never exits 75 (fail-open). Non-heavy subcommands (`--version`, `metadata`, `fmt`, etc.) bypass the gate entirely (C-S1 fast-path); the merge bypass (`DF_VERIFY_ROLE=merge`) is unchanged and skips both dimensions. Knobs: `REIFY_CPU_ADMIT_AGENT_THRESHOLD` (CPU; independently tunable from verify.sh's `compile_gate` threshold of 85 %) and `REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD` (memory; shared with the direct-exec CLI — an independent per-shim `REIFY_CPU_ADMIT_AGENT_MEM_FULL_THRESHOLD` knob in `scripts/agent-bin/cargo` is a separate, optional follow-up).
3. **Held-slot semaphore** (the existing `scripts/lib_test_semaphore.sh` region, unchanged) — test×test hard cap; composes below the two agent-spawn controls.

**Three orthogonal axes:**

| Axis | Mechanism | Scope | Knob family |
|---|---|---|---|
| CPU-time share | cgroup `cpu.weight` (γ) | once per agent process | `REIFY_CPU_GOVERN_W_*` |
| PSI admission | `cpu-admit.sh admit` (α via β) | per heavy `cargo` subcommand | `REIFY_CPU_ADMIT_AGENT_THRESHOLD` (CPU), `REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD` (memfull, default-ON at 10 as of task 4911) |
| Test×test count | held-slot semaphore (lib_test_semaphore.sh) | per verify test phase | `REIFY_TEST_SEMAPHORE_*` |

Dark-factory ζ activates the agent-launch path by reading `orchestrator.yaml cpu_governance:` — the `DF_AGENT_CPU_GOVERN: 1` value signals that reify's primitives are wired. Reify ships α/β/γ; ζ does the wiring (cross-repo seam).

## Merge-worker trivial-pass drift guard (background)

The merge worker's **trivial-pass** fast-path (scope=config, diff touches only non-Rust/non-TS files) lands config-only changes (e.g. `orchestrator.yaml` tweaks) without a full `--scope all` verify.

**Drift-guard exception — verify-pipeline files are NOT trivially config-only.** Changes touching `scripts/verify.sh`, its live `source`d libs (`occt-scope-lib.sh`, `release-scope-lib.sh`, `affected-crates-lib.sh`, `lib_test_semaphore.sh`), or the verify-pipeline data files (`.config/nextest.toml`, `scripts/occt-touching-crates.txt`, `scripts/release-sensitive-crates.txt`, `scripts/verify-pipeline-infra-tests.txt`, `scripts/gen-nextest-config.sh`) are NOT safe to fast-path even though they are non-Rust/non-TS — these files load-bear the `--scope all` plan, and a plan-count change that skips the full gate ambushes the next Rust task with a RED `tests/infra/test_verify_throughput.sh` (root-caused via esc-4288-206; the #4618/#4624 → #4288 ambush is the canonical incident).

**Doc-sync extension (task 4955) — operational docs are NOT trivially config-only either.** The 2026-07-02 W8 incident (esc-4791-35 / esc-4906-34) is the **doc-side analogue** of the script-side ambush above: commit 96ce210ebd (the CLAUDE.md radical trim) moved this operational digest out of CLAUDE.md into `docs/notes/verify-pipeline-knobs.md` (this file) and landed it via the trivial-pass fast-path. That fast-path skipped `tests/infra/run_all.sh`, so `test_verify_compile_gate.sh`'s W8 check — which greps this doc for compile-gate knob strings like `REIFY_COMPILE_GATE_THRESHOLD` — never ran at merge, and main went RED on the next task. The load-bearing set now also includes **doc-sync docs**: operational docs cross-referenced by tests/infra doc-sync checks, listed in `scripts/doc-sync-paths.txt` (this file is one of the 7 entries). A doc-only edit that would remove or break a synced string/marker/file no longer takes the config-only fast-path.

The canonical source of truth for the load-bearing set is:
- `scripts/verify-pipeline-paths.txt` — static manifest of non-`source`-derivable deps
- verify.sh's live `source "$SCRIPT_DIR/..."` lines — auto-derived, self-healing for future additions
- `scripts/doc-sync-paths.txt` — static manifest of doc-sync docs (task 4955); see that file's header for the per-entry citing-test rationale

The consultable oracle is `scripts/verify-pipeline-guard.sh`:
```
bash scripts/verify-pipeline-guard.sh requires-full-gate <changed-files...>
```
Exit 0 → route to the full `--scope all` gate (or at minimum run `tests/infra/test_verify_throughput.sh` + `tests/infra/test_verify_scope.sh`; for a doc-sync doc, at minimum run its citing infra test — see below). Exit 1 → fast-path safe. Exit 2 → usage error.

**Complementary lever — citing-test subset (task 4955):** routing a doc-sync doc to the guard's exit-0 is the safe default (full gate), but `scripts/verify-pipeline-infra-tests.txt` also maps each doc-sync doc to its citing infra test (e.g. `docs/notes/verify-pipeline-knobs.md` → `tests/infra/test_verify_compile_gate.sh`). `verify.sh`'s `select_infra_tests()` already exact-matches these rows against a task-scope (branch/staged) diff, so a task that edits a doc-sync doc runs just that citing test as a fail-fast pole in its own task-verify — catching desync at authoring time, cheaper than the full gate, and path-agnostic (fires for docs-only diffs even with `RUN_RUST=0`). No `verify.sh` code change was needed; this is the same selective-infra-injection mechanism task 4523 built for script-side artifacts (see `tests/infra/test_verify_scope.sh` VS-\* scenarios), now with doc rows added (DS-\* scenarios).

**Cross-repo seam:** the merge-worker trivial-pass classifier is dark-factory code and **must be wired to consult this script** before taking the config-only fast-path. This interface is unchanged by the doc-sync extension — docs simply join the existing exit-0 set. Whether dark-factory honors exit-0 for a doc-only diff via the full gate or the citing-test subset the map now enables is the consumer's cost-point choice. Reify ships the oracle/manifest/map; dark-factory does the wiring (tracked separately as a non-blocking follow-up to esc-4288-206).
