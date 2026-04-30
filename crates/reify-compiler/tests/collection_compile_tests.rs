//! Collection compilation tests (step-29 through step-36).

use reify_test_support::parse_and_compile;
use reify_types::{CompiledExprKind, Type, Value, ValueMap};

/// Helper: get the default_expr for a value cell by member name.
fn get_cell_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_types::CompiledExpr {
    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("should have '{}' value cell", member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{}' should have a default expr", member))
}

// ─── step-29: List, Set, Map, IndexAccess compilation ───

#[test]
fn compile_list_literal() {
    let compiled = parse_and_compile("structure S { let x = [1, 2, 3] }");
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::ListLiteral(elems) => {
            assert_eq!(elems.len(), 3);
            assert!(
                matches!(&elems[0].kind, CompiledExprKind::Literal(Value::Int(1))),
                "elem 0: {:?}",
                elems[0].kind
            );
            assert!(
                matches!(&elems[1].kind, CompiledExprKind::Literal(Value::Int(2))),
                "elem 1: {:?}",
                elems[1].kind
            );
            assert!(
                matches!(&elems[2].kind, CompiledExprKind::Literal(Value::Int(3))),
                "elem 2: {:?}",
                elems[2].kind
            );
        }
        other => panic!("expected ListLiteral, got {:?}", other),
    }
}

#[test]
fn compile_list_literal_empty() {
    let compiled = parse_and_compile("structure S { let x = [] }");
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::ListLiteral(elems) => {
            assert_eq!(elems.len(), 0);
        }
        other => panic!("expected ListLiteral, got {:?}", other),
    }
}

#[test]
fn compile_set_literal() {
    let compiled = parse_and_compile("structure S { let x = set{1, 2} }");
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::SetLiteral(elems) => {
            assert_eq!(elems.len(), 2);
            assert!(
                matches!(&elems[0].kind, CompiledExprKind::Literal(Value::Int(1))),
                "elem 0: {:?}",
                elems[0].kind
            );
            assert!(
                matches!(&elems[1].kind, CompiledExprKind::Literal(Value::Int(2))),
                "elem 1: {:?}",
                elems[1].kind
            );
        }
        other => panic!("expected SetLiteral, got {:?}", other),
    }
}

#[test]
fn compile_map_literal() {
    let compiled = parse_and_compile(r#"structure S { let x = map{"a" => 1} }"#);
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::MapLiteral(entries) => {
            assert_eq!(entries.len(), 1);
            assert!(
                matches!(
                    &entries[0].0.kind,
                    CompiledExprKind::Literal(Value::String(s)) if s == "a"
                ),
                "key: {:?}",
                entries[0].0.kind
            );
            assert!(
                matches!(&entries[0].1.kind, CompiledExprKind::Literal(Value::Int(1))),
                "val: {:?}",
                entries[0].1.kind
            );
        }
        other => panic!("expected MapLiteral, got {:?}", other),
    }
}

#[test]
fn compile_index_access() {
    let compiled = parse_and_compile("structure S { let items = [10, 20, 30]  let x = items[0] }");
    let expr = get_cell_expr(&compiled, "x");
    match &expr.kind {
        CompiledExprKind::IndexAccess { object, index } => {
            // object should be a ValueRef to items
            assert!(
                matches!(&object.kind, CompiledExprKind::ValueRef(id) if id.member == "items"),
                "object: {:?}",
                object.kind
            );
            // index should be literal Int(0)
            assert!(
                matches!(&index.kind, CompiledExprKind::Literal(Value::Int(0))),
                "index: {:?}",
                index.kind
            );
        }
        other => panic!("expected IndexAccess, got {:?}", other),
    }
}

// ─── step-31: MemberAccess -> MethodCall compilation ───

#[test]
fn compile_member_access_count() {
    // items.count should compile to MethodCall { method: "count", args: [] }
    let compiled = parse_and_compile("structure S { let items = [1, 2, 3]  let n = items.count }");
    let expr = get_cell_expr(&compiled, "n");
    match &expr.kind {
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            assert_eq!(method, "count");
            assert!(args.is_empty(), "count takes no args");
            assert!(
                matches!(&object.kind, CompiledExprKind::ValueRef(id) if id.member == "items"),
                "object: {:?}",
                object.kind
            );
        }
        other => panic!("expected MethodCall(count), got {:?}", other),
    }
}

#[test]
fn compile_member_access_sum() {
    let compiled = parse_and_compile("structure S { let items = [1, 2, 3]  let s = items.sum }");
    let expr = get_cell_expr(&compiled, "s");
    match &expr.kind {
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            assert_eq!(method, "sum");
            assert!(args.is_empty());
            assert!(
                matches!(&object.kind, CompiledExprKind::ValueRef(id) if id.member == "items"),
                "object: {:?}",
                object.kind
            );
        }
        other => panic!("expected MethodCall(sum), got {:?}", other),
    }
}

#[test]
fn compile_member_access_keys() {
    let compiled = parse_and_compile(r#"structure S { let m = map{"a" => 1}  let k = m.keys }"#);
    let expr = get_cell_expr(&compiled, "k");
    match &expr.kind {
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            assert_eq!(method, "keys");
            assert!(args.is_empty());
            assert!(
                matches!(&object.kind, CompiledExprKind::ValueRef(id) if id.member == "m"),
                "object: {:?}",
                object.kind
            );
        }
        other => panic!("expected MethodCall(keys), got {:?}", other),
    }
}

#[test]
fn compile_member_access_values() {
    let compiled = parse_and_compile(r#"structure S { let m = map{"a" => 1}  let v = m.values }"#);
    let expr = get_cell_expr(&compiled, "v");
    match &expr.kind {
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            assert_eq!(method, "values");
            assert!(args.is_empty());
            assert!(
                matches!(&object.kind, CompiledExprKind::ValueRef(id) if id.member == "m"),
                "object: {:?}",
                object.kind
            );
        }
        other => panic!("expected MethodCall(values), got {:?}", other),
    }
}

// ─── step-33: Integration tests (parse + compile + eval) for collection literals ───

#[test]
fn e2e_list_literal() {
    let compiled = parse_and_compile("structure S { let x = [1, 2, 3] }");
    let expr = get_cell_expr(&compiled, "x");
    let values = ValueMap::new();
    let result = reify_expr::eval_expr(expr, &reify_expr::EvalContext::simple(&values));
    match result {
        Value::List(elems) => {
            assert_eq!(elems.len(), 3);
            assert_eq!(elems[0], Value::Int(1));
            assert_eq!(elems[1], Value::Int(2));
            assert_eq!(elems[2], Value::Int(3));
        }
        other => panic!("expected List, got {:?}", other),
    }
}

#[test]
fn e2e_set_literal() {
    let compiled = parse_and_compile("structure S { let x = set{1, 2, 3} }");
    let expr = get_cell_expr(&compiled, "x");
    let values = ValueMap::new();
    let result = reify_expr::eval_expr(expr, &reify_expr::EvalContext::simple(&values));
    match result {
        Value::Set(elems) => {
            assert_eq!(elems.len(), 3);
            assert!(elems.contains(&Value::Int(1)));
            assert!(elems.contains(&Value::Int(2)));
            assert!(elems.contains(&Value::Int(3)));
        }
        other => panic!("expected Set, got {:?}", other),
    }
}

#[test]
fn e2e_map_literal() {
    let compiled = parse_and_compile(r#"structure S { let x = map{"a" => 1, "b" => 2} }"#);
    let expr = get_cell_expr(&compiled, "x");
    let values = ValueMap::new();
    let result = reify_expr::eval_expr(expr, &reify_expr::EvalContext::simple(&values));
    match result {
        Value::Map(entries) => {
            assert_eq!(entries.len(), 2);
            assert_eq!(
                entries.get(&Value::String("a".into())),
                Some(&Value::Int(1))
            );
            assert_eq!(
                entries.get(&Value::String("b".into())),
                Some(&Value::Int(2))
            );
        }
        other => panic!("expected Map, got {:?}", other),
    }
}

#[test]
fn e2e_empty_list() {
    let compiled = parse_and_compile("structure S { let x = [] }");
    let expr = get_cell_expr(&compiled, "x");
    let values = ValueMap::new();
    let result = reify_expr::eval_expr(expr, &reify_expr::EvalContext::simple(&values));
    match result {
        Value::List(elems) => {
            assert_eq!(elems.len(), 0);
        }
        other => panic!("expected empty List, got {:?}", other),
    }
}

// ─── step-35: Integration tests for .count and index access ───

#[test]
fn e2e_list_count() {
    let compiled = parse_and_compile("structure S { let items = [1, 2, 3]  let n = items.count }");
    // First evaluate 'items' to populate the ValueMap, then evaluate 'n'
    let items_expr = get_cell_expr(&compiled, "items");
    let n_expr = get_cell_expr(&compiled, "n");

    let mut values = ValueMap::new();
    let items_id = reify_types::ValueCellId::new("S", "items");
    let items_val = reify_expr::eval_expr(items_expr, &reify_expr::EvalContext::simple(&values));
    values.insert(items_id, items_val);

    let result = reify_expr::eval_expr(n_expr, &reify_expr::EvalContext::simple(&values));
    assert_eq!(result, Value::Int(3), "items.count should be 3");
}

#[test]
fn e2e_list_index_access() {
    let compiled = parse_and_compile("structure S { let items = [10, 20, 30]  let x = items[1] }");
    let items_expr = get_cell_expr(&compiled, "items");
    let x_expr = get_cell_expr(&compiled, "x");

    let mut values = ValueMap::new();
    let items_id = reify_types::ValueCellId::new("S", "items");
    let items_val = reify_expr::eval_expr(items_expr, &reify_expr::EvalContext::simple(&values));
    values.insert(items_id, items_val);

    let result = reify_expr::eval_expr(x_expr, &reify_expr::EvalContext::simple(&values));
    assert_eq!(result, Value::Int(20), "items[1] should be 20");
}

#[test]
fn e2e_map_index_access() {
    let compiled =
        parse_and_compile(r#"structure S { let m = map{"a" => 10, "b" => 20}  let x = m["a"] }"#);
    let m_expr = get_cell_expr(&compiled, "m");
    let x_expr = get_cell_expr(&compiled, "x");

    let mut values = ValueMap::new();
    let m_id = reify_types::ValueCellId::new("S", "m");
    let m_val = reify_expr::eval_expr(m_expr, &reify_expr::EvalContext::simple(&values));
    values.insert(m_id, m_val);

    let result = reify_expr::eval_expr(x_expr, &reify_expr::EvalContext::simple(&values));
    assert_eq!(result, Value::Int(10), r#"m["a"] should be 10"#);
}

#[test]
fn e2e_list_sum() {
    let compiled = parse_and_compile("structure S { let items = [1, 2, 3]  let s = items.sum }");
    let items_expr = get_cell_expr(&compiled, "items");
    let s_expr = get_cell_expr(&compiled, "s");

    let mut values = ValueMap::new();
    let items_id = reify_types::ValueCellId::new("S", "items");
    let items_val = reify_expr::eval_expr(items_expr, &reify_expr::EvalContext::simple(&values));
    values.insert(items_id, items_val);

    let result = reify_expr::eval_expr(s_expr, &reify_expr::EvalContext::simple(&values));
    assert_eq!(result, Value::Int(6), "items.sum should be 6");
}

// ─── task 2698: type inference for `single` and `flat_map` ───

/// `single(List<T>) -> T`. Without a name-driven branch, the compiler's
/// `OverloadResolution::NoUserFunctions` arm falls back to the first arg's
/// type (List<Int>), which is wrong — the inferred result type must unwrap
/// the list to its element type.
#[test]
fn compile_single_infers_element_type() {
    let compiled = parse_and_compile("structure S { let top = single([42]) }");
    let expr = get_cell_expr(&compiled, "top");
    assert_eq!(
        expr.result_type,
        Type::Int,
        "single([Int]) should have result_type Int, got {:?}",
        expr.result_type
    );
}

/// `flat_map(List<A>, (A) -> List<B>) -> List<B>`. The element type of the
/// result follows the lambda's return type, NOT the input list's element
/// type — so a Bool-bodied lambda must yield `List<Bool>` rather than
/// `List<Int>` (which would happen via the first-arg fallback).
#[test]
fn compile_flat_map_infers_lambda_return_type_bool() {
    let compiled =
        parse_and_compile("structure S { let xs = flat_map([1, 2, 3], |x| [x > 0]) }");
    let expr = get_cell_expr(&compiled, "xs");
    assert_eq!(
        expr.result_type,
        Type::List(Box::new(Type::Bool)),
        "flat_map([Int], |x| [Bool]) should have result_type List<Bool>, got {:?}",
        expr.result_type
    );
}

/// Second discriminator: untyped lambda params default to Real (current
/// language behaviour — see compile_expr_guarded's Lambda arm), so the
/// body `[x, x]` has return_type `List<Real>` regardless of the input
/// list element type. The new branch reads that lambda return_type, so
/// `flat_map([Int], |x| [x, x])` infers `List<Real>` — which differs from
/// the first-arg fallback's `List<Int>`, proving the new branch fired.
/// (Refining lambda-param inference from the input list element type is
/// out-of-scope for task 2698 per the design decisions.)
#[test]
fn compile_flat_map_infers_lambda_return_type_real() {
    let compiled =
        parse_and_compile("structure S { let xs = flat_map([1, 2, 3], |x| [x, x]) }");
    let expr = get_cell_expr(&compiled, "xs");
    assert_eq!(
        expr.result_type,
        Type::List(Box::new(Type::Real)),
        "flat_map([Int], |x: Real| [x, x]) should have result_type List<Real>, got {:?}",
        expr.result_type
    );
}
