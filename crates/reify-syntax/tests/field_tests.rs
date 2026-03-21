//! Field declaration tests.
//!
//! Tests for `field def name : DomainType -> CodomainType { source = kind { ... } }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("field_test"));
    (module.declarations, module.errors)
}

// ── Step 1: analytical field ─────────────────────────────────────────

#[test]
fn parse_analytical_field() {
    let (decls, errors) = parse_decls(
        "field def temp : Point3 -> Scalar { source = analytical { |p| p } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let field = match &decls[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };

    assert_eq!(field.name, "temp");
    assert!(!field.is_pub);
    assert_eq!(field.domain_type.name, "Point3");
    assert_eq!(field.codomain_type.name, "Scalar");

    match &field.source {
        FieldSource::Analytical { expr } => {
            // The expression should be a lambda: |p| p
            match &expr.kind {
                ExprKind::Lambda { params, .. } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].name, "p");
                }
                other => panic!("expected Lambda in analytical source, got {:?}", other),
            }
        }
        other => panic!("expected Analytical source, got {:?}", other),
    }
}
