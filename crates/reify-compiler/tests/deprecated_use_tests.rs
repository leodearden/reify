//! Deprecated-on-use compilation tests.
//!
//! Tests that the compiler emits Warning diagnostics when a deprecated entity
//! is referenced at a use-site (not at its definition site).

use reify_test_support::{compile_source, errors_only, warnings_only};

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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldBolt");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one deprecation warning for OldBolt, got: {:?}",
        warns
    );
    assert!(
        warns[0].message.contains("Use NewBolt"),
        "expected warning to mention 'Use NewBolt', got: {}",
        warns[0].message
    );

    // Span must point at the use-site, not the definition.
    let label = warns[0]
        .labels
        .first()
        .expect("expected at least one diagnostic label");
    assert!(
        !label.span.is_empty(),
        "expected non-empty span in deprecation label, got: {:?}",
        label.span
    );
    let use_site_offset = source
        .find("sub b")
        .expect("test source must contain 'sub b'") as u32;
    assert!(
        label.span.start >= use_site_offset,
        "expected span.start ({}) >= use-site offset ({}); span is inside definition, not use-site",
        label.span.start,
        use_site_offset
    );
    assert!(
        (label.span.end as usize) <= source.len(),
        "expected span.end ({}) <= source.len() ({})",
        label.span.end,
        source.len()
    );
    let span_text = &source[label.span.start as usize..label.span.end as usize];
    assert!(
        span_text.contains("OldBolt"),
        "expected span text to contain 'OldBolt', got: {:?}",
        span_text
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
    let module = compile_source(source);
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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "old_calc");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one deprecation warning for old_calc, got: {:?}",
        warns
    );
    assert!(
        warns[0].message.contains("Use new_calc"),
        "expected warning to mention 'Use new_calc', got: {}",
        warns[0].message
    );
}

// ── Step 3b: deprecated function called via default-padding emits warning ────

#[test]
fn deprecated_function_called_via_default_padding_emits_warning() {
    // A zero-arg call to a fn with one defaulted param forces OverloadResolution::NoMatch →
    // try_default_padding, which historically skipped the deprecation check.
    let source = r#"
        @deprecated("Use new_calc")
        fn old_calc(x: Real = 1.0) -> Real { x }

        structure S {
            let y = old_calc()
        }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "old_calc");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one deprecation warning for old_calc (via default-padding), got: {:?}",
        warns
    );
    assert!(
        warns[0].message.contains("Use new_calc"),
        "expected warning to mention 'Use new_calc', got: {}",
        warns[0].message
    );
    // Format parity with the Resolved arm (deprecation_warning_message_format_contract).
    assert_eq!(
        &warns[0].message,
        "use of deprecated function 'old_calc': Use new_calc",
        "message format must match the explicit-call path"
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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldTrait");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one deprecation warning for OldTrait, got: {:?}",
        warns
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
    let module = compile_source(source);
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

// ── Step 8: deprecation label highlights only the refinement identifier ────

#[test]
fn deprecated_trait_used_as_refinement_warning_points_at_refinement() {
    // Same fixture as deprecated_trait_used_as_refinement_emits_warning above.
    let source =
        "@deprecated\ntrait Base { param x : Real }\n\ntrait Derived : Base { param y : Real }";
    let module = compile_source(source);
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

    let labels = &warns[0].labels;
    assert!(
        !labels.is_empty(),
        "expected at least one label on deprecation warning"
    );
    let s = labels[0].span;
    assert_eq!(
        &source[s.start as usize..s.end as usize],
        "Base",
        "deprecation label should highlight exactly the 'Base' refinement identifier"
    );
    assert_eq!(
        s.end - s.start,
        4,
        "span length should be exactly 4 (len of 'Base'), not the whole declaration"
    );
}

// ── Step 9: deprecated structure used as purpose parameter emits warning ─────

#[test]
fn deprecated_structure_used_as_purpose_param_emits_warning() {
    let source = r#"
        @deprecated("Use NewS")
        structure OldS { param x : Real = 1.0 }

        purpose P(subject : OldS) {
            constraint 1 > 0
        }
    "#;
    let module = compile_source(source);
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

// ── Step 10: deprecated occurrence used as purpose parameter emits warning ────

#[test]
fn deprecated_occurrence_used_as_purpose_param_emits_warning() {
    let source = r#"
        @deprecated("Use NewOcc")
        occurrence def OldOcc { param m : Length = 10mm }

        purpose P(subject : OldOcc) {
            constraint 1 > 0
        }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldOcc");
    assert!(
        !warns.is_empty(),
        "expected deprecation warning for OldOcc, got warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns[0].message.contains("Use NewOcc"),
        "expected warning to mention 'Use NewOcc', got: {}",
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
    let module = compile_source(source);
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
    let module = compile_source(source);
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
    let module = compile_source(source);
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
    let module = compile_source(r#"@deprecated("old") structure S { param x : Real }"#);
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

// ── Definition-only edge cases (fn, trait) ────────────────────────────────

#[test]
fn deprecated_fn_and_trait_definition_alone_produce_no_warning() {
    // deprecated fn — no call-site
    let module = compile_source(r#"@deprecated("old") fn only_fn(x: Real) -> Real { x }"#);
    assert!(
        errors_only(&module).is_empty(),
        "fn: errors: {:?}",
        errors_only(&module)
    );
    assert!(
        deprecation_warnings(&module, "only_fn").is_empty(),
        "fn: expected no deprecation warning for definition-only, got: {:?}",
        deprecation_warnings(&module, "only_fn")
    );

    // deprecated trait — no implementor
    let module = compile_source(r#"@deprecated("old") trait OnlyTrait { param w : Real }"#);
    assert!(
        errors_only(&module).is_empty(),
        "trait: errors: {:?}",
        errors_only(&module)
    );
    assert!(
        deprecation_warnings(&module, "OnlyTrait").is_empty(),
        "trait: expected no deprecation warning for definition-only, got: {:?}",
        deprecation_warnings(&module, "OnlyTrait")
    );
}

// ── Message format contract ───────────────────────────────────────────────

#[test]
fn deprecation_warning_message_format_contract() {
    let source = r#"
        @deprecated("Use NewBolt version 2")
        structure OldBolt { param d : Real = 1.0 }

        structure Assembly {
            sub b = OldBolt()
        }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns = deprecation_warnings(&module, "OldBolt");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one deprecation warning, got: {:?}",
        warns
    );

    // Full format assertion — locks the diagnostic format as a stable contract.
    assert_eq!(
        &warns[0].message, "use of deprecated structure 'OldBolt': Use NewBolt version 2",
        "message format mismatch"
    );
}

// ── Multi-refinement span precision ──────────────────────────────────────────

#[test]
fn deprecated_trait_multi_refinement_each_warning_points_at_own_identifier() {
    // Two deprecated parents; the label for each warning must highlight its own
    // identifier, not the whole `trait D : B + C` declaration.  A regression to
    // the old `trait_decl.span` behaviour would pass the single-refinement test
    // but would produce identical (wrong) spans here.
    let source = "@deprecated\ntrait B { param x : Real }\n@deprecated\ntrait C { param y : Real }\ntrait D : B + C { param z : Real }";
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let warns_b = deprecation_warnings(&module, "B");
    assert!(
        !warns_b.is_empty(),
        "expected deprecation warning for B, got: {:?}",
        warnings_only(&module)
    );
    assert!(
        !warns_b[0].labels.is_empty(),
        "expected at least one label on B warning"
    );
    let s_b = warns_b[0].labels[0].span;
    assert_eq!(
        &source[s_b.start as usize..s_b.end as usize],
        "B",
        "B warning label should highlight exactly 'B'"
    );

    let warns_c = deprecation_warnings(&module, "C");
    assert!(
        !warns_c.is_empty(),
        "expected deprecation warning for C, got: {:?}",
        warnings_only(&module)
    );
    assert!(
        !warns_c[0].labels.is_empty(),
        "expected at least one label on C warning"
    );
    let s_c = warns_c[0].labels[0].span;
    assert_eq!(
        &source[s_c.start as usize..s_c.end as usize],
        "C",
        "C warning label should highlight exactly 'C'"
    );

    // The two spans must be distinct — they point at different tokens.
    assert_ne!(
        s_b, s_c,
        "B and C warning spans should be distinct (different tokens in the refinement list)"
    );
}
