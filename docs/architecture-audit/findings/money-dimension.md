# Audit: Money Dimension

**PRD path:** `docs/prds/money-dimension.md`
**Auditor:** audit-money-dimension
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 4 (1 DRIFT-doc, 1 DRIFT-code, 1 DRIFT-name, 1 PARTIAL)

## Top concerns

- **PRD claims tasks 2379–2383 are "in-flight"** in its header table, but `mcp__fused-memory__get_task` shows all six (2378–2383) at `status=done` with merge commits. The PRD is stale; the substantive mechanisms are wired.
- **Buy trait dimension type drift, deliberate.** PRD §5 cites `param unit_cost : Scalar<Money>` (echoing `docs/reify-stdlib-reference.md` §9); actual stdlib `io.ri` uses bare `param unit_cost : Money`. The `io.ri` header comment explicitly documents this as "deviation 1" but the PRD has not been updated to match. The compile pipeline accepts `Money` as a `Scalar<Money>` alias via `resolve_dimension_type`, so this is cosmetic, but the divergence between PRD prose and stdlib reality is a maintenance hazard.
- **Buy trait default drift.** PRD §5 quotes `param lead_time : Time = undef` from `docs/reify-stdlib-reference.md` §9, but stdlib `io.ri:77` declares `param lead_time : Time` (no default). Tests in io_traits_tests.rs only check the type, not the default, so the drift went unnoticed.
- **`sum(... for ... in ...)` generator-comprehension syntax in PRD §5 is fiction at the syntax level** — Reify has no generator/comprehension form. The example file works because it uses `[a, b].sum` (list-literal then `.sum`), a syntactically different idiom. Worked example in the PRD is misleading.
- **Example filename uses underscore, PRD uses hyphen.** PRD §5 says `examples/cost-aggregation.ri`; actual file is `examples/cost_aggregation.ri`. Tests reference the underscore form. Trivial DRIFT.

## Mechanisms

### M-001: `DimensionVector` slot-9 Money basis

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/dimension.rs:115-129`, basis constants; tests at lines 962-976 (`money_constant_populates_slot_9`), 786-794 (`money_does_not_leak_into_unrelated_arithmetic`).
- **Blocks:** none
- **Note:** 10-slot vector with `MONEY` at index 9 fully populated and pinned by `NAMED_DIMENSIONS`-driven canonical-name round trip.

### M-002: `DimensionVector::mul` / `div` / `pow` / `root` propagate slot 9

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dimension.rs:255-296`; tests `money_mul_with_mass_keeps_both_slots` (744), `money_div_by_mass_produces_cost_per_mass` (751), `money_pow_2_doubles_slot_9` (767), `money_root_2_halves_slot_9` (772), `money_div_by_money_is_dimensionless` (777).
- **Blocks:** none
- **Note:** All four arithmetic operations operate uniformly on the 10-slot array; no Money-specific code path needed.

### M-003: `DimensionVector::Display` renders `"USD"` and composites

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dimension.rs:397-426` (`names[9]="USD"`); tests `money_display_outputs_usd` (717), `money_per_mass_display_renders_compositely` (727 — pins `"USD·kg^-1"` with U+00B7), `money_dimensionless_after_self_cancel_displays_dimensionless` (736), `money_display_with_squared_exponent` (722 — pins `"USD^2"`).
- **Blocks:** none
- **Note:** Positive-exponent-first ordering produces `"USD·kg^-1"` rather than `"kg^-1·USD"`.

### M-004: `DimensionVector::to_display_units` Money branch

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dimension.rs:302-318` (Money branch `(si_value, "USD")`); test `to_display_units_recognises_money` (904) and the negative-coverage probe `to_display_units_keeps_si_fallback_for_unknown_composed_dim` (914) that guards against bare `"USD"` leaking onto composite Money/Length dimensions.
- **Blocks:** none
- **Note:** Branch returns SI value unchanged (factor 1.0) — by-design for a base monetary unit.

### M-005: `DimensionVector::canonical_name` returns `"Money"` via `NAMED_DIMENSIONS` table

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dimension.rs:244-249` (linear scan); `NAMED_DIMENSIONS` entry at 367; tests `canonical_name_money_returns_money` (835), `canonical_name_covers_all_named_singletons` (877). The table is shared with `resolve_dimension_type` in `crates/reify-compiler/src/type_resolution.rs`, so name↔dim is single-source.
- **Blocks:** none
- **Note:** Used by `format_dimension_mismatch_diagnostic` to produce the "Money and Force are different dimensions" hint.

### M-006: `content_hash` covers slot 9

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dimension.rs:320-331` (40-byte buffer = 10×4); tests `content_hash_buffer_covers_slot_9` (892), `money_content_hash_is_deterministic` (797), `money_content_hash_differs_from_other_base_dimensions` (805).
- **Blocks:** none
- **Note:** Hash buffer width is correctly sized to 10 slots, not legacy-9, and slot 9 actually feeds the digest.

### M-007: `unit USD : Money` stdlib declaration

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/stdlib/units.ri:74` (`pub unit USD : Money`); task 2378 `done_provenance.commit=94b2fac20`; tests `stdlib_units_module_contains_USD_with_money_dimension`, `stdlib_USD_is_publicly_visible_in_prelude` in `crates/reify-compiler/tests/money_units_tests.rs`.
- **Blocks:** none
- **Note:** No `= scale` body (factor=1.0 by default for base unit). Loaded in prelude bootstrap so all user files have USD in scope without import.

### M-008: `25USD` quantity-literal compile-time resolution to `Value::Scalar { si=25.0, MONEY }`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/expr.rs:553-580` (`QuantityLiteral` arm); `crates/reify-compiler/tests/money_units_tests.rs::stdlib_USD_quantity_literal_resolves_to_money_scalar`; runtime is a no-op because the compiler embeds the resolved Scalar directly as a `Literal`.
- **Blocks:** none
- **Note:** Registry lookup happens at compile time via `scope.lookup_unit_in_registry`; non-finite SI values are filtered with a dedicated overflow diagnostic.

### M-009: User-declared `unit GBP : Money = 1.25USD` compile-time-const factor

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/type_resolution.rs:417-498` (`compile_unit` + `evaluate_const_expr`); tests `user_defined_gbp_registers_with_money_dim_and_factor_125`, `gbp_quantity_literal_resolves_via_user_factor`, `cross_currency_addition_compiles_with_money_dim` in `crates/reify-compiler/tests/money_arithmetic_tests.rs`. PRD §2's "non-constant initialiser is a static error" is enforced via `evaluate_const_expr` returning `None` and emitting a diagnostic on any non-const expression.
- **Blocks:** none
- **Note:** Defense-in-depth rejection of factor=0 and non-finite factor at lines 436-451.

### M-010: Compile-time dimension-arithmetic invariants (`25USD/1kg → MONEY/MASS`, `(25USD/1kg)*2kg → MONEY`, `25USD/25USD → dimensionless`, `25USD*25USD → MONEY^2`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/tests/money_arithmetic_tests.rs` tests 1-5 (lines 31-265); `compound_money_per_mass_via_inline_user_unit_decl`, `money_per_mass_times_mass_cancels_to_money_at_compile_time`. Uses BinOp result_type propagation in `crates/reify-compiler/src/expr.rs` (Mul/Div paths).
- **Blocks:** none
- **Note:** Eval-side mirror at `crates/reify-eval/tests/money_arithmetic_eval.rs::money_per_mass_times_mass_evaluates_to_50_usd` pins runtime SI propagation. Acceptance sweep table-row `25USD * 25USD → Money^2` is pinned at the type level by `money_pow_2_doubles_slot_9` but does NOT have a dedicated compile-or-eval test for the explicit binop form — likely an acceptance test gap that the sweep does not catch.

### M-011: `format_dimension_mismatch_diagnostic` → `DiagnosticCode::DimensionMismatch` with named-dimension secondary label

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/type_compat.rs:320-355` formatter; `crates/reify-types/src/diagnostics.rs:327` enum variant; in-tree call sites at `expr.rs:742` (binop) and `expr.rs:845` (range); tests in `crates/reify-compiler/tests/money_force_diagnostic_tests.rs` cover `25USD + 5N`, `25USD - 5N`, `5N + 25USD`, and `25USD..5N`. Anti-cascade contract (Type::Error suppression) handled by the producer-poison pattern at the BinOp site (line 730 area).
- **Blocks:** none
- **Note:** PRD §4 says raw exponent vectors `[0,…,1]` must not appear; the formatter delegates to `Type`'s `Display` which uses `DimensionVector::Display`, so raw arrays are structurally excluded.

### M-012: Source-form unit display in LSP hovers, diagnostics, engine errors, and `format_value`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-lsp/src/hover.rs:262-282` test `hover_on_money_param_shows_usd_in_type_string` (asserts `"Scalar[USD]"`, NOT `"Rational"` or `"DimensionVector("`); `crates/reify-lsp/src/analysis.rs:1271-1278` `format_value_scalar_money_renders_usd`; `crates/reify-types/src/value.rs:2007-2028` `Display for Value::Scalar` delegates to `DimensionVector::Display`; engine error formatter chain in `crates/reify-eval/src/engine_admin.rs:39-41` references `EngineError::DimensionMismatch` using Display.
- **Blocks:** none
- **Note:** Task 2382 wired Display through all the documented surfaces. PRD lists `crates/reify-lsp/src/diagnostics.rs` as a target, but grep shows that file does not reference Money/USD/DimensionMismatch directly — it presumably routes the compiler-emitted `Diagnostic` (already source-formatted) without re-formatting. No regression risk visible.

### M-013: `Buy` and `Costed` traits in `std.io`; `Costed` provides `let line_cost : Money = unit_cost * quantity_produced`

- **State:** DRIFT
- **Failure mode:** Doc-vs-impl skew (PRD/spec say `Scalar<Money>`+`lead_time = undef`; code says bare `Money`+no default).
- **Evidence:** `crates/reify-compiler/stdlib/io.ri:73-78` (`Buy`), `108-111` (`Costed`); `docs/reify-stdlib-reference.md:980-985` (PRD-quoted form with `Scalar<Money>` + `= undef`); io.ri header comment "deviation 1" acknowledges the Money-vs-Scalar<Money> bare-form choice; PRD §5 echoes the stale spec form. Tests `cost_aggregation_costed_trait_present_in_std_io_with_required_quantity_produced` and `cost_aggregation_costed_exposes_line_cost_let_default_with_money_dim` pin the implemented shape, not the PRD-specified shape.
- **Blocks:** none — system works; PRD documentation is stale
- **Note:** Bare `Money` resolves to `Scalar<MONEY>` via `resolve_dimension_type` (the dimension type IS the Scalar type at the value level), so the two forms are equivalent at compile. The `lead_time = undef` default is the real semantic skew; current `io.ri` has no default at all. Two doc-drift gaps in one trait declaration.

### M-014: `sum(... for ... in ...)` generator-comprehension idiom in PRD §5 worked example

- **State:** FICTION (in the worked example only)
- **Failure mode:** F1 — PRD assumes a syntax not implemented
- **Evidence:** `docs/prds/money-dimension.md:228` shows `sum(buy.unit_cost * buy.quantity for buy in buys)`. Search of `crates/reify-syntax/src/lib.rs` for `comprehension|GeneratorExpr|for.*in.*for|ExprFor` returns no comprehension/generator AST node. The actual canonical example uses `[self.bolts.line_cost, self.mounts.line_cost].sum` (list-literal then `.sum` method), wired at `crates/reify-compiler/src/expr.rs:1804` for `Type::List(inner)`.
- **Blocks:** none — the implemented `.sum` over `List<Scalar<Money>>` covers the use case
- **Note:** The PRD worked example as written would not parse. The actual `examples/cost_aggregation.ri` file works because it uses the supported list-literal idiom. This is a worked-example bug in the PRD, not a missing feature — but a reader following the PRD literally would hit a parse error.

## Cross-PRD breadcrumbs

- **`Buy` / `Costed` traits in `std.io`** are shared by any PRD that touches procurement or BOM aggregation; deviation #1 (bare `Money` vs `Scalar<Money>`) is a stdlib-wide convention and likely surfaces in any future "cost-anything" PRD.
- **`DimensionMismatch` diagnostic infrastructure** (M-011) is consumed by every dimension-checking PRD; PRDs `field-source-kinds.md` (cited at PRD §4 suppression) and any FEA PRD that adds new dimensions would lean on the same `format_dimension_mismatch_diagnostic` plumbing.
- **`.sum` collection aggregation** (M-014) sits alongside `count`/`keys`/`values` in `expr.rs:408`. PRDs that assume map/iterator-style aggregation (e.g., multi-load-case envelope worst-case, FEA result reduction) probably want richer aggregations — out of scope here but worth noting that the current set is fixed to four members.
- **PRD-vs-task status drift.** PRD header still labels 2379–2383 as "in-flight" though all six tasks landed (commits 94b2fac20, f46855b4ea, 26f6a3782a, 62fc8bdb6c, ec8655dd91, plus 2383 closed via `found_on_main`). Pattern likely affects other PRDs authored before their tasks completed.

## Notable observations

- The acceptance-sweep test file pair (`money_acceptance_sweep_tests.rs` + `money_acceptance_sweep_eval.rs`) is a model for what task-level acceptance harnesses look like in this codebase: a thin glue layer that re-runs the cross-dimension invariants (Angle/Torque distinction, slot-9 isolation) at both the compile and eval layers.
- The PRD's "regression-pin index" table is unusually rigorous — explicitly naming the test function for every pinned invariant. Every test name in that table actually exists in the named file (verified by grep). This makes the PRD self-auditing for drift in code, even if the doc itself drifts on task status.
- No FICTION-state mechanism was found at the implementation level. The PRD's only fictional bit is the worked-example syntax (M-014), and the cited test files are all real (a couple have slightly different paths than the PRD states, captured as M-013 drift).
- GR-001 (struct-constructor runtime evaluation) is NOT in scope for this PRD — the Money dimension uses unit-decl ctors (`USD`, `GBP`, `EUR`) and bare `Money` param types, not struct-constructor invocation. No transitive blocker.
