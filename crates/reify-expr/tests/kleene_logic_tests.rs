//! Integration tests — Kleene logic commutativity pins for the public API of
//! `reify_expr::kleene` (§9.2.3 of `docs/reify-language-spec.md`).
//!
//! **Purpose:** this file is a *separate test binary* (compiled by `cargo test
//! --test kleene_logic_tests`), which means it exercises the *published public
//! surface* of `reify_expr::kleene`.
//!
//! **What is tested here:** commutativity of `kleene_and` and `kleene_or` over
//! the full Kleene domain.  The exhaustive AND/OR/NOT truth-table pins live in
//! the unit-test block of `kleene.rs` (`cargo test -p reify-expr --lib`).
//! Sibling crates (`reify-eval`) already import `kleene_and`, `kleene_or`,
//! `kleene_not`, and `KBool` from outside the crate, so any visibility
//! regression (accidental un-`pub`-ifying, removed `pub mod kleene;` re-export)
//! would fail their compile immediately.  Commutativity is the additional
//! property that is *not* covered by the inline unit tests.
//!
//! **Implies:** `kleene_implies` was deliberately removed as YAGNI in Task 2294
//! (commit 31fc333c5).  No implies tests are included here; the de-Morgan
//! rewrite path is already exercised by the actual evaluator in
//! `crates/reify-eval/tests/kleene_e2e.rs`.  See `docs/prds/kleene-logic.md`
//! §2 for the rationale.
//!
//! **Spec citation:** §9.2.3 of `docs/reify-language-spec.md`.

use reify_expr::kleene::{KBool, kleene_and, kleene_or};

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
