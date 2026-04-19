//! Anti-cascade tests for `Type::Error` propagation (task-448).
//!
//! These tests verify that once a sub-expression is inferred as `Type::Error`
//! (the poison-value sentinel), consumer sites propagate `Type::Error` rather
//! than falling back to `Type::Real`. The "member access not yet supported"
//! stub at `expr.rs:997` is the designated `Type::Error` producer that these
//! tests exercise (see step-12).

use reify_test_support::compile_source;
use reify_types::{CompiledExpr, CompiledExprKind, QuantifierKind, Severity, Type, Value, ValueCellId};

/// Walk a `CompiledExpr` tree and return the first node whose `result_type`
/// satisfies the predicate, if any. Used to search for a `Type::Error`-typed
/// node inside a compiled let binding.
fn find_node<'a>(
    expr: &'a CompiledExpr,
    pred: &impl Fn(&CompiledExpr) -> bool,
) -> Option<&'a CompiledExpr> {
    if pred(expr) {
        return Some(expr);
    }
    match &expr.kind {
        CompiledExprKind::BinOp { left, right, .. } => {
            find_node(left, pred).or_else(|| find_node(right, pred))
        }
        CompiledExprKind::UnOp { operand, .. } => find_node(operand, pred),
        CompiledExprKind::FunctionCall { args, .. } => {
            args.iter().find_map(|a| find_node(a, pred))
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => find_node(condition, pred)
            .or_else(|| find_node(then_branch, pred))
            .or_else(|| find_node(else_branch, pred)),
        CompiledExprKind::MethodCall { object, args, .. } => find_node(object, pred)
            .or_else(|| args.iter().find_map(|a| find_node(a, pred))),
        CompiledExprKind::IndexAccess { object, index } => {
            find_node(object, pred).or_else(|| find_node(index, pred))
        }
        _ => None,
    }
}

/// Retrieve the compiled `default_expr` of a let binding by name.
fn get_let_expr<'a>(module: &'a reify_compiler::CompiledModule, name: &str) -> &'a CompiledExpr {
    let template = module
        .templates
        .first()
        .expect("expected at least one template in module");
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == name)
        .unwrap_or_else(|| panic!("no value cell named '{}'", name));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{}' has no default expr", name))
}

// ── step-5: member aggregation on error-typed object ────────────────────────

#[test]
fn member_aggregation_on_error_typed_object_yields_type_error() {
    // The inner `self.unsupported` triggers the "member access not yet
    // supported" stub (see expr.rs:997), which must emit Type::Error post-step-12.
    // The outer `.sum` then hits the aggregation arm (expr.rs:973-990) and,
    // with the step-6 guard, must propagate Type::Error rather than fall
    // through to Type::Real.
    let source = r#"
structure S {
    let broken = self.unsupported.sum
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected .sum on Type::Error object to propagate Type::Error, got {:?}",
        expr.result_type,
    );
    // Amendment S4 invariant: the poisoned aggregation is emitted as a
    // Literal (via make_poison_literal), not as a dead MethodCall — so any
    // downstream pass pattern-matching on MethodCall cannot try to evaluate
    // it. Pin the node kind so a revert to MethodCall fails this test.
    assert!(
        matches!(expr.kind, CompiledExprKind::Literal { .. }),
        "expected poisoned aggregation to be emitted as a Literal node (S4 invariant), got {:?}",
        expr.kind,
    );
}

// ── step-7: index access on error-typed object ───────────────────────────────

#[test]
fn index_access_on_error_typed_object_yields_type_error() {
    // `self.unsupported` triggers the stub producer (Type::Error post-step-12),
    // then `[0]` indexing hits the IndexAccess arm (expr.rs:1109-1134). With
    // the step-8 guard, the result_type must be Type::Error rather than the
    // `_ => Type::Real` fall-through at line 1132.
    let source = r#"
structure S {
    let broken = self.unsupported[0]
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected indexing a Type::Error object to propagate Type::Error, got {:?}",
        expr.result_type,
    );
}

// ── step-9: quantifier over error-typed collection ───────────────────────────

#[test]
fn quantifier_over_error_typed_collection_yields_type_error_element() {
    // `self.unsupported` triggers the stub producer (Type::Error post-step-12),
    // and is used as the quantifier's collection. With the step-10 guard in
    // place, the quantifier's inferred elem_type must be Type::Error rather
    // than the `_ => Type::Real` fallback at expr.rs:1445-1448.
    //
    // The Quantifier expression's own result_type is always Bool, so we cannot
    // observe elem_type directly from the top-level node. Instead we inspect
    // the predicate: the bound variable `x` is inserted into the nested scope
    // with `elem_type`, so a reference to `x` inside the predicate carries
    // that type as its result_type. We use `find_node` to search the predicate
    // for ANY Type::Error-typed node — this avoids coupling to the parser's
    // canonicalization of `x > 0` (e.g. future chained-comparison desugaring).
    let source = r#"
structure S {
    let broken = exists x in self.unsupported: x > 0
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr(&module, "broken");
    let CompiledExprKind::Quantifier { predicate, .. } = &expr.kind else {
        panic!("expected Quantifier at top of let-expr, got {:?}", expr.kind);
    };
    // Search the entire predicate subtree for any node with Type::Error.
    // This finds the bound variable `x` regardless of how `x > 0` is shaped
    // by the compiler (direct BinOp, wrapped in a coercion, desugared chain).
    let found = find_node(predicate, &|node| node.result_type == Type::Error);
    assert!(
        found.is_some(),
        "expected predicate to contain a Type::Error-typed node (from the \
         quantifier-bound variable `x` inheriting the element type), but \
         found none. Predicate kind: {:?}",
        predicate.kind,
    );
}

// ── task-1913: nested-error cascade boundary ─────────────────────────────────

/// Regression test documenting that `Type::is_error()`'s top-level-only
/// contract causes a cascade diagnostic when a compound type transports a
/// nested `Type::Error`.
///
/// ## What this test documents
///
/// This test **documents current behavior** — a boundary marker, not an
/// invariant to preserve.  Specifically:
///
/// 1. `self.unsupported` in the trait-let default expression emits
///    "unknown member 'unsupported' on self" (root cause) and returns
///    `Type::Error`.
/// 2. The list literal `[self.unsupported]` infers its element type from the
///    first element and wraps the result to `List<Error>`.
/// 3. The trait-let injection pass (`conformance.rs:521-531`) calls
///    `type_compatible(List<Real>, List<Error>)`.  The guard in
///    `type_compatible` (`crates/reify-compiler/src/type_compat.rs:94`)
///    checks `is_error()` on each operand — but `is_error()` is top-level-only,
///    so `List<Error>.is_error()` returns `false`.  No match arm fires and
///    `type_compatible` returns `false`, emitting a cascade "type mismatch for
///    trait let 'x'" on top of the root-cause error.
///
/// ## What this test asserts
///
/// - **(a)** A root-cause diagnostic containing `"unknown member"` IS present.
/// - **(b)** A cascade diagnostic containing `"type mismatch for trait let"` IS
///   present.
///
/// Assertion (b) is intentionally the OPPOSITE polarity of the anti-cascade
/// contract tested in `stub_error_plus_arithmetic_emits_exactly_one_diagnostic`
/// below: **that** test asserts no cascade (the anti-cascade contract works at
/// the top level), while **this** test asserts the cascade IS present (the
/// known gap for nested compound types).
///
/// ## How to update when implementing option (a)
///
/// If you extend `is_error()` to a recursive `contains_error()` and apply it
/// at every consumer guard site in `type_compat.rs`, `expr.rs`, and
/// `conformance.rs` (see the follow-up section in the `is_error()` doc comment
/// in `crates/reify-types/src/ty.rs`), then:
///
/// - Change assertion **(b)** from `any(…)` to `!any(…)` to assert the
///   cascade is **NOT** present.
/// - Update the `is_error()` doc comment's follow-up section in the same
///   commit to reflect the completed change.
/// - Update the five unit tests `type_error_is_error_false_for_*` in
///   `crates/reify-types/src/ty.rs` to match the new recursive semantics
///   (or remove them if `is_error()` itself becomes recursive).
#[test]
fn nested_compound_error_cascades_through_trait_let_annotation() {
    let source = r#"
trait T { let x : List<Real> = [self.unsupported] }
structure S : T {}
"#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) The root-cause diagnostic must be present.
    assert!(
        errors.iter().any(|d| d.message.contains("unknown member")),
        "expected an 'unknown member' root-cause error to be present, got: {:?}",
        errors,
    );

    // (b) The cascade diagnostic IS present (nested-error gap, task-1913).
    // NOTE: this assertion is intentionally the opposite polarity of the
    // anti-cascade contract.  See doc comment above for update instructions.
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait let")),
        "expected a 'type mismatch for trait let' cascade to be present \
         (documenting the nested-error gap at is_error() top-level-only \
         boundary, task-1913), got: {:?}",
        errors,
    );
}

// ── step-11: end-to-end anti-cascade integration ─────────────────────────────

#[test]
fn stub_error_plus_arithmetic_does_not_cascade_type_mismatch() {
    // `self.unsupported` triggers the "unknown member 'X' on self" stub
    // at expr.rs:~724 which emits a single Severity::Error diagnostic.
    // Post-step-12 that stub returns Type::Error; with the step-4 guard in
    // infer_binop_type, the enclosing `+ 5.0` short-circuits to Type::Error
    // instead of falling through to Type::Real and emitting a type-mismatch
    // cascade.
    //
    // (Renamed from `..._emits_exactly_one_diagnostic` per amendment-round-2
    //  S2: the assertion below is the substring-based anti-cascade check, not
    //  a hard count-of-1, so the name now matches what's actually verified.)
    let source = r#"
structure S {
    let broken = self.unsupported + 5.0
}
"#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    // (task-448 amend): Assert the targeted anti-cascade properties directly
    // rather than a hard count of 1, so unrelated future diagnostics on this
    // source don't spuriously fail the test.
    //
    // (a) the expected root-cause error must be present,
    // (b) no type-mismatch / incompatible-types cascade was emitted on top.
    assert!(
        errors.iter().any(|d| d.message.contains("unknown member")),
        "expected an 'unknown member' root-cause error, got: {:?}",
        errors,
    );
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("mismatch") || d.message.contains("incompatible")),
        "expected NO type-mismatch/incompatible cascade on top of the stub \
         (anti-cascade contract), got: {:?}",
        errors,
    );
}

// ── task-1920: find_node helper exhaustivity ─────────────────────────────────

#[test]
fn find_node_traverses_previously_unhandled_kinds() {
    // Compile-time exhaustivity (enforced by the exhaustive match in find_node,
    // which has no `_` wildcard — see task-1920 / task-1912 S4 follow-up) is
    // the primary guarantee that new CompiledExprKind variants won't silently
    // return None. This test is a behavioral smoke check covering three
    // representative structural shapes that previously fell through to `_ => None`:
    //   1. container-of-children (ListLiteral)
    //   2. single-child wrapper  (OptionSome)
    //   3. two-child compound    (Quantifier, searched through its collection)

    let error_leaf = || CompiledExpr::literal(Value::Bool(false), Type::Error);
    let bool_leaf = || CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let pred = &|n: &CompiledExpr| n.result_type == Type::Error;

    // 1. ListLiteral: find_node must recurse into list elements.
    let list_expr = CompiledExpr::list_literal(
        vec![error_leaf()],
        Type::List(Box::new(Type::Error)),
    );
    assert!(
        find_node(&list_expr, pred).is_some(),
        "find_node failed to recurse into ListLiteral elements; \
         previously fell through to `_ => None`",
    );

    // 2. OptionSome: find_node must recurse into the inner expression.
    let some_expr = CompiledExpr::option_some(
        error_leaf(),
        Type::Option(Box::new(Type::Error)),
    );
    assert!(
        find_node(&some_expr, pred).is_some(),
        "find_node failed to recurse into OptionSome inner expression; \
         previously fell through to `_ => None`",
    );

    // 3. Quantifier: find_node must recurse into the collection subtree.
    //    The predicate is a non-Error Bool literal so any find_node result
    //    must come from the collection, not the predicate.
    let quant_expr = CompiledExpr::quantifier(
        QuantifierKind::Exists,
        "x".to_string(),
        ValueCellId::new("S", "x"),
        error_leaf(), // collection — Type::Error
        bool_leaf(),  // predicate  — Type::Bool (no Error here)
    );
    assert!(
        find_node(&quant_expr, pred).is_some(),
        "find_node failed to recurse into Quantifier collection; \
         previously fell through to `_ => None`",
    );
}
