//! Connect and chain statement tests.
//!
//! Tests for `connect a -> b` and `chain a -> b -> c` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("connect_test"));
    (module.declarations, module.errors)
}
