# Audit: Per-Purpose Representation Tolerance Contract

**PRD path:** `docs/prds/v0_2/per-purpose-tolerance.md`
**Auditor:** audit-per-purpose-tolerance
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 6

## Top concerns

- **DRIFT between stdlib `Input` trait and the eval-side import-promise extractor.** The stdlib `io.ri` `Input` trait has no `tolerance` parameter — its tolerance attribute is `Provenance.tolerance_guarantee` (an indirection through the `provenance` parameter). The eval-side `extract_input_tolerance_promise` (and the wired `emit_imported_tolerance_promise_diagnostics_for_module`) looks up `ValueCellId(input_template_name, "tolerance")`. Production-stdlib imports never satisfy gate 1; the entire imported-geometry promise pipeline is end-to-end live only against synthetic test templates (`step_input_template` builds bespoke `tolerance` cells). This is the load-bearing concern for Phase 3.
- **Per-purpose tolerance is wired into the v0.2 OCCT single-kernel path; the multi-kernel motivator is still scaffolding.** Cache (`RealizationCache`), bucket, scope, combine, budget primitives and the `build()` integration all exist with passing e2e tests; but `compute_realization_tolerance_budget` hard-codes `BUDGET_QUERY_TRIPLE_V02 = (BooleanUnion, BRep, &[BRep])`, so every plan is 0-conversion and the per-stage `^(1/N) × 0.8` formula degenerates to pass-through. The conversion-chain motivator of the PRD only fires when `dispatcher::dispatch` is wired into op execution — explicitly deferred to task 2642 in `dispatcher.rs:35-37`.
- **Tolerance-scope extraction is MVP-clipped: only bare-param subjects.** `tolerance_scope.rs:14-19` openly states member-access subjects like `RepresentationWithin(subject.head, tol)` are deferred; the "entity-level escape hatch" the PRD treats as a peer to purpose-level scope is achieved only via overlapping purposes today.
- **Long-chain diagnostic is constructed but never emitted.** `long_chain_diagnostic` is scaffolding with a `TODO(task-2642)` comment and no in-tree caller (`dispatcher.rs:148-157`). The PRD's "long-chain warning at >2 stages AND >500ms" predicate is wired functionally but unobservable on the build path.

## Mechanisms

### M-001: `RepresentationWithin(subject, tolerance)` constraint recognition on purposes

- **State:** PARTIAL
- **Failure mode:** F1 (compile-time contract → no runtime backing for the documented superset)
- **Evidence:** `crates/reify-eval/src/tolerance_scope.rs:71-172` (extractor + bare-param gate); module docstring `tolerance_scope.rs:13-19` explicitly defers member-access subjects (`RepresentationWithin(subject.head, tol)`); single-binding contract enforced by debug_assert at `tolerance_scope.rs:160-170`
- **Blocks:** Entity-level override semantics ("tighter entity-level overrides" per PRD §"Tolerance lives at the purpose")
- **Note:** Today recognises only `RepresentationWithin(<bare-purpose-param-StructureRef>, <LENGTH-literal>)`. Member-access subjects and multi-param subjects are deferred to a follow-up. PRD says "the runtime extracts these into a tolerance scope"; this is half-extracted.

### M-002: Tolerance-scope inheritance via dot-prefix descendant propagation

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/tolerance_scope.rs:183-216` (`propagate_subject_to_descendants` + `merge_with_min`); tests at `tests/tolerance_scope.rs:537-578`; e2e pin `tests/tolerance_wiring_e2e.rs:1146` (eval-then-activate-then-build preserves scope)
- **Blocks:** None
- **Note:** Walks `PersistentMap<ValueCellId, ValueCellNode>` with `id.entity == subject || id.entity.starts_with(prefix)` (dot-boundary safe). Multi-purpose contributors are `min`-folded for "tighter satisfies looser" semantics. Solid for the bare-param subject case.

### M-003: `Engine::active_tolerance_for(entity_ref) -> Option<f64>` lookup

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_tolerance.rs:126-128` (delegates to `Engine.active_tolerance_scope`); `engine_purposes.rs:315-324` (scope re-extracted on each `activate_purpose`); `tests/tolerance_scope.rs:44-121` (activate/deactivate round-trip)
- **Blocks:** None
- **Note:** The demand-side single-entry query that the cache-key path consumes. Scope is recomputed when purposes activate/deactivate.

### M-004: Float tolerance carried as SI metres (no class enum)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/tolerance_gate.rs:43-45` (`is_valid_tolerance_si` canonical predicate); every `tolerance_*` module funnels through it; PRD §"Float tolerance, not class enum" pinned by `tolerance_promise.rs:339-353`, `tolerance_scope.rs:153`, etc.
- **Blocks:** None
- **Note:** Consistent finite-and-non-negative gating across `tolerance_promise`, `tolerance_combine`, `tolerance_scope`, `tolerance_bucket`, `tolerance_budget`, `dispatcher`. Cross-extractor symmetry is structural — single source of truth.

### M-005: `RealizationCache` keyed on `(entity_id, repr_kind, tolerance)` with partial-order lookup

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/realization_cache.rs:62-141`; `crates/reify-eval/src/tolerance_bucket.rs:34-146` (inner bucket + `<=` lookup + SOFT_CAPACITY=5 eviction); engine field `crates/reify-eval/src/lib.rs:612`; e2e at `tests/tolerance_wiring_e2e.rs:226` and `:296`
- **Blocks:** None
- **Note:** Three-dimensional key (`entity_id`, `ReprKind`, `f64 tol`), bounded bucket (≤5), `cached_tol ≤ requested_tol` "tighter satisfies looser" rule. Reset on `edit_param` / `edit_source` (engine_edit.rs:895, 2003).

### M-006: Realization cache short-circuit in `build()` / `build_snapshot()`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `engine_build.rs:1664-1694` (cache-hit short-circuit guarded on `demanded_tol.is_some() && realization_name.is_some()`); `compute_demanded_tols` at `engine_build.rs:1227-1238`; e2e at `tests/tolerance_wiring_e2e.rs:296`
- **Blocks:** None
- **Note:** Cache key only includes `ReprKind::BRep` today (v0.2 OCCT-only); cache-key is anonymous-realization-aware (skip cache for unnamed realizations — pinned by `tests/tolerance_wiring_e2e.rs:1229`).

### M-007: `combine_demanded_tolerance(output_bound, purpose_bound)` min-fold

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/tolerance_combine.rs:47-69`; `extract_output_tolerance_bound` at `tolerance_combine.rs:130-212`; engine query at `engine_tolerance.rs:180-193`; truth-table pinned at `tests/tolerance_combine.rs`
- **Blocks:** None
- **Note:** Combines an output occurrence's `RepresentationWithin` bound with active-purpose scope bound; both share SI-metre units; min wins. Output-bound extractor mirrors the purpose-side recognition gates (TODO at `tolerance_combine.rs:122-129` notes the duplicated shape recognition).

### M-008: Per-stage tolerance budget allocation (`requested^(1/N) × 0.8`)

- **State:** PARTIAL
- **Failure mode:** F2 (mechanism exists but real-world inputs are degenerate)
- **Evidence:** `crates/reify-eval/src/tolerance_budget.rs:46-59` (formula); `dispatcher.rs:265-318` (`per_stage_tolerance_for_plan`); `engine_build.rs:1145-1161` (`compute_realization_tolerance_budget`); but `BUDGET_QUERY_TRIPLE_V02 = (BooleanUnion, BRep, &[BRep])` at `engine_build.rs:1184-1185` keeps `n_stages` at 0 → pass-through under v0.2 OCCT-only
- **Blocks:** Multi-kernel conversion-chain budget pressure (the PRD's primary motivator for the formula)
- **Note:** Formula and tests are real; the production call site forces `n_stages == 0` so the safety-factor never bites. Will activate when v0.3 multi-kernel adapters wire `dispatcher::dispatch` into op execution (deferred to task 2642 per `dispatcher.rs:33-41`).

### M-009: Imported-geometry tolerance promise extraction from `Input` occurrence's `tolerance` parameter

- **State:** DRIFT
- **Failure mode:** F1 (PRD/extractor name a `tolerance` param; stdlib `Input` trait has no such param)
- **Evidence:** `crates/reify-eval/src/tolerance_promise.rs:106-170` reads `ValueCellId(input_template_name, "tolerance")`; **but** stdlib `crates/reify-compiler/stdlib/io.ri:65-68` declares `trait Input : Source { param source : String; param provenance : Provenance }` with the tolerance attribute living on `Provenance.tolerance_guarantee` (`io.ri:43-48`); test fixtures (`step_input_template`) bypass the trait and write a bespoke `tolerance` cell
- **Blocks:** Production users importing STEP/STL via the stdlib `Input` trait will never trigger `ImportedTolerancePromiseInsufficient` / `InputTolerancePromiseIsZero` diagnostics; e2e tests use synthetic templates that don't match the stdlib trait
- **Note:** Either (a) the extractor needs to walk `provenance.tolerance_guarantee`, or (b) the stdlib `Input` trait needs a top-level `tolerance` parameter, or (c) the PRD's "parameter of the `Input` occurrence" wording needs to be reconciled with the v0.1-shipped stdlib that uses `provenance.tolerance_guarantee`. Phase 3 question.

### M-010: `Engine::check_imported_tolerance_promise` + production emission on `build`/`build_snapshot`/`tessellate_*`

- **State:** WIRED
- **Failure mode:** N/A (mechanism is wired correctly; M-009 is the upstream gate that silently blocks it in production)
- **Evidence:** `engine_tolerance.rs:69-109` (two-branch dispatch: zero-promise lint + insufficient lint); `engine_tolerance.rs:235-296` (`emit_imported_tolerance_promise_diagnostics_for_module` — walks `module.templates` filtering by extractor success, then for every (Input × Output × active-purpose-binding) triple forwards diagnostics); production call sites at `engine_build.rs:512, 758, 1001, 2232`
- **Blocks:** None directly; transitively gated by M-009
- **Note:** Both `ImportedTolerancePromiseInsufficient` and `InputTolerancePromiseIsZero` diagnostic codes are emitted; strict-`<` rule for insufficient. The wiring is correct — the upstream extractor is the blocker.

### M-011: Long-chain realization diagnostic (>2 stages AND >500ms wall)

- **State:** PARTIAL
- **Failure mode:** F4 (predicate + builder exist; no in-tree caller emits it)
- **Evidence:** `dispatcher.rs:125-131` (`is_long_chain_realization` predicate); `dispatcher.rs:179-207` (`long_chain_diagnostic` builder with `DiagnosticCode::LongChainRealization`); `dispatcher.rs:227-263` (env-var override `REIFY_LONG_CHAIN_THRESHOLD_MS`); explicit `TODO(task-2642)` at `dispatcher.rs:150-157`: "wire into the realization timing loop in `geometry_ops.rs` once the kernel-registry mechanism + OCCT adapter migration lands"
- **Blocks:** User-facing visibility into long-chain budget pressure
- **Note:** Strict-`>` boundaries on both stage count (>2 ⇒ ≥3) and elapsed (>500ms). Unit-tested but unreachable from `build()` because dispatch is not yet on the hot path.

### M-012: Stdlib helper for explicit re-meshing/healing of imported geometry

- **State:** FICTION
- **Failure mode:** F1 (PRD calls for a stdlib helper; no implementation)
- **Evidence:** PRD §"Imported geometry promise" — "Users opt into explicit re-meshing/healing through a stdlib helper rather than the runtime silently doing it." Grep across stdlib + eval crate yields no `re-mesh`/`remesh`/`heal_input`/`heal_imported`/`reify_remesh` helper for STEP/STL imports. (`reify-shell-extract`'s `max_remesh_iterations` is FEA-shell-mesh extraction, not imported-geometry remediation.)
- **Blocks:** PRD's "Users opt into re-meshing/healing" path; without it the diagnostic at M-010 has no documented remediation
- **Note:** PRD treats the stdlib helper as the user-visible remediation when a downstream demand exceeds the imported promise. The diagnostic warns but the suggested remediation does not exist as code.

### M-013: `Input` / `Output` occurrence template recognition in the build path

- **State:** PARTIAL
- **Failure mode:** F5 (recognition is by shape, not by trait — risks false matches and false negatives)
- **Evidence:** `engine_tolerance.rs:251-278` identifies Input templates by `extract_input_tolerance_promise(...).is_some()` and Output templates by `extract_output_tolerance_bound(...).is_some()`. There is no check that the template actually conforms to `trait Input` or `trait Output` from stdlib `io.ri`. Cross-product over `(Input × Output × active-purpose-binding)` triples may dispatch on shape coincidences.
- **Blocks:** Soundness of `emit_imported_tolerance_promise_diagnostics_for_module` against non-trait templates
- **Note:** A template that happens to declare both a `tolerance` cell and a `RepresentationWithin` constraint (regardless of whether it conforms to `Input` / `Output` traits) will participate in the dispatch. Arch §14.5 boundary-contract framing assumes trait conformance; the implementation relies on shape coincidence.

### M-014: Tolerance change triggers full recompute (no incremental cache invalidation by tolerance)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `engine_edit.rs:895, 2003` flush the entire `realization_cache` on `edit_param` / `edit_source`; PRD §"Tolerance lives at the purpose" says "a tolerance change is a major design decision and a full recompute is acceptable"
- **Blocks:** None
- **Note:** PRD-accepted blunt-instrument invalidation. The PRD justifies it as "tolerance change is a major design decision".

## Cross-PRD breadcrumbs

- **`multi-kernel.md`** — M-008 (per-stage budget) and M-011 (long-chain diagnostic) both depend on multi-kernel dispatch being wired into op execution (task 2642). Until then, both mechanisms are scaffolding. The long-chain diagnostic's PRD reference is shared between `multi-kernel.md` and `per-purpose-tolerance.md`.
- **`imported-field-source.md`** — M-009 / M-010 share the `Input` occurrence boundary contract (arch §14.5). The `field_import_provenance.rs` machinery (`Provenance` builder) is task-5 of that PRD's decomp and reads from `Input.provenance` cells, suggesting the `tolerance_guarantee` route is in fact the intended source of truth — reinforcing M-009's DRIFT classification.
- **`structural-analysis-fea.md`** / **`structural-analysis-shells.md`** — both may consume a purpose-active `RepresentationWithin` on subjects-with-member-access (e.g. `bracket.fea_subject`). M-001's MVP scope clip silently drops these — Phase 3 should check whether FEA decompositions assumed member-access subjects work.

## Things taken as given (not re-researched)

- GR-001 (struct-ctor runtime evaluation) does not transitively affect this PRD: tolerance values are `f64` literals, not struct instances. `RepresentationWithin` is matched as `UserFunctionCall` with literal args, not as a structure-ctor.
