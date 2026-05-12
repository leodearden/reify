# Audit: Kleene Three-Valued Logic — Test Design Reference

**PRD path:** `docs/prds/kleene-logic.md`
**Auditor:** audit-kleene-logic
**Date:** 2026-05-12
**Mechanism count:** 9
**Gap count:** 2

## Top concerns

- **The PRD is a retrospective test-design reference, not a forward-looking PRD.** It documents what is already in place (Kleene helpers, integration tests) plus one deferral (`kleene_implies` + `BinOp::Implies`). Most "mechanisms" are WIRED — the audit's load-bearing finding is on the deliberate gap, not architectural drift.
- **`implies` is a spec-defined operator with no parser, AST, or evaluator backing.** §15 EBNF, §16 precedence table, and §9.2.3 truth table all reference `implies`, but `BinOp` enum (`crates/reify-types/src/expr.rs:174-189`) has no `Implies` variant and the tree-sitter grammar (`tree-sitter-reify/grammar.js:722-735`) has no `implies` (or `and`/`or`/`not` keyword) production. The PRD acknowledges this as YAGNI — but the spec still advertises a syntax users cannot write today. This is a documented FICTION at the language-surface level (M-002), not a hidden one.
- **The de-Morgan rewrite that stands in for `implies` is a user-side workaround, not a compiler rewrite.** Today the only way to express implication is to write `!b || a` (or `not b or a`, if those keywords ever ship) by hand. The PRD treats this as adequate "until the evaluation path is complete," but there is no parser/compiler desugar that takes `a implies b` and lowers it to `!a || b` — the spec → impl gap requires authors to know the rewrite.
- **Unit-test block and integration test binary are intentionally duplicative for visibility-regression detection.** PRD §3 calls this out explicitly; integration binary catches `pub mod kleene` regressions at compile time via sibling-crate import. This is wired and load-bearing, but is the kind of test-shape decision Phase 3 might want to see surfaced.

## Mechanisms

### M-001: `KBool` enum + `kleene_and`/`kleene_or`/`kleene_not` helpers in shared module

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/kleene.rs:37-86` (enum + three pure functions matching §9.2.3 truth table); unit-test block `crates/reify-expr/src/kleene.rs:122-230` (9-row AND, 9-row OR, 3-row NOT); task 2294 (done) consolidated these into a single helper module
- **Blocks:** None
- **Note:** Single source of truth for §9.2.3 — kernel of the PRD. Implementation is straightforward 9-row match; tests pin every row.

### M-002: `implies` operator parsed and evaluated end-to-end

- **State:** FICTION
- **Failure mode:** F1 (compile-time / source-level contract → no parser, AST, or evaluator backing)
- **Evidence:** Spec §15 grammar (`docs/reify-language-spec.md:2467`) and §16 precedence table (`:2542`) declare `implies` as an operator. PRD §2 (lines 19-32) accurately reports that `crates/reify-types/src/expr.rs:174-189` `BinOp` enum has no `Implies` variant. Tree-sitter grammar `tree-sitter-reify/grammar.js:722-735` contains only `||`/`&&`/comparison/arithmetic — no `implies` or alphabetic `and`/`or`. Author guidance is to use de-Morgan rewrite by hand.
- **Blocks:** None tracked. No task filed for the eval/parser wiring; reintroduction noted only as a forward-looking comment in `kleene.rs` documentation (per PRD §2 last sentence — "When `BinOp::Implies` evaluation is wired...").
- **Note:** The PRD acknowledges this as a YAGNI deferral from task 2294; the spec, however, still presents `implies` as available syntax. Either the spec needs a v0.1 note (`implies` deferred) or a parser/AST/eval extension is needed. No task tracks the wiring.

### M-003: `kleene_implies` helper function

- **State:** TODO
- **Failure mode:** F1 (deferred helper; reintroduction documented in PRD §2)
- **Evidence:** `crates/reify-expr/src/kleene.rs` exposes only `kleene_and`, `kleene_or`, `kleene_not` (PRD §2 quote: "therefore exposes only..."). PRD §2 last paragraph: "When `BinOp::Implies` evaluation is wired, a `kleene_implies` function and direct truth-table coverage should be reintroduced in `kleene.rs`." Task 2294 commit `31fc333c5` removed it.
- **Blocks:** Direct truth-table coverage of the asymmetric `implies` rows (the rows that "often regress" per task 2294 details: `false implies undef = true`, `undef implies false = undef`)
- **Note:** Differs from M-002 only in scope — `kleene_implies` is the helper-side; M-002 is the operator-side. Both are gated on the same wiring decision. Reintroduction is documented but unowned (no task).

### M-004: Kleene AND/OR routed through shared helpers from the evaluator

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/lib.rs:1471-1473` (eval_binop dispatches `BinOp::And`/`BinOp::Or` to `eval_and`/`eval_or`); `:1516-1557` (`eval_and`/`eval_or` convert via `KBool::try_from`, short-circuit on absorbing element, fold via `kleene::kleene_and`/`kleene_or`); task 2294 (done) consolidated the routing
- **Blocks:** None
- **Note:** Short-circuits preserve laziness for absorbing elements (`False ∧ x = False`, `True ∨ x = True`) while still calling the shared kleene helper for non-absorbing branches.

### M-005: Kleene NOT routed through shared helper from the evaluator

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/lib.rs:2506-2510` (UnOp::Not converts via `KBool::try_from`, folds via `kleene::kleene_not`); test pin `:4321-4324` ensures non-bool returns `Value::Undef` ("type-error → Undef" contract)
- **Blocks:** None
- **Note:** Consistent with AND/OR pattern — TryFrom + kleene helper + From into Value.

### M-006: `KBool::try_from(&Value)` + `From<KBool> for Value` round-trip conversions

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/kleene.rs:88-120` (impls); unit tests `:196-228` (5 rows: Bool(true), Bool(false), Undef, Int err, Real err, plus 3-row round-trip)
- **Blocks:** None
- **Note:** The `Err(())` contract for non-bool/non-undef values is what lets the eval-side preserve its "type-error → `Value::Undef`" catch-all. Pinned at unit and integration level.

### M-007: Commutativity tests for `kleene_and` and `kleene_or` over full domain

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/tests/kleene_logic_tests.rs:31-43` (AND commutativity over 9 pairs); `:49-61` (OR commutativity over 9 pairs); PRD §3 documents both
- **Blocks:** None
- **Note:** Separate test binary specifically to catch visibility regressions of the public `pub mod kleene` exports — relies on a sibling crate (`reify-eval`) importing from outside the crate.

### M-008: End-to-end evaluator path for AND/OR/de-Morgan-rewritten-implies + `forall`-with-undef

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/tests/kleene_e2e.rs:60-122` (4 tests covering AND absorption, OR absorption, implies-via-de-Morgan vacuous-true, and forall-undef-propagation through `compile_with_stdlib` pipeline); fixture `examples/kleene_e2e.ri:1-19`; engine forall integration `crates/reify-expr/src/lib.rs:771-916` (forall/exists with `has_undef` tracking matching §9.2.6 table)
- **Blocks:** None
- **Note:** This is the "implies works in practice" pin — confirms the de-Morgan workaround flows through real compile+eval. Without the symbolic operator, this is currently the only `implies` integration coverage.

### M-009: `forall`/`exists` Kleene propagation (PRD-adjacent — §9.2.6 implied by §3 example)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/lib.rs:771-916` (`eval_quantifier` with `has_undef` tracking, false-short-circuit for forall, true-short-circuit for exists, vacuous-true on empty for forall and vacuous-false for exists); test `kleene_e2e.rs:113-122` (forall-over-mixed-undef returns `Undef`); spec §9.2.6 (`docs/reify-language-spec.md:1776-1789`)
- **Blocks:** None
- **Note:** PRD §3 does not directly enumerate forall as a tested mechanism, but its e2e fixture exercises it (`p4 = forall x in xs: x`). Included as a mechanism because the PRD's e2e test path explicitly depends on it. Spec §9.2.6 is cited by the e2e test header. Adjacent to PRD scope.

## Cross-PRD breadcrumbs

- **Spec §15 / §16 advertise `implies` as an operator with no parser/eval backing.** This is a language-spec-vs-implementation drift that may affect any PRD that mentions "static implication check" — notably spec §8.10 ("Guarded Declaration Reference Safety: A reference is valid only if the referencing declaration's guard implies the referenced entity's guard. Static implication check on boolean guard expressions"). The static implication check on guards exists (guard-compilation tests under `crates/reify-compiler/tests/guard_compilation.rs`), but it does NOT use a runtime `implies` operator — it uses structural implication on compiled guard expressions. Mentioning so Phase 3 can decide whether this is an `implies`-PRD concern or a guards-PRD concern.
- **`undef` propagation table (§9.2)** is the broader umbrella. Other Kleene-adjacent sections — §9.2.1 arithmetic, §9.2.2 comparison, §9.2.4 conditional, §9.2.5 match, §9.2.7 function application, §9.2.8 Option — are referenced by this PRD only obliquely. If Phase 3 wants a complete `undef`-propagation audit, this PRD covers §9.2.3 + §9.2.6 only.
- **Tree-sitter grammar uses `&&`/`||`/`!` while spec uses `and`/`or`/`not`.** Outside the kleene-logic PRD scope, but flagged: spec §15 grammar (`docs/reify-language-spec.md:2465-2470`) declares `and`/`or`/`not` as the keywords; tree-sitter grammar (`tree-sitter-reify/grammar.js:722-735, 747-750`) accepts only `&&`/`||`/`!`. This is independent drift from the `implies` gap (M-002), though they share a root cause (parser doesn't match spec's operator names).
- **No GR-001 (structure-constructor runtime eval) interaction.** This PRD touches only primitive `Bool`/`Undef` Values and the `BinOp`/`UnOp` evaluator path. Struct constructors do not appear in the PRD or its fixtures.
