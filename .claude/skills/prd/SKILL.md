---
name: prd
description: "Author and decompose Reify PRDs under the 2026-05-12 audit-derived gates that prevent incomplete/ill-formed implementation chains. ALWAYS use this skill for: /prd commands, authoring a new Reify PRD, decomposing a committed PRD into tasks, queueing tasks from a PRD into the orchestrator. Triggers on requests like 'let's write a PRD for X', 'draft a PRD', 'decompose this PRD', 'queue tasks from <prd>.md', or any mention of starting/finishing PRD-shaped work in the Reify repo. Walks G1 (consumer named), G2 (user-observable leaf signal), G3 (grammar verified), G4 (cross-PRD seam ownership), G5 (design-first when stakes are high), G6 (premise validity — signals' numeric/exactness/capability premises are true and achievable), and the meta-gate 'would this PRD produce a complete/coherent/cohesive/good design under decompose-and-queue without further oversight?' before saving or queueing. This is NOT for: editing existing PRDs without re-running gates, running tasks (use /orchestrate), reviewing landed code (use /review), unblocking tasks (use /unblock)."
---

# PRD Authoring + Decomposition (Reify)

The skill operationalizes the 2026-05-12 architecture audit findings: ~19/44 mechanism clusters fit the **incomplete/ill-formed implementation chain** pattern (see memory `preferences_implementation_chain_naming`). The audit's dominant prevention is discipline at PRD-authoring and decomposition time, applied before any task reaches the orchestrator. This skill is that discipline.

The portfolio approaches baked in here are **A** (consumer-first), **D** (user-observable leaf), **E** (cross-PRD seam ownership, conditional), **H** (design-first / contracts / two-way boundary tests, conditional) plus the grammar gate. **C-as-integration-gate** is the task-DAG-shape template the decompose mode produces. See memory `preferences_implementation_chain_portfolio`.

`F` (audit cadence + tracking infra) and `G` (corpus-level reviewer lint) are out of scope here — separate future session pairs.

## Modes

Pick from context — bare `/prd` invocation:

- No PRD exists yet for the topic at hand → **author mode**.
- PRD is committed at `docs/prds/...` and tasks not yet queued → **decompose mode**.
- Both apply (e.g. author finished, session has room to continue) → confirm with Leo before transitioning.

### Author mode

A conversational design session that ends with a committed PRD on disk. The PRD is the **output** of the conversation, not a template to fill in. The skill drives discussion through the gates, surfaces design choices, helps resolve them, then writes + commits.

The session is complete when this can be answered "yes":

> If I decompose and queue this PRD without further oversight, will the architecture of what gets implemented be complete, coherent, cohesive, and **good**?

No open **design** questions remain at PRD-save time. Tactical / implementation-time open questions are fine and go in a `## Open questions` section.

See `references/author-mode.md`.

### Decompose mode

Read a committed PRD, re-walk gates against it, then file the whole task batch via fused-memory `submit_task` with **`planning_mode=True` on every task, no exceptions** (synchronous, curator-bypassing; lands them as `deferred`, returns `task_id` directly — no `resolve_ticket` round trip). After the batch is filed, wire **all** dependencies, then flip the **entire batch** from `deferred` to `pending` in a single bulk `set_task_status` call. Fused-memory owns persistence — no commit step.

See `references/decompose-mode.md`.

## Gates

Each gate has a calibrated response level. See `references/gates.md` for what each catches (with audit cluster references) and the exact application algorithm.

| Gate | What | Level |
|---|---|---|
| **G1** | Consumer named for every mechanism introduced (which other PRD or user surface consumes it) | **block** |
| **G2** | Every leaf task names a user-observable signal proving completion | **block** (decompose only) |
| **G3** | Novel syntax verified to parse OR queued as explicit prerequisite | **block** |
| **G4** | Cross-PRD seams have a named owner; reciprocal "the other owns it" patterns resolved | **prompt** |
| **G5** | High-stakes or architecturally-complex PRDs use approach **B + H** (contracts + boundary tests) rather than bare B | **prompt with heuristic** |
| **G6** | Every signal asserting a number/exactness/end-to-end capability has its premise validated — achievable, true, and producible from the task's own dependency set | **block** |
| **META** | The "yes" question above | **block** at PRD save |

- `block` — the phase cannot complete until the gap is resolved.
- `prompt` — surface the gap, Leo decides.
- `prompt with heuristic` — propose default behaviour with reasoning, Leo confirms or overrides.

## Grammar-gate mechanics

Try-parse-then-confirm. Extract every novel-looking syntax fragment from the PRD prose, write each as a small `.ri` fixture, run `tree-sitter parse --quiet <fixture>` from the `tree-sitter-reify/` directory. Exit 0 = pass. Exit 1 (CST contains `ERROR` nodes) = fail. On fail or ambiguous extraction, ask Leo whether to (a) rewrite the PRD prose to use existing grammar or (b) queue grammar work as an explicit prerequisite task in the DAG.

See `references/grammar-gate.md` for fixture-extraction heuristics and the exact command.

## Outputs

**Author mode:**
- A saved PRD at `docs/prds/<vM_N>/<slug>.md` (path elicited in conversation; `<vM_N>` is the milestone directory like `v0_3`, `v0_4`, `v0_5`; root-level `docs/prds/` for version-agnostic foundations).
- Section structure follows the audit-derived shape (match by content, not literal numbering): consumer + user-observable surface; sketch of approach; pre-conditions; resolved design decisions; out of scope; cross-PRD relationship + seam-owner table; decomposition plan (one bullet per task naming its observable signal); open (tactical) questions.
- **Committed to git** in the same skill turn, per memory `feedback_commit_prds_before_referencing_tasks`.

**Decompose mode:**
- A batch of tasks filed via `submit_task` with `planning_mode=True` (always, every task). Each carries metadata fields:
  - `user_observable_signal` (string): the CLI/viewport/LSP/example-`.ri` signal proving completion.
  - `consumer_ref` (string): the PRD or user surface that consumes this task's output.
  - `grammar_confirmed` (bool): true iff the task's mechanism uses existing grammar, false if it queues grammar work.
- All declared dependencies (intra-batch and out-of-batch, including cross-PRD per memory `preferences_cross_prd_deps_real_edges`) wired via `add_dependency` while the batch is still `deferred`.
- The whole batch flipped `deferred` → `pending` together in a single bulk `set_task_status` call — never one-at-a-time.
- Fused-memory owns task persistence: no git commit, no on-disk artifact to manage.
- The orchestrator does **not** currently read the `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata fields. They are substrate for the F-infra follow-up session; surface this in the hand-back when decompose mode finishes.

## Gold-standard exemplar

`docs/prds/v0_3/compute-node-contract.md` is the reference shape: §0 supersession + cross-PRD reference, §1 GR-001 link, §2–§6 contract sections, §7 boundary-test sketch facing both ways (the H component), §8 vertical-slice DAG with per-leaf observable signals, §9 open (tactical) questions. New PRDs need not match its numbering literally but should match it conceptually.

## Conversational style

Terse, technical. No preamble. Surface design choices as 2–4 way option menus via `AskUserQuestion` when the choice is genuinely independent of context; otherwise raise the question inline. Push back if Leo's framing has an unstated assumption. Do not recommend a single answer unless analysis genuinely converges.

## Anti-triggers

- Editing an existing PRD without re-running gates → not this skill. If the edit changes a load-bearing mechanism, run `/prd` author for a fresh design pass.
- Running tasks → `/orchestrate`.
- Reviewing landed code → `/review`.
- Resolving blocked tasks → `/unblock`.
- Authoring `.ri` design files (parametric parts/assemblies) → `/reify-design`.

## Related memories

- `preferences_implementation_chain_portfolio` — the 8-approach portfolio.
- `preferences_implementation_chain_naming` — terminology.
- `feedback_task_chain_user_observable` — G2 source.
- `feedback_prd_grammar_gate` — G3 source.
- `feedback_orchestrator_narrow_locks_favor_upfront_design` — why G5 tilts toward H.
- `feedback_commit_prds_before_referencing_tasks` — author commits before decompose references.
- `feedback_planning_mode_scope` — why decompose uses planning_mode=True.
- `procedural_fused_memory_two_phase_writes` — submit_task + resolve_ticket pattern (applies to **non-**planning_mode only; decompose mode's planning_mode=True path is synchronous and skips the ticket round trip).
- `preferences_bookmark_task_pattern` — bookmark/deferred-batch lifecycle.

## Audit foundation

- `docs/architecture-audit/README.md` — three-phase shape, audit motivation.
- `docs/architecture-audit/audit-brief.md` — failure-mode catalog (F1–F7).
- `docs/architecture-audit/phase-3-files-synthesis.md` — cluster table; §2 Pattern 1, §5 surprises.
- `docs/architecture-audit/phase-3-scaffold-pattern-critique.md` — Type A/B/C decomposition + the seven approaches.
- `docs/architecture-audit/phase-3-breadcrumb-map.md` — §3 contested-ownership pairs.
- `docs/architecture-audit/gap-register.md` — GR-IDs the skill may cite at G4 / META time.
