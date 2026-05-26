//! Integration parity test: `annotation_schema_registry_parity`
//!
//! Verifies that `validate_annotations` (which post-refactor routes through
//! `schema::validate_via_schema`) produces diagnostics byte-identical to
//! the legacy per-annotation match-arm for every annotation × context ×
//! arg-shape edge case combination.
//!
//! The fixture matrix is inline Rust data. Each row specifies:
//! - `label`:         human-readable identifier for the case
//! - `name`:          annotation name string
//! - `context`:       context string passed to `validate_annotations`
//! - `args`:          annotation argument list
//! - `expected`:      expected diagnostics (may be empty)
//!
//! The shim `reify_compiler::__validate_annotations_for_parity_test` is
//! feature-gated under `test-support`; this integration test target opts in via
//! the self-pull `reify-compiler = { path = ".", features = ["test-support"] }`
//! in `[dev-dependencies]`.

#![cfg(feature = "test-support")]

/// Shape of an expected diagnostic in the parity test matrix.
#[derive(Debug)]
struct ExpectedDiag {
    /// Substring that must appear in `diagnostic.message`.
    message_substr: &'static str,
    /// Substring that must appear in `diagnostic.labels[0].message`.
    label_substr: &'static str,
}

impl ExpectedDiag {
    fn new(message_substr: &'static str, label_substr: &'static str) -> Self {
        Self {
            message_substr,
            label_substr,
        }
    }
}

/// One row of the parity fixture matrix.
struct FixtureRow {
    label: &'static str,
    name: &'static str,
    context: &'static str,
    args: Vec<reify_types::AnnotationArgValue>,
    expected: Vec<ExpectedDiag>,
}

impl FixtureRow {
    fn new(
        label: &'static str,
        name: &'static str,
        context: &'static str,
        args: Vec<reify_types::AnnotationArgValue>,
        expected: Vec<ExpectedDiag>,
    ) -> Self {
        Self {
            label,
            name,
            context,
            args,
            expected,
        }
    }

    fn ok(
        label: &'static str,
        name: &'static str,
        context: &'static str,
        args: Vec<reify_types::AnnotationArgValue>,
    ) -> Self {
        Self::new(label, name, context, args, vec![])
    }
}

#[test]
fn annotation_schema_registry_parity() {
    use reify_types::AnnotationArgValue::{Bool, Ident, Int, Real, String as Str};

    #[rustfmt::skip]
    let rows: Vec<FixtureRow> = vec![
        // ── @test: valid contexts ────────────────────────────────────────
        FixtureRow::ok("test/structure", "test", "structure", vec![]),
        FixtureRow::ok("test/occurrence", "test", "occurrence", vec![]),
        FixtureRow::ok("test/function", "test", "function", vec![]),
        FixtureRow::ok("test/constraint_def", "test", "constraint_def", vec![]),

        // @test: invalid contexts
        FixtureRow::new("test/param-invalid", "test", "param", vec![],
            vec![ExpectedDiag::new("@test is not valid on param declarations", "@test")]),
        FixtureRow::new("test/let-invalid", "test", "let", vec![],
            vec![ExpectedDiag::new("@test is not valid on let declarations", "@test")]),
        FixtureRow::new("test/trait-invalid", "test", "trait", vec![],
            vec![ExpectedDiag::new("@test is not valid on trait declarations", "@test")]),
        FixtureRow::new("test/purpose-invalid", "test", "purpose", vec![],
            vec![ExpectedDiag::new("@test is not valid on purpose declarations", "@test")]),
        FixtureRow::new("test/field-invalid", "test", "field", vec![],
            vec![ExpectedDiag::new("@test is not valid on field declarations", "@test")]),

        // ── @deprecated: valid in every known context ────────────────────
        FixtureRow::ok("deprecated/structure", "deprecated", "structure", vec![]),
        FixtureRow::ok("deprecated/occurrence", "deprecated", "occurrence", vec![]),
        FixtureRow::ok("deprecated/function", "deprecated", "function", vec![]),
        FixtureRow::ok("deprecated/constraint_def", "deprecated", "constraint_def", vec![]),
        FixtureRow::ok("deprecated/trait", "deprecated", "trait", vec![]),
        FixtureRow::ok("deprecated/purpose", "deprecated", "purpose", vec![]),
        FixtureRow::ok("deprecated/param", "deprecated", "param", vec![]),
        FixtureRow::ok("deprecated/let", "deprecated", "let", vec![]),
        FixtureRow::ok("deprecated/field", "deprecated", "field", vec![]),

        // ── @optimized: valid contexts, no args → 0 diags ───────────────
        FixtureRow::ok("optimized/structure-no-args", "optimized", "structure", vec![]),
        FixtureRow::ok("optimized/occurrence-no-args", "optimized", "occurrence", vec![]),

        // @optimized: valid contexts with string arg → 0 diags
        FixtureRow::ok("optimized/structure-string-arg", "optimized", "structure",
            vec![Str("k::f".to_string())]),
        FixtureRow::ok("optimized/occurrence-string-arg", "optimized", "occurrence",
            vec![Str("k::f".to_string())]),
        FixtureRow::ok("optimized/constraint_def-string-arg", "optimized", "constraint_def",
            vec![Str("k::f".to_string())]),
        FixtureRow::ok("optimized/function-string-arg", "optimized", "function",
            vec![Str("k::f".to_string())]),

        // @optimized: constraint_def/function with no args → missing-target warning
        FixtureRow::new("optimized/constraint_def-no-args", "optimized", "constraint_def", vec![],
            vec![ExpectedDiag::new("requires a string literal target", "@optimized missing target")]),
        FixtureRow::new("optimized/function-no-args", "optimized", "function", vec![],
            vec![ExpectedDiag::new("requires a string literal target", "@optimized missing target")]),

        // @optimized: invalid contexts
        FixtureRow::new("optimized/param-invalid", "optimized", "param", vec![],
            vec![ExpectedDiag::new("@optimized is not valid on param declarations", "@optimized")]),
        FixtureRow::new("optimized/trait-invalid", "optimized", "trait", vec![],
            vec![ExpectedDiag::new("@optimized is not valid on trait declarations", "@optimized")]),

        // ── @solver_hint: valid contexts ─────────────────────────────────
        FixtureRow::ok("solver_hint/structure", "solver_hint", "structure", vec![]),
        FixtureRow::ok("solver_hint/occurrence", "solver_hint", "occurrence", vec![]),
        FixtureRow::ok("solver_hint/param", "solver_hint", "param", vec![]),
        FixtureRow::ok("solver_hint/let", "solver_hint", "let", vec![]),

        // @solver_hint: invalid context
        FixtureRow::new("solver_hint/function-invalid", "solver_hint", "function", vec![],
            vec![ExpectedDiag::new("@solver_hint is not valid on function declarations", "@solver_hint")]),
        FixtureRow::new("solver_hint/trait-invalid", "solver_hint", "trait", vec![],
            vec![ExpectedDiag::new("@solver_hint is not valid on trait declarations", "@solver_hint")]),

        // ── @shell: valid contexts, various arg shapes ────────────────────
        FixtureRow::ok("shell/structure-bare", "shell", "structure", vec![]),
        FixtureRow::ok("shell/occurrence-bare", "shell", "occurrence", vec![]),
        FixtureRow::ok("shell/structure-real", "shell", "structure", vec![Real(0.5)]),
        FixtureRow::ok("shell/structure-int", "shell", "structure", vec![Int(2)]),
        FixtureRow::ok("shell/occurrence-real", "shell", "occurrence", vec![Real(1.0)]),

        // @shell: non-numeric arg
        FixtureRow::new("shell/structure-string-arg", "shell", "structure",
            vec![Str("thick".to_string())],
            vec![ExpectedDiag::new("must be a numeric literal", "non-numeric thickness")]),

        // @shell: extra args
        FixtureRow::new("shell/structure-extra-args", "shell", "structure",
            vec![Real(0.5), Real(0.6)],
            vec![ExpectedDiag::new("at most one argument", "too many arguments")]),

        // @shell: invalid context with arg (short-circuit: only context-mismatch)
        FixtureRow::new("shell/function-with-arg", "shell", "function",
            vec![Str("x".to_string())],
            vec![ExpectedDiag::new("@shell is not valid on function declarations", "@shell")]),

        // ── @solid: valid contexts ────────────────────────────────────────
        FixtureRow::ok("solid/structure-bare", "solid", "structure", vec![]),
        FixtureRow::ok("solid/occurrence-bare", "solid", "occurrence", vec![]),

        // @solid: any arg on valid context → takes-no-arguments
        FixtureRow::new("solid/structure-real", "solid", "structure", vec![Real(0.5)],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),
        FixtureRow::new("solid/structure-int", "solid", "structure", vec![Int(2)],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),
        FixtureRow::new("solid/structure-string", "solid", "structure",
            vec![Str("foo".to_string())],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),
        FixtureRow::new("solid/structure-bool", "solid", "structure", vec![Bool(true)],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),
        FixtureRow::new("solid/structure-ident", "solid", "structure",
            vec![Ident("x".to_string())],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),
        FixtureRow::new("solid/structure-two-reals", "solid", "structure",
            vec![Real(0.5), Real(0.6)],
            vec![ExpectedDiag::new("takes no arguments", "@solid takes no arguments")]),

        // @solid: invalid context with arg → context-mismatch only (short-circuit)
        FixtureRow::new("solid/function-with-arg", "solid", "function", vec![Real(0.5)],
            vec![ExpectedDiag::new("@solid is not valid on function declarations", "@solid")]),

        // ── Unknown annotation ────────────────────────────────────────────
        FixtureRow::new("unknown/future_annotation", "future_annotation", "structure", vec![],
            vec![ExpectedDiag::new("unknown annotation @future_annotation", "unknown annotation")]),
        FixtureRow::new("unknown/xyz", "xyz_not_real", "occurrence", vec![],
            vec![ExpectedDiag::new("unknown annotation @xyz_not_real", "unknown annotation")]),
    ];

    // ── Single-annotation rows ────────────────────────────────────────────────
    for row in &rows {
        let ann = reify_types::Annotation {
            name: row.name.to_string(),
            args: row
                .args
                .iter()
                .cloned()
                .map(reify_types::AnnotationArg::positional)
                .collect(),
            span: reify_types::SourceSpan::empty(0),
        };
        let diags = reify_compiler::__validate_annotations_for_parity_test(
            std::slice::from_ref(&ann),
            row.context,
        );
        assert_eq!(
            diags.len(),
            row.expected.len(),
            "[{}] expected {} diagnostics, got {}: {:?}",
            row.label,
            row.expected.len(),
            diags.len(),
            diags,
        );
        for (i, (diag, exp)) in diags.iter().zip(row.expected.iter()).enumerate() {
            assert!(
                diag.message.contains(exp.message_substr),
                "[{}] diag[{}] message {:?} does not contain {:?}",
                row.label,
                i,
                diag.message,
                exp.message_substr
            );
            assert!(
                !diag.labels.is_empty(),
                "[{}] diag[{}] has no labels",
                row.label,
                i
            );
            assert!(
                diag.labels[0].message.contains(exp.label_substr),
                "[{}] diag[{}] label {:?} does not contain {:?}",
                row.label,
                i,
                diag.labels[0].message,
                exp.label_substr
            );
        }
    }

    // ── Duplicate-@optimized slice cases ─────────────────────────────────────

    // Two valid @optimized on constraint_def → 1 duplicate warning
    {
        let a1 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("a".to_string()))],
            span: reify_types::SourceSpan::empty(0),
        };
        let a2 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("b".to_string()))],
            span: reify_types::SourceSpan::empty(10),
        };
        let anns = vec![a1, a2.clone()];
        let diags = reify_compiler::__validate_annotations_for_parity_test(&anns, "constraint_def");
        assert_eq!(
            diags.len(),
            1,
            "duplicate-@optimized/constraint_def: expected 1 diag, got: {:?}",
            diags
        );
        assert!(
            diags[0].message.contains("multiple @optimized annotations"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "duplicate @optimized");
        assert_eq!(
            diags[0].labels[0].span,
            a2.span,
            "duplicate warning must be on second annotation's span"
        );
    }

    // Two valid @optimized on function → 1 duplicate warning
    {
        let a1 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("a".to_string()))],
            span: reify_types::SourceSpan::empty(0),
        };
        let a2 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("b".to_string()))],
            span: reify_types::SourceSpan::empty(10),
        };
        let anns = vec![a1, a2.clone()];
        let diags = reify_compiler::__validate_annotations_for_parity_test(&anns, "function");
        assert_eq!(
            diags.len(),
            1,
            "duplicate-@optimized/function: expected 1 diag, got: {:?}",
            diags
        );
        assert!(diags[0].message.contains("multiple @optimized annotations"));
        assert_eq!(diags[0].labels[0].span, a2.span);
    }

    // Two valid @optimized on structure → 0 duplicate warnings
    {
        let a1 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("a".to_string()))],
            span: reify_types::SourceSpan::empty(0),
        };
        let a2 = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("b".to_string()))],
            span: reify_types::SourceSpan::empty(10),
        };
        let anns = vec![a1, a2];
        let diags = reify_compiler::__validate_annotations_for_parity_test(&anns, "structure");
        assert!(
            diags.is_empty(),
            "duplicate-@optimized/structure: expected 0 diags, got: {:?}",
            diags
        );
    }

    // Malformed then valid @optimized on constraint_def → 1 missing-target, 0 dup
    {
        let a_malformed = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![],
            span: reify_types::SourceSpan::empty(0),
        };
        let a_valid = reify_types::Annotation {
            name: "optimized".to_string(),
            args: vec![reify_types::AnnotationArg::positional(Str("b".to_string()))],
            span: reify_types::SourceSpan::empty(10),
        };
        let anns = vec![a_malformed, a_valid];
        let diags = reify_compiler::__validate_annotations_for_parity_test(&anns, "constraint_def");
        assert_eq!(
            diags.len(),
            1,
            "malformed-then-valid/constraint_def: expected 1 diag (missing-target only), got: {:?}",
            diags
        );
        assert!(
            diags[0].message.contains("requires a string literal target"),
            "unexpected message: {}",
            diags[0].message
        );
        // No dup warning
        assert!(!diags[0].message.contains("multiple @optimized"));
    }
}
