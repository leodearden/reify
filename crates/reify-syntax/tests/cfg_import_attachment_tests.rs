//! Tests for positional `#cfg(...)` attachment to import declarations.
//!
//! A `#cfg(...)` pragma immediately preceding an `import` is attached to
//! `ImportDecl.cfg_predicates`; a pragma preceding a non-import declaration
//! (or at EOF) is NOT attached (and will later produce W_CFG_NO_IMPORT).

use reify_ast::*;

/// Helper: parse source and return the ParsedModule.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_core::ModulePath::single("cfg_attach_test"))
}

// ── S1: happy-path attachment ────────────────────────────────────────────────

/// A single `#cfg(linux)` immediately before an import attaches one predicate.
#[test]
fn cfg_before_import_attaches_one_predicate() {
    let source = "#cfg(linux)\nimport a.b";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let import = match &module.declarations[0] {
        Declaration::Import(i) => i,
        other => panic!("expected Import, got {:?}", other),
    };

    assert_eq!(
        import.cfg_predicates.len(),
        1,
        "expected 1 cfg_predicate, got {:?}",
        import.cfg_predicates
    );
    let pred = &import.cfg_predicates[0];
    assert_eq!(pred.name, "cfg");
    assert_eq!(pred.args.len(), 1, "expected 1 arg, got {:?}", pred.args);
    match &pred.args[0] {
        PragmaArg::Bare(PragmaValue::Ident(s)) => {
            assert_eq!(s, "linux", "expected ident 'linux', got '{}'", s);
        }
        other => panic!("expected Bare(Ident(\"linux\")), got {:?}", other),
    }
}

/// Two stacked `#cfg` pragmas before an import produce two predicates in source order.
#[test]
fn stacked_cfg_before_import_attaches_two_predicates() {
    let source = "#cfg(linux)\n#cfg(target = \"wasm\")\nimport a.b";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let import = match &module.declarations[0] {
        Declaration::Import(i) => i,
        other => panic!("expected Import, got {:?}", other),
    };

    assert_eq!(
        import.cfg_predicates.len(),
        2,
        "expected 2 cfg_predicates in source order, got {:?}",
        import.cfg_predicates
    );
    assert_eq!(import.cfg_predicates[0].name, "cfg");
    assert_eq!(import.cfg_predicates[1].name, "cfg");

    // First: bare ident "linux"
    match &import.cfg_predicates[0].args[0] {
        PragmaArg::Bare(PragmaValue::Ident(s)) => assert_eq!(s, "linux"),
        other => panic!("expected Bare(Ident(\"linux\")), got {:?}", other),
    }
    // Second: key-value target="wasm" (string literal → PragmaValue::String)
    match &import.cfg_predicates[1].args[0] {
        PragmaArg::KeyValue { key, value: PragmaValue::String(v) } => {
            assert_eq!(key, "target");
            assert_eq!(v, "wasm");
        }
        other => panic!("expected KeyValue{{target, String(\"wasm\")}}, got {:?}", other),
    }
}

/// A non-cfg pragma (`#version`) before an import does NOT populate cfg_predicates.
#[test]
fn non_cfg_pragma_before_import_leaves_cfg_predicates_empty() {
    let source = "#version(0.1)\nimport a.b";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let import = match &module.declarations[0] {
        Declaration::Import(i) => i,
        other => panic!("expected Import, got {:?}", other),
    };

    assert!(
        import.cfg_predicates.is_empty(),
        "expected empty cfg_predicates for non-cfg pragma, got {:?}",
        import.cfg_predicates
    );
}

