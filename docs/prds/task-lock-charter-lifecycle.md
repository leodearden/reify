# PRD — Task module-lock charter lifecycle: tight-or-empty at queue time + set-to-plan + anti-anchored architect

**Status:** deferred (author 2026-06-18). Version-agnostic build-host / orchestrator
infrastructure (root `docs/prds/`, alongside `cpu-load-admission-control.md`,
`test-run-concurrency-semaphore.md`, `jobserver-merge-priority-balancer.md`).
**Approach: B + H** (cross-repo seam into the scheduler's lock lifecycle; load-bearing →
contracts + two-way boundary tests).

**One-line goal:** make a task's module-lock charter (`metadata.files`) **tight or empty
at queue time** and **exactly the plan's footprint once a plan exists** — so the
orchestrator stops serializing a large pending backlog behind a handful of
over-declared directory locks (the jun18 *0-of-164-pending-dispatchable* result),
**without** weakening any correctness guarantee (BRE + re-pend stays untouched).

---

## 0. Origin & relationship to the tactical precursor

A jun18 contention analysis found **0 of 164 pending tasks** were fully dispatchable —
all serialized behind ~3–4 hot-module lock-holders. Root cause: many tasks declare
**whole-directory** locks (`crates/reify-eval/src/`, `crates/`, …) that vastly exceed
their real edit footprint. A four-line investigation established the mechanics
(`reference_orchestrator_module_lock_semantics_dir_overgreed`):

- Lock conflict is pure path-prefix math (`shared/locking.py:20-27 modules_conflict`);
  `normalize_lock(depth)` truncates to `lock_depth` components (`orchestrator.yaml:13`
  `lock_depth: 4`, `max_per_module: 1`).
- The over-wide dirs are written at **creation time** (PRD decompose / human-decompose
  authored `metadata.modules` = crate-coarse prose, migrated verbatim into
  `metadata.files` by `fabfa367f5`), **not** by the architect.
- The architect's plan is the **authoritative** footprint, far tighter than the
  queue-time guess — but its tightening is never written back on the success path
  (`scheduler.py` success branch mutates only the in-memory lock table).

A tactical precursor narrowed 28 of the worst offenders by hand (read-modify-write on
`metadata.files`). **This PRD is the durable, systematic fix** so the over-wide locks
cannot be reintroduced and the steady-state hold is always tight.

---

## 1. Consumer & user-observable surface (G1 / G2)

**Named consumers / enforcement points** (no orphan producers):

| Mechanism | Consumer / enforcement point |
|---|---|
| Deterministic directory-lock predicate (α) | the `/prd` decompose guard (β) **and** the `submit_task`/`commit_planning` backstop (γ) both call it |
| `/prd` decompose authoring rule (β) | the `/prd` skill decompose step — every leaf it files |
| `submit_task`/`commit_planning` backstop (γ) | **every** task-creation path (incl. human-decompose, e.g. #4552 `origin:todo-audit`) |
| set-to-plan release (δ) | the scheduler plan-complete **success** branch; downstream consumer = all pending tasks (they can now acquire the released modules) |
| anti-anchored first architect (ε) | the architect/design phase's plan-derivation input |

**User-observable surface (operator / orchestrator-facing):**

1. **No queue-time directory locks accepted.** `submit_task`/`commit_planning` with a
   directory in `metadata.files` is **rejected with a clear error**; a file-level set or
   `[]` is accepted. (Observable: the submit call's return; a guard test.)
2. **Dispatch concurrency rises.** With over-declaration gone, the count of
   simultaneously-dispatchable pending tasks on the hot crates rises from ~0 toward the
   `max_concurrent_tasks` ceiling (gated by deps + *real* file overlap, not subtree
   over-claim). (Observable: `get_scheduler_state` holders vs pending count.)
3. **Steady-state held locks equal the plan.** After a task's architect plan completes,
   `get_scheduler_state` shows its held modules **= `plan.files`** (over-claimed modules
   released); a second task needing a released module then **dispatches**. (Observable:
   `get_scheduler_events` lock_released + the waiter's task_started.)
4. **The architect derives the footprint independently.** The first architect pass does
   **not** receive `metadata.files`, so `plan.files` is its own derivation, not a
   rubber-stamp of the queue-time guess. (Observable: the plan input; `plan.files`
   diverging from the queue-time set.)
5. **No correctness regression.** An *under*-declared task still acquires every file its
   plan touches **before editing** (via existing BRE); a BRE-fail still **re-pends +
   revalidates** (staleness check preserved). (Observable: BRE/lock events; a boundary
   test.)

§§1.1–1.5 are the §8 boundary-test sketch made observable; the integration-gate leaf
(ζ) is the harness that asserts them.

---

## 2. The premise — the converged model (G6, validated by code-trace)

Two **distinct** cost centers, at two stages, with two fixes (do not conflate):

| Cost | Stage | Cause | Fix |
|---|---|---|---|
| Dispatch serialization (the 0/164) | **pre-dispatch** | over-declared queue-time `metadata.files` (a held `A/` blocks every task needing anything under `A`) | tight/empty queue-time declaration + deterministic guard (α/β/γ) |
| Spurious BRE-fail / long over-hold | **post-dispatch** | imprecise *held* locks (held wider than the real footprint for the whole task duration) | set-to-plan (δ) + anti-anchored architect (ε) |

**The charter serves two jobs with opposite optimal timing.** (a) *Pre-dispatch
collision-avoidance* wants a known-before-dispatch **superset**, tolerant of looseness;
(b) *tight steady-state working set* wants the **exact** footprint, knowable only after
the plan. The upfront `metadata.files` can only serve (a); only the plan can serve (b).
Optimizing "how good is the upfront guess" is the wrong variable — the fix is *when* the
tight set is applied.

**Under-declaration is the safe error direction** (the non-obvious load-bearing fact):
if the queue-time set misses a file the task really touches, the architect's plan
includes it and **BRE acquires it before any editing** — so two tasks never concurrently
edit the same file; the gap is closed at plan time, worst case a cheap re-pend.
**Over-declaration is the cardinal sin**: it serializes *dispatch* (upstream of where any
plan-time correction can act) **and** inflates *spurious* BRE-fails (a coarse held `A/`
makes a `A/1`-plan re-pend even when the holder only touches `A/5`).

**set-to-plan does NOT fix queue-time over-declaration** (the crux). Dispatch keys on the
*currently-held* lock; a holder's over-declared `A/` blocks others until it reaches
plan-complete — upstream of set-to-plan. So both fixes are required; neither subsumes the
other.

**BRE + re-pend is correctness machinery, not waste** — kept verbatim. A BRE-fail fires
*precisely* when another task is documented mutating the files this plan depends on, so
the plan's premises may be stale; re-pend → revalidate before proceeding is the safe
move. Cost ladder justifying it: architect recheck ≪ L2 + `/unblock` (~20×) ≪ a semantic
conflict that passes verify and corrupts main (>100×). We never run unchecked plans, and
never skip the architect even when a valid plan exists at workflow start.

---

## 3. The chosen approach (G5 — B + H)

**Three coordinated changes + one keep-as-is:**

- **Queue-time: tight-or-empty, enforced (α/β/γ).** Never put a directory in
  `metadata.files`. Name a file only where the task text gives a high-confidence anchor;
  otherwise `[]` (defer to the architect — `[]` + BRE handles even genuinely-broad
  refactors correctly: it dispatches on deps, the architect derives the broad scope, BRE
  acquires it). A **deterministic** guard (pure syntactic, no LLM) rejects directory-
  shaped entries at both the `/prd` decompose step and the `submit_task`/
  `commit_planning` backstop. **No refactor-exception is needed** — `[]` subsumes it.
- **Plan-time: set-to-plan (δ).** On plan-complete, set the held lock to **exactly
  `plan.files`** by releasing `held ∖ plan` on the success branch. (The *acquire* half,
  `plan ∖ held`, already exists as BRE; this PRD adds only the **release** half.)
- **Plan-time: anti-anchor the first architect (ε).** Hide `metadata.files` (keep the
  prose/intent) from the first architect's plan-derivation, so it derives the footprint
  independently rather than rubber-stamping a coarse guess (which would defeat δ's
  tightness). Revalidation passes are **not** anti-anchored (they legitimately re-check
  an existing plan).
- **Keep BRE + re-pend untouched** (§2) — the staleness-recheck is the correctness floor.

**Why not the partial options** (recorded so they aren't re-litigated): *guard-only*
fixes dispatch serialization but leaves locks over-held for the whole task duration
(spurious BRE-fails, late release) → fails surface (3). *set-to-plan-only* tightens
steady-state but cannot help a task that **never dispatches** because a holder over-
declared (§2 crux) → fails surface (2). *A heavy upfront predictor* (agent-team / richer
guess) is low-ROI once set-to-plan exists and BRE makes under-declaration safe — skipped.
*Measuring the spurious-vs-real BRE ratio to calibrate guess tightness* — academic: there
is no calibration knob, and post-change concurrency rises until BREs fail ~everywhere
regardless; BRE-fails are the accepted cost of uncertainty.

---

## 4. Contracts (the H component — pin the dangerous invariants)

### 4.1 Deterministic directory-lock predicate (α)

A pure, LLM-free check over a single declared path string:

- **C-P1 (reject predicate).** A path is a **directory declaration** (rejected) iff,
  after stripping a trailing `/`, its final path segment has **no recognized code
  extension** (allowlist: `.rs .ri .toml .cpp .h .hpp .c .md .json .yaml .yml .lock .py
  .sh .ts .tsx .js .txt .step .stl …`). `crates/`, `crates/reify-eval/src`,
  `crates/reify-eval/tests`, `examples`, `compute_targets`, `modal` → **reject**.
- **C-P2 (accept predicate).** A file-level path (`crates/x/src/foo.rs`,
  `examples/foo.ri`, `crates/x/tests/foo_e2e.rs`) → **accept**. An **empty** `files` list
  (`[]`) → **accept** (the deliberate "defer to architect" value).
- **C-P3 (determinism).** No model call, no filesystem stat — string predicate only, so
  it is identical at the `/prd` site and the `submit_task` site and cannot drift.
- **C-P4 (orthogonal to `lock_depth`).** The guard governs *declaration honesty* (no
  directory strings), **not** lock granularity. A deep file path
  (`…/compute_targets/foo.rs`) is accepted even though `lock_depth:4` later coarsens its
  *lock* to the `compute_targets` dir — that coarsening is a separate concern (raising
  `lock_depth` is the orthogonal lever, out of scope here).

### 4.2 set-to-plan release (δ)

- **C-S1 (set, not merely shrink).** On plan-complete, the authoritative footprint is
  `plan.files`; the held lock becomes **exactly** `plan.files`. The *acquire* half
  (`plan ∖ held`) is the existing BRE (`scheduler.py handle_blast_radius_expansion`);
  this PRD adds the **release** half (`held ∖ plan`) on the success branch (which today
  writes nothing back to the store).
- **C-S2 (ordering — never release before acquiring).** Release happens only **after**
  the task holds a superset of `plan.files`. If BRE must first acquire `plan ∖ held` and
  that acquisition re-pends (a needed module is busy), **no release occurs** — the task
  re-pends with its current charter intact. (Release is strictly a success-path action.)
- **C-S3 (pre-implementation timing — correctness).** set-to-plan runs at plan-complete,
  **before** the implementation/edit phase, so releasing `held ∖ plan` cannot release a
  module the task is mid-edit on. If implementation later needs a released file, that is
  ordinary lock escalation (re-acquire → possible re-pend), the rare case.
- **C-S4 (no silent main-break).** Residual risk is *plan incompleteness* (the plan
  under-states what implementation touches). This is pre-existing and orthogonal to this
  PRD; it is bounded by the full-workspace merge verify (a true semantic conflict → RED
  merge → requeue, **not** a silent main break) and by the charter convention. set-to-
  plan does not weaken either.

### 4.3 Anti-anchored first architect (ε)

- **C-A1 (hide files, keep intent).** The **first** architect plan-derivation receives
  the task description/intent but **not** `metadata.files`. The derived `plan.files` is
  an independent footprint.
- **C-A2 (revalidation exempt).** Re-pend → revalidation passes may see the prior
  `plan.files` (revalidation is checking an existing plan against moved main, not a fresh
  derivation) — anti-anchoring applies to the **first** derivation only.

### 4.4 Keep-as-is (C-K)

- **C-K1.** BRE (`plan ∖ held` acquire) and **re-pend-on-conflict + revalidate** are
  unchanged. The staleness-recheck property (§2) is the correctness floor and is not
  touched by δ/ε.

---

## 5. Resolved design decisions

1. **Tight-or-empty, never directory** at queue time; `[]` is a first-class value
   (defer to architect), and it subsumes the "broad refactor" case — **no refactor
   exception** in the guard.
2. **Under-declaration is the safe error direction** (BRE acquires-before-edit);
   over-declaration is the cardinal sin (serializes dispatch + spurious BRE). Authoring
   bias is therefore "name only high-confidence anchors, else `[]`."
3. **Deterministic guard, not LLM.** Directory-vs-file is a syntactic property; a pure
   predicate (C-P1) is robust, driftless, and runs identically at both enforcement sites.
4. **Guard at BOTH sites.** `/prd` decompose (primary — where over-wide values
   originate) **and** `submit_task`/`commit_planning` (backstop — catches every other
   creation path, incl. human-decompose). One shared predicate (α), two call sites.
5. **set-to-plan = the release half on the success branch.** The acquire half is the
   existing BRE; unifying both as "held := plan.files" is cleaner than a one-directional
   shrink. Release strictly after acquire (C-S2), strictly pre-implementation (C-S3).
6. **Anti-anchor the first architect only** (C-A2) — revalidation legitimately sees the
   prior plan.
7. **BRE + re-pend stays** (C-K1) — it is correctness machinery (staleness recheck), not
   the waste I initially mis-modelled it as. The cost ladder (recheck ≪ unblock ≪ main
   corruption) justifies never running unchecked plans.
8. **Reify ships the predicate + owns the `/prd` discipline; dark-factory owns the
   backstop + scheduler changes** (the established "reify ships primitive, DF wires"
   seam pattern — §7). The measurement of spurious-vs-real BRE ratio is **explicitly out
   of scope** (academic; no calibration knob — §3).

---

## 6. Pre-conditions / substrate (G3 — confirmed by jun18 code-trace; re-confirm at impl)

The substrate here is orchestrator / fused-memory **code**, not `.ri` grammar — so the
`.ri` grammar gate and `scripts/prd-decompose-verify.mjs` workflow are **N/A** (forcing
them would spurious-block; G3/G6 are done by direct code-trace, per the
`cpu-load-admission-control` / `warm-lane-pool` precedent). Confirmations:

| Capability | Status | Evidence (jun18 trace) |
|---|---|---|
| Lock conflict = path-prefix; `normalize_lock(depth)`; `lock_depth:4`, `max_per_module:1` | ✅ | `shared/locking.py:20-27,30-38`; `orchestrator.yaml:13` |
| BRE acquire exists (`plan ∖ held`) | ✅ | `scheduler.py` `handle_blast_radius_expansion` (~:3395), the requeue branch |
| Plan-complete **success** branch writes nothing back (the δ insertion point) | ✅ | `scheduler.py` success path (~:3367-3377) mutates only the in-memory lock table |
| Task-creation path = `submit_task`/`commit_planning`; `modules→files` migration | ✅ | fused-memory `task_interceptor` / `commit_planning`; `fabfa367f5` + `migrate_metadata_modules_to_files.py` |
| Architect plan input / where `metadata.files` is read (the ε hide-point) | ⚠️ confirm | `mcp/plan_tools.py` `create_plan`/`update_plan_metadata` (~:54,194); `briefing.py` `build_plan_tightening_prompt` (~:259) — **precise hide-point to be confirmed at ε impl** |
| No diff-vs-charter merge gate (so charter is convention; under-declare safe via BRE only) | ✅ | grep: reify `hooks/pre-merge-commit` = full-workspace verify; no scope gate |

**G3 verdict: PASS** with one ⚠️ (the exact architect input field to suppress for ε)
flagged for re-confirmation at implementation — it does not block the design.

---

## 7. Cross-PRD / cross-repo relationship & seam ownership (G4)

**Center of gravity is the orchestrator (dark-factory); authored as a reify PRD** because
reify owns the `/prd` authoring discipline that is the primary origin point — mirrors
`cpu-load-admission-control` (reify ships primitives, DF wires the launch path).

| Deliverable | Owner | Repo | Note |
|---|---|---|---|
| α — deterministic predicate (canonical check + test) | **reify** | reify (`scripts/`, `tests/infra/`) | the shared primitive both sites call |
| β — `/prd` decompose authoring rule + guard wiring | **reify** | reify (`.claude/skills/prd/`) | primary enforcement |
| γ — `submit_task`/`commit_planning` backstop | **dark-factory** | fused-memory | external-deps on α; catches non-`/prd` creation |
| δ — set-to-plan release on success branch | **dark-factory** | orchestrator (`scheduler.py`) | the steady-state-tightness change |
| ε — anti-anchor the first architect | **dark-factory** | orchestrator (`plan_tools.py`/`briefing.py`) | independent footprint derivation |
| ζ — integration gate | **reify** | reify (`tests/infra/`, drives via fused-memory MCP) | observes scheduler state/events |

**No reciprocal-ownership ambiguity** (reify cannot edit the DF scheduler; DF cannot edit
the reify `/prd` skill) — ownership is clean by construction. γ/δ/ε are filed as
`dark_factory` external-deps tasks depending on reify's α landing first (the predicate
contract). **Relationship to the tactical precursor:** the 28 hand-narrowed tasks (jun18)
are the manual version of β/γ's effect; this PRD makes it systematic and durable.

---

## 8. Boundary-test sketch (H — facing both producer and consumer sides)

ζ realizes this table (driving the orchestrator and asserting via `get_scheduler_state`/
`get_scheduler_events`, plus the submit path for the guard).

| # | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|
| 1 | Directory declaration rejected | `submit_task(files=["crates/reify-eval/src/"])` | **rejected** with a clear error (C-P1); the negative assertion is **observed** firing |
| 2 | File / empty declaration accepted | `files=["crates/x/src/foo.rs"]`; `files=[]` | both **accepted** (C-P2) |
| 3 | Predicate identical at both sites | same dir path via `/prd` decompose path and via `submit_task` | **both reject**, same verdict (C-P3) |
| 4 | set-to-plan releases over-claim | task held coarse-but-legal set; plan completes with a subset `plan.files` | held lock == `plan.files`; `held ∖ plan` **released** (lock_released events) (C-S1) |
| 5 | Released module unblocks a waiter | task T2 needs a module T1 over-held; T1 plan completes | T2 **dispatches** after T1's set-to-plan release (C-S1 downstream) |
| 6 | Under-declared task acquires before edit | task declares `[]` (or a subset); plan needs more | BRE **acquires `plan ∖ held` before implementation**; no concurrent edit (C-S2, C-K1) |
| 7 | set-to-plan never releases pre-acquire | plan needs a busy module | BRE re-pends; **no release**; charter intact (C-S2) |
| 8 | Staleness re-pend preserved | another task holds a plan-premise file | BRE-fail → **re-pend + revalidate** still fires (C-K1) |
| 9 | First architect is anti-anchored | task with a (legal, file-level) `metadata.files` | first plan input **excludes** `metadata.files`; `plan.files` independently derived (C-A1) |
| 10 | Revalidation not anti-anchored | re-pended task with an existing plan | revalidation sees the prior plan (C-A2) |

Facing-the-producer rows: 1, 2, 3 (the guard predicate in isolation), 9, 10 (the
architect input). Facing-the-consumer rows: 4, 5, 6, 7, 8 (the composed scheduler
behavior under real multi-task contention). ζ is the leaf whose observable signal **is**
this table.

---

## 9. Decomposition plan (one bullet per task → its observable signal)

Greek labels; actual task IDs assigned at decompose. **B+H shape:** foundation predicate
(α) → enforcement at both sites (β reify, γ DF) ‖ scheduler tightening (δ, ε DF) →
integration gate (ζ).

- **α — Deterministic directory-lock predicate (reify primitive + test).**
  *Modules:* a reify script (e.g. `scripts/lock-charter-guard.sh` or `.py`),
  `tests/infra/`.
  *Signal (intermediate → unlocks β, γ, ζ):* `tests/infra/test_lock_charter_guard.sh`
  drives the predicate and observes: every directory-shaped path **rejected** (incl.
  trailing-slash, deep module dirs like `compute_targets`); every file-level path and
  `[]` **accepted**; verdict is deterministic (no model/FS). *G6:* the rejection is
  **observed** firing (negative-assertion mandate), not asserted. *Manifest:* `wired` —
  β and γ call it.

- **β — `/prd` decompose authoring rule + guard wiring (reify).**
  *Modules:* `.claude/skills/prd/references/decompose-mode.md`, `.claude/skills/prd/project.md`
  (the "name-when-confident-else-`[]`, never-a-directory" rule), the decompose filing step.
  *Signal (leaf):* a decompose run that would file a directory-shaped `metadata.files`
  is **blocked** by α before `submit_task`; filed leaves contain **zero** directory
  entries (inspect the filed tasks). *Manifest:* `wired` — decompose calls α (grep the
  call site).

- **γ — [dark-factory, external-deps] `submit_task`/`commit_planning` backstop (fused-memory).**
  *Repo:* dark-factory (fused-memory task-creation path).
  *Signal (leaf; consumer = every creation path):* `submit_task`/`commit_planning` with a
  directory in `metadata.files` is **rejected with a clear error**; file-level/`[]`
  accepted — observed on the submit call (catches human-decompose, e.g. the #4552 class).
  *Depends:* α (the predicate). *Owner:* dark-factory.

- **δ — [dark-factory, external-deps] set-to-plan release on the plan-complete success branch (scheduler).**
  *Repo:* dark-factory (`orchestrator/scheduler.py`).
  *Signal (leaf):* after a task's architect plan completes, `get_scheduler_state` shows
  its held modules **= `plan.files`** (`held ∖ plan` released via lock_released events);
  a second task needing a released module **dispatches** (task_started). Release happens
  **only on the success branch and only after** any BRE acquire (C-S2). *Depends:* —
  (independent scheduler change; composes with existing BRE). *Owner:* dark-factory.

- **ε — [dark-factory, external-deps] anti-anchor the first architect (orchestrator).**
  *Repo:* dark-factory (`mcp/plan_tools.py` / `briefing.py` — exact hide-point confirmed
  at impl per §6 ⚠️).
  *Signal (leaf):* the **first** architect plan-derivation input **excludes**
  `metadata.files` (keeps the description); the derived `plan.files` is independent of
  the queue-time set; revalidation passes are unaffected (C-A2). *Depends:* — *Owner:*
  dark-factory.

- **ζ — Integration gate: the converged behavior end-to-end (reify harness).**
  *Modules:* `tests/infra/` (drives the orchestrator; asserts via fused-memory
  `get_scheduler_state`/`get_scheduler_events` + the submit path).
  *Signal (the leaf — full surface §1):* the §8 table — guard rejects dir / accepts
  file+`[]` at both sites; set-to-plan releases over-claim and a waiter dispatches; an
  under-declared task BRE-acquires before edit; a BRE-fail still re-pends+revalidates
  (staleness preserved); first architect anti-anchored. *Depends:* α, γ, δ, ε.
  *G6:* the "no correctness regression" claims (rows 6, 8) are **observed** (BRE acquire
  precedes edit; re-pend fires), not assumed. *Manifest:* `rejection-check` (rows 1, 8)
  + `scheduler-state` (rows 4, 5, 9).

**DAG:** α → β; α → γ; α → ζ; γ → ζ; δ → ζ; ε → ζ. (δ, ε are independent DF scheduler
changes; β is reify discipline.) ζ is the integration leaf (G2 escape hatch: α is the
foundation intermediate roped into ζ). reify's α/β/ζ are landable and observable; γ/δ/ε
are the dark-factory external-deps.

---

## 10. Out of scope / accepted limitations

- **Raising `lock_depth`** (finer granularity for deeply-nested module dirs like
  `compute_targets`) — orthogonal lever, separate decision (C-P4). This PRD governs
  declaration honesty, not lock granularity.
- **A heavy upfront footprint predictor** (agent-team / `_tag_task_modules` recycling) —
  low-ROI once set-to-plan + BRE exist (§3); explicitly not pursued.
- **The spurious-vs-real BRE-fail measurement** — academic (no calibration knob; §3).
- **Enforcing the charter as a hard diff-scope gate** (so an agent *cannot* touch a file
  outside its lock) — a distinct, larger change; today the charter is a convention and
  the full-workspace merge verify is the backstop (C-S4). Noted as the natural companion
  that would make set-to-plan's released-module window fully safe, but out of scope here.
- **The 28 hand-narrowed precursor tasks** — already done tactically; this PRD supersedes
  the need to repeat that by hand.

---

## 11. Open questions (tactical — deferred, not design-level)

1. **Predicate transport across repos.** α ships as a reify script; does γ (fused-memory)
   shell out to it, vendor a copy, or re-implement against a shared spec? The predicate is
   ~10 lines of pure logic. **Suggested:** reify ships the canonical script + the spec in
   the PRD; γ re-implements against the spec with a shared test vector (avoids a fused-
   memory→reify runtime dependency). Decide during γ.
2. **Exact extension allowlist** (C-P1). Confirm completeness against real
   `metadata.files` corpora (`.ri .rs .toml .cpp .h .md .json .yaml .py .sh .ts .step
   .stl …`). Decide during α.
3. **ε hide-point.** Precise architect-input field to suppress (`plan_tools.create_plan`
   args vs the briefing prompt assembly) — confirm at ε impl (§6 ⚠️).
4. **set-to-plan event shape.** Whether the release emits per-module `lock_released` (as
   today) or a single `set_to_plan` event — affects ζ's assertion granularity. Decide
   during δ.
5. **`/prd` rule wording.** The exact "high-confidence anchor" heuristic phrasing in
   decompose-mode.md (so authors reliably choose file-or-`[]`, never a directory). Decide
   during β.
