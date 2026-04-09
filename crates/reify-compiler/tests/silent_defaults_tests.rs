//! Tests for silent type defaults and missing diagnostics fixes (task 117).
//!
//! These tests verify that the compiler emits diagnostics instead of silently
//! swallowing errors or using misleading defaults.

use reify_types::Severity;

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("silent_defaults_test"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics.
fn error_diagnostics(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── H2: collection member typo should produce a diagnostic ──────────────

#[test]
fn collection_member_typo_produces_diagnostic() {
    // "diametr" is a typo for "diameter" — the compiler should emit
    // a diagnostic about an unknown member rather than silently defaulting
    // to Type::Real.
    let source = r#"
        structure Bolt {
            param diameter : Scalar = 10mm
        }
        structure Assembly {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[0].diametr
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_unknown_member = errors.iter().any(|d| d.message.contains("unknown member"));
    assert!(
        has_unknown_member,
        "expected diagnostic about 'unknown member', got: {:?}",
        errors
    );
}
// ── M7: compile_field returns direct value ──────────────────────────────

#[test]
fn compile_field_returns_direct_value() {
    // Regression guard: fields should compile successfully and be present
    // in compiled.fields, both before and after the Option removal refactor.
    let source = r#"
        field def temp : Point3 -> Scalar {
            source = analytical { |p| p }
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");
    assert_eq!(module.fields[0].name, "temp");
}

// ── L1: duplicate function signature diagnostic has context ─────────────

#[test]
fn duplicate_function_signature_diagnostic_has_context() {
    // Two functions with the same name and param types should produce a
    // diagnostic that includes the function name and parameter types.
    let source = r#"
        fn add(a: Scalar, b: Scalar) -> Scalar { a + b }
        fn add(a: Scalar, b: Scalar) -> Scalar { a - b }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let dup_error = errors
        .iter()
        .find(|d| d.message.contains("duplicate function signature"));
    assert!(
        dup_error.is_some(),
        "expected 'duplicate function signature' diagnostic, got: {:?}",
        errors
    );
    let msg = &dup_error.unwrap().message;
    assert!(
        msg.contains("add"),
        "diagnostic should mention function name 'add', got: {}",
        msg
    );
    assert!(
        msg.contains("Scalar"),
        "diagnostic should mention parameter type 'Scalar', got: {}",
        msg
    );
}

// ── L6: unlabeled constraint in trait uses Option<String> ────────────────

#[test]
fn unlabeled_constraint_in_trait_uses_option_none() {
    // A trait with an unlabeled constraint should compile its default
    // with `name: None` (not an empty string sentinel).
    let source = r#"
trait Bounded {
    param x : Length
    constraint x > 0mm
}
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Bounded")
        .expect("should have trait Bounded");

    // Find the unlabeled constraint default
    let constraint_default = trait_def
        .defaults
        .iter()
        .find(|d| matches!(d.kind, reify_compiler::DefaultKind::Constraint(_)))
        .expect("trait should have a constraint default");

    assert!(
        constraint_default.name.is_none(),
        "unlabeled constraint should have name: None, got: {:?}",
        constraint_default.name
    );
}

// ── L6 regression: param and let defaults always have Some(name) ──────

#[test]
fn trait_default_param_and_let_always_have_name() {
    // A trait with both param and let defaults should have `name.is_some()`
    // for each Param and Let entry. This is a regression guard confirming
    // the invariant before hardening with .expect() in step-14.
    let source = r#"
trait Configurable {
    param width : Length = 100mm
    param height : Length = 50mm
    let area = width * height
}
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Configurable")
        .expect("should have trait Configurable");

    for default in &trait_def.defaults {
        match &default.kind {
            reify_compiler::DefaultKind::Param { .. } => {
                assert!(
                    default.name.is_some(),
                    "DefaultKind::Param should always have Some(name), got None"
                );
            }
            reify_compiler::DefaultKind::Let(_) => {
                assert!(
                    default.name.is_some(),
                    "DefaultKind::Let should always have Some(name), got None"
                );
            }
            reify_compiler::DefaultKind::Constraint(_) => {
                // Constraints may or may not have names — not checked here
            }
        }
    }
}

// ── H3: geometry call diagnostics ──────────────────────────────────────

#[test]
fn box_wrong_arg_count_produces_preexisting_diagnostic() {
    // box() expects 3 arguments — passing only 2 should produce a diagnostic
    let source = r#"
        structure S {
            let shape = box(10mm, 20mm)
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_arg_count_error = errors
        .iter()
        .any(|d| d.message.contains("expects 3 arguments"));
    assert!(
        has_arg_count_error,
        "expected diagnostic about argument count, got: {:?}",
        errors
    );
}

// ── task-823 step-5: conformance.rs no-type-annotation diagnostics ──────────

#[test]
fn trait_member_no_type_annotation_emits_diagnostic() {
    // A structure implementing a trait where one of the structure's params
    // has no type annotation should produce a diagnostic (conformance.rs:46
    // outer unwrap_or). Currently defaults silently to Type::Real.
    let source = r#"
        trait T {
            param x : Real
        }
        structure S : T {
            param x = 5.0
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_type_annotation_error = errors.iter().any(|d| {
        d.message.contains("no type annotation")
            || d.message.contains("missing type annotation")
            || d.message.contains("cannot infer type")
    });
    assert!(
        has_type_annotation_error,
        "expected diagnostic about missing type annotation for conformance, got: {:?}",
        errors
    );
}

#[test]
fn trait_let_no_type_annotation_emits_diagnostic() {
    // A structure implementing a trait where a let declaration has no type
    // annotation should produce a diagnostic (conformance.rs:73 outer
    // unwrap_or). Currently defaults silently to Type::Real.
    let source = r#"
        trait T {
            param x : Real
        }
        structure S : T {
            param x : Real = 5.0
            let y = x * 2.0
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_type_annotation_error = errors.iter().any(|d| {
        d.message.contains("no type annotation")
            || d.message.contains("missing type annotation")
            || d.message.contains("cannot infer type")
    });
    assert!(
        has_type_annotation_error,
        "expected diagnostic about missing type annotation for let in conformance, got: {:?}",
        errors
    );
}

// ── task-823 step-3: entity.rs ICE paths — green path (no ICE on valid code) ──

#[test]
fn structure_param_resolves_without_ice() {
    // Verifies that entity.rs:525 (scope.resolve in pass 2 for structure param)
    // does NOT emit an ICE diagnostic for valid code. The two-pass compilation
    // registers all names in pass 1, so pass-2 resolve should always succeed
    // for well-formed structures.
    let source = r#"
        structure S {
            param x : Real = 1.0
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_ice = errors.iter().any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid structure param, got: {:?}",
        errors
    );
    assert!(errors.is_empty(), "expected no errors at all, got: {:?}", errors);
}

#[test]
fn port_member_resolves_without_ice() {
    // Verifies that entity.rs:856 (scope.resolve in pass 2 for port member param)
    // does NOT emit an ICE diagnostic for valid code.
    let source = r#"
        trait MechPort {
            param diameter : Length
        }
        structure S {
            port mount : MechPort {
                param diameter : Length = 5mm
            }
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_ice = errors.iter().any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid port member, got: {:?}",
        errors
    );
    assert!(errors.is_empty(), "expected no errors at all, got: {:?}", errors);
}

// ── task-823 step-1: port param unknown type name emits diagnostic ──────

#[test]
fn port_param_unknown_type_name_emits_error() {
    // A port param whose type name doesn't exist (Nonexistent) should produce
    // an error diagnostic. Previously, entity.rs:366 silently defaulted to
    // Type::Real via unwrap_or without any diagnostic.
    let source = r#"
        trait MechPort {
            param diameter : Length
        }
        structure S {
            port mount : MechPort {
                param diameter : Nonexistent
            }
        }
    "#;
    let module = compile_module(source);
    let errors = error_diagnostics(&module);

    let has_type_error = errors.iter().any(|d| {
        d.message.contains("Nonexistent") || d.message.contains("unresolved type name")
    });
    assert!(
        has_type_error,
        "expected diagnostic about unknown type 'Nonexistent' in port param, got: {:?}",
        errors
    );
}
