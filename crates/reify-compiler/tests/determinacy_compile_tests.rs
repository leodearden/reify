//! Compiler-level tests for determinacy predicate intrinsics.
//!
//! Verifies that `determined`, `undetermined`, `constrained`, and
//! `partially_determined` calls compile to the correct
//! `CompiledExprKind::DeterminacyPredicate` nodes with the right
//! `DeterminacyPredicateKind` variant and `ValueCellId`.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let template = compiled
        .templates
        .into_iter()
        .next()
        .expect("expected at least 1 template");
    (template, compiled.diagnostics)
}

fn errors_only(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ---------------------------------------------------------------------------
// Happy-path: each predicate compiles to the correct DeterminacyPredicateKind
// ---------------------------------------------------------------------------

#[test]
fn compile_determined_predicate() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint determined(x)
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert!(
        errors_only(&diagnostics).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&diagnostics)
    );

    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );
    let constraint_expr = &template.constraints[0].expr;

    match &constraint_expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::Determined);
            assert_eq!(cell, &ValueCellId::new("S", "x"));
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }

    assert_eq!(constraint_expr.result_type, Type::Bool);
}

#[test]
fn compile_undetermined_predicate() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint undetermined(x)
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert!(
        errors_only(&diagnostics).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&diagnostics)
    );

    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::Undetermined);
            assert_eq!(cell, &ValueCellId::new("S", "x"));
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }
    assert_eq!(constraint_expr.result_type, Type::Bool);
}

#[test]
fn compile_constrained_predicate() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint constrained(x)
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert!(
        errors_only(&diagnostics).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&diagnostics)
    );

    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::Constrained);
            assert_eq!(cell, &ValueCellId::new("S", "x"));
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }
    assert_eq!(constraint_expr.result_type, Type::Bool);
}

#[test]
fn compile_partially_determined_predicate() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint partially_determined(x)
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert!(
        errors_only(&diagnostics).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&diagnostics)
    );

    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::PartiallyDetermined);
            assert_eq!(cell, &ValueCellId::new("S", "x"));
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }
    assert_eq!(constraint_expr.result_type, Type::Bool);
}

// ---------------------------------------------------------------------------
// Error cases: wrong argument count and non-ValueRef arguments
// ---------------------------------------------------------------------------

#[test]
fn determined_zero_args_emits_error() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint determined()
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors = errors_only(&diagnostics);
    assert!(
        errors.iter().any(|d| d.message.contains("requires exactly 1 argument")),
        "expected 'requires exactly 1 argument' error, got: {:?}",
        errors
    );
}

#[test]
fn determined_two_args_emits_error() {
    let source = r#"
structure S {
    param x : Length = 10mm
    param y : Length = 20mm
    constraint determined(x, y)
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors = errors_only(&diagnostics);
    assert!(
        errors.iter().any(|d| d.message.contains("requires exactly 1 argument")),
        "expected 'requires exactly 1 argument' error, got: {:?}",
        errors
    );
}

#[test]
fn determined_quantity_literal_arg_emits_error() {
    let source = r#"
structure S {
    param x : Length = 10mm
    constraint determined(10mm)
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors = errors_only(&diagnostics);
    assert!(
        errors.iter().any(|d| d.message.contains("must be a direct cell reference")),
        "expected 'must be a direct cell reference' error, got: {:?}",
        errors
    );
}

#[test]
fn determined_binary_expr_arg_emits_error() {
    let source = r#"
structure S {
    param x : Length = 10mm
    param y : Length = 20mm
    constraint determined(x + y)
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors = errors_only(&diagnostics);
    assert!(
        errors.iter().any(|d| d.message.contains("must be a direct cell reference")),
        "expected 'must be a direct cell reference' error, got: {:?}",
        errors
    );
}
