# F-infra (audit cadence + tracking infra) design session — start prompt

Paste the block below into a fresh Claude Code session in this repo. Self-contained.

This is the **design** half of Leo's "design+implement session pair" for portfolio approach F. The implement session follows separately, only after the design lands and Leo approves it. Don't try to design + implement in one session — the surface is wide enough that they want separation.

F is **complementary** to (not overlapping with) the PRD-decomposition skill (approach A+D+H+E + grammar gate, designed in a separate session). The skill is upstream-of-orchestrator (catches gaps at PRD-authoring/decomposition time); F is downstream (catches gaps that slip past the skill at any later stage of the lifecycle).

---

## Paste this block

```
You are designing the audit-cadence + tracking infrastructure that
catches incomplete/ill-formed implementation chains AFTER the PRD-
decomposition skill (upstream) has done its work. This is portfolio
approach F — complementary to A (consumer-named) / D (user-observable
leaf) / G (corpus-level reviewer lint) / H (design-first contracts).

DESIGN ONLY in this session. Implementation is a separate session.

## DELIVERABLES

  a. A design document at `docs/architecture-audit/f-infra-design.md`
     covering:
       - The trigger surface (when does the audit fire — periodic,
         orchestrator-event-driven, /review-skill-invoked, all three)
       - The data substrate (what task metadata, what graph-walk
         invariants, what disk artifacts)
       - The invariants enforced (the catalog of "incomplete/ill-formed
         implementation chain" patterns to detect — Type A producer-
         orphan, Type B consumer-with-stub, Type C both-built-not-
         bridged)
       - The intervention vocabulary (alert / file follow-up task /
         escalation / refuse-merge / report-only — different gates
         likely warrant different levels)
       - Interaction with existing /review skill (does F extend
         /review or run separately?)
       - Interaction with the orchestrator (does the orchestrator
         block merges on F violations, or just report?)
       - The implementation cost budget (what's reasonable for one
         implementation session — probably should be scoped tight)
  b. A small follow-up note appended to
     `~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md`
     pointing at the design doc.
  c. NO implementation. The design doc terminates with a "Next
     session: implement" hand-off.

## REQUIRED READING (in order)

Audit context:
  1. docs/architecture-audit/README.md
  2. docs/architecture-audit/audit-brief.md (failure-mode catalog
     F1..F7 — F1..F4 are the patterns F-infra detects)
  3. docs/architecture-audit/phase-3-files-synthesis.md §1 + §5a
     ("the scaffold-without-a-caller pattern is endemic" — the
     scale F has to handle)
  4. docs/architecture-audit/phase-3-scaffold-pattern-critique.md
     §1.3 Type A/B/C decomposition (the categories F detects)

Leo's portfolio + policies:
  5. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
     (F is approach F; see Notes for its scope vs G + skill)
  6. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_naming.md
  7. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
     (the user-observable-leaf policy F enforces post-hoc)
  8. ~/.claude/projects/-home-leo-src-reify/memory/feedback_orchestrator_narrow_locks_favor_upfront_design.md

Existing infra (so F doesn't duplicate):
  9. List available skills via system reminders. Read these in detail:
       - review (the multi-phase review skill F may extend)
       - orchestrate (orchestrator dispatch; F may add hooks)
       - review-briefing (briefing schema; F may add fields)
 10. CLAUDE.md (project root — fused-memory usage, task routing)
 11. Sample task records via `mcp__fused-memory__get_task` for a
     few tasks (e.g. 3462, 3382, 2954) to understand what metadata
     fields exist today

## CONTEXT — what F catches (the failure-mode catalog)

These are the patterns F's invariants enforce. Each design decision
should map to "which pattern(s) does this catch":

  P1. **Type A producer-orphan.** Producer module / function /
      task closes with unit tests passing; no production caller
      exists. Detection signals: zero-callers grep, no
      consumer-side fixture, downstream PRDs that mention the
      mechanism without a wiring task.
  P2. **Type B consumer-with-stub.** Consumer-side code exists
      and references a name; producer-side is a stub or
      placeholder. Detection signals: hardcoded `Undef` returns,
      `TODO`/`unimplemented!`/`task_X_pending` markers, missing
      fields named in PRDs.
  P3. **Type C both-built-not-bridged.** Both halves exist;
      bridge missing. Detection signals: producer outputs not
      consumed in any downstream `Engine::*` path, two parallel
      taxonomies with no merge.
  P4. **Grammar-fiction surface.** PRD prose names DSL syntax
      that doesn't parse. Detection: grep PRD examples through
      tree-sitter-reify; if it fails to parse, flag the PRD.
  P5. **Phantom-done / found_on_main false-positive.** Task
      marked done but metadata.files don't reflect the cited
      work, or `done_provenance.kind=found_on_main` on a branch
      with zero commits diff. Detection: SQL-style query on
      `runs.db`.
  P6. **Contested seam ownership.** Two PRDs reference the same
      seam in their breadcrumbs without either claiming
      ownership. Detection: cross-reference scan across
      `docs/prds/**/*.md` + `docs/architecture-audit/findings/*.md`.
  P7. **PRD-vs-shipped drift.** PRD documents behavior X; code
      ships behavior Y. Cluster C-20 (MITC3+ vs bare-MITC3) was
      the canonical pre-fix instance. Detection: hardest — needs
      test-pass-band analysis or per-PRD assertion checking.

The design should make explicit choices about which patterns are
in scope for the first implementation slice (probably P1+P2+P5 are
the minimum useful set).

## OPEN QUESTIONS TO RESOLVE IN CONVERSATION

  Q-F-1. **Trigger granularity.** Per-commit (CI)? Per-PR
     (orchestrator merge gate)? Per-task closure (orchestrator
     done-flip)? Periodic (cron)? Some combination?

  Q-F-2. **Detector mechanism.** Static analysis? Runtime
     introspection? Task-metadata queries? Hybrid?

  Q-F-3. **Intervention type per pattern.** Refuse-merge is
     expensive but strong; alert-only is cheap but ignorable.
     Per-pattern severity calibration.

  Q-F-4. **Relationship to /review skill.** Does /review
     invoke F as a phase, or is F a separate sweep that
     contributes findings to /review's output? Either is
     reasonable — design choice.

  Q-F-5. **Task metadata schema additions.** What fields does
     F need at decompose time (user_observable_signal,
     named_consumer, gates_passed)? These fields are produced
     by the PRD-decomposition skill — F consumes them. Confirm
     contract with the skill's output.

  Q-F-6. **Graph-walk invariants.** Concretely: which
     invariants does F walk the dependency graph to verify?
     Examples: "every producer task has a consumer task in its
     downstream chain", "no task closes with `done_provenance.
     kind=found_on_main` unless metadata.files matches the diff".

  Q-F-7. **Storage of detected gaps.** Inline in gap-register?
     Separate `audit-findings/` directory? Time-series so we can
     see if regression rate is going up or down?

  Q-F-8. **First-slice scope.** What's the minimal implementable
     subset that delivers user-observable value (P1+P2 detection
     for tasks in pending+done state)? The implement session
     should target this minimum, not the full surface.

## CONVERSATIONAL STYLE

- Leo wants terse, technical responses.
- Use AskUserQuestion for crisp design choices.
- Push back if a design decision implies cross-crate work that
  starves under the orchestrator's narrow-lock model (per
  feedback_orchestrator_narrow_locks_favor_upfront_design — F's
  implementation itself shouldn't be an instance of the failure
  mode it's designed to prevent).

## SESSION END

Stop when:
  1. Design doc at `docs/architecture-audit/f-infra-design.md`
     covers the eight Q-F-* questions explicitly.
  2. First-slice scope (Q-F-8) is named with named-pattern
     coverage.
  3. Implementation session's expected duration + scope is
     estimated.
  4. Hand-back paragraph to Leo summarizing the design + the
     trigger to launch the implement session.

Do NOT:
  - Implement anything. This is design-only.
  - Modify gap-register.md, the contract, the structure-instance-
    runtime PRD, or any other audit artifact.
  - Touch G (corpus-level reviewer lint) design — that's a
    separate session (`g-reviewer-tool-design-session-prompt.md`).

Hard cap: 100k tokens.
```

---

## Notes for Leo

- Run any time after the PRD-decomposition skill design (item 1 in your portfolio) is reasonably stable. F consumes the metadata that skill produces, so the skill's output schema is an input to F's design.
- Expected session length: 60–90 minutes interactive.
- The implement session (separate prompt: would be `f-infra-implement-session-prompt.md` — generate when design is approved) follows.
- F + G together cover what the skill misses. Don't try to combine all three in one design — they're better-separated.
