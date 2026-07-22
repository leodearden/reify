//! Tests for the `TypeExprKind` / `DimOp` enum refactor.
//!
//! These tests assert the *target* AST shape and are expected to **fail to compile**
//! until `TypeExprKind` and `DimOp` are introduced (Step 2).  That compile failure
//! is the "red" phase that justifies the implementation step.

use reify_ast::*;

// ── Local helpers ─────────────────────────────────────────────────

fn parse_single_type_alias(source: &str) -> TypeAliasDecl {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("type_expr_kind_test"),
    );
    assert!(
        module.errors.is_empty(),
        "unexpected parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1, "expected 1 declaration");
    match module.declarations.into_iter().next().unwrap() {
        Declaration::TypeAlias(ta) => ta,
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

/// Unwrap a `TypeExprKind::Named` arm, panicking on mismatch.
fn as_named(te: &TypeExpr) -> (&str, &[TypeExpr]) {
    match &te.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected TypeExprKind::Named, got {:?}", other),
    }
}

/// Unwrap a `TypeExprKind::DimensionalOp` arm, panicking on mismatch.
fn as_dim_op(te: &TypeExpr) -> (DimOp, &TypeExpr, &TypeExpr) {
    match &te.kind {
        TypeExprKind::DimensionalOp { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        other => panic!("expected TypeExprKind::DimensionalOp, got {:?}", other),
    }
}

// ── Case (a): division → DimensionalOp / ─────────────────────────

#[test]
fn type_expr_kind_dimensional_division() {
    let ta = parse_single_type_alias("type Pressure = Force / Area");
    let (op, left, right) = as_dim_op(&ta.type_expr);
    assert!(
        matches!(op, DimOp::Div),
        "expected DimOp::Div, got {:?}",
        op
    );
    let (lname, largs) = as_named(left);
    assert_eq!(lname, "Force");
    assert!(largs.is_empty());
    let (rname, rargs) = as_named(right);
    assert_eq!(rname, "Area");
    assert!(rargs.is_empty());
}

// ── Case (b): Named with type arg ────────────────────────────────

#[test]
fn type_expr_kind_named_parameterized() {
    let ta = parse_single_type_alias("type BoxedInt = Box<Int>");
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "Box");
    assert_eq!(args.len(), 1);
    let (inner_name, inner_args) = as_named(&args[0]);
    assert_eq!(inner_name, "Int");
    assert!(inner_args.is_empty());
}

// ── Case (c): multiplication → DimensionalOp * ───────────────────

#[test]
fn type_expr_kind_dimensional_multiplication() {
    let ta = parse_single_type_alias("type Energy = Mass * Length");
    let (op, left, right) = as_dim_op(&ta.type_expr);
    assert!(
        matches!(op, DimOp::Mul),
        "expected DimOp::Mul, got {:?}",
        op
    );
    let (lname, _) = as_named(left);
    assert_eq!(lname, "Mass");
    let (rname, _) = as_named(right);
    assert_eq!(rname, "Length");
}

// ── Case (d): DimOp round-trip ────────────────────────────────────

#[test]
fn dimop_as_str_roundtrip() {
    assert_eq!(DimOp::Mul.as_str(), "*");
    assert_eq!(DimOp::Div.as_str(), "/");
}

// ── Simple Named (no args) ────────────────────────────────────────

#[test]
fn type_expr_kind_simple_named() {
    let ta = parse_single_type_alias("type Pressure = Force");
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "Force");
    assert!(args.is_empty());
}

// ── Function type (arrow type) lowering — task 4595 step-3 ────────
//
// RED: `TypeExprKind::Function { params, return_type }` does not yet exist
// (compile error) and no lowering arm produces it.  step-4 adds the variant
// and `lower_function_type`, making these pass.

/// Parse a source containing exactly one fn declaration and return its FnDef.
fn parse_single_fn(source: &str) -> FnDef {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("function_type_lowering_test"),
    );
    assert!(
        module.errors.is_empty(),
        "unexpected parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1, "expected 1 declaration");
    match module.declarations.into_iter().next().unwrap() {
        Declaration::Function(f) => f,
        other => panic!("expected Declaration::Function, got {:?}", other),
    }
}

/// Unwrap a `TypeExprKind::Function` arm, panicking on mismatch.
fn as_function(te: &TypeExpr) -> (&[TypeExpr], &TypeExpr) {
    match &te.kind {
        TypeExprKind::Function {
            params,
            return_type,
        } => (params.as_slice(), return_type.as_ref()),
        other => panic!("expected TypeExprKind::Function, got {:?}", other),
    }
}

/// `fn apply_it(g: (T) -> U) -> Bool { true }` lowers param `g`'s type to
/// `Function { params: [Named("T")], return_type: Named("U") }`.
#[test]
fn type_expr_kind_function_single_param() {
    let f = parse_single_fn("fn apply_it(g: (T) -> U) -> Bool { true }");
    let param_ty = &f.params[0].type_expr;
    let (params, ret) = as_function(param_ty);
    assert_eq!(params.len(), 1, "single-param arrow type must have 1 param");
    let (pname, pargs) = as_named(&params[0]);
    assert_eq!(pname, "T");
    assert!(pargs.is_empty());
    let (rname, rargs) = as_named(ret);
    assert_eq!(rname, "U");
    assert!(rargs.is_empty());
}

/// `fn apply_it(g: (A, B) -> C) -> Bool { true }` lowers param `g`'s type to
/// `Function { params: [Named("A"), Named("B")], return_type: Named("C") }`.
#[test]
fn type_expr_kind_function_multi_param() {
    let f = parse_single_fn("fn apply_it(g: (A, B) -> C) -> Bool { true }");
    let param_ty = &f.params[0].type_expr;
    let (params, ret) = as_function(param_ty);
    assert_eq!(params.len(), 2, "multi-param arrow type must have 2 params");
    assert_eq!(as_named(&params[0]).0, "A");
    assert!(as_named(&params[0]).1.is_empty());
    assert_eq!(as_named(&params[1]).0, "B");
    assert!(as_named(&params[1]).1.is_empty());
    assert_eq!(as_named(ret).0, "C");
    assert!(as_named(ret).1.is_empty());
}

// ── Display behavior tests (step-5) ──────────────────────────────
//
// These tests lock down the `Display` format used in diagnostic messages,
// ensuring the design decision to use `{type_expr}` instead of
// `{type_expr.name}` preserves readable output for both variants.

fn span() -> reify_core::SourceSpan {
    reify_core::SourceSpan::new(0, 0)
}

fn named_te(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_owned(),
            type_args: args,
        },
        span: span(),
    }
}

fn dim_op_te(op: DimOp, left: TypeExpr, right: TypeExpr) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::DimensionalOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span: span(),
    }
}

// (a) Simple named type renders as bare name.
#[test]
fn display_simple_named() {
    let te = named_te("Force", vec![]);
    assert_eq!(format!("{te}"), "Force");
}

// (b) Named type with one arg renders as Name<Arg>.
#[test]
fn display_named_single_type_arg() {
    let te = named_te("Box", vec![named_te("T", vec![])]);
    assert_eq!(format!("{te}"), "Box<T>");
}

// (c) Named type with two args renders as Name<K, V>.
#[test]
fn display_named_two_type_args() {
    let te = named_te("Map", vec![named_te("K", vec![]), named_te("V", vec![])]);
    assert_eq!(format!("{te}"), "Map<K, V>");
}

// (d) DimensionalOp renders as "Left / Right".
#[test]
fn display_dimensional_op_div() {
    let te = dim_op_te(
        DimOp::Div,
        named_te("Force", vec![]),
        named_te("Area", vec![]),
    );
    assert_eq!(format!("{te}"), "Force / Area");
}

// (e) Nested DimensionalOp renders flat (no parens), preserving left-to-right order.
#[test]
fn display_nested_dimensional_op() {
    // (Mass * Length) / Time → "Mass * Length / Time"
    let inner = dim_op_te(
        DimOp::Mul,
        named_te("Mass", vec![]),
        named_te("Length", vec![]),
    );
    let te = dim_op_te(DimOp::Div, inner, named_te("Time", vec![]));
    assert_eq!(format!("{te}"), "Mass * Length / Time");
}
