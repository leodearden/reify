//! Integration tests — exhaustive Kleene truth-table pins for the public API of
//! `reify_expr::kleene` (§9.2.3 of `docs/reify-language-spec.md`).
//!
//! **Purpose:** this file is a *separate test binary* (compiled by `cargo test
//! --test kleene_logic_tests`), which means it exercises the *published public
//! surface* of `reify_expr::kleene`.  It would catch a regression where an item
//! is accidentally un-`pub`-ified, or where the `pub mod kleene;` re-export in
//! `lib.rs` is removed — failures that the inline `#[cfg(test)] mod tests`
//! block inside `kleene.rs` would *not* catch (those run against the private
//! internals).
//!
//! **Relationship to inline tests:** `crates/reify-expr/src/kleene.rs` already
//! contains `kleene_and_truth_table`, `kleene_or_truth_table`, and
//! `kleene_not_truth_table` as unit tests.  The duplication here is intentional
//! — see Task 2314 design decision "Keep duplication with the inline tests".
//!
//! **Implies:** `kleene_implies` was deliberately removed as YAGNI in Task 2294
//! (commit 31fc333c5).  We test it here via the de-Morgan rewrite
//! `a implies b ≡ ¬a ∨ b` (see the private `implies` helper below and the
//! design decision in the plan).
//!
//! **Spec citation:** §9.2.3 of `docs/reify-language-spec.md` (lines 1662-1680
//! at time of writing; use the section anchor for link stability).

use reify_expr::kleene::{KBool, kleene_and, kleene_not, kleene_or};

// ---------------------------------------------------------------------------
// Private file-local helper for implies via de-Morgan rewrite
// ---------------------------------------------------------------------------

/// `a implies b` expressed as the de-Morgan rewrite `¬a ∨ b`.
///
/// No `kleene_implies` exists in the helper (deliberately YAGNI per Task 2294,
/// commit 31fc333c5: "No BinOp::Implies exists in the grammar; the function and
/// its truth-table coverage will be reintroduced together with the operator in a
/// future task").  This composes only `kleene_or` / `kleene_not` from that
/// helper and matches the production rewrite used in
/// `crates/reify-eval/tests/kleene_e2e.rs`.
fn implies(a: KBool, b: KBool) -> KBool {
    kleene_or(kleene_not(a), b)
}

// ---------------------------------------------------------------------------
// §9.2.3 truth tables — AND
// ---------------------------------------------------------------------------

#[test]
fn kleene_and_truth_table_spec_9_2_3() {
    use KBool::*;
    // T ∧ T = T
    assert_eq!(kleene_and(True, True), True);
    // T ∧ F = F
    assert_eq!(kleene_and(True, False), False);
    // T ∧ U = U
    assert_eq!(kleene_and(True, Undef), Undef);
    // F ∧ T = F
    assert_eq!(kleene_and(False, True), False);
    // F ∧ F = F
    assert_eq!(kleene_and(False, False), False);
    // F ∧ U = F  (absorbing element)
    assert_eq!(kleene_and(False, Undef), False);
    // U ∧ T = U
    assert_eq!(kleene_and(Undef, True), Undef);
    // U ∧ F = F  (absorbing element)
    assert_eq!(kleene_and(Undef, False), False);
    // U ∧ U = U
    assert_eq!(kleene_and(Undef, Undef), Undef);
}

// ---------------------------------------------------------------------------
// §9.2.3 truth tables — OR
// ---------------------------------------------------------------------------

#[test]
fn kleene_or_truth_table_spec_9_2_3() {
    use KBool::*;
    // T ∨ T = T
    assert_eq!(kleene_or(True, True), True);
    // T ∨ F = T
    assert_eq!(kleene_or(True, False), True);
    // T ∨ U = T  (absorbing element)
    assert_eq!(kleene_or(True, Undef), True);
    // F ∨ T = T
    assert_eq!(kleene_or(False, True), True);
    // F ∨ F = F
    assert_eq!(kleene_or(False, False), False);
    // F ∨ U = U
    assert_eq!(kleene_or(False, Undef), Undef);
    // U ∨ T = T  (absorbing element)
    assert_eq!(kleene_or(Undef, True), True);
    // U ∨ F = U
    assert_eq!(kleene_or(Undef, False), Undef);
    // U ∨ U = U
    assert_eq!(kleene_or(Undef, Undef), Undef);
}

// ---------------------------------------------------------------------------
// §9.2.3 truth tables — NOT
// ---------------------------------------------------------------------------

#[test]
fn kleene_not_truth_table_spec_9_2_3() {
    use KBool::*;
    // ¬T = F
    assert_eq!(kleene_not(True), False);
    // ¬F = T
    assert_eq!(kleene_not(False), True);
    // ¬U = U
    assert_eq!(kleene_not(Undef), Undef);
}

// ---------------------------------------------------------------------------
// Commutativity — AND over the full Kleene domain
// ---------------------------------------------------------------------------

#[test]
fn kleene_and_commutative_over_full_kleene_domain() {
    use KBool::*;
    for a in [True, False, Undef] {
        for b in [True, False, Undef] {
            assert_eq!(
                kleene_and(a, b),
                kleene_and(b, a),
                "and({a:?}, {b:?}) != and({b:?}, {a:?})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Commutativity — OR over the full Kleene domain
// ---------------------------------------------------------------------------

#[test]
fn kleene_or_commutative_over_full_kleene_domain() {
    use KBool::*;
    for a in [True, False, Undef] {
        for b in [True, False, Undef] {
            assert_eq!(
                kleene_or(a, b),
                kleene_or(b, a),
                "or({a:?}, {b:?}) != or({b:?}, {a:?})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// §9.2.3 truth tables — implies (via de-Morgan rewrite ¬a ∨ b)
// ---------------------------------------------------------------------------

#[test]
fn implies_truth_table_via_de_morgan_spec_9_2_3() {
    use KBool::*;
    // T → T = T
    assert_eq!(implies(True, True), True);
    // T → F = F
    assert_eq!(implies(True, False), False);
    // T → U = U
    assert_eq!(implies(True, Undef), Undef);
    // F → T = T  (vacuously true — premise is false)
    assert_eq!(implies(False, True), True);
    // F → F = T  (vacuously true — premise is false)
    assert_eq!(implies(False, False), True);
    // F → U = T  (vacuously true — premise is false)
    assert_eq!(implies(False, Undef), True);
    // U → T = T  (vacuously true — consequent is true)
    assert_eq!(implies(Undef, True), True);
    // U → F = U  (unknown premise, false consequent)
    assert_eq!(implies(Undef, False), Undef);
    // U → U = U
    assert_eq!(implies(Undef, Undef), Undef);
}

// ---------------------------------------------------------------------------
// Asymmetric implies rows (highest-regression-risk, pinned individually)
// ---------------------------------------------------------------------------

#[test]
fn implies_asymmetric_pin_rows_spec_9_2_3() {
    use KBool::*;
    // false → undef = true  (vacuously true: premise is false, so implication holds regardless)
    assert_eq!(implies(False, Undef), True);
    // undef → false = undef  (consequent is false but premise is unknown: result is indeterminate)
    assert_eq!(implies(Undef, False), Undef);
    // undef → true = true   (consequent is true: implication holds regardless of premise)
    assert_eq!(implies(Undef, True), True);
}
