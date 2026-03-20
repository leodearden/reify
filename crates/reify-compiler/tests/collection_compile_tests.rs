//! Collection compilation tests (step-29 through step-36).

use reify_types::{CompiledExprKind, Severity, Value, ValueMap};

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_coll"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );
    compiled
}

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
    let compiled = compile_no_errors("structure S { let x = [1, 2, 3] }");
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
    let compiled = compile_no_errors("structure S { let x = [] }");
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
    let compiled = compile_no_errors("structure S { let x = set{1, 2} }");
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
    let compiled = compile_no_errors(r#"structure S { let x = map{"a" => 1} }"#);
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
    let compiled = compile_no_errors(
        "structure S { let items = [10, 20, 30]  let x = items[0] }",
    );
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
    let compiled = compile_no_errors(
        "structure S { let items = [1, 2, 3]  let n = items.count }",
    );
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
    let compiled = compile_no_errors(
        "structure S { let items = [1, 2, 3]  let s = items.sum }",
    );
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
    let compiled = compile_no_errors(
        r#"structure S { let m = map{"a" => 1}  let k = m.keys }"#,
    );
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
    let compiled = compile_no_errors(
        r#"structure S { let m = map{"a" => 1}  let v = m.values }"#,
    );
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
    let compiled = compile_no_errors("structure S { let x = [1, 2, 3] }");
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
    let compiled = compile_no_errors("structure S { let x = set{1, 2, 3} }");
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
    let compiled = compile_no_errors(r#"structure S { let x = map{"a" => 1, "b" => 2} }"#);
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
    let compiled = compile_no_errors("structure S { let x = [] }");
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
