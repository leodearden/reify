//! Integration tests for the multi-objective `ObjectiveSet` container (PRD §6.1).
//!
//! Step 1 (RED): asserts structural shape, defaults, and constructor behaviour.
//! Steps 3/4 add the I2 round-trip test and wire `ResolutionProblem`.

use reify_ir::{ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm};

// ── helper ────────────────────────────────────────────────────────────────────

/// Mirror of the in-module `make_literal_expr()` helper in `constraint.rs`.
/// Integration tests cannot see `#[cfg(test)]` helpers inside the crate, so we
/// rebuild it here.  Same shape: `Value::Real(1.0)` literal, `Type::Real`,
/// `ContentHash::of(b"test")`.
fn make_literal_expr() -> reify_ir::CompiledExpr {
    use reify_ir::expr::CompiledExprKind;
    use reify_ir::value::Value;
    use reify_core::hash::ContentHash;
    use reify_core::ty::Type;
    reify_ir::CompiledExpr {
        kind: CompiledExprKind::Literal(Value::Real(1.0)),
        result_type: Type::Real,
        content_hash: ContentHash::of(b"test"),
    }
}

// ── (a) ObjectiveSense ────────────────────────────────────────────────────────

#[test]
fn objective_sense_variants_exist() {
    let _min = ObjectiveSense::Minimize;
    let _max = ObjectiveSense::Maximize;
}

#[test]
fn objective_sense_variants_are_distinct() {
    assert_ne!(ObjectiveSense::Minimize, ObjectiveSense::Maximize);
}

#[test]
fn objective_sense_is_copy_clone_eq_hash() {
    let s = ObjectiveSense::Minimize;
    let s2 = s; // Copy
    assert_eq!(s, s2); // PartialEq + Eq

    let s3 = Clone::clone(&s); // Clone
    assert_eq!(s, s3);

    // Hash: usable as HashMap key
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(ObjectiveSense::Minimize, "min");
    map.insert(ObjectiveSense::Maximize, "max");
    assert_eq!(map.get(&ObjectiveSense::Minimize), Some(&"min"));
    assert_eq!(map.get(&ObjectiveSense::Maximize), Some(&"max"));
}

#[test]
fn objective_sense_debug() {
    assert!(format!("{:?}", ObjectiveSense::Minimize).contains("Minimize"));
    assert!(format!("{:?}", ObjectiveSense::Maximize).contains("Maximize"));
}

// ── (b) ObjectiveCombination ─────────────────────────────────────────────────

#[test]
fn objective_combination_variants_exist() {
    let _ws = ObjectiveCombination::WeightedSum;
    let _lex = ObjectiveCombination::Lexicographic;
}

#[test]
fn objective_combination_variants_are_distinct() {
    assert_ne!(
        ObjectiveCombination::WeightedSum,
        ObjectiveCombination::Lexicographic
    );
}

#[test]
fn objective_combination_is_copy_clone_eq_hash() {
    let c = ObjectiveCombination::WeightedSum;
    let c2 = c; // Copy
    assert_eq!(c, c2);

    let c3 = Clone::clone(&c);
    assert_eq!(c, c3);

    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(ObjectiveCombination::WeightedSum, "ws");
    map.insert(ObjectiveCombination::Lexicographic, "lex");
    assert_eq!(map.get(&ObjectiveCombination::WeightedSum), Some(&"ws"));
}

#[test]
fn objective_combination_debug() {
    assert!(format!("{:?}", ObjectiveCombination::WeightedSum).contains("WeightedSum"));
    assert!(format!("{:?}", ObjectiveCombination::Lexicographic).contains("Lexicographic"));
}

// ── (c) ObjectiveTerm::new — both senses ─────────────────────────────────────

#[test]
fn objective_term_new_minimize_defaults() {
    let expr = make_literal_expr();
    let saved_hash = expr.content_hash.clone();
    let term = ObjectiveTerm::new(ObjectiveSense::Minimize, expr);
    assert_eq!(term.sense, ObjectiveSense::Minimize);
    assert_eq!(term.weight, 1.0);
    assert_eq!(term.priority, 0);
    assert_eq!(term.expr.content_hash, saved_hash);
}

#[test]
fn objective_term_new_maximize_defaults() {
    let expr = make_literal_expr();
    let saved_hash = expr.content_hash.clone();
    let term = ObjectiveTerm::new(ObjectiveSense::Maximize, expr);
    assert_eq!(term.sense, ObjectiveSense::Maximize);
    assert_eq!(term.weight, 1.0);
    assert_eq!(term.priority, 0);
    assert_eq!(term.expr.content_hash, saved_hash);
}

// ── (d/e) ObjectiveSet::single — both senses ─────────────────────────────────

#[test]
fn objective_set_single_minimize() {
    let expr = make_literal_expr();
    let saved_hash = expr.content_hash.clone();
    let set = ObjectiveSet::single(ObjectiveSense::Minimize, expr);
    assert_eq!(set.terms.len(), 1);
    assert_eq!(set.terms[0].sense, ObjectiveSense::Minimize);
    assert_eq!(set.terms[0].weight, 1.0);
    assert_eq!(set.terms[0].priority, 0);
    assert_eq!(set.terms[0].expr.content_hash, saved_hash);
    assert_eq!(set.combination, ObjectiveCombination::WeightedSum);
}

#[test]
fn objective_set_single_maximize() {
    let expr = make_literal_expr();
    let saved_hash = expr.content_hash.clone();
    let set = ObjectiveSet::single(ObjectiveSense::Maximize, expr);
    assert_eq!(set.terms.len(), 1);
    assert_eq!(set.terms[0].sense, ObjectiveSense::Maximize);
    assert_eq!(set.terms[0].weight, 1.0);
    assert_eq!(set.terms[0].priority, 0);
    assert_eq!(set.terms[0].expr.content_hash, saved_hash);
    assert_eq!(set.combination, ObjectiveCombination::WeightedSum);
}

// ── (f) Debug smoke check ─────────────────────────────────────────────────────

#[test]
fn objective_set_debug_contains_type_name_and_combination() {
    let expr = make_literal_expr();
    let set = ObjectiveSet::single(ObjectiveSense::Minimize, expr);
    let debug = format!("{:?}", set);
    assert!(debug.contains("ObjectiveSet"), "Debug should contain 'ObjectiveSet'; got: {debug}");
    assert!(debug.contains("WeightedSum"), "Debug should contain 'WeightedSum'; got: {debug}");
}

// ── (g) Clone ────────────────────────────────────────────────────────────────

#[test]
fn objective_set_clone_produces_structurally_equal_set() {
    let expr = make_literal_expr();
    let set = ObjectiveSet::single(ObjectiveSense::Minimize, expr);
    let set2 = set.clone();
    let d1 = format!("{:?}", set);
    let d2 = format!("{:?}", set2);
    assert_eq!(d1, d2, "Clone should produce a structurally equal ObjectiveSet");
}
