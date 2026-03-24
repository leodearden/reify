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

// ── Step 3/4: pragma with key=value args ─────────────────────────

#[test]
fn parse_key_value_pragma_args() {
    let source = "#config(level=3, name=\"test\")\nstructure S {}";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "config");
    assert_eq!(pragma.args.len(), 2, "expected 2 args, got {:?}", pragma.args);

    // First arg: level=3
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "level");
            assert_eq!(*value, PragmaValue::Number(3.0));
        }
        other => panic!("expected KeyValue, got {:?}", other),
    }

    // Second arg: name="test"
    match &pragma.args[1] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "name");
            assert_eq!(*value, PragmaValue::String("test".to_string()));
        }
        other => panic!("expected KeyValue, got {:?}", other),
    }
}
