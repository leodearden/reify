# Design — Offline deep-test lane (tier heavy tests off the merge gate)

**Provenance:** single-session design (2026-06-09), spawned from the warmer-builds Phase 0
baseline. Every mechanism cited below is grounded in live code (`scripts/verify.sh`,
`.config/nextest.toml`, the `reify-solver-elastic`/`reify-eval` test trees) or the dark-factory
orchestrator (`merge_queue.py`, `workflow.py`, `b3_gate.py`, `harness.py`, `service_restart.py`),
with file:line anchors. The filesystem/CoW facts in §9 were measured on the box this session.

**Companion (same effort, do not duplicate):** `docs/design/warmer-builds-merge-verify.md`.
This lane is the concrete consumer of the **test-exec-floor lever** that the warmer-builds
**Phase 0** baseline names ("tier the heavy tests off the merge gate"), and it **reuses the
warmer-builds Phase 1 machinery** (persistent warm worktree). It is a *sibling* of Phase 1, not a
standalone idea — see §8/§10. **Hard sequencing dep: warmer-builds Phase 1 lands first.**

---

## 1. Problem

Phase 0 (`docs/notes/warmer-builds-phase0-baseline.md`) measured a *warm* merge-gate verify at
~11 min, of which **~643 s is test-exec the warm build cannot touch** (compile collapses to ~9 s
under Phase 1; the exec floor does not move). That floor is **tail-latency-bound, not CPU-bound**:
each nextest pass saturates 32 cores clearing ~11k fast tests in the first ~30–60 s, then spends its
final 60–120 s on a handful of long tests with most cores idle. The long poles are all numeric:

- `reify-solver-elastic` `determinism::*` — parallel-tolerance / bit-stability **thread-count
  sweeps** (run the solver at 1/2/4/… threads and compare). The release
  `default_parallel_tolerance_equivalent_across_thread_counts` alone is `SLOW [>120 s]` and nearly
  spans the release pass.
- `reify-solver-elastic` P2 analytical validation (cantilever, thick-walled cylinder), euler-column
  buckling, modal benchmarks.
- Two heavy `reify-eval` FEA numeric binaries in the serial OCCT pass (~113 s + ~95 s).

These cost a lot and add little **marginal gate coverage**: the merge gate's job is to catch
regressions in *the merge delta*, and a full parallel-tolerance thread sweep / analytical
convergence study is not delta-shaped. Several are already conceded off the gate —
`cantilever_faithful_convergence_study` / `cylinder_lame_convergence_study` are `#[ignore]`'d
(`crates/reify-solver-elastic/tests/analytical_validation.rs:806,1341`), and the modal/buckling
benchmarks are release-only via `#[cfg_attr(debug_assertions, ignore)]`
(`crates/reify-solver-elastic/tests/modal_benchmarks.rs:409`,
`crates/reify-eval/tests/buckling_smoke.rs:75`). The heavy set that *does* run runs in **both** the
debug and release passes — **paid ~twice**.

## 2. Goal

A **post-merge, asynchronous deep-test lane** that runs the heavy numeric suite **off the verify
hot path**, on a single-flight cadence keyed to `main` advancing, with **autonomous failure
handling**. The merge gate gets faster *and coverage increases* — freed from the hot path, the lane
runs the full matrix (all thread counts, tight tolerances, the convergence studies currently
`#[ignore]`'d), i.e. **more than the gate ever ran**. This is **tiering, not deletion**.

**Non-goals.** It is **not a gate** — it never blocks a merge. It does **not** replace the thin
solver smoke that stays *on* the gate (§4). It is **not** nightly — per-main-advance gives
merge-level attribution; nightly would batch a whole day and make bisection hard.

## 3. Ratified decisions

| # | Decision | Rationale |
|---|---|---|
| **D1** | **Tier, don't remove.** A thin solver smoke stays on the merge gate (blocks merges, commit-level attribution); the full matrix moves offline. | Gross regressions must still fail *synchronously* at merge time. The async lane is a safety net, not the first line. |
| **D2** | **Trigger on main-advance; single-flight; always-from-head.** | Merge-level attribution beats nightly; coalescing keeps a slow suite from queueing behind itself; snapshot-at-run-start guarantees the run reflects current head. |
| **D3** | **Footprint = idle scheduling class + nextest thread cap, off the merge jobserver.** *Not* a hard 1-CPU pin. | Phase 0's headline is contention; idle-class yields completely under load yet uses slack when idle. A hard 1-core pin balloons the thread-sweep tests and widens the attribution batch. |
| **D4** | **Failure handling = confirmation re-run → dedup → normal pending fix task + `escalate_info`; promote to `escalate_blocker` only on stall.** | Lower blast-radius than red-main, and the fix lands *through the gate* — so it can be more autonomous than the human-only red-main path, while dedup + a flake re-run keep it quiet. |
| **D5** | **Warm build = dedicated self-warming worktree reusing Phase-1 machinery — NOT shared/overlaid artifacts.** | Sharing the merge lane's live `target/` is forbidden and pointless (§8/§9). A second dedicated worktree self-warms at head and honors every warmer-builds §11 invariant. |

## 4. What moves, what stays — the test partition

**Moves offline (the `heavy` set):** `reify-solver-elastic` `determinism::*` thread-count sweeps;
`analytical_validation` P2 validation **and** the `#[ignore]`'d convergence studies (run them
first-class offline); `modal_benchmarks`; `buckling_smoke`; the two heavy `reify-eval` OCCT FEA
binaries. Offline, run **once in the profile that matters** (release for the numeric validation),
not both — so even the offline cost is ~half what the gate pays today.

**Stays on the gate (thin smoke):** determinism at **1-vs-2 threads only**, **one** analytical
benchmark at coarse tolerance, one profile. Enough to catch a gross regression in the merge delta
synchronously; cheap enough to leave on the hot path.

**Mechanism — a nextest filterset, not `#[ignore]`.** `.config/nextest.toml` already exists (the
`occt` test-group is staged for task 3767). Add a declarative `heavy` filterset, e.g.
`package(reify-solver-elastic) & test(/determinism|convergence|modal|boussinesq|cylinder/)` plus the
heavy `reify-eval` binaries. Two views over the same set:

- **gate**: `--filterset 'not heavy'` (the smoke runs because it's authored as separate, lighter
  test functions outside the `heavy` pattern).
- **offline**: the `heavy` filterset **+ `--run-ignored all`** (picks up the convergence studies).

A filterset keeps the tests **visible and runnable locally** (unlike `#[ignore]`, which hides them)
and is **auditable** — you can list exactly what's deferred. Selection is driven by a new
**`DF_VERIFY_ROLE=offline`** role in `scripts/verify.sh`, symmetric with the existing `task`/`merge`
roles (`verify.sh:288,294,348`).

## 5. Trigger & single-flight

**Trigger seam.** Reuse the merge worker's existing post-advance callback:
`on_merge_landed → service_restart.note_merge(task_id, base_sha, head_sha)`
(`harness.py:3245`, `service_restart.py:156`). Add an `on_post_merge` hook alongside it — full async
context, exact SHAs, fires at the precise advance moment. (Alternative robustness seam: the reify
`hooks/reference-transaction` main-move log; useful as a fallback when the orchestrator is down,
e.g. a `scripts/land.sh` landing — but the callback is primary.)

**Single-flight / coalescing.** A `dirty` flag is set on each advance. The worker loop: when idle
and dirty, **snapshot current head**, clear dirty, run the suite at that head; if dirty was set
again during the run, immediately re-run at the *new* head; else wait. The next run always starts
from the **head of main**, never a stale SHA.

**Correctness lives in the snapshot, not the trigger.** Because head is sampled at *run-start* (not
trigger-time), a missed trigger only costs *granularity*, never *correctness* — the next advance (or
a cheap `git rev-parse main` poll) catches up. Under a low merge rate you get per-commit attribution;
under a high rate you get small batches (the fix workflow bisects within the batch — see §7).

**Singleton.** A systemd `--user` unit modeled on the existing jobserver-canary pattern
(`scripts/setup-dev.sh`), or an orchestrator-managed singleton; a lockfile enforces one instance.
Note that *single-flight* (one instance, next-from-head) comes entirely from the lock/coalescing and
is **independent of** the CPU footprint decision in §6.

## 6. Footprint / non-contention

The system is **contention-dominated** (Phase 0 finding #1: idle cold ≈ 29 min vs contended
80–148 min). The lane must not add to that.

- **Idle scheduling class** — `nice -n 19 ionice -c3` (SCHED_IDLE). The kernel gives it the whole
  box when there's slack (fast runs, tight batches) and yields **completely** the instant a
  normal-class thread (a task lane or the merge gate) is runnable.
- **Off the merge jobserver.** Phase 0: token hand-off on the shared 32-slot jobserver
  (`/tmp/reify-jobserver`, `verify.py:1632`) is **priority-blind** — `nice` governs runnable
  threads, the jobserver governs *admission*, and a merge `rustc` blocked on a token never gets to
  matter. So the lane must **not** draw from that pool. Its compile demand is ~0 anyway (warm
  worktree, §8); the only real parallelism knob is `cargo nextest --test-threads=N`.
- **`--test-threads=N` is the cap**, and `N` is a tuning knob against the attribution-batch window:
  `N=1` is safest for contention but balloons the thread-sweep tests (a 1-vs-32 sweep time-slices on
  one core) and widens batches; start modest (not 1), measure, tune.

## 7. Failure handling (autonomous, staged, non-blocking)

1. **Confirmation re-run.** Before declaring red, re-run *only* the failing tests, isolated/serial,
   once. This filters (a) pure infra flake — an OOM or timeout *under load* reads as red — and
   (b) the marginal nondeterminism these tolerance tests are prone to. A test that **fails-then-
   passes** is logged as a lower-severity "intermittent nondeterminism" signal, **not** a fix task.

2. **Dedup — mandatory.** Because the lane re-runs from head on *every* advance, a suite that stays
   red while a fix is in flight would, naively, spawn a fresh task + escalation on every advance.
   Fingerprint on the **failing-test-set signature** (model on
   `compute_preexisting_main_break_fingerprint`, `workflow.py:300`, but key on the failing tests,
   **not** `main_sha` — you want to dedup *across* advances while the same test stays red). While an
   open fix task exists for signature *S*, a new red run with *S* **updates** it (append the new
   suspect commit range); a *different* failing test spawns its own task.

3. **Spawn a normal fix task — not the red-main path.** File a **`pending`** fix task (failing test
   IDs + suspect commit range in `metadata`) that the orchestrator dispatches through the standard
   TDD → PR → **merge-gate** loop. This is deliberately **more autonomous than red-main autofix**:
   the post-merge-red-main class is the most-restricted path in the system — B3 *hard-aborts* it and
   routes to a human (`b3_gate.py:288`, "highest-blast-radius unattended-edit scenario") because it
   fix-forwards straight onto main. Here the fix is a **normal queued task that goes through the
   gate**, not an unattended main edit, and a numeric-tolerance regression is lower blast-radius than
   a post-merge type-check break — so the human-only model is the wrong fit.

4. **Escalate, staged.** Raise **`escalate_info`** on confirmed red (visibility, no page). Promote to
   **`escalate_blocker`** (L2) **only** if the fix task can't land or the suite stays red past *N*
   advances.

5. **Never blocks the merge queue.** The signal is asynchronous; merges keep flowing.

## 8. Warm build — dedicated self-warming worktree

Reuse the warmer-builds **Phase 1 machinery** (create-once-at-fixed-path / reset-in-place / retain
`target/` / exempt-from-prune; `warmer-builds-merge-verify.md` §6 Phase 1, `git_ops.py`), but
**instantiate it a second time** for this lane (e.g. `.worktrees/_offline-deep`), dedicated and
tracking head:

- **Single-consumer of its *own* `target/`** → honors warmer-builds §11 invariants L150/L179/L180
  verbatim (the warm `target/` is safe only when single-consumer; never shared across concurrent
  cargo; dedicated, not borrowed).
- **Self-warms after run one.** It always runs at head, so head→head is the near-pure cargo
  fingerprint pass Phase 0 measured (compile → ~9 s). Only the per-merge delta + reverse-dep closure
  ever recompiles. It does **not** need the merge worker's artifacts after the one-time bootstrap.
- **Reuse the event + shared sccache, not the artifacts.** The `on_post_merge` event (§5) is the
  trigger; sccache already shares dependency rlibs cross-worktree, so the first cold bootstrap is
  cheap-ish and one-time (irrelevant for a background lane).
- **Disk is the only real cost — bound it by scope.** The lane runs *only* the `heavy` filterset, so
  its build is the **dependency cone of `reify-solver-elastic` + `reify-eval`**, not the full
  workspace (no clippy-all-targets, no gui-check, no ~745-bin sweep). That `target/` is a *fraction*
  of the merge lane's 177 GB. warmer-builds Phase 3 (debuginfo trim) shrinks it further; a dedicated
  lean profile is possible.

## 9. Rejected artifact-reuse alternatives (pre-empts "can't we reuse the warm build?")

The instinct to reuse the merge worker's *live* warm `target/` is natural; for *this* lane it does
not pay off, for layered reasons. (CoW reflink reuse *is* worthwhile — but on the merge lane's
verify pool, not here; see (b) and warmer-builds PRD §10.) Recorded so the question is not
re-litigated.

- **(a) Live shared `target/`** — **rejected.** Two failures, both re-creating the contention we're
  removing: cargo takes a target-dir build lock, so the two lanes' builds **serialize / block each
  other**; and the lanes sit at **different trees** (offline at head, merge worker at a *candidate* =
  head + the in-flight task's diff), so fingerprints **thrash** on every crate that differs —
  including `reify-solver-elastic` exactly when a task touches it. Explicitly forbidden by
  warmer-builds §11 L150 ("concurrent cargo on one `target/` is unsafe… single-consumer only because
  the lane is serial"), L179, L180.

- **(b) reflink / CoW snapshot of the post-advance `target/`** — **possible now, but not worth it
  for *this* lane.** `xfsprogs` is installed, so an XFS (or loop-backed XFS) volume gives real
  reflink — a cheap CoW snapshot of a warm `target/` *is* achievable (the box is ext4 today — root
  `/dev/nvme0n1p5`, worktree store `data_lv_1` = `/dev/mapper/vgroup0-data1`, where `cp --reflink`
  fails — so it needs that XFS volume first). But the offline lane is **single-flight, narrow-cone
  scoped, and self-warming**: there is no pool of concurrent verifies, so no base to amortize across
  K slots, and its dedicated worktree (§8) is already warm at head after run one. CoW base-sharing is
  a *disk* optimization that pays off across a **pool** of lanes — i.e. on the **merge lane** (see
  warmer-builds PRD §10, "Future-lever note — per-host parallel warm verifies"), not for a single
  self-warming offline lane. Reflinking a base across paths also hits the **path-sensitivity trap**
  (base at path X, snapshot at path Y → fingerprints invalidate → full recompile; warmer-builds
  invariant 2) unless each snapshot is bind-mounted to a canonical path — overhead this lane has no
  reason to take on.

- **(c) overlayfs** — **rejected (and now moot).** overlayfs *would* give copy-on-write on any FS,
  but it requires a **frozen, read-only-stable lowerdir** (kernel contract: mutating the lower under
  a live mount is *undefined behavior*), and the merge worker **mutates its `target/` continuously**
  (back-to-back per Phase 0) — there is no stable window to overlay the live tree; freezing a lower
  needs a copy anyway. And it shares **storage, not fingerprints** — crates that differ still
  recompile (only disk is saved) — with cargo/overlayfs friction (hardlink copy-up breakage,
  mtime-fingerprint quirks, rename-across-layer edges). With XFS reflink available (b), a reflink
  *snapshot* is the cleaner CoW primitive regardless; overlayfs has no remaining role.

- **Strategic:** for *this* lane the conclusion is unchanged — use the dedicated self-warming
  worktree (§8). It **self-warms after run one** and never needs the merge artifacts post-bootstrap;
  the only genuine prize — disk — is already bounded by narrow-cone scoping + Phase 3, and the
  cacheable dependency rlibs are **already shared by sccache** (the diverging bin/test artifacts are
  exactly the category sccache can't cover, and the one that would thrash). The right reuse boundary
  here is **machinery + trigger event + shared sccache**, not the artifact tree. CoW reflink reuse is
  real and worthwhile — but on the *merge* lane's verify pool (warmer-builds §10 Future-lever note),
  where a shared base amortizes across K concurrent slots; this single-flight offline lane is not
  that case.

## 10. Ownership / cross-repo seam

- **reify-local:** the `heavy` nextest filterset (`.config/nextest.toml`); the `DF_VERIFY_ROLE=offline`
  role + verify command in `scripts/verify.sh`; the thin gate-smoke residue; an optional lean build
  profile. Shippable **first**, independently — moving the heavy set behind the filterset and adding
  the smoke can land and be verified before any orchestrator wiring exists.
- **dark-factory:** the `on_post_merge` trigger (`harness.py`/`merge_queue.py`); the singleton lane
  worker (single-flight / coalesce / from-head); the dedup'd fix-task spawn + `escalate_info` /
  `escalate_blocker` staging (`workflow.py`); the **second** persistent-worktree instantiation
  (Phase-1 machinery, `git_ops.py`).
- **Sequencing:** the DF lane has a **hard dependency on warmer-builds Phase 1** (the persistent
  worktree path) landing first. It gets Phase 2 (linker) and Phase 3 (debuginfo) benefits for free.

## 11. Invariants / do-nots

- **Never a gate.** The lane MUST NOT block, halt, or gate the merge queue.
- **Keep the gate smoke.** Do not pull *all* solver coverage off the gate — gross regressions must
  still fail synchronously with commit-level attribution.
- **Dedicated `target/` only.** Never share or overlay the merge lane's `target/` (§9; warmer-builds
  §11).
- **Off the merge jobserver.** Never draw from `/tmp/reify-jobserver` (priority-blind admission).
- **Dedup is mandatory.** No naive per-advance fix-spawn — it would flood tasks/escalations while red.
- **Confirmation re-run before escalating.** Always filter flake/contention first.

## 12. Status / next step

**PRD-ready once warmer-builds Phase 1 is committed** (the machinery dependency). Natural
decomposition: a **reify-local PRD** (test partition + `offline` role + gate smoke — independently
shippable) and a **dark-factory PRD** (the lane worker + trigger + failure handling, gated on
Phase 1). Hand off via `/prd`. **Do not implement from this doc directly, and do not author the PRD
as part of the drafting session.**
