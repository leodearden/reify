//! Collection compilation tests (step-29 through step-36).

use reify_types::{CompiledExprKind, Severity, Value};

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
