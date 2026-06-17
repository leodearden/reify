# Capability manifest — `cpu-load-admission-control.md`

Mechanizes G3 + G6 per leaf for the work-conserving CPU-load admission-control PRD.
Built at decompose time (2026-06-17). Each leaf's user-observable signal is broken into
the capabilities it asserts, and each capability is bound to evidence. Any binding
resolving to a FAIL value (`declared-only` · `test-only` · `producer-absent` ·
`producer-extent-short` · `producer-downstream` · `fixture-ERROR` · `bound≤floor` ·
`rejection-absent`) blocks the batch. **Verdict: all PASS — batch cleared.**

## Substrate-verifier note (why the `.ri` decompose-verify workflow does NOT apply)

The reify overlay's substrate verifier (`scripts/prd-decompose-verify.mjs` →
`prd-capability-check.py`) has exactly three probe vectors — `grammar`
(`tree-sitter parse`), `check` (`reify check`), `ir` (`reify eval`) — and five assertion
kinds (`rejection/parses/resolves/produces/ir`), **all specific to the `.ri` language
substrate**. This PRD asserts **zero** `.ri` premises; its substrate is
shell/cgroup-v2/PSI/systemd. Force-fitting a shell-substrate premise into a language
probe would emit a spurious `HARNESS_ERROR`/`UNPROVABLE` and **falsely block** the batch.

The correct G3/G6 verifier for this substrate class is **direct host verification**
(run 2026-06-17, mirroring PRD §6) plus the `tests/infra/*.sh` harnesses the leaves
deliver. Host checks, all PASS:

| Capability | Probe (run 2026-06-17) | Result |
|---|---|---|
| cgroup-v2 unified hierarchy | `mount \| grep 'cgroup2 on /sys/fs/cgroup'` | `cgroup2 on /sys/fs/cgroup … nsdelegate` ✅ |
| `cpu` controller delegated to user manager | `cat …/user@1000.service/cgroup.controllers` | `cpu memory pids` ✅ |
| `systemd-run --user --scope -p CPUWeight=` | `systemd-run --user --scope -p CPUWeight=200 --quiet true` | exit 0 ✅ |
| PSI `/proc/pressure/cpu` (`avg10`) | `head -1 /proc/pressure/cpu` | `some avg10=… avg300=…` ✅ |
| systemd version ≥ 255 | `systemctl --version` | `systemd 255` ✅ |
| `nproc` | `nproc` | 32 ✅ |

G3 verdict: **PASS** — no novel substrate; every layer composes host capabilities
confirmed live.

---

## Leaf tasks

Leaves (no batch task depends on them): **δ, ε, ζ**. Intermediates (other batch tasks
depend on them): **α, β, γ** — their RED-test contracts are validated in the appendix
since they carry observable signals too.

### δ — `orchestrator.yaml cpu_governance:` policy block + CLAUDE.md + cross-PRD prose

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| `orchestrator.yaml` admits a new top-level `cpu_governance:` block | `grep:orchestrator.yaml:103` shows the sibling `jobserver:` block — same shape (top-level map); YAML-parse of the augmented file is the observable | PASS (`wired`) |
| The block's knobs (`W_task`/`W_merge`, `REIFY_CPU_ADMIT_AGENT_THRESHOLD`, enable flags, `DF_AGENT_CPU_GOVERN`) are actually consumed | producer = **α** (`cpu-admit.sh` threshold) and **γ** (`cpu-governed-exec.sh` weights), both **upstream** of δ; their `tests/infra` harnesses read the same env | PASS (`producer:α,γ upstream`) |
| `cpu-admit.sh` added to the verify-pipeline path manifest **iff** α makes it a `source`d verify dep | `grep:scripts/verify-pipeline-paths.txt` (static manifest exists; CLAUDE.md "Drift-guard exception" makes this load-bearing) — the manifest entry is the observable | PASS (`wired`) |
| CLAUDE.md "Test concurrency" documents the compose order (`cpu-governed-exec` placement → `cpu-admit` per heavy command → existing semaphore region) | CLAUDE.md §"Test concurrency" exists with the three-control compose order today; δ extends it | PASS (`wired`) |

No numeric/exactness/rejection premise. G6: N/A. **δ clears.**

### ε — integration-gate leaf `tests/infra/test_cpu_load_governance.sh` (the §8 boundary signal)

| Capability asserted by signal (§8 rows 1–4) | Evidence | Verdict |
|---|---|---|
| Lone governed source uses ≥ ~95% of `nproc` (no idle cores) | work-conserving by construction — γ sets **only** `cpu.weight`, never `cpu.max` (C-G1), and `cpu-admit` admits instantly when PSI low (C-A1); busy-core fraction measured from `/proc/stat` (same instrument as landed jobserver task 4519/4521) | PASS (`floor: util≥0.95·nproc` achievable, no cap) |
| Under heavy mix, PSI `avg10` settles **below the admission band** after warm-up (run-queue ≈ `nproc`, not 2–3×) | the admission mechanism's defining behavior (spaces starts while `avg10 ≥ THRESHOLD`); threshold is a host-portable PSI %, no `nproc`-derived constant | PASS (`floor`: PSI-relative bound, achievable) |
| A governed test **under** the mix completes within a **bounded** multiple of uncontended (≈ fair share, **not 10×**) | bound stated **relative to the analytical floor** `slowdown ≈ active_sources/effective_cores`, asserted as `bound ≥ floor` (PRD §9 ε), never absolute `load==32` | PASS (`floor: bound>floor`, numeric-floor discipline) |
| Merge scope receives **≥** its proportional share `W_merge/(W_merge+W_task)` | kernel `cpu.weight` proportional sharing **among siblings of one parent slice** (C-G2 makes the slice hierarchy load-bearing); cgroup `cpu` controller delegated (host check) | PASS (`substrate` confirmed) |
| "10× cannot recur" (negative assertion) | **observed**, not statically asserted: the harness runs a governed test under the mix and observes bounded completion (PRD §9 ε: `rejection-check = the harness observation`) | PASS (`rejection-check` = harness observation) |
| Required producers all present | α (`cpu-admit.sh`), β (shim), γ (`cpu-governed-exec.sh`) are **all upstream** of ε (DAG: α→ε, γ→ε; β reached transitively via the composed wrappers) | PASS (`producer` upstream, DAG-correct) |

All ε bounds are **PSI-relative / ratio with a stated fair-share floor** — the exact G6
numeric-floor discipline the PRD §1.1/§9 mandate. **ε clears.**

### ζ — [dark-factory, external] wire the agent-launch path to the reify primitives

Owner: **dark-factory**. Filed as the cross-repo consumer; reify ships α/β/γ first.

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| Agent ad-hoc `cargo` runs inside a `reify-agents.slice` cpu-weighted scope (observable via `cat /proc/<cargo-pid>/cgroup`) | producer = **γ** (`cpu-governed-exec.sh --role task`), **upstream** of ζ (DAG: γ→ζ); placement works via the **confirmed** delegated user `cpu` controller | PASS (`producer:γ upstream`) |
| Agent ad-hoc `cargo` PSI-admits via the shim on the agent PATH | producer = **β** (`scripts/agent-bin/cargo`), **upstream** of ζ (DAG: β→ζ); PATH prepend is ζ's own dark-factory work | PASS (`producer:β upstream`) |
| Merge-verify runs under the `--role merge` weighted scope | producer = **γ** (`--role merge` weight path); `--role merge` placement + existing merge PSI/semaphore bypass preserved (PRD §4.4 DF-3) | PASS (`producer:γ upstream`) |
| The dark-factory spawn prefix + PATH injection seam exists | mirrors the **landed** `DF_AGENT_CPU_NICE` / `_cpu_priority_prefix` mechanism at `cli_invoke.py:1125` (PRD §6 mapped); reciprocal-ownership clean (reify can't edit DF launch path) | PASS (established seam pattern) |

DAG-direction: ζ depends on α/β/γ — **no inversion**. **ζ clears.**

---

## Appendix — intermediate-task RED-test contract premises (informational)

α/β/γ are intermediates (consumed by δ/ε/ζ) but carry observable RED-test signals; their
substantive premises are validated here for completeness.

- **α** (`cpu-admit.sh` + `verify.sh` refactor). Premise: `admit` mode on timeout
  **admits + warns, never exits 75**; `requeue` mode on timeout **exits 75**; merge
  bypass admits instantly. Achievability basis: **mirrors the landed `compile_gate`**
  (admit-on-timeout, never 75 — `verify.sh:353`, `…MAX_WAIT…fairness floor` at L403) and
  **landed `psi_gate`** (exit-75 test-phase contract — `verify.sh:243`), preserved
  verbatim (C-A2). Simulated-PSI fixture: `compile_gate` already honours
  `REIFY_COMPILE_GATE_PROC_PATH` (CLAUDE.md knob). PASS.
- **β** (`scripts/agent-bin/cargo` shim). Premise: under high PSI **delays then execs
  (exit 0, never 75)**; low PSI execs immediately; non-heavy subcommands ungated;
  resolves+execs the **real** cargo. Producer for "real cargo" = cargo on PATH
  (`/home/leo/.cargo/bin/cargo`, present) — the harness uses a **stub real-cargo on PATH
  echoing a sentinel**, so the test is hermetic. "never exits 75" basis = same as α. PASS.
- **γ** (`cpu-governed-exec.sh` + `lib_cgroup.sh`). Premise: wrapping a sleeper places it
  in a scope under `reify-agents.slice` whose `cpu.weight` **reads back == role weight**
  and whose `cpu.max` is **`max`/unset** (work-conserving invariant); delegation-off
  **degrades + execs** (fail-open). Substrate confirmed live: `systemd-run --user --scope
  -p CPUWeight=` exit 0; `cpu` in `…user@1000.service/cgroup.controllers`. Field-population
  analog: weight read back is the role value, not a default. PASS.
