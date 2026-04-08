//! Deprecated-on-use compilation tests.
//!
//! Tests that the compiler emits Warning diagnostics when a deprecated entity
//! is referenced at a use-site (not at its definition site).

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("deprecated_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics (ignoring warnings).
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect()
}

/// Helper: return only warning-severity diagnostics.
fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Warning)
        .collect()
}

/// Helper: filter warnings whose message contains the given substring.
fn deprecation_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains("deprecated") && d.message.contains(substr))
        .collect()
}

// ── Step 1: sub-component reference to deprecated structure ─────────────────

#[test]
fn deprecated_structure_used_as_sub_emits_warning() {
    let source = r#"
        @deprecated("Use NewBolt")
        structure OldBolt { param d : Real = 1.0 }

        structure Assembly {
            sub b = OldBolt()
        }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldBolt");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for OldBolt, got warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns[0].message.contains("Use NewBolt"),
        "expected warning to mention 'Use NewBolt', got: {}",
        warns[0].message
    );
}

#[test]
fn deprecated_structure_with_no_message_sub_emits_generic_warning() {
    let source = r#"
        @deprecated
        structure OldPart { param w : Real = 2.0 }

        structure Assembly {
            sub p = OldPart()
        }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldPart");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for OldPart, got warnings: {:?}",
        warnings_only(&module)
    );
    // No message suffix when no arg
    assert!(
        !warns[0].message.contains(": "),
        "expected no trailing ': ', got: {}",
        warns[0].message
    );
}

// ── Step 3: deprecated function call emits warning ──────────────────────────

#[test]
fn deprecated_function_called_emits_warning() {
    let source = r#"
        @deprecated("Use new_calc")
        fn old_calc(x: Real) -> Real { x }

        structure S {
            param x : Real = 1.0
            let y = old_calc(x)
        }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "old_calc");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for old_calc, got warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns[0].message.contains("Use new_calc"),
        "expected warning to mention 'Use new_calc', got: {}",
        warns[0].message
    );
}

// ── Step 5: deprecated trait used as trait bound emits warning ──────────────

#[test]
fn deprecated_trait_used_as_trait_bound_emits_warning() {
    let source = r#"
        @deprecated("Use NewTrait")
        trait OldTrait { param w : Real }

        structure S : OldTrait { param w : Real = 1.0 }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldTrait");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for OldTrait, got warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns[0].message.contains("Use NewTrait"),
        "expected warning to mention 'Use NewTrait', got: {}",
        warns[0].message
    );
}

// ── Step 7: deprecated trait used as trait refinement emits warning ─────────

#[test]
fn deprecated_trait_used_as_refinement_emits_warning() {
    let source = r#"
        @deprecated
        trait Base { param x : Real }

        trait Derived : Base { param y : Real }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "Base");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for Base, got warnings: {:?}",
        warnings_only(&module)
    );
}

// ── Step 9: deprecated structure used as purpose parameter emits warning ─────

#[test]
fn deprecated_structure_used_as_purpose_param_emits_warning() {
    let source = r#"
        @deprecated("Use NewS")
        structure OldS { param x : Real = 1.0 }

        purpose P(subject : OldS) {
            constraint subject.x > 0.0
        }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldS");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for OldS, got warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns[0].message.contains("Use NewS"),
        "expected warning to mention 'Use NewS', got: {}",
        warns[0].message
    );
}

// ── Step 11: edge cases ──────────────────────────────────────────────────────

#[test]
fn defining_deprecated_entity_without_using_produces_no_use_warning() {
    // Just defining a deprecated entity — no use-site — should produce NO deprecation-use warning
    let source = r#"
        @deprecated("Old")
        structure OldBolt { param d : Real = 1.0 }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    // The structure defines @deprecated but is not used — no use-site warning expected
    let warns = deprecation_warnings(&module, "OldBolt");
    assert!(
        warns.is_empty(),
        "expected NO deprecation warning for unused OldBolt, got: {:?}",
        warns
    );
}

#[test]
fn multiple_uses_of_deprecated_structure_produce_multiple_warnings() {
    let source = r#"
        @deprecated("Old")
        structure OldBolt { param d : Real = 1.0 }

        structure Assembly1 { sub b1 = OldBolt() }
        structure Assembly2 { sub b2 = OldBolt() }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldBolt");
    assert_eq!(
        warns.len(),
        2,
        "expected 2 deprecation warnings for OldBolt, got: {:?}",
        warns
    );
}

#[test]
fn deprecated_no_args_produces_warning_without_trailing_colon() {
    let source = r#"
        @deprecated
        fn old_fn(x: Real) -> Real { x }

        structure S {
            param x : Real = 1.0
            let y = old_fn(x)
        }
    "#;
    let module = compile_module(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "old_fn");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for old_fn, got: {:?}",
        warnings_only(&module)
    );
    // Should not have a trailing ': ' when no message
    assert!(
        !warns[0].message.ends_with(": "),
        "expected no trailing ': ', got: {}",
        warns[0].message
    );
    assert!(
        !warns[0].message.contains(": "),
        "expected no colon separator when no message, got: {}",
        warns[0].message
    );
}

#[test]
fn annotation_compile_tests_no_regression() {
    // Ensure that compiling a @deprecated annotation on a struct without using it
    // does NOT produce a use-site warning (regression guard for annotation_compile_tests).
    let module = compile_module(
        r#"@deprecated("old") structure S { param x : Real }"#,
    );
    let errors: Vec<_> = errors_only(&module);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    // Only annotation-context validation warnings expected (none for @deprecated on structure)
    let dep_use_warns = deprecation_warnings(&module, "S");
    assert!(
        dep_use_warns.is_empty(),
        "unexpected use-site warning for S (just defined, not used): {:?}",
        dep_use_warns
    );
}
