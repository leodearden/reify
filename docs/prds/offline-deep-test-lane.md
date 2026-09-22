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
| `REIFY_GATE_EXCLUDE_HEAVY` knob (default **0** = current behavior) | verify.sh gate roles read it; **default keeps the heavy set on the gate** | Part B flips it to `1` (dark-factory-orchestrator.yaml verify env) the moment the lane is live |
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
  in `dark-factory-orchestrator.yaml`'s verify env the moment the async lane is live — a one-line, immediate,
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
- **DA5 — `scripts/land.sh` sets `REIFY_GATE_EXCLUDE_HEAVY=1`; `hooks/pre-merge-commit` does NOT.**
  Resolved by Leo, 2026-08-31 (esc-6485-3, option B), settling what §10 previously carried as an open
  tactical question. `scripts/land.sh` exports the knob alongside its existing `DF_VERIFY_ROLE=merge`,
  so the sanctioned manual-land path scopes its gate exactly as `dark-factory-orchestrator.yaml` already
  does for every orchestrator-spawned role. The hook is deliberately left alone: it is the shared gate
  entry point, and a bare local `git merge --no-ff` on `main` is already unsanctioned by CLAUDE.md, so
  the hook keeps the wider coverage.
  **The trade that decided it** was local-land DIAGNOSABILITY over pre-merge-blocking local-land
  COVERAGE. Without the carve-out this path runs `--profile both --scope all` (so `NARROW_ACTIVE=0` and
  the debug `--workspace` pass runs heavy members) under a binding 3600s wall, where the then-12h
  per-test ceiling in `.config/nextest.toml` was unreachable and a heavy hang degraded to a bare
  `timeout` exit 124 naming nothing — the task 4877/4878 zero-attribution shape. Coverage is DEFERRED,
  not deleted: the offline lane's trigger is SHA-based, and a local land bypasses the orchestrator's
  `on_post_merge` notifiee, so it is the lane's poll backstop
  (`git.offline_lane_poll_interval_secs`, 120s, comparing `main`'s tip against the head of the last
  completed run) that picks the commit up — within ~2 minutes — and runs the heavy set there under the
  by-name ceiling.
  **PREMISE CHANGED, ruling untouched (task 7552, 2026-09-22).** The diagnosability half of that trade
  no longer holds as stated: DA6's ceiling is now 2160s, which this path's own 3600s debug wall clears
  by more than the measured time a pass takes to reach a heavy test, so running heavy members here
  would be attributed by name rather than degrading to exit 124. DA5 is a ratified human ruling and task 7552 did not re-open it — its other stated grounds (the
  hook is the shared gate entry point; coverage is deferred by ~2 minutes, not lost) are unaffected, and
  the latency/cost of running the heavy set on every local land was never the zero-attribution argument
  anyway. Recorded here so the next reader does not cite a premise that has since changed. Whether DA5
  is still the right call on its remaining grounds is a question for its owner, not for the task that
  changed the number.
  **Accepted residual:** a heavy failure on a locally-landed commit yields a fix task rather than
  blocking the land.
  The default is expressed as `${REIFY_GATE_EXCLUDE_HEAVY:-1}`, not a bare `1`: DA5 settles what
  happens when nobody says otherwise, not whether an operator may say otherwise. `REIFY_GATE_EXCLUDE_HEAVY=0
  scripts/land.sh <branch>` still buys full local heavy coverage at the cost of the attribution above.
  Adjacent `DF_VERIFY_ROLE=merge` is genuinely non-negotiable on this path and stays unconditional.
- **DA6 — Three ceiling tiers, and what makes each one safe.** THIS IS THE NORMATIVE COPY of the
  argument; `.config/nextest.toml`, `scripts/verify.sh`, `scripts/land.sh` and the guard tests carry a
  rule and a pointer here, not a restatement. Six copies of one argument is how the claim "no gate path
  runs heavy members" came to be carried, in this file's own words, after it had become false.

  `.config/nextest.toml` sets a per-test `slow-timeout`/`terminate-after` ceiling in three tiers:

  | tier | ceiling | members |
  |---|---|---|
  | default | 1200s | everything with no more specific override |
  | gate-resident | 1800s | `representation_within_assertion` (LPT tier 50), `solve_elastic_static_body_e2e` (task 7339 contention headroom) |
  | heavy | 2160s (120s x 18) | all 8 members of `REIFY_HEAVY_NEXTEST_FILTER` |

  **What the ceiling buys.** On expiry nextest SIGTERMs the offending test BY NAME. The pass-level
  `timeout` wall in `scripts/verify.sh` does not: it kills the whole nextest process tree as exit 124
  attributing nothing — the task 4877/4878 shape. So a ceiling is only worth having where it is
  REACHABLE. **"Reachable" is NOT "strictly under the wall"**, which is how this document had it and
  is the error corrected below: the wall's clock and the ceiling's clock do not start together, so
  reachability is `binding_wall - ceiling > the time the pass takes to reach the test`.

  **SUPERSEDED (1 of 2), 2026-09-22 (task 7552): the heavy tier was 43200s (12h).** The 12h
  figure is kept visible rather than overwritten, because what changed is not a typo but a decision
  this PRD had already flagged as its own weakest point. DA6 closed by conceding "12h is not a measured
  figure" and naming the measurement ticket as the one that "should be settled BEFORE" the background
  ticket, since "a small enough measured ceiling dissolves the background gap rather than answering it".
  Task 7552 did exactly that, in that order. The paragraphs this replaces are recoverable from git
  (`docs/prds/offline-deep-test-lane.md` @ 8897299ac0, the task-6485 amendment that first collapsed six
  copies of this argument into DA6).

  **SUPERSEDED (2 of 2), same day, same task: the first re-size landed 3240s and the RULE that produced
  it was wrong.** This is the second supersession in one document and the intact trail is the point,
  because the obvious question — why did a *measured* figure need correcting within hours? — has a
  specific answer, and it is not "the measurement was bad".

  **The defect: the 0.9x rule compared two clocks that do not start together.** `scripts/verify.sh`
  emits `timeout --kill-after=60 ${outer_timeout} ... cargo nextest run ...` around what its own comment
  calls "one combined build+execution nextest pass per profile" — that clock starts at PASS start, and
  the BUILD is inside it. nextest's `slow-timeout.terminate-after` clock starts at TEST-PROCESS start.
  A ceiling C is therefore the binding bound only for a test that starts within `W - C` of pass start,
  which at C=3240 and W=3600 is 360 seconds. Two figures already in this repository exceeded that
  before the measurement was taken: `scripts/verify.sh` records **798.9s** as the worst observed
  HEALTHY whole-pass debug completion (cold target, warm sccache, and on a test set that did not yet
  carry today's 8 heavy members), and this task's own notes file records **580s** for the RELEASE,
  heavy-ONLY `--no-run` build — a strictly smaller build than the `--workspace` debug one `background`
  runs. `background` was never reliably reachable at 3240s.

  **What made it survive review.** The wrong rule was the ASSERTED rule:
  `tests/infra/test_nextest_slow_priority.sh` mechanised the same bare `wall > ceiling` comparison, so
  Assertion L reported REACHABLE and stayed green while the config, this PRD and the guard were
  reviewed together. Fixing only the number would have left the rule in place. The predicate moved
  first (`_role_class`, the one helper both the assertions and the fixtures drive), and the number
  followed from it.

  **Which wall binds is a per-role fact**, and it is the joint everything else turns on. A role's
  binding wall is the tighter of the walls for the profiles it forces. `offline` forces `release` and
  gets a role-scoped 13h wall (46800s). `background` forces `both`, so its binding wall is the 60m
  (3600s) debug one — the TIGHTEST wall any heavy-running role imposes, and therefore the one the
  ceiling has to fit under, by the margin the paragraph above defines. `task` and `merge` do not run
  heavy members at all.

  **How 2160s was derived.** Top-down from reachability, with the measurement used as a floor
  VALIDATION rather than as the source of the number. This direction is deliberate: a per-test ceiling
  exists to ATTRIBUTE a hang by name before the pass-level `timeout` fires exit 124 naming nothing, so
  its correctness condition is a relation to the WALL, not to a measurement. A bottom-up figure
  (measured max x some margin) can land anywhere relative to the wall — which is precisely how 43200s
  came to sit 12x above the wall that binds `background`.

  | bound | value | source |
  |---|---|---|
  | upper (reachability) | <= `binding_wall - O_bound` = 3600s - 1078.7s = 2521.3s | wall derived from `scripts/verify.sh`; `O_bound` measured |
  | ~~upper, SUPERSEDED~~ | ~~<= 0.9 x 3600s = 3240s~~ | ~~the two-clock error above: 10% of the wall is not the time the pass takes to reach the test~~ |
  | lower (tier order) | > 1800s gate-resident, > 1200s default | `.config/nextest.toml` |
  | grain | multiple of the 120s `period` | `.config/nextest.toml` |
  | floor (no false kills) | >= 3 x measured per-test max, in EITHER profile | measurement, below |

  `O_bound` = **1078.7s**, the pre-test-start overhead: the seconds from the `cargo nextest run`
  invocation to the LATEST-starting heavy atom's process start, measured on the pass that actually
  binds — `background`'s debug `--workspace` pass. The latest-starting one, not the first, because any
  of the 8 could be the member that hangs. It has two terms and both matter: the combined build the
  outer `timeout` also wraps (544.5s in the worse of two runs), and queue position (a further 534.2s),
  because the LPT `priority = 100/50` overrides cover only 4 of the 8 atoms and the other three start
  several hundred seconds after the first test.

  The window `[max(3 x M, 1800), 2521.3]` is non-empty — `{1920, 2040, 2160, 2280, 2400, 2520}` —
  and C = **2160s** (`terminate-after = 18`).

  **WHY NOT 2520s, THE LARGEST CANDIDATE.** It was taken first, on a rule that said the largest
  candidate always wins: when the false-kill floor and the attribution bound pull against each other
  the floor should win, because a ceiling below a legitimate test's real cost SIGTERMs a healthy test
  and reports it as a timeout — destroying the very signal the ceiling exists to produce — while a
  ceiling too high to be reached merely fails to improve on the pass-level `timeout`. That reasoning
  is sound and still holds **when the two bounds are in conflict**. Here they were not: `3 x M` is
  1635.6s and the reachability bound is 2521.3s, so every candidate in the window already clears the
  floor and the tie-break never applied. Taking the top of the window anyway spent 100% of the derived
  headroom to buy false-kill margin that was not being contested.

  And the headroom is the side that cannot afford it. `3600 - 2520 = 1080` clears the 1078.7s budget
  by 1.3s, and that remainder is the GRAIN STEP, not a safety margin — the leftover under a
  largest-step rule is always in `[0, 120)` and carries no information. All the safety sat in
  `O_bound`, which is a max over N=2 observations on ONE host with a WARM `target/` (run 1 executed
  only 201 compilations), while `scripts/verify.sh` separately records **798.9s** as a worst healthy
  whole-pass debug completion on a COLD target with a SMALLER test set than today's. The build term in
  `O_bound` is therefore known to be under-measured, and the failure it admits is silent: a
  freshly-seeded cold lane exceeds `O`, the hung test is never reached before the 3600s wall fires,
  `background` degrades to the bare exit 124 this task exists to close — and no guard reds, because
  Assertion L pins the literal 1079 and cannot see a real `O` that has drifted.

  `3600 - 2160 = 1440` instead: ~33% margin over the measured budget, at a cost of 4.0x M rather than
  4.6x M against a floor of 3x. Anyone re-tuning should re-measure `O` first — on a cold lane — rather
  than reason about the remainder.

  **The cheap structural alternative, not taken here.** `O_bound`'s 534.2s queue term exists only
  because the LPT `priority` overrides cover 4 of the 8 heavy atoms; giving `buckling_smoke` and the
  two `harness_fea_solver_e2e` filters a priority would start them with the others and collapse `O`
  to a build-time fact alone, roughly halving it. It is deliberately out of scope: `priority` is
  merge-gate SCHEDULING, a different lever from a kill ceiling, and `.config/nextest.toml` records
  that separation at those blocks. It is the first thing to try if `O` is ever re-measured too high.

  **The measurement, in two passes.** The first measured the OFFLINE lane's release invocation —
  lifted from its `--print-plan` output, `--run-ignored all` included — twice, back to back under the
  host's natural contention (loadavg 57-131 on 32 cores). 52 tests, 52 passed both runs, no FAIL and
  no TIMEOUT. **Worst per-test cost, release: 545.0s.**

  The second closed a gap the first left, and it is the reason the floor is now stated "in EITHER
  profile": `[profile.default]`'s overrides govern BOTH profiles while `background` forces
  `--profile both`, so a release-only figure was standing in for a bound it had never been measured
  against — and could have been understating it. Two further full debug `--workspace` runs (loadavg 254 then 82 on 32 cores,
  24690/24690 passing both, exit 0 both) settled it and produced `O_bound` from the same runs.
  **Worst per-test cost, debug: 545.2s — the SAME test, at the same cost.** So the debug profile does
  not move the worst case on this host, and **M = 545.2s**; 2160s clears 3 x M = 1635.6s at 4.0x.
  (`reify-solver-elastic::modal_benchmarks` is `cfg_attr(debug_assertions, ignore)` and does not run in
  debug at all, so 7 of the 8 atoms, 43 tests.)

  Full per-test tables both ways, host state, sccache deltas, method and caveats:
  `docs/notes/heavy-test-per-test-duration-measurement.md`. **N = 2 per profile** — two samples bound a
  worst case observed on one host and are not a distribution; the slowest tests more than doubled
  between runs tracking host load, so contention, not intrinsic cost, is the dominant term in M, and
  cache/`target/` state is the dominant term in `O`.

  **Why 3x and not 4.5x.** 3x is this repo's own PER-TEST precedent: task 7339 put 1800s over a 615s
  worst COMPLETED run under measured contention. `scripts/verify.sh`'s 4.5x idiom is for whole-PASS
  walls absorbing a cold compile and does not transfer here, where a wider ceiling is a COST — it
  widens the window before a genuine hang is attributed, which is the defect task 5141 introduced the
  per-test ceiling to fix.

  **Why a 2160s ceiling is safe on the BLOCKING gate.** Not because "no gate path runs heavy members" —
  that is false as stated. `REIFY_GATE_EXCLUDE_HEAVY=1` is set for every orchestrator-spawned role
  (`dark-factory-orchestrator.yaml`) and by `scripts/land.sh`, but setting the env var decides nothing:
  `scripts/verify.sh` scopes its EFFECT to the `task` and `merge` roles alone (`_GATE_HEAVY_EXCLUDE`).
  Those two, plus the sanctioned manual-land path, genuinely do not run heavy members — and that, not
  the env var, is what makes the tier safe where a red blocks a merge. Since task 7552 the tier is also
  safe in the stronger sense that it is reachable everywhere it applies.

  **Accepted residual: NONE (task 7552 retired it) — and here is the honest version of why.**
  `background`'s binding 3600s debug wall exceeds the 2160s ceiling by 1440s, comfortably more than
  the 1078.7s the pass was measured to take to reach the latest-starting heavy atom. That is the claim,
  stated against the clock that actually applies; the earlier form of this paragraph ("the wall
  strictly exceeds the ceiling") was true of the numbers and false of the mechanism. The two
  non-orchestrator paths close with it: a `scripts/verify.sh` run with `REIFY_GATE_EXCLUDE_HEAVY`
  unset runs under that same 3600s debug wall, and a bare `cargo nextest run` has no outer wall for
  the ceiling to be unreachable under at all — the last of those closes unconditionally and does not
  depend on any measurement.

  **What would re-open it.** A `background` debug pass whose build plus queue position exceeds 1440s —
  a cold `target/`, a heavier workspace, more heavy atoms left outside the LPT priority set. `O_bound`
  is a two-sample max, not a guarantee, so this residual is retired on measured evidence rather than
  proved absent. The cheap structural improvement, if it does re-open, is to give the three
  un-prioritised heavy atoms an LPT `priority` so they start with the other four: that removes the
  534s queue term outright and leaves `O` as a build-time fact alone. The expensive ones are the two
  named under "why the re-size" below, both still rejected.

  `.config/nextest.toml` keeps the CONTRACT that introduced the list — a role may be unreachable only
  if named both in that paragraph and in Assertion L's allowlist — with no members. Assertion L's
  residual branch is exercised by a synthetic-role fixture scaffold rather than by a live gap, so the
  machinery stays honest at zero members and a future residual needs no fixture work.

  **Why the re-size, and not either remedy DA6 originally offered.** Both were coverage/budget
  decisions needing the same kind of explicit human ruling DA5 got, and task 7552 took neither:

  - (a) Extending the exclusion to `task|merge|background` reduces main-tip heavy coverage and
    contradicts the stated "Role=background NEVER skips (the sweep IS the backstop)" contract.
  - (b) Role-scoping `background`'s walls past the ceiling is not merely undesirable, it is
    **structurally defeated**, and this is the load-bearing finding. dark-factory's `run_main_tip_sweep`
    calls `run_full_verification(..., role='background')` — the NON-merge branch of
    `_resolve_verify_timeout` — so the entire `--profile both --scope all` sweep is bounded by
    `verify_command_timeout_secs: 7200` in `dark-factory-orchestrator.yaml`. Raising verify.sh's INNER
    walls above ~3600s for `background` buys nothing unless that OUTER wall is also raised past the
    ceiling — which is exactly the "one hung heavy test stalls the cadence sweep for 13 hours" outcome
    (b) was rejected for, now with a second repo's config in the blast radius.

  The third path is the one DA6 named itself and ordered first, and it is strictly better than either:
  re-sizing under the existing wall dissolves the gap instead of answering it, and deletes the residual
  machinery's live user rather than moving it.

  **The offline 13h release wall STAYS, on a different basis.** Its ORIGINAL justification is gone:
  at a 2160s ceiling the 90m (5400s) BASE release wall clears it by 3240s — comfortably more than the
  580s that release, heavy-only build was measured to take — so "13h exists to make the per-test
  ceiling the binding bound" is no longer true of anything. (That comparison is made against the
  corrected rule, not the superseded one: 3240s of headroom against a 580s build, not merely
  5400 > 2160.) A second basis survives and was
  never the stated one — WHOLE-RUN headroom for the lane's `--run-ignored all` release pass, whose
  recorded sub-runs reach 2625s against that 5400s base wall (barely 2x) while heavy membership has
  grown from 6 atoms to 8 since the lane was designed. (Task 7552's own timed runs of that pass, 770s
  and 671s, ran against a PRE-BUILT target and bound the execution half only; they are a floor under the
  2625s figure, not a replacement for it.) This is recorded explicitly so a future reader neither
  deletes the scoping against a rationale that is already gone, nor keeps it for one.
  `scripts/verify.sh` carries the same restatement at all three sites that used to assert the old one.

  **Mechanised, not asserted — but mechanising the WRONG rule is what let 3240s land.** That is the
  lesson this paragraph has to carry, not just the assertion list.
  `tests/infra/test_nextest_slow_priority.sh` derives the heavy set from
  `scripts/heavy-test-filter-lib.sh` and the role sets, walls and per-role profiles from
  `scripts/verify.sh`: Assertion J requires a 2160s block per heavy atom, K requires every slow-timeout
  override to classify as heavy or gate-resident, K-tier (new in 7552) requires the heavy tier to stay
  strictly ABOVE the gate-resident tier — newly load-bearing now the two are within 1.2x rather than 24x —
  and L requires every heavy-RUNNING role to either reach the ceiling or be an enumerated residual.

  L's reachability predicate now lives in ONE helper, `_role_class`, as
  `wall - ceiling > L_START_OFFSET_BUDGET_SECONDS`, with the RESIDUAL branch written as its exact
  complement so no role can fall between them. That budget (1079s) is the ONLY measured literal in the
  file and cannot be otherwise — the seconds a pass takes to reach a test are a property of the build
  and of nextest's scheduling, and no file in the tree states them — so it is kept honest by being
  compared only against a DIFFERENCE of two file-derived numbers. Two fixtures were added with it,
  because the predicate changed shape: one seeds a wall that EXCEEDS the ceiling but not by the budget
  (the false green itself, which the old rule accepted), and one seeds a roomy ceiling that must still
  classify REACHABLE, without which a budget that failed to parse would reject every role and look
  like rigour.
  DA6 predicted "L reds again, deliberately, once remedy (a) lands and the `background` entry goes
  stale". **That prediction came true through a different remedy and is now closed**: L red-lit on the
  re-size, not on remedy (a), and the allowlist entry and the config's residual note were retired in the
  same commit — which is what the guard was built self-cleaning to force. L's residual allowlist is now
  EMPTY, so its residual branch has no live user; it is kept honest by a synthetic-role fixture scaffold
  rather than by a real gap. `tests/infra/test_occt_flock_gate.sh` T14-T17 pin offline's role-scoped
  wall and T18-BG (new in 7552) pins `background`'s base 60m/90m walls and its heavy membership, the two
  facts the re-size rests on.

  **Follow-up tickets, both discharged by task 7552.** `tkt_0RTN426YPJ2JWVP3YGQQZ8KH7C` (measure the
  heavy set, including the `#[ignore]`d convergence studies) — done twice over: the release pass it
  asked for, and then the debug pass and pre-test-start overhead the first re-size turned out to need.
  Evidence for both in `docs/notes/heavy-test-per-test-duration-measurement.md`.
  `tkt_0RTN0PQ35EZGXGQ7WF2HXZE2N9` (the `background` reachability gap) — dissolved rather than
  answered, per the sequencing DA6 itself prescribed, and dissolved on the CORRECTED bound rather than
  the 0.9x one it was first closed against. Re-open it if a re-measured `O` exceeds 1440s; "what would
  re-open it" above says what to try first.

  **Rejected: role-scoping the ceiling in the DERIVED config (option D, esc-6485-3).** It would dissolve
  the verify.sh-mediated paths but not a bare `cargo nextest run`, and `gen-nextest-config.sh` is a
  line-anchored sed rewriter whose ceiling literal is not unique across blocks — doing it correctly needs
  a section-aware pass keyed on each block's `filter =` line plus new test pins. A task of its own.

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
| **`flip-gate-exclude-heavy`** — set `REIFY_GATE_EXCLUDE_HEAVY=1` in `dark-factory-orchestrator.yaml` verify env | **dark-factory (Part B)** | **cross-project edge → Part A knob leaf** + Part B lane-live leaf |

**The flip seam contract (the one interface both PRDs must agree on):**
> `scripts/verify.sh`, on role `task`/`merge`, applies the nextest filter `not (heavy)` **iff**
> `REIFY_GATE_EXCLUDE_HEAVY` is exactly `1`; for any other value (unset/empty/0) the gate runs the
> full set unchanged. The variable is read from the environment so `dark-factory-orchestrator.yaml`'s verify env
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
- ~~**Do the currently-`#[ignore]`'d convergence studies pass when first run first-class offline?**~~
  **SETTLED, task 7552 (2026-09-22): YES, both pass.** Executed first-class for the first time on any
  path, under the offline lane's own `--run-ignored all` invocation, twice:
  `cantilever_faithful_convergence_study` 44.5s / 32.1s and `cylinder_lame_convergence_study` 2.8s /
  2.5s, PASS in both runs. Cost is modest, not the unknown DA6 feared — they rank 17th and 32nd of 52
  by duration. No RED, so the non-blocking D1 fix-task path was not needed. Evidence:
  `docs/notes/heavy-test-per-test-duration-measurement.md`.
- **Precise warm-gate seconds saved post-LPT.** A Phase-0-style warm `DF_VERIFY_ROLE=merge` timing
  once the partition exists quantifies the win; not required to ship Part A (premise confirmed
  structurally in §1).
