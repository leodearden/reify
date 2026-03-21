//! Purpose declaration tests.
//!
//! Tests for `purpose Name(param : EntityKind) { ... }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("purpose_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic purpose (grammar-level) ─────────────────────────

#[test]
fn parse_basic_purpose_grammar() {
    // Parse a purpose declaration and verify it produces a Purpose variant
    // (not an error). This tests that the tree-sitter grammar accepts the syntax.
    let source = "purpose mfg_ready(subject : Structure) { constraint subject.params == subject.params }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::Purpose(p) => {
            assert_eq!(p.name, "mfg_ready");
        }
        other => panic!("expected Purpose, got {:?}", other),
    }
}
