//! Compiler tests for determinacy predicate intrinsics.
//!
//! Tests verify that the compiler intercepts determined(), undetermined(),
//! constrained(), and partially_determined() calls and emits
//! CompiledExprKind::DeterminacyPredicate instead of FunctionCall.

use reify_types::{CompiledExprKind, DeterminacyPredicateKind, Type, ValueCellId};

/// Helper: compile source and extract the value cell named `cell_name`'s default_expr.
/// Panics if there are errors or the cell is missing.
fn compile_and_get_expr(
    source: &str,
    cell_name: &str,
) -> reify_types::CompiledExpr {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_determinacy"));
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
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_determinacy"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

// ---------------------------------------------------------------------------
// step-1: compile determined(x) → DeterminacyPredicate { Determined, S.x }
// ---------------------------------------------------------------------------

#[test]
fn compile_determined_param() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    let d = determined(x)
}
"#;
    let expr = compile_and_get_expr(source, "d");

    assert_eq!(
        expr.result_type,
        Type::Bool,
        "determined(x) should have type Bool, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::Determined);
            assert_eq!(*cell, ValueCellId::new("S", "x"));
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }
}
