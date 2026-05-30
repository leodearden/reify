//! Compile-time surface pin for `reify-ast`.
//!
//! Pins the full public API that `reify-ast` MUST export after the atomic
//! module move (step-2), in both the flat form (`reify_ast::Expr`) and
//! the module-path form (`reify_ast::ast::Expr`).
//!
//! Both spellings remain in sync because `reify-ast/src/lib.rs` exports
//! `pub mod ast` AND re-exports its symbols at the crate root.
//!
//! This test intentionally fails to compile before step-2 because the
//! types don't yet exist in reify-ast.

// ── flat root imports ────────────────────────────────────────────────────────
use reify_ast::{DimOp, Expr, ExprKind, LambdaParam, MatchArm, MatchPattern, QuantifierKind, TypeExpr, TypeExprKind};

// ── module-path imports ──────────────────────────────────────────────────────
use reify_ast::ast::{
    DimOp as DimOpMod,
    Expr as ExprMod,
    ExprKind as ExprKindMod,
    LambdaParam as LambdaParamMod,
    MatchArm as MatchArmMod,
    QuantifierKind as QuantifierKindMod,
    TypeExpr as TypeExprMod,
    TypeExprKind as TypeExprKindMod,
};

// ── reify-core dep edge ──────────────────────────────────────────────────────
use reify_core::SourceSpan;

// ─────────────────────────────────────────────────────────────────────────────
// Surface assertions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn expr_flat_and_module_path_constructible() {
    let span = SourceSpan::new(0, 10);

    // Build a numeric-literal Expr via flat path.
    let expr: Expr = Expr {
        kind: ExprKind::NumberLiteral { value: 42.0, is_real: false },
        span,
    };
    assert!(matches!(expr.kind, ExprKind::NumberLiteral { value, .. } if value == 42.0));

    // Build a numeric-literal Expr via module path.
    let expr_mod: ExprMod = ExprMod {
        kind: ExprKindMod::NumberLiteral { value: 1.0, is_real: true },
        span,
    };
    assert!(matches!(expr_mod.kind, ExprKindMod::NumberLiteral { is_real: true, .. }));
}

#[test]
fn expr_kind_cross_assignment_proves_same_type() {
    // Cross-assign flat → module-path: proves they name the same type.
    let flat: ExprKind = ExprKind::NumberLiteral { value: 1.0, is_real: false };
    let _same: ExprKindMod = flat;
}

#[test]
fn type_expr_flat_and_module_path_constructible() {
    let span = SourceSpan::new(0, 4);

    // Named TypeExpr via flat path.
    let ty: TypeExpr = TypeExpr {
        kind: TypeExprKind::Named { name: "Scalar".into(), type_args: vec![] },
        span,
    };
    assert!(matches!(&ty.kind, TypeExprKind::Named { name, .. } if name == "Scalar"));

    // Named TypeExpr via module path.
    let ty_mod: TypeExprMod = TypeExprMod {
        kind: TypeExprKindMod::Named { name: "Bool".into(), type_args: vec![] },
        span,
    };
    assert!(matches!(&ty_mod.kind, TypeExprKindMod::Named { name, .. } if name == "Bool"));

    // Auto TypeExpr — Display round-trip.
    let auto_ty = TypeExpr {
        kind: TypeExprKind::Auto { free: false, bound: "Bound".into() },
        span,
    };
    assert_eq!(auto_ty.to_string(), "auto: Bound");
}

#[test]
fn match_arm_flat_and_module_path_constructible() {
    let span = SourceSpan::new(0, 5);
    let body = Expr { kind: ExprKind::BoolLiteral(true), span };

    let arm: MatchArm = MatchArm {
        patterns: vec![MatchPattern::Variant("In".into())],
        body,
        span,
    };
    assert_eq!(arm.patterns.len(), 1);
    assert_eq!(arm.patterns[0], MatchPattern::Variant("In".into()));

    // Module-path alias resolves to the same type.
    let _arm_mod: MatchArmMod = arm;
}

#[test]
fn lambda_param_flat_and_module_path_constructible() {
    let span = SourceSpan::new(0, 3);
    let param: LambdaParam = LambdaParam { name: "x".into(), type_expr: None, span };
    assert_eq!(param.name, "x");

    // Module-path alias resolves to the same type.
    let _param_mod: LambdaParamMod = param;
}

#[test]
fn quantifier_kind_flat_and_module_path() {
    let qk: QuantifierKind = QuantifierKind::ForAll;
    assert_eq!(qk, QuantifierKind::ForAll);

    let qk_mod: QuantifierKindMod = QuantifierKindMod::Exists;
    assert_eq!(qk_mod, QuantifierKindMod::Exists);

    // Cross-assign flat → module-path.
    let _same: QuantifierKindMod = QuantifierKind::ForAll;
}

#[test]
fn dim_op_as_str_and_cross_assignment() {
    let mul: DimOp = DimOp::Mul;
    assert_eq!(mul.as_str(), "*");

    let div: DimOpMod = DimOpMod::Div;
    assert_eq!(div.as_str(), "/");

    // Cross-assign module-path → flat.
    let _same: DimOp = DimOpMod::Mul;
}

#[test]
fn type_expr_kind_cross_assignment_proves_same_type() {
    let flat: TypeExprKind = TypeExprKind::IntegerLiteral(3);
    let _same: TypeExprKindMod = flat;
}
