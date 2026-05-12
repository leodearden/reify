# ComputeNode + GR-001 contract session — start prompt

Paste the block below into a fresh Claude Code session in this repo. It is self-contained: the session agent will read the cited audit artifacts, then run an interactive contract-authoring conversation with Leo.

---

## Paste this block

```
You are running an interactive contract-authoring session with Leo. The goal:
resolve two interlocked architectural questions from Reify's 2026-05-12
architecture audit:

  1. GR-001 — the architectural shape of struct-constructor runtime
     evaluation (nominal vs structural conformance for kind-tagged Maps)
  2. ComputeNode — the dispatch-seam contract whose absence is the
     canonical instance of the audit's dominant failure mode

Deliverables of this session, in this order:

  a. A decision on GR-001's architectural shape, with rationale captured
     in `docs/architecture-audit/gap-register.md` (promote GR-001 to a
     resolved entry; cluster C-01 disposition recorded).
  b. A ComputeNode CONTRACT document at `docs/prds/v0_3/compute-node-contract.md`
     (new file; supersedes the existing `compute-node-infrastructure.md`'s
     accreted design questions). The contract resolves: cancellation type +
     semantics, pending lifecycle, dispatch-registry scope, OpaqueState
     transfer rules, and the corpus-level policy of which features route
     through ComputeNode vs bypass.
  c. A SKETCH of boundary tests facing both ways across the seam — which
     crates the tests live in, which scenarios they cover, the
     producer-side and consumer-side preconditions/postconditions they
     pin. Sketch only — do not implement.
  d. A DAG of integration-phase tasks (replaces / supersedes 3383/3384/
     3379), each leaf naming a user-observable signal per the
     `feedback-task-chain-user-observable` policy. Do NOT file the tasks
     during this session — produce the DAG as a doc for Leo to approve
     before filing.
  e. Disposition notes for cluster C-02 (and any others made obvious by
     the GR-001 decision) in `docs/architecture-audit/gap-register.md`.

## REQUIRED READING (in this order — do not skim)

Audit corpus:
  1. docs/architecture-audit/README.md
  2. docs/architecture-audit/audit-brief.md  (failure-mode catalog,
     state codes)
  3. docs/architecture-audit/gap-register.md  (seeded with GR-001)
  4. docs/architecture-audit/phase-3-files-synthesis.md  (esp. C-01,
     C-02, C-08, C-16; §5b "runtime/compile-time boundary recurrently
     mis-modeled")
  5. docs/architecture-audit/phase-3-scaffold-pattern-critique.md
     (Type A/B/C decomposition; ComputeNode is Type A canonical)
  6. docs/architecture-audit/phase-3-breadcrumb-map.md  (Cluster A
     GR-001 cross-cites — 17 PRDs; Cluster C ComputeNode chain — 15
     PRDs)
  7. docs/architecture-audit/phase-3-fixnow-filing-log.md  (notes the
     transitive GR-001 dependency surfaced when applying the new
     user-observable-leaf policy)

Findings files (the per-PRD audit detail):
  8. docs/architecture-audit/findings/compute-node-infrastructure.md
     (M-001 through M-018 — the producer-side audit; pay particular
     attention to M-012/M-013/M-014/M-015/M-016/M-017 + "Top concerns"
     for the four open design questions)
  9. docs/architecture-audit/findings/structural-analysis-fea.md
     (M-001/M-002 — the consumer side; the `solve_elastic_static`
     stdlib fn FICTION)
 10. docs/architecture-audit/findings/multi-load-case-fea.md
     (assumes LoadCase/MultiCaseResult ctors and @optimized dispatch
     simultaneously — both questions interlock here)
 11. docs/architecture-audit/findings/persistent-fea-cache.md
     (consumes ComputeNodeData.cache_key + PersistentlyCacheable; M-011
     blocked on dispatch landing)
 12. docs/architecture-audit/findings/warm-state-eviction.md
     (CgWarmState producer side; OpaqueState transfer assumption)
 13. docs/architecture-audit/findings/mesh-morphing.md
     (THE NEGATIVE CASE — explicitly opts OUT of routing through
     @optimized; informs the corpus-level "which features route through
     ComputeNode" policy)

Current PRDs (read for context, not for verbatim adherence — the contract
may supersede portions):
 14. docs/prds/v0_3/compute-node-infrastructure.md
 15. docs/prds/v0_3/structural-analysis-fea.md  (especially §"Sketch of
     approach")
 16. docs/prds/v0_3/multi-load-case-fea.md
 17. docs/prds/v0_3/persistent-fea-cache.md
 18. docs/prds/v0_3/mesh-morphing.md  (the explicit non-consumer)

Leo's recently-recorded preferences (CRITICAL — these set the discipline
of this session):
 19. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_naming.md
       ("incomplete/ill-formed implementation chain" — the term to use
       in this session and the contract document; NOT "scaffold")
 20. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
       (B + H is the chosen resolution mode; C-as-integration-gate is
       the DAG-friendly approximation of producer-consumer-pair)
 21. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
       (every leaf task in the deliverable (d) DAG must name a
       user-observable signal)
 22. ~/.claude/projects/-home-leo-src-reify/memory/feedback_prd_grammar_gate.md
       (if the contract assumes any DSL surface syntax, verify the
       grammar production exists in tree-sitter-reify and the parser
       test covers it — or queue the grammar work as a prerequisite)
 23. ~/.claude/projects/-home-leo-src-reify/memory/feedback_orchestrator_narrow_locks_favor_upfront_design.md
       (the integration-phase DAG should be small + each task narrowly
       lockable; the cross-crate contract artifact itself should be
       authored interactively (this session) or as a high/critical-
       priority wide-lock task — medium priority will starve)

## CONTEXT — what's already been decided (do not relitigate)

- Audit failure-mode terminology: **"incomplete/ill-formed implementation
  chain"** (incomplete = link missing; ill-formed = links contradict).
  ComputeNode is the canonical Type A (producer-orphan) instance per the
  Phase 3 scaffold critique.
- Resolution mode for ComputeNode: **Option B + Approach H** (vertical-
  slice decomposition under design-first/contracts/boundary-tests
  discipline). Confirmed by Leo 2026-05-12.
- The new policy is in force: leaf tasks must deliver user-observable
  behavior (CLI output / .ri eval / GUI state).
- Three contested-ownership pairs from the breadcrumb map have already
  been resolved by Leo (LGTM 2026-05-12): OpenVDB → multi-kernel;
  Manifold propagate_attributes → persistent-naming-v2; the 11 missing
  try_eval_topology_selector dispatch arms → topology-selectors. These
  are recorded in tasks-to-do list; don't re-decide them here, but the
  GR-001 decision may affect how they wire up.
- 13 fix-now tasks (#3462-#3474) already filed under the new policy.
  C-29 (`to_global` + envelope stress helpers) was filed with a Rust-
  API observability path because user-observable testing of those
  helpers is transitively blocked by GR-001. The GR-001 resolution
  therefore unblocks C-29's full DSL-visible behavior.

## OPEN QUESTIONS TO RESOLVE IN CONVERSATION

These should be answered in the contract document or its GR-001
companion decision.

GR-001 architectural question (decide FIRST — it cascades into the
ComputeNode contract):
  Q-GR1. Does Reify's trait system stay strictly nominal (declared
         `: TraitName` bounds only)? If yes, struct-ctor evaluation must
         produce a typed value carrier — likely `Value::StructureInstance`
         or convergence on kind-tagged Maps as the canonical struct
         shape (with the current snake_case/PascalCase inconsistency
         resolved). If structural conformance is admitted for kind-
         tagged Maps, the existing builtin-dispatch path (`point_load`,
         `FixedSupport`) is the canonical shape and struct-ctor eval
         lowers to "produce a kind-tagged Map keyed by struct name."
         The decision affects: nominal-trait conformance pathway,
         `Value` enum variants, `value_type_kind_matches`, persistent
         cache key composition, the ComputeNode trampoline signature.

ComputeNode contract questions (decide AFTER GR-001):
  Q-CN1. Cancellation type and semantics. Three options on task #3384:
         `Arc<AtomicBool>`, `tokio_util::sync::CancellationToken`,
         custom type. Constraints from FEA #16 (task 2924): the
         regression test must drive rapid input changes and assert no
         orphaned solver threads / memory. Constraints from the
         orchestrator: tokio_util is a sizable dep — pull it in only
         if the semantics demand it.
  Q-CN2. Pending lifecycle. Three options: new `Value::Pending`
         variant (ripples through every Value match site); reuse
         existing `Freshness::Pending` (couples ComputeNode lifetime
         to freshness-walk timing); sentinel-by-convention (e.g.
         `Value::Undef` with a tag). Constraints from downstream:
         `max_von_mises < yield_stress` constraint needs SOME signal
         for "FEA still running"; the persistent-fea-cache PRD also
         relies on a stable shape across sessions.
  Q-CN3. Dispatch-registry scope. Global `OnceLock` vs per-`Engine`.
         Constraint-side precedent in `engine_admin.rs:415-422`
         (`Engine::register_optimized_impl`) is per-Engine; that's the
         straightforward extension. Global is simpler for callers but
         complicates test isolation.
  Q-CN4. OpaqueState transfer rules — when does warm state move
         between graph slot and `CacheStore` at dispatch boundaries?
         Slot exists, Clone deliberately drops it, `donate_warm_state`/
         `get_warm_state` exist NodeId-keyed but no path connects them.
         The warm-state-eviction PRD assumes this works; persistent-
         fea-cache presupposes it; design is open.

Cross-cutting consumer policy (decide concurrently with the contract):
  Q-POL. Which features route through ComputeNode, which bypass?
         Confirmed in-scope by their PRDs: FEA solve_elastic_static,
         multi-load-case solve_load_cases, persistent-fea-cache as a
         storage tier under, warm-state-eviction as an attached state
         lifecycle. Confirmed OUT-of-scope by its own PRD:
         mesh-morphing (explicitly composes solver primitives directly).
         AMBIGUOUS: a-posteriori-error-estimation ZZ recovery
         (candidate ComputeNode but not yet committed); modal/thermal
         (future); buckling solve_buckling (assumed but not committed).
         The contract should state the policy and its rationale.

## CONVERSATIONAL STYLE FOR THIS SESSION

- Leo wants terse, technical responses. No preamble, no apologies, no
  "great question."
- Present option menus for architectural choices; do NOT recommend a
  single answer unless the analysis genuinely converges.
- Push back if Leo's framing has an unstated assumption you can detect
  — he'd rather hear it now than after the contract is committed.
- Cite file:line evidence from the findings files when relevant.
- Use AskUserQuestion for crisp 2-4 way option menus where the choice
  is genuinely independent of other context in the conversation;
  otherwise lay options in prose.
- This is a contract-authoring session, NOT an implementation session
  — do not edit code, do not file tasks, do not commit.

## DELIVERABLE FORMAT (when Leo confirms a decision)

Write incrementally:
- GR-001 decision: promote the existing `gap-register.md` GR-001 entry
  in place (don't create a new file), with `Disposition` and an
  explicit rationale paragraph. If a follow-up cleanup PRD is needed,
  name it.
- Contract document at `docs/prds/v0_3/compute-node-contract.md`. Use
  the existing PRD conventions in `docs/prds/`. Structure: §0 Purpose
  & supersession of compute-node-infrastructure.md prose, §1 GR-001
  decision summary (or pointer to gap-register), §2 Cancellation,
  §3 Pending lifecycle, §4 Dispatch registry scope, §5 OpaqueState
  transfer, §6 Consumer policy (which features route through, with
  named rationales), §7 Boundary test sketch (cross-crate scenarios),
  §8 Integration-phase DAG (the proposed task tree, not yet filed),
  §9 Open questions (anything that surfaced but didn't reach decision).
- Boundary test sketch lives in §7 of the contract doc; it's a sketch,
  not test code.
- Integration DAG lives in §8; each leaf task has a name, a 1-line
  observable signal, prerequisites. Don't file via fused-memory
  during this session — Leo will approve and file later.

## SESSION END

Stop when:
  1. GR-001 decision is recorded in gap-register.md
  2. The contract document at compute-node-contract.md is complete,
     including the integration DAG sketch and consumer policy
  3. C-02 disposition is recorded in gap-register.md
  4. Leo has explicitly approved the contract (look for "approved",
     "lgtm", or "ship it" — don't infer)
  5. A short hand-back paragraph is written to Leo summarizing what
     was decided and naming the next move (file the DAG tasks /
     queue the contract implementation / merge the contract PR /
     etc.)

Do NOT:
  - File integration tasks during this session
  - Edit code under crates/
  - Commit any of the written artifacts unless Leo explicitly asks
  - Promote other gap-register clusters beyond C-01 and C-02 (those
    are this session's scope; the rest are for separate work)

If the session runs long and you hit ~150k tokens of your own context,
write a hand-off note at `docs/architecture-audit/computenode-contract-session-handoff.md`
capturing what's decided, what's open, and the next agent's required
reading. Then stop and tell Leo to start a fresh session.

Hard cap: 200k tokens of your own context. Plan accordingly.
```

---

## Notes for Leo

- The block above is self-contained. Paste it into a fresh Claude Code session running in `/home/leo/src/reify`.
- The session will likely run 1–3 hours of interactive back-and-forth depending on how many of Q-CN1..Q-CN4 you want to dig into.
- The contract document this session produces is the entry-point for the integration-phase DAG. Filing that DAG (after your approval) can happen in a separate short session or via the same agent before it ends.
- GR-001's resolution may cascade into other cluster decisions (C-08, C-16, C-19). The session prompt restricts disposition recording to C-01 and C-02 to keep scope tight; promote others in a later Phase-3 register session.
- If the session needs to be split, the hand-off mechanism is built into the prompt (look for `computenode-contract-session-handoff.md`).
