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

// ── Simple type alias ─────────────────────────────────────────────

#[test]
fn parse_simple_type_alias() {
    let source = "type Pressure = Force";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Pressure");
            assert!(!ta.is_pub);
            assert!(ta.type_params.is_empty());
            assert_eq!(ta.type_expr.name, "Force");
            assert!(ta.type_expr.type_args.is_empty());
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Pub type alias ────────────────────────────────────────────────

#[test]
fn parse_pub_type_alias() {
    let source = "pub type Stress = Force";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert!(ta.is_pub, "expected is_pub == true");
            assert_eq!(ta.name, "Stress");
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Type alias with parameterized RHS ─────────────────────────────

#[test]
fn parse_type_alias_parameterized_rhs() {
    let source = "type StringList = List<String>";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "StringList");
            assert_eq!(ta.type_expr.name, "List");
            assert_eq!(ta.type_expr.type_args.len(), 1);
            assert_eq!(ta.type_expr.type_args[0].name, "String");
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Type alias with type parameters ───────────────────────────────

#[test]
fn parse_type_alias_with_type_params() {
    let source = "type Container<T> = Box<T>";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Container");
            assert_eq!(ta.type_params.len(), 1);
            assert_eq!(ta.type_params[0].name, "T");
            assert_eq!(ta.type_expr.name, "Box");
            assert_eq!(ta.type_expr.type_args.len(), 1);
            assert_eq!(ta.type_expr.type_args[0].name, "T");
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Dimensional type: division ────────────────────────────────────

#[test]
fn parse_type_alias_dimensional_division() {
    let source = "type Pressure = Force / Area";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Pressure");
            // Dimensional binary op: name is the operator, type_args are operands
            assert_eq!(ta.type_expr.name, "/");
            assert_eq!(ta.type_expr.type_args.len(), 2);
            assert_eq!(ta.type_expr.type_args[0].name, "Force");
            assert_eq!(ta.type_expr.type_args[1].name, "Area");
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Dimensional type: multiplication ──────────────────────────────

#[test]
fn parse_type_alias_dimensional_multiplication() {
    let source = "type Energy = Force * Length";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Energy");
            assert_eq!(ta.type_expr.name, "*");
            assert_eq!(ta.type_expr.type_args.len(), 2);
            assert_eq!(ta.type_expr.type_args[0].name, "Force");
            assert_eq!(ta.type_expr.type_args[1].name, "Length");
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Dimensional type: chained operations ──────────────────────────

#[test]
fn parse_type_alias_dimensional_chained() {
    // Mass * Length / Time / Time — left-associative
    let source = "type Force = Mass * Length / Time / Time";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
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
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Type alias with bounded type parameter ────────────────────────

#[test]
fn parse_type_alias_bounded_type_param() {
    let source = "type Wrapper<T: Numeric> = Box<T>";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
            assert_eq!(ta.name, "Wrapper");
            assert_eq!(ta.type_params.len(), 1);
            assert_eq!(ta.type_params[0].name, "T");
            assert_eq!(ta.type_params[0].bounds, vec!["Numeric"]);
        }
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
}

// ── Type alias with default type parameter ────────────────────────

#[test]
fn parse_type_alias_default_type_param() {
    let source = "type Mapping<K, V = String> = Map<K, V>";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    match &decls[0] {
        Declaration::TypeAlias(ta) => {
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
        other => panic!("expected Declaration::TypeAlias, got {:?}", other),
    }
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
    // We don't require a specific number of errors — just that it doesn't panic
    // and does NOT produce a well-formed TypeAlias (or produces errors).
    let has_type_alias = decls.iter().any(|d| matches!(d, Declaration::TypeAlias(_)));
    assert!(
        !has_type_alias || !errors.is_empty(),
        "expected either no TypeAlias or at least one error for malformed input, got decls={:?}, errors={:?}",
        decls,
        errors,
    );
}

#[test]
fn parse_dimensional_type_missing_left_operand_no_panic() {
    // `type Foo = / Area` — operator without left operand.
    // Should NOT panic. Should produce parse error(s).
    let source = "type Foo = / Area";
    let (decls, errors) = parse_decls(source);
    let has_type_alias = decls.iter().any(|d| matches!(d, Declaration::TypeAlias(_)));
    assert!(
        !has_type_alias || !errors.is_empty(),
        "expected either no TypeAlias or at least one error for malformed input, got decls={:?}, errors={:?}",
        decls,
        errors,
    );
}

#[test]
fn parse_dimensional_type_missing_both_operands_no_panic() {
    // `type Foo = /` — only the operator, no operands at all.
    // Should NOT panic. Should produce parse error(s).
    let source = "type Foo = /";
    let (decls, errors) = parse_decls(source);
    let has_type_alias = decls.iter().any(|d| matches!(d, Declaration::TypeAlias(_)));
    assert!(
        !has_type_alias || !errors.is_empty(),
        "expected either no TypeAlias or at least one error for malformed input, got decls={:?}, errors={:?}",
        decls,
        errors,
    );
}

// ── Error case: missing name ─────────────────────────────────────

#[test]
fn parse_type_alias_missing_name_no_panic() {
    // `type = Force` — name is absent.
    // Should NOT panic. Should produce parse error(s), no valid TypeAlias emitted.
    let source = "type = Force";
    let (decls, errors) = parse_decls(source);
    let has_type_alias = decls.iter().any(|d| matches!(d, Declaration::TypeAlias(_)));
    assert!(
        !has_type_alias || !errors.is_empty(),
        "expected either no TypeAlias or at least one error for malformed input, got decls={:?}, errors={:?}",
        decls,
        errors,
    );
}
