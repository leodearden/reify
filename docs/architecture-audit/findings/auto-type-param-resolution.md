# Audit: `auto` Type-Parameter Resolution (`Bearing<auto: Seal>`)

**PRD path:** `docs/prds/auto-type-param-resolution.md`
**Auditor:** audit-auto-type-param-resolution
**Date:** 2026-05-12
**Mechanism count:** 16
**Gap count:** 11

Breakdown by state: WIRED=5 (M-001,002,003,004,015), PARTIAL=5 (M-005,006,007,008,012), TODO=2 (M-013,014), FICTION=4 (M-009,010,011,016), DRIFT=0, ORPHAN=0.

## Top concerns

- **The entire end-user surface is fiction.** Library-level Phase A/B/C/orchestrator code is well-tested and shipped, but the PRD's *user-visible* surface — the source-level `Bearing<auto: Seal>` syntax — does not parse (`tree-sitter-reify/grammar.js:606-610` admits only `type_expr | number_literal`), and `resolve_auto_type_params` is not invoked from any production code path in `crates/reify-compiler/src/lib.rs::compile_*`. `CompiledModule.auto_type_substitution` is initialised to `AutoTypeSubstitution::default()` in both `compile_builder/ctx.rs:188` and `compile_builder/defs_phase.rs:296` and is never written by any non-test caller. The producer side of the substitution pipeline is dark; the consumer (`Snapshot::from_compiled_module` at `crates/reify-eval/src/snapshot.rs:54`) just sees an empty vec. This is the same gap M-002/M-014 in the sibling v0.2 audit (`auto-resolution-backtracking.md`) — and the v0.1 PRD is its *origin*, not the v0.2 follow-up.
- **The deferred type-substitution mechanic is the keystone, and no v0.1 task owns it.** The PRD's Phase B (criterion 1–9) requires Phase B to actually vary outcomes per candidate by substituting `Type::TypeParam(T)` → `Type::StructureRef(candidate)` into cell types before re-running the constraint checker. The code explicitly documents this as deferred (`auto_type_param.rs:32-35, 84-86, 766-769`); Phase B today uses an empty `ValueMap`, so candidate identity does *not* yet affect feasibility verdicts. Without this, criterion 6 ("`B`'s candidate pool is computed against the resolved `A`") and criterion 5 ("a constraint excludes both" candidates) cannot fire from real source. No v0.1 PRD task owns it (PRD's listed tasks 1–8 do not include it), and the sibling v0.2 PRD also defers it.
- **Criterion 8 — kind-bound `auto: Nat` — is unimplemented and unguarded.** PRD criterion 8 says "Either errors loudly with a 'kind-bound auto unsupported in v0.1' diagnostic, or is supported; pick the simpler path and document." Neither path exists. `enumerate_candidates` walks `template_registry` for trait conformance only; there is no kind-bound code path, no `E_AUTO_TYPE_PARAM_KIND_UNSUPPORTED` diagnostic, and no documentation note. If the parser ever learns `auto: Nat`, the request silently falls through `satisfies_trait_bound` (which treats `"Nat"` as a missing trait → no matches → `Empty` arm → `E_AUTO_TYPE_PARAM_NO_CANDIDATE` — wrong diagnostic for the user).
- **PRD references `SchemaNode` (criterion 7) but the type does not exist** — the substitute is `EvaluationGraph::topology_fingerprint()`. Multiple production comments and tests call this out as the placeholder mapping; the PRD wording about "SchemaNode re-elaborates with the concrete type substituted" is satisfied at the fingerprint level only. This is documented in `auto_type_param.rs:93, 1061` and in the topology-trigger tests (`crates/reify-eval/tests/auto_type_param_topology_trigger_tests.rs:48-54`); the audit catalogues it as DRIFT-adjacent but classifies it under WIRED-with-naming-drift (M-012) because the underlying contract is exercised end-to-end at the test level.

## Mechanisms

### M-001: Phase A candidate enumeration with cap-of-10 + alphabetical determinism (`enumerate_candidates`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/auto_type_param.rs:556-632`; `MAX_AUTO_TYPE_PARAM_CANDIDATES = 10` (line 284); `tests/auto_type_param_phase_a_tests.rs` (multiple); reuses `entity::satisfies_trait_bound` for trait-conformance predicate (line 585; see M-002). Visits templates in sorted-name order to make the result independent of `HashMap` iteration order; emits `AutoTypeParamPoolOverflow` with `with_candidates(first 10 alphabetical)`. PRD acceptance criterion 4 covered at library level.
- **Note:** Library-level only — the function takes `template_registry`/`trait_registry` slices; the call-site that would build those slices from the LSP/compile pipeline does not exist (see M-009/M-010).

### M-002: Trait-conformance predicate (`satisfies_trait_bound`) — PRD's "#66"

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/entity.rs:3016`; consumed by Phase A (`auto_type_param.rs:585`), trait-typed-param checks (`conformance/mod.rs:354, 372, 607`), and trait-bounded type-arg validation (`trait_bounds_tests.rs`). Walks trait refinement chains transitively (PRD §"Trait-bound" line 28). The "nominal not structural" stance documented in audit-brief is enforced here.
- **Note:** The PRD's reference to "#66 (trait conformance)" matches this code. Composite-bound intersection (PRD §"Phase A" line 31) is implemented in Phase A's `bounds.iter().all(...)` (`auto_type_param.rs:585`), not in `satisfies_trait_bound` itself.

### M-003: Phase B per-candidate feasibility filter with monotonic-undef semantics (`filter_feasible_candidates`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:750-811`; uses `ConstraintInput { values: empty_values, … }` with monotonic predicate `satisfaction != Violated` per architecture §2.5; explicit scope cuts documented at lines 28-40 and 711-721; `tests/auto_type_param_phase_b_tests.rs`. Hoists `build_constraints_template` outside the loop to avoid O(candidates × constraints) re-build.
- **Note:** The feasibility check is structurally wired and tested, but is *operationally inert* until the deferred type-substitution mechanic (M-013) actually causes candidate identity to vary constraint outcomes. Phase B today returns the *same* verdict for every candidate when constraints don't reference `Type::TypeParam`-typed cells. This is documented at lines 32-35 (scope cut 2). Counted as WIRED because the function is correct per its declared invariants; the inertness is on M-013.

### M-004: Phase C selection with strict-vs-free dispatch + lex-first tiebreak (`select_candidate`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `auto_type_param.rs:813-1011`; 3-arm `SelectionResult` (`Selected`/`NoCandidate`/`Ambiguous`); strict 0/1/≥2 × free 0/1/≥2 dispatch table at lines 50-56; debug-asserts alphabetical input order; emits `AutoTypeParamNoCandidate`/`AutoTypeParamAmbiguous`/`AutoTypeParamNonUnique` with `with_candidates(...)` lists. `tests/auto_type_param_phase_c_tests.rs`. PRD §"Phase C" + acceptance criteria 1–3, 5 covered at library level.
- **Note:** Single-feasible branch is `free`-independent (returns `Selected` without diagnostic regardless of strict/free) — PRD-compliant and explicitly documented.

### M-005: Multi-param BFS orchestrator with declared-order halt-on-first-failure (`resolve_auto_type_params`)

- **State:** PARTIAL
- **Failure mode:** F2 (assumed pre-existing infrastructure is API-only, not user-reachable)
- **Evidence:** `auto_type_param.rs:1064-1180`; `MultiParamResolutionOutcome { per_param, substitution }`; halt-on-first-failure rule documented at lines 76-80, 1143-1173; 1075-line test suite `tests/auto_type_param_multi_param_tests.rs`. Function is reachable only from tests and from the v0.2 BFS-fallback wrapper (`resolve_auto_type_params_with_backtracking`); no production caller exists (see M-009).
- **Blocks:** Every source-level `auto:` use; criterion 6 (multi-auto declared order) — the substitution Vec is plumbed but not consumed by Phase A's bounds slice or Phase B's `ValueMap` (lines 81-86 + M-013).
- **Note:** Sibling v0.2 PRD `auto-resolution-backtracking.md` audit catalogues the same fact as its M-001 (PARTIAL). Mirrored here because the v0.1 PRD is the *origin* of the library-only delivery, not a downstream effect.

### M-006: Substitution-Vec topology trigger (criterion 7 — `EvaluationGraph::topology_fingerprint` updates from `auto_type_substitution`)

- **State:** PARTIAL
- **Failure mode:** F2 (consumer wired; producer dark)
- **Evidence:** Consumer: `crates/reify-eval/src/snapshot.rs:47-55` reads `module.auto_type_substitution` into `graph.auto_type_substitution` *before* `topology_fingerprint()`; `crates/reify-eval/src/graph.rs:216` declares the field; `:740-757` hashes it into fingerprint bucket 7 with insertion-order-independent sort. `crates/reify-eval/tests/auto_type_param_topology_trigger_tests.rs` covers criterion 7 (flip changes fingerprint; revert restores; insertion-order-independent; empty-default equivalence). Producer: `CompiledModule.auto_type_substitution` is initialised to `AutoTypeSubstitution::default()` at `compile_builder/ctx.rs:188` and `compile_builder/defs_phase.rs:296` and **never written by any non-test caller** (grep across non-test, non-worktree code returns zero writes).
- **Blocks:** Criterion 7 ("Resolution flips a SchemaNode's topology fingerprint"). The test fixtures construct `EvaluationGraph` directly and *manually* assign `graph.auto_type_substitution = vec![…]` (`topology_trigger_tests.rs:61-86`); no source-level path reaches the field.
- **Note:** Production warm-state pool reuse (criterion 7 second half) is keyed by `NodeId` and is fingerprint-independent (`topology_trigger_tests.rs:167-170`), so pool survival on revert is covered by `crates/reify-eval/tests/warm_state_donation.rs` and not by anything that exercises `auto:` resolution. The PRD's "warm-state pool keyed by node-type + path-based identity" claim is satisfied at the unit level by a totally different code path.

### M-007: Configurable substitution-mechanics deferral (`AutoTypeSubstitution` newtype + producer-bug uniqueness assert)

- **State:** PARTIAL
- **Failure mode:** F2 (data structure wired; producer absent)
- **Evidence:** `crates/reify-compiler/src/types.rs:155-207`; uniqueness asserted in *both* debug and release builds (line 179 — note the `assert!` not `debug_assert!`); `into_inner()` consumed by `snapshot.rs:54`; `topology_trigger_tests.rs:208-281` covers the producer-bug panic in debug, and the deref/equality contract.
- **Blocks:** Same as M-006 — needs a producer.
- **Note:** This is the wire-format between `resolve_auto_type_params` output and `EvaluationGraph.auto_type_substitution`. The `Deref<[(String,String)]>` interface keeps the *consumer* unaware of the newtype. Counted as PARTIAL not WIRED because the newtype's `new()` ctor has zero non-test callers in the production graph (M-009).

### M-008: SchemaNode topology-change trigger (PRD criterion 7, arch §6.2 row 5)

- **State:** PARTIAL
- **Failure mode:** F4 (PRD names a concept that maps to a differently-named mechanism)
- **Evidence:** PRD repeatedly references "SchemaNode" (lines 14-15, 52-53, 80, task 5, criterion 7). The Rust codebase has **no** `SchemaNode` type. The closest realised concept is `EvaluationGraph::topology_fingerprint()` (`crates/reify-eval/src/graph.rs:740-790`); the mapping is explicit in `topology_trigger_tests.rs:48-54` ("Architecture mapping: `EvaluationGraph::topology_fingerprint()` maps to the `SchemaNode.compute()` concept in arch §6.2-6.4"). Doc-comments at `auto_type_param.rs:93, 1061` reserve the name "Phase D / SchemaNode topology-trigger work" for a future task (2388).
- **Blocks:** None directly — the fingerprint contract is what every consumer reads. But the naming mismatch makes the PRD harder to map onto code.
- **Note:** I classify this as PARTIAL (not DRIFT) because the *contract* (a topology fingerprint that flips on substitution change and is the cache key for re-elaboration) is wired; only the *name* drifts. Phase 3 may want to decide whether to rename in the PRD (since the codebase uses `topology_fingerprint`) or rename in code (introduce a `SchemaNode` wrapper).

### M-009: Compile-pipeline call-site that invokes `resolve_auto_type_params` and writes `CompiledModule.auto_type_substitution`

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no production producer)
- **Evidence:** `crates/reify-compiler/src/lib.rs` exports `pub mod auto_type_param;` (line 9) but no `compile_*` entry point references the module beyond the export. Grep across the workspace shows the only writes to `auto_type_substitution` (other than `default()` init) live in tests. Every `from_compiled_module` call sees `auto_type_substitution = Vec::new()`.
- **Blocks:** Every source-level `auto:` claim; criteria 1, 2, 3, 4, 5, 6, 7, 11, 12.
- **Note:** Mirrors v0.2 audit M-014. No v0.1 PRD task lists this work (tasks 1–8 cover phases, multi-param, topology, diagnostics, docs — but not "wire it into compile_module").

### M-010: Parser accepts `auto:` / `auto(free):` inside `type_arg_list`

- **State:** FICTION
- **Failure mode:** F1 (source-level contract → no parser backing)
- **Evidence:** `tree-sitter-reify/grammar.js:606-610` (`type_arg_list`) accepts only `choice($.type_expr, $.number_literal)` — `auto_keyword` is not admitted in this position. Module doc-comment at `auto_type_param.rs:100-106` explicitly calls this out: "the parser does not yet accept `auto: TraitName` syntax inside `type_arg_list`". Fixture `examples/bearing_auto_seal.ri:20-25` and the determinism / topology-trigger test headers all document this gap. *(2026-05-27 update: `auto_keyword` is no longer "only used as a value default" — after task 3802 (commit e411301f69), `auto_keyword` is admitted at all 5 binding-site value positions via the shared `_binding_value` rule (grammar.js ~line 752). The gap M-010 documents remains genuine: `auto_keyword` is still not admitted inside `type_arg_list`. Value-position `auto` coverage is owned by `docs/prds/auto-binding-site-positions.md`; the type-arg-position gap here is a separate matter.)*
- **Blocks:** Same as M-009.
- **Note:** Mirrors v0.2 audit M-002. No v0.1 PRD task lists parser work either; the closest mention is task 7 ("Add an example file exercising `Bearing<auto: Seal>` end-to-end") which the fixture file `bearing_auto_seal.ri` defers by using a concrete `Bearing<ORingSeal>` instead. Acceptance criteria 1, 2, 3, 4, 5, 6 cannot fire from real `.ri` source.

### M-011: Kind-bound `auto: Nat` / `auto: Dimension` handling (criterion 8)

- **State:** FICTION
- **Failure mode:** F1 (PRD specifies behaviour; neither support nor a guard diagnostic exists)
- **Evidence:** `auto_type_param.rs:556-632` (`enumerate_candidates`) accepts a `bounds: &[String]` slice and dispatches through `satisfies_trait_bound`; no branch checks whether a bound name is a kind tag (`"Nat"`, `"Dimension"`, etc.) vs a trait name. No `E_AUTO_TYPE_PARAM_KIND_UNSUPPORTED` (or similarly-named) diagnostic exists in `crates/reify-types/src/diagnostics.rs`. No test covers the kind-bound path. PRD criterion 8 explicitly offers two acceptable resolutions ("error loudly" or "support it"); neither has shipped.
- **Blocks:** Criterion 8. Latent risk: if the parser ever learns `auto: Nat`, `satisfies_trait_bound` returns false for every template (no template declares conformance to a trait named `"Nat"`), producing `CandidateEnumeration::Empty` → `AutoTypeParamNoCandidate` — a misleading diagnostic.
- **Note:** Open question in PRD §"Open questions deferred to implementation" (line 101) acknowledges the deferral, but the deferral itself is a gap because *neither* path has been chosen.

### M-012: Composite trait-bound intersection (`auto: TraitA + TraitB`)

- **State:** PARTIAL
- **Failure mode:** F2 (algorithm wired, exercise inert)
- **Evidence:** `auto_type_param.rs:583-586` implements `bounds.iter().all(|b| satisfies_trait_bound(...))` as intersection semantics; the `AutoTypeParam.bounds: Vec<String>` field (line 179) is plural-by-design. `tests/auto_type_param_phase_a_tests.rs` covers the intersection arm at unit level.
- **Blocks:** Criterion 9 (acceptance). The library is correct; the user surface to produce `bounds.len() >= 2` is dark because no parser path emits a multi-trait `AutoTypeParam`.
- **Note:** No `bounds.is_empty()` arm — guarded by `debug_assert!(!bounds.is_empty())` (line 563). Counted as PARTIAL because the source-level surface is fictional (M-010); the library mechanism is otherwise WIRED.

### M-013: Type-substitution mechanics (`Type::TypeParam(T)` → `Type::StructureRef(candidate)` in Phase B)

- **State:** TODO
- **Failure mode:** F5 (PRD-implicit precondition; explicit TODOs in code; no v0.1 task)
- **Evidence:** Phase B uses an empty `ValueMap` (`auto_type_param.rs:763, 781`); per `auto_type_param.rs:32-35` Phase B *scope cut 2* says "A future task will substitute `Type::TypeParam(T)` → `Type::StructureRef(candidate)`"; same TODO blocks at lines 84-86 (orchestrator) and 766-769 (Phase B). The PRD's Phase B (line 35-39) specifies "Instantiate the parameterized definition with the candidate substituted for the auto type-param" — that instantiation does not happen. No v0.1 PRD task (1–8) lists this work. Sibling v0.2 audit catalogues this as its M-013 (TODO).
- **Blocks:** Criteria 5 (no-candidate), 6 (declared order *changes outcome*), 9 (composite-bound exclusion), the full intent of Phase B itself.
- **Note:** This is the keystone — *without* type substitution, candidate identity does not affect feasibility verdicts, and the user-visible outcome of `auto:` is determined entirely by "first alphabetical that has no other-cause-Violated constraint." The PRD does not classify it as deferred; the implementation does.

### M-014: "How `auto` type-param resolution works" doc (PRD task 8)

- **State:** TODO
- **Failure mode:** F3 (partial — spec cross-ref shipped, dedicated doc partly missing)
- **Evidence:** `docs/reify-language-spec.md:476-518` covers §3.9 + the cap-of-10 + lex-first FQN + deferral-to-v0.2 note. `docs/reify-stdlib-reference.md:12` references it. `docs/auto-type-param-resolution.md` exists. Task 8 of the PRD says: "Update language-spec / stdlib reference cross-refs and add a 'How `auto` type-param resolution works' doc with the algorithm, the cap-of-10, and the deferred-to-v0.2 cross-param backtracking note." Cross-refs are shipped; the standalone doc exists at `docs/auto-type-param-resolution.md` but I did not exhaustively diff it against PRD's bullet list — flag for Phase 3 confirmation of completeness.
- **Blocks:** None operational; only PRD task 8 acceptance.
- **Note:** Counted as TODO conservatively. If the standalone doc is complete on inspection, this can be promoted to WIRED.

### M-015: Diagnostics surface in LSP (criterion 10) — code + candidates + data wire-format

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-lsp/src/convert.rs:196-220` debug-traces six AutoTypeParam codes on `reify_lsp::auto_type_param` target; `:658-720` covers the candidate-list → LSP `data` field conversion; `:747-797` proves exactly six emit on `AutoTypeParam` and zero on other codes. Diagnostic codes registered at `crates/reify-types/src/diagnostics.rs:510, 571, 601, 622, 646, 677`. Every PRD-specified code has a `with_candidates` payload (`auto_type_param.rs:347-349, 484-489, 622-627, 933-934, 985-986, 1004-1007`).
- **Note:** WIRED at the conversion layer; rendered to the user only via M-009 — i.e. only iff a producer ever calls the orchestrator. Library-level diagnostic-surface tests cover the code/data wire format directly.

### M-016: v0.1 example corpus determinism + perf regression guard (criteria 11, 12)

- **State:** FICTION
- **Failure mode:** F4 (test scaffolding shipped; not the regression guard the PRD specifies)
- **Evidence:** `examples/bearing_auto_seal.ri` shipped (task 7); `crates/reify-eval/tests/auto_type_param_determinism_tests.rs` covers Phase A → C determinism by loading the fixture and *manually* constructing `AutoTypeParam` instances (lines 80-130) — it does **not** load source-level `Bearing<auto: Seal>` (per its own header at lines 8-15). Run-twice-hash-equality on the *resolved snapshot* (criterion 11 wording: "Same source produces same resolution choice across runs and across machines") does not exist as a test; the closest is the `topology_fingerprint` equality test (`topology_trigger_tests.rs:80-93`). Criterion 12 (per-param resolution cost bounded; no regression on v0.1 corpus) is not exercised by any perf test in the workspace search.
- **Blocks:** Criteria 11, 12 acceptance.
- **Note:** Counted FICTION because the PRD specifies *end-to-end source-level* determinism and corpus perf — both of which require M-009 + M-010. The library-level determinism is well-tested.

## Cross-PRD breadcrumbs

- **Sibling v0.2 PRD `docs/prds/v0_2/auto-resolution-backtracking.md`** has already been audited (findings at `docs/architecture-audit/findings/auto-resolution-backtracking.md`). My M-005, M-009, M-010, M-013 are duplicates of its M-001, M-014, M-002, M-013 — Phase 3 should dedupe and decide whether the v0.2 audit "inherits" these as origin-on-v0.1 or treats them as independently surfaced.
- **Architecture §6.2-6.4** (referenced by PRD criterion 7) defines `SchemaNode` as one of six topology-change sources and the elaboration-evaluation cycle. The `SchemaNode` naming mismatch (M-008) likely affects every architecture-§6.x-referencing PRD; Phase 3 may want a sweep.
- **Architecture §2.5** (monotonic-feasible / undef-does-not-falsify) is the basis of Phase B's `Indeterminate = feasible` semantics. The constraint-checker contract lives in `crates/reify-constraints/`; any PRD that reuses Phase B's feasibility primitives implicitly depends on §2.5.
- **PRD §"Dependencies" #66 (trait conformance)** is `entity::satisfies_trait_bound` (M-002). This same predicate is used by `conformance/checker.rs` and `trait_typed_param` tests; if Phase 3 audits the trait-conformance PRD separately, this is the same code.
- The deferred type-substitution work (M-013) is also a prerequisite for the **`@optimized fn` lowering / ComputeNode infrastructure** referenced by FEA-style PRDs — both need *some* mechanism for substituting concrete types into a parameterized template. Phase 3 may want to bundle.
