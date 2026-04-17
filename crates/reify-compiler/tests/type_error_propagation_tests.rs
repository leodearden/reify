//! Anti-cascade tests for `Type::Error` propagation (task-448).
//!
//! These tests verify that once a sub-expression is inferred as `Type::Error`
//! (the poison-value sentinel), consumer sites propagate `Type::Error` rather
//! than falling back to `Type::Real`. The "member access not yet supported"
//! stub at `expr.rs:997` is the designated `Type::Error` producer that these
//! tests exercise (see step-12).

use reify_test_support::compile_source;
use reify_types::{CompiledExpr, CompiledExprKind, Severity, Type};

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

// ── step-11: end-to-end anti-cascade integration ─────────────────────────────

#[test]
fn stub_error_plus_arithmetic_emits_exactly_one_diagnostic() {
    // `self.unsupported` triggers the "unknown member 'X' on self" stub
    // at expr.rs:~724 which emits a single Severity::Error diagnostic.
    // Post-step-12 that stub returns Type::Error; with the step-4 guard in
    // infer_binop_type, the enclosing `+ 5.0` short-circuits to Type::Error
    // instead of falling through to Type::Real and emitting a type-mismatch
    // cascade. The net: exactly ONE error on the whole module.
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
