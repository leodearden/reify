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

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `tree-sitter-reify/grammar.js:710-714` admits `choice($.type_expr, $.number_literal, $.auto_type_arg)`; the `auto_type_arg` rule at `grammar.js:725-729` reuses `auto_keyword` (defined at ~`grammar.js:459`, accepts both `auto` and `auto(free)`) followed by `':' <bound>`. Corpus tests at `tree-sitter-reify/test/corpus/auto_type_arg.txt` exercise both `Bearing<auto: Seal>` and `Bearing<auto(free): Seal>`; landed in commit `a46e7d3888`. Compile-pipeline call-site wired at `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs:148` (`resolve_auto_type_params_with_backtracking`) via 3558 (commit `8d1cf09598`). Follow-up task 3896 filed to refresh stale source citations in module doc-comment and e2e test header.
- **Note:** Previously PARTIAL (parser landed; compile-pipeline wiring missing). Resolved by 3558 (commit `8d1cf09598`). Both v0.1 and v0.2 PRDs assume this exists. Sibling v0.1 PRD `docs/prds/auto-type-param-resolution.md` does not surface this gap.

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

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1338-1356`; diagnostic registered at `crates/reify-core/src/diagnostics.rs` (formerly `crates/reify-types/src/diagnostics.rs:646`); `NOTE(substitution-pass-trigger)` at `auto_type_param.rs:1322-1336`. α=4431 landed the substitution mechanics; γ=4434 delivered the single joint-recheck at fallback with `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` and reverted post-substitution hoists (task 3637), closing the soundness gap.
- **Resolution (tasks 3637, γ=4434):** `TODO(post-substitution-mechanics)` markers replaced with `NOTE(substitution-pass-trigger)` (task 3637). γ=4434 delivered the single joint-recheck + reverted hoists, closing the previously-deferred substitution-deferral soundness hazard. BFS fallback is now sound end-to-end.
- **Note:** Previously PARTIAL (soundness gated on deferred substitution). Closed by γ=4434 joint-recheck + hoists revert. The PRD frames BFS fallback as a soft cap; this is now the implementation reality as well.

### M-006: Configurable `max_cross_product_size` cap (default 100,000) with BFS fallback

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1442-1495`; `crates/reify-config/src/lib.rs` `DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE = 100_000`; `crates/reify-config/tests/auto_type_params_config.rs:67-100`; diagnostic registered at `crates/reify-core/src/diagnostics.rs` (formerly `crates/reify-types/src/diagnostics.rs:677`). Task 2662 done.
- **Resolution (tasks 3637, γ=4434):** Mirror of M-005 resolution. `TODO(post-substitution-mechanics)` marker replaced with `NOTE(substitution-pass-trigger)` (task 3637). γ=4434 delivered the single joint-recheck + reverted hoists, closing the substitution-deferral soundness hazard in the cross-product-size-cap branch.
- **Note:** Previously PARTIAL (same soundness gap as M-005). Saturating multiply guards `usize` overflow. Closed by γ=4434 joint-recheck + hoists revert.

### M-007: Backjumping via static constraint-blame map (`build_constraint_blame_map`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:1902-1941` (`build_constraint_blame_map`), `:2111-2123` (`compute_deepest_blame_level`), `:2174-2268` (`dfs_search` with `DfsControl::BackjumpTo`); test coverage in `tests/auto_type_param_backtracking_tests.rs`. Task 2660 done (`ebc94ff262`). β=4433 wired real-checker candidate substitution in-loop; γ=4434 seeds per-candidate literal defaults into `ConstraintInput.values` via `seed_candidate_value_map`. `crates/reify-compiler/tests/auto_backjumping_real_source.rs` (ε/4436) is the first integration test exercising backjumping from real `.ri` candidate data with no `MockConstraintChecker`, proving the blame map fires and `DfsControl::BackjumpTo` prunes subtrees from real source.
- **Note:** Previously PARTIAL (algorithm wired but inert — cells not yet typed `TypeParam`). Closed by β=4433 + γ=4434 + ε/4436 real-source test. The "absent ↔ ordinary backtrack" contract preserves correctness in the non-blame path.

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

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** α=4431 landed the `Type::TypeParam → Type::StructureRef` apply/substitution mechanics (apply pass); δ=4435 wired value-population consumers. `auto_type_param.rs:32-35`, `:84-86` reflect updated typings. `NOTE(substitution-pass-trigger)` blocks at hoist sites (task 3637) document the historical deferral context and replaced the former `TODO(post-substitution-mechanics)` markers.
- **Cross-reference (task 3637):** Diagnostic self-documentation for M-005/M-006 wired (BFS-fallback messages state the substitution-soundness hazard at emission time). Dangling `TODO(post-substitution-mechanics)` markers replaced with `NOTE(substitution-pass-trigger)` plus inline soundness rationale.
- **Note:** Previously TODO (PRD-declared deferred precondition; no production code). Resolved by α=4431 (substitution mechanics) + δ=4435 (value population). M-005/M-006 soundness gap closed by γ=4434 joint-recheck.

### M-014: Compile-pipeline integration (call-site that invokes the orchestrator from `compile`/`compile_with_*`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Compile-pipeline call-site wired at `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs:148` (`resolve_auto_type_params_with_backtracking`); `CompiledModule.auto_type_substitution` populated at `:214` (`AutoTypeSubstitution::new(...)`). Resolved by 3558 (commit `8d1cf09598`). Task 3558 done.
- **Note:** Previously FICTION (no caller from production code). Resolved by 3558 (commit `8d1cf09598`). Discovered alongside M-002; both gaps closed by the same commit. The Phase-A/B/C invocation site, candidate substitution into resolved type-arg lists, and `SelectionResult::Ambiguous` result-handling path are all present in the new phase module.

## Cross-PRD breadcrumbs

- The v0.1 sibling PRD `docs/prds/auto-type-param-resolution.md` (Phase A/B/C/D) likely shares M-002 and M-014 as orphaned mechanisms — leave to its own audit.
- `Type::TypeParam` typing of cells (M-007 inertness) potentially interacts with the trait-resolution / fn-signature work resolved in task 3440 (two-pass fn signature type resolution) and the broader "types as values" surface; out of scope here.
- The "constraint-feasibility incremental binding" optimization (PRD line 52 + 64) is described as a future v0.2.x bookmark task. Whether such a bookmark exists in tasks/memory wasn't confirmed within the audit's research budget — flag for Phase 3 sweep.
