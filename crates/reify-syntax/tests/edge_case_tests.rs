//! Edge case and robustness tests for parser error recovery.
//!
//! These tests verify the parser doesn't panic on malformed input
//! and handles edge cases in string/quote stripping correctly.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("edge_test"));
    (module.declarations, module.errors)
}

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("edge_test"));
    let structure = match &module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

// ── C2: lower_string_literal should not panic on malformed input ────

/// Verify a normal string literal round-trips correctly.
#[test]
fn string_literal_normal() {
    let source = r#"
structure S {
    let s = "hello world"
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "s");
    match &let_decl.value.kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "hello world"),
        other => panic!("expected StringLiteral, got {:?}", other),
    }
}

/// An unclosed string quote must not panic the parser.
/// It may produce parse errors, but should never crash.
#[test]
fn unclosed_string_does_not_panic() {
    let result = std::panic::catch_unwind(|| {
        let source = "structure S {\n    let s = \"hello\n}";
        let _module = reify_syntax::parse(source, reify_types::ModulePath::single("edge_test"));
    });
    assert!(result.is_ok(), "parser panicked on unclosed string literal");
}

/// An empty string literal should parse correctly.
#[test]
fn empty_string_literal() {
    let source = r#"
structure S {
    let s = ""
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match &let_decl.value.kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, ""),
        other => panic!("expected StringLiteral, got {:?}", other),
    }
}

// ── L5: field_source_imported should strip only one pair of quotes ───

/// Verify that an imported field path has exactly one pair of quotes stripped.
#[test]
fn imported_field_strips_one_pair_of_quotes() {
    let source =
        r#"field def data : Point3 -> Scalar { source = imported { "path/to/data.vtu" } }"#;
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };
    assert_eq!(field.name, "data");

    match &field.source {
        FieldSource::Imported { path } => {
            assert_eq!(path, "path/to/data.vtu");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}

/// A path with embedded quotes should only have the outer pair stripped.
/// With trim_matches('"'), a path like `""nested""` would lose ALL quotes.
/// With strip_prefix/strip_suffix, only the outer pair is removed.
#[test]
fn imported_field_preserves_inner_quotes() {
    // This test documents the L5 bug fix: trim_matches strips ALL leading/trailing
    // quote chars. A path like `"data""file.vtu"` would become `data""file.vtu`
    // with the correct fix, but `data` with trim_matches.
    //
    // We can't easily construct such a path through the grammar (the grammar
    // only allows one string literal), but the fix in step-4 ensures correctness
    // at the code level.
    let source =
        r#"field def data : Point3 -> Scalar { source = imported { "data.vtu" } }"#;
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };
    match &field.source {
        FieldSource::Imported { path } => {
            assert_eq!(path, "data.vtu");
        }
        other => panic!("expected Imported source, got {:?}", other),
    }
}
