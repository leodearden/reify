# Annotation-args PRD authoring session — start prompt

Paste the block below into a fresh Claude Code session in this repo. Self-contained.

This is the **unified** annotation-args PRD covering both surfaces flagged by the 2026-05-12 audit's grammar-fiction triage:

- **Flag-form** (`@allow(shadowing)`) — named-arg(s) for annotations; arg is a bare identifier/flag; no runtime evaluation. Needed by `shadowing-warning` PRD.
- **Runtime-evaluable form** (`@shell(thickness = linear_taper(...))`) — named-arg whose RHS is an expression evaluated at compile time or eval time. Needed by v0.5 `varying-thickness-shells` PRD.

Leo chose Option B (unified PRD) over flag-form-first or defer-both on 2026-05-12. This session designs both forms together.

---

## Paste this block

```
You are running an interactive PRD-authoring session with Leo. The goal:
author a unified annotation-args PRD covering both the flag-form surface
(@allow(shadowing)) and the runtime-evaluable surface (@shell(thickness =
linear_taper(...))) — the two grammar fictions surfaced by the 2026-05-12
architecture audit's grammar-fiction triage.

Currently the language accepts only `@name(string_literal)` annotation
args (e.g. @optimized("solver::elastic_static")). This PRD designs the
broadened surface.

## DELIVERABLES

  a. PRD at `docs/prds/annotation-args.md` (top-level under docs/prds/,
     not version-specific — this is a language-feature foundation
     spanning versions). Follow PRD conventions in `docs/prds/`;
     reference shape is `docs/prds/v0_3/compute-node-contract.md`.
  b. Decomposition DAG sketched in the PRD with each leaf naming its
     user-observable signal. Do NOT file tasks via fused-memory in
     this session — separate short filing session.
  c. Update affected PRDs to remove their "TBD when annotation-args
     designed" placeholders:
       - `docs/prds/shadowing-warning.md` — the @allow(shadowing)
         suppression syntax can now be referenced as designed
       - `docs/prds/v0_5/varying-thickness-shells.md` — the
         @shell(thickness = linear_taper(...)) form can now be
         referenced as designed (still v0.5-deferred, just no
         longer fiction-flagged)
  d. Update `docs/architecture-audit/gap-register.md`: find the GR
     entries for clusters C-06 (grammar fictions) AND any cluster
     entries whose grammar-fiction Notes pointed at annotation-args;
     update Notes to reference this PRD as the resolution.

## REQUIRED READING (in order)

Audit context:
  1. docs/architecture-audit/phase-3-grammar-fiction-triage-log.md
     (the triage report that surfaced this need; the "annotation-args
     expansion" cross-cutting flag)
  2. docs/architecture-audit/phase-3-files-synthesis.md §1 cluster
     C-06 (grammar-level fictions — annotation-args is one)
  3. docs/architecture-audit/gap-register.md (find the GR entries
     for C-06 + cross-references)

Leo's policies (CRITICAL):
  4. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
     (use approach H: design-first contract; flag-form is simple
     enough for approach B if you split it, but the unified PRD wants
     H for the runtime-evaluable half)
  5. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
  6. ~/.claude/projects/-home-leo-src-reify/memory/feedback_prd_grammar_gate.md
     (this PRD IS the grammar — its tasks must include the
     tree-sitter-reify production + parser test for both forms
     before any consumer task can claim done)
  7. ~/.claude/projects/-home-leo-src-reify/memory/feedback_orchestrator_narrow_locks_favor_upfront_design.md

Reference PRDs:
  8. docs/prds/v0_3/compute-node-contract.md — gold-standard PRD
     shape
  9. docs/prds/shadowing-warning.md — consumer for flag-form
 10. docs/prds/v0_5/varying-thickness-shells.md — consumer for
     runtime-evaluable form

GR-001 dependency:
 11. docs/architecture-audit/gap-register.md GR-001 §"Resolution"
     (Value::StructureInstance) — the runtime-evaluable form
     evaluates expressions producing Value-typed results; the PRD
     should align its semantics with the post-GR-001 Value model

Codebase grounding (on demand):
  - `tree-sitter-reify/grammar.js` — current annotation grammar
  - `crates/reify-compiler/src/annotations.rs` — current annotation
    compile-side; lines 64-130 accept @optimized, etc.
  - `crates/reify-compiler/src/types.rs:839` — CompiledFunction's
    optimized_target field as the existing single-arg precedent
  - Any existing `@shell` annotation handling for shell extraction

## CONTEXT — what's settled

- Both forms are in scope for this single PRD (Leo's 2026-05-12
  choice of Option B over flag-form-first or defer-both).
- Flag-form is the easier half; runtime-evaluable form is the
  language-shaping half.
- The runtime-evaluable form is bounded by GR-001's
  Value::StructureInstance design — annotation expressions evaluate
  to typed Values, not free-form expression trees.
- This PRD is foundational; many downstream PRDs will reference it.
  Naming, error semantics, and grammar shape matter more than
  implementation speed.

## OPEN QUESTIONS TO RESOLVE IN CONVERSATION

Argument syntax:
  Q-AA-1. **Positional vs named.** Currently @optimized takes one
     positional string. Does the new surface require positional,
     allow positional, or move to named-only? Forward-compat
     constraint: existing @optimized("...") sites must continue to
     parse.
  Q-AA-2. **Flag-form representation.** `@allow(shadowing)` —
     is `shadowing` a bare identifier? A path? A symbol?
     Forward-compat: should be a strict subset of the
     runtime-evaluable named-arg form so flag-form sites parse
     correctly under the broader grammar.

Runtime-evaluable form:
  Q-AA-3. **Expression evaluation timing.** Compile-time
     constant-fold? Eval-time when the annotated entity is
     instantiated? Per-call when the annotation is queried?
     Different choices have very different implications for
     @shell(thickness = linear_taper(z)) — does it evaluate once
     at shell-construction or per-element?
  Q-AA-4. **Scope capture.** Does the annotation expression see
     surrounding parameters, local bindings, or only top-level
     names? Closure semantics?
  Q-AA-5. **Type discipline.** Is the RHS expression type-checked
     against an expected type the annotation declares? Or is it
     free-form Value?
  Q-AA-6. **Error semantics.** Annotation arg eval errors —
     compile error, runtime error, or warning + ignore?
  Q-AA-7. **Value model alignment.** The RHS expression produces
     a Value (post-GR-001 likely Value::StructureInstance or
     ordinary numeric Value). Confirm the contract with
     structure-instance-runtime PRD if it's authored by session
     time; otherwise sketch the expected shape and flag the
     dependency.

Composition with existing surface:
  Q-AA-8. **Unified dispatch.** The existing @optimized("string")
     surface is a special case of named-or-positional. Should the
     PRD unify into one annotation-args grammar rule with
     constraints (e.g. @optimized accepts only string), or
     introduce a second grammar production for the broader form?
  Q-AA-9. **Existing call sites.** @optimized, @shell, @sandbox,
     @allow, @kernel — survey their current grammar acceptance +
     intended argument shapes. Some sites are already in PRDs;
     this PRD should land or supersede those acceptances.

Grammar shape:
  Q-AA-10. **Tree-sitter production.** Reuse existing expression
     nonterminals on the RHS, or define a restricted
     `annotation_expression` to bound complexity? Reusing is
     simpler; restricting catches misuse earlier.
  Q-AA-11. **Lowering target.** What's the IR representation of
     an annotation with args? Map<String, Expr>? Vec<(Name, Expr)>?
     Typed per-annotation struct? Affects every consumer.

Implementation slicing:
  Q-AA-12. **First slice scope.** Probably: grammar + parser test
     + flag-form lowering + flag-form consumer (shadow-warning
     suppression) end-to-end. Runtime-evaluable form ships as
     follow-up tasks. Confirm.
  Q-AA-13. **Migration of existing @optimized.** Does this PRD
     rewrite @optimized's argument handling, or leave it as a
     legacy single-string-arg path? Forward-compat says rewrite,
     but the cost is real.

## CONVERSATIONAL STYLE

- Leo wants terse, technical responses.
- Use AskUserQuestion for crisp design choices.
- Push back if a design decision would make the runtime-evaluable
  form unimplementable on top of GR-001's Value::StructureInstance.
- This is a foundational language-feature PRD. Get naming + grammar
  shape right; implementation can iterate.

## DECOMPOSITION DAG

The PRD's §"Decomposition" should sketch a DAG of leaf tasks under
approach B+H + D. Suggested anchors (refine in conversation):

  - **Foundation:** tree-sitter-reify grammar production for the
    unified annotation-args form + parser test fixtures covering
    flag-form, named-string (existing @optimized), and (deferred)
    named-expression. Leaf signal: `tree-sitter-reify parse` succeeds
    on the fixtures.
  - **Compile-lowering:** AST → IR; existing @optimized continues
    to lower; new flag-form lowers to its representation. Leaf:
    round-trip test of @allow(shadowing) preserved through compile.
  - **Flag-form consumer wire:** shadow-warning suppression machinery
    reads the lowered flag-form. Leaf: a .ri file with @allow(shadowing)
    silences the linter for the annotated entity; verified by
    integration test.
  - **Runtime-evaluable form (deferred to v0.5 slice):** named-
    expression lowering + eval-time evaluation + scope capture +
    Value::StructureInstance integration. Multiple leaves, each
    referencing GR-001's structure-instance-runtime PRD.
  - **Boundary tests:** producer-side (compile lowers correctly) +
    consumer-side (annotations queryable from runtime). Sketch which
    crates host the tests.

Each leaf names its user-observable signal in description.

## SESSION END

Stop when:
  1. PRD at `docs/prds/annotation-args.md` covers all 13 Q-AA-*
     questions explicitly.
  2. Decomposition DAG sketched with user-observable signals per
     leaf; first-slice scope clearly named.
  3. `docs/prds/shadowing-warning.md` and
     `docs/prds/v0_5/varying-thickness-shells.md` updated to remove
     their "TBD" placeholders.
  4. gap-register.md cross-references updated.
  5. Leo approves the PRD.
  6. Hand-back paragraph summarizing the next move (commit the PRD;
     short session to file the DAG tasks; runtime-evaluable form
     remains v0.5-deferred but no longer fiction-flagged).

Do NOT:
  - File any tasks via fused-memory in this session.
  - Edit code under crates/.
  - Commit unless Leo explicitly asks.
  - Make breaking changes to existing @optimized acceptance
    without an explicit migration plan in the PRD.

Hard cap: 200k tokens. If running long, write hand-off note at
`docs/architecture-audit/annotation-args-session-handoff.md`.
```

---

## Notes for Leo

- This PRD is foundational and forward-compatible-sensitive — getting names + grammar shape right matters more than implementation speed.
- Can run independently of structure-instance-runtime, but the runtime-evaluable form's leaves will reference Value::StructureInstance; if structure-instance-runtime hasn't landed yet, this PRD sketches the dependency and continues.
- Expected session length: 90–150 minutes interactive (foundational design surfaces tend to surface more sub-questions than fix-now sessions).
- After this session: commit the PRD, run a short filing session for the first-slice DAG tasks (flag-form ships; runtime-evaluable remains v0.5-deferred).
