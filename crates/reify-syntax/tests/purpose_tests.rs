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

// ── Step 3: detailed purpose AST ─────────────────────────────────

#[test]
fn parse_purpose_with_forall_constraint() {
    let source = "purpose manufacturing_ready(subject : Structure) { constraint forall p in subject.params: determined(p) }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let purpose = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };

    assert_eq!(purpose.name, "manufacturing_ready");
    assert!(!purpose.is_pub);
    assert!(purpose.type_params.is_empty());

    // Check parameters
    assert_eq!(purpose.params.len(), 1);
    assert_eq!(purpose.params[0].name, "subject");
    assert_eq!(purpose.params[0].entity_kind, "Structure");

    // Check members
    assert_eq!(purpose.members.len(), 1);
    match &purpose.members[0] {
        MemberDecl::Constraint(c) => {
            // The constraint contains a forall quantifier
            match &c.expr.kind {
                ExprKind::Quantifier { kind, variable, .. } => {
                    assert_eq!(*kind, QuantifierKind::ForAll);
                    assert_eq!(variable, "p");
                }
                other => panic!("expected Quantifier, got {:?}", other),
            }
        }
        other => panic!("expected Constraint member, got {:?}", other),
    }
}
