//! Pragma parsing tests.
//!
//! Tests for `#ident` and `#ident(args)` pragma syntax at module and block level.

use reify_syntax::*;

/// Helper: parse source and return the ParsedModule.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_types::ModulePath::single("pragma_test"))
}

// ── Step 1: bare pragma at module level ────────────────────────────

#[test]
fn parse_bare_module_pragma() {
    let source = "#optimize\nstructure S { param x: Real }";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);
    assert_eq!(
        module.pragmas.len(),
        1,
        "expected 1 module-level pragma, got {:?}",
        module.pragmas
    );
    assert_eq!(module.pragmas[0].name, "optimize");
    assert!(
        module.pragmas[0].args.is_empty(),
        "expected no args, got {:?}",
        module.pragmas[0].args
    );
}
