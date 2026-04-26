# Kleene Three-Valued Logic — Test Design Reference

**Task:** 2314  
**Spec reference:** `docs/reify-language-spec.md` §9.2.3 (lines 1662–1680 at
time of writing; use the section anchor for link stability since content above
§9.2.3 may shift).

---

## 1. Spec Reference

Reify logical operators follow **Kleene's strong three-valued logic**, where
`undef` acts as "unknown."  A logical operator absorbs `undef` only when the
result is determined regardless of the unknown operand's value.

Source: `docs/reify-language-spec.md` §9.2.3.

---

## 2. Truth Tables

The tables below are reproduced verbatim from §9.2.3.  Absorbing-element rows
and vacuously-true implies rows are annotated.

### AND

| `a`     | `b`     | `a and b`            |
|---------|---------|----------------------|
| `true`  | `true`  | `true`               |
| `true`  | `false` | `false`              |
| `true`  | `undef` | `undef`              |
| `false` | `true`  | `false`              |
| `false` | `false` | `false`              |
| `false` | `undef` | **`false`** ← absorbing (`false` is the absorbing element for AND) |
| `undef` | `true`  | `undef`              |
| `undef` | `false` | **`false`** ← absorbing |
| `undef` | `undef` | `undef`              |

### OR

| `a`     | `b`     | `a or b`             |
|---------|---------|----------------------|
| `true`  | `true`  | `true`               |
| `true`  | `false` | `true`               |
| `true`  | `undef` | **`true`** ← absorbing (`true` is the absorbing element for OR) |
| `false` | `true`  | `true`               |
| `false` | `false` | `false`              |
| `false` | `undef` | `undef`              |
| `undef` | `true`  | **`true`** ← absorbing |
| `undef` | `false` | `undef`              |
| `undef` | `undef` | `undef`              |

### NOT

| `a`     | `not a` |
|---------|---------|
| `true`  | `false` |
| `false` | `true`  |
| `undef` | `undef` |

### IMPLIES

| `a`     | `b`     | `a implies b`        |
|---------|---------|----------------------|
| `true`  | `true`  | `true`               |
| `true`  | `false` | `false`              |
| `true`  | `undef` | `undef`              |
| `false` | `true`  | **`true`** ← vacuously true (premise is false) |
| `false` | `false` | **`true`** ← vacuously true (premise is false) |
| `false` | `undef` | **`true`** ← vacuously true (premise is false) |
| `undef` | `true`  | **`true`** ← vacuously true (consequent is true) |
| `undef` | `false` | `undef`              |
| `undef` | `undef` | `undef`              |

---

## 3. Commutativity

`and` and `or` are **commutative** over the full Kleene domain — operand order
does not affect the result.  The spec states (§9.2.3):

> Logical operators are **commutative with respect to `undef`** — operand order
> does not affect propagation.  This is consistent with Reify's declarative
> semantics (not imperative short-circuit evaluation).

`implies` is **intentionally asymmetric** — it is *not* commutative.  The
asymmetric rows are the highest-regression-risk entries (see §4).

---

## 4. Asymmetric Implies Rows

These three rows are specifically called out because they represent the most
counter-intuitive results and are the most likely to be inadvertently broken:

| Expression           | Result  | Why                                                    |
|----------------------|---------|--------------------------------------------------------|
| `false implies undef`| `true`  | Vacuously true — the premise is false, so the implication holds regardless of what the consequent is. |
| `undef implies false`| `undef` | The consequent is false, but the premise is unknown — we cannot determine whether the implication holds. |
| `undef implies true` | `true`  | Vacuously true — the consequent is already true, so the implication holds regardless of the premise.  |

Note the asymmetry: `false implies undef = true` but `undef implies false =
undef`.  These are **not** equal, which confirms implies is not commutative.

---

## 5. Implementation Note — `kleene_implies`

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

This rewrite is used in two places:

1. **Integration tests** — `crates/reify-expr/tests/kleene_logic_tests.rs`
   defines a private `fn implies(a: KBool, b: KBool) -> KBool` using this
   rewrite to pin the §9.2.3 truth table.
2. **E2E tests** — `crates/reify-eval/tests/kleene_e2e.rs` lines 108–116
   (`kleene_e2e_implies_vacuous_true`) uses the same path.

When `BinOp::Implies` evaluation is wired, a `kleene_implies` function and
direct truth-table coverage should be reintroduced in `kleene.rs`.

---

## 6. Test Inventory

### Integration test binary — Task 2314

**File:** `crates/reify-expr/tests/kleene_logic_tests.rs`

Run: `cargo test -p reify-expr --test kleene_logic_tests`

| Test name | What it covers |
|-----------|----------------|
| `kleene_and_truth_table_spec_9_2_3` | All 9 AND rows from §9.2.3 |
| `kleene_or_truth_table_spec_9_2_3` | All 9 OR rows from §9.2.3 |
| `kleene_not_truth_table_spec_9_2_3` | All 3 NOT rows from §9.2.3 |
| `kleene_and_commutative_over_full_kleene_domain` | 9-pair cartesian product for AND commutativity |
| `kleene_or_commutative_over_full_kleene_domain` | 9-pair cartesian product for OR commutativity |
| `implies_truth_table_via_de_morgan_spec_9_2_3` | All 9 implies rows via `¬a ∨ b` |
| `implies_asymmetric_pin_rows_spec_9_2_3` | 3 highest-regression-risk asymmetric rows (§4 above) |

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

The integration and unit test surfaces are intentionally distinct: the
integration tests exercise the *public* API from a separate binary (catching
accidental visibility regressions), while the unit tests exercise the private
internals and conversion impls.
