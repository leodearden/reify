//! Type alias declaration tests.
//!
//! Tests for `type Name<Params> = type_expr` declarations, including dimensional
//! type expressions like `Force / Area`.

use reify_ast::{Declaration, DimOp, ParseError, TypeAliasDecl, TypeExpr, TypeExprKind};

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("type_alias_test"));
    (module.declarations, module.errors)
}

/// Helper: assert no errors, exactly one declaration, and extract &TypeAliasDecl.
fn unwrap_single_type_alias<'a>(
    decls: &'a [Declaration],
    errors: &[ParseError],
) -> &'a TypeAliasDecl {
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);
    match &decls[0] {
        Declaration::TypeAlias(ta) => ta,
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

/// Helper: assert that malformed input either produces no TypeAlias or produces errors.
fn assert_malformed_recovers(decls: &[Declaration], errors: &[ParseError]) {
    let has_type_alias = decls.iter().any(|d| matches!(d, Declaration::TypeAlias(_)));
    assert!(
        !has_type_alias || !errors.is_empty(),
        "expected either no TypeAlias or at least one error for malformed input, got decls={:?}, errors={:?}",
        decls,
        errors,
    );
}

/// Helper: unwrap a Named type_expr returning (name, type_args).
fn as_named(te: &TypeExpr) -> (&str, &[TypeExpr]) {
    match &te.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected TypeExprKind::Named, got {:?}", other),
    }
}

/// Helper: unwrap a DimensionalOp type_expr returning (op, left, right).
fn as_dim_op(te: &TypeExpr) -> (DimOp, &TypeExpr, &TypeExpr) {
    match &te.kind {
        TypeExprKind::DimensionalOp { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        other => panic!("expected TypeExprKind::DimensionalOp, got {:?}", other),
    }
}

/// Helper: if the parser recovers a TypeAlias named "Foo", assert the recovery is NOT
/// a well-formed binary dimensional op.
fn assert_no_valid_binop_recovery(decls: &[Declaration]) {
    if let Some(ta) = decls.iter().find_map(|d| match d {
        Declaration::TypeAlias(ta) => Some(ta),
        _ => None,
    }) {
        assert_eq!(ta.name, "Foo");
        let looks_like_valid_binop =
            matches!(&ta.type_expr.kind, TypeExprKind::DimensionalOp { .. });
        assert!(
            !looks_like_valid_binop,
            "malformed input should not produce well-formed dimensional binary op, got {:?}",
            ta.type_expr,
        );
    }
}

// ── Simple type alias ─────────────────────────────────────────────

#[test]
fn parse_simple_type_alias() {
    let source = "type Pressure = Force";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Pressure");
    assert!(!ta.is_pub);
    assert!(ta.type_params.is_empty());
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "Force");
    assert!(args.is_empty());
}

// ── Pub type alias ────────────────────────────────────────────────

#[test]
fn parse_pub_type_alias() {
    let source = "pub type Stress = Force";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert!(ta.is_pub, "expected is_pub == true");
    assert_eq!(ta.name, "Stress");
}

// ── Type alias with parameterized RHS ─────────────────────────────

#[test]
fn parse_type_alias_parameterized_rhs() {
    let source = "type StringList = List<String>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "StringList");
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "List");
    assert_eq!(args.len(), 1);
    let (inner_name, inner_args) = as_named(&args[0]);
    assert_eq!(inner_name, "String");
    assert!(inner_args.is_empty());
}

// ── Type alias with type parameters ───────────────────────────────

#[test]
fn parse_type_alias_with_type_params() {
    let source = "type Container<T> = Box<T>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Container");
    assert_eq!(ta.type_params.len(), 1);
    assert_eq!(ta.type_params[0].name, "T");
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "Box");
    assert_eq!(args.len(), 1);
    let (inner_name, _) = as_named(&args[0]);
    assert_eq!(inner_name, "T");
}

// ── Dimensional type: division ────────────────────────────────────

#[test]
fn parse_type_alias_dimensional_division() {
    let source = "type Pressure = Force / Area";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Pressure");
    // Dimensional binary op: DimOp::Div with Named left/right operands
    let (op, left, right) = as_dim_op(&ta.type_expr);
    assert!(matches!(op, DimOp::Div), "expected DimOp::Div");
    let (lname, _) = as_named(left);
    assert_eq!(lname, "Force");
    let (rname, _) = as_named(right);
    assert_eq!(rname, "Area");
}

// ── Dimensional type: multiplication ──────────────────────────────

#[test]
fn parse_type_alias_dimensional_multiplication() {
    let source = "type Energy = Force * Length";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Energy");
    let (op, left, right) = as_dim_op(&ta.type_expr);
    assert!(matches!(op, DimOp::Mul), "expected DimOp::Mul");
    let (lname, _) = as_named(left);
    assert_eq!(lname, "Force");
    let (rname, _) = as_named(right);
    assert_eq!(rname, "Length");
}

// ── Dimensional type: chained operations ──────────────────────────

#[test]
fn parse_type_alias_dimensional_chained() {
    // Mass * Length / Time / Time — left-associative
    // ((Mass * Length) / Time) / Time
    let source = "type Force = Mass * Length / Time / Time";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Force");

    // Outer: ((Mass * Length) / Time) / Time — op is Div
    let (outer_op, outer_left, outer_right) = as_dim_op(&ta.type_expr);
    assert!(matches!(outer_op, DimOp::Div), "outer op should be Div");

    // Right of outer /: Time
    let (rname, _) = as_named(outer_right);
    assert_eq!(rname, "Time");

    // Left of outer /: (Mass * Length) / Time — op is Div
    let (inner_op, inner_left, inner_right) = as_dim_op(outer_left);
    assert!(matches!(inner_op, DimOp::Div), "inner op should be Div");

    // Right of inner /: Time
    let (inner_rname, _) = as_named(inner_right);
    assert_eq!(inner_rname, "Time");

    // Left of inner /: Mass * Length — op is Mul
    let (mul_op, mul_left, mul_right) = as_dim_op(inner_left);
    assert!(matches!(mul_op, DimOp::Mul), "mul op should be Mul");
    let (mname, _) = as_named(mul_left);
    assert_eq!(mname, "Mass");
    let (lname, _) = as_named(mul_right);
    assert_eq!(lname, "Length");
}

// ── Type alias with bounded type parameter ────────────────────────

#[test]
fn parse_type_alias_bounded_type_param() {
    let source = "type Wrapper<T: Numeric> = Box<T>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Wrapper");
    assert_eq!(ta.type_params.len(), 1);
    assert_eq!(ta.type_params[0].name, "T");
    assert_eq!(ta.type_params[0].bounds, vec!["Numeric"]);
}

// ── Type alias with default type parameter ────────────────────────

#[test]
fn parse_type_alias_default_type_param() {
    let source = "type Mapping<K, V = String> = Map<K, V>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Mapping");
    assert_eq!(ta.type_params.len(), 2);
    assert_eq!(ta.type_params[0].name, "K");
    assert!(ta.type_params[0].default.is_none());
    assert_eq!(ta.type_params[1].name, "V");
    let default = ta.type_params[1]
        .default
        .as_ref()
        .expect("expected default type");
    let (default_name, _) = as_named(default);
    assert_eq!(default_name, "String");
}

// ── Type alias among other declarations ───────────────────────────

#[test]
fn parse_type_alias_among_other_declarations() {
    let source = "\
structure Bolt {}
type Pressure = Force / Area
unit mm : Length = 0.001
";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 3, "expected 3 declarations");

    match &decls[0] {
        Declaration::Structure(s) => assert_eq!(s.name, "Bolt"),
        other => panic!("expected Declaration::Structure, got {:?}", other),
    }
    match &decls[1] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Pressure");
            assert!(
                matches!(
                    &ta.type_expr.kind,
                    TypeExprKind::DimensionalOp { op: DimOp::Div, .. }
                ),
                "expected DimensionalOp(Div), got {:?}",
                ta.type_expr.kind
            );
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
    match &decls[2] {
        Declaration::Unit(u) => assert_eq!(u.name, "mm"),
        other => panic!("expected Declaration::Unit, got {:?}", other),
    }
}

// ── Multiple type aliases ─────────────────────────────────────────

#[test]
fn parse_multiple_type_aliases() {
    let source = "\
type Pressure = Force / Area
type Velocity = Length / Time
type Energy = Force * Length
";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 3);

    let names: Vec<&str> = decls
        .iter()
        .map(|d| match d {
            Declaration::TypeAlias(ta) => ta.name.as_str(),
            other => panic!("expected Declaration::TypeAlias, got {:?}", other),
        })
        .collect();

    assert_eq!(names, vec!["Pressure", "Velocity", "Energy"]);
}

// ── Malformed dimensional type expressions (should not panic) ────

#[test]
fn parse_dimensional_type_missing_right_operand_no_panic() {
    // `type Foo = Force /` — EOF after operator, right operand missing.
    // Should NOT panic. Should produce parse error(s), no valid TypeAlias emitted.
    let source = "type Foo = Force /";
    let (decls, errors) = parse_decls(source);
    assert_malformed_recovers(&decls, &errors);
    assert_no_valid_binop_recovery(&decls);
}

#[test]
fn parse_dimensional_type_missing_left_operand_no_panic() {
    // `type Foo = / Area` — operator without left operand.
    // Should NOT panic. Should produce parse error(s).
    let source = "type Foo = / Area";
    let (decls, errors) = parse_decls(source);
    assert_malformed_recovers(&decls, &errors);
    assert_no_valid_binop_recovery(&decls);
}

#[test]
fn parse_dimensional_type_missing_both_operands_no_panic() {
    // `type Foo = /` — only the operator, no operands at all.
    // Should NOT panic. Should produce parse error(s).
    let source = "type Foo = /";
    let (decls, errors) = parse_decls(source);
    assert_malformed_recovers(&decls, &errors);
    assert_no_valid_binop_recovery(&decls);
}

// ── Error case: missing name ─────────────────────────────────────

#[test]
fn parse_type_alias_missing_name_no_panic() {
    // `type = Force` — name is absent.
    // Should NOT panic. Should produce parse error(s), no valid TypeAlias emitted.
    let source = "type = Force";
    let (decls, errors) = parse_decls(source);
    assert_malformed_recovers(&decls, &errors);
}

// ── Error case: missing '=' ──────────────────────────────────────

#[test]
fn parse_type_alias_missing_equals_no_panic() {
    // `type Foo Force` — missing '=' between name and RHS.
    // Should NOT panic. Should produce parse error(s), no valid TypeAlias emitted.
    let source = "type Foo Force";
    let (decls, errors) = parse_decls(source);
    assert_malformed_recovers(&decls, &errors);
}

// ── Error case: empty RHS ────────────────────────────────────────

#[test]
fn parse_type_alias_empty_rhs_no_panic() {
    // `type Foo =` — RHS is empty (no type expression after '=').
    // Should NOT panic. Tree-sitter error recovery produces a zero-width node
    // that gets lowered to a TypeAlias with an empty-name Named type_expr.
    let source = "type Foo =";
    let (decls, errors) = parse_decls(source);
    assert!(
        errors.is_empty(),
        "expected no parse errors for empty-RHS recovery, got: {errors:?}"
    );

    // Tree-sitter recovery produces a TypeAlias — extract it.
    // This .expect() depends on the current Tree-sitter grammar/error-recovery behavior;
    // if a grammar change stops producing a TypeAlias node here, this will need updating.
    let ta = decls
        .iter()
        .find_map(|d| match d {
            Declaration::TypeAlias(ta) => Some(ta),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected Tree-sitter recovery to produce a TypeAlias for empty RHS, \
                 got decls={decls:?}, errors={errors:?}"
            )
        });

    // Name should survive recovery intact
    assert_eq!(
        ta.name, "Foo",
        "name should survive recovery; decls={decls:?}, errors={errors:?}"
    );
    // Zero-width recovery node produces an empty-name Named type_expr with no type_args
    let (te_name, te_args) = as_named(&ta.type_expr);
    assert!(
        te_name.is_empty(),
        "expected empty name for zero-width recovery node, got {:?}; errors={errors:?}",
        te_name,
    );
    assert!(
        te_args.is_empty(),
        "expected no type_args for zero-width recovery node, got {:?}; errors={errors:?}",
        te_args,
    );
}

// ── Type params combined with dimensional RHS ────────────────────

#[test]
fn parse_type_alias_type_params_with_dimensional_rhs() {
    // Combines type parameters and dimensional expressions — previously untested together.
    let source = "type Velocity<T> = T / Time";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Velocity");
    // Type parameter
    assert_eq!(ta.type_params.len(), 1);
    assert_eq!(ta.type_params[0].name, "T");
    // Dimensional expression: T / Time
    let (op, left, right) = as_dim_op(&ta.type_expr);
    assert!(matches!(op, DimOp::Div), "expected DimOp::Div");
    let (lname, _) = as_named(left);
    assert_eq!(lname, "T");
    let (rname, _) = as_named(right);
    assert_eq!(rname, "Time");
}

// ── Nested parameterized types ───────────────────────────────────

#[test]
fn parse_type_alias_nested_parameterized_types() {
    // Nested type arguments: Map<String, List<Int>> — previously only single-level tested.
    let source = "type Registry = Map<String, List<Int>>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Registry");
    let (name, args) = as_named(&ta.type_expr);
    assert_eq!(name, "Map");
    assert_eq!(args.len(), 2);
    // First type arg: String (simple)
    let (arg0_name, arg0_args) = as_named(&args[0]);
    assert_eq!(arg0_name, "String");
    assert!(arg0_args.is_empty());
    // Second type arg: List<Int> (nested parameterized)
    let (arg1_name, arg1_args) = as_named(&args[1]);
    assert_eq!(arg1_name, "List");
    assert_eq!(arg1_args.len(), 1);
    let (inner_name, _) = as_named(&arg1_args[0]);
    assert_eq!(inner_name, "Int");
}

// ── Mixed operator precedence ────────────────────────────────────

#[test]
fn parse_type_alias_mixed_operator_precedence() {
    // Mass / Time * Scalar — left-associative with equal precedence.
    // Expected parse: (Mass / Time) * Scalar — outer node is '*'.
    // This tests '/' followed by '*' (the existing chained test uses '* / /').
    let source = "type X = Mass / Time * Scalar";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "X");
    // Outer: (Mass / Time) * Scalar — op is Mul
    let (outer_op, outer_left, outer_right) = as_dim_op(&ta.type_expr);
    assert!(matches!(outer_op, DimOp::Mul), "outer op should be Mul");

    // Right of outer *: Scalar
    let (rname, _) = as_named(outer_right);
    assert_eq!(rname, "Scalar");

    // Left of outer *: Mass / Time — op is Div
    let (div_op, div_left, div_right) = as_dim_op(outer_left);
    assert!(matches!(div_op, DimOp::Div), "inner op should be Div");
    let (div_lname, _) = as_named(div_left);
    assert_eq!(div_lname, "Mass");
    let (div_rname, _) = as_named(div_right);
    assert_eq!(div_rname, "Time");
}
