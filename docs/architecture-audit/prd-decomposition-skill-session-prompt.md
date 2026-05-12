# PRD-and-decomposition skill design+implement session — start prompt

Paste the block below into a fresh Claude Code session in this repo. It is self-contained: the session agent reads the cited audit artifacts + skill conventions, then runs an interactive design+implement session with Leo.

The output of this session is a working Claude Code skill that bakes the audit-derived portfolio (approaches A, D, H, E + the grammar gate) into the PRD-authoring + decomposition workflow.

---

## Paste this block

```
You are designing and implementing a Claude Code skill that operationalizes
the systematic preventions Leo adopted from the 2026-05-12 Reify architecture
audit. The audit found that 19 of 44 mechanism clusters fit the "incomplete/
ill-formed implementation chain" pattern; the most upstream defence is
discipline at PRD-authoring and decomposition time, applied before any
task reaches the orchestrator.

Currently the policies are recorded as memories (advice to future-me) but no
skill, tool, or template enforces them. This session changes that.

## DELIVERABLES (in this order)

1. **Skill design:** a written specification of the skill — trigger
   surface, when it activates, what reads it does, what gates it applies,
   what artifacts it produces, how it interacts with the existing skill
   set (review, review-briefing, orchestrate, init).
2. **Skill implementation:** the skill itself at the appropriate path
   under `.claude/skills/` (verify by exploring; the repo may have its
   own conventions distinct from `~/.claude/skills/`). Skill file
   structure follows existing repo conventions; if there are reference
   files / checklists / templates, place them appropriately.
3. **Test run:** apply the skill to ONE example — either a real queued
   PRD pending decomposition OR a hypothetical short PRD authored
   specifically as a test. The test demonstrates the skill catching at
   least two of the audit-relevant failure modes (e.g. an unstated
   consumer, a leaf task without user-observable signal, a grammar
   surface that doesn't parse).
4. **Memory entry:** if the skill establishes a workflow Leo will rely
   on going forward, add a procedural memory pointing at it.

## REQUIRED READING (in order — do not skim)

Audit foundation:
  1. docs/architecture-audit/README.md
  2. docs/architecture-audit/audit-brief.md  (failure-mode catalog)
  3. docs/architecture-audit/phase-3-files-synthesis.md  (esp. §2
     Pattern 1, §5 surprises — to understand what the skill is
     defending against)
  4. docs/architecture-audit/phase-3-scaffold-pattern-critique.md
     (Type A/B/C decomposition; the menu of seven approaches — the
     skill operationalizes a SUBSET of these)
  5. docs/architecture-audit/phase-3-breadcrumb-map.md  (§3 contested-
     ownership pairs — to understand where seam-ownership matters)

Leo's adopted policies (CRITICAL — these are what the skill enforces):
  6. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
     (the eight-approach portfolio; A/D/H/E are the ones the skill
     bakes in)
  7. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_naming.md
     (use "incomplete/ill-formed implementation chain" terminology in
     the skill)
  8. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
     (approach D — every leaf task names a user-observable signal)
  9. ~/.claude/projects/-home-leo-src-reify/memory/feedback_prd_grammar_gate.md
     (grammar/parser/lowering verified before any task in a PRD claims
     done)
 10. ~/.claude/projects/-home-leo-src-reify/memory/feedback_orchestrator_narrow_locks_favor_upfront_design.md
     (priority-graded breadth dynamics; cross-crate work needs either
     interactive landing OR high/critical priority wide-lock task)
 11. ~/.claude/projects/-home-leo-src-reify/memory/feedback_commit_prds_before_referencing_tasks.md
     (PRDs must be on disk before tasks reference them)
 12. ~/.claude/projects/-home-leo-src-reify/memory/feedback_planning_mode_scope.md
     (when to use planning_mode for task batches)

Existing skill conventions:
 13. List skills available with whatever Claude Code provides
     (system reminders enumerate user-invocable skills). Read 2-3 of
     the more elaborate ones for structure — likely candidates:
       - review (multi-phase deep review)
       - review-briefing (briefing maintenance)
       - orchestrate (PRD → orchestrator dispatch)
       - init (CLAUDE.md scaffolding)
     Look at any reference files those skills load (under
     `.claude/skills/<name>/references/*.md` or equivalent).
 14. CLAUDE.md (project root) — for the project's own conventions
     about memory writes, fused-memory usage, task routing.

Sample PRDs for grounding (read for shape, not adherence):
 15. docs/prds/v0_3/compute-node-contract.md  (the gold standard PRD,
     just landed 2026-05-12 — it embodies approach H end-to-end and
     is the reference shape the skill should produce)
 16. docs/prds/v0_3/structural-analysis-fea.md  (a typical accreted
     PRD that exhibits the failure modes the skill prevents)
 17. docs/prds/v0_3/mesh-morphing.md  (a PRD whose breadcrumbs
     explicitly opt out of certain mechanisms — the skill should
     surface this kind of cross-PRD intent)

## CONTEXT — what's already settled

- Audit terminology: "incomplete/ill-formed implementation chain"
  (NOT "scaffold without a caller").
- The skill operationalizes a SUBSET of the portfolio:
    **A** (consumer-first PRD section)
    **D** (user-observable leaf — already a hard policy via memory)
    **E** (cross-PRD seam ownership when load-bearing — conditional)
    **H** (design-first / interface contracts / two-way boundary tests
           when architectural simplicity is low or stakes are high)
  Plus the grammar gate from `feedback_prd_grammar_gate.md`.
  Plus **C-as-integration-gate** as a task-shape template (the DAG
  pattern: integration-gate task with prereqs on producer + consumer).

- NOT in this skill's scope:
    F (audit cadence + tracking infra — separate design+implement
       session pair)
    G (lint-level production-caller gate — global reviewer tool,
       runs at corpus / review-skill level, not PRD-authoring level)
  These can be planned as follow-ups in the session's hand-back, but
  not implemented here.

- This is a Reify-specific skill (lives in the repo's `.claude/skills/`
  rather than `~/.claude/skills/`). Verify by exploring the repo for
  existing skills before placing.

- Solo OSS context with the dark-factory orchestrator: the skill is
  invoked interactively by Leo+session during PRD authoring or
  decomposition, NOT by automated CI hooks. It's a gate of the form
  "you walked into the room — here's the checklist."

## SKILL DESIGN — open questions to resolve in conversation

These are the design questions the session should converge on
collaboratively. Don't decide alone:

  Q-SKILL-1. **Trigger surface.** Auto-activate on edits under
     `docs/prds/`? Slash command (`/prd`, `/decompose`)? Both?
     What's the failure mode if it doesn't trigger (silent skip)?

  Q-SKILL-2. **Skill scope.** One skill covering both PRD-authoring
     and decomposition? OR two skills (`prd-authoring`,
     `prd-decomposition`)? They share most policies but the
     decomposition skill cares more about the leaf-observability
     check and task DAG shape; authoring cares more about consumer-
     identification and grammar-gate.

  Q-SKILL-3. **Output format.** When the skill detects a gap, does
     it (a) refuse to proceed until fixed, (b) prompt for resolution
     and continue, or (c) produce a warning report Leo reads?
     Different gates may warrant different levels.

  Q-SKILL-4. **Interaction with orchestrator.** Does the skill
     produce task metadata fields the orchestrator should respect
     (e.g. a "user_observable_signal" field per task)? If so, that
     metadata needs to be respected at orchestrator-side too — but
     that's F-infra territory and out of this session's scope. The
     skill can WRITE the field; the orchestrator-side READ is a
     follow-up.

  Q-SKILL-5. **Grammar-gate verification mechanism.** Does the
     skill literally try to parse a fixture through
     tree-sitter-reify, or does it ask Leo to confirm? The former
     is more rigorous but more brittle.

  Q-SKILL-6. **PRD template.** Should the skill author a PRD
     template / scaffolding file the user fills in? Or read the
     finished PRD and apply gates? Both?

## SKILL CONTENT — the gates it must apply

Whatever shape the design lands on, the skill must apply at
minimum these gates per PRD authored or decomposed under it:

  G1. **Consumer named.** Every mechanism in the PRD that produces
      a value, struct, fn, or syntax names at least one consumer
      (which other PRD or which user-observable surface consumes
      it). If no consumer can be named, the PRD is incomplete by
      construction; raise it.

  G2. **User-observable leaf.** When the PRD is decomposed, every
      leaf task names the user-observable signal that proves
      completion. Intermediate tasks state which downstream
      prerequisites they unlock. Producer-only tasks with no
      downstream chain are not acceptable.

  G3. **Grammar gate.** For every novel syntax surface the PRD
      assumes, confirm: tree-sitter-reify production exists OR
      parser test exists OR lowering wire is real. If any missing,
      either rewrite the assumed syntax to use existing grammar OR
      queue the grammar work as an explicit prerequisite in the
      task DAG.

  G4. **Seam ownership (conditional).** If the PRD names a
      cross-PRD seam (mechanism that lives between two PRDs), an
      owner is named. Reciprocal references where both PRDs treat
      the seam as the OTHER's responsibility must be resolved
      before the PRD can claim coverage.

  G5. **Approach H trigger (conditional).** For PRDs whose
      mechanism count exceeds a threshold OR whose cross-PRD
      blast radius exceeds a threshold (heuristic — propose
      values), require a contract document + boundary test sketch
      before tasks are queued. Otherwise approach B (vertical
      slice) is acceptable.

These gates can be encoded as a checklist the skill walks through,
as references the skill loads, as prompts the skill emits, or some
combination. Design choice.

## CONVERSATIONAL STYLE FOR THIS SESSION

- Leo wants terse, technical responses. No preamble, no apologies.
- Present option menus for design questions; do NOT recommend a
  single answer unless analysis genuinely converges.
- Push back if Leo's framing has an unstated assumption you can
  detect.
- Use AskUserQuestion for crisp 2-4 way option menus where the
  choice is genuinely independent of other context.
- This is a design + implement session — both phases need real
  artifacts. Don't stop at design.

## TEST RUN — what to demonstrate

When the skill is implemented, pick one of:
  - A real queued PRD from `docs/prds/` (one whose decomposition is
    still pending — check fused-memory for PRDs that haven't been
    decomposed yet)
  - The structure-instance-runtime PRD (which Leo plans to author
    next anyway — this session can produce a draft outline using
    the skill, leaving completion to the dedicated authoring
    session)
  - A small hypothetical PRD authored as part of the test

Demonstrate the skill catching AT LEAST two failure modes
(unnamed consumer, leaf without observable signal, grammar
fiction, unowned seam). Capture the test run output in the
session hand-back.

## SESSION END

Stop when:
  1. The skill is implemented at the agreed-upon path under
     `.claude/skills/` (or whatever the repo convention is).
  2. The test run demonstrates the skill catching real failure
     modes; output captured in hand-back.
  3. Memory entry written if the skill introduces a workflow
     Leo will rely on (likely yes; under
     `~/.claude/projects/-home-leo-src-reify/memory/`).
  4. Hand-back paragraph to Leo summarizing: what shipped, what
     was deferred, what F/G follow-up sessions are now visible.

Do NOT:
  - Implement F (audit cadence + tracking infra) — separate session.
  - Implement G (corpus-level lint) — separate session.
  - Modify existing PRDs.
  - File any tasks via fused-memory (the skill itself enforces task
    discipline; this session is meta — implementing the enforcer).
  - Commit unless Leo explicitly asks.

If the session runs long and you hit ~150k tokens of your own
context, write a hand-off note at
`docs/architecture-audit/prd-decomposition-skill-session-handoff.md`
capturing what's decided, what's open, the next agent's required
reading. Then stop and tell Leo to start a fresh session.

Hard cap: 200k tokens. Plan accordingly.
```

---

## Notes for Leo

- Self-contained block; paste into a fresh Claude Code session running in `/home/leo/src/reify`.
- Session is design + implement — expect 1.5–3 hours interactive depending on how rigorous you want G3 (grammar-gate verification) to be.
- The session does NOT touch F (audit cadence + tracking infra) or G (global reviewer lint) — those are separate session pairs visible in your portfolio. If you want prompts for those, ask.
- The skill produced by this session should make future PRD-authoring sessions cheaper and lower-defect: the gates run automatically rather than relying on me/future-me remembering the policy.
- If the skill's design lands but you want to defer the implementation, the hand-off mechanism in the prompt covers that.
