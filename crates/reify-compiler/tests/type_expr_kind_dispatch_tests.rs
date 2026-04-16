//! Compiler dispatch tests for the `TypeExprKind` enum refactor (step 3 red phase).
//!
//! These tests construct `TypeExpr` nodes directly using the new enum API
//! (`TypeExprKind::DimensionalOp`, `TypeExprKind::Named`) and verify that
//! `reify_compiler::compile()` dispatches on them correctly.
//!
//! These tests **fail to compile** until step 4 migrates reify-compiler's consumer
//! sites — that compile failure is the "red" phase that justifies step 4.

use reify_compiler::*;
use reify_syntax::*;
use reify_types::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn dummy_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

fn dummy_hash(s: &str) -> ContentHash {
    ContentHash::of_str(s)
}

/// Build a `TypeExpr` that is `TypeExprKind::Named { name, type_args: [] }`.
fn named(name: &str) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_owned(),
            type_args: vec![],
        },
        span: dummy_span(),
    }
}

/// Build a `TypeExpr` that is `TypeExprKind::Named { name, type_args }`.
fn named_with_args(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_owned(),
            type_args: args,
        },
        span: dummy_span(),
    }
}

/// Build a `TypeExpr` that is `TypeExprKind::DimensionalOp { op, left, right }`.
fn dim_op(op: DimOp, left: TypeExpr, right: TypeExpr) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::DimensionalOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span: dummy_span(),
    }
}

/// Build a minimal `ParsedModule` containing a single `TypeAliasDecl`.
fn module_with_alias(alias_name: &str, type_expr: TypeExpr) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("dispatch_test"),
        declarations: vec![Declaration::TypeAlias(TypeAliasDecl {
            name: alias_name.to_owned(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            type_expr,
            span: dummy_span(),
            content_hash: dummy_hash(alias_name),
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: dummy_hash("dispatch_test_module"),
        pragmas: vec![],
    }
}

// ── Test (a): DimensionalOp(Mul, Force, Length) → ENERGY ─────────────────────

#[test]
fn dimensional_op_mul_force_length_resolves_to_energy() {
    // Manually construct: Force * Length (= Energy)
    let te = dim_op(DimOp::Mul, named("Force"), named("Length"));
    let parsed = module_with_alias("MyEnergy", te);
    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Force * Length alias should compile without errors, got: {:?}",
        errors
    );

    let alias = compiled
        .type_aliases
        .iter()
        .find(|a| a.name == "MyEnergy")
        .expect("alias should be compiled");

    let resolved = alias.resolved_type.as_ref().expect("alias should be resolved");
    assert!(
        matches!(resolved, Type::Scalar { dimension } if *dimension == DimensionVector::ENERGY),
        "Force * Length should resolve to ENERGY dimension, got: {:?}",
        resolved
    );
}

// ── Test (b): Nested (Mass * Length) / Time → MOMENTUM ───────────────────────

#[test]
fn dimensional_op_nested_mass_length_over_time_resolves() {
    // Manually construct: (Mass * Length) / Time = Momentum (kg⋅m/s)
    let mass_times_length = dim_op(DimOp::Mul, named("Mass"), named("Length"));
    let te = dim_op(DimOp::Div, mass_times_length, named("Time"));
    let parsed = module_with_alias("Momentum", te);
    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "(Mass * Length) / Time alias should compile without errors, got: {:?}",
        errors
    );

    let alias = compiled
        .type_aliases
        .iter()
        .find(|a| a.name == "Momentum")
        .expect("alias should be compiled");

    let resolved = alias.resolved_type.as_ref().expect("alias should be resolved");
    // Momentum = kg⋅m/s = Mass * Length / Time
    let expected = DimensionVector::MASS.mul(DimensionVector::LENGTH).div(DimensionVector::TIME);
    assert!(
        matches!(resolved, Type::Scalar { dimension } if *dimension == expected),
        "(Mass * Length) / Time should resolve to momentum dimension, got: {:?}",
        resolved
    );
}

// ── Test (c): DimensionalOp leaf names — no "*" or "/" in diagnostics ─────────

#[test]
fn dimensional_op_no_operator_strings_in_diagnostics() {
    // A well-formed DimensionalOp should not produce diagnostics mentioning "*" or "/"
    // as unresolved type names. If collect_type_expr_names leaks operator strings, we'd
    // see diagnostics like 'unresolved type "*"'.
    let te = dim_op(DimOp::Div, named("Force"), named("Area"));
    let parsed = module_with_alias("Pressure", te);
    let compiled = reify_compiler::compile(&parsed);

    for diag in &compiled.diagnostics {
        assert!(
            !diag.message.contains("\"*\"") && !diag.message.contains("\"/\""),
            "operator string should not appear as unresolved type in diagnostic: {:?}",
            diag.message
        );
    }
}

// ── Test (d): Named with args — collect_type_expr_names behavior ──────────────

#[test]
fn named_with_type_args_unresolved_diagnostic_mentions_type_names() {
    // Named("UnresolvedBox", [Named("UnresolvedT")]) — both are unresolved names.
    // The compiler should produce a diagnostic mentioning the *type names*, not "*" or "/".
    let te = named_with_args("UnresolvedBox", vec![named("UnresolvedT")]);
    let parsed = module_with_alias("AliasToUnresolved", te);
    let compiled = reify_compiler::compile(&parsed);

    let err_messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .map(|d| d.message.as_str())
        .collect();

    // Should have some error (UnresolvedBox isn't a known type)
    assert!(
        !err_messages.is_empty(),
        "expected errors for unresolved type, got none"
    );
    // No diagnostic should mention raw operator strings
    for msg in &err_messages {
        assert!(
            !msg.contains("\"*\"") && !msg.contains("\"/\""),
            "operator string leaked into error message: {msg:?}"
        );
    }
}
