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
// step-1/2: compile determined(x) → DeterminacyPredicate { Determined, S.x }
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

// ---------------------------------------------------------------------------
// step-3/4: all four determinacy kinds compile correctly
// ---------------------------------------------------------------------------

#[test]
fn compile_all_four_determinacy_kinds() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    let a = determined(x)
    let b = undetermined(x)
    let c = constrained(x)
    let d = partially_determined(x)
}
"#;
    let expected = [
        ("a", DeterminacyPredicateKind::Determined),
        ("b", DeterminacyPredicateKind::Undetermined),
        ("c", DeterminacyPredicateKind::Constrained),
        ("d", DeterminacyPredicateKind::PartiallyDetermined),
    ];

    for (cell_name, expected_kind) in &expected {
        let expr = compile_and_get_expr(source, cell_name);
        assert_eq!(
            expr.result_type,
            Type::Bool,
            "{}() should have type Bool",
            cell_name
        );
        match &expr.kind {
            CompiledExprKind::DeterminacyPredicate { kind, cell } => {
                assert_eq!(kind, expected_kind, "wrong kind for cell '{}'", cell_name);
                assert_eq!(*cell, ValueCellId::new("S", "x"), "wrong cell for '{}'", cell_name);
            }
            other => panic!("expected DeterminacyPredicate for '{}', got {:?}", cell_name, other),
        }
    }
}

// ---------------------------------------------------------------------------
// step-5/6: error on wrong arity (0 args, 2 args)
// ---------------------------------------------------------------------------

#[test]
fn error_wrong_arity_zero_args() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    let d = determined()
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for determined() with 0 args");
    assert!(
        errors[0].message.contains("exactly 1 argument"),
        "error message should mention 'exactly 1 argument', got: {:?}",
        errors[0].message
    );
    // The let should NOT compile to DeterminacyPredicate (falls back to Literal Undef)
    let template = &compiled.templates[0];
    let cell = template.value_cells.iter().find(|vc| vc.id.member == "d");
    if let Some(c) = cell {
        if let Some(expr) = &c.default_expr {
            assert!(
                !matches!(&expr.kind, CompiledExprKind::DeterminacyPredicate { .. }),
                "should NOT be DeterminacyPredicate on error, got {:?}",
                expr.kind
            );
        }
    }
}

#[test]
fn error_wrong_arity_two_args() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    param y : Scalar = 3mm
    let d = determined(x, y)
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for determined(x, y) with 2 args");
    assert!(
        errors[0].message.contains("exactly 1 argument"),
        "error message should mention 'exactly 1 argument', got: {:?}",
        errors[0].message
    );
}

// ---------------------------------------------------------------------------
// step-7/8: error on non-identifier argument
// ---------------------------------------------------------------------------

#[test]
fn error_non_identifier_argument() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    let d = determined(x + 1)
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for determined(x + 1)");
    assert!(
        errors[0].message.contains("identifier"),
        "error message should mention 'identifier', got: {:?}",
        errors[0].message
    );
    // Should NOT be DeterminacyPredicate
    let template = &compiled.templates[0];
    let cell = template.value_cells.iter().find(|vc| vc.id.member == "d");
    if let Some(c) = cell {
        if let Some(expr) = &c.default_expr {
            assert!(
                !matches!(&expr.kind, CompiledExprKind::DeterminacyPredicate { .. }),
                "should NOT be DeterminacyPredicate on error"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// step-9/10: error on unresolved name
// ---------------------------------------------------------------------------

#[test]
fn error_unresolved_name() {
    let source = r#"
structure S {
    let d = determined(nonexistent)
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected an error for determined(nonexistent)");
    assert!(
        errors[0].message.contains("nonexistent"),
        "error message should mention 'nonexistent', got: {:?}",
        errors[0].message
    );
    // Should NOT be DeterminacyPredicate
    let template = &compiled.templates[0];
    let cell = template.value_cells.iter().find(|vc| vc.id.member == "d");
    if let Some(c) = cell {
        if let Some(expr) = &c.default_expr {
            assert!(
                !matches!(&expr.kind, CompiledExprKind::DeterminacyPredicate { .. }),
                "should NOT be DeterminacyPredicate on error"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// step-11/12: collect_value_refs includes cell from DeterminacyPredicate
// ---------------------------------------------------------------------------

#[test]
fn collect_value_refs_includes_cell() {
    let cell = ValueCellId::new("S", "x");
    let expr = reify_types::CompiledExpr::determinacy_predicate(
        DeterminacyPredicateKind::Determined,
        cell.clone(),
    );
    let refs = expr.collect_value_refs();
    assert!(
        refs.contains(&cell),
        "collect_value_refs should contain the cell, got: {:?}",
        refs
    );
}

// ---------------------------------------------------------------------------
// step-13/14: content hash stability and uniqueness
// ---------------------------------------------------------------------------

#[test]
fn content_hash_stability_and_uniqueness() {
    let cell_x = ValueCellId::new("S", "x");
    let cell_y = ValueCellId::new("S", "y");

    // Same input → same hash
    let expr1 = reify_types::CompiledExpr::determinacy_predicate(
        DeterminacyPredicateKind::Determined,
        cell_x.clone(),
    );
    let expr2 = reify_types::CompiledExpr::determinacy_predicate(
        DeterminacyPredicateKind::Determined,
        cell_x.clone(),
    );
    assert_eq!(
        expr1.content_hash, expr2.content_hash,
        "same input should produce same hash"
    );

    // Different kind → different hash
    let expr3 = reify_types::CompiledExpr::determinacy_predicate(
        DeterminacyPredicateKind::Undetermined,
        cell_x.clone(),
    );
    assert_ne!(
        expr1.content_hash, expr3.content_hash,
        "Determined vs Undetermined should have different hashes"
    );

    // Different cell → different hash
    let expr4 = reify_types::CompiledExpr::determinacy_predicate(
        DeterminacyPredicateKind::Determined,
        cell_y.clone(),
    );
    assert_ne!(
        expr1.content_hash, expr4.content_hash,
        "S.x vs S.y should have different hashes"
    );
}

// ---------------------------------------------------------------------------
// step-15/16: NOT compiled as FunctionCall
// ---------------------------------------------------------------------------

#[test]
fn not_compiled_as_function_call() {
    let source = r#"
structure S {
    param x : Scalar = 5mm
    let d = determined(x)
}
"#;
    let expr = compile_and_get_expr(source, "d");

    // Top-level should not be FunctionCall
    assert!(
        !matches!(&expr.kind, CompiledExprKind::FunctionCall { .. }),
        "determined(x) should NOT be FunctionCall, got {:?}",
        expr.kind
    );

    // Walk the tree — no visited node should be a FunctionCall with name 'determined'
    let mut found_determined_fn = false;
    expr.walk(&mut |node| {
        if let CompiledExprKind::FunctionCall { function, .. } = &node.kind {
            if function.name == "determined" {
                found_determined_fn = true;
            }
        }
    });
    assert!(
        !found_determined_fn,
        "should not find any FunctionCall named 'determined' in the expression tree"
    );
}
