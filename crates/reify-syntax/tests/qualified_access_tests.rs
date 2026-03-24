//! Qualified access expression tests.
//!
//! Tests for `TypeName::ident` (qualified trait access) and
//! `expr.(TypeName::ident)` (instance-level qualified trait access).

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("qualified_access_test"));
    (module.declarations, module.errors)
}
