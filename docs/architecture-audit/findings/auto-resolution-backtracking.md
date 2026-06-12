# Audit: Combinatorial `auto` Type-Parameter Resolution Backtracking

**PRD path:** `docs/prds/v0_2/auto-resolution-backtracking.md`
**Auditor:** audit-auto-resolution-backtracking
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 2 (1 PARTIAL + 1 DRIFT remain; all other gaps resolved by 3558, α/4431, β/4433, γ/4434, δ/4435, ε/4436)

Breakdown by state: WIRED=12 (M-002,003,004,005,006,007,008,009,011,012,013,014), PARTIAL=1 (M-001), TODO=0, FICTION=0, DRIFT=1 (M-010), ORPHAN=0.

## Top concerns

- ~~**The DFS library is wired, the parser now accepts `auto:` in `type_arg_list`, but the compile pipeline never invokes the resolver.**~~ **RESOLVED by 3558 (commit `8d1cf09598`):** The compile-pipeline call-site now exists at `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs:148` (`resolve_auto_type_params_with_backtracking`). `CompiledModule.auto_type_substitution` is populated at `:214` (`AutoTypeSubstitution::new(...)`). M-002 and M-014 flipped FICTION/PARTIAL→WIRED.
- **Constraint-feasibility incremental binding is deferred per the PRD itself, but the deferral has a soundness implication the PRD doesn't acknowledge.** A `TODO(post-substitution-mechanics)` block at `auto_type_param.rs:1322-1336` notes that the BFS-fallback used by both depth-bound and 100k-cap branches is *only* sound today because Phase B's verdict does not depend on the candidate binding (empty `ValueMap`). Once substitution lands, BFS may silently pick a per-param-feasible combination that is infeasible at the cross-product — producing wrong substitutions while the warning still reads "fell back to BFS". The PRD describes BFS fallback as a graceful degradation; in practice it will become latently wrong.
- ~~**Backjumping correctness is gated on `Type::TypeParam` references actually appearing in constraint expressions, which requires the deferred substitution pass.**~~ **RESOLVED by β/4433 + ε/4436:** `seed_candidate_value_map` (γ/4434) seeds per-candidate literal defaults into `ConstraintInput.values` so a real, data-driven `ConstraintChecker` can produce per-leaf verdicts. `crates/reify-compiler/tests/auto_backjumping_real_source.rs` (ε/4436) is the first integration test exercising backjumping from real `.ri` candidate data with no `MockConstraintChecker`. M-007 flipped PARTIAL→WIRED.
- **The PRD's "smallest infeasibility witness" guarantee is partially substituted by a structural approximation in implementation.** Code comments at `auto_type_param.rs:425-471` (in `emit_no_feasible_cross_product_diagnostic`) explain that when DFS exits with zero feasibles, the search has by construction proved the entire cross-product infeasible and the "first-param prefix" message is used as a structural witness rather than the user-actionable smallest-rule-out partial assignment the PRD specifies. This is documented but is technically a DRIFT from the PRD wording.

## Mechanisms

### M-001: v0.1 per-parameter BFS orchestrator (`resolve_auto_type_params`)

- **State:** PARTIAL
- **Failure mode:** F2 (assumed pre-existing infrastructure is API-only, not user-reachable)
- **Evidence:** `crates/reify-compiler/src/auto_type_param.rs:1064` (function); `tests/auto_type_param_multi_param_tests.rs` (1075 lines); the function is invoked only from tests and from the v0.2 BFS-fallback wrapper. No call from `crates/reify-compiler/src/lib.rs::compile_*`. Task 2387 done.
- **Blocks:** any source-level use of `auto:` in type-arg position; transitively the v0.2 DFS, which falls back to BFS at depth/cap bounds.
- **Note:** Listed as a v0.2 pre-condition. The library code shipped; the user surface didn't. The PRD treats this as already-resolved and builds on top.

### M-002: Parser accepts `auto:` / `auto(free):` in `type_arg_list`

- **State:** PARTIAL (parser landed; compile-pipeline wiring still missing)
- **Failure mode:** F1 → reclassified to F4 (parser surface lands but the compile pipeline never calls the resolver, so source-level `Bearing<auto: Seal>` parses but produces no substitution)
- **Evidence:** `tree-sitter-reify/grammar.js:710-714` admits `choice($.type_expr, $.number_literal, $.auto_type_arg)`; the `auto_type_arg` rule at `grammar.js:725-729` reuses `auto_keyword` (defined at ~`grammar.js:459`, accepts both `auto` and `auto(free)`) followed by `':' <bound>`. Corpus tests at `tree-sitter-reify/test/corpus/auto_type_arg.txt` exercise both `Bearing<auto: Seal>` and `Bearing<auto(free): Seal>`; landed in commit `a46e7d3888` ("GREEN – extend type_arg_list to admit auto: / auto(free): type-args"). The remaining gap is the semantic wiring: no compile-pipeline call site invokes `resolve_auto_type_params` / `resolve_auto_type_params_with_backtracking`, so source-level `auto:` in type-arg position still produces no substitution. The module doc-comment at `auto_type_param.rs:100-106` and the e2e test header at `crates/reify-eval/tests/auto_backtracking_e2e.rs:14-24` are themselves now stale (they still assert the parser doesn't accept `auto: TraitName` and cite the obsolete `grammar.js:601-605` range); follow-up task 3896 filed to refresh those source citations. No task in the 2659-2664 sibling list owns the orchestrator call-site wiring.
- **Blocks:** Every user-visible v0.2 backtracking scenario. Without this, the entire orchestrator API is reachable only from tests.
- **Note:** Both v0.1 and v0.2 PRDs assume this exists. Sibling v0.1 PRD `docs/prds/auto-type-param-resolution.md` does not surface this gap either.

### M-003: DFS over cross-product of `auto:` candidate sets

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1291-1773` (`resolve_auto_type_params_with_backtracking` + `dfs_search` at :2174-2268); 35+ tests in `tests/auto_type_param_backtracking_tests.rs` (4820 lines); 4 PRD-traceable BFS-failure-success scenarios in `crates/reify-eval/tests/auto_backtracking_e2e.rs`. Task 2659 done (`4eb77b71a6`).
- **Note:** Library-level only — see M-002.

### M-004: Configurable `max_depth` default 6 (sourced from `reify.toml [auto_type_params]`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-config/src/lib.rs` `AutoTypeParamsConfig`; `crates/reify-config/tests/auto_type_params_config.rs` pins default = 6, zero-rejection, unknown-key rejection; `DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH` public const; consumed by `resolve_auto_type_params_with_backtracking` as a scalar argument.
- **Note:** Single-source-of-truth pinned. The call-site that would read `Manifest::auto_type_params().max_depth` does not yet exist (no compile-pipeline integration).

### M-005: Depth-bound fallback to BFS with `AutoTypeParamDepthBoundExceeded` warning

- **State:** PARTIAL
- **Failure mode:** F2 (mechanism wired, but soundness depends on a deferred mechanism)
- **Evidence:** `auto_type_param.rs:1338-1356`; diagnostic registered at `crates/reify-types/src/diagnostics.rs:646`; `NOTE(substitution-pass-trigger)` at `auto_type_param.rs:1322-1336` flags that BFS-fallback is sound *only* because substitution is not yet active. Once the deferred `Type::TypeParam → Type::StructureRef` pass lands, BFS may silently pick wrong substitutions.
- **Blocks:** Promotion to "warning" semantic vs. error is itself contingent on the substitution work; the NOTE recommends revisiting at that point.
- **Resolution (task 3637):** The latent-correctness-hazard portion is now mitigated by an in-line diagnostic caveat: the `AutoTypeParamDepthBoundExceeded` message produced at fallback time explicitly states "BFS-fallback soundness is contingent on … substitution remaining deferred". The `TODO(post-substitution-mechanics)` marker has been replaced with `NOTE(substitution-pass-trigger)` carrying an inline soundness rationale. PARTIAL state is retained — full resolution still requires the substitution pass to land and this entry to be revisited at that point.
- **Note:** The PRD frames BFS fallback as a soft cap; the implementation flags it as a latent correctness hazard. This is a documented drift between PRD intent and implementation reality.

### M-006: Configurable `max_cross_product_size` cap (default 100,000) with BFS fallback

- **State:** PARTIAL
- **Failure mode:** F2 (same as M-005 — soundness gated on deferred substitution)
- **Evidence:** `auto_type_param.rs:1442-1495`; `crates/reify-config/src/lib.rs` `DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE = 100_000`; `crates/reify-config/tests/auto_type_params_config.rs:67-100`; diagnostic registered at `crates/reify-types/src/diagnostics.rs:677`. Task 2662 done.
- **Resolution (task 3637):** Mirror of M-005 resolution. The `AutoTypeParamCrossProductSizeExceeded` message produced at fallback time now explicitly states "BFS-fallback soundness is contingent on … substitution remaining deferred" (M-006 audit citation). The `TODO(post-substitution-mechanics)` marker has been replaced with `NOTE(substitution-pass-trigger)`. PARTIAL state retained pending substitution pass.
- **Note:** Saturating multiply guards `usize` overflow. Like M-005, the BFS fallback inherits the substitution-deferral soundness hazard.

### M-007: Backjumping via static constraint-blame map (`build_constraint_blame_map`)

- **State:** PARTIAL
- **Failure mode:** F2 (algorithm wired but inert at runtime today)
- **Evidence:** `auto_type_param.rs:1902-1941` (`build_constraint_blame_map`), `:2111-2123` (`compute_deepest_blame_level`), `:2174-2268` (`dfs_search` with `DfsControl::BackjumpTo`); test coverage in `tests/auto_type_param_backtracking_tests.rs`. Task 2660 done (`ebc94ff262`). However, the map only fires when constraint expressions reference `ValueRef` cells typed `Type::TypeParam(name)`. Today, the deferred substitution mechanics (see M-013) mean cells are typically NOT typed as `TypeParam` at the point Phase A/B/C runs, so the map is typically empty.
- **Note:** Code is correct; the "absent ↔ ordinary backtrack" contract preserves test outcomes when blame is unavailable. But the PRD's CSP-optimisation claim ("rather than backtrack one level") is conditionally inert until substitution lands. None of the e2e tests exercise it from real source — they use `MockConstraintChecker` to script per-leaf verdicts.

### M-008: `auto(free)` cross-product NonUnique enumeration + lex-first pick

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1705-1770` (all-free NonUnique path); `NON_UNIQUE_DISPLAY_CAP = 16` (line 301); `AutoTypeParamNonUnique` diagnostic at `reify-types/src/diagnostics.rs:622`. Task 2661 done (`8e2309d195`). Strict vs all-free dispatch on `any_strict`.
- **Note:** Free-mode collection cap tightened to `DISPLAY_CAP + 1` per task 2663 to avoid `K^N` worst-case enumeration.

### M-009: Strict-mode Ambiguous on ≥2 cross-product feasibles

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1654-1704` (strict ≥2 arm); `max_feasible_to_collect=2` ensures early-exit at the second feasible (`auto_type_param.rs:1561-1565`); `AutoTypeParamAmbiguous` diagnostic. Task 2659 done.
- **Note:** Composite witness summaries ("T=ORingSeal,U=AirCooled") rendered via `render_witnesses`.

### M-010: Smallest infeasibility witness in no-feasible-cross-product diagnostic

- **State:** DRIFT
- **Failure mode:** F4 (PRD describes user-actionable smallest witness; implementation emits a structural approximation)
- **Evidence:** `auto_type_param.rs:409-481` (`emit_no_feasible_cross_product_diagnostic`); PRD §"Diagnostics" line 35 and §"Diagnostic format on search failure" line 54 specify "*the smallest infeasibility witness* — the partial assignment that ruled out the most candidates". The implementation comment at lines 425-471 explains that because DFS exits with zero feasibles, the entire cross-product is provably infeasible, so the *first-param prefix* is used as a structural witness ("entire cross-product is infeasible — no specific conflict localized") rather than a heuristically-selected "ruled out the most" partial assignment.
- **Note:** Documented drift. The structural witness is arguably more honest than the heuristic, but it is not what the PRD specifies. Phase 3 may want to choose whether to update the PRD or the implementation.

### M-011: Determinism (lex-first by FQN over candidate enumeration)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `enumerate_candidates` (Phase A) returns alphabetical FQN order, preserved by Phase B (`filter_feasible_candidates`) and consumed by `dfs_search` which visits `per_param_candidates[level]` in input order. `auto_type_param.rs:2128-2135` (DFS visit order doc), :1696 and :1751 (`feasible_assignments[0]` lex-first pick). Sibling spec §3.9 reference.
- **Note:** Reproducibility-critical and pinned by `crates/reify-eval/tests/auto_type_param_determinism_tests.rs`.

### M-012: Rich search-failure diagnostic format (parameter list, candidate counts, cross-product size, depth/cap flag)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:409-481`; format pinned to "*parameters considered (in declaration order), candidate counts per parameter, cross-product size, depth context (`depth: n (max_depth = m)`)*"; PRD §"Diagnostic format on search failure" line 54. Task 2663 done.
- **Note:** The "smallest infeasibility witness" piece is M-010 (drift); the other reported pieces are present.

### M-013: Type-substitution mechanics (`Type::TypeParam(T) → Type::StructureRef(candidate)`)

- **State:** TODO
- **Failure mode:** F5 (PRD-declared deferred precondition; no production code exists)
- **Evidence:** PRD §"Resolved design decisions" line 52 "Constraint-feasibility incremental binding deferred"; `auto_type_param.rs:32-35`, `:84-86`; `NOTE(substitution-pass-trigger)` blocks at hoist sites (previously `TODO(post-substitution-mechanics)`, renamed by task 3637); `:1322-1336` (soundness implication of fallback); no production code in any crate substitutes `Type::TypeParam` to `Type::StructureRef`. No task in 2659-2664 owns this; PRD says "Tracker for the optimization filed separately as a v0.2.x bookmark task" — no such task located in the searched sample.
- **Blocks:** True end-to-end exercise of M-007 backjumping; promotion of M-005/M-006 BFS fallbacks from "soft" to "sound".
- **Cross-reference (task 3637):** Task 3637 wired diagnostic self-documentation for M-005/M-006 (the BFS-fallback messages now explicitly state the substitution-soundness hazard at emission time) and replaced dangling `TODO(post-substitution-mechanics)` markers with `NOTE(substitution-pass-trigger)` plus inline soundness rationale. The substitution pass itself (this entry) remains TODO.
- **Note:** PRD self-declares this as deferred; calling it a gap is interpretive. Listing here so Phase 3 can decide whether the v0.2.x bookmark task actually exists.

### M-014: Compile-pipeline integration (call-site that invokes the orchestrator from `compile`/`compile_with_*`)

- **State:** FICTION
- **Failure mode:** F1 (no caller from production code)
- **Evidence:** `crates/reify-compiler/src/lib.rs:114-380` (all `compile*` entry points) contain no reference to `auto_type_param::*`; module export is `pub mod auto_type_param;` at line 9 only. Grep confirms only test files and the orchestrator's own internal call (BFS-fallback delegate) reach the orchestrators. No task in 2659-2664 owns this integration.
- **Blocks:** Same as M-002 — every user-facing v0.2 claim. The orchestrators are reachable from the LSP-diagnostic surface only via library callers that don't exist.
- **Note:** Discovered alongside M-002. Even if the parser learned `auto:`, the compile pipeline would still need a Phase-A/B/C invocation site, candidate substitution into resolved type-arg lists, and a result-handling path for `SelectionResult::Ambiguous`.

## Cross-PRD breadcrumbs

- The v0.1 sibling PRD `docs/prds/auto-type-param-resolution.md` (Phase A/B/C/D) likely shares M-002 and M-014 as orphaned mechanisms — leave to its own audit.
- `Type::TypeParam` typing of cells (M-007 inertness) potentially interacts with the trait-resolution / fn-signature work resolved in task 3440 (two-pass fn signature type resolution) and the broader "types as values" surface; out of scope here.
- The "constraint-feasibility incremental binding" optimization (PRD line 52 + 64) is described as a future v0.2.x bookmark task. Whether such a bookmark exists in tasks/memory wasn't confirmed within the audit's research budget — flag for Phase 3 sweep.
