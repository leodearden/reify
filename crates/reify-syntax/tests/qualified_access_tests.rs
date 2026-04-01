//! Qualified access (`::`) and instance qualified access (`.(...)`) parsing integration tests.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("qualified_test"));
    (module.declarations, module.errors)
}
