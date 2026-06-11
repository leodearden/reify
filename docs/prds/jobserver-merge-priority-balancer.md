# PRD: Priority-aware dual-pool jobserver with a token balancer

**Status:** authored 2026-06-10 · build-infrastructure · B+H (contracts + two-way boundary tests)
**Slug:** `jobserver-merge-priority-balancer`
**Sibling:** `docs/prds/warmer-builds-merge-verify.md` (orthogonal merge-verify speedup — warm worktree + linker; this PRD is the *allocation* layer, that one is the *per-verify cost* layer).

## 1. Consumer & user-observable surface (G1/G2)

**Consumer:** the orchestrator's verify pipeline — `scripts/verify.sh apply_env()` (which selects the cargo jobserver) and the dark-factory merge + task verify dispatch that invokes it. This is **not** an in-engine seam (none of the 7 `engine-integration-norm.md` §3 seams apply); it is host-level build infrastructure.

**User-observable surface (the proof this works):**
1. **Merge-verify wall-clock improves under concurrent task load** — a merge verify running alongside N task verifies finishes measurably faster than on the single-pool baseline, because it is no longer rationed to ~32/N compile tokens.
2. **Full CPU utilization in steady state** — measured busy-core fraction stays ≈ `nproc` (no idle cores) in *all three* load regimes: just-task, just-merge, mixed. This is the headline invariant.
3. **The concurrency cap can rise again** — `max_concurrent_tasks` can be raised back toward 48 (from the defensive 24 in `orchestrator.yaml:11`) **without** re-triggering the merge-starvation freeze that the 48→24 cut was a band-aid for.

All three are measurable end-to-end (see §10 tuning protocol + §9 leaf η).

## 2. The problem (why priority-blindness bites)

Today a single 32-token FIFO jobserver (`/tmp/reify-jobserver`, `reify-jobserver.service`) is shared by *all* cargo across *all* worktrees. Token handoff is a FIFO byte read — **the kernel wakes some blocked reader, with no priority ordering.** `nice -n 5` (merge) vs `nice -n 15` (task) only orders *CPU scheduling once a token is held*; it does not order *token acquisition*. So the merge verify competes equally with ~24 task verifies for 32 tokens and starves. That is the documented 48→24 incident (`orchestrator.yaml:4-11`): tasks drained the pool, the serial merge verify timed out (exit 124), main froze 2 h.

The asymmetry that sets the policy (§5.A): **merge-starves-task is self-healing** — a slow task just waits, the merge queue drains, then tasks proceed. **Task-starves-merge is catastrophic** — the queue backs up, merge verifies time out, and the pipeline livelocks. So the correct bias is *absolute* merge priority.

## 3. Pre-conditions / substrate (G3 — all verified 2026-06-10)

| Substrate | Status |
|---|---|
| `nproc == 32`; current seed is 32 (`reify-jobserver.service`, `jobserver-canary.sh:20`) | ✓ total tokens = nproc |
| cargo reads `CARGO_MAKEFLAGS=--jobserver-auth=fifo:<path>` for an arbitrary FIFO path | ✓ (current mechanism, `verify.sh:404-412`) |
| python3 `fcntl`/`termios.FIONREAD` + `os.open(O_RDWR\|O_NONBLOCK)` available | ✓ (canary already uses FIONREAD) |
| `DF_VERIFY_ROLE` ∈ {task, merge} reliably stamped by the orchestrator on the merge path | ✓ (`verify.sh` already branches on it 4 ways; `orchestrator.yaml:30,34-35`) |
| systemd `--user` service + FIFO-custodian pattern (`exec N<>fifo; sleep infinity`) | ✓ (existing) |
| The **implicit token**: every cargo always runs ≥1 rustc without drawing the pool | ✓ (GNU-make jobserver semantics) — this is task's hard floor under absolute priority |

No grammar/parser substrate is involved (not a `.ri` capability). No FEA/numeric kernel surface.

## 4. Contracts (the H component — pin the dangerous invariants)

- **C1 — Token conservation.** At all times `free(merge) + free(task) + held_by_running_rustc == nproc`. A token is only ever *in transit* inside the balancer between one non-blocking read and its paired write; the balancer must never drop a token it has read. Balancer death mid-transfer is a leak (→ C5 + γ canary).
- **C2 — Canary invariant (two-pool).** When the build is **idle**, `FIONREAD(merge) + FIONREAD(task) == nproc`. The canary checks the **sum**, never the per-pool split (a legitimate idle state may be `32/0`). `sum < nproc` when idle ⟹ leaked tokens ⟹ reseed.
- **C3 — Role→FIFO selection (single source).** Exactly one place sets `CARGO_MAKEFLAGS`: `verify.sh apply_env()`. `DF_VERIFY_ROLE=merge` → merge FIFO; `task` → task FIFO. Per-role `-p` guard: if the role's FIFO is absent, leave `CARGO_MAKEFLAGS` unset → cargo's private pool (graceful degradation, exactly as today). `orchestrator.yaml verify_env` **must not** also inject `CARGO_MAKEFLAGS` (resolves the never-unset asymmetry — see §7).
- **C4 — Balancer policy (absolute merge priority).** A single-threaded control loop that continuously drives toward *"merge holds every token it can consume; task holds the residual"*:
  - **Sense** each tick via `FIONREAD` on both FIFOs (cheap, non-destructive).
  - **Donate idle → demanded.** If a pool has free tokens and the other is at 0-free with live demand, move the free tokens to the demanded pool. When a donor is idle its tokens are free, so this is a contention-free **spin-grab**: non-blocking reads until `EAGAIN`, each paired with a write to the recipient — "as fast as it can spin", bounded by ≤nproc tokens (microseconds). This delivers full utilization for just-task and just-merge.
  - **Contention (both saturated, 0-free each) → ratchet task→merge.** Non-blocking spin-grab on the task FIFO catches task-token *releases* and moves them to merge; merge, being saturated, never gives them back while `free(merge)==0`. Monotonic toward `merge=nproc, task=0`. Task survives on its **implicit tokens** (C3/§3). Single-threaded + non-blocking poll makes this a **bounded ratchet, not instant preemption** — acceptable because merge verifies run tens of minutes and the goal is utilization + *eventual* priority, not millisecond preemption. (A faster multi-threaded stealer is the explicit escape hatch in §6.)
  - **Give-back only merge's spare.** When `free(merge) > 0` (merge demand satisfied), donate `free(merge) − ε` to task, keeping a small ε buffer in merge to damp thrash (ε is a tuning output, §10).
  - **Idle → baseline reset.** When neither pool has demand (no live cargo/rustc, sustained window), reset to the seeded baseline split. Baseline seeds **merge-favored** (consistent with absolute priority → fastest merge cold-start); the choice is near-irrelevant to utilization because a fast poll interval redistributes an idle donor in one tick (§10).
- **C5 — Custodian / liveness.** The balancer process **is** the FIFO custodian: it holds both FIFOs `O_RDWR` open (their buffered tokens evaporate if no FD holds them). It replaces the old `sleep infinity` seeder as `reify-jobserver.service`'s `ExecStart`. `Restart=on-failure` + `PartOf=orchestrator-reify.service` (re-seed on orchestrator restart) + the γ canary (idle-leak reseed) are its three recovery paths.

## 5. Resolved design decisions

- **A — Absolute merge priority (not a task floor).** Under full contention task ratchets to 0 pool tokens, surviving on implicit tokens; task timeouts rise to cover worst-case cold compile at implicit-only. Rationale = the self-healing/catastrophic asymmetry in §2, plus: task verifies are PSI-gated and tightly `--scope branch` scoped (~1–3 min vs ~80 min for a cold merge), so they tolerate yielding. *(This reverses an earlier "derived floor" lean; the livelock asymmetry dominates the task-timeout-cascade risk I had over-weighted.)* The one numeric premise this creates — "a worst-case cold task verify at implicit-only still completes within an acceptable raised timeout" — is **measured, not assumed** (§10, leaf ε); if some crate can't, that surfaces as data, not a silent stall.
- **B — Single-threaded balancer, spin-grab within a transfer.** One control thread; "as fast as it can spin" applies *inside* a transfer burst (non-blocking reads to `EAGAIN`), paced by a short poll interval across ticks. Sufficient for the utilization goal (idle-donor draining is contention-free and instant); gives weak-but-monotonic contention preemption (fine for long merges).
- **C — Empirical split + timeouts (no frozen guesses).** The baseline split, poll interval, ε buffer, and **all** verify timeout budgets are *outputs* of a measurement harness (§10), not numbers chosen by taste. This is the G6 discipline applied to infra: the current band-aid timeout bumps (`verify.sh:686-690`, esc-4178/4447 lineage) are **retired** by measured values.
- **D — `CARGO_MAKEFLAGS` ownership moves wholly into `verify.sh`.** Removed from `orchestrator.yaml verify_env`. Both files live in the reify repo, so this is a reify-only change (no dark-factory coordination).
- **E — Total tokens = `nproc`, always.** No oversubscription; the split only *partitions* nproc between the two pools.

## 6. Out of scope

- **OCCT cross-worktree semaphore priority** — delegated to a **distinct PRD (new session)**. Rationale: the OCCT gate is one flock-slot *per verify invocation* (not per test; `--test-threads=1` serializes intra-process), so peak demand is ~`max_concurrent_tasks + 1`; with N=32 it does **not** bind until the cap is raised past ~31. Its slots are also not fungible the way FIFO tokens are (a flock lock can't be shuttled), so it needs its own mechanism (static reserved slot vs FIFO-token conversion). Named owner = that future PRD (§7).
- **Multi-threaded / faster-preemption balancer** — explicit escape hatch if measured contention-ratchet latency proves too slow; deferred, with C4 written so it can be added without reshaping the pools.
- **Warm merge worktree / linker speedups** — sibling `warmer-builds-merge-verify.md`; reduces per-verify *cost*, orthogonal to this *allocation* layer. They compose (a warm merge that also gets full token allocation is the best case).
- **Raising `max_concurrent_tasks`** itself — this PRD *unblocks* it (surface #3) but the cap change is a separate one-line `orchestrator.yaml` decision Leo makes after the acceptance gate (η) is green.

## 7. Cross-PRD relationship + seam ownership (G4)

| Seam | Owner | Resolution |
|---|---|---|
| `DF_VERIFY_ROLE=merge` stamping | dark-factory (already ships it) | **Assumption holds today**; no DF code change. The only DF dependency. |
| `CARGO_MAKEFLAGS` injection (verify_env vs apply_env) | **this PRD (reify)** | Conflict resolved by removing it from `orchestrator.yaml verify_env`; `verify.sh apply_env` becomes the single source (C3/§5.D). Both files are reify-owned. |
| Balancer service re-seed on orchestrator restart | reify (`setup-dev.sh`) | Inherits the existing `PartOf=orchestrator-reify.service` on `reify-jobserver.service`. |
| OCCT semaphore merge-priority | **future OCCT PRD (new session)** | Out of scope here (§6); named owner, no reciprocal-ownership ambiguity. |
| Warm merge worktree | `warmer-builds-merge-verify.md` | Orthogonal sibling; composes. |

No new contested-ownership pair is introduced (the three in the overlay's G4 list are all kernel/grammar seams, untouched here).

## 8. Two-way boundary tests (H)

- **T-a — balancer ↔ canary (both directions).**
  - *balancer→canary:* the balancer may legitimately leave the split skewed (e.g. `32/0` after a just-merge burst); the canary must **not** false-positive on a skewed-but-sum-correct idle state (it checks the sum, C2).
  - *canary→balancer:* an injected real leak (one pool short, sum < nproc, idle) must trigger a reseed (service restart) that restores `sum == nproc`; a leak injected *mid-build* must be **skipped** (idle-only guard).
- **T-b — verify.sh ↔ balancer (both directions).**
  - *verify.sh→balancer:* `verify.sh --print-plan` env block shows the role-appropriate FIFO path (merge role → merge FIFO, task role → task FIFO) — an oracle test in the style of `tests/infra/test_occt_flock_gate.sh`.
  - *balancer→verify.sh:* a merge-role cargo actually draws from the merge pool (observable as a FIONREAD drop on the merge FIFO, not the task FIFO, during a real merge compile).

## 9. Decomposition plan (one leaf per bullet; observable signal in **bold**)

Incremental-land-safe: α and δ each degrade to cargo's private pool until the other lands (the `-p` guard), so neither half breaks verification on its own — but they should land close together (or δ first) to minimize the private-pool window. β/γ no-op safely until α's daemon exists.

- **α — Dual-FIFO seeding + balancer daemon (custodian).** New `scripts/jobserver-balancer.py`; rewrite `reify-jobserver.service` `ExecStart` to run it (seeds merge+task FIFOs to baseline, holds both `O_RDWR`, runs the loop). *Signal:* **`systemctl --user start reify-jobserver` → both FIFOs exist and `FIONREAD` sums to nproc; a scripted idle-donor load shows tokens migrate to the demanded pool (observable FIONREAD shift).**
- **β — Balancer policy (C4).** Sense + donate-idle + contention-ratchet (absolute merge priority) + ε-buffer give-back + idle baseline-reset; spin-grab non-blocking transfer. *Signal:* **a balancer integration test over synthetic FIFO states asserts: just-merge→merge reaches nproc; just-task→task reaches nproc; contested→task ratchets monotonically to 0 (merge=nproc); idle→reset to baseline.** (dep α)
- **γ — Two-pool canary.** Rewrite `scripts/jobserver-canary.sh` to the C2 sum invariant; reseed both pools (restart the balancer service) on idle-leak; keep the idle-only guard. *Signal:* **injected one-pool-short fixture when idle → canary restarts service (sum restored); injected mid-build → canary skips; a `32/0` idle split does NOT trigger a reseed.** (dep α)
- **δ — verify.sh role→FIFO selection + ownership move.** `apply_env` selects merge/task FIFO by `DF_VERIFY_ROLE` with per-role `-p` fallback; delete `CARGO_MAKEFLAGS` from `orchestrator.yaml verify_env`. *Signal:* **`verify.sh --print-plan` env line shows role-appropriate FIFO path (merge→merge FIFO, task→task FIFO); `orchestrator.yaml` no longer injects `CARGO_MAKEFLAGS`** (oracle test).
- **ε — Empirical split + timeout tuning harness.** Drive just-task / just-merge / mixed **real** cargo loads; measure CPU utilization (`/proc/stat`), per-pool occupancy (FIONREAD time series), and merge + worst-case-cold-task verify wall-clocks. Output the tuned baseline split, poll interval, ε, and re-derived timeout budgets; commit a tuning report. *Signal:* **report shows utilization ≥ threshold in all 3 regimes AND worst-case cold task verify fits the chosen raised timeout; derived constants committed.** (dep α, β, δ)
- **ζ — Timeout re-derivation wired + drift tests updated.** *[AMENDED 2026-06-11, esc-4520-22 disposition D (Leo-ratified): ε's committed timeout constants (2 s) are artifacts of the harness's synthetic CPU-burn stub — flagged `SYNTHETIC_TIMEOUT_NOT_AUTHORITATIVE`, lower-bound guard added by #4526 — so the ζ↔η dep direction is flipped: the budget floor is measured by η under real load.]* Re-derive budgets from **η's** measured merge + worst-case task verify wall-clocks (margin per §10.3) and apply to `verify.sh add_test_passes` outer timeouts; update the asserting drift oracle in lockstep — that oracle is `test_occt_flock_gate.sh` Test 17 **only** (debug 60m; the `test_release_*` tests assert no timeout constants — original premise corrected), and add the currently-missing release-75m assertion alongside it; retire the #4447 band-aid narrative in the verify.sh provenance comment. *Signal:* **infra drift tests green against the new constants with BOTH debug and release outer timeouts asserted; a cold task verify at implicit-only completes within budget (measured by η).** (dep η; fallback: a dedicated real-load ε′ re-measure campaign remains available if η's capture lacks a cold-cache datapoint)
- **η — End-to-end mixed-load acceptance gate (CRITICAL).** Real merge verify concurrent with N task verifies, run under the **standing** verify.sh budgets (debug 60m / release 75m — battle-tested; not ε's synthetic constants). *Signal:* **(a) box ≈ fully utilized; (b) merge wall-clock improved vs single-pool baseline; (c) no task verify spuriously exits 124; (d) under contention merge reaches full token allocation.** The user-observable proof + the C-as-integration-gate. The committed acceptance report additionally records the merge wall-clock and the slowest task verify wall-clock with sccache cache state per run — the authoritative floor ζ consumes. (dep α, β, γ, δ — ζ moved downstream of η, 2026-06-11)

## 10. Empirical tuning protocol (the §5.C measurement spine)

The harness (leaf ε) is itself the artifact that makes the numbers honest:

1. **Regimes.** Drive three real loads against a controlled cache state: *just-task* (M task verifies, no merge), *just-merge* (one merge verify, no tasks), *mixed* (one merge + M tasks). Run each at both warm and cold sccache.
2. **Instruments.** CPU utilization from `/proc/stat` (busy-core fraction over the run); per-pool occupancy from a FIONREAD sampler on both FIFOs; wall-clock of the merge verify and of the *slowest* task verify; count of any exit-124.
3. **Derived outputs (the constants this PRD ships):**
   - **Baseline idle split** (merge-favored; exact value tuned so merge cold-start is fast and a single fast poll redistributes for task-only loads).
   - **Poll interval** + **ε give-back buffer** (smallest that holds utilization ≥ threshold without thrash).
   - **Task timeout budgets** ≥ measured worst-case cold task verify at implicit-only, with margin; **merge timeout budgets** re-derived from measured merge wall-clock at full allocation.
4. **Acceptance floors (G6 — assert measured > floor, never guess):** utilization ≥ threshold (target ≈ nproc-busy) in *all three* regimes; `total == nproc` throughout; worst-case task verify wall-clock < chosen task timeout. If implicit-only task compile can't fit any sane timeout, that is surfaced as a finding (candidate to revisit the absolute-priority decision, or to lean on the warmer-builds sibling) — not silently absorbed.

## 11. Open (tactical) questions

- **Balancer language:** python3 (matches the canary idiom, scriptable/tunable, I/O-bound so GIL/startup irrelevant) vs a small Rust binary. Lean python3; decide at α.
- **Poll cadence shape:** fixed interval vs adaptive (tighten while a transfer is active, relax when idle). Start fixed; let ε's data justify adaptivity.
- **Rollout sequencing:** land δ-then-α, α-then-δ, or both together — all have a brief private-pool window (which is just today's degraded mode). Pick at decompose; not a design blocker.
- **ε buffer vs absolute priority:** the ε give-back buffer means merge briefly holds 1–2 tokens it isn't using; confirm during tuning this doesn't measurably dent task throughput in the just-task regime (it shouldn't — task reclaims them in one poll).
