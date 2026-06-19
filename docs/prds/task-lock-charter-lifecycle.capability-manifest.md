# Capability manifest — Task module-lock charter lifecycle

Mechanizes G3 + G6 per leaf for `docs/prds/task-lock-charter-lifecycle.md`. Built at
decompose time **by direct code-trace** (orchestrator / fused-memory Python + reify
shell substrate — the `.ri` grammar gate and `scripts/prd-decompose-verify.mjs` are
**N/A** here, per the PRD §6 / `cpu-load-admission-control` / `warm-lane-pool`
precedent). Evidence forms: `grep:<file>:<line> wired-on-main` · `producer:task-<label>
upstream` · `substrate:<file>:<line> exists`. Any FAIL value (`declared-only` ·
`test-only` · `producer-absent` · `producer-extent-short` · `producer-downstream` ·
`rejection-absent`) blocks the batch.

> **Repos.** α/β/ζ are **reify** tasks (substrate under `scripts/`, `tests/infra/`,
> `.claude/skills/prd/`). γ/δ/ε are **dark_factory** external-deps (substrate under
> `/home/leo/src/dark-factory/...`, traced at HEAD `f15c295914`).

> **Decompose-time G6 correction (2026-06-18).** A code-trace found δ's *in-memory*
> release half **already exists** (`scheduler.py:3418 handle_blast_radius_expansion` →
> `release_subset` + `lock_released`/`plan_refinement`, landed `6f29517823` 2026-04-21).
> PRD §3/§4.2/§6/§9 were corrected and δ re-scoped to its true residual (persist the
> tightened set on the success branch + observability). See the δ block below.

---

## α — Deterministic directory-lock predicate (reify primitive + test) — intermediate

Signal: `tests/infra/test_lock_charter_guard.sh` drives the predicate — every
directory-shaped path **rejected** (incl. trailing-slash + deep module dirs like
`compute_targets`), every file-level path and `[]` **accepted**, verdict deterministic
(no model/FS). The rejection is **observed firing** (negative-assertion mandate).

| Capability | Evidence | Verdict |
|---|---|---|
| Pure-syntactic dir-vs-file predicate C-P1/C-P2/C-P3 (new reify code) | `producer:task-α` — α builds the script; substrate `scripts/lib_portable.sh`, `scripts/verify-pipeline-guard.sh` (precedent guard-script pattern) exist | **PASS** |
| `tests/infra/` harness can drive a shell predicate + observe its verdict | `substrate:tests/infra/run_all.sh` + `test_agent_cargo_shim.sh`/`test_audit_orphan_producers.sh` (existing `.sh` harness pattern) | **PASS** |
| Rejection mechanism exists and fires on a directory path (G6 branch 4 / rejection-check) | `producer:task-α` (α **is** the rejection mechanism) + its own `test_lock_charter_guard.sh` authors a dir path and **observes** the reject verdict — not asserted | **PASS** (producer = self, observed) |
| Wired into a consumer (anti-orphan) | downstream `producer:task-β` (α→β) **and** `producer:task-γ` (α→γ) both call/re-implement the predicate against the shared spec | **PASS** (wired, both downstream) |

---

## β — `/prd` decompose authoring rule + guard wiring (reify) — **leaf**

Signal: a decompose run that would file a directory-shaped `metadata.files` is
**blocked** by α before `submit_task`; filed leaves contain **zero** directory entries
(inspect the filed tasks; grep the α call site in the decompose step).

| Capability | Evidence | Verdict |
|---|---|---|
| The α predicate (dir-rejection extent) | `producer:task-α upstream` (α→β edge); extent = directory-rejection, exactly what β consumes | **PASS** |
| `/prd` decompose skill files to add the rule + call site | `substrate:.claude/skills/prd/references/decompose-mode.md` + `.claude/skills/prd/project.md` exist | **PASS** |
| Block is observable (rejection-check) | `producer:task-α` fires the reject; β observes the decompose filing is blocked / filed leaves have no dir entries | **PASS** (observed, producer upstream) |

---

## γ — `submit_task`/`commit_planning` backstop (dark_factory, external-deps) — intermediate

Signal: `submit_task`/`commit_planning` with a directory in `metadata.files` is
**rejected with a clear error**; file-level/`[]` accepted — observed on the submit call
(catches non-`/prd` creation, e.g. the #4552 human-decompose class).

| Capability | Evidence | Verdict |
|---|---|---|
| Task-creation path to hook the backstop into | `grep:fused-memory/src/fused_memory/server/tools.py:2356 submit_task` + `:2626 commit_planning` (wired-on-main) | **PASS** |
| Predicate spec to enforce (shared with α) | `producer:task-α upstream` (α→γ external-dep edge); OQ#1 — γ re-implements against α's spec + shared test vector | **PASS** |
| Rejection mechanism fires on a dir path at submit (rejection-check) | `producer:task-γ` (γ builds the backstop) — observed firing in ζ row 1/3 | **PASS** (producer upstream of ζ) |
| Wired into a consumer (anti-orphan) | downstream `producer:task-ζ` (γ→ζ) asserts it via the submit path | **PASS** |

---

## δ — Persist set-to-plan tightening on the success branch (dark_factory, external-deps) — intermediate

Signal: after a task's architect plan completes, `get_scheduler_state` shows held
modules **= `plan.files`** (`held ∖ plan` released via `lock_released`) **and** the
persisted `metadata.files` equals `plan.files` (survives a scheduler restart); a waiter
needing a released module **dispatches** (`task_started`). Release only on the success
branch, only after BRE acquire (C-S2).

| Capability | Evidence | Verdict |
|---|---|---|
| In-memory release `held ∖ plan` on success + `lock_released` event | `grep:scheduler.py:3436 stale = current_set - needed_set` → `:3442 release_subset` → `:3444-3448 lock_released/'plan_refinement'` (**already wired-on-main**, `6f29517823` 2026-04-21); called from `workflow.py:2448/2560/2720` on any `plan_modules != self.modules` | **PASS** (already present — δ does **not** re-implement it) |
| Acquire half `plan ∖ held` (existing BRE, composes) | `grep:scheduler.py:3440 try_acquire_additional` (wired-on-main) | **PASS** (already present) |
| **Persist tightened set to `metadata.files` on the success branch** (the δ residual) | `producer:task-δ` (DF) — substrate: success path `workflow.py:2465 self.modules = plan_modules` is in-memory only; the requeue branch `scheduler.py:3466-3469 update_task(... {'files': needed})` is the **pattern to mirror** onto the success path | **PASS** (producer = δ; substrate confirmed) |
| Waiter dispatches after release (downstream of release) | existing scheduler dispatch on freed lock (`get_scheduler_events task_started`); observed in ζ row 5 | **PASS** |
| Observability ζ asserts on | `grep:scheduler.py:3444 EventType.lock_released` (wired); per-module vs single `set_to_plan` event is OQ#4 (decide during δ) | **PASS** |
| Wired into a consumer (anti-orphan) | downstream `producer:task-ζ` (δ→ζ) | **PASS** |

*G6 note:* δ's PRD-as-authored premise ("adds the release half; success path writes
nothing back") was **FALSE by code-trace** — resolved via G6 resolution (b)+(c):
re-scoped to the true residual (persist + observe) before filing, so the DF architect
does not re-implement an existing function and escalate at dispatch.

---

## ε — Anti-anchor the first architect (dark_factory, external-deps) — intermediate

Signal: the **first** architect plan-derivation input **excludes** `metadata.files`
(keeps the description/intent); derived `plan.files` is independent of the queue-time
set; **revalidation** passes are unaffected (C-A2).

| Capability | Evidence | Verdict |
|---|---|---|
| Architect plan-derivation input assembly (the hide-point) | `substrate:orchestrator/src/orchestrator/mcp/plan_tools.py:404 create_plan` / `:503 update_plan_metadata`; `BriefingAssembler.build_plan_tightening_prompt` `workflow.py:216`. **Exact field to suppress = ⚠️ confirm-at-impl** (PRD §6 / OQ#3) | **PASS-with-⚠️** (hide-point is a known tactical OQ, DF-owned; not a blocker) |
| First-derivation vs revalidation distinction (C-A2 — anti-anchor first only) | `grep:workflow.py:2146 revalidation = False` / `:2209 revalidation = True` + `build_revalidation_prompt` `:255` — the two paths are already distinguishable on main | **PASS** |
| Derived `plan.files` independent of queue-time set | `producer:task-ε` (DF builds the suppression) | **PASS** |
| Wired into a consumer (anti-orphan) | downstream `producer:task-ζ` (ε→ζ); ε is also what makes δ's existing release fire (un-dormants it) | **PASS** |

---

## ζ — Integration gate: converged behavior end-to-end (reify harness) — **leaf**

Signal: the §8 boundary-test table — guard rejects dir / accepts file+`[]` at **both**
sites; set-to-plan releases over-claim and a waiter dispatches; an under-declared task
BRE-acquires **before** edit; a BRE-fail still re-pends+revalidates (staleness
preserved); first architect anti-anchored. Drives the orchestrator; asserts via
fused-memory `get_scheduler_state`/`get_scheduler_events` + the submit path.

| Capability | Evidence | Verdict |
|---|---|---|
| Observation substrate (scheduler state + events + submit) | `substrate:mcp__fused-memory__get_scheduler_state` + `get_scheduler_events` (live MCP tools) + `submit_task` | **PASS** |
| Guard rejects dir at both sites (rows 1–3) | `producer:task-α` + `producer:task-γ` **upstream** (α→ζ, γ→ζ) | **PASS** |
| set-to-plan releases over-claim + waiter dispatches (rows 4–5) | `producer:task-δ upstream` (δ→ζ) + existing in-memory release `grep:scheduler.py:3442` | **PASS** |
| Under-declared task BRE-acquires before edit (row 6, no-regression) | `grep:scheduler.py:3440 try_acquire_additional` (existing BRE, wired-on-main) — **observed** (acquire precedes edit), not assumed | **PASS** |
| BRE-fail re-pends + revalidates (row 8, staleness preserved) | `grep:workflow.py:2451-2464 REQUEUED` + `_last_block_reason='plan_blast_radius_lock_conflict'` (wired-on-main) — **observed** firing | **PASS** |
| First architect anti-anchored (row 9) / revalidation exempt (row 10) | `producer:task-ε upstream` (ε→ζ) | **PASS** |
| Rejection-check (rows 1, 8) | ζ authors the dir-submit / staleness scenario and **observes** the reject / re-pend fire (producers α/γ/existing-BRE upstream) | **PASS** (observed) |
| Field-population / numeric-floor / grammar-fixture | **N/A** — scheduler-state + events, no `Value` field sampling, no numeric bound, no `.ri` grammar | N/A |

DAG-direction (anti-inversion): every producer (α, γ, δ, ε) is **upstream** of ζ — no
`producer-downstream` inversion.

---

## Summary

| Leaf | Repo | Bindings | Result |
|---|---|---|---|
| α | reify | predicate (self) · harness · rejection-check (self, observed) · wired (β,γ) | **PASS** |
| β | reify | α upstream · prd-skill substrate · block observed | **PASS** |
| γ | dark_factory | submit/commit_planning wired · α spec upstream · rejection (self) · wired (ζ) | **PASS** |
| δ | dark_factory | in-memory release **already wired** · acquire wired · **persist = producer δ** · observe · wired (ζ) | **PASS** (re-scoped) |
| ε | dark_factory | hide-point substrate (⚠️ confirm-at-impl) · first/reval distinction wired · producer ε · wired (ζ) | **PASS-with-⚠️** |
| ζ | reify | observation substrate · α/γ/δ/ε all upstream · no-regression observed · N/A numeric/grammar/field | **PASS** |

**No FAIL bindings → batch is clear to queue.** Two carried notes: (1) δ re-scoped from
its false PRD premise to the true residual (G6 resolution applied, PRD corrected); (2)
ε's exact hide-point field is a DF-owned confirm-at-impl tactical OQ (§6 ⚠️ / OQ#3),
not a substrate gap.
