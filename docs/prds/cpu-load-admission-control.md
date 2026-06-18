# PRD — Work-conserving CPU-load admission control over ALL load sources

**Status:** deferred (author 2026-06-17). Version-agnostic build-host infrastructure
(root `docs/prds/`, alongside `test-run-concurrency-semaphore.md` and
`jobserver-merge-priority-balancer.md`). **Approach: B + H** (cross-repo seam,
load-bearing infra → contract + two-way boundary tests).

**One-line goal:** put *every* significant CPU source on the build host —
agent ad-hoc `cargo build`/test-execution, the verify pipeline's cargo, rustc/sccache,
the merge verify — under a single **work-conserving** governance regime, so the host
never oversubscribes into the 2–3×-`nproc` thrash that ballooned a ~1-minute
`cargo test -p reify-solver-elastic` to ~10 minutes on 2026-06-16, **without** ever
idling a core when only one source is active.

---

## 1. Consumer & user-observable surface (G1 / G2)

**Named consumers / enforcement points** (every mechanism this PRD introduces wires
into one of these — no orphan producers):

| Mechanism | Consumer / enforcement point |
|---|---|
| `scripts/cpu-admit.sh` (shared PSI-admission core) | `scripts/verify.sh` (`psi_gate`/`compile_gate` refactor to call it); the agent cargo shim (β) |
| `scripts/agent-bin/cargo` (PSI shim) | the **agent's PATH** — every ad-hoc `cargo build`/`cargo test`/`cargo nextest` the agent runs via its Bash tool |
| `scripts/cpu-governed-exec.sh` + `scripts/lib_cgroup.sh` (cgroup `cpu.weight` placement) | the **dark-factory agent-launch path** (agent spawn prefix) and the **merge-verify** subprocess |
| `orchestrator.yaml` `cpu_governance:` block | `scripts/verify.sh` (knob defaults) + dark-factory `_build_agent_env` (env injection) |
| `tests/infra/test_cpu_load_governance.sh` | CI / `verify.sh --include-infra` — the integration-gate proof |

**User-observable surface (operator-facing).** Under a heavy realistic mix
(≈24 task lanes, several agents each running a solver/CPU-bound `cargo test`, plus a
merge verify in flight):

1. **No oversubscription thrash.** CPU PSI `avg10` settles **below the admission
   threshold band** after a short warm-up (run-queue near `nproc`, *not* the
   1.3–2.8×-`nproc` of the 2026-06-16 incident). *(Stated in PSI terms, not as a bare
   `load == 32` — see §G6; load average counts runnable+blocked threads and is the
   wrong primitive; PSI measures runnable-but-stalled time, which is exactly the
   pathology.)*
2. **Full utilisation for a lone source.** A **single** agent's `cargo test` on an
   otherwise-quiet box still runs at full speed across all 32 cores — **zero idle
   cores**, no static cap throttling it.
3. **No source starved.** Every admitted source makes proportional progress; the
   merge verify keeps its favored share (consistent with the `DF_VERIFY_ROLE=merge`
   exemption everywhere else).
4. **The 4415-class incident cannot recur.** A governed `cargo test` under the heavy
   mix completes within a **bounded** multiple of its uncontended time (≈ its fair
   share, not 10×).

These four are the §8 boundary-test sketch made observable; the integration-gate leaf
(ε) is the harness that asserts them.

---

## 2. The premise — what is and isn't governed today (G6, validated by code-trace)

On 2026-06-16 the 32-core host ran at **load average 42–89**. That oversubscription
inflated a normally-fast `cargo test -p reify-solver-elastic` to **~10 minutes**
wall-clock, exhausting agent timeout budgets (the false "startup wedge" telemetry) and
overflowing fused-memory reconciliation (1558/1500, esc-fused-memory-239) — one root
cause (host CPU oversubscription), many symptoms.

This persisted **despite** substantial governance already landed:

| Control | What it governs | The gap |
|---|---|---|
| `compile_gate()` (verify.sh, #4618) | PSI admission for verify clippy/check/compile | verify pipeline only (`DF_VERIFY_ROLE`-keyed) |
| `psi_gate()` (verify.sh) | PSI admission for the verify **test-execution** phase | verify pipeline only |
| `lib_test_semaphore.sh` | hard test×test cap (N=1) | **inside `verify.sh` only**; and it is a *fixed count* — a sub-100% cap |
| jobserver (#1745, `CARGO_MAKEFLAGS`) | compile **job parallelism** (make tokens) | does **not** touch test-binary **runtime**; falls back to unbounded private `-j` when the FIFO is absent |
| `nice` (#1767, `DF_AGENT_CPU_NICE`) | scheduling **priority** of agent subprocesses | priority ≠ cap; reorders under contention, does not bound aggregate runnable load |

**The leak (confirmed):** an agent's *ad-hoc* `cargo test` launched via its own Bash
tool — the dominant, spikiest real load — is governed by **none** of the above for the
part that matters (the test-binary runtime). With `max_concurrent_tasks: 24`, up to 24
agents can each spin a CPU-bound test binary at once, entirely ungoverned → load 42–89.
**`max_concurrent_tasks` is a LANE cap, not a LOAD cap** (the standing lesson:
*govern load, not lanes* — `reference_deb_stdlib_fanout_merge_timeout_jun15`).

**The mechanism of the balloon (the design-shaping fact).** The slowdown is caused by
**aggregate runnable-thread count ≫ `nproc`** — 24 agents × a multi-threaded `cargo
test` (nextest defaults to `-j nproc`) = hundreds of runnable threads on 32 cores,
context-switch / cache thrash. This is why the fix is a **hybrid** (see §3): a CPU-time
*share* mechanism (`cpu.weight`) reallocates time fairly but does **not** reduce the
thread count, so it alone cannot lower the run-queue; a *pressure-reactive admission*
mechanism bounds how many heavy commands run concurrently, which is what actually keeps
the run-queue near `nproc`. The two are complementary and both required.

---

## 3. The hard constraint & the chosen approach (G5)

> **Requester constraint (do not violate):** allow *full utilisation for all load
> mixes* — **no fixed per-load-source hard cap below 100% utilisation.**

A lone source must reach all 32 cores; under contention N sources share
**proportionally / by pressure feedback**, never throttled by a static sub-100%
per-source ceiling that idles cores when the rest of the box is quiet. **`cpu.weight`
proportional sharing and PSI-reactive backoff are work-conserving by construction**;
fixed `-jK` caps, fixed CPU quotas (`cpu.max`), and fixed-count semaphores are **not**
(they idle capacity). The design uses **only** the former.

**Chosen approach — Hybrid (two work-conserving layers):**

- **Layer 1 — cgroup-v2 `cpu.weight` proportional sharing.** Every governed source
  (each agent's process tree; the merge verify) is placed in a cgroup scope under a
  common slice, weighted. Under contention the kernel shares CPU time by weight; when a
  source is alone its scope absorbs the whole box (no `cpu.max` is ever set). Delivers
  surface (2) "lone source full speed" and (3) "no source starved / merge favored".
- **Layer 2 — generalized PSI-reactive admission (`cpu-admit.sh`).** Extracted from
  `verify.sh`'s `psi_gate`/`compile_gate` into one shared primitive that the **agent
  cargo shim** and the verify pipeline both call before a heavy command. Admits
  instantly when PSI is low (work-conserving — a lone source never waits); spaces out
  starts when PSI is high so the run-queue stays near `nproc`. Delivers surface (1)
  "no thrash" and (4) "no 10× balloon".

**Why not the simpler single-layer options** (recorded so this isn't re-litigated):
*cgroup-only* shares time fairly but leaves the run-queue at hundreds of threads → load
still 89, thrash unchanged → fails surface (1)/(4). *Admission-only* prevents the
balloon and is work-conserving but is coarse (binary admit/wait, no proportional
fairness, no merge-favoring at the CPU-time layer). Only the hybrid satisfies all four
observable surfaces; the two halves of the G2 signal map one-to-one onto the two layers.

**What stays orthogonal (not subsumed):** the dual-pool **jobserver**
(`jobserver-merge-priority-balancer.md`) governs compile **token count**; the cgroup
layer governs CPU **time share**. Different axes — they compose, both stay live. `nice`
(#1767) also stays (cheap priority hint). This PRD adds the missing **CPU-time / load**
axis and the missing **agent-ad-hoc** reach.

---

## 4. Contracts (the H component — pin the dangerous invariants)

### 4.1 `scripts/cpu-admit.sh <mode>` — the shared PSI-admission core

```
cpu-admit.sh admit      # admit-on-timeout mode (agent shim, compile phase)
cpu-admit.sh requeue    # exit-75-on-timeout mode (verify test phase)
```

- **C-A1 (work-conserving).** Admits immediately while `avg10 < THRESHOLD`
  (`/proc/pressure/cpu`, host-portable %, no `nproc`-derived constant). A lone source
  on a quiet box is never delayed.
- **C-A2 (mode contract).** `admit` mode on `MAX_WAIT` timeout **admits + warns,
  NEVER exits 75** (mirrors today's `compile_gate`; an agent's mid-turn `cargo`
  command has no requeue semantics — a hard-fail would spuriously break the command).
  `requeue` mode on timeout **exits 75** (EX_TEMPFAIL → orchestrator requeues — the
  existing `psi_gate` test-phase contract, preserved verbatim).
- **C-A3 (merge bypass).** `DF_VERIFY_ROLE=merge` ⇒ immediate admit (never waits).
- **C-A4 (fail-open).** Missing/unreadable `/proc/pressure/cpu` ⇒ admit + warn.
- **C-A5 (no hard count).** `cpu-admit.sh` is pressure-reactive only; it introduces
  **no fixed-count semaphore** for the agent path (that would be a sub-100% cap). The
  existing held-slot test-semaphore stays scoped to the verify test×test region only.

### 4.2 `scripts/cpu-governed-exec.sh --role <task|merge> -- CMD…` — cgroup placement

- **C-G1 (proportional, never a quota).** Places `CMD`'s process tree (which, via
  `start_new_session=True` at the dark-factory spawn, captures all cargo/rustc/test
  children) into a cgroup **scope** under a fixed slice, with `cpu.weight` set by role.
  **Never sets `cpu.max`** — the only knob is the relative weight. A lone scope under
  the slice gets 100% of the box.
- **C-G2 (sibling comparability — correctness invariant).** All governed scopes share
  one parent slice so `cpu.weight` values are *comparable*: `reify-governed.slice` →
  `reify-agents.slice` (agents land here, weight `W_task`) and `reify-merge.slice`
  (merge verify, weight `W_merge > W_task`). `cpu.weight` is proportional only **among
  siblings of the same parent** — scopes under different parents do **not** share by
  weight, so the slice hierarchy is load-bearing, not cosmetic.
- **C-G3 (merge-favored).** Default weights mirror the landed jobserver merge:task
  ≈ 3:1 baseline (`task_baseline = max(1, nproc//4)`): `W_task=100` (cgroup default),
  `W_merge=300`. Tunable in `orchestrator.yaml`.
- **C-G4 (fail-open / no root).** Placement uses the **user** systemd manager
  (`systemd-run --user --scope --slice=…`), which works because the `cpu` controller is
  delegated to `user@<uid>.service` (verified §6) — **no root required**. If delegation
  or `systemd-run` is absent (e.g. minimal CI), degrade to `cpu-admit` + `nice` and exec
  anyway — **never block the command**.

### 4.3 `scripts/agent-bin/cargo` — the agent ad-hoc PSI shim

- **C-S1 (transparent).** For heavy subcommands `{build, test, nextest, check, clippy,
  bench, doc, build-std}` → `cpu-admit.sh admit` then `exec` the **real** cargo
  (resolved by stripping the shim dir from PATH). For all other subcommands → `exec`
  immediately, **no gate** (never stall `cargo --version`/`metadata`).
- **C-S2 (semantics-preserving).** Adds only admission latency; never alters cargo's
  args, exit code, or stdout/stderr (apart from one stderr admission notice).

### 4.4 The cross-repo env contract (the §7 seam — owned by dark-factory, ζ)

Mirrors the existing `DF_AGENT_CPU_NICE` / `_cpu_priority_prefix` mechanism exactly:

- **DF-1.** dark-factory prepends `cpu-governed-exec.sh --role task --` to the agent
  spawn, gated by a new `DF_AGENT_CPU_GOVERN` env (reify-owned value in
  `orchestrator.yaml`), at `shared/src/shared/cli_invoke.py:1125` alongside the nice
  prefix.
- **DF-2.** dark-factory prepends reify's `scripts/agent-bin` to the agent's **PATH**
  via `_build_agent_env` / `env_overrides`, so the cargo shim is active for ad-hoc
  commands.
- **DF-3.** the **merge-verify** subprocess is spawned with `--role merge` (favored
  weight). The merge gate's existing PSI/semaphore bypasses are untouched.

---

## 5. Resolved design decisions

1. **Hybrid, not single-layer.** cgroup `cpu.weight` (time share) + generalized PSI
   admission (start spacing). The two G2-signal halves require both (§3).
2. **Reify ships the primitives; dark-factory prepends an env-driven prefix** — the
   established `verify-pipeline-guard` / main-gate seam pattern. Policy and tests live
   reify-side; the dark-factory change is ~one prefix function + a PATH prepend (§7).
3. **One shared admission primitive** (`cpu-admit.sh`); `verify.sh`'s `psi_gate`/
   `compile_gate` refactor to call it. No second copy of the PSI logic to drift
   (the hazard CLAUDE.md's verify-pipeline-guard already warns about).
4. **Agent placement = once per agent at spawn** (whole tree, one cgroup), **not**
   per-command — the children inherit the cgroup. PSI admission, by contrast, is
   **per heavy command** via the cargo shim (gating every `ls` on PSI would be wrong).
5. **Work-conservation is structural:** only `cpu.weight` (never `cpu.max`); only
   PSI-reactive admission (never a fixed count for agents); `admit`-mode never
   exits 75. Any approach that can idle a core under a single-source load is rejected.
6. **Merge stays favored & exempt** across all three layers (weight `W_merge>W_task`;
   `cpu-admit` merge bypass; existing verify semaphore/psi merge exemption preserved).
7. **Jobserver + nice stay live** (orthogonal axes); this PRD adds the CPU-time/load
   axis and the agent-ad-hoc reach, it does not replace them.
8. **PRD scope is disjoint** from the sibling 2026-06-17 heartbeat/telemetry PRD
   (that one: *don't kill productive-but-slow work, classify it right*; this one:
   *stop the oversubscription that makes work slow*). Together they close the loop.

---

## 6. Pre-conditions / substrate (G3 — verified on host 2026-06-17)

| Capability | Status | Evidence |
|---|---|---|
| cgroup-v2 unified hierarchy | ✅ present | `mount` → `cgroup2 on /sys/fs/cgroup` |
| `cpu` controller **delegated to the user manager** (no root needed) | ✅ present | `cat .../user@1000.service/cgroup.controllers` → `cpu memory pids` |
| `systemd-run --user --scope -p CPUWeight=` | ✅ present | `systemd 255`; user-delegated `cpu` controller makes a `--user` `--scope` weightable |
| PSI `/proc/pressure/cpu` (`avg10`) | ✅ present | live read; already consumed by `psi_gate`/`compile_gate` |
| `nproc` = 32 | ✅ | sizing reference (weights are nproc-independent %/ratios) |
| Existing PSI/semaphore primitives to generalize | ✅ present | `verify.sh` `psi_gate`/`compile_gate`; `lib_test_semaphore.sh` |
| dark-factory agent spawn seam (the §7 insertion point) | ✅ mapped | `cli_invoke.py:1125` `spawn_cmd = _cpu_priority_prefix(env) + cmd`; `start_new_session=True`; **no cgroups in dark-factory today** |

**No novel substrate is invented** — every layer composes capabilities verified above.
G3 verdict: PASS (the only "new" element, cgroup placement, rests on a confirmed,
root-free, delegated controller).

---

## 7. Cross-PRD / cross-repo relationship & seam ownership (G4)

| Other PRD / repo | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **dark-factory** (`agents/invoke.py`, `shared/cli_invoke.py` `_build_agent_env`/`_run_subprocess`) | reify **produces** the primitive; dark-factory **consumes** it | `DF_AGENT_CPU_GOVERN` spawn prefix (`cpu-governed-exec.sh`) + `scripts/agent-bin` PATH prepend + merge-verify `--role merge` placement | **dark-factory** (ζ, external-deps task) | queued (depends on reify α/β/γ landing first) |
| `docs/prds/test-run-concurrency-semaphore.md` | this PRD **refactors** its `psi_gate`/semaphore into the shared core | `psi_gate`/`compile_gate` → `cpu-admit.sh`; held-slot semaphore contract preserved | **this PRD** (α) | this-prd |
| `docs/prds/jobserver-merge-priority-balancer.md` | **orthogonal / composes** | jobserver = compile token count; cgroup = CPU-time share (disjoint axes) | both (disjoint) | wired |
| 2026-06-17 heartbeat/telemetry PRD (sibling) | **complementary, disjoint** | heartbeat classifies slow-but-productive work; this prevents oversubscription | sibling | disjoint |

**The one cross-repo seam is owned, not hand-waved:** the CPU-governance *primitive*
is reify-side (`cpu-admit.sh`, `cpu-governed-exec.sh`, the cargo shim); the *enforcement
of agent ad-hoc commands* requires dark-factory to (a) prepend the wrapper at agent
launch and (b) inject the shim dir onto the agent PATH. dark-factory holds the
integration task (ζ); reify ships and tests the oracle. This mirrors the
`verify-pipeline-guard` and main-gate seams precisely. **No reciprocal-ownership
ambiguity** (reify cannot edit the dark-factory launch path; dark-factory cannot edit
the reify scripts) — ownership is clean by construction.

---

## 8. Boundary-test sketch (H — facing both producer and consumer sides)

The integration-gate leaf (ε) realizes this table as `tests/infra/test_cpu_load_governance.sh`.

| # | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|
| 1 | Lone governed source, box idle | one governed CPU-bound source, others quiet | uses ~all 32 cores; utilisation ≥ ~95% of `nproc`; completes ≈ uncontended time (**no idle cores**) |
| 2 | Heavy mix: N≈24 governed agent test sources + 1 merge verify | full mix admitted | CPU PSI `avg10` settles **below the admission band** after warm-up (run-queue near `nproc`, not 2–3×); every source progresses |
| 3 | Single governed test **under** the heavy mix (the 4415 case) | mix as #2 | the test completes within a **bounded** multiple of uncontended (≈ fair share), **NOT 10×** |
| 4 | Merge-favored share | merge scope `W_merge` vs agents `W_task` under contention | merge scope receives **≥ its proportional** (`W_merge/(W_merge+W_task)`) CPU-time share |
| 5 | PSI low, agent runs `cargo test` (shim) | quiet box | shim admits **immediately** (no added latency) |
| 6 | PSI high sustained, agent `cargo test` (shim, `admit` mode) | saturated box | shim **waits ≤ MAX_WAIT then ADMITS** — **never exits 75**, never blocks forever |
| 7 | PSI high sustained, **verify** test phase (`requeue` mode) | saturated box | `cpu-admit requeue` **exits 75** → orchestrator requeues (existing contract preserved) |
| 8 | cgroup delegation / `systemd-run` absent (minimal CI) | no user `cpu` controller | wrapper **degrades to PSI-admit + nice, execs anyway** (fail-open; build not blocked) |
| 9 | `DF_VERIFY_ROLE=merge` | merge role | all admission layers **bypass** (never wait) + merge-weighted cgroup (favored) |

Facing-the-producer rows: 1, 5, 6, 7, 8 (the reify primitives behave correctly in
isolation). Facing-the-consumer rows: 2, 3, 4, 9 (the composed system under the real
agent+verify+merge mix). ε is the leaf whose observable signal **is** this table.

---

## 9. Decomposition plan (one bullet per task → its observable signal)

Greek labels; actual task IDs assigned at decompose. **B+H shape:** Phase 1 foundation
(α, γ) → Phase 2 vertical slice governing the dominant source (β) → Phase 3
integration gate (ε) → cross-repo seam (ζ) → companion corrections (δ).

- **α — Extract `scripts/cpu-admit.sh` shared PSI core; refactor `verify.sh`.**
  *Modules:* `scripts/cpu-admit.sh` (new), `scripts/verify.sh`, `tests/infra/`.
  *Signal (intermediate → unlocks β, ε):* `verify.sh --print-plan` still emits the
  gate lines unchanged; the existing `test_verify_throughput.sh`/`test_verify_scope.sh`
  + psi/compile-gate mechanism tests stay **GREEN**; new `tests/infra/test_cpu_admit.sh`
  drives `cpu-admit.sh` directly and observes: `admit` mode under simulated high PSI
  (`REIFY_..._PROC_PATH` fixture) **waits then exits 0** (never 75); `requeue` mode
  **exits 75**; merge bypass admits instantly. *G6:* thresholds are PSI % (host-portable,
  no `nproc` constant); achievability = mirrors landed `compile_gate`/`psi_gate`.
  *Manifest:* `wired` — `verify.sh` sources it (grep the `source`/call site).

- **β — Agent ad-hoc cargo PSI shim `scripts/agent-bin/cargo`.**
  *Modules:* `scripts/agent-bin/cargo` (new), `tests/infra/`.
  *Signal (leaf; consumer = ζ's PATH injection):* `tests/infra/test_agent_cargo_shim.sh`
  — invoking the shim as `cargo test` under simulated high PSI **delays then execs**
  (exit 0, never 75); under low PSI **execs immediately**; `cargo --version` and other
  non-heavy subcommands **exec with no gate**; the shim resolves and execs the **real**
  cargo (asserted via a stub real-cargo on PATH echoing a sentinel). *G6:* end-to-end
  capability = real cargo on PATH minus shim dir (producible). *Manifest:* `producer`
  = real cargo (present); negative path = non-heavy subcommand observably ungated.

- **γ — cgroup `cpu.weight` placement: `scripts/cpu-governed-exec.sh` + `scripts/lib_cgroup.sh`.**
  *Modules:* both new, `tests/infra/`.
  *Signal (intermediate → unlocks ε, ζ):* `tests/infra/test_cpu_governed_exec.sh` —
  on a host with cpu-delegated cgroup-v2, wrapping a sleeper places it in a scope under
  `reify-agents.slice` whose `cpu.weight` **read back from the cgroup fs equals** the
  role weight (`W_task`/`W_merge`), and **`cpu.max` is `max`/unset** (no quota — the
  work-conserving invariant, asserted); with delegation forced off it **degrades and
  still execs** (fail-open). *G6:* substrate confirmed (§6); field-population analog =
  weight read back is the role value, not a default. *Manifest:* `substrate` =
  `systemd-run --user --scope` (confirmed); `numeric-floor` N/A (weight is a ratio).

- **δ — `orchestrator.yaml` `cpu_governance:` policy block + CLAUDE.md + cross-PRD prose.**
  *Modules:* `orchestrator.yaml`, `CLAUDE.md` ("Test concurrency"), `docs/prds/test-run-concurrency-semaphore.md` + `jobserver-merge-priority-balancer.md` cross-refs.
  *Signal (companion-corrections leaf):* the new `cpu_governance:` block (weights,
  enable flags, knob defaults, `DF_AGENT_CPU_GOVERN`) parses and the reify primitives
  honour its knobs (covered by α/γ tests reading the same env); CLAUDE.md documents the
  full compose order (`cpu-governed-exec` placement → `cpu-admit` per heavy command →
  existing semaphore region); `scripts/verify-pipeline-guard.sh` / the verify-pipeline
  path manifest updated if `cpu-admit.sh` becomes a `source`d verify dep. *Depends:*
  α, β, γ. *Note:* `orchestrator.yaml` is loaded once at startup — landing this is a
  commit-then-restart per CLAUDE.md "Deploying the orchestrator".

- **ε — Integration-gate leaf: `tests/infra/test_cpu_load_governance.sh`** (the §8 boundary signal).
  *Modules:* `tests/infra/` (+ a small fixture load generator).
  *Signal (the leaf — full user-observable surface §1):* the §8 table, scenarios 1–4
  asserted under the composed wrappers — PSI `avg10` band after warm-up; lone-source
  utilisation ≥ ~95% `nproc`; bounded slowdown (≈ fair share, not 10×); merge ≥
  proportional share. *Depends:* α, β, γ. *G6 (the crux):* bounds are **PSI-relative /
  ratio**, never absolute `load==32`; floor = fair share (`slowdown ≈
  active_sources/effective_cores`, asserted as the *floor* the bound must be ≥, not 0).
  The "10× cannot recur" negative assertion is **observed** by the harness running a
  governed test under the mix and seeing bounded completion. *Manifest:* `rejection-check`
  = the harness observation; `numeric-floor` = fair-share band stated.

- **ζ — [dark-factory, external-deps] Wire the agent-launch path to the reify primitives.**
  *Repo:* dark-factory (`shared/cli_invoke.py`, `orchestrator/workflow.py` `_build_agent_env`).
  *Signal (cross-repo seam, §4.4):* an agent's ad-hoc `cargo test` runs (a) inside a
  `reify-agents.slice` cpu-weighted scope — observable via `cat /proc/<cargo-pid>/cgroup`
  showing the scope — and (b) PSI-admits via the shim (shim dir on the agent PATH);
  the merge-verify runs under the `--role merge` weighted scope. *Depends:* α, β, γ
  (reify ships the primitives first). *Owner:* dark-factory.

**DAG:** α → β; α → ε; γ → ε; γ → ζ; β → ζ; {α,β,γ} → δ; {α,β,γ} → ζ.
ε is the integration leaf (G2 escape hatch: α and γ are foundation intermediates roped
into ε). ζ is the cross-repo consumer; reify's α/β/γ/δ/ε are landable and observable
**without** ζ (the harness ε proves the primitives compose; ζ then points the real
agent launch at them).

---

## 10. Out of scope / accepted limitations

- **Direct test-binary invocation bypasses the shim.** The cargo shim catches `cargo …`
  (bare-name PATH lookup — how agents invoke it ~always). An agent that runs
  `./target/debug/foo` directly bypasses PSI admission (still governed by the cgroup
  `cpu.weight` layer, just not admission-spaced). Accepted for v1; revisit if observed.
- **Memory / IO pressure.** This PRD governs **CPU** (PSI cpu + cgroup `cpu.weight`).
  Memory-pressure governance (the 119 GiB kernel-leak class,
  `reference_kernel_leak_escalation_cluster_jun16`) and IO are separate concerns.
- **Per-agent runaway isolation.** Agents share `reify-agents.slice` (collectively
  `W_task`); a single runaway agent is not individually weighted below its peers. Adding
  per-agent sub-scopes is a future refinement, not v1.
- **Heartbeat/telemetry** (sibling PRD) — explicitly disjoint (§5.8).
- **Replacing the jobserver or `nice`** — orthogonal, both stay (§5.7).
- **Non-Linux / non-PSI hosts** — fail-open throughout (degrade to nice/no-gate);
  governance is a Linux-host optimisation, not a correctness gate.

---

## 11. Open questions (tactical — deferred, not design-level)

1. **`cpu-governed-exec.sh` internal transport: `systemd-run --user --scope` vs direct
   `cgroup.procs` write.** Both satisfy C-G1–C-G4; systemd-run gets lifecycle/teardown
   for free. **Suggested:** `systemd-run --user --scope --slice=reify-agents.slice`,
   fall back to direct cgroup write only if `systemd-run` absent. Decide during γ.
2. **Exact default weights `W_task`/`W_merge`.** Default `100`/`300` (mirrors the
   jobserver ≈3:1 merge:task baseline). Retune empirically à la the jobserver tuning
   harness if the merge share proves insufficient. Decide during δ/ε.
3. **`cpu-admit` agent-path threshold.** Reuse `psi_gate`'s 50% or `compile_gate`'s 85%?
   The agent ad-hoc test phase is test-execution → lean toward the 50% test-phase
   threshold; **suggested** a distinct `REIFY_CPU_ADMIT_AGENT_THRESHOLD` (default 50).
   Decide during α/β.
4. **Heavy-subcommand list for the shim** (`build/test/nextest/check/clippy/bench/doc`).
   Confirm completeness against observed agent cargo usage. Decide during β.
5. **Warm-up window & PSI band thresholds for ε's assertions** — the harness needs a
   settle window before sampling `avg10`. **Suggested:** 10–20 s warm-up, assert
   `avg10 < REIFY_CPU_ADMIT_AGENT_THRESHOLD` thereafter. Decide during ε.
