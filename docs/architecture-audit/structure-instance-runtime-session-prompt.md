# structure-instance-runtime PRD authoring session — start prompt

Paste the block below into a fresh Claude Code session in this repo. Self-contained: the session agent reads the GR-001 resolution + adjacent context, then runs an interactive PRD-authoring session with Leo, producing a PRD on disk and a decomposition DAG.

The design is **already settled** by the 2026-05-12 GR-001 resolution. This session's job is to author the formal PRD and decompose it cleanly under the new policies — not to re-litigate Option B.

---

## Paste this block

```
You are running an interactive PRD-authoring session with Leo. The goal:
author the GR-001 follow-up PRD that operationalizes Option B (typed
`Value::StructureInstance` variant + nominal conformance everywhere +
existing builtin-dispatch ctors rewritten as stdlib structure_defs).

The GR-001 design is settled. This session's job is to author the PRD
artifact + a clean decomposition that satisfies the new
feedback-task-chain-user-observable policy.

## DELIVERABLES (in this order)

  a. PRD at `docs/prds/v0_3/structure-instance-runtime.md`. Follow PRD
     conventions in `docs/prds/` (similar shape to
     `docs/prds/v0_3/compute-node-contract.md` — the gold standard).
  b. A decomposition DAG sketched in the PRD's §"Decomposition" (or
     equivalent), with each leaf naming its user-observable signal.
     Do NOT file tasks via fused-memory in this session — Leo will
     do that in a short follow-up session.
  c. Update `docs/architecture-audit/gap-register.md`:
     - GR-001 entry: add a `### Follow-up PRD: structure-instance-runtime.md`
       sub-section pointing at the file
     - GR-011 (Load/Support kind-tagged Maps vs trait-typed structs) and
       GR-019 (Material starter library) and GR-031 (composed stress
       recovery) entries: update their Notes to point at this PRD as the
       resolution mechanism

## REQUIRED READING (in order)

GR-001 settled design + audit context:
  1. docs/architecture-audit/gap-register.md (GR-001 §"Resolution" — the
     authoritative design statement; GR-011, GR-019, GR-031 entries —
     the downstream clusters this PRD covers)
  2. docs/architecture-audit/audit-brief.md ("Things to take as given"
     section, esp. nominal trait system + Solid as Type::Geometry +
     snake_case/PascalCase ctor inconsistency)
  3. docs/architecture-audit/phase-3-files-synthesis.md §1 cluster
     C-01, C-08, C-16, C-29 (the cluster details this PRD covers)
  4. docs/architecture-audit/phase-3-scaffold-pattern-critique.md
     (§1.3 sub-shapes — this PRD addresses Type A producer-orphan at
     the language layer)

Leo's adopted policies (CRITICAL):
  5. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_naming.md
  6. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
     (use approach H: design-first / contracts / two-way boundary
     tests for the Value::StructureInstance + match-site adapter
     contract)
  7. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
     (every leaf in the §Decomposition DAG names its user-observable
     signal)
  8. ~/.claude/projects/-home-leo-src-reify/memory/feedback_prd_grammar_gate.md
     (struct-ctor call syntax already parses per task 2039; verify
     and reference)
  9. ~/.claude/projects/-home-leo-src-reify/memory/feedback_commit_prds_before_referencing_tasks.md
     (the PRD must be committed before §8 DAG filing session — that
     dependency is between this session and the next)
 10. ~/.claude/projects/-home-leo-src-reify/memory/feedback_orchestrator_narrow_locks_favor_upfront_design.md
     (the Value::StructureInstance variant addition + adapter sweep
     spans many files — design accordingly)

Reference PRD shape:
 11. docs/prds/v0_3/compute-node-contract.md — the gold-standard PRD
     just landed 2026-05-12; mirror its section structure where
     applicable

Codebase grounding (read on demand, not upfront):
  - `crates/reify-eval/src/value.rs` (current Value enum — where the
    new variant goes)
  - `crates/reify-eval/src/engine_eval.rs:114-125` (the current
    struct-ctor → Undef site GR-001 names; verify state hasn't
    changed since the audit)
  - `crates/reify-types/src/type_resolution.rs:513` (Solid alias,
    nominal-conformance machinery)
  - `crates/reify-compiler/stdlib/*.ri` (existing builtin-dispatch
    constructors that will be rewritten — point_load, FixedSupport,
    PressureLoad, etc.)
  - `crates/reify-eval/src/cache.rs` (persistent cache key
    composition — needs adapter for the new variant)

## CONTEXT — what's already settled (do not relitigate)

- **Option B chosen** for GR-001 disposition 2026-05-12. Option A
  (Map-convergence) and Option C / hybrids (structural conformance
  for kind-tagged Maps) were considered and rejected. Rationale
  recorded in gap-register.md GR-001 §"Resolution". Do not re-open.
- **Nominal trait conformance stays.** No structural-shape
  admission is introduced — even for Value::StructureInstance.
  `structure_def Foo : TraitName { ... }` remains the explicit
  locus of author intent.
- **PascalCase sweep is in scope.** Existing snake_case ctors
  (`point_load`) become PascalCase (`PointLoad`) when rewritten as
  stdlib structure_defs. Aligns with the existing PRD-corpus
  convention.
- **Builtins → stdlib structure_defs.** The Rust-side
  builtin-dispatch entry points (`point_load`, `FixedSupport`,
  `PressureLoad`, etc.) are rewritten as `.ri` `structure_def`
  declarations producing the new variant. The language describes
  itself.
- **Value::Map stays.** Genuinely-map-shaped data (e.g.
  `Map<String, ElasticResult>` for multi-case results, dictionary
  config data) continues to be Value::Map. The two shapes are
  linguistically distinguishable.
- **ComputeNode trampoline adapts.** The compute-node-contract
  (committed at d2cfe40980) names that the trampoline signature
  must handle Value::StructureInstance arms during dispatch. This
  PRD doesn't change the contract; it produces the variant the
  contract anticipates.

## OPEN QUESTIONS THAT BELONG IN THIS PRD (to resolve in
conversation)

  Q-SIR-1. **Exact StructureInstance shape.** Settled by GR-001 to
     `{ type_id: StructureTypeId, fields: PersistentMap<String, Value> }`.
     But: does `type_id` carry a version (for migration purposes)?
     A source location (for diagnostics)? A trait-bound set
     (precomputed for fast `satisfies_trait_bound` lookup)? Or is it
     a pure opaque ID with everything else looked up in a side table?
     This affects the size of every Value in memory.

  Q-SIR-2. **Match-site adapter sweep.** Every `match value`
     site in the codebase needs an arm for the new variant. Survey
     the count; design a migration shape that the orchestrator can
     run as a single wide-lock task (per
     `feedback_orchestrator_narrow_locks_favor_upfront_design.md` —
     this is a cross-crate refactor that must be high/critical
     priority OR landed interactively).

  Q-SIR-3. **Persistent cache key composition.** A struct
     instance's serialization must be deterministic and stable across
     sessions. Field-key ordering, type_id stability under stdlib
     edits, version migration — propose the contract.

  Q-SIR-4. **value_type_kind_matches enrichment.** Currently
     does kind-tag inspection on Maps. For StructureInstance, exact
     type_id check suffices. But: does the trait conformance check
     still consult declared bounds (`structure_def : TraitName`) at
     this site, or earlier?

  Q-SIR-5. **Migration / rollout.** Two ways to land: (1) ship the
     variant + all adapters in one merge, then rewrite builtins as a
     second wave; (2) ship variant + adapters + ONE builtin rewrite
     end-to-end as a vertical slice (approach B+H), then sweep the
     rest. The portfolio favors (2). Confirm.

  Q-SIR-6. **Naming sweep scope.** Which existing builtin-dispatch
     ctors get rewritten in the initial slice? Probably the smallest
     useful subset (one Load + one Support? `Steel_AISI_1045` as the
     headline GR-001 case?). The rest in follow-up tasks.

  Q-SIR-7. **examples/structure-instance.ri.** The PRD names an
     example file demonstrating runtime user-observable construction.
     Sketch what it contains.

## CONVERSATIONAL STYLE

- Leo wants terse, technical responses.
- Present option menus for design questions; do NOT recommend a
  single answer unless analysis genuinely converges.
- Push back if Leo's framing has an unstated assumption you can detect.
- Use AskUserQuestion for crisp 2-4 way option menus.
- The PRD's prose is the durable artifact — write it carefully.

## DECOMPOSITION DAG

The PRD's §"Decomposition" should sketch a DAG of leaf tasks under
approach B+H + D. Suggested anchors (refine in conversation):

  - **Foundation:** add Value::StructureInstance variant + all
    match-site adapters in one wide-lock task. Leaf: workspace
    cargo test passes; one unit test constructs a StructureInstance
    directly via a Rust-API path.
  - **Compile-lowering:** struct-ctor calls in .ri source lower to
    Value::StructureInstance. Leaf: parsing
    `Steel_AISI_1045()` and evaluating yields a non-Undef Value
    inspectable in a test.
  - **Cache adapter:** persistent cache key composition handles the
    new variant. Leaf: a cache round-trip test on a struct-bearing
    result reads back identical.
  - **Stdlib structure_def rewrite (first slice):** rewrite one
    Load + one Support + Steel_AISI_1045 as .ri structure_defs.
    Leaf: a .ri example file evaluates to expected runtime values
    via `reify eval`.
  - **PascalCase sweep + remaining builtin rewrites:** follow-up
    tasks (one per ctor or small batch). Leaf per task: the named
    ctor evaluates correctly via .ri.
  - **Boundary tests:** producer-side (compile lowers correctly)
    + consumer-side (FEA stack receives expected structure shape).
    Sketch which crates the tests live in.
  - **ComputeNode trampoline arm:** the trampoline accepts
    Value::StructureInstance arguments and unpacks fields per the
    contract. This is shared work with §8 DAG task ε/η; sequencing
    is "either order works, but commit this PRD's foundation first."

Each leaf names its user-observable signal in description.

## SESSION END

Stop when:
  1. PRD at `docs/prds/v0_3/structure-instance-runtime.md` is
     complete with the seven Q-SIR-* questions resolved.
  2. Decomposition DAG sketched with user-observable signals per
     leaf.
  3. gap-register.md updated to point at the new PRD from GR-001,
     GR-011, GR-019, GR-031.
  4. Leo approves the PRD ("approved" / "lgtm" / "ship it").
  5. Hand-back paragraph summarizing what's authored + the next
     move (commit the PRD; then a short session to file the DAG
     tasks via fused-memory planning_mode).

Do NOT:
  - File any tasks via fused-memory in this session. Filing is a
    separate short session that runs after the PRD is committed.
  - Edit code under crates/.
  - Commit unless Leo explicitly asks.
  - Modify the ComputeNode contract — its trampoline-arm
    expectation is the constraint this PRD must satisfy.

Hard cap: 200k tokens. If running long, write hand-off note at
`docs/architecture-audit/structure-instance-runtime-session-handoff.md`.
```

---

## Notes for Leo

- The PRD design is fully constrained by GR-001's Option B resolution + the ComputeNode contract's trampoline expectation. The session's job is documentation + decomposition, not architectural redesign.
- Expected session length: 45–90 minutes interactive.
- After this session: commit the PRD, then run the §8 DAG filing session (separate prompt: `eight-dag-filing-session-prompt.md`).
- The PRD-decomposition skill (if it's landed by the time you run this) should be active and applied to the decomposition step.
