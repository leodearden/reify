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

// ── step-7: cross-type collision: field + structure same name ─────────────

/// A field def followed by a structure with the same name produces a
/// 'duplicate entity definition' error. The first-declared entity wins.
#[test]
fn field_and_structure_same_name_produce_error() {
    let source = r#"
field def Sensor : Real -> Real { source = analytical { |x| x } }

structure Sensor {
    param value : Real = 0.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    // Should have at least one duplicate-entity error (may have other warnings)
    let dup_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("duplicate entity definition") && d.message.contains("Sensor"))
        .collect();
    assert_eq!(
        dup_errors.len(),
        1,
        "expected exactly 1 duplicate-entity error for 'Sensor', got errors: {:?}",
        errors
    );

    // First-declared entity (field) wins: no template should be compiled for Sensor
    assert_eq!(
        module.templates.len(),
        0,
        "expected 0 compiled templates (structure 'Sensor' is a duplicate), got {}",
        module.templates.len()
    );
    // The field should still be compiled
    assert_eq!(
        module.fields.len(),
        1,
        "expected the field 'Sensor' to be compiled, got {}",
        module.fields.len()
    );
}

// ── step-9: cross-type collision: constraint + structure same name ─────────

/// A constraint def followed by a structure with the same name produces a
/// 'duplicate entity definition' error. Constraints reserve names in the
/// entity namespace even though constraint compilation is not yet implemented.
#[test]
fn constraint_and_structure_same_name_produce_error() {
    let source = r#"
constraint def Shape { x > 0 }

structure Shape {
    param side : Real = 1.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    let dup_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("duplicate entity definition") && d.message.contains("Shape"))
        .collect();
    assert_eq!(
        dup_errors.len(),
        1,
        "expected exactly 1 duplicate-entity error for 'Shape', got errors: {:?}",
        errors
    );

    assert_eq!(
        dup_errors[0].labels.len(),
        2,
        "expected 2 labels (duplicate + first), got {:?}",
        dup_errors[0].labels
    );

    // The structure 'Shape' is a duplicate — should not be compiled
    assert_eq!(
        module.templates.len(),
        0,
        "expected 0 compiled templates (structure 'Shape' is a duplicate of constraint 'Shape'), got {}",
        module.templates.len()
    );
}

// ── step-11: type parameters do NOT distinguish entity names ──────────────

/// Two structures with the same name but different type parameters still
/// produce a 'duplicate entity definition' error. Spec §4.2.1: entity overloading
/// is forbidden regardless of type parameters (name-only comparison).
#[test]
fn duplicate_structure_names_with_different_type_params_produce_error() {
    let source = r#"
structure Box<T> {
    param value : Real = 0.0
}

structure Box<T, U> {
    param width : Real = 1.0
    param height : Real = 2.0
}
"#;
    let module = compile_module(source);
    let errors = errors_only(&module);

    let dup_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("duplicate entity definition") && d.message.contains("Box"))
        .collect();
    assert_eq!(
        dup_errors.len(),
        1,
        "expected exactly 1 duplicate-entity error for 'Box', got errors: {:?}",
        errors
    );

    // Only 1 template compiled (first definition wins regardless of type params)
    assert_eq!(
        module.templates.len(),
        1,
        "expected only 1 compiled template, got {}",
        module.templates.len()
    );
}
