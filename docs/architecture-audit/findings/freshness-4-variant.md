# Audit: Freshness 4-Variant Cache State

**PRD path:** `docs/prds/freshness-4-variant.md`
**Auditor:** audit-freshness-4-variant
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 5 (mechanisms not in WIRED state)

## Top concerns

- **`propagate_freshness_only` walk has zero production callers.** The function and its test suite are extensive (`freshness_walk.rs`, 1300+ lines including tests), but no engine code path invokes it outside tests and `gating.rs` self-tests. The freshness-only-walk acceptance criterion (PRD task 6, arch §3.5) is implemented in the abstract but is not wired into any edit/eval cycle. Without a production driver, "value-unchanged-but-freshness-changed" transitions never actually propagate in real engine runs.
- **Naming DRIFT: `EventKind::error` (PRD/spec/arch §8.2) vs `EventKind::Failed` (code).** The PRD's acceptance criteria and language spec line 1863 / arch line 874 both name the event `EventKind::error`; the implementation calls it `EventKind::Failed`. Functionally equivalent — but reviewers chasing the spec word will not find anything by that name. Same drift independently noted in `docs/reviews/cross-document-review.md` (realization vs realisation).
- **`still_refining` flag is plumbed but never driven `true` in production.** `derive_output_freshness(still_refining=true, ...) → Intermediate{generation}` is wired and unit-tested, but every production call site (e.g. `engine_eval.rs:2835`) hard-codes `still_refining=false` with a comment noting "no progressive nodes exist yet (that is PRD task 4+ scope)" — yet this freshness-4-variant PRD's task 4 is the `Failed` path, not progressive emission. The progressive-eval producer is the `node-trait-composition` PRD (`NodeTraits::PROGRESSIVE`, arch §7.6). So `Freshness::Intermediate` is reachable in production only via dirty-flag bulk mark and via downstream propagation from a (currently non-existent) Intermediate input — not via a self-driven refining state.
- **No GUI snapshot smoke-test on `examples/m5_purpose.ri`.** The PRD AC #7 says "Smoke-test: open `examples/m5_purpose.ri` or any file with intermediate / failed nodes and confirm by snapshot or inspection." Per-component unit tests exist (`DesignTree.test.tsx`, `PropertyEditor.test.tsx` cover the four data-freshness states), but there's no end-to-end snapshot/visual-regression covering the file mentioned in the AC. The MCP debug-window screenshot tooling required for that would touch the `screenshot_window` task family.

## Mechanisms

### M-001: `Freshness` enum with four variants

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/value.rs:2419-2466` — public enum with `Final | Intermediate { generation: u64 } | Pending { last_substantive: ResultRef } | Failed { error: ErrorRef }`. Doc comments cite arch §7.1 lines 716-728 and §9.2 lines 880-890. Unit tests `:2976-3030` exercise round-trips for each variant. `Default::default() = Final` and `is_final()` const-fn helper (`:7511-7570`).
- **Blocks:** N/A
- **Note:** Variant set is byte-exact with the PRD spec. `Default = Final` pins the §7.1 read-on-absent contract.

### M-002: `ResultRef` opaque carrier for last-substantive result

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/value.rs:2308-2342` — `pub struct ResultRef(Option<ContentHash>)` with private field; `none()` / `of_hash()` constructors, `has_hash()` / `content_hash()` accessors. Doc comments cite arch §7.1.
- **Blocks:** N/A
- **Note:** Two-state semantics (no prior result vs. content-hash-identified). Opaque-by-construction satisfies the PRD's "implementation detail" caveat for `ResultRef`.

### M-003: `ErrorRef` opaque carrier with optional `DiagnosticCode`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/value.rs:2344-2410` — wraps `EvalError` with optional `DiagnosticCode`. `with_code()` builder, `message()` / `code()` accessors, `From<EvalError>` for ergonomics. `DiagnosticCode` enum at `crates/reify-types/src/diagnostics.rs:156-...` with non-exhaustive variants including `ConstraintViolated`, `TraitNotImplemented`, etc.
- **Blocks:** N/A
- **Note:** PRD dependency on #2253 (typed DiagnosticCode) appears satisfied; the integration point is `ErrorRef::with_code(DiagnosticCode::...)`. The audit did not confirm #2253 task status, but the codebase artifact exists and is usable.

### M-004: `NodeCache` (cache entry) carries `freshness` field

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/cache.rs:162-188` — `pub freshness: Freshness` + `pub pending_cause: Option<NodeId>` side-table on `NodeCache`. `record_evaluation_propagating_freshness` constructs entries with the derived freshness.
- **Blocks:** N/A
- **Note:** PRD task 2 done. The `pending_cause` companion field is the diagnostic-chain side-table that the PRD's "with diagnostic chain" language refers to.

### M-005: Single API surface for cache freshness reads/writes

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/cache.rs:720-770` — `CacheStore::freshness(&NodeId) → Freshness` (canonical reader, returns `Freshness::default()` for absent entries) and `set_freshness(&NodeId, Freshness) → bool` (canonical writer with `Pending`/`Failed` precondition panic). Specialized writers `mark_failed`, `mark_pending`, `mark_pending_with_cause`, `restore_final` enforce variant-specific invariants. `#[must_use]` on set_freshness for absent-node returns. Precondition tests at `:2786-2860`.
- **Blocks:** N/A
- **Note:** PRD task 2 AC ("a single API surface") satisfied. `Engine::freshness(&NodeId)` re-exports the reader.

### M-006: §7.2 propagation rule (input freshnesses → output freshness)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/cache.rs:1001-1135` — `derive_output_freshness(still_refining, inputs, generation) → Freshness` truth-table function with `_with_cause` variant returning `(Freshness, Option<NodeId>)`. Truth-table unit test at `:2862-2920` (`derive_output_freshness_implements_arch_7_2_truth_table`). Production wire via `record_evaluation_propagating_freshness` at `engine_eval.rs:2830-2836`.
- **Blocks:** N/A
- **Note:** The truth table extends arch §7.2 (which only addresses Intermediate inputs) with §9.2's Failed → Pending carve-out, all in one helper. `Pending` and `Failed` inputs both push the output to `Pending` (with `last_substantive: ResultRef::none()`); `Intermediate` inputs push to `Intermediate{generation}`.

### M-007: `still_refining` flag plumbing

- **State:** PARTIAL
- **Failure mode:** F4 (mechanism plumbed but no production driver — progressive emission deferred to a separate PRD)
- **Evidence:** `crates/reify-eval/src/cache.rs:1023-1029` accepts `still_refining: bool`. The only production call site, `engine_eval.rs:2835`, hard-codes `false` with comment: "`still_refining=false` is the only valid value today — no progressive nodes exist yet (that is PRD task 4+ scope)." Test-only synthetic exercise in `tests/progressive_emission.rs` injects Intermediate states by hand.
- **Blocks:** none directly — but the lack of progressive producers means the "self-refining → output Intermediate" arm of §7.2 is exercised only synthetically.
- **Note:** The comment incorrectly attributes the progressive-eval driver to "PRD task 4+ scope" of this PRD; actual driver is `NodeTraits::PROGRESSIVE` from `node-trait-composition.md` (arch §7.6). DRIFT-adjacent: the §7.2 mechanism is implemented faithfully, but the spec's self-refining producer story belongs to a different PRD that has not landed.

### M-008: Pre-eval Pending gate (quiet downstream on Failed/Pending inputs)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_eval.rs:2684-2745` — peeks at input freshness via the freshly-built dependency trace, and if `derive_output_freshness_from_trace_with_cause` returns `Pending` + a cause, calls `mark_pending_with_cause` and skips eval_expr to preserve `last_substantive`. Pinned by `tests/failed_propagation.rs:270-...` chain test.
- **Blocks:** N/A
- **Note:** This is the §7.2 line 748 / §9.2 line 890 implementation. The cold-start fallback (entry absent) is documented but explicitly untested.

### M-009: `Failed` failure path — panic boundary

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_eval.rs:2755-2810` — `panic::catch_unwind` around eval_expr; on panic, writes a stub Undef result, calls `cache.mark_failed(error)`, and records `EventKind::Failed { error }` on the journal. Test-instrumentation hook `set_panic_on_eval(ValueCellId)` at `engine_admin.rs` is `#[cfg(any(test, feature = "test-instrumentation"))]`. Pinned by `tests/failed_propagation.rs:60-119` (exactly-one event, NodeId scoping, no Completed event).
- **Blocks:** N/A
- **Note:** AC #4 panic-injection clause met. `tests/failed_propagation.rs:147-...` also pins the `remove_panic_on_eval` recovery branch (Failed → Final after re-eval).

### M-010: `Failed` failure path — kernel-error realizations

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_build.rs:581-617` and `:830-867` (parallel paths) — `kernel_error: Option<ErrorRef>` accumulator captured during step-extract; if `Some(error)`, the realization NodeId gets `mark_failed(error)` + `EventKind::Failed` event. Helper at `:1637+` (`step_extract_*` family). Pinned in `tests/failed_propagation.rs:530-...` via `FailingMockGeometryKernel`.
- **Blocks:** N/A
- **Note:** Realization side. Out-of-bounds index / missing-key map (PRD task 4 examples) at the *expression* level are NOT explicitly routed to Failed — they currently surface as `Value::Undef` per Kleene semantics (see `engine_eval.rs:537, 1421` comments). Whether that counts as a gap depends on PRD intent; flagging as adjacent to M-016 below.

### M-011: `Pending` propagation downstream of `Failed` with chain

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/cache.rs:585-640` (`mark_pending`, `mark_pending_with_cause`) plus `pending_cause: Option<NodeId>` side-table. Chain-forwarding logic in `derive_output_freshness_with_cause` at `:1080-1135` — Pending input forwards its `pending_cause` upstream; Failed input contributes its own NodeId as cause. Pinned by `tests/failed_propagation.rs:270-388` chain depth ≥ 2 test (a → b → c; panic on a; b's pending_cause = a; c's pending_cause = a chain-forwarded).
- **Blocks:** N/A
- **Note:** AC #4 chain clause met. `mark_pending` (no-cause) intentionally clears stale chain; `mark_pending_with_cause` is canonical for §9.2 propagation.

### M-012: §9.3 separation — constraint violations stay on `Satisfaction::Violated`, never produce `Freshness::Failed`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_constraints.rs:271+` doc explicitly carves out: violations route through `ConstraintCheckEntry` + `DiagnosticCode::ConstraintViolated`, NOT through `Freshness::Failed`. Pinned by `tests/failed_propagation.rs:454-528` (`constraint_violation_does_not_set_failed_freshness_or_emit_failed_event`). LSP-side sanity test at `crates/reify-lsp/src/diagnostics.rs:1089+` also asserts the channel split.
- **Blocks:** N/A
- **Note:** AC #6 explicitly met. Channel separation is mechanism-load-bearing — the constraint subsystem and the freshness subsystem use disjoint diagnostic infrastructure.

### M-013: `propagate_freshness_only` — freshness-only propagation walk

- **State:** PARTIAL
- **Failure mode:** F5 (implementation + tests exist; no production caller — value-unchanged-but-freshness-changed transitions never actually propagate at runtime)
- **Evidence:** `crates/reify-eval/src/freshness_walk.rs:50-200` defines the walk; `:392-1410` exhaustive test suite covers Final↔Intermediate↔Pending↔Failed transitions, Compute-node fan-out (M-006 of compute-node-infrastructure audit), and idempotency. **But:** grepping production code (excluding `tests/` and the function's own self-tests) finds zero callers: `grep -rn "propagate_freshness_only" crates/reify-eval/src/engine*.rs crates/reify-eval/src/lib.rs crates/reify-runtime/src/` returns nothing. Only `crates/reify-eval/src/gating.rs` lines 457/479 call it — and those are inside `#[cfg(test)]` (lines 391-...).
- **Blocks:** AC #5 (freshness-only propagation walks the dirty cone) — without a production caller, this AC is structurally unmet at runtime. Any future "upstream Intermediate → Final without value change" transition will go un-propagated.
- **Note:** This is the biggest functional gap in the PRD. The mechanism is correct and well-tested in isolation, but unwired. A natural call site would be incremental-edit handlers (`engine_edit.rs::edit_param` / `edit_source`) that currently use `mark_pending` bulk passes; another would be when a kernel job completes and an upstream node flips Intermediate→Final. Neither has been retrofitted.

### M-014: `EventKind::Failed` event (PRD names it `EventKind::error`)

- **State:** DRIFT
- **Failure mode:** F7 (naming drift — PRD/spec/arch §8.2 vs. code)
- **Evidence:** PRD lines 23, 30, 43, 53 reference `EventKind::error`. `docs/reify-language-spec.md:1863` and `docs/reify-implementation-architecture.md:874` use the same name. The code defines `EventKind::Failed { error: ErrorRef }` at `crates/reify-eval/src/journal.rs:43`. Functionally equivalent. Cross-confirmed by `docs/reviews/cross-document-review.md:11` (realization/realisation spelling drift, same area).
- **Blocks:** No tasks blocked — purely cosmetic. But anyone grepping the spec for `EventKind::error` will find nothing.
- **Note:** Trivial to resolve by renaming the variant or updating the spec; routing to Phase 3.

### M-015: GUI badge surfacing (four distinct states)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src-tauri/src/types.rs:492-510` — `format_freshness(&Freshness) -> &'static str` maps to lowercase tags `"final" | "intermediate" | "pending" | "failed"`. `engine.rs:1521+` threads it into Value/Outline payloads. CSS in `gui/src/panels/{DesignTree,PropertyEditor}.module.css` selectors `[data-freshness="..."]` provide distinct visual styling for each non-Final state. Unit tests at `gui/src/__tests__/DesignTree.test.tsx:970-1000` and `PropertyEditor.test.tsx` cover all four states.
- **Blocks:** N/A
- **Note:** AC #6 four-distinct-badges met for component-level rendering. Container nodes use a fifth synthetic tag `"aggregate"` (DRIFT-lite — see M-016).

### M-016: GUI `"aggregate"` synthetic tag for container nodes

- **State:** DRIFT
- **Failure mode:** F7 (GUI introduces a 5th freshness-string variant not in the PRD's 4-variant spec)
- **Evidence:** `gui/src-tauri/src/engine.rs:2272-2287` — sub-component container nodes hard-code `freshness: "aggregate".to_string()` because they have no single backing `NodeId`. `gui/src/panels/DesignTree.tsx:198` excludes `'aggregate'` from badge rendering. Test `gui/src-tauri/src/tests/engine_tests.rs:5331-5332` pins the convention.
- **Blocks:** N/A
- **Note:** This is a presentation-layer convention, not a 5th `Freshness` enum variant — the back-end enum is still 4-variant. But it widens the wire-tag domain to 5, which the PRD AC #6 doesn't anticipate. Future "drill into a container to see per-child freshness" UX work might collapse this.

### M-017: LSP diagnostic surfacing of Pending/Failed via existing diagnostic channel

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-lsp/src/diagnostics.rs:257-327` — emits `Diagnostic` entries for cells whose freshness is `Failed { error }` (code `"computation-failed"`) or `Pending { .. }` (code `"computation-pending"`); `Final | Intermediate` produce no diagnostic. Threaded via the existing diagnostic pipeline, no protocol changes. Constraint-violation channel separation test at `:1989+`. Unit tests at `:1397-1700`.
- **Blocks:** N/A
- **Note:** PRD scope §6 ("LSP plumbing — pass freshness through existing diagnostic / hover surfaces only") met. Intermediate freshness is intentionally not surfaced as a diagnostic per the PRD's "spinner / progress" UI hint (handled in GUI badges, not LSP).

### M-018: GUI smoke-test on `examples/m5_purpose.ri`

- **State:** TODO
- **Failure mode:** F5 (AC explicitly names an example file; no test or recorded snapshot references it)
- **Evidence:** `examples/m5_purpose.ri` exists. `grep -rn "m5_purpose" gui/` returns nothing. AC #7: "Smoke-test: open `examples/m5_purpose.ri` or any file with intermediate / failed nodes and confirm by snapshot or inspection."
- **Blocks:** None directly — but the PRD's stated AC is unmet at the integration level. Per-component unit tests do exist (M-015), so the underlying mechanism is exercised; just not via the named smoke path.
- **Note:** Low-risk gap. The AC's "or any file" clause permits any equivalent; the per-component unit tests arguably satisfy the spirit. Calling out for Phase 3 visibility.

## Cross-PRD breadcrumbs

- **Compute-node-infrastructure PRD audit (`findings/compute-node-infrastructure.md`)** independently flagged `Freshness::Pending` reuse as the leading candidate for ComputeNode running-state representation (M-013 there). The mechanism here is sufficient — `pending_cause` chain works the same way for any `NodeId::Compute` entry once compute-node dispatch (M-014 of that PRD, currently FICTION) starts producing live ComputeNodes.
- **Node-trait-composition PRD (referenced obliquely)** — owns the `still_refining=true` producer side (the `PROGRESSIVE` node trait, arch §7.6). Without it landing, M-007 here remains test-only-exercised. The misattributed comment in `engine_eval.rs:2827` ("PRD task 4+ scope") should redirect blame to that PRD, not this one.
- **Persistent-fea-cache PRD** — persistent caching across runs (PRD §"Out of scope" excludes it from freshness-4-variant) presumably touches `result_hash` and `ResultRef` content-hash identity. If that PRD lands and stores `ResultRef::of_hash(...)` across sessions, the opaque-ID assumption (M-002) becomes load-bearing for persistence too.

## How I read the failure-mode codes used here

The audit-brief catalog defines F1–F7 informally. The compute-node-infrastructure findings file uses F2/F3/F4/F6 with prose elaboration. I follow that precedent:

- **F4** = mechanism named/plumbed, real driver deferred or absent (used for M-007).
- **F5** = AC-named artifact not present in code (used for M-013, M-018).
- **F7** = naming or shape drift between PRD/spec and code (used for M-014, M-016).
