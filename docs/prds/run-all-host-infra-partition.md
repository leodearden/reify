# PRD — `run_all.sh` host-infra partition + concurrent hermetic pool, Part A (reify-local)

**Status:** author-complete, gates passed (2026-07-01). Decompose-ready — **stop here for Leo's review; do NOT queue.**
**Slug:** `run-all-host-infra-partition` · **Milestone:** version-agnostic verify-pipeline infra (root `docs/prds/`).
**Authoritative brief:** `~/.claude/spawn-briefs/run-all-host-infra-partition-prd.md` (this session; verified against HEAD).
**Structural precedent (MIRRORED):** `docs/prds/offline-deep-test-lane.md` (Part A/B split + off-by-default flip seam + resolve-to-disk drift-guard).
**Companions:** `docs/prds/infra-test-wallclock-deflake.md` (T8 handoff — §5); `docs/prds/cpu-load-admission-control.md` (the α/β/γ CPU-governance axis these tests exercise).

This PRD is the **reify-local, independently-shippable Part A** — the sibling of `offline-deep-test-lane.md`
for the *infra-test suite* rather than the *numeric solver suite*. It ships primitives + an **off-by-default
flip seam** + an executable drift-guard. **Part B** (the dark-factory cold-lane worker/trigger/flip) is
`docs/prds/offline-deep-test-lane-worker.md` — **author-complete & decompose-ready** (2026-07-01), scoped to the
*numeric* nextest suite. **See the §11 reconciliation addendum**, which supersedes every stale "pending / NOT YET
AUTHORED" reference below and records the final decisions + cross-project-edge sequencing. The infra host-global
residue rides the *same* post-merge cold lane only once Part B's worker is **extended** with a `run_all --scope
host-infra` invocation of reify's new **H9** runner (§6/§11).

---

## 0. Scope & consumers (G1)

**What Part A introduces (every mechanism has a named consumer — no orphan producers):**

| Mechanism | Consumer (in Part A) | Downstream consumer (Part B) |
|---|---|---|
| **Classification manifest** — `host-exclusive` + `intra-run-serial` declared sets over all `test_*.sh` | H2 pool reads it (what to parallelize/serialize); H3 seam reads it (what to exclude); the H1 drift-guard enforces it | the cold-lane worker reads the **host-exclusive** set to know what to run off-gate |
| **Classification drift-guard** meta-test (mirror `occt-scope-lib`/`release-scope-lib`) | the verify pipeline (infra step) fails RED on any **unclassified** `test_*.sh` | — |
| **Concurrent hermetic pool** in `run_all.sh` (bounded by a host-global counting semaphore + PSI soft-gate; per-test output buffering) | every `run_all.sh` caller — each task/merge verify + the operator — runs the SAFE_LIGHT set concurrently | — |
| **`REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` knob** (default **0** = host-infra still runs on the hot path) | `run_all.sh` reads it; default keeps the residue on the hot path | **Part B flips it to `1`** (orchestrator verify env) the moment the cold lane is live |
| **cpu_load_governance pool-safe conversion** (slice-naming `$$`-prefix fix + synthetic-PSI fixturization + confined-cgroup-quota) | H2 pool runs the converted file concurrently, host-load-independently | — |
| **cpu_governed_exec split** (fixturize `A*`/`B1–B7`/`C*`; extract real-placement `D*` residue) | H2 pool runs the hermetic remainder | the cold lane runs the extracted `D*` residue |
| **warm_lane_pool B11 → private FS** (`df` delta measured on a private loopback, not shared `/tmp`) | H2 pool / hot path runs B11 immune to a concurrent disk-writer | the cold lane runs the substrate-real blocks |
| **Lane-X host-exclusive flock primitive** (shipped, *not* invoked) | *(no reify-local consumer — a shipped primitive)* | **Part B invokes it** to enforce the cold lane's single-flight (cross-project dep, §6) |

**User-observable surface (the reify-local leaf signals, G2):**
- `bash tests/infra/run_all.sh` runs the ~87 SAFE_LIGHT tests **concurrently**, yet still emits the byte-identical
  `=== Summary: N discovered, M failed ===` line **and** the bare `FAILED <names>` classifier marker (dark-factory
  `verify.py` `^FAILED\s` regex), with per-test output buffered and emitted in discovered order.
- A **fault-injected** failing test still surfaces by name in the `FAILED …` marker under concurrency (the contract
  is not silently dropped when tests interleave).
- The classification **drift-guard** runs green in the verify pipeline and goes **RED** when a new/renamed `test_*.sh`
  is left unclassified, or when a declared entry no longer resolves to a file on disk.
- With the default knob (`REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` unset/0), `run_all.sh`'s discovered/run set is
  **strictly additive** — every host-infra residue test still runs on the hot path (**no test runs nowhere**).
- With `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1`, the discovered set = full **minus** the declared host-exclusive set,
  the `Summary` count drops by **exactly** the host-exclusive count, and the drift-guard proves the pool ⊕ serial ⊕
  host-exclusive buckets **partition** the universe with **no overlap and no orphan** — proving the flip seam works.
- `test_cpu_load_governance.sh` passes under **concurrent load** (its `cpu.weight`-ratio rows measured inside a
  confined CPU-quota'd cgroup subtree, host-load-independent) **or** skips cleanly (never false-RED) when cgroup
  delegation is unavailable; two concurrent runs use disjoint per-`$$` parent slices.

## 1. Problem & premise (G6 record)

`tests/infra/run_all.sh` discovers **94** `test_*.sh` files (excl. `test_helpers.sh`) and runs them **serially**
(`run_all.sh:53` `for test_file in …; do … bash "$test_file"`). Two forces make this the wrong shape:

1. **Host-level concurrency is a hard requirement.** The dark-factory orchestrator runs many warm-lane worktrees'
   verifies **simultaneously** on one box; `run_all.sh` is a plan line inside each verify. The suite must be
   robust — and fast — when N copies run at once across the shared host.
2. **A host-global-unsafe minority poisons the hot path.** Four files do **real** CPU burn / real cgroup
   delegation / real reflink-FS + cargo, and give **wrong answers or false-REDs under concurrent load** — the exact
   class that has repeatedly ambushed unrelated green tasks' merge gates (e.g. ROW4-1 false-RED, task #4656;
   host-baked occt-cap, task #4901). They belong on an **idle box**, not the per-task/per-merge hot path.

**The partition (this PRD).** Run the hermetic majority **concurrently**; move the host-global-unsafe minority off
the hot path into a post-merge **cold lane** — via an **off-by-default seam** that is **strictly additive on
landing** (zero coverage gap). Where a load-sensitive test can be made **host-load-independent** cheaply (the
cpu_load_governance `cpu.weight`-ratio rows, via a confined CPU-quota'd cgroup subtree), **rescue it into the pool**
rather than exile it — coverage stays on the hot path, robustly.

### G6 premise re-checks vs HEAD (main @ `255e36b7ad`, 2026-07-01) — do not rubber-stamp

This PRD is shell/test infra — no `.ri` grammar, no numeric solver claims — so G6 branches 1/2 fire only on the
concurrency-control constants and the assertions this PRD *touches*. Each is validated, not asserted:

1. **No host-baked constant (anti-#4901).** The pool's concurrency bound `N` is derived from `nproc` **at runtime**
   (`lib_slot_acquire.sh` takes `N` as an argument; the caller computes it) — **never** a frozen integer. Host-baked
   counts have deterministically false-blocked the merge gate (occt-cap=24 on the 16-core laptop, #4901;
   `feedback_load_flake_may_be_host_baked_constant`). The drift-guard and the semaphore both stay `nproc`-relative.
2. **No new wall-clock upper bound (anti-regression).** The wall-clock de-flake is **complete** under
   `infra-test-wallclock-deflake.md` (T1–T9 landed; `test_no_new_wallclock_upper_bounds.sh` is the standing guard).
   Any new poll budget this PRD adds **must** use `load_tolerant_attempts` (MAX-clamped; `load_tolerance_lib.sh:177`)
   so that guard stays green. This PRD must **not** reintroduce, re-tune, or re-litigate any wall-clock assertion.
3. **Confined-quota is scale-invariant — no new numeric bound.** cgroup `cpu.weight` distributes *available* CPU
   proportionally among siblings **regardless of the parent's quota**, so two children at 300:100 split their
   parent's budget 3:1 whether the parent has 32 cores or a confined 2. The confined measurement therefore
   reproduces the **same** ratio as the full-box measurement — this PRD **inherits** ROW4-1's already-passing,
   already-de-flaked `merge_share ≥ W_merge/(W_merge+W_task) − tol = 0.65` bound (#4634/#4656) and asserts **no new
   number**. The quiet-box-skip fallback (`quiet_box_met`, pure) covers delegation-unavailable hosts.
4. **B11's `≤50 MiB` df-delta is inherited, not new** — this PRD fixes **where** it is measured (a private
   loopback vs shared `/tmp`), not the bound (`test_warm_lane_pool.sh:1763`).

**Conclusion:** the premise holds — the concurrency win and the load-robustness fix are real, and Part A introduces
**no** frozen constant, **no** new wall-clock bound, and **no** new numeric assertion.

## 2. Goal & non-goals

**Goal (Part A).** Ship the reify-local **mechanism** that (a) runs the hermetic `run_all.sh` majority concurrently
under a host-global bound, (b) classifies every `test_*.sh` into pool / intra-run-serial / host-exclusive with an
executable drift-guard, (c) makes the four host-global-unsafe files either **pool-safe** (cpu_load_governance) or
cleanly **extractable** to a cold lane (cpu_governed_exec `D*`, cpu_load_governance_deflake, warm_lane_pool residue),
and (d) exposes an **off-by-default** exclusion seam Part B flips, such that:
- nothing runs *nowhere* on landing (strictly additive; `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=0`);
- the SAFE_LIGHT set runs concurrently immediately, preserving `run_all.sh`'s exact output contract;
- Part B can move the residue off the hot path by flipping **one env knob** (wired via a cross-project dep, §6).

**Non-goals (Part A).**
- **Not** the cold lane itself — the trigger, the single-flight worker, the flock **invocation**, the
  `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1` **flip** — **all Part B** (dark-factory; shares offline-deep-test-lane's lane).
- **Not** the `test_cpu_governed_exec.sh` D1–D6 **production-slice contamination** fix — that is a standalone bug,
  **task #4919** (in-progress). This PRD's cpu_governed_exec split **depends on** #4919 and must **not** duplicate it.
- **Not** any wall-clock re-tuning — done under `infra-test-wallclock-deflake.md` (§1 premise 2).
- **Never a gate.** The residue's cold-lane run never blocks a merge — but that invariant is enforced by Part B;
  Part A's role is to keep the residue on the hot path (default 0) and simply *not wire it into* any exclusion.

## 3. The partition — three buckets, behavior-based

`run_all.sh` discovers whole `test_*.sh` files, so classification is at **file** granularity. Crucially it is
**behavior-based, not name-based**: of the ~19 files whose names match cpu/warm-lane/governance/semaphore/psi, only
**four** do real burn/substrate; the other ~15 are hermetic (mktemp + PATH-stubbed `systemctl`/`mount`/`mkfs`/
`losetup`) and belong in the pool. No filename heuristic can bucket them — hence the *explicit* declared sets +
hard drift-guard (ratified #2).

| Bucket | Members (this session's audit) | Runtime treatment | Exclusion knob |
|---|---|---|---|
| **pool** (SAFE_LIGHT, ~87) | the hermetic majority | run **concurrently**, bounded by the host-global semaphore + PSI soft-gate | never excluded |
| **intra-run-serial** (3) | `test_reify_audit_ptodo.sh`, `test_tree_sitter_pipeline.sh`, `test_verify_semaphore_e2e.sh` | run **serially within one `run_all` invocation** (they mutate the lane's own per-lane CoW `target/` + working-tree `parser.c`); isolated **across** lanes (separate working trees) so different lanes' serial groups still overlap | never excluded |
| **host-exclusive** (residue → cold lane) | initially `test_cpu_load_governance.sh`, `test_cpu_governed_exec.sh`, `test_cpu_load_governance_deflake.sh`, `test_warm_lane_pool.sh`; **refined** by H5–H7 (see below) | run on the hot path (serially) when knob=0; **excluded** (cold lane runs them) when knob=1 | `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` |

**Conservative-first, refine-later (DA1-safe).** H1 lands the classification with the four unsafe files bucketed
**whole-file host-exclusive** (safe default: at knob=0 they still run on the hot path, just outside the concurrent
pool). H5–H7 then **refine**, moving hermetic portions into the pool:
- **cpu_load_governance → pool** (H4+H5): the 5 real rows are *rescued* via confined-cgroup-quota (host-load-
  independent) + quiet-box-skip fallback; the hermetic rows run under synthetic-PSI fixtures. The whole file becomes
  pool-safe — no host-exclusive residue.
- **cpu_governed_exec → split** (H6): `A*`/`B1–B7`/`C*` → pool (fixtures / pure string-reads); the real-scope-
  placement `D*` residue is **extracted** into a sibling `test_cpu_governed_exec_hostexcl.sh` listed host-exclusive.
  Depends on #4919 (which isolates D1–D6 to `$$`-scoped slices first). B8 (host-gated detection *read*, places no
  scope) → pool; exact per-row line finalized at decompose against the drift-guard.
- **warm_lane_pool** (H7): stays host-exclusive for its substrate-real blocks (real reflink + cargo); B11's `df`
  delta is moved onto a **private** loopback so it is immune to a concurrent disk-writer.
- **cpu_load_governance_deflake**: stays host-exclusive (rides its SUT's real burns); classification-only (H1).

Because refinement only ever moves files **into** the pool (more parallel, never fewer tests run), each intermediate
state is correct and DA1-safe. The drift-guard enforces **full coverage** at every step: every discovered `test_*.sh`
is in exactly one bucket; a new/unclassified test is **RED** (ratified #2). At runtime, an unclassified test
fail-safes to the intra-run-serial bucket (runs, never wrongly parallelized/excluded) so a classify-lag never breaks
the suite — but the drift-guard still hard-fails to force the classification.

## 4. Ratified decisions (Leo, this session — folded in so no re-interrogation is needed)

| # | Decision |
|---|---|
| **DA1** | **Strictly additive on landing; the flip is deferred to Part B.** `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` defaults to **0** — every host-exclusive test keeps running on the hot path until Part B's cold lane is live. **No test ever runs nowhere.** (Chosen over "flip in Part A + accept a bounded gap".) |
| **DA2** | **The flip is an off-by-default env knob, pulled by Part B via a cross-project dep, immediate & reversible.** `run_all.sh` reads `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` (default 0). Part B sets it to `1` in the orchestrator verify env the instant the cold lane is live — a config deploy, **not** a reify code change. A real `add_dependency` edge makes Part B's flip task depend on **both** this PRD's knob leaf (**H3**) **and** Part B's lane-live leaf, so the flip fires exactly when the lane can catch the excluded tests. Analogous to the (planned, off-by-default) `REIFY_GATE_EXCLUDE_HEAVY` of the sibling PRD. |
| **#1 idle-box coverage** | **confined-cgroup-quota** for cpu_load_governance's 5 real rows — measure the `cpu.weight` ratio inside a **delegated, CPU-quota'd cgroup subtree** so it is **host-load-independent**; **fall back to a quiet-box skip** (`quiet_box_met`, `load_tolerance_lib.sh:146`) when cgroup delegation is unavailable at runtime. (b-else-a.) This *rescues* the rows into the pool rather than exiling them. |
| **#2 new-test default** | **explicit classification required** + a drift-guard meta-test that **fails** on any unclassified `test_*.sh` (mirror `occt-scope-lib`/`release-scope-lib`: declared set == discovered set, diff fails). |
| **#3 pool concurrency** | a **host-global counting semaphore** (reuse `scripts/lib_slot_acquire.sh`), **`N` derived from `nproc` at runtime — NO baked constant** + a **PSI soft-gate** to yield under concurrent cargo. Preserve `run_all`'s output contract under concurrency: the `=== Summary: N discovered, M failed ===` line and the **bare `FAILED <names>`** classifier marker (`verify.py` `^FAILED\s`); **buffer each test's output per-test and emit in discovered order.** |
| **#4 cold-lane home** | **dark-factory Part B** (`docs/prds/offline-deep-test-lane-worker.md`, **not yet authored**). This PRD ships **only** primitives + the off-by-default seam. Do **not** scope the trigger, the worker, the flock *invocation*, or the knob flip. |
| **#5 flock** | a **single coarse host-exclusive Lane-X flock** (the cold lane is single-flight). Ship the **primitive** (**H8**); **Part B invokes it.** |
| **DA-fixed-paths** | All coordination primitives — the pool semaphore, the Lane-X flock — live at **fixed HOST paths** (per-uid, like the existing test semaphore), **not** worktree-relative, so warm lanes coordinate across the shared box (warm lanes share one `.git` but each has its own working tree + per-lane CoW `target/`). |

## 5. Pre-conditions / substrate (G3 — all verified against HEAD this session)

- **`scripts/lib_slot_acquire.sh`** — `slot_acquire LOCK_BASE N WAIT [REASON]`: N-slot shuffle-acquire flock
  semaphore, `N` a runtime arg (no baked constant), holds FD 9, `WAIT="unlimited"` sentinel supported, `@@REIFY_CLOCK_*@@`
  markers. Registered in `verify-pipeline-paths.txt`. ✔ (the pool's host-global bound — H2.)
- **`scripts/cpu-admit.sh`** — PSI soft-gate: `cpu_admit_read_avg10()` parses `/proc/pressure/cpu` `avg10=`
  (`:93`); `_cpu_admit_mem_pressure_high()` reads `/proc/pressure/memory` (`:131`); admit mode is admit-on-timeout
  (never exit 75). ✔ (the pool's PSI soft-gate — H2.)
- **`tests/infra/load_tolerance_lib.sh`** — `load_tolerant_attempts` (`:177`, MAX-clamped poll budgets) and
  `quiet_box_met AVG10 CEILING` (`:146`, **pure** — caller samples avg10). ✔ (H2 poll budgets + H5 skip fallback.)
- **`scripts/provision-warm-lane-fs.sh`** — builds a dedicated XFS-reflink loopback (`mkfs.xfs -m reflink=1` `:245`,
  `losetup` `:196`, `mount` `:253`), `REIFY_WARM_LANE_MOUNT` (`:98`), mandatory `cp --reflink=always` probe. ✔ (H7.)
- **`scripts/lib_cgroup.sh`** — reads `REIFY_CPU_GOVERN_SLICE_TASK` (`:85`, default `reify-governed-agents.slice`) /
  `REIFY_CPU_GOVERN_SLICE_MERGE` (`:88`, default `reify-governed-merge.slice`) in `cgroup_role_slice()`. The H4
  slice-naming fix is fed **through these knobs from the test** — **no `lib_cgroup.sh` change.** ✔
- **`tests/infra/test_cpu_load_governance.sh`** — current private slices `_ROW4_SLICE_TASK="reify-govtest-agents.slice"`
  / `_ROW4_SLICE_MERGE="reify-govtest-merge.slice"` (`:605/606`) nest under a **shared** `reify-govtest.slice`
  parent (`:549-550`); real rows ROW1-1/ROW2-1/ROW2-2/ROW3-1/ROW4-1 host-gated + `quiet_box_met`-gated; hermetic
  rows SELF-1..5 / FIXTURE-1..6 / ROW4-BYPASS / ROW1-2 use synthetic PSI (`REIFY_CPU_GOV_TEST_PROC_PATH`,
  `REIFY_CPU_ADMIT_PROC_PATH`). ✔ (H4/H5.)
- **`tests/infra/test_cpu_governed_exec.sh`** — `A*`/`B1–B7`/`C*` hermetic; **D1–D6 place scopes under production
  `reify-governed.slice/…`** (`:266/283`, the #4919 contamination); D7/D8 already use `$$`-scoped slices
  (`:36-37`); B8 (`:191`) is a host-gated detection **read** (no scope placed). ✔ (H6.)
- **`tests/infra/test_cpu_load_governance_deflake.sh`** — meta-test driving `test_cpu_load_governance.sh` as SUT
  (`:27`); hermetic itself but inherits the SUT's real burns. ✔ (H1 classification.)
- **`tests/infra/test_warm_lane_pool.sh`** — `detect_substrate()` (`:188`) ladder: `REIFY_WARM_LANE_MOUNT` →
  **`${TMPDIR:-/tmp}` scratch reflink probe (rung 2, can win)** → opt-in `provision-warm-lane-fs.sh` (rung 3, gated
  on `REIFY_RUN_WARM_LANE_GATE=1`). B11's `df --output=avail -m "$_GATE_DIR"` deltas (`:463/473`) → `≤50 MiB`
  assertion (`:1763`). ✔ (H7 forces B11 onto a private mount.)
- **The 3 intra-run-serial tests' shared-within-lane resource** — `test_verify_semaphore_e2e.sh` regenerates
  `parser.c` via `scripts/tree-sitter-generate.sh --force` (`:163`); `test_reify_audit_ptodo.sh` runs the
  `reify_audit_guard … rebuild-budget-safe` pre-build preamble (`:98`) — both must be preserved under serialization. ✔
- **Drift-guard precedent** — `scripts/occt-scope-lib.sh` (`occt_declared_set` reads `occt-touching-crates.txt`) +
  `tests/infra/test_occt_gated_scope.sh` (declared==derived, `test -z "$_DIFF_OUT"`); `scripts/release-scope-lib.sh`
  + `tests/infra/test_release_scoped_scope.sh`. ✔ (H1 mirrors this shape.)
- **`scripts/verify-pipeline-infra-tests.txt`** (artifact→infra-test-glob registry) + **`scripts/verify-pipeline-paths.txt`**
  (load-bearing manifest — register `lib_slot_acquire.sh` is already listed). ✔ (register the H1 drift-guard.)
- **`scripts/tree-sitter-generate.sh`** — regenerates `src/parser.c`, flock-guarded, `--force`. ✔
- **`REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` is absent from the tree** (this PRD introduces it); the analogous
  `REIFY_GATE_EXCLUDE_HEAVY` is **docs-only** in the sibling PRD (not yet in code) — a *planned* off-by-default
  precedent, not landed substrate. ✔

## 6. Cross-PRD relationship + seam ownership (G4)

Two PRDs, split across repos — the same cross-repo seam class as cpu-governance (α/β/γ ↔ ζ), the warm-lane D8/D10
seams, and offline-deep-test-lane's flip seam: **reify ships the primitives, dark-factory wires the consumer.**
Part B is `docs/prds/offline-deep-test-lane-worker.md` (**pending**, Leo) — and the infra residue rides the **same**
cold lane offline-deep-test-lane Part B establishes (one lane, two residue sources).

| Deliverable | Owner | Depends on |
|---|---|---|
| classification manifest + drift-guard, concurrent pool, `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` seam, cpu_load_governance/cpu_governed_exec/warm_lane_pool fixturization, Lane-X flock **primitive** | **reify (Part A — this PRD)** | existing `run_all.sh` + `lib_slot_acquire.sh` + `cpu-admit.sh` + `provision-warm-lane-fs.sh` only; cpu_governed_exec split also on **#4919** |
| `on_post_merge` trigger + single-flight cold-lane worker (always-from-head) | **dark-factory (Part B)** | Part A; warm-lane pool (live) |
| **Lane-X flock invocation** around the single-flight cold-lane run | **dark-factory (Part B)** | **cross-project edge → Part A H8 (flock primitive)** |
| dedup'd fix-task spawn + `escalate_info`/`escalate_blocker` staging for residue failures | **dark-factory (Part B)** | Part A |
| **flip `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1`** in orchestrator verify env | **dark-factory (Part B)** | **cross-project edge → Part A H3 (knob leaf)** + Part B lane-live leaf |

**The flip seam contract (the one interface both PRDs must agree on):**
> `tests/infra/run_all.sh` **excludes** exactly the files declared in the host-exclusive set **iff**
> `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` is exactly `1`; for any other value (unset/empty/0) it runs the full set
> unchanged. The variable is read from the environment so the orchestrator's verify env can set it without a reify
> code change. Flipping it is immediate and reversible; the `Summary` count drops by exactly the excluded count.

**Ownership is unambiguous — no reciprocal "the other owns it" pattern.** Part A owns the seam + the default (`0`)
+ the flock primitive; Part B owns the pull (`1`), the flock invocation, and the async lane that makes the pull
safe. Both cross-project edges are wired at decompose time (Part B's flip task deps-on H3; Part B's flock-invocation
task deps-on H8), per DA2.

## 7. Out of scope (Part A)

- The entire dark-factory cold lane (trigger, single-flight worker, flock **invocation**, dedup, fix-spawn,
  escalation, the knob **flip**) — Part B.
- The `test_cpu_governed_exec.sh` D1–D6 production-slice **contamination** fix — **task #4919** (in-progress); H6
  depends on it and must not duplicate it.
- Any wall-clock re-tuning / re-litigation — done under `infra-test-wallclock-deflake.md`.
- Fixing any test that turns out RED when first run first-class in the cold lane — a normal fix-task finding
  (non-blocking), **not** a Part-A blocker.
- A second warm worktree for the cold lane — Part B (reuses the live warm-lane CoW pool).

## 8. Invariants / do-nots

- **Additive only on landing.** `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` defaults to `0`; Part A must not change what
  `run_all.sh` runs (beyond running the SAFE_LIGHT set concurrently). **No test may run *nowhere* at any point.**
- **Partition completeness (executable).** pool ⊕ intra-run-serial ⊕ host-exclusive must **cover** all discovered
  `test_*.sh` with **no overlap and no orphan**; a new/unclassified test is a **hard RED** (ratified #2). Enforced
  by the drift-guard, not a tabulated promise.
- **Fixed host paths.** The pool semaphore + Lane-X flock live at **per-uid fixed host paths** (mirror the existing
  test semaphore), never worktree-relative — warm lanes must coordinate across the shared box.
- **Preserve the output contract.** Under concurrency, `run_all.sh` must still emit `=== Summary: N discovered, M
  failed ===` and the bare `FAILED <names>` marker (`verify.py` `^FAILED\s`); buffer per-test and emit in discovered
  order. A concurrency bug that drops/garbles the marker reclassifies a real failure as `tree_sitter_generate_error`
  (a thrash-escalating L1) — the exact failure mode `run_all.sh`'s header warns about.
- **No host-baked constant.** `N` is `nproc`-derived at runtime (anti-#4901).
- **No new wall-clock upper bound.** New poll budgets use `load_tolerant_attempts` (MAX-clamped) so
  `test_no_new_wallclock_upper_bounds.sh` stays green. No wall-clock assertion is added, re-tuned, or re-litigated.
- **Non-vacuous confined-quota.** The confined cpu_load_governance rows must still go **RED** if governance is
  broken (under a quiet/delegated box) — the skip is only for delegation-unavailable hosts, never a blanket pass.
- **Preserve the serial subgroup's preambles.** `test_reify_audit_ptodo.sh`'s rebuild-budget-safe pre-build and
  `test_verify_semaphore_e2e.sh`'s `tree-sitter-generate.sh --force` regen must remain intact under serialization.

## 9. Decomposition plan (leaf tasks — each names a user-observable signal, G2)

> All leaves are reify-local, verifiable without any orchestrator wiring. `metadata.files` follows the tight-or-empty
> rule (name a file only on a high-confidence anchor; `[]` otherwise). The H4/H5 pair and the H6 split all edit test
> files under `tests/infra/`; H4→H5 are chained (same file) to avoid concurrent-lock/merge conflicts.

- **H1 — classification manifest + drift-guard meta-test (+ registry row).** Declare every `test_*.sh` into
  pool / intra-run-serial / host-exclusive (the four unsafe files start whole-file host-exclusive — conservative).
  A drift-guard (mirror `occt-scope-lib`/`test_occt_gated_scope.sh`) asserts declared-union == discovered set (no
  orphan, no overlap) and each entry resolves to a file. Register in `scripts/verify-pipeline-infra-tests.txt`.
  *Signal:* the drift-guard runs green in the verify pipeline; adding an unclassified `test_*.sh` (or deleting a
  declared one) makes it **RED**. *Files:* the manifest file(s) under `tests/infra/`, `tests/infra/test_run_all_classification.sh`,
  `scripts/verify-pipeline-infra-tests.txt`. **Keystone — H2/H3/H6/H7 consume it.**

- **H2 — concurrent hermetic pool in `run_all.sh`.** Parallelize the pool bucket bounded by a host-global counting
  semaphore (`lib_slot_acquire.sh`, `N` from `nproc` at runtime, fixed per-uid host path) + a PSI soft-gate
  (`cpu-admit.sh` avg10) + **per-test output buffering emitted in discovered order**; run the intra-run-serial
  subgroup serially (preserving its preambles). *Signal:* `bash tests/infra/run_all.sh` runs the SAFE_LIGHT set
  concurrently, emits the byte-identical `=== Summary: N discovered, M failed ===` line + bare `FAILED <names>`
  marker, and a fault-injected failing test still appears in the marker under concurrency. *Files:* `tests/infra/run_all.sh`
  (+ a small pool helper lib if extracted). *Depends:* H1 (needs the intra-run-serial set).

- **H3 — off-by-default `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` exclusion seam (default 0).** `run_all.sh` excludes the
  declared host-exclusive files **iff** the knob is exactly `1`; default 0 keeps them on the hot path. *Signal:*
  knob unset/0 → discovered/run set unchanged (residue still runs); knob=1 → discovered set = full minus
  host-exclusive and the `Summary` count drops by exactly that count. **This is the cross-project flip seam** Part B
  depends on. *Files:* `tests/infra/run_all.sh`. *Depends:* H1.

- **H4 — cpu_load_governance slice-naming `$$`-in-PREFIX fix.** Replace `reify-govtest-{agents,merge}.slice` with
  `reify-govtest$$-{agents,merge}.slice` nested under a common `reify-govtest$$.slice` parent — the `$$` **must** be
  the parent-defining prefix segment (not trailing), so each concurrent run gets a **unique parent** while the two
  children stay siblings (the C-G2 weight-ratio invariant). Fed through `REIFY_CPU_GOVERN_SLICE_TASK/_MERGE` (no
  `lib_cgroup.sh` change). *Signal:* two concurrent `test_cpu_load_governance.sh` runs use disjoint parent slices
  (`reify-govtest<pidA>.slice` vs `<pidB>`), so their `cpu.weight` measurements don't cross-contaminate; the
  ROW4 sibling-under-common-parent assertion still holds. *Files:* `tests/infra/test_cpu_load_governance.sh`.

- **H5 — cpu_load_governance pool-safe conversion (rescue into the pool).** Synthetic-PSI-fixturize the hermetic
  rows (SELF-*, FIXTURE-1/2/3/5, ROW4-BYPASS, ROW1-2, FIXTURE-6) and apply **confined-cgroup-quota** to the 5 real
  rows (ROW1-1/ROW2-1/ROW2-2/ROW3-1/ROW4-1) — measure the `cpu.weight` ratio inside a delegated CPU-quota'd cgroup
  subtree (host-load-independent), with a `quiet_box_met` skip fallback when delegation is unavailable. Reclassify
  the file **pool** in the H1 manifest. *Signal:* the 5 rows pass under concurrent load (or skip cleanly, never
  false-RED) via the confined quota; the proportional-share assertion still RED if governance is broken (non-vacuous;
  inherits the 0.65 bound — no new number). *Files:* `tests/infra/test_cpu_load_governance.sh` + the H1 manifest.
  *Depends:* H4 (same file; slice naming first).

- **H6 — cpu_governed_exec split.** Fixturize `A*`/`B1–B7`/`C*` (+ B8 host-gated detection read) into the pool via
  synthetic PSI / pure string-reads; **extract** the real-scope-placement `D*` residue into a sibling
  `test_cpu_governed_exec_hostexcl.sh` declared host-exclusive in the H1 manifest. **Depends on #4919** (which
  isolates D1–D6 to `$$`-scoped slices); does **not** duplicate the contamination fix. Exact per-row line (esp. B8)
  finalized against the drift-guard. *Signal:* the pool file runs `A*`/`B1–B7`/`C*` concurrently; the extracted
  host-exclusive file runs only on the hot path (knob=0) / cold lane (knob=1), and the drift-guard sees both files
  bucketed. *Files:* `tests/infra/test_cpu_governed_exec.sh`, `tests/infra/test_cpu_governed_exec_hostexcl.sh`, the
  H1 manifest. *Depends:* H1, **#4919** (cross-batch).

- **H7 — warm_lane_pool B11 → private FS.** Force B11's `df --output=avail` measurement onto a **private** loopback
  (require `REIFY_WARM_LANE_MOUNT` / provision via `provision-warm-lane-fs.sh`, bypassing the shared-`/tmp` rung of
  `detect_substrate`) so the delta measures private free space, immune to a concurrent disk-writer. Confirm the
  substrate-real blocks are declared host-exclusive. (This is the reflink/disk residue `infra-test-wallclock-deflake`
  **T8 explicitly handed off** — not wall-clock work.) *Signal:* B11's `df` delta ≤ the existing 50 MiB bound holds
  under a concurrent disk-writer because it measures the private mount; the substrate-real blocks are classified
  host-exclusive. *Files:* `tests/infra/test_warm_lane_pool.sh`, the H1 manifest. *Depends:* H1.

- **H8 — Lane-X host-exclusive flock primitive (shipped, not invoked).** A sourceable helper acquiring a single
  coarse host-exclusive lock at a fixed per-uid host path for the cold lane's single-flight run. *Signal:* the
  primitive acquires/releases the lock; a second concurrent acquire blocks / fails-fast; it is **not** wired into
  any `run_all.sh` path (Part B invokes it). **Consumer = Part B** (cross-PRD, §6). *Files:* a new `scripts/lib_*`.

- **H9 — `run_all.sh --scope host-infra` runner (the reify-local off-hot-path executor).** A run mode that runs
  **exactly** the declared host-exclusive set (the inverse of H3's exclusion), acquiring the **H8** Lane-X flock,
  preserving the `=== Summary: N discovered, M failed ===` line + bare `FAILED <names>` marker. This is the
  reify-local executable consumer of the host-exclusive bucket (the G1 consumer + the manual bridge during the
  Part-B window) **and** Part B's clean invocation target — mirroring offline-deep-test-lane's A5
  `run-offline-deep.sh`. *Signal:* with `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` unset, `bash tests/infra/run_all.sh
  --scope host-infra` runs only the host-exclusive files under the flock and reports pass/fail; a knob=1 hot-path
  run ⊕ a `--scope host-infra` run together cover the full universe (pool ⊕ serial ⊕ host-infra) exactly once.
  *Files:* `tests/infra/run_all.sh`. *Depends:* H1 (the host-exclusive set), H8 (the flock).

**Suggested edges:** H2→H1; H3→H1; H5→H4; H6→H1 (+ **#4919**); H7→H1; **H9→{H1, H8}**. H1 keystone; H4, H8 independent.
**Cross-project edges — DO NOT wire at this decompose** (Part B is authored but **not yet decomposed**; its task IDs
do not exist — wire from the Part-B side when it decomposes, per §11): Part B `flip REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1`
→ **H3**; Part B `invoke Lane-X flock` → **H8**; Part B `worker-extension: invoke run_all --scope host-infra` → **H9**.
The same-repo edge **H6 → task 4919** (the production-slice fix, already filed) **IS** wired now.

## 10. Open (tactical) questions

- **Pool concurrency `N`.** The `nproc`-derived formula (e.g. `max(1, nproc/2)` vs `nproc`) — pick a starting
  divisor; it is a knob, not frozen, and must stay `nproc`-relative (no baked constant). Measure and tune.
- **Manifest file layout.** One 3-valued manifest (`<test> <bucket>`) vs three separate declared lists
  (`host-exclusive-tests.txt` + `intra-run-serial-tests.txt` + a safe-light list). Either satisfies the
  full-coverage drift-guard; decided at H1 impl.
- **Extract-vs-in-file-gate for cpu_governed_exec `D*`.** Extraction into a sibling `_hostexcl.sh` keeps the
  manifest a clean filename set (preferred); an in-file env-gate is the alternative. Finalized at H6 against #4919's
  landed shape.
- **Does cpu_load_governance_deflake become pool-safe once H5 confines its SUT's burns?** It rides the SUT; if the
  confined quota bounds the inherited burns, it may move pool. Left host-exclusive by default (conservative); revisit
  after H5.
- **B8's bucket.** Host-gated detection *read* (places no scope) — likely pool; confirmed at H6 against the
  drift-guard, not pre-frozen here.
- **PSI soft-gate threshold for the pool.** Reuse `cpu-admit.sh`'s avg10 band vs a pool-specific ceiling; a knob,
  tuned under real concurrent-verify load.

## 11. Reconciliation addendum (post-Part-B-authoring, 2026-07-01)

Authored from a brief that predated Part B landing. These corrections + decisions are folded in for decompose and
**supersede** any "pending / NOT YET AUTHORED" text above.

- **Part B EXISTS.** `docs/prds/offline-deep-test-lane-worker.md` is author-complete & decompose-ready, and is
  **scoped to the numeric nextest suite** (its worker β2 invokes `run-offline-deep.sh`, not `run_all`). So the infra
  host-global residue does **not** ride Part B unchanged — see decision (a).
- **DECISION (a) — extend Part B's worker (NOT a separate lane).** Part B already builds the entire generic
  cold-lane engine (trigger, single-flight, always-from-head, dedup fix-task, escalate, never-a-gate, warm
  worktree). The infra residue reuses all of it via **one added Part-B worker leaf** that invokes reify's **H9**
  `run_all --scope host-infra` under the **H8** flock — not a duplicate lane. (Part B's warm-worktree/build-cone
  needs differ — the infra host-exclusive set does real cgroup/burn + synth-workspace cargo, not the solver+eval
  cone — a Part-B decompose detail.)
- **DECISION (b) — `cpu_governed_exec` D* stays host-exclusive** (as §3/H6 has it), NOT rescued to the pool.
  D1–D8/B8 need real cgroup **delegation** but do no burn and no ratio-under-contention; in the pool they'd **skip**
  on any non-delegated host (a hot-path coverage gap) and add real systemd `--user` scope-churn to **every** verify,
  whereas the cold lane gives them a **guaranteed-delegated** environment. So the cold lane is the *better* home,
  not merely the safer one. (`cpu_load_governance`'s pool-rescue is justified only by its confined quota, which both
  bounds footprint and makes the ratio host-independent — D* has no such lever.) A later measurement showing D* is
  trivially cheap in the pool is a one-line manifest reclassification.
- **H9 added (this addendum).** The `run_all.sh --scope host-infra` runner — the reify-local off-hot-path executor
  of the host-exclusive set + Part B's invocation target — was missing (the PRD shipped the exclusion knob + flock
  but no runner). offline-deep-test-lane deliberately ships the analog (A5 `run-offline-deep.sh`). Its G3 substrate
  (`run_all.sh` + H8 flock + H1 manifest) all exist; G6 introduces no numeric bound / no baked constant. **The
  capability manifest needs an H9 row** (derive at decompose).
- **CROSS-PROJECT EDGE SEQUENCING (per DA2 + both §6 Sequencing clauses).** Part B is authored but **not yet
  decomposed** — its task IDs don't exist. At this reify decompose: wire all reify-internal edges (H2/H3/H6/H7→H1;
  H5→H4; H9→{H1,H8}) **and** the same-repo edge **H6 → task 4919** (the production-slice fix, already filed). Leave
  the three **cross-project** edges — Part B flip → **H3**, Part B flock-invocation → **H8**, Part B
  worker-extension → **H9** — as **documented follow-ups** (recorded in task metadata + here), to be wired from the
  Part-B side when it decomposes.
