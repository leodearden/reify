# PRD — Offline deep-test lane, Part A (reify-local test partition + `offline` role)

**Status:** author-complete, gates passed (2026-07-01). Decompose-ready.
**Slug:** `offline-deep-test-lane` · **Milestone:** version-agnostic verify-pipeline infra (root `docs/prds/`).
**Authoritative design:** `docs/design/offline-deep-test-lane.md` (ratified D1–D5, 2026-06-09).
**Companion baseline:** `docs/notes/warmer-builds-phase0-baseline.md`; `docs/design/warmer-builds-merge-verify.md`.

This PRD is **Part A** of the two-PRD decomposition the design §12 names: the **reify-local,
independently-shippable** slice. Part B (the dark-factory async lane worker + trigger + failure
handling + the gate flip) is a separate PRD owned by dark-factory — see §6.

---

## 0. Scope & consumers (G1)

**What Part A introduces (all mechanisms have a named consumer *within Part A* — no orphan producers):**

| Mechanism | Consumer (in Part A) | Downstream consumer (Part B) |
|---|---|---|
| `heavy` nextest filter expression (single source of truth) | the `offline` role selects it (`+ --run-ignored all`); the gate-exclusion knob negates it | — |
| `DF_VERIFY_ROLE=offline` role in `scripts/verify.sh` | **`scripts/run-offline-deep.sh`** (reify-local one-shot runner) executes it | the DF singleton lane worker invokes `run-offline-deep.sh` |
| `scripts/run-offline-deep.sh` one-shot runner | operator / local timer (manual bridge) | the DF lane worker's `subprocess` entry point |
| thin gate smoke binary (`solver_gate_smoke.rs`) | the merge/task gate runs it (it is outside the `heavy` pattern) | — |
| `REIFY_GATE_EXCLUDE_HEAVY` knob (default **0** = current behavior) | verify.sh gate roles read it; **default keeps the heavy set on the gate** | Part B flips it to `1` (orchestrator.yaml verify env) the moment the lane is live |
| `tests/infra/test_verify_offline_partition.sh` drift-guard | the verify pipeline (infra step) | — |

**User-observable surface (the reify-local leaf signals, G2):**
- `DF_VERIFY_ROLE=offline ./scripts/verify.sh test --print-plan` emits a plan that runs **exactly** the
  `heavy` filter **+ `--run-ignored all`**, at idle scheduling class, single (release) profile.
- `./scripts/run-offline-deep.sh` **executes** that plan (heavy set + ignored convergence studies) off
  the merge hot path — a real, runnable reify-local consumer, not just a printed plan.
- With the default knob (`REIFY_GATE_EXCLUDE_HEAVY` unset/0), the merge/task gate `--print-plan` is
  **unchanged** (heavy set still runs on the gate) **plus** the new thin smoke — i.e. Part A is
  strictly additive; it removes no coverage.
- With `REIFY_GATE_EXCLUDE_HEAVY=1`, the gate `--print-plan` runs `not (heavy)` — proving the flip
  seam works — and the drift-guard asserts the offline set ⊕ gate-smoke set partition the heavy
  universe with **no overlap and no orphan** (nothing runs nowhere).

## 1. Problem & premise (G6 record)

Phase 0 (`docs/notes/warmer-builds-phase0-baseline.md`, measured 2026-06-09 on an idle box) found a
*warm* merge-gate verify ≈ 11 min, of which **~643 s is test-exec that warmth cannot touch** (compile
collapses to ~9 s under the warm worktree; the exec floor does not move). That floor is
**tail-latency-bound**: each nextest pass clears ~11k fast tests in the first ~30–60 s, then spends
60–120 s on a handful of long numeric tests with most cores idle. The long poles are all numeric
(`reify-solver-elastic` `determinism::*` thread-count sweeps — one release test `SLOW [>120 s]` alone
nearly spans the pass; `analytical_validation` P2 + the `#[ignore]`'d convergence studies; `modal_benchmarks`;
`buckling_smoke`; two heavy `reify-eval` OCCT FEA binaries ≈ 113 s + 95 s). They add little
**marginal gate coverage** (a full parallel-tolerance thread sweep / analytical convergence study is
not *delta*-shaped) and the heavy set that runs on the gate **runs in both the debug and release
passes — paid ~twice** (`verify.sh` merge default is `--profile both` → `PROFILES=(debug release)`).

### G6 premise re-check vs HEAD (main @ `0113758b11`, 2026-07-01) — the gate you must not rubber-stamp

The design was written *before* **LPT nextest scheduling landed** (task #4627, `.config/nextest.toml`
priority overrides). LPT reorders the heavy binaries to start first so they overlap the tail, rather
than dangling off it (it was added to keep `tensegrity_t0a`'s >180 s straggler from pushing the
`--include-infra` makespan past the 1800 s wall, esc-4536-63). **Re-validated empirically against
current HEAD — the premise still holds:**

1. **The floor was *measured*, not asserted** (Phase-0 table: ~643 s warm exec floor; "These run in
   **both** debug and release passes (paid ~twice)"). The double-pay is a structural property of
   `--profile both`, which LPT does not touch.
2. **LPT cannot shrink a single test that spans a whole pass.** `default_parallel_tolerance_equivalent_across_thread_counts`
   is `SLOW [>120 s]` and "alone nearly spans the release pass" — starting it at t=0 (LPT) makes the
   makespan ≈ that test's own duration. Ordering is irrelevant to a critical-path singleton.
3. **The LPT overrides target the *same* heavy binaries** (`tensegrity_t0a`, `fea_diagnostics_e2e`,
   `analytical_validation`, `determinism`) — living code-evidence they are *still* the recognized long
   poles post-LPT. The heavy tests, the `#[ignore]`'d convergence studies, and the release-only
   `#[cfg_attr(debug_assertions, ignore)]` modal/buckling gates are all still present and shaped
   exactly as the design describes (verified this session).

**Conclusion:** LPT and the offline lane are **complementary** — LPT optimizes the makespan of what
*stays* on the gate; the offline lane removes the removable floor + the double-pay that LPT cannot.
The PRD is **not dead work**. (A precise warm-gate re-measurement of the exact seconds saved is a
tactical open question — §10 — not a blocker, and is naturally captured by Part A's own drift-guard
plus a Phase-0-style timing once the partition exists.)

## 2. Goal & non-goals

**Goal (Part A).** Ship the reify-local **mechanism** that lets the heavy numeric suite be run
**off the merge hot path** — the `heavy`/`not heavy` test partition, the `DF_VERIFY_ROLE=offline`
role, a thin gate smoke, a one-shot runner, and the off-by-default flip seam — such that:
- nothing changes on the merge gate on landing (strictly additive; `REIFY_GATE_EXCLUDE_HEAVY=0`);
- the heavy set is immediately **runnable off-gate** via `run-offline-deep.sh`;
- Part B can make the gate faster by flipping **one env knob**, immediately and reversibly, the
  instant the async lane is live (wired via a cross-project dependency edge, §6).

**Non-goals (Part A).**
- **Not** the async lane worker, the `on_post_merge` trigger, single-flight/coalescing, dedup'd
  fix-task spawn, or `escalate_info`/`escalate_blocker` staging — **all Part B** (dark-factory).
- **Not** the second persistent warm worktree instantiation — Part B (reuses warmer-builds Phase-1
  machinery).
- **Not** the gate flip itself — Part A ships the *seam* (off-by-default knob); Part B *pulls* it.
- **Not** a lean build profile — deferred (design §8/§10 "optional"), tracked as a follow-up.
- **Never a gate.** The offline lane never blocks a merge (design D1/§11) — but that invariant is
  enforced by Part B's async worker; Part A's role is simply *not wired into* any blocking path.

## 3. The test partition

**Moves to the `heavy` set (run offline, once, in the release profile that matters — `--run-ignored all`):**
`reify-solver-elastic` `determinism` thread-count sweeps; `analytical_validation` (P2 validation **and**
the `#[ignore]`'d `cantilever_faithful_convergence_study` / `cylinder_lame_convergence_study`);
`modal_benchmarks`; the heavy `reify-eval` OCCT FEA binaries (`buckling_smoke`, `tensegrity_t0a`,
`fea_diagnostics_e2e` — the ≈113 s + ≈95 s serial-OCCT poles; exact membership finalized at decompose
against measured timings and the drift-guard).

**Stays on the gate — the thin smoke (a NEW, dedicated, lighter binary):** determinism at **1-vs-2
threads only**, **one** analytical benchmark at **coarse** tolerance, one profile. Enough to catch a
gross regression in the merge delta synchronously; cheap enough to leave on the hot path.

**Mechanism — a nextest *filter*, not `#[ignore]`.** A binary-level nextest filter expression
(auditable; each atom resolves to a real `crates/<pkg>/tests/<bin>.rs` — extends the existing
`tests/infra/test_nextest_slow_priority.sh` "resolve-to-disk" drift-guard pattern). Two views over the
same set, both driven by `scripts/verify.sh`:
- **offline** (`DF_VERIFY_ROLE=offline`): the `heavy` filter **+ `--run-ignored all`** (picks up the
  convergence studies), release profile, idle class.
- **gate** (`task`/`merge`): `not (heavy)` **only when `REIFY_GATE_EXCLUDE_HEAVY=1`**; otherwise the
  gate is unchanged (full set + the new smoke). The smoke runs under `not (heavy)` because it lives in
  a separate binary outside the `heavy` pattern.

A filter keeps the heavy tests **visible and runnable locally** (unlike `#[ignore]`, which hides them)
and **auditable** — the drift-guard lists exactly what is deferred.

## 4. Ratified decisions

**Imported from the design (parent decisions; D2/D4/D5 are Part-B-realized):**

| # | Decision |
|---|---|
| **D1** | **Tier, don't remove.** A thin solver smoke stays on the merge gate; the full matrix moves offline. |
| **D2** | (Part B) Trigger on main-advance; single-flight; always-from-head. |
| **D3** | **Footprint = idle scheduling class (`nice -n 19 ionice -c3`) + `--test-threads=N` cap, off the merge jobserver.** Not a hard 1-CPU pin. |
| **D4** | (Part B) Failure handling = confirmation re-run → dedup → normal `pending` fix task + `escalate_info`; `escalate_blocker` only on stall. |
| **D5** | (Part B) Warm build = dedicated self-warming worktree reusing Phase-1 machinery. |

**Part-A-specific decisions (resolved this session):**

- **DA1 — Part A is strictly additive; the gate flip is deferred to Part B.** Part A lands the
  filter + `offline` role + thin smoke + runner + drift-guard + flip seam. `REIFY_GATE_EXCLUDE_HEAVY`
  defaults to `0`, so the gate keeps running the full heavy set on landing. **Zero coverage change,
  zero coverage gap** — the heavy set never runs *nowhere*. (Chosen over "flip in Part A + accept a
  bounded gap": the whole point of this skill is to not let coverage silently erode while a consumer
  is pending.)
- **DA2 — The flip is an off-by-default env knob, pulled by Part B via a cross-project dep, immediate.**
  `scripts/verify.sh` gate roles read `REIFY_GATE_EXCLUDE_HEAVY` (default `0`). Part B sets it to `1`
  in `orchestrator.yaml`'s verify env the moment the async lane is live — a one-line, immediate,
  reversible config deploy, **not** a reify code change. A real `add_dependency` edge makes Part B's
  `flip-gate-exclude-heavy` task depend on **both** Part A's knob leaf **and** Part B's lane-live leaf,
  so the flip fires exactly when the lane can catch the offline runs (no lingering double-pay, no gap).
- **DA3 — `DF_VERIFY_ROLE=offline` selection & footprint.** Symmetric with `task`/`merge`
  (`verify.sh` role `case`): idle class `nice -n 19 ionice -c3` (SCHED_IDLE, design §6), single
  **release** profile, `heavy` filter + `--run-ignored all`, **off the merge jobserver**
  (`CARGO_MAKEFLAGS` left unset — the offline role draws from neither the task nor the merge FIFO).
  The role's `--print-plan` is the primary leaf signal. Update the `want task|merge` error message to
  `want task|merge|offline`.
- **DA4 — Binary-level `heavy` filter + dedicated smoke binary + resolve-to-disk drift-guard.** The
  `heavy` set is a binary-level expression (robust vs test-name-regex; each atom must resolve to a
  file on disk). The thin smoke lives in a **new** binary (`crates/reify-solver-elastic/tests/solver_gate_smoke.rs`)
  outside the `heavy` pattern, so no `heavy`-binary membership can accidentally capture it.

## 5. Pre-conditions / substrate (G3 — all verified present this session)

- **`.config/nextest.toml` exists** with the `occt` test-group + the #4627 LPT priority overrides;
  nextest 0.9.x supports binary/test filter expressions and `--run-ignored all`. ✔
- **`scripts/verify.sh` role dispatch** (`DF_VERIFY_ROLE` `case`, ~line 414; error at ~432) knows only
  `task|merge` today — the `offline` arm is a clean addition. ✔
- **The heavy tests all exist and are shaped as the design describes** — `determinism.rs`
  (`default_parallel_tolerance_equivalent_across_thread_counts` + thread-sweep bit-stability tests),
  `analytical_validation.rs` (P2 tests + `#[ignore = "convergence study; run explicitly with --ignored"]`
  on `cantilever_faithful_convergence_study` @832 / `cylinder_lame_convergence_study` @1367),
  `modal_benchmarks.rs` (`cfg_attr(debug_assertions, ignore)` release-gate), `buckling_smoke.rs`
  (`#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]` @75/148/305/425),
  the `reify-eval` OCCT FEA binaries (`tensegrity_t0a.rs`, `fea_diagnostics_e2e.rs`, …). ✔
- **Drift-guard precedent exists** — `tests/infra/test_nextest_slow_priority.sh` already resolves each
  nextest filter atom to `crates/<pkg>/tests/<bin>.rs`; the partition guard extends this pattern. ✔
- **No novel `.ri` grammar** — this PRD is shell/config/test infra; the G3 grammar gate is trivially
  satisfied (no new syntax). ✔
- **No warm-worktree dependency for Part A.** The filter/role/smoke/runner/guard run anywhere; only
  Part B needs the (already-live) warm-lane CoW pool. So Part A is genuinely independently shippable
  with no infra prerequisites. ✔

## 6. Cross-PRD relationship + seam ownership (G4)

Two PRDs, split across repos — the same cross-repo seam class as cpu-governance (α/β/γ ↔ ζ) and the
warm-lane D8 seam in `CLAUDE.md`: **reify ships the primitives, dark-factory wires the consumer.**

| Deliverable | Owner | Depends on |
|---|---|---|
| `heavy` filter, `offline` role, thin smoke, `run-offline-deep.sh`, `REIFY_GATE_EXCLUDE_HEAVY` seam, partition drift-guard | **reify (Part A — this PRD)** | existing nextest + verify.sh only |
| `on_post_merge` trigger (`harness.py`/`merge_queue.py`) | **dark-factory (Part B)** | Part A |
| singleton lane worker: single-flight / coalesce / always-from-head (`workflow.py`) | **dark-factory (Part B)** | Part A; warmer-builds Phase-1 (warm-lane pool — **live**, task ε #4663) |
| dedup'd fix-task spawn (failing-test-set signature) + `escalate_info`/`escalate_blocker` staging | **dark-factory (Part B)** | Part A |
| second persistent-worktree instantiation (`_offline-deep`, Phase-1 machinery, `git_ops.py`) | **dark-factory (Part B)** | warm-lane pool (live) |
| **`flip-gate-exclude-heavy`** — set `REIFY_GATE_EXCLUDE_HEAVY=1` in `orchestrator.yaml` verify env | **dark-factory (Part B)** | **cross-project edge → Part A knob leaf** + Part B lane-live leaf |

**The flip seam contract (the one interface both PRDs must agree on):**
> `scripts/verify.sh`, on role `task`/`merge`, applies the nextest filter `not (heavy)` **iff**
> `REIFY_GATE_EXCLUDE_HEAVY` is exactly `1`; for any other value (unset/empty/0) the gate runs the
> full set unchanged. The variable is read from the environment so `orchestrator.yaml`'s verify env
> can set it without a reify code change. Flipping it is immediate and reversible.

**Ownership is unambiguous — no reciprocal "the other owns it" pattern.** Part A owns the seam + the
default (`0`); Part B owns the pull (`1`) *and* the async lane that makes the pull safe. The
cross-project dependency edge is wired at decompose time (Part B's flip task deps-on Part A's knob
leaf), per the user directive to "make the flip immediate."

## 7. Out of scope (Part A)

- The entire dark-factory async lane (trigger, worker, single-flight, dedup, fix-spawn, escalation) — Part B.
- The gate flip itself (`REIFY_GATE_EXCLUDE_HEAVY=1`) — Part B pulls the seam.
- A dedicated lean build profile for the lane (design §8/§10 "optional") — deferred follow-up.
- Re-measuring the exact warm-gate seconds saved — tactical (§10); does not gate this PRD.
- Fixing any test that turns out RED when a currently-`#[ignore]`'d convergence study is first run
  first-class offline — surfaced as a finding (a normal fix task), **not** a Part-A blocker; the lane
  is non-blocking by design (D1).

## 8. Invariants / do-nots

- **Additive only on landing.** `REIFY_GATE_EXCLUDE_HEAVY` defaults to `0`; Part A must not change
  what the gate runs (beyond adding the cheap smoke). No heavy test may run *nowhere* at any point.
- **Partition completeness.** The heavy set (offline) ⊕ the gate-smoke set must have **no overlap and
  no orphan** — every heavy test runs offline; the smoke is a distinct lighter binary. The drift-guard
  enforces this executably (not a tabulated promise).
- **Off the merge jobserver.** The `offline` role must never draw from `/tmp/reify-jobserver-*`
  (priority-blind admission) — `CARGO_MAKEFLAGS` unset.
- **Idle class.** The `offline` role runs at `nice -n 19 ionice -c3`; it must yield completely to any
  normal-class thread.
- **Keep the gate smoke.** Do not pull *all* solver coverage off the gate — gross regressions must
  still fail synchronously with commit-level attribution (holds trivially in Part A since the flip is
  Part B's; the smoke is authored here).
- **Resolve-to-disk filters.** Every `heavy` filter atom must resolve to a real
  `crates/<pkg>/tests/<bin>.rs` (drift-guard assertion) — a typo'd filter silently matching nothing is
  a coverage hole.

## 9. Decomposition plan (leaf tasks — each names a user-observable signal, G2)

> All leaves are reify-local, verifiable without any orchestrator wiring. `metadata.files` follows the
> tight-or-empty rule (name a file only on a high-confidence anchor; `[]` otherwise).

- **A1 — `heavy` nextest filter (single source of truth) + resolve-to-disk drift atoms.** Define the
  binary-level `heavy` expression consumed by both views. *Signal:* the expression is committed and
  each atom resolves to a real `crates/<pkg>/tests/<bin>.rs`; asserted by A6. *Files:* `.config/nextest.toml`
  (and/or a `verify.sh` filter constant — decided at impl). `grammar_confirmed: true` (no `.ri`).
- **A2 — `DF_VERIFY_ROLE=offline` role in `scripts/verify.sh`.** Add the role `case` arm (idle class,
  release profile, `heavy` + `--run-ignored all`, jobserver-detached); update the `want task|merge`
  error to `want task|merge|offline`. *Signal:* `DF_VERIFY_ROLE=offline ./scripts/verify.sh test
  --print-plan` emits a plan running exactly the `heavy` filter + `--run-ignored all` at
  `nice -n 19 ionice -c3`, release only. *Files:* `scripts/verify.sh`.
- **A3 — thin gate smoke binary.** New `crates/reify-solver-elastic/tests/solver_gate_smoke.rs`:
  determinism 1-vs-2 threads (exact bit-stability — no numeric floor), one analytical benchmark at a
  **coarse tolerance pinned to an already-passing bound** (e.g. the existing `_within_5pct_` cantilever
  P1 tolerance — G6: above the P1-tet bending-lock floor). *Signal:* the smoke binary compiles and its
  tests pass on the gate under `not (heavy)`; visible in `--print-plan` / `nextest list`. *Files:*
  `crates/reify-solver-elastic/tests/solver_gate_smoke.rs`.
- **A4 — `REIFY_GATE_EXCLUDE_HEAVY` knob-gated gate exclusion (default 0).** Gate roles apply
  `not (heavy)` iff the knob is exactly `1`. *Signal:* knob unset/0 → gate `--print-plan` unchanged
  (heavy still runs); knob=1 → gate `--print-plan` runs `not (heavy)`. **This is the cross-project
  flip seam** Part B's flip task depends on. *Files:* `scripts/verify.sh`.
- **A5 — `scripts/run-offline-deep.sh` one-shot runner.** Thin wrapper: `DF_VERIFY_ROLE=offline
  ./scripts/verify.sh test …` (release, heavy + ignored). The reify-local executable consumer of the
  `offline` role (G1) and the manual bridge during the Part-B window. *Signal:* running it executes
  the heavy set + ignored studies off-gate at idle priority and reports pass/fail. *Files:*
  `scripts/run-offline-deep.sh`.
- **A6 — `tests/infra/test_verify_offline_partition.sh` drift-guard + registry row.** Asserts: (a)
  offline plan = `heavy` + `--run-ignored all`; (b) knob=1 gate plan = `not (heavy)`, no heavy leak;
  (c) knob=0 (default) gate plan unchanged; (d) heavy ⊕ smoke partition, no overlap/orphan; (e) each
  `heavy` atom resolves to disk. Register in `scripts/verify-pipeline-infra-tests.txt`. *Signal:* the
  infra test runs green in the verify pipeline and fails on a deliberately broken partition. *Files:*
  `tests/infra/test_verify_offline_partition.sh`, `scripts/verify-pipeline-infra-tests.txt`.

**Suggested edges:** A2→A1; A4→A1; A5→A2; A6→{A1,A2,A3,A4}. A3 independent. (Finalized at decompose.)
**Cross-project edge (wired at decompose, per user directive):** Part B `flip-gate-exclude-heavy`
→ **A4** (and → Part B lane-live leaf).

## 10. Open (tactical) questions

- **Exact `heavy` membership.** Finalize the binary list against fresh measured timings (which
  `reify-eval` OCCT binaries are the ≈113 s + ≈95 s poles — `tensegrity_t0a` + `fea_diagnostics_e2e`
  are the leading candidates from the LPT set). A6 makes whatever is chosen auditable.
- **`--test-threads=N` default for the `offline` role.** Design §6: start modest (not 1), measure,
  tune; N balloons the thread-sweep tests if too low, over-subscribes if too high. Pick a starting N;
  it is a knob, not frozen.
- **Do the currently-`#[ignore]`'d convergence studies pass when first run first-class offline?**
  Unknown until executed via A5. Any RED is a normal fix-task finding (non-blocking, D1), not a
  Part-A blocker.
- **Precise warm-gate seconds saved post-LPT.** A Phase-0-style warm `DF_VERIFY_ROLE=merge` timing
  once the partition exists quantifies the win; not required to ship Part A (premise confirmed
  structurally in §1).
- **Whether the local `hooks/pre-merge-commit` (land.sh) path should also honor `REIFY_GATE_EXCLUDE_HEAVY`.**
  Default `0` keeps local landings running the full set until an operator opts in; revisit with Part B.
