# Capability manifest — `run-all-host-infra-partition` (Part A)

Mechanizes G3 + G6 per leaf for `docs/prds/run-all-host-infra-partition.md`. Each binding ties a leaf's
asserted capability to **evidence** (grep/command/file:line). Any **FAIL** binding blocks queueing until
resolved. Verified against main @ `255e36b7ad`, 2026-07-01.

**Domain notes.** This PRD is **shell/test-partition/concurrency-control infra** — no `.ri` syntax, no
result fields — so the reify field-population sentinel (`Value::Undef`) and grammar-fixture checks are
**N/A by construction** (recorded per binding, not silently skipped). The live G6 surface is narrow and
entirely **anti-regression**: (a) no host-baked constant (H2 `N` from `nproc`), (b) no new wall-clock
upper bound (H2/H5 poll budgets via `load_tolerant_attempts`), (c) confined-quota **scale-invariance** so
the rescued rows **inherit** the already-green `0.65` share bound (H5 — no new number), (d) B11's `50 MiB`
df-delta is **inherited**, only re-sited (H7). No leaf authors a new numeric assertion.

---

## H1 — classification manifest + drift-guard meta-test

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `tests/infra/` harness (`test_helpers.sh`) + registry `scripts/verify-pipeline-infra-tests.txt` (47 lines) exist. Drift-guard precedent: `scripts/occt-scope-lib.sh` `occt_declared_set()` (reads `occt-touching-crates.txt`) + `tests/infra/test_occt_gated_scope.sh` (declared==derived, `test -z "$_DIFF_OUT"`); `scripts/release-scope-lib.sh` + `tests/infra/test_release_scoped_scope.sh`. |
| **Executable classification (anti-tabulation)** | **PASS — this leaf's whole job** | The guard **runs** `ls test_*.sh` and diffs against the declared union; a new/unclassified test → RED (ratified #2). Not a tabulated promise. |
| **Anti-orphan / wired** | **PASS** | Consumed within Part A by H2 (reads intra-run-serial set), H3 (reads host-exclusive set), H6/H7 (add entries); registered in `verify-pipeline-infra-tests.txt` so the pipeline runs it. |
| Grammar-fixture / field-population | **N/A** | No `.ri` syntax, no result fields. |

## H2 — concurrent hermetic pool in `run_all.sh`

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `scripts/lib_slot_acquire.sh` `slot_acquire LOCK_BASE N WAIT` (N a runtime arg; FD-9 hold; per-uid host path) ✔; `scripts/cpu-admit.sh` `cpu_admit_read_avg10()` (`:93`, `/proc/pressure/cpu`) ✔; `run_all.sh` output contract present (`:76` Summary, `:79` bare `FAILED %s`). |
| **No host-baked constant (G6 anti-#4901)** | **PASS** | `N` derived from `nproc` at runtime; `lib_slot_acquire.sh` takes `N` as an argument — **no frozen integer**. Precedent this defends: occt-cap=24 deterministically false-blocked the 16-core laptop merge gate (#4901). |
| **No new wall-clock upper bound (G6 anti-regression)** | **PASS** | New poll budgets use `load_tolerant_attempts` (MAX-clamped, `load_tolerance_lib.sh:177`) so `tests/infra/test_no_new_wallclock_upper_bounds.sh` (the T9 standing guard) stays green. No wall-clock assertion added. |
| **Output-contract preservation (negative-assertion)** | **PASS (by signal)** | The H2 signal is a **fault-injected** failing test still appearing in the bare `FAILED <names>` marker under concurrency — observed, not promised. A dropped/garbled marker reclassifies a real fail as `tree_sitter_generate_error` (thrash-escalating L1) — the exact hazard `run_all.sh:16-23` documents. |
| Grammar-fixture / field-population | **N/A** | Infra. |

## H3 — off-by-default `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` exclusion seam

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `run_all.sh` already loops over discovered `test_*.sh` (`:53`); filtering by a declared set behind an env check is a local addition. Env-var read makes the knob settable from the orchestrator verify env with no reify code change (the flip-seam contract, PRD §6). `REIFY_RUN_ALL_EXCLUDE_HOST_INFRA` is confirmed **absent** from the tree (this leaf introduces it). |
| **Additive-default (anti-regression)** | **PASS (by signal)** | Default `0` ⇒ discovered/run set unchanged; the H3 signal asserts knob-off = full set, knob-on = full minus host-exclusive with the `Summary` count dropping by exactly the excluded count. Strictly-additive-on-landing (§8) observed, not assumed. |
| **Negative-assertion (strict `1`)** | **PASS (by signal)** | Any value other than exactly `1` (unset/empty/`0`/garbage) runs the full set — a silent-accept of a malformed knob that *excluded* host-infra would be a coverage hole. |
| **Anti-orphan / wired (cross-repo, tracked)** | **PASS** | The `1` value's consumer is Part B's flip task (orchestrator verify env), wired via a real cross-project `add_dependency` edge at decompose (PRD §6, DA2). Named owner = dark-factory `offline-deep-test-lane-worker.md` → not an unclaimed orphan. Same accepted seam class as the sibling `REIFY_GATE_EXCLUDE_HEAVY`. |

## H4 — cpu_load_governance slice-naming `$$`-in-PREFIX fix

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | Current `_ROW4_SLICE_TASK="reify-govtest-agents.slice"` / `_MERGE="reify-govtest-merge.slice"` at `test_cpu_load_governance.sh:605-606`, nesting under shared `reify-govtest.slice` (`:549-550`). Fed through `REIFY_CPU_GOVERN_SLICE_TASK/_MERGE`, read in `lib_cgroup.sh:85/88` — **no `lib_cgroup.sh` change** needed (test sets the env). |
| **Correctness premise (prefix, not trailing)** | **PASS** | systemd derives a slice's parent by stripping the last `-`-segment: `reify-govtest$$-agents.slice` → parent `reify-govtest$$.slice`. Putting `$$` in the parent-defining `govtest$$` segment gives each concurrent run a **unique parent** while keeping the two children **siblings** (the C-G2 weight-ratio invariant, task #4632). Trailing `$$` would leave the shared parent `reify-govtest.slice` cross-run → collision. |
| **Anti-orphan / wired** | **PASS** | Consumed by H5 (confined ROW4 measurement) + H2 pool (concurrent runs of the file). |
| Numeric floor / grammar | **N/A** | Renaming asserts no numeric bound; infra. |

## H5 — cpu_load_governance pool-safe conversion (confined-quota + synthetic-PSI)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | Synthetic-PSI fixtures already used in-file (`REIFY_CPU_GOV_TEST_PROC_PATH`, `REIFY_CPU_ADMIT_PROC_PATH` at ROW4-BYPASS); `quiet_box_met AVG10 CEILING` (pure, `load_tolerance_lib.sh:146`); confined cgroup subtree via the delegated substrate the D-cycle already proves present. |
| **Numeric floor (G6) — inherited, not new** | **PASS — bound NOT re-authored** | Confined `cpu.weight` share is **quota-scale-invariant** (children split the parent's budget by weight regardless of the parent's quota), so the confined 2-core measurement reproduces the same ratio as the full box. H5 **inherits** ROW4-1's already-green, already-de-flaked `merge_share ≥ W_merge/(W_merge+W_task) − tol = 0.65` bound (#4634/#4656). **Do NOT** re-tighten or re-derive it (that re-opens the #4656 flake class). |
| **Non-vacuous (negative-assertion)** | **PASS (by signal)** | The confined proportional-share assertion must still go **RED** if governance is broken (under a quiet/delegated box); the `quiet_box_met` skip fires **only** when cgroup delegation is unavailable — never a blanket pass. |
| **No new wall-clock bound** | **PASS** | Any confined-window poll uses `load_tolerant_attempts`; the `_LIVE_BUDGET_S` anti-hang guard is already generous/skip-gated (#4846). |
| Field-population / grammar | **N/A** | Infra. |

## H6 — cpu_governed_exec split (fixturize `A*`/`B1–B7`/`C*`; extract `D*` residue)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `A*`/`B1–B7`/`C*` hermetic (pure lib asserts / fail-open); D7/D8 already use `$$`-scoped isolated slices (`test_cpu_governed_exec.sh:36-37`); B8 (`:191`) is a host-gated detection **read** (places no scope). The extracted host-exclusive file is a new `test_*.sh` the H1 drift-guard buckets. |
| **Dependency named — NOT duplicated (§7)** | **PASS** | The D1–D6 **production-slice contamination** (scopes under `reify-governed.slice/…`, `:266/283`) is **task #4919** (in-progress), not this leaf. H6 **depends on #4919** (cross-batch `add_dependency`) and only fixturizes/extracts — it does not re-implement the isolation. |
| **Anti-orphan / wired** | **PASS** | Pool remainder consumed by H2; the extracted `_hostexcl.sh` consumed by H1 (classification) + Part B (cold lane runs it). |
| Numeric floor / grammar | **N/A** | Placement/config asserts; infra. |

## H7 — warm_lane_pool B11 → private FS

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `provision-warm-lane-fs.sh` builds a private XFS-reflink loopback (`mkfs.xfs -m reflink=1` `:245`, `REIFY_WARM_LANE_MOUNT` `:98`); `detect_substrate()` ladder (`test_warm_lane_pool.sh:188`) confirms the shared-`/tmp` rung (rung 2) currently can win over the private loopback (rung 3, gated on `REIFY_RUN_WARM_LANE_GATE=1`) — the exact hole this leaf closes for B11. |
| **Inherited numeric bound (G6) — re-sited, not re-authored** | **PASS** | B11's `df --output=avail` deltas (`:463/473`) → the `≤50 MiB` assertion (`:1763`). H7 changes **where** the df is measured (private mount, so a concurrent disk-writer can't dilute it), **not** the bound. No new number. |
| **Provenance — T8 handoff, not wall-clock** | **PASS** | This is the reflink/disk residue `infra-test-wallclock-deflake.md` **T8 explicitly handed off** to the warm-lane owner (§5.3 / T8 note: "out-of-class reflink/disk residue handed to the warm-lane owner") — not wall-clock work, so it does not re-open the deflake PRD. |
| **Anti-orphan / wired** | **PASS** | B11 robustness consumed by H2 pool / hot path; substrate-real blocks classified host-exclusive (H1) → cold lane (Part B). |

## H8 — Lane-X host-exclusive flock primitive (shipped, not invoked)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `flock` + fixed per-uid host-path idiom already proven in `lib_slot_acquire.sh` / the test semaphore; H8 is a coarse single-slot variant. |
| **Anti-orphan / wired (cross-repo, tracked)** | **PASS (cross-PRD consumer named)** | No reify-local consumer **by design** — it is a shipped primitive whose consumer is Part B's flock **invocation** (single-flight cold-lane run), wired via a real cross-project `add_dependency` edge at decompose (PRD §6). Same accepted seam class as the offline-deep-test-lane knob leaf (reify ships the primitive; DF invokes it). The seam has a **named owner** → not an unclaimed orphan. |
| **Not wired into any blocking path (negative-assertion)** | **PASS (by signal)** | The H8 signal asserts the primitive is **not** invoked by any `run_all.sh` path — Part A ships it inert; only Part B invokes it. |
| Numeric floor / grammar | **N/A** | Lock primitive; infra. |

## H9 — `run_all.sh --scope host-infra` runner (reify-local off-hot-path executor)

*(Added in the §11 reconciliation addendum — the PRD shipped the exclusion knob + flock but no runner; offline-deep-test-lane ships the analog A5 `run-offline-deep.sh`.)*

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `run_all.sh` discovery loop (`:53`) + output contract (`:76` `=== Summary:` line, bare `FAILED` marker `:17-18`) present on HEAD ✔. The **Lane-X flock** it acquires is H8's primitive and the **host-exclusive set** it runs is H1's manifest — both **intra-batch deps** (H9 → {H1, H8}), not missing external substrate. Structural analog: offline-deep-test-lane A5 `run-offline-deep.sh`. |
| **Partition completeness (executable, negative-assertion)** | **PASS (by signal)** | The H9 signal asserts a knob=1 hot-path run ⊕ a `--scope host-infra` run together cover the full universe (pool ⊕ serial ⊕ host-infra) **exactly once** — observed against the H1 drift-guard, not tabulated. `--scope host-infra` is the **inverse** of H3's exclusion (runs exactly the declared host-exclusive set). |
| **No numeric bound / no baked constant (G6)** | **PASS** | The runner selects by the declared host-exclusive set (H1) and acquires H8's flock; it asserts **no number** and freezes **no constant** (G6 surface empty). |
| **Anti-orphan / wired** | **PASS** | Reify-local executable consumer of the host-exclusive bucket (the G1 consumer + the manual bridge during the Part-B window); **also** Part B's clean invocation target — cross-project edge → **H9** (§6/§11, wired from the Part-B side when Part B decomposes). |
| Grammar-fixture / field-population | **N/A** | Infra. |

---

## Summary

| Leaf | Blocking verdict |
|---|---|
| H1 classification + drift-guard | **PASS** (executable, not tabulated) |
| H2 concurrent pool | **PASS** (`N` from `nproc` — no baked constant; output contract observed under fault injection) |
| H3 exclusion seam | **PASS** (default-additive; strict-`1`; cross-repo consumer named & dep-edged) |
| H4 slice-naming `$$`-prefix | **PASS** (prefix-segment correctness premise sound) |
| H5 pool-safe conversion | **PASS** (confined-quota scale-invariant → inherits the green 0.65 bound; non-vacuous) |
| H6 cpu_governed_exec split | **PASS** (depends on #4919; does not duplicate the contamination fix) |
| H7 warm_lane B11 → private FS | **PASS** (inherited 50 MiB bound re-sited; T8 handoff) |
| H8 Lane-X flock primitive | **PASS** (cross-PRD consumer named; inert in Part A) |
| H9 `run_all --scope host-infra` runner | **PASS** (G3 substrate on HEAD; deps H1+H8 intra-batch; no numeric bound / no baked constant) |

**No FAIL bindings. Batch is clear to queue.** The bindings above are shell/infra premises verified
executably by direct `file:line` grep against HEAD (the appropriate "execute, don't tabulate" check for a
PRD with **no `.ri` grammar / semantic premises** — the `.ri`-probe D3 workflow `prd-decompose-verify.mjs`
is a category mismatch here and is intentionally not run). At decompose, wire the **same-repo** edge
**#4919 → H6** (the production-slice fix, already filed) **now**; leave the **three cross-project** edges —
Part B flip → **H3**, Part B flock-invocation → **H8**, Part B worker-extension → **H9** — as **documented
follow-ups** (recorded in task metadata + PRD §11), to be wired from the Part-B side when it decomposes.
The G6 surface is entirely anti-regression (no baked constant, no new wall-clock bound, both numeric bounds
inherited) — the manifest flags each so decompose does not re-author a frozen constant or re-tighten an
inherited bound.
