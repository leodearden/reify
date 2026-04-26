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

`BinOp::Implies` exists in the grammar (see `docs/reify-language-spec.md`
§9.2.3 and the operator precedence table).  However, as of Task 2294
(commit 31fc333c5), **no `kleene_implies` function exists** in
`crates/reify-expr/src/kleene.rs`.  Task 2294's reviewer removed the helper as
YAGNI:

> "No BinOp::Implies exists in the grammar; the function and its truth-table
> coverage will be reintroduced together with the operator in a future task."

*(Note: `BinOp::Implies` does appear in the grammar and spec, but the
evaluation path for it had not yet been wired up at the time of Task 2294.)*

Until the evaluation path is complete, `implies` is expressed via the
**de-Morgan rewrite**:

```
a implies b  ≡  ¬a ∨ b  ≡  kleene_or(kleene_not(a), b)
```

This rewrite is exercised by the actual evaluator path in
`crates/reify-eval/tests/kleene_e2e.rs` lines 108–116
(`kleene_e2e_implies_vacuous_true`).

When `BinOp::Implies` evaluation is wired, a `kleene_implies` function and
direct truth-table coverage should be reintroduced in `kleene.rs`.

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
| `try_from_value_bool_true` | `Value::Bool(true)` → `KBool::True` |
| `try_from_value_bool_false` | `Value::Bool(false)` → `KBool::False` |
| `try_from_value_undef` | `Value::Undef` → `KBool::Undef` |
| `try_from_value_non_bool_is_err` | Non-bool/undef values return `Err(())` |
| `from_kbool_into_value` | `KBool` → `Value` round-trip |

The integration tests cover *commutativity* (a property not covered by the unit
tests), while sibling crates (`reify-eval`) import the public API and catch any
visibility regression at compile time.
