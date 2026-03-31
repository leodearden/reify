//! Tests for type alias registry and resolution (task 145).
//!
//! Validates TypeAliasEntry, TypeAliasRegistry, alias compilation in the pre-pass,
//! dimensional aliases, transitive resolution, cycle detection, parameterized aliases,
//! and integration with existing type resolution paths.

use reify_compiler::{compile, CompiledModule, TypeAliasEntry, TypeAliasRegistry};
use reify_types::{ContentHash, ModulePath, Severity, SourceSpan, Type};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("alias_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    compile(&parsed)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ─── step-1: TypeAliasEntry and TypeAliasRegistry data structures ────────────

#[test]
fn type_alias_entry_fields_exist() {
    let dummy_span = SourceSpan::new(0, 0);
    let hash = ContentHash::of_str("Pressure");
    let entry = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Scalar {
            dimension: reify_types::DimensionVector::LENGTH,
        }),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: dummy_span,
        content_hash: hash,
    };
    assert_eq!(entry.name, "Pressure");
    assert!(entry.resolved_type.is_some());
    assert!(entry.type_params.is_empty());
    assert!(entry.type_expr.is_none());
    assert!(entry.is_pub);
}

#[test]
fn type_alias_registry_new_and_lookup_empty() {
    let reg = TypeAliasRegistry::new();
    assert!(reg.lookup("Pressure").is_none());
    assert!(reg.lookup("Velocity").is_none());
}

#[test]
fn type_alias_registry_register_and_lookup() {
    let mut reg = TypeAliasRegistry::new();
    let entry = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Real),
        type_params: vec![],
        type_expr: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Pressure"),
    };
    assert!(reg.register(entry).is_ok());
    let looked_up = reg.lookup("Pressure");
    assert!(looked_up.is_some());
    assert_eq!(looked_up.unwrap().name, "Pressure");
}

#[test]
fn type_alias_registry_duplicate_register_returns_err() {
    let mut reg = TypeAliasRegistry::new();
    let entry1 = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Real),
        type_params: vec![],
        type_expr: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Pressure"),
    };
    let entry2 = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Int),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: SourceSpan::new(10, 15),
        content_hash: ContentHash::of_str("Pressure2"),
    };
    assert!(reg.register(entry1).is_ok());
    assert!(reg.register(entry2).is_err());
}
