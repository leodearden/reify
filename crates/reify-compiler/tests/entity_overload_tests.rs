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

// ── step-1/3: duplicate entity names ─────────────────────────────────────

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

// ── step-3: duplicate occurrence names ────────────────────────────────────

/// Two occurrences with the same name produce a 'duplicate entity definition'
/// error diagnostic with two labels. Only one template is compiled.
#[test]
fn duplicate_occurrence_names_produce_error() {
    let source = r#"
occurrence Weld {
    param duration : Real = 5.0
}

occurrence Weld {
    param energy : Real = 100.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for duplicate occurrence, got: {:?}",
        errors
    );

    let msg = &errors[0].message;
    assert!(
        msg.contains("duplicate entity definition") && msg.contains("Weld"),
        "error should say 'duplicate entity definition' for 'Weld', got: {:?}",
        msg
    );

    assert_eq!(
        errors[0].labels.len(),
        2,
        "expected 2 labels (duplicate + first), got {:?}",
        errors[0].labels
    );

    // Only 1 template compiled (first definition wins)
    assert_eq!(
        module.templates.len(),
        1,
        "expected only 1 compiled template, got {}",
        module.templates.len()
    );
}

// ── step-5: cross-type collision: structure + occurrence same name ─────────

/// A structure and an occurrence with the same name produce a
/// 'duplicate entity definition' error. Labels identify the entity kinds.
/// Only 1 template is compiled.
#[test]
fn structure_and_occurrence_same_name_produce_error() {
    let source = r#"
structure Widget {
    param size : Real = 1.0
}

occurrence Widget {
    param duration : Real = 5.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for structure+occurrence collision, got: {:?}",
        errors
    );

    let msg = &errors[0].message;
    assert!(
        msg.contains("duplicate entity definition") && msg.contains("Widget"),
        "error should say 'duplicate entity definition' for 'Widget', got: {:?}",
        msg
    );

    // Labels should identify both entity kinds
    assert_eq!(
        errors[0].labels.len(),
        2,
        "expected 2 labels, got {:?}",
        errors[0].labels
    );
    let label_msgs: Vec<&str> = errors[0].labels.iter().map(|l| l.message.as_str()).collect();
    // One label should mention "occurrence", the other "structure"
    assert!(
        label_msgs.iter().any(|m| m.contains("occurrence")),
        "one label should mention 'occurrence', got: {:?}",
        label_msgs
    );
    assert!(
        label_msgs.iter().any(|m| m.contains("structure")),
        "one label should mention 'structure', got: {:?}",
        label_msgs
    );

    // Only 1 template compiled (the structure, which was defined first)
    assert_eq!(
        module.templates.len(),
        1,
        "expected only 1 compiled template, got {}",
        module.templates.len()
    );
}
