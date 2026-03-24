//! Tests for entity definition overload ban (Task 172).
//!
//! Spec §4.2.1: two entity definitions (structures, occurrences, constraints,
//! fields) sharing the same name are a compile error, regardless of type parameters.
//! A unified `seen_entity_names` tracker in the pre-pass detects all cases and
//! emits a two-label diagnostic pointing at both definitions.

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("entity_overload_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics.
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect()
}

// ── step-1: duplicate structure names ──────────────────────────────────────

/// Two structures with the same name produce a 'duplicate entity definition'
/// error diagnostic with two labels. Only one template is compiled.
#[test]
fn duplicate_structure_names_produce_error() {
    let source = r#"
structure Bracket {
    param width : Real = 10.0
}

structure Bracket {
    param height : Real = 20.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    // Exactly one duplicate-entity error
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for duplicate structure, got: {:?}",
        errors
    );

    let msg = &errors[0].message;
    assert!(
        msg.contains("duplicate entity definition") && msg.contains("Bracket"),
        "error should say 'duplicate entity definition' for 'Bracket', got: {:?}",
        msg
    );

    // Two labels pointing at both definitions
    assert_eq!(
        errors[0].labels.len(),
        2,
        "expected 2 labels (duplicate + first), got {:?}",
        errors[0].labels
    );

    // Only 1 template compiled (the first definition wins, duplicate is skipped)
    assert_eq!(
        module.templates.len(),
        1,
        "expected only 1 compiled template, got {}",
        module.templates.len()
    );
}
