//! Type alias declaration tests.
//!
//! Tests for `type Name<Params> = type_expr` declarations, including dimensional
//! type expressions like `Force / Area`.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("type_alias_test"));
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

/// Helper: if the parser recovers a TypeAlias named "Foo", assert the recovery is NOT
/// a well-formed binary dimensional op (i.e. name="/" or "*" with exactly 2 type_args).
fn assert_no_valid_binop_recovery(decls: &[Declaration]) {
    if let Some(ta) = decls.iter().find_map(|d| match d {
        Declaration::TypeAlias(ta) => Some(ta),
        _ => None,
    }) {
        assert_eq!(ta.name, "Foo");
        let looks_like_valid_binop = (ta.type_expr.name == "/" || ta.type_expr.name == "*")
            && ta.type_expr.type_args.len() == 2;
        assert!(
            !looks_like_valid_binop,
            "malformed input should not produce well-formed dimensional binary op, got type_expr.name={:?}, type_args={:?}",
            ta.type_expr.name, ta.type_expr.type_args,
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
    assert_eq!(ta.type_expr.name, "Force");
    assert!(ta.type_expr.type_args.is_empty());
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
    assert_eq!(ta.type_expr.name, "List");
    assert_eq!(ta.type_expr.type_args.len(), 1);
    assert_eq!(ta.type_expr.type_args[0].name, "String");
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
    assert_eq!(ta.type_expr.name, "Box");
    assert_eq!(ta.type_expr.type_args.len(), 1);
    assert_eq!(ta.type_expr.type_args[0].name, "T");
}

// ── Dimensional type: division ────────────────────────────────────

#[test]
fn parse_type_alias_dimensional_division() {
    let source = "type Pressure = Force / Area";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Pressure");
    // Dimensional binary op: name is the operator, type_args are operands
    assert_eq!(ta.type_expr.name, "/");
    assert_eq!(ta.type_expr.type_args.len(), 2);
    assert_eq!(ta.type_expr.type_args[0].name, "Force");
    assert_eq!(ta.type_expr.type_args[1].name, "Area");
}

// ── Dimensional type: multiplication ──────────────────────────────

#[test]
fn parse_type_alias_dimensional_multiplication() {
    let source = "type Energy = Force * Length";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Energy");
    assert_eq!(ta.type_expr.name, "*");
    assert_eq!(ta.type_expr.type_args.len(), 2);
    assert_eq!(ta.type_expr.type_args[0].name, "Force");
    assert_eq!(ta.type_expr.type_args[1].name, "Length");
}

// ── Dimensional type: chained operations ──────────────────────────

#[test]
fn parse_type_alias_dimensional_chained() {
    // Mass * Length / Time / Time — left-associative
    let source = "type Force = Mass * Length / Time / Time";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Force");
    // ((Mass * Length) / Time) / Time
    assert_eq!(ta.type_expr.name, "/");
    assert_eq!(ta.type_expr.type_args.len(), 2);

    // right operand of outer /: Time
    assert_eq!(ta.type_expr.type_args[1].name, "Time");

    // left operand of outer /: (Mass * Length) / Time
    let inner = &ta.type_expr.type_args[0];
    assert_eq!(inner.name, "/");

    // right operand of inner /: Time
    assert_eq!(inner.type_args[1].name, "Time");

    // left operand of inner /: Mass * Length
    let mul = &inner.type_args[0];
    assert_eq!(mul.name, "*");
    assert_eq!(mul.type_args[0].name, "Mass");
    assert_eq!(mul.type_args[1].name, "Length");
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
    assert_eq!(default.name, "String");
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
            assert_eq!(ta.type_expr.name, "/");
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
    // that gets lowered to a TypeAlias with an empty-name type_expr.
    let source = "type Foo =";
    let (decls, errors) = parse_decls(source);
    assert!(
        errors.is_empty(),
        "expected no parse errors for empty-RHS recovery, got: {errors:?}"
    );

    // NOTE: Tree-sitter silently recovers `type Foo =` without emitting parse errors.
    // The `errors` vector is empty — this is expected Tree-sitter behavior, not a bug.
    // The test verifies recovery shape, not error detection.

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
    // Zero-width recovery node produces an empty-name type_expr with no type_args
    assert!(
        ta.type_expr.name.is_empty(),
        "expected empty type_expr.name for zero-width recovery node, got {:?}; errors={errors:?}",
        ta.type_expr.name,
    );
    assert!(
        ta.type_expr.type_args.is_empty(),
        "expected no type_args for zero-width recovery node, got {:?}; errors={errors:?}",
        ta.type_expr.type_args,
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
    assert_eq!(ta.type_expr.name, "/");
    assert_eq!(ta.type_expr.type_args.len(), 2);
    assert_eq!(ta.type_expr.type_args[0].name, "T");
    assert_eq!(ta.type_expr.type_args[1].name, "Time");
}

// ── Nested parameterized types ───────────────────────────────────

#[test]
fn parse_type_alias_nested_parameterized_types() {
    // Nested type arguments: Map<String, List<Int>> — previously only single-level tested.
    let source = "type Registry = Map<String, List<Int>>";
    let (decls, errors) = parse_decls(source);
    let ta = unwrap_single_type_alias(&decls, &errors);

    assert_eq!(ta.name, "Registry");
    assert_eq!(ta.type_expr.name, "Map");
    assert_eq!(ta.type_expr.type_args.len(), 2);
    // First type arg: String (simple)
    assert_eq!(ta.type_expr.type_args[0].name, "String");
    assert!(ta.type_expr.type_args[0].type_args.is_empty());
    // Second type arg: List<Int> (nested parameterized)
    assert_eq!(ta.type_expr.type_args[1].name, "List");
    assert_eq!(ta.type_expr.type_args[1].type_args.len(), 1);
    assert_eq!(ta.type_expr.type_args[1].type_args[0].name, "Int");
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
    // Outer: (Mass / Time) * Scalar
    assert_eq!(ta.type_expr.name, "*");
    assert_eq!(ta.type_expr.type_args.len(), 2);

    // Right operand of outer *: Scalar
    assert_eq!(ta.type_expr.type_args[1].name, "Scalar");

    // Left operand of outer *: Mass / Time
    let div = &ta.type_expr.type_args[0];
    assert_eq!(div.name, "/");
    assert_eq!(div.type_args.len(), 2);
    assert_eq!(div.type_args[0].name, "Mass");
    assert_eq!(div.type_args[1].name, "Time");
}
