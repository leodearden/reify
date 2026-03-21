//! Field declaration tests.
//!
//! Tests for `field def name : DomainType -> CodomainType { source = kind { ... } }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("field_test"));
    (module.declarations, module.errors)
}
