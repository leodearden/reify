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

// ── S3: no-leak guard ────────────────────────────────────────────────────────

/// A `#cfg` before a structure must NOT leak forward to a later import.
///
/// With S2's minimal impl (pending_cfg not cleared on non-import arms), the
/// cfg would incorrectly carry over to the `import a.b` — this test catches that.
#[test]
fn cfg_before_structure_does_not_leak_to_later_import() {
    let source = "#cfg(linux)\nstructure S { param x: Real }\nimport a.b";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let import = module
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Import(i) => Some(i),
            _ => None,
        })
        .expect("expected an Import declaration");

    assert!(
        import.cfg_predicates.is_empty(),
        "cfg before a structure must not leak to a later import, got {:?}",
        import.cfg_predicates
    );
}

/// A `#cfg` before a refused (malformed) import is consumed by that import, not carried
/// forward to gate the next, well-formed one.
#[test]
fn cfg_before_a_refused_import_does_not_leak_to_the_next_import() {
    let source = "#cfg(linux)\nimport a.b.{C D}\nimport c.d";
    let module = parse_module(source);
    assert!(
        module
            .errors
            .iter()
            .any(|e| e.message.starts_with("invalid import: ")),
        "`{source}`: expected the malformed import to be refused, got errors: {:?}",
        module.errors
    );

    let imports: Vec<&ImportDecl> = module
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Import(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(
        imports.len(),
        1,
        "`{source}`: expected only the well-formed import, got declarations: {:?}",
        module.declarations
    );
    assert_eq!(imports[0].path, "c.d", "`{source}`");
    assert!(
        imports[0].cfg_predicates.is_empty(),
        "`{source}`: the #cfg before a refused import must not leak to the next import, got {:?}",
        imports[0].cfg_predicates
    );
}
