# Kleene Three-Valued Logic — Test Design Reference

**Task:** 2314  
**Spec reference:** `docs/reify-language-spec.md` §9.2.3 (lines 1662–1680 at
time of writing; use the section anchor for link stability since content above
§9.2.3 may shift).

---

## 1. Spec Reference

Reify logical operators follow **Kleene's strong three-valued logic**, where
`undef` acts as "unknown."  The truth tables for `and`, `or`, `not`, and
`implies` are defined in `docs/reify-language-spec.md` §9.2.3 — refer there
for the authoritative tables.

---

## 2. Implementation Note — `kleene_implies`

The `implies` keyword is part of the Reify language grammar: it appears in the
EBNF production grammar (`docs/reify-language-spec.md` §15 Grammar Summary,
around line 2409) and in the operator-precedence table (§16 Appendix: Operator
Precedence Table, around line 2484 — level 15, right-associative).  Its truth table is in §9.2.3 (already cited in §1 above).

**As of task ζ (task 3921, merge `acfbfb5f61`), `BinOp::Implies` evaluation is
fully wired.**  `BinOp` now lives in `crates/reify-ir/src/expr.rs` (the
`reify-types` façade was retired in the atomic cutover, commit `3f3da9f03d`)
and includes the `Implies` variant.  `kleene_implies` was reintroduced in
`crates/reify-expr/src/kleene.rs` (feature commit `7fd180a4d3`) alongside
`kleene_and`, `kleene_or`, and `kleene_not`; the `kleene_implies` helper had
been deferred as YAGNI in Task 2294 (commit `31fc333c5`).

The closed form `kleene_implies` implements is the **de-Morgan equivalence**:

```
a implies b  ≡  ¬a ∨ b  ≡  kleene_or(kleene_not(a), b)
```

This identity was used as a temporary evaluator workaround before task ζ
(exercised by `kleene_e2e_implies_vacuous_true` in
`crates/reify-eval/tests/kleene_e2e.rs`); it is now formalised as
`kleene_implies`'s function body.

`BinOp::Implies` evaluation is dispatched via `eval_implies`
(`crates/reify-expr/src/lib.rs`) → `kleene::kleene_implies`.  The truth table
is pinned by `kleene_implies_truth_table` in `kleene.rs` (all 9 spec §9.2.3
rows).  The `implies` keyword path is covered end-to-end by the
`implies_e2e_*` tests in `crates/reify-eval/tests/implies_e2e.rs` (signal
rows, right-associativity, precedence).

---

## 3. Test Inventory

### Integration test binary — Task 2314

**File:** `crates/reify-expr/tests/kleene_logic_tests.rs`

Run: `cargo test -p reify-expr --test kleene_logic_tests`

| Test name | What it covers |
|-----------|----------------|
| `kleene_and_commutative_over_full_kleene_domain` | 9-pair cartesian product for AND commutativity |
| `kleene_or_commutative_over_full_kleene_domain` | 9-pair cartesian product for OR commutativity |

### Unit test block — existing (Task 2294)

**File:** `crates/reify-expr/src/kleene.rs` — `#[cfg(test)] mod tests`

Run: `cargo test -p reify-expr --lib`

| Test name | What it covers |
|-----------|----------------|
| `kleene_and_truth_table` | All 9 AND rows (private API surface) |
| `kleene_or_truth_table` | All 9 OR rows (private API surface) |
| `kleene_not_truth_table` | All 3 NOT rows (private API surface) |
| `kleene_implies_truth_table` | All 9 IMPLIES rows (private API surface) |
| `try_from_value_bool_true` | `Value::Bool(true)` → `KBool::True` |
| `try_from_value_bool_false` | `Value::Bool(false)` → `KBool::False` |
| `try_from_value_undef` | `Value::Undef` → `KBool::Undef` |
| `try_from_value_non_bool_is_err` | Non-bool/undef values return `Err(())` |
| `from_kbool_into_value` | `KBool` → `Value` round-trip |

The integration tests cover *commutativity* (a property not covered by the unit
tests), while sibling crates (`reify-eval`) import the public API and catch any
visibility regression at compile time.
