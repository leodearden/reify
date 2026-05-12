# Audit: Node-Trait Composition

**PRD path:** `docs/prds/node-trait-composition.md`
**Auditor:** audit-node-trait-composition
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 8

## Top concerns

- **Parallel taxonomies that don't compose.** The PRD's central premise — a single `NodeTraits` declaration informing the scheduler — landed as TWO independent mechanisms: (1) `NodeTraits` bitflags + `NodeArchKind` enum in `reify-types/src/node_traits.rs` (declarative, never read), and (2) `NodePolicyOverrides` + `NodeKind` + `NodeCommitmentOverride` in `reify-runtime/src/commitment.rs` (the actual scheduler hook). The two enums (`NodeArchKind` with 7 variants vs. `NodeKind` with 5 variants) and the two trait/override surfaces have NEVER converged — `node_traits.rs:147-153` explicitly documents the divergence and defers the merge "once future tasks introduce the missing struct counterparts." Scheduler dispatch uses `NodeCommitmentOverride`, not `NodeTraits` flags. As of today, `NodeTraits::IMMEDIATE | WARM_STARTABLE | PROGRESSIVE | COMMITTABLE` is referenced by exactly ONE non-test grep hit: the `pub use` line in `reify-types/src/lib.rs:66`.
- **PRD acceptance criterion #1 ("Document the trait→priority mapping table") is unsatisfied at the wiring layer.** A doc-table exists in `node_traits.rs:204-225` but no code maps `NodeTraits::IMMEDIATE` → `Priority::P0Interactive`/`P1Fast` at scheduling time. The scheduler reads `config.node_priorities: HashMap<NodeId, Priority>` (populated externally) and does not consult `default_traits()`.
- **PROGRESSIVE trait is "wired" only via a freshness-only-walk integration test;** there is no scheduler-side branch that says "this node is PROGRESSIVE, so allow N cache updates." The PROGRESSIVE-tagged behavior is in fact the default freshness-propagation behavior, so the trait flag adds no runtime affordance — the test (`progressive_emission.rs`) is a compile-time anchor test, not a behavior-discriminating one (test comment line 295-298 admits this explicitly).
- **The §7.6 "Corresponds to P0/P1-fast" never-cancelled alignment is undocumented and unwired.** Arch §7.5 says P0/P1-fast nodes are never cancelled; PRD §Scope item 5 says "trait `immediate` must align with P0/P1-fast." No code path checks `NodeTraits::IMMEDIATE` to skip cancellation. Cancellation is decided by `CommitmentTracker::should_continue` based on `NodeCommitmentOverride` and elapsed time, not by trait.

## Mechanisms

### M-001: `NodeTraits` bitflag type with four flags (IMMEDIATE, WARM_STARTABLE, PROGRESSIVE, COMMITTABLE)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/node_traits.rs:30-95` (full bitflag implementation incl. `union`/`intersection`/`Not` with ALL_MASK); 12 unit tests `node_traits.rs:243-460`; task 2350 done (commit e11933c8bf)
- **Blocks:** none directly; downstream consumers are M-003..M-008
- **Note:** Type exists, exported via `reify-types/src/lib.rs:66`, doc-comments cite arch §7.6. Acceptance-criterion #1 (the type itself) is met.

### M-002: `NodeArchKind` enum (7 variants) declaring default `NodeTraits` per architectural node kind

- **State:** PARTIAL
- **Failure mode:** F3 (decision-without-machine-execution: data structure exists, no code reads it)
- **Evidence:** `node_traits.rs:154-194` (enum), `default_traits()` impl `node_traits.rs:226-240`; assignments match the PRD spec exactly (ValueCellScalar/SchemaNode/SourceNode→IMMEDIATE, Resolution/Realization/ComputeNode→WARM_STARTABLE|COMMITTABLE, ConstraintNode→empty); zero non-test grep hits for `NodeArchKind` or `default_traits` outside the defining file
- **Blocks:** the §7.6 trait→priority mapping (M-008), the FEA-node stand-in test (M-014)
- **Note:** Per-kind defaults are encoded as data but never read by the scheduler, eval engine, or any non-test code. Module-level comment `node_traits.rs:16-17` explicitly states: "Nothing in this crate or its dependents currently dispatches on these traits. They are purely declarative scaffolding for downstream scheduler/cache tasks to adopt." This is the PRD's central deliverable and is unwired — see M-005.

### M-003: Trait→priority mapping table (PRD scope item 2: "Document the trait→priority mapping table")

- **State:** PARTIAL
- **Failure mode:** F3 (doc-only mapping; no code)
- **Evidence:** A mapping table appears in `node_traits.rs:204-225` doc-comment (textual prose tying IMMEDIATE→§3.3 P0/P1-fast). No function `traits_to_priority(NodeTraits) -> Priority` exists; the scheduler's `config.node_priorities: HashMap<NodeId, Priority>` is populated externally with no `default_traits()` consultation (`concurrent.rs:305-310`).
- **Blocks:** trait-informed-priority semantics; ad-hoc external priority sources can diverge from declared trait set
- **Note:** Acceptance criterion #1 "Document the trait→priority mapping table" is satisfied as documentation; the implementation surface ("traits inform priority assignment but do not replace it" — PRD goal) has no executable bridge.

### M-004: `NodePolicyOverrides` struct with instance / type / default precedence resolution

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-runtime/src/commitment.rs:81-128` (struct + `set_instance`/`set_type`/`resolve` with documented precedence); `commitment.rs:641` `precedence_instance_wins_over_type_wins_over_default` unit test; task 2353 done (commit 11102b6c10).
- **Blocks:** none
- **Note:** All three policy values match §7.3: `CommitIfSlow` (default), `AlwaysCancelWhenStale`, `OnlyRunOnFinalInputs`. Default impl on `NodeCommitmentOverride` returns `CommitIfSlow`. Acceptance criterion "Override resolution order: instance > type > default. Tested." is met.

### M-005: Per-node `NodeTraits` attachment to runtime node instances

- **State:** FICTION
- **Failure mode:** F1 (declaration without runtime backing)
- **Evidence:** No struct, hashmap, or call site associates a `NodeId` (or `NodeCache` entry, or `EvaluationGraph` node) with a `NodeTraits` value. `progressive_emission.rs:298` documents this gap: "no node-id → traits map exists today; see `node_traits.rs:148-153`." All cache/scheduler types in `reify-eval/src/cache.rs` and `reify-eval/src/graph.rs` lack any `traits: NodeTraits` field. The PRD's worked example "RealizationNode declares `warm_startable + committable`" is currently a declaration about `NodeArchKind::RealizationNode` (a 7-variant enum) — not about any particular instance/cache entry/`NodeId::Realization` value.
- **Blocks:** trait-informed scheduling (M-008, M-010, M-011); meaningful trait-composition tests (PRD acceptance #7); any future FEA solver node wishing to declare `warm_startable + progressive + committable`
- **Note:** The whole point of "each node type declares its default trait set" (PRD scope item 1) is to let the scheduler ask "does this node have PROGRESSIVE?" There is no API to answer that question. The override surface (`NodePolicyOverrides`) is keyed on `NodeId`/`NodeKind`, but it carries `NodeCommitmentOverride` (3-valued), not `NodeTraits` (4-flag-bitfield). These are unrelated data shapes.

### M-006: `OnlyRunOnFinalInputs` gating in scheduler (skip node when any input non-Final)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/gating.rs:69-74` (`has_non_final_inputs`); `crates/reify-eval/src/gating.rs:102-115` (`unblocked_gated_nodes`); `crates/reify-runtime/src/concurrent.rs:285-296` (scheduler skip logic); `crates/reify-eval/tests/only_run_on_final_inputs_gating.rs` (integration test); `crates/reify-runtime/tests/concurrent_eval.rs:2950-2980` and `:3608-3673` (skip + run-when-final cases). Task 2356 done (commit 45e7e61e40).
- **Blocks:** none
- **Note:** PRD scope item 4 and acceptance criterion ("Scheduler honours `OnlyRunOnFinalInputs`") are met. Gating helper is layered correctly: lives in `reify-eval`, scheduler in `reify-runtime` wraps it via `NodePolicyOverrides::resolve()` — see `gating.rs:7-14` layering note.

### M-007: `AlwaysCancelWhenStale` cancellation refinement

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `commitment.rs:190-220` (`check_commitment` short-circuits `AlwaysCancelWhenStale → NeverCommit` regardless of elapsed time); `concurrent.rs:329-369` (scheduler `should_continue` honours `NeverCommit`); `concurrent_eval.rs:3417-3605` (3 integration tests covering elapsed > threshold, 0ms threshold, and `NeverCommit` symmetric drop). Task 2358 done via `found_on_main` (commit 7f47912164, with note that the wiring landed in commit 357226436 four weeks before the task closed).
- **Blocks:** none
- **Note:** PRD acceptance criterion ("Scheduler honours `AlwaysCancelWhenStale`") is met. Cancellation override is implemented at the `NodeCommitmentOverride` layer — NOT at the `NodeTraits::COMMITTABLE` layer. The PRD's framing ("absent COMMITTABLE, always cancellable" — arch §7.6 row 4) is realised inversely: presence of `AlwaysCancelWhenStale` override forces never-commit; absence of COMMITTABLE flag has no effect.

### M-008: `IMMEDIATE` trait → P0/P1-fast alignment (arch §7.5 never-cancelled invariant)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §Background line 14 and §Scope item 2 require this alignment. `concurrent.rs:329-369` cancellation logic checks `should_continue(node, true)` against `CommitmentTracker` decision, with no reference to `Priority::P0Interactive` / `Priority::P1Fast` or `NodeTraits::IMMEDIATE`. No grep hit for "never cancel", "P0.*never", or trait-priority bridging in cancellation paths.
- **Blocks:** correct ValueCell / SchemaNode / SourceNode handling under cancellation (PRD assignment: these three default to IMMEDIATE)
- **Note:** Arch §7.5 ("P0/P1-fast Never cancelled") is enforced only insofar as those priorities sort first via Ord-derived priority ordering and complete before cancellation fires. There is no explicit guard. If a P0 task's cancellation-token is observed mid-evaluation, it would be cancelled. Whether this matters in practice depends on whether P0 nodes ever poll the cancel token — a downstream-discovery question.

### M-009: `PROGRESSIVE` trait → multiple-cache-updates-per-evaluation semantics

- **State:** PARTIAL (effectively FICTION at the trait level; the underlying behavior is the cache default)
- **Failure mode:** F1+F3 (declarative flag exists; no scheduler branch reads it; the behavior the flag is supposed to enable is the existing default)
- **Evidence:** `crates/reify-eval/tests/progressive_emission.rs:300-304` admits this directly: "Compile-time anchor to NodeTraits::PROGRESSIVE (arch §7.6 / PRD task #5). This line causes the test to fail to compile if PROGRESSIVE is ever deleted; the assertion is a no-op at runtime (no node-id → traits map exists today; see `node_traits.rs:148-153`)." The test verifies that `propagate_freshness_only` correctly propagates Intermediate→Final through the freshness walk — which works for any node regardless of trait. Task 2360 done (no commit hash in metadata).
- **Blocks:** any actual differentiation between PROGRESSIVE and non-PROGRESSIVE nodes
- **Note:** PRD scope item 6 says "Wire `progressive` so the scheduler expects multiple cache updates per evaluation (does not error on intermediate emission)." The scheduler does NOT error on intermediate emission today — but this is because the cache is permissive, not because the scheduler discriminates. If a non-PROGRESSIVE node writes intermediates, nothing rejects it. The trait is not load-bearing.

### M-010: Project-configuration ingestion of `NodePolicyOverrides`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `reify-config/src/lib.rs` defines `AutoTypeParamsConfig`, `Manifest`, `KernelId`, `KernelPin` but contains zero references to `NodePolicyOverrides`, `NodeCommitmentOverride`, `CommitmentPolicy`, or `node_overrides`. `concurrent.rs:30-31` accepts `node_overrides` programmatically only.
- **Blocks:** non-programmatic per-instance policy assignment; the "dedicated UI widget" prerequisites (PRD §Out of scope says UI is OOS but "data structure and config-file path" should be in)
- **Note:** PRD acceptance criterion #3: "Settable from project configuration and/or programmatically." Programmatic path: WIRED. Configuration-file path: FICTION. The "/or" makes this partially satisfiable, but the PRD's "Out of scope" disclaimer explicitly preserves the config-file path as in-scope. No PRD-decomposition task seems to own the config-file integration; this is a quiet gap.

### M-011: Tests for `NodeTraits` composition asserting per-kind default sets

- **State:** PARTIAL
- **Failure mode:** F4 (test exists but verifies a different surface than the PRD describes)
- **Evidence:** `node_traits.rs:393-460` tests `NodeArchKind::RealizationNode.default_traits() == WARM_STARTABLE|COMMITTABLE` etc.; `reify-types/tests/exports.rs:216-225` tests crate-export visibility. PRD acceptance criterion bullet #2 reads: "`RealizationNode` declares `warm_startable + committable`" — but `RealizationNode` is a `NodeId` variant in `reify-eval/src/cache.rs:15-25`, NOT a `NodeArchKind` variant. The test asserts the `NodeArchKind` shadow declaration, not the actual `NodeId::Realization` variant's traits.
- **Blocks:** none directly; surface drift complicates future PRD-extension
- **Note:** The PRD names node kinds in terms of arch §2.1 (which has 7 kinds: SchemaNode, SourceNode also in the list); the codebase has TWO disjoint taxonomies as documented in `node_traits.rs:147-153`. Tests are written against the new 7-variant `NodeArchKind` placeholder, not the 5-variant runtime `NodeId`. This is internally consistent but does not pin trait-set declarations against the actual scheduler-visible node types.

### M-012: Instance-level vs type-level override precedence integration test (PRD acceptance criterion + scope item 7)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `commitment.rs:641-...` unit test `precedence_instance_wins_over_type_wins_over_default`; `concurrent_eval.rs:3779-3865` integration test `type_level_override_routes_through_resolve` (verifies `set_type(NodeKind::Value, OnlyRunOnFinalInputs)` skips both `Value` nodes via the `NodePolicyOverrides::resolve()` chain).
- **Blocks:** none
- **Note:** PRD acceptance criterion "Override resolution order: instance > type > default. Tested." is met at both the unit level and the scheduler-integration level.

### M-013: `WARM_STARTABLE` trait → `WarmStartable` protocol correspondence

- **State:** PARTIAL
- **Failure mode:** F3
- **Evidence:** `WarmStartable` protocol exists at `crates/reify-types/src/warm.rs:58-66` with `OpaqueState` plumbing; `reify-solver-elastic/src/warm_state.rs` and `reify-kernel-occt/src/stubs.rs:196,346` both implement it. Warm-state pool exists at `reify-eval/src/warm_pool.rs`. No code consults `NodeTraits::WARM_STARTABLE` before donating/restoring warm state — the protocol is implemented directly on solver/kernel types.
- **Blocks:** future scheduler-driven warm-state lifecycle (e.g., "skip warm-state donation for nodes lacking WARM_STARTABLE")
- **Note:** PRD §Dependencies says: "Builds on existing `WarmStartable` protocol from #27 — `WARM_STARTABLE` trait is a declarative tag that corresponds to implementing the existing protocol." The "tag" is declarative-only: no Rust-trait blanket impl, no static-check, no runtime check ties `NodeTraits::WARM_STARTABLE` to `impl WarmStartable for T`. A node could declare WARM_STARTABLE without implementing the protocol (or vice versa) and nothing would notice.

### M-014: End-to-end FEA-stand-in fixture exercising `warm_startable + progressive + committable` composition (PRD task #6 optional)

- **State:** TODO
- **Failure mode:** F3 (PRD-named optional task; no implementation)
- **Evidence:** PRD task breakdown item 6 marks this "Optional". No grep hits for a fixture matching `warm_startable.*progressive.*committable` or `stand-in.*FEA` test in `crates/*/tests/`. The PRD's worked example "FEA solver node" remains hypothetical — `reify-solver-elastic` exists (M-013) but is not wired through a NodeTraits-decorated end-to-end harness.
- **Blocks:** confidence that the four traits actually compose at runtime under a non-trivial node
- **Note:** Marked optional in the PRD so this is informational. The absence of this fixture is consistent with M-005 (no node-id → traits map): there is nothing for such a test to assert against, beyond the trait-declaration unit tests.

## Cross-PRD breadcrumbs

- **`freshness-4-variant.md`** (sibling PRD): PRD §Dependencies explicitly says tasks 3 and 5 depend on the Freshness PRD. Task 2335 (freshness propagation walk) is the gating dependency for M-006 and M-009; verified done via `freshness_walk::propagate_freshness_only` existing. The freshness audit at `findings/freshness-4-variant.md` should cross-check the `Pending`/`Intermediate` semantics that this PRD's gating logic depends on.
- **`compute-node-infrastructure.md`** (v0_3 PRD): ComputeNode runtime struct exists (`reify-eval/src/graph.rs:62-80`), and `NodeArchKind::ComputeNode` declares `WARM_STARTABLE | COMMITTABLE`. The "missing struct counterparts" deferral in `node_traits.rs:148-153` references future ComputeNode/SchemaNode/SourceNode work; that PRD's audit should note whether trait-set assignments are expected to follow.
- **`structural-analysis-fea.md`**: The PRD's worked example node (`warm_startable + progressive + committable`) targets FEA solvers. The FEA PRD's `@optimized` registration (cited in GR-001 transitively and the audit-brief worked example) is the actual consumer that would need M-005 (per-node traits attachment) to function.
- **`reify-implementation-architecture.md` §7.5**: Cancellation behaviour by priority. M-008 documents the gap between trait-declared `IMMEDIATE` and the priority-derived never-cancel guarantee.
