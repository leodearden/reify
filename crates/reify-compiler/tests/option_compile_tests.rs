//! Compiler tests for some(expr) and none expressions.
//!
//! Tests verify that the compiler emits CompiledExprKind::OptionSome and
//! CompiledExprKind::OptionNone with correct types instead of falling through
//! to generic function call resolution.

use reify_types::{CompiledExprKind, Type};

/// Helper: compile source and extract the value cell named `cell_name`'s default_expr.
/// Panics if there are errors or the cell is missing.
fn compile_and_get_expr(
    source: &str,
    cell_name: &str,
) -> reify_types::CompiledExpr {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_option"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no error diagnostics, got: {:?}", errors);

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell_name)
        .unwrap_or_else(|| panic!("should have '{}' value cell", cell_name));

    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{}' cell should have a default_expr", cell_name))
        .clone()
}

/// Helper: compile source and expect diagnostics (errors allowed). Returns compiled module.
fn compile_expecting_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_option"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

// ---------------------------------------------------------------------------
// step-2: some(42) → OptionSome with Int inner and Option<Int> result type
// ---------------------------------------------------------------------------

/// step-2: compile `let x = some(42)` → OptionSome wrapping Literal(Int(42)).
/// Currently FAILS because some(42) compiles as stdlib FunctionCall.
#[test]
fn compile_some_integer_literal() {
    let source = r#"
structure S {
    let x = some(42)
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Int)),
        "some(42) should have type Option<Int>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(inner) => {
            assert!(
                matches!(&inner.kind, CompiledExprKind::Literal(v) if matches!(v, reify_types::Value::Int(42))),
                "expected Literal(Int(42)), got {:?}",
                inner.kind
            );
            assert_eq!(inner.result_type, Type::Int);
        }
        other => panic!("expected OptionSome, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-4: none → OptionNone with Option(_) result type
// ---------------------------------------------------------------------------

/// step-4: compile `let x = none` → OptionNone.
/// Currently FAILS because none produces 'unresolved name' error.
#[test]
fn compile_none_as_let_value() {
    let source = r#"
structure S {
    let x = none
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert!(
        matches!(expr.result_type, Type::Option(_)),
        "none should have type Option<_>, got {:?}",
        expr.result_type
    );

    assert!(
        matches!(&expr.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        expr.kind
    );
}

// ---------------------------------------------------------------------------
// step-6: param with Option<Int> annotation and none default → typed OptionNone
// ---------------------------------------------------------------------------

/// step-6: compile `param x: Option<Int> = none` → OptionNone with type Option<Int>.
/// Currently FAILS because: (1) resolve_type doesn't handle Option<T>,
/// (2) none doesn't get type context from param annotation.
#[test]
fn compile_param_option_int_default_none() {
    let source = r#"
structure S {
    param x: Option<Int> = none
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_option"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no error diagnostics, got: {:?}", errors);

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");

    // The cell type should be Option<Int>
    assert_eq!(
        cell.cell_type,
        Type::Option(Box::new(Type::Int)),
        "cell_type should be Option<Int>, got {:?}",
        cell.cell_type
    );

    // The default_expr should be OptionNone with type Option<Int>
    let default = cell.default_expr.as_ref().expect("should have default");
    assert_eq!(
        default.result_type,
        Type::Option(Box::new(Type::Int)),
        "default_expr should have type Option<Int>, got {:?}",
        default.result_type
    );
    assert!(
        matches!(&default.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        default.kind
    );
}

// ---------------------------------------------------------------------------
// step-8: edge cases
// ---------------------------------------------------------------------------

/// step-8a: some() with 0 args → diagnostic error emitted.
#[test]
fn compile_some_zero_args_emits_error() {
    let source = r#"
structure S {
    let x = some()
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for some() with 0 args");
    let msg = &errors[0].message;
    assert!(
        msg.contains("some") && (msg.contains("1") || msg.contains("argument")),
        "error message should mention 'some' and argument count, got: {:?}",
        msg
    );
}

/// step-8b: some(1, 2) with 2 args → diagnostic error emitted.
#[test]
fn compile_some_two_args_emits_error() {
    let source = r#"
structure S {
    let x = some(1, 2)
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for some(1, 2) with 2 args");
}

/// step-8c: nested some(some(42)) → OptionSome(OptionSome(Literal(42))) with type Option<Option<Int>>.
#[test]
fn compile_some_nested() {
    let source = r#"
structure S {
    let x = some(some(42))
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Option(Box::new(Type::Int)))),
        "some(some(42)) should have type Option<Option<Int>>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(outer_inner) => {
            assert_eq!(
                outer_inner.result_type,
                Type::Option(Box::new(Type::Int)),
                "inner should have type Option<Int>, got {:?}",
                outer_inner.result_type
            );
            match &outer_inner.kind {
                CompiledExprKind::OptionSome(innermost) => {
                    assert!(
                        matches!(&innermost.kind, CompiledExprKind::Literal(v) if matches!(v, reify_types::Value::Int(42))),
                        "expected Literal(Int(42)), got {:?}",
                        innermost.kind
                    );
                }
                other => panic!("expected inner OptionSome, got {:?}", other),
            }
        }
        other => panic!("expected outer OptionSome, got {:?}", other),
    }
}

/// step-8d: some(x) where x is a param → OptionSome(ValueRef) with Option<param_type>.
#[test]
fn compile_some_param_ref() {
    let source = r#"
structure S {
    param x: Int
    let y = some(x)
}
"#;
    let expr = compile_and_get_expr(source, "y");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Int)),
        "some(x) where x:Int should have type Option<Int>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(inner) => {
            assert!(
                matches!(&inner.kind, CompiledExprKind::ValueRef(_)),
                "expected ValueRef for 'x', got {:?}",
                inner.kind
            );
            assert_eq!(inner.result_type, Type::Int);
        }
        other => panic!("expected OptionSome, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-10: no-context none → OptionNone with Option<Real> default, no error
// ---------------------------------------------------------------------------

/// step-10: `let x = none` (no type annotation) → OptionNone with Type::Option(Type::Real),
/// no error diagnostics. Verifies graceful fallback when type cannot be inferred.
#[test]
fn compile_none_no_context_defaults_to_option_real() {
    let source = r#"
structure S {
    let x = none
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_option"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors for `let x = none`, got: {:?}", errors);

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");

    let default = cell.default_expr.as_ref().expect("should have default");
    assert_eq!(
        default.result_type,
        Type::Option(Box::new(Type::Real)),
        "none without context should default to Option<Real>, got {:?}",
        default.result_type
    );
    assert!(
        matches!(&default.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        default.kind
    );
}
