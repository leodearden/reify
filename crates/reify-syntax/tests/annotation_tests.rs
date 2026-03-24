//! Annotation parsing tests.
//!
//! Tests for `@ident` and `@ident(args)` annotation syntax at top-level declarations.

use reify_syntax::*;

/// Helper: parse source and return the ParsedModule.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_types::ModulePath::single("annotation_test"))
}

// ── Step 1/2: bare annotation on structure ───────────────────────────────────

#[test]
fn parse_bare_annotation_on_structure() {
    let source = "@test structure S { param x: Real }";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);
    assert_eq!(
        module.declarations.len(),
        1,
        "expected 1 declaration, got {:?}",
        module.declarations
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "test");
    assert!(
        s.annotations[0].args.is_empty(),
        "expected no args, got {:?}",
        s.annotations[0].args
    );
}

// ── Step 5/6: annotation with identifier arg ─────────────────────────────────

#[test]
fn parse_annotation_with_identifier_arg() {
    let source = "@category(mechanical) structure S {}";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "category");
    assert_eq!(
        s.annotations[0].args.len(),
        1,
        "expected 1 arg, got {:?}",
        s.annotations[0].args
    );
    match &s.annotations[0].args[0].kind {
        ExprKind::Ident(name) => assert_eq!(name, "mechanical"),
        other => panic!("expected Ident(\"mechanical\"), got {:?}", other),
    }
}

// ── Step 3/4: annotation with string literal arg ──────────────────────────────

#[test]
fn parse_annotation_with_string_arg() {
    let source = r#"@deprecated("use NewS") structure S {}"#;
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "deprecated");
    assert_eq!(
        s.annotations[0].args.len(),
        1,
        "expected 1 arg, got {:?}",
        s.annotations[0].args
    );
    match &s.annotations[0].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "use NewS"),
        other => panic!("expected StringLiteral(\"use NewS\"), got {:?}", other),
    }
}
