# Audit: Field Source Kinds

**PRD path:** `docs/prds/field-source-kinds.md`
**Auditor:** audit-field-source-kinds
**Date:** 2026-05-12
**Mechanism count:** 23
**Gap count:** 5 (3 DRIFT, 2 ORPHAN)

## Top concerns

- **PRD is stale relative to the code it specifies.** The "v0.1 coverage per task" table at the top declares Sampled as a v0.1 *deferral diagnostic* with v0.2 implementation pending (task 2341), but task 2341 is `done` (commit db45b8c97a) and `compile_field` no longer emits `FieldSampledV02`. The v0.2 implementation has landed and is reachable from user code, but a reader of this PRD would not know it.
- **Several precise code citations have rotted.** PRD cites `eval_expr` `evaluated_args.iter().any(|v| v.is_undef())` "line ~119"; actual line is 151. PRD describes Kleene and/or short-circuits as "explicit `Bool(false)` / `Bool(true)` early-outs in the `and`/`or` dispatch arms of `eval_expr`"; semantics are correct but they actually live in helpers `eval_and` / `eval_or` (lib.rs:1516, 1541) delegating to `kleene::kleene_and/or`. Behaviour intact; documentation drifted.
- **No PRD prose covers the Composed kind beyond "TBD — task 2343".** Task 2343 is `done` (commit 2b567cd9e2). The Composed implementation (incremental re-elaboration on field edits, `check_field_composition_types`, `collect_composed_field_dependencies`) is wired and tested but the PRD never describes the type-composition rules or cache-invalidation semantics — they exist only as code + tests. This is the inverse of GR-001's pattern (PRD assumes mechanism, code lacks it) — here code has mechanism, PRD lacks specification.
- **No PRD prose covers the Imported kind beyond "TBD — task 2344".** Task 2344 (the deferral diagnostic) is `done`. A separate v0.2 PRD (`docs/prds/v0_2/imported-field-source.md`) covers the actual import-pipeline design. This PRD points there only by task number, not by cross-reference link. A reader landing on this file gets a dead end.

## Mechanisms

### M-001: `FieldSource::Analytical` compiler arm

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/functions.rs:256-300` (the `FieldSource::Analytical` arm of `compile_field`); `CompiledFieldSource::Analytical { expr }`; `field_compile_tests.rs` pins emission of analytical+lambda body; task 2336 (done, commit 07410de450).
- **Blocks:** none
- **Note:** Compiles the lambda body, runs codomain check, debug_asserts the body is a Lambda when result_type is non-Error.

### M-002: `DiagnosticCode::FieldCodomainMismatch`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/diagnostics.rs:300` (variant declaration); emit at `crates/reify-compiler/src/functions.rs:283-294`; pinned by `field_compile_tests.rs:494-555` (positive + negative cases) and `type_expr_kind_dispatch_tests.rs:296`.
- **Blocks:** none
- **Note:** Mnemonic `E_FIELD_CODOMAIN_MISMATCH` matches PRD prose.

### M-003: `implicitly_converts_to` predicate

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/type_compat.rs:52` (`pub fn implicitly_converts_to`); doc explicitly notes asymmetric anti-cascade — `Type::Error` on the FROM side silently converts, but a debug_assert (line ~69) forbids `Type::Error` on the TO side.
- **Blocks:** none
- **Note:** PRD-claimed asymmetric anti-cascade contract (task-1918) is encoded in the function's own comments.

### M-004: Anti-cascade suppression in codomain check

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `functions.rs:279-282` — `!body_ty.is_error() && !codomain_type.is_error() && !field_codomain_compatible(...)`; test `field_compile_tests.rs` (line ~575-590) explicitly asserts NO `FieldCodomainMismatch` fires when a prior parse failure / domain failure poisoned a type.
- **Blocks:** none
- **Note:** Both conditions enforced as PRD describes.

### M-005: Int→Real widening in codomain check

- **State:** DRIFT
- **Failure mode:** N/A (informational drift)
- **Evidence:** PRD prose does not mention this rule at all, yet `field_codomain_compatible` in `functions.rs:172` deliberately accepts `(Type::Int, Type::Real)` as a widening exception on top of `implicitly_converts_to`. Pinned by `field_compile_tests.rs:575-590` ("expected NO FieldCodomainMismatch for Real->Real field with Int literal body").
- **Blocks:** none
- **Note:** Behaviour is sensible (whole-number literals parse as `Int`) but the PRD's "compile-time codomain type-check" section is silent on it. Phase 3 may want PRD prose to be updated.

### M-006: `sample(field, point)` dispatch arm

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/lib.rs:158` (`"sample" if evaluated_args.len() == 2`); pinned by `crates/reify-expr/tests/field_eval_tests.rs::sample_propagates_undef_point_argument` (line 1211) and many surrounding tests.
- **Blocks:** none
- **Note:** Dispatch matches against `FieldSourceKind` to route to Analytical/Sampled/Composed/Gradient/Divergence/Curl/Laplacian/VonMises/PrincipalStresses/MaxShear/SafetyFactor backends; first three correspond to PRD.

### M-007: Single-param lambda — whole-point binding

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `apply_lambda_with_point_unpacking` at `lib.rs:1095-1110`; comment "A single-param lambda (params.len() == 1) always receives the whole Point/Vector unchanged (no unpacking)".
- **Blocks:** none
- **Note:** Matches PRD exactly.

### M-008: Multi-param lambda — component unpacking

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `lib.rs:1101-1106`; matches `Value::Point | Value::Vector` with `params.len() == items.len()`. PRD's claim that the point must be a `Value::Point` or `Value::Vector` AND lengths must equal is exact.
- **Blocks:** none
- **Note:** Fallthrough (length mismatch / scalar arg) passes the whole input as a single arg, hitting M-009's arity check.

### M-009: Arity-mismatch returns Undef

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `apply_lambda` at `lib.rs:1066-1068` — `if args.len() != params.len() { return Value::Undef; }`.
- **Blocks:** none
- **Note:** PRD-claimed Undef on arity mismatch is direct.

### M-010: Strict-Undef argument short-circuit (Kleene rule 1)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `lib.rs:151` — `if evaluated_args.iter().any(|v| v.is_undef()) { return Value::Undef; }` (within `FunctionCall` arm, before dispatch). PRD cites "line ~119"; actual is 151 — minor citation rot. Test: `field_eval_tests.rs:1211 sample_propagates_undef_point_argument`.
- **Blocks:** none
- **Note:** Behavioural contract intact; line number stale.

### M-011: Undef captured in lambda environment

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** PRD's rule 2 is not exercised by a named regression test in `field_eval_tests.rs`; the mechanism reduces to "captured cell holds Undef → ValueRef lookup returns Undef → per-op Kleene rule 3 propagates". The `ValueRef` path returns `Value::Undef` via `ctx.values.get_or_undef(id)` at `lib.rs:142`.
- **Blocks:** none
- **Note:** Sufficient by composition of M-012 + ValueRef get_or_undef. A direct closure-capture regression test would be valuable but its absence isn't a gap per the catalog — this is mechanically a corollary, not an independent mechanism.

### M-012: Per-op Kleene rules in arithmetic / div by zero

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `eval_binop` at `lib.rs:1468`; division-by-zero check at `lib.rs:2238`; pinned by `field_eval_tests.rs:1269 sample_propagates_undef_from_lambda_body_division_by_zero` and the truth-table tests at `lib.rs:4231/4256`.
- **Blocks:** none
- **Note:** Two regression tests directly cited in PRD both present and named verbatim.

### M-013: Kleene shortcuts for and/or

- **State:** DRIFT
- **Failure mode:** N/A
- **Evidence:** PRD says these "are implemented as explicit `Bool(false)` / `Bool(true)` early-outs in the `and`/`or` dispatch arms of `eval_expr`". They actually live in `eval_and` (`lib.rs:1516`) and `eval_or` (`lib.rs:1541`) helpers, delegating to `kleene::kleene_and / kleene_or` in `crates/reify-expr/src/kleene.rs`. `eval_binop` has `BinOp::And | BinOp::Or => unreachable!()` at line 1505 — and/or are routed before reaching binop.
- **Blocks:** none
- **Note:** Semantics correct (`return Value::Bool(false)` / `return Value::Bool(true)` on absorbing element). Citation drift only — agents grepping `eval_expr` for `Bool(false)` won't find these.

### M-014: `FieldSource::Sampled` v0.1 deferral diagnostic (`FieldSampledV02`)

- **State:** DRIFT
- **Failure mode:** N/A (PRD describes a state of the world that no longer exists)
- **Evidence:** PRD header table claims "§ Sampled — v0.1 deferral diagnostic (task 2416); v0.2 implementation (task 2341)" suggesting v0.1 emits a hard error. Task 2416 is `done` (commit 7166d913c3) but task 2341 has subsequently landed (`done`, commit db45b8c97a) — `FieldSource::Sampled` no longer emits any diagnostic at compile time. `FieldSampledV02` enum variant still exists at `crates/reify-types/src/diagnostics.rs:249` but no `compile_field` arm references it. `field_compile_tests.rs:60-73` explicitly asserts "expected zero FieldSampledV02 errors after v0.2 implementation".
- **Blocks:** none
- **Note:** ORPHAN sub-finding: the `FieldSampledV02` enum variant is unused. Likely intentional (kept for stability) but worth flagging.

### M-015: `FieldSource::Sampled` v0.2 implementation pipeline

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/functions.rs:301-392` (compile arm with 5-key validation: `grid` / `bounds` / `spacing` / `interpolation` / `data`); runtime materialization in `crates/reify-eval/src/engine_eval.rs::build_sampled_field` (line 739); sampled dispatch in `crates/reify-expr/src/sampled.rs`; `Value::SampledField` variant at `value.rs:498`. Task 2341 done.
- **Blocks:** none
- **Note:** Compile-time validation has hard errors for missing required keys, unknown keys, and duplicate keys — surfaced as PRD-DRIFT against the PRD's brief "Config syntax" sentence which doesn't mention these validations.

### M-016: Grid-kind surface — `RegularGrid1` / `RegularGrid2` / `RegularGrid3`

- **State:** DRIFT
- **Failure mode:** N/A
- **Evidence:** PRD says "Grid kinds — `RegularGrid1`, `RegularGrid2`, `RegularGrid3`, parameterised by `BoundingBox` bounds and per-axis `Length` spacing." Implementation surfaces these as **string tags**: `grid = "RegularGrid1"` (a `Value::String`), not as typed constructors. See `engine_eval.rs:705` ("`grid` — `Value::String` matching `\"RegularGrid1\"|\"RegularGrid2\"|\"RegularGrid3\"`"), `engine_eval.rs:1003-1010`, and tests at `field_compile_tests.rs:62`. The decision was made under esc-2341-149 (2026-04-29) per the in-code comment at `functions.rs:319-327`: Reify lacks anonymous struct-literal syntax and `RegularGrid*` stdlib constructors, so `grid` / `bounds` / `spacing` are surfaced as separate top-level string-keyed entries. This composition is also linked to GR-001 (struct-constructor runtime evaluation does not work).
- **Blocks:** none directly; conceptually depends on GR-001 being resolved before typed constructors could replace the string tag.
- **Note:** Phase 3 input: this is exactly the kind of "PRD says X, runtime ships Y because Reify lacks the implied mechanism" gap the audit is hunting.

### M-017: `InterpolationMethod` enum + RBF/Kriging fallback

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/interp.rs:53` (`pub enum InterpolationMethod`); `resolve_method` at line 97 maps RBF/Kriging → Linear and emits `Diagnostic` warning. Test in `field_eval_tests.rs:1492` ("task 2341 step-17" pinning the fallback). Task 2338 (interp.rs landing) is referenced by task 2341 and confirmed present.
- **Blocks:** none
- **Note:** PRD spec matches.

### M-018: `W_FIELD_OUT_OF_BOUNDS` once-per-field warning

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `DiagnosticCode::FieldOutOfBounds` at `diagnostics.rs:266`; emit site in `crates/reify-expr/src/sampled.rs::sample_at_point`; once-per-field enforcement via `AtomicBool oob_emitted` on `SampledField` (`value.rs`). Test: `crates/reify-eval/tests/field_eval_tests.rs:1421-1480` asserts exactly-one warning per field per session.
- **Blocks:** none
- **Note:** Spec-clean.

### M-019: `W_INTERPOLATION_DEFERRED` warning

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `DiagnosticCode::InterpolationDeferred` referenced from `crates/reify-kernel-openvdb/src/ingest.rs:445-468` (with comments noting the precedent), `engine_edit.rs:1007`/`2340`, `engine_eval.rs:1205`. Tests in `crates/reify-kernel-openvdb/tests/ingest_tests.rs:232-271`.
- **Blocks:** none
- **Note:** Code is wired in both consumer paths.

### M-020: `FieldSource::Composed` compile path

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `functions.rs:393-398` (compile arm produces `CompiledFieldSource::Composed { expr }`); `elaborate_field` arm sharing the Analytical evaluator at `engine_eval.rs:601-610`; incremental re-elaboration on edits at `engine_edit.rs:1111-1127` and `engine_eval.rs:1140-1146`. Task 2343 done.
- **Blocks:** none
- **Note:** PRD prose is "TBD — task 2343" but the implementation is comprehensive. See top-concerns: spec hasn't been written even though impl shipped.

### M-021: Composed-field type composition check

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `functions.rs::check_field_composition_types` (line 489); used from `compile_builder/post_passes.rs:16`. Verifies `inner_field.codomain_type → outer_field.domain_type` via `implicitly_converts_to`. Inline unit tests at `functions.rs:587-647` pin three composition rules (Vector→Tensor1, Matrix↔Tensor2 directional, etc.).
- **Blocks:** none
- **Note:** This is a substantive type system feature not mentioned in PRD prose.

### M-022: Composed-field dependency tracking for cache invalidation

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `collect_composed_field_dependencies` at `functions.rs:469`; post-pass merging captures at `compile_builder/post_passes.rs:106-140`; dependents wiring at `crates/reify-eval/src/deps.rs:894+`. Task 2343 step-5/8 commentary throughout.
- **Blocks:** none
- **Note:** PRD's "TBD" obscures this entirely — substantial mechanism, no spec.

### M-023: `FieldSource::Imported` v0.2 deferral diagnostic

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `functions.rs:399-411` emits `DiagnosticCode::FieldImportedV02` with the message "imported field sources are deferred to v0.2; v0.1 supports analytical and composed only"; variant at `diagnostics.rs:253`. Tests: `crates/reify-eval/tests/imported_field_e2e.rs:33-77`, `field_compile_tests.rs:690-705`. Task 2344 done.
- **Blocks:** none
- **Note:** PRD line 175 ("v0.2 deferral diagnostic already implemented") is accurate but it has no spec body — full design is in `docs/prds/v0_2/imported-field-source.md`.

### M-024: Cross-cutting smoke test (task 2346)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/tests/field_source_kinds_smoke.rs`; uses fixture `examples/fields/composed_stiffness.ri`. Four named tests: `composed_stiffness_ri_parses`, `composed_stiffness_compiles_with_stdlib`, `composed_stiffness_evals_with_two_field_source_kinds`, `composed_stiffness_constraints_all_satisfied`. Task 2346 done (commit f3e09f61bd).
- **Blocks:** none
- **Note:** Fixture was reduced to two field kinds after task 2416 (sampled moved out of v0.1). PRD never anticipated this reduction.

### M-025: `FieldSampledV02` enum variant (ORPHAN)

- **State:** ORPHAN
- **Failure mode:** N/A
- **Evidence:** Declared at `crates/reify-types/src/diagnostics.rs:249`; not emitted from any source. Held only because `field_compile_tests.rs:60-73` asserts it is *not* emitted post-2341.
- **Blocks:** none
- **Note:** Low priority. Phase 3 may decide between deletion (with test rewrite) and "retain for stability". Not strictly fictional — code path is intact, just unreachable.

## Cross-PRD breadcrumbs

- **GR-001 (structure-constructor runtime eval)** is the immediate parent of M-016: the choice to surface grid kinds as string tags instead of `RegularGrid1 { spacing = ..., bounds = ... }` typed constructors was forced by Reify's lack of struct-literal syntax + missing stdlib constructors. The escalation comment at `functions.rs:319-327` explicitly cites this.
- **`docs/prds/v0_2/imported-field-source.md`** owns the actual Imported pipeline (tasks 2667/2668 etc.). PRD-internal M-023 points at it via task 2344 only — a more discoverable cross-link would help.
- **`docs/prds/v0_2/sampled-field-source.md` / equivalent** — the in-code commentary at `functions.rs:318-334` references "esc-2341-149, 2026-04-29 steward" decision for the 5-key surface. There is no PRD owning the *evolved* design. This is a candidate cross-PRD coordination gap for Phase 3.

## Summary

23 mechanisms enumerated (M-001 through M-024, plus M-025 orphan).

- WIRED: 19
- DRIFT: 4 (M-005 Int→Real widening undocumented; M-013 and/or shortcut location; M-014 PRD's sampled-deferred section stale; M-015 sampled compile-time validation stricter than PRD admits; M-016 grid-kind surface diverged from PRD due to language limits)
- ORPHAN: 1 (M-025 `FieldSampledV02` enum)
- FICTION: 0
- PARTIAL: 0
- TODO: 0

The PRD is essentially **a stale spec for a feature that has shipped**: every mechanism enumerated is present in the code, but the PRD text describes an earlier moment in the feature's lifecycle (v0.1 with sampled deferred, composed/imported "TBD") that no longer matches reality.
